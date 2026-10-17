//! FLIP placement for the lazy multi-box slivers: a child whose laid-out
//! position moves is painted from where it was and slides to where it is.
//!
//! The owner of an animated list keeps one [`PlacementSlot`] per item and one
//! clock for the whole list, and hands both to the sliver as a
//! [`Placement`]. The sliver compares each child's position with the one it
//! recorded in the previous layout pass and, when an item moved, restarts the
//! clock with every visible child's remaining displacement rebased onto the
//! new run. Only a child laid out in both passes can move: one that enters the
//! band appears at its place, so scrolling an item in never animates it.
//!
//! Comparison is armed only by a change of the owner's roster
//! ([`Placement::generation`]): scrolling, an estimate correction or an item
//! resizing moves children without a reorder, and those land at once. The
//! arm stays set across the passes of the frame that applies the change,
//! until a pass requests no further children, so an item built mid-frame
//! still shifts its followers. Displacements add up across those passes.
//!
//! Positions are content coordinates — the main-axis layout offset and the
//! cross-axis offset — so the scroll offset cancels out. The sliver turns a
//! displaced content position into a paint offset with its own placement
//! function, which keeps the growth and cross-axis directions consistent.
//! Paint and hit testing both read the displaced offset.

use std::cell::Cell;
use std::fmt;
use std::rc::Rc;

use crate::subscription::RenderSubscription;
use flui_animation::curve::Curve;
use flui_animation::{
    Animation, AnimationController, MotionSpec, SpringSimulation, Tolerance, curves,
};
use flui_foundation::panic::PanicRecovery;
use flui_rendering::pipeline::LocalRenderInvalidationHandle;

/// A position along both sliver axes: main-axis layout offset, cross-axis
/// offset.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Axes {
    main: f64,
    cross: f64,
}

impl Axes {
    const ZERO: Self = Self {
        main: 0.0,
        cross: 0.0,
    };

    fn plus(self, other: Self) -> Self {
        Self {
            main: self.main + other.main,
            cross: self.cross + other.cross,
        }
    }

    fn minus(self, other: Self) -> Self {
        Self {
            main: self.main - other.main,
            cross: self.cross - other.cross,
        }
    }

    fn scaled(self, factor: f64) -> Self {
        Self {
            main: self.main * factor,
            cross: self.cross * factor,
        }
    }

    /// Finite on both axes; a non-finite displacement is dropped rather than
    /// published as geometry.
    fn or_zero(self) -> Self {
        if self.main.is_finite() && self.cross.is_finite() {
            self
        } else {
            Self::ZERO
        }
    }
}

/// One item's placement record, shared by an animated list's owner and the
/// sliver that lays the item out.
///
/// The owner creates one per item and keeps it for the item's whole life,
/// whatever index the item moves to; the sliver reads and writes it during
/// layout. It holds no animation of its own: the displacement is
/// `from × remaining`, where `remaining` is the shared clock's share of the
/// run still ahead.
#[derive(Debug, Default)]
pub struct PlacementSlot {
    /// Where the item was laid out, and the sliver's pass that did it.
    last: Cell<Option<(Axes, u64)>>,
    /// The displacement from the laid-out position when the clock last
    /// restarted.
    from: Cell<Axes>,
}

