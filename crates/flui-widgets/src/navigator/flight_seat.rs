//! The flight seat: where a widget that can take off from sits while it is away.
//!
//! A [`Hero`](super::Hero) and an `OpenContainer` both hand their content to a
//! copy that flies in an overlay, and both must leave something behind that
//! keeps the layout still. The seat is that shared half. It owns three things:
//!
//! * **A render anchor** the seat's widget publishes, so a flight can name the
//!   render node it takes off from. `BuildContext` walks ancestors only, so the
//!   node below a view has to be published from the render side.
//! * **A measurement**: the seat's committed size, and its rect in an
//!   ancestor's coordinates or in the render root's. A flight asks again every
//!   time it needs the source (the source may have scrolled), so the seat never
//!   caches a rect.
//! * **A frozen placeholder**: the size the seat held when the flight began,
//!   whether its child stays mounted offstage, and the [`SeatHold`] allowed to
//!   release it.
//!
//! What a flight *is* (matching tags and diverting for `Hero`, a route morph for
//! `OpenContainer`) stays with the flight; nothing here knows about routes.
//!
//! # State survives the flight
//!
//! A seat that keeps its child builds one constant chain
//! `SizedBox → Offstage → TickerMode → child` in and out of flight, so the
//! child's element is never reparented and its state survives with no
//! `GlobalKey`. The destination of a flight sets `include_child` to `false`
//! instead: its seat shows a bare `SizedBox` of the frozen size, because the
//! shuttle carries its content.
//!
//! # Not public API
//!
//! The module is exported to workspace crates through
//! [`crate::__private`], which carries no semver guarantee.

use std::cell::RefCell;
use std::rc::Rc;

use flui_foundation::RenderId;
use flui_foundation::geometry::{Rect, Size};
use flui_objects::SubtreeAnchor;
use flui_rendering::pipeline::{PipelineOwner, WeakPipelineCell};
use flui_view::prelude::*;

use super::lifecycle::Terminal;
use crate::__private::AnchoredBox;
use crate::{Offstage, SizedBox, Stack, TickerMode};

/// Builds the widget left in a seat while it is in flight. It takes only the
/// frozen [`Size`], never the child, so it cannot drop the child: the real
/// child stays offstage and its state survives.
pub type PlaceholderBuilder = Rc<dyn Fn(Size) -> BoxedView>;

/// The allocation identity of one logical flight, independent of which widget
/// instance flies. Only the flight that froze a seat may release it.
#[derive(Clone, Default)]
pub struct SeatClaim(Rc<()>);