impl PlacementSlot {
    /// A slot for an item that has not been laid out yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// What a lazy sliver needs to animate its children's placement: one
/// [`PlacementSlot`] per logical index, the clock that runs every slide, how
/// the clock moves, and the generation of the owner's roster.
///
/// The owner bumps `generation` whenever items are inserted, removed or
/// reordered; a placement with an unchanged generation only records
/// positions.
#[derive(Clone)]
pub struct Placement {
    clock: AnimationController,
    motion: MotionSpec,
    slots: Rc<[Rc<PlacementSlot>]>,
    generation: u64,
}

impl fmt::Debug for Placement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Placement")
            .field("motion", &self.motion)
            .field("slots", &self.slots.len())
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl Placement {
    /// A placement over `slots`, indexed by the sliver's logical child index.
    ///
    /// `clock` belongs to the owner, which binds it to a frame registry and
    /// disposes it; the sliver only restarts it and listens to it. It should
    /// rest at its upper bound until the first move.
    #[must_use]
    pub fn new(
        clock: AnimationController,
        motion: MotionSpec,
        slots: Rc<[Rc<PlacementSlot>]>,
        generation: u64,
    ) -> Self {
        Self {
            clock,
            motion,
            slots,
            generation,
        }
    }

    /// Whether `other` describes the same roster with the same motion, so
    /// the sliver has nothing to redo.
    fn same_as(&self, other: &Self) -> bool {
        self.generation == other.generation
            && Rc::ptr_eq(&self.slots, &other.slots)
            && self.motion == other.motion
    }

    /// The share of the current slide still ahead: `1` at its start, `0` at
    /// rest.
    fn remaining(&self) -> f64 {
        let t = self.clock.value();
        let eased = match &self.motion {
            MotionSpec::Curve { curve, .. } => curve.transform(t),
            MotionSpec::Spring(_) => t,
            _ => curves::EASE_IN_OUT.transform(t),
        };
        let remaining = 1.0 - eased;
        if remaining.is_finite() {
            remaining
        } else {
            0.0
        }
    }

    /// Start a fresh slide from the beginning. A refused run leaves the clock
    /// where it is, so displacements land at once instead of sticking.
    fn restart(&self) {
        let run = match &self.motion {
            MotionSpec::Spring(spring) => {
                SpringSimulation::new(*spring, 0.0, 1.0, 0.0, Tolerance::DEFAULT)
                    .map_err(|error| tracing::warn!(%error, "placement spring refused"))
                    .ok()
                    .map(|simulation| {
                        self.clock.set_value(0.0);
                        self.clock.animate_with(simulation)
                    })
            }
            MotionSpec::Curve { duration, .. } => {
                self.clock.set_duration(*duration);
                Some(self.clock.forward_from(Some(0.0)))
            }
            _ => {
                self.clock.set_duration(MotionSpec::DEFAULT_DURATION);
                Some(self.clock.forward_from(Some(0.0)))
            }
        };
        if let Some(Err(error)) = run {
            tracing::warn!(%error, "placement clock refused a slide; children land at once");
        }
    }
}

/// One child the sliver laid out this pass, at its undisplaced content
/// position.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Placed {
    /// The child's dense slot.
    pub(crate) slot: usize,
    /// The child's logical index, which picks its [`PlacementSlot`].
    pub(crate) logical: usize,
    /// Main-axis layout offset.
    pub(crate) main: f64,
    /// Cross-axis offset.
    pub(crate) cross: f64,
    /// Main-axis extent, for the sliver's placement function.
    pub(crate) extent: f64,
}

/// A sliver's side of a [`Placement`]: the current placement, its pass
/// counter and its subscription to the clock.
#[derive(Default)]
pub(crate) struct PlacementTrack {
    placement: Option<Placement>,
    /// The last generation whose comparison finished: a later generation arms
    /// the next passes.
    applied: Option<u64>,
    /// Layout passes run so far.
    pass: u64,
    handle: Option<LocalRenderInvalidationHandle>,
    listener: Option<RenderSubscription>,
}

impl fmt::Debug for PlacementTrack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlacementTrack")
            .field("placement", &self.placement)
            .field("applied", &self.applied)
            .field("pass", &self.pass)
            .finish_non_exhaustive()
    }
}

impl Clone for PlacementTrack {
    /// A clone shares the placement but is detached: it has no subscription.
    fn clone(&self) -> Self {
        Self {
            placement: self.placement.clone(),
            applied: self.applied,
            pass: self.pass,
            handle: None,
            listener: None,
        }
    }
}

impl PlacementTrack {
    /// Whether a placement is installed.
    pub(crate) fn is_active(&self) -> bool {
        self.placement.is_some()
    }