impl SeatClaim {
    /// Whether both claims name the same flight.
    #[must_use]
    pub fn is_same(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// Who may release a frozen seat.
#[derive(Clone)]
pub enum SeatHold {
    /// The flight with this claim, and only it.
    Claimed(SeatClaim),
    /// Nobody in flight: a landed source keeps its geometry until it unmounts or
    /// flies again.
    Settled,
}

struct Frozen {
    size: Size,
    include_child: bool,
    hold: SeatHold,
}

/// The seat of one mounted flier. See the [module docs](self).
#[derive(Default)]
pub struct FlightSeat {
    /// The seat's own render node, published on `attach` and cleared on
    /// `detach`.
    anchor: SubtreeAnchor,
    /// Weak render access, so a measurement cannot keep the presentation alive.
    owner: RefCell<Option<WeakPipelineCell>>,
    frozen: RefCell<Option<Frozen>>,
}

impl Drop for FlightSeat {
    fn drop(&mut self) {
        drop(Terminal::new(self.owner.get_mut().take()));
    }
}

impl FlightSeat {
    /// A seat that is not mounted and holds nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The anchor the seat's widget publishes into: wrap the seat's content in
    /// an [`AnchoredBox`] over it, or call [`build`](Self::build).
    #[must_use]
    pub fn anchor(&self) -> SubtreeAnchor {
        self.anchor.clone()
    }

    /// Point measurements at `owner`, the render tree the seat is mounted in.
    pub fn bind(&self, owner: Option<WeakPipelineCell>) {
        let previous = self.owner.replace(owner);
        drop(previous);
    }

    /// Withdraw render access and the frozen placeholder: the seat is
    /// unmounting.
    pub fn withdraw(&self) {
        let owner = self.owner.take();
        let frozen = self.frozen.take();
        drop((owner, frozen));
    }

    /// The seat's render node, or `None` before it attaches and after it
    /// detaches.
    ///
    /// `Some` says nothing about layout: `attach` runs during build. Ask
    /// [`size`](Self::size) for geometry.
    #[must_use]
    pub fn render_id(&self) -> Option<RenderId> {
        self.anchor.get()
    }

    /// Run `read` against the render tree the seat is bound to, or `None` when
    /// it is unbound, gone, or busy with a layout pass.
    fn read<R>(&self, read: impl FnOnce(&PipelineOwner, RenderId) -> Option<R>) -> Option<R> {
        let render_id = self.render_id()?;
        let owner = self.owner.borrow().as_ref()?.upgrade()?;
        owner.try_with_layout(|owner| read(owner, render_id))?
    }

    /// The seat's committed size, or `None` when it is not laid out.
    #[must_use]
    pub fn size(&self) -> Option<Size> {
        self.read(PipelineOwner::box_size)
    }

    /// The seat's bounding box in `ancestor`'s coordinate space, or `None` when
    /// it is unmounted, not laid out, or not a descendant of `ancestor`.
    #[must_use]
    pub fn bounding_box_in(&self, ancestor: RenderId) -> Option<Rect> {
        self.read(|owner, id| {
            let size = owner.box_size(id)?;
            let transform = owner.transform_to(id, ancestor)?;
            Some(transform.transform_rect(&Rect::from_ltwh(0.0, 0.0, size.width, size.height)))
        })
    }

    /// Freeze the seat at its committed size under `hold`.
    ///
    /// Returns the captured size, or `None` when the seat has no committed
    /// layout to freeze: such a flier simply does not fly. The caller rebuilds
    /// the seat's widget afterwards.
    ///
    /// `include_child` is `true` for the source of a flight, whose subtree
    /// stays offstage so its state survives, and `false` for a destination.
    pub fn freeze(&self, hold: SeatHold, include_child: bool) -> Option<Size> {
        let size = self.size()?;
        let previous = self.frozen.replace(Some(Frozen {
            size,
            include_child,
            hold,
        }));
        drop(previous);
        Some(size)
    }

    /// The frozen size, withdrawn when the seat unmounts.
    #[must_use]
    pub fn frozen_size(&self) -> Option<Size> {
        self.frozen.borrow().as_ref().map(|frozen| frozen.size)
    }

    /// Whether a frozen seat keeps its child offstage inside the placeholder.
    /// `true` when nothing is frozen.
    #[must_use]
    pub fn includes_child(&self) -> bool {
        self.frozen
            .borrow()
            .as_ref()
            .is_none_or(|frozen| frozen.include_child)
    }

    /// Release the placeholder regardless of who froze it. `keep` leaves it
    /// frozen. Returns whether the seat changed and needs a rebuild.
    pub fn release(&self, keep: bool) -> bool {
        if keep {
            return false;
        }
        let removed = self.frozen.take();
        let changed = removed.is_some();
        drop(removed);
        changed
    }

    /// Release the placeholder only if `claim` froze it, so an old flight
    /// cannot release a replacement's placeholder. `keep` hands the geometry
    /// over to [`SeatHold::Settled`] instead. Returns whether the seat changed
    /// and needs a rebuild.
    pub fn release_for(&self, claim: &SeatClaim, keep: bool) -> bool {
        let removed = {
            let mut frozen = self.frozen.borrow_mut();
            let Some(current) = frozen.as_mut() else {
                return false;
            };
            let SeatHold::Claimed(held) = &current.hold else {
                return false;
            };
            if !held.is_same(claim) {
                return false;
            }
            if keep {
                current.hold = SeatHold::Settled;
                return false;
            }
            frozen.take()
        };
        drop(removed);
        true
    }

    /// The seat's widget: an [`AnchoredBox`] over `child` in the constant chain
    /// the module docs describe, or over `placeholder`'s view while frozen.
    ///
    /// The anchored box is always present, in flight or not: it is what
    /// publishes the `RenderId` a flight measures, and a node that came and
    /// went would make [`render_id`](Self::render_id) flicker.
    ///
    /// | Case | Structure |
    /// |---|---|
    /// | custom `placeholder` | child kept offstage at a constant path, placeholder shown as a sibling |
    /// | frozen, `include_child == false` | a bare `SizedBox` of the frozen size |
    /// | otherwise | `SizedBox(size?) → Offstage(frozen) → TickerMode(!frozen) → child` |
    #[must_use]
    pub fn build(&self, child: &BoxedView, placeholder: Option<&PlaceholderBuilder>) -> AnchoredBox {
        let anchor = self.anchor();
        let frozen = self.frozen_size();
        let show_placeholder = frozen.is_some();

        // A custom placeholder uses one constant structure in and out of flight,
        // `SizedBox → Stack[Offstage → child, …]`, so the child's element (slot
        // 0) is never reparented. The placeholder is appended at slot 1 only
        // while in flight; it never sees the child, so it cannot drop it.
        if let Some(build_placeholder) = placeholder {
            let mut layers: Vec<BoxedView> = vec![
                Offstage::new()
                    .offstage(show_placeholder)
                    .child(TickerMode::new(child.clone()).enabled(!show_placeholder))
                    .into_view()
                    .boxed(),
            ];
            if let Some(size) = frozen {
                layers.push(build_placeholder(size));
            }
            let sized = match frozen {
                Some(size) => SizedBox::new(size.width, size.height),
                None => SizedBox::default(),
            };
            return AnchoredBox::new(anchor, sized.child(Stack::new(layers)));
        }

        // The destination drops its child (the shuttle carries it), so this
        // branch legitimately changes shape and the child's state is not kept.
        if let Some(size) = frozen
            && !self.includes_child()
        {
            return AnchoredBox::new(anchor, SizedBox::new(size.width, size.height));
        }

        // The fixed chain. Not in flight (`SizedBox::default()`, `Offstage(false)`)
        // and in flight with the child kept (`SizedBox(size)`, `Offstage(true)`)
        // put the child at the same depth under the same view types, so
        // reconciliation preserves its element. The seat left behind offstage
        // keeps its subtree but its animations must not run while a copy flies.
        let sized = match frozen {
            Some(size) => SizedBox::new(size.width, size.height),
            None => SizedBox::default(),
        };
        AnchoredBox::new(
            anchor,
            sized.child(
                Offstage::new()
                    .offstage(show_placeholder)
                    .child(TickerMode::new(child.clone()).enabled(!show_placeholder)),
            ),
        )
    }
}

impl std::fmt::Debug for FlightSeat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlightSeat")
            .field("frozen", &self.frozen_size())
            .finish_non_exhaustive()
    }
}