    /// Install `placement`. A different roster or motion needs layout; the
    /// clock subscription follows the installed placement while attached.
    pub(crate) fn set(
        &mut self,
        placement: Option<&Placement>,
    ) -> flui_rendering::RenderUpdateImpact {
        let unchanged = match (&self.placement, placement) {
            (None, None) => true,
            (Some(current), Some(next)) => current.same_as(next),
            _ => false,
        };
        if unchanged {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.unsubscribe();
        self.placement = placement.cloned();
        self.subscribe();
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    pub(crate) fn attach(&mut self, handle: LocalRenderInvalidationHandle) {
        self.unsubscribe();
        self.handle = Some(handle);
        self.subscribe();
    }

    pub(crate) fn detach(&mut self) {
        self.unsubscribe();
        self.handle = None;
    }

    /// Relayout on every tick of the installed clock while attached.
    fn subscribe(&mut self) {
        if let (Some(placement), Some(handle)) = (&self.placement, &self.handle) {
            let mut recovery = PanicRecovery::new();
            self.listener = RenderSubscription::subscribe(
                || Some(Rc::new(placement.clock.clone())),
                Some(handle),
                flui_rendering::RenderUpdateImpact::LAYOUT,
                &mut recovery,
            );
            recovery.finish();
        }
    }

    fn unsubscribe(&mut self) {
        if let Some(subscription) = self.listener.take() {
            let mut recovery = PanicRecovery::new();
            subscription.close(&mut recovery);
            recovery.finish();
        }
    }

    /// Record where this pass laid `children` out, start a slide when an
    /// armed pass moved one of them, and hand each displaced child to
    /// `position` with its displaced content position `(main, cross)`.
    ///
    /// `settled` says the pass requested no children and corrected no scroll
    /// offset, so the roster change it armed for is fully applied.
    pub(crate) fn place(
        &mut self,
        children: &[Placed],
        settled: bool,
        mut position: impl FnMut(&Placed, f64, f64),
    ) {
        // Saturates rather than wraps: past the last pass no child has a
        // previous one, so children land instead of sliding from a stale spot.
        self.pass = self.pass.saturating_add(1);
        let pass = self.pass;
        let Some(placement) = &self.placement else {
            return;
        };
        let armed = self.applied != Some(placement.generation);
        let remaining = placement.remaining();

        // A child's slot, where the previous pass laid it out (when it did)
        // and where this pass lays it out.
        let track = |child: &Placed| {
            let slot = placement.slots.get(child.logical)?;
            let previous = slot
                .last
                .get()
                .and_then(|(at, laid)| (laid.checked_add(1) == Some(pass)).then_some(at));
            let here = Axes {
                main: child.main,
                cross: child.cross,
            };
            Some((slot, previous, here))
        };
        let moved = armed
            && children
                .iter()
                .filter_map(&track)
                .any(|(_, previous, here)| {
                    previous.is_some_and(|at| at.minus(here).or_zero() != Axes::ZERO)
                });

        // A moved pass rebases every continuing child's displacement onto a
        // fresh run; a child new to the band appears where it is laid out.
        for (slot, previous, here) in children.iter().filter_map(&track) {
            match previous {
                Some(at) if moved => {
                    let now = slot.from.get().scaled(remaining).or_zero();
                    slot.from.set(now.plus(at.minus(here).or_zero()));
                }
                Some(_) => {}
                None => slot.from.set(Axes::ZERO),
            }
            slot.last.set(Some((here, pass)));
        }
        if moved {
            placement.restart();
        }
        if settled {
            self.applied = Some(placement.generation);
        }

        let remaining = placement.remaining();
        if remaining <= 0.0 {
            return;
        }
        for child in children {
            let Some(slot) = placement.slots.get(child.logical) else {
                continue;
            };
            let displacement = slot.from.get().scaled(remaining).or_zero();
            if displacement != Axes::ZERO {
                position(
                    child,
                    child.main + displacement.main,
                    child.cross + displacement.cross,
                );
            }
        }
    }
}
