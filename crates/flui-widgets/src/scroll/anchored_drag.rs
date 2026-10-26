//! [`AnchoredDrag`] — a one-dimensional drag that comes to rest on anchors
//! (ADR-0209 §9).
//!
//! An anchored drag is a bounded scroll leaf of the nested scroll channel,
//! not a second scroll path: it composes a [`Scrollable`] whose offset is the
//! drag's offset, whose extents are the lowest and highest anchor, and whose
//! snap policy is an [`AnchorSnap`] over the anchors' offsets. A drag on it
//! is an axial arena member that takes its axis first and passes the
//! remainder up its chain; a release settles through the policy, a fling
//! taking the next anchor in its direction. For the scrollables inside it,
//! it is a participant of their chains instead: what they leave on its axis
//! moves it, and their release settles it.
//!
//! # Offsets
//!
//! An anchor is an offset of the child along the axis: down on a vertical
//! drag, toward the reading end on a horizontal one (right under
//! left-to-right text, left under right-to-left), so the keys of a row's
//! leading and trailing actions keep their meaning in either direction.
//! The scrollable's pixels are that offset; its orientation is the opposite
//! of the scroll convention, because the content follows the finger.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

use flui_animation::{
    Animation, AnimationStatus, ArcCurve, ConstantAnimation, ProxyAnimation, StatusObserver,
    StatusSubscription,
};
use flui_foundation::geometry::{Axis, Offset, Vec2};
use flui_foundation::panic::PanicRecovery;
use flui_foundation::{ChangeNotifier, Listenable, ListenerId, ListenerObserver};
use flui_interaction::{HapticsHandle, InputSequenceId};
use flui_painting::typography::TextDirection;
use flui_platform_api::HapticFeedback;
use flui_platform_api::haptics::{HapticRequest, HapticTiming};
use flui_rendering::constraints::AxisDirection;
use flui_rendering::pipeline::WeakPipelineCell;
use flui_view::element::ElementKind;
use flui_view::{
    AnimatedView, BoxedView, BuildContext, EventCx, IntoView, LifecycleContext, StatefulView, View,
    ViewExt, ViewState,
};

use super::nested::presentation_device_pixel_ratio;
use super::nested::{
    AnchorSnap, ChainNode, FlingHandoff, FlingStep, NestedScrollConnection, NestedScrollScope,
    Participant, PlaneSource, ScrollCause, ScrollDevice, ScrollEndReason, ScrollStep,
};
use super::scroll_owner::{MotionHook, MoveOutcome, ScrollOwner, SharedSnapPolicy};
use super::{ScrollController, Scrollable};
use crate::anchored_box::AnchoredBox;
use crate::interaction::raw_gesture_detector::ContactAdmission;
use crate::transitions::transform_view::AnimatedTransformView;
use crate::{
    Action, ActionOutcome, Actions, Directionality, GestureAlternatives, Intent, Listener,
    Semantics, SemanticsRole, Shortcuts, SingleActivator,
};
use flui_objects::TransformMotion;
use flui_platform_api::keyboard::NamedKey;
use flui_rendering::hit_testing::HitTestBehavior;

/// The largest distance from zero an anchor may lie at, in logical pixels:
/// far past any display, and small enough that spans, velocities and their
/// products stay finite.
const MAX_ANCHOR_OFFSET: f64 = 1.0e9;

/// How long [`AnchoredDragController::set_anchors`] takes to move a resting
/// drag onto its new anchor.
const RESETTLE: Duration = Duration::from_millis(250);

/// Why a set of anchors, or a key into one, was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AnchorsError {
    /// No anchor was given: there is nowhere to rest.
    #[error("an anchored drag needs at least one anchor")]
    Empty,
    /// The anchor at `index`, in the order given, is NaN or infinite.
    #[error("anchor {index} is not finite")]
    NonFinite {
        /// The anchor's position in the input.
        index: usize,
    },
    /// The anchor at `index` lies farther from zero than
    /// [`DraggableAnchors::MAX_OFFSET`].
    #[error("anchor {index} lies outside the admitted offsets")]
    OutOfRange {
        /// The anchor's position in the input.
        index: usize,
    },
    /// The anchor at `index` repeats the key of an earlier one.
    #[error("anchor {index} repeats the key of an earlier anchor")]
    Duplicate {
        /// The anchor's position in the input.
        index: usize,
    },
    /// The anchor at `index` lies on the offset of an earlier one: the drag
    /// could not tell which of the two it rests on.
    #[error("anchor {index} lies on the offset of an earlier anchor")]
    Coincident {
        /// The anchor's position in the input.
        index: usize,
    },
    /// The key names none of the anchors.
    #[error("the key is not one of the anchors")]
    Missing,
}

/// The anchors an [`AnchoredDrag`] rests on: one offset per key.
///
/// Built and validated once by [`DraggableAnchors::builder`]; cloning shares
/// the set. The offsets go to an [`AnchorSnap`], which holds offsets, not
/// keys (ADR-0209 §9); this set keeps the keys.
///
/// ```
/// use flui_widgets::{AnchorsError, DraggableAnchors};
///
/// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// enum Stop {
///     Closed,
///     Open,
/// }
///
/// let anchors = DraggableAnchors::builder()
///     .at(Stop::Closed, 0.0)
///     .at(Stop::Open, 80.0)
///     .build()
///     .expect("two finite anchors");
/// assert_eq!(anchors.offset(Stop::Open), Some(80.0));
/// assert_eq!(anchors.closest(50.0), Stop::Open);
///
/// let twice = DraggableAnchors::builder().at(Stop::Open, 0.0).at(Stop::Open, 80.0).build();
/// assert_eq!(twice.unwrap_err(), AnchorsError::Duplicate { index: 1 });
/// ```
#[derive(Clone)]
pub struct DraggableAnchors<K> {
    /// Ascending by offset; keys and offsets distinct, never empty.
    entries: Rc<[(K, f64)]>,
    /// The offsets as the drag's snap policy, one allocation per set: the
    /// scrollable's owner compares it by identity.
    policy: SharedSnapPolicy,
}

impl<K: fmt::Debug> fmt::Debug for DraggableAnchors<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.entries.iter().map(|(key, offset)| (key, offset)))
            .finish()
    }
}

/// Collects the anchors of a [`DraggableAnchors`]; see
/// [`DraggableAnchors::builder`].
#[derive(Debug, Clone)]
#[must_use]
pub struct DraggableAnchorsBuilder<K> {
    entries: Vec<(K, f64)>,
}

impl<K: Copy + Eq + 'static> DraggableAnchorsBuilder<K> {
    /// An anchor for `key` at `offset` logical pixels along the axis.
    pub fn at(mut self, key: K, offset: f64) -> Self {
        self.entries.push((key, offset));
        self
    }

    /// The anchors, in any order.
    ///
    /// # Errors
    ///
    /// [`AnchorsError::Empty`] for no anchors; for the first offending
    /// anchor in the order given, [`AnchorsError::NonFinite`],
    /// [`AnchorsError::OutOfRange`], [`AnchorsError::Duplicate`] for a
    /// repeated key and [`AnchorsError::Coincident`] for a repeated offset.
    pub fn build(self) -> Result<DraggableAnchors<K>, AnchorsError> {
        let mut entries = self.entries;
        for (index, &(key, offset)) in entries.iter().enumerate() {
            if !offset.is_finite() {
                return Err(AnchorsError::NonFinite { index });
            }
            if offset.abs() > MAX_ANCHOR_OFFSET {
                return Err(AnchorsError::OutOfRange { index });
            }
            let earlier = &entries[..index];
            if earlier.iter().any(|&(other, _)| other == key) {
                return Err(AnchorsError::Duplicate { index });
            }
            if earlier.iter().any(|&(_, other)| other == offset) {
                return Err(AnchorsError::Coincident { index });
            }
        }
        if entries.is_empty() {
            return Err(AnchorsError::Empty);
        }
        entries.sort_by(|(_, a), (_, b)| a.total_cmp(b));
        let snap = AnchorSnap::new(entries.iter().map(|&(_, offset)| offset))
            .expect("BUG: the anchors were checked non-empty and finite above");
        Ok(DraggableAnchors {
            entries: entries.into(),
            policy: Arc::new(snap),
        })
    }
}

impl<K: Copy + Eq + 'static> DraggableAnchors<K> {
    /// The largest distance from zero an anchor may lie at, in logical
    /// pixels.
    pub const MAX_OFFSET: f64 = MAX_ANCHOR_OFFSET;

    /// Start a set of anchors.
    pub fn builder() -> DraggableAnchorsBuilder<K> {
        DraggableAnchorsBuilder {
            entries: Vec::new(),
        }
    }

    /// The offset of `key`'s anchor.
    #[must_use]
    pub fn offset(&self, key: K) -> Option<f64> {
        self.entries
            .iter()
            .find(|&&(other, _)| other == key)
            .map(|&(_, offset)| offset)
    }

    /// The anchor nearest `offset`; from exactly halfway, the higher one. A
    /// NaN offset gives the lowest anchor.
    #[must_use]
    pub fn closest(&self, offset: f64) -> K {
        let above = self.entries.partition_point(|&(_, anchor)| anchor < offset);
        match (above.checked_sub(1), self.entries.get(above)) {
            (Some(below), Some(&(key, anchor))) => {
                let (lower, lower_offset) = self.entries[below];
                if offset - lower_offset < anchor - offset {
                    lower
                } else {
                    key
                }
            }
            (_, Some(&(key, _))) => key,
            (Some(below), None) => self.entries[below].0,
            (None, None) => self.entries[0].0,
        }
    }

    /// The same anchors, settled by `policy` instead of the nearest-anchor
    /// snap: a release goes where `policy` targets, which should be one of
    /// the anchors.
    pub(crate) fn settled_by(mut self, policy: SharedSnapPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// The lowest and the highest offset.
    fn extent(&self) -> (f64, f64) {
        (self.entries[0].1, self.entries[self.entries.len() - 1].1)
    }

    /// The first anchor above `offset`.
    fn after(&self, offset: f64) -> Option<f64> {
        let index = self
            .entries
            .partition_point(|&(_, anchor)| anchor <= offset);
        self.entries.get(index).map(|&(_, anchor)| anchor)
    }

    /// The last anchor below `offset`.
    fn before(&self, offset: f64) -> Option<f64> {
        let index = self.entries.partition_point(|&(_, anchor)| anchor < offset);
        index.checked_sub(1).map(|index| self.entries[index].1)
    }

    /// The anchor a move `from` one offset `to` another arrives at last: the
    /// farthest one along the way that lies past `from` and not past `to`.
    fn crossed(&self, from: f64, to: f64) -> Option<K> {
        if to > from {
            self.entries
                .iter()
                .rev()
                .find(|&&(_, anchor)| anchor > from && anchor <= to)
                .map(|&(key, _)| key)
        } else if to < from {
            self.entries
                .iter()
                .find(|&&(_, anchor)| anchor < from && anchor >= to)
                .map(|&(key, _)| key)
        } else {
            None
        }
    }
}

/// Where a mounted [`AnchoredDrag`]'s scrollable publishes the owner that
/// serves it, so the controller and the drag's own callbacks reach it.
#[derive(Default)]
pub(crate) struct LeafSlot {
    pub(crate) owner: RefCell<Weak<ScrollOwner>>,
}

impl LeafSlot {
    fn owner(&self) -> Option<Rc<ScrollOwner>> {
        self.owner.borrow().upgrade()
    }
}

/// What an [`AnchoredDrag`] hands the [`Scrollable`] it drives: the hook
/// that hears every move, the slot the owner is published in, which
/// contacts its drag takes, and what hears a cancelled drag after its settle
/// began.
#[derive(Clone)]
pub(crate) struct AnchoredLeaf {
    pub(crate) motion: MotionHook,
    pub(crate) slot: Rc<LeafSlot>,
    pub(crate) admission: Option<ContactAdmission>,
    pub(crate) cancelled: Option<Rc<dyn Fn()>>,
}

struct Inner<K> {
    anchors: RefCell<DraggableAnchors<K>>,
    /// The anchor the offset last arrived at.
    current: Cell<K>,
    /// Whether a drag or a run moved the offset at the last activity change.
    moving: Cell<bool>,
    scroll: ScrollController,
    slot: Rc<LeafSlot>,
    notifier: Rc<ChangeNotifier>,
    activity: Cell<Option<ListenerId>>,
}

impl<K> Drop for Inner<K> {
    fn drop(&mut self) {
        if let Some(id) = self.activity.take() {
            self.scroll.position().remove_activity_listener(id);
        }
    }
}

impl<K: Copy + Eq + 'static> Inner<K> {
    /// Record a move `from` one offset `to` another. True when it arrived at
    /// another anchor than the current one, which becomes current.
    fn arrive(&self, from: f64, to: f64) -> bool {
        let crossed = self.anchors.borrow().crossed(from, to);
        match crossed {
            Some(key) if key != self.current.get() => {
                self.current.set(key);
                true
            }
            _ => false,
        }
    }
}

/// The state of an [`AnchoredDrag`]: its anchors, where it rests, and the
/// commands that move it. Cheap to clone; clones share the state. One drag
/// at a time shows a controller.
///
/// [`current`](Self::current) is the anchor the offset last arrived at: one
/// a drag carried it onto or across, or the one a run settled on. The
/// [`listenable`](Self::listenable) notifies when the anchors, the current
/// anchor or [`is_moving`](Self::is_moving) change.
#[derive(Clone)]
pub struct AnchoredDragController<K> {
    inner: Rc<Inner<K>>,
}

impl<K: fmt::Debug + Copy> fmt::Debug for AnchoredDragController<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnchoredDragController")
            .field("current", &self.inner.current.get())
            .field("offset", &self.inner.scroll.pixels())
            .field("anchors", &*self.inner.anchors.borrow())
            .finish_non_exhaustive()
    }
}

impl<K: Copy + Eq + 'static> AnchoredDragController<K> {
    /// A drag over `anchors`, resting on `initial`.
    ///
    /// # Errors
    ///
    /// [`AnchorsError::Missing`] when `initial` is not one of `anchors`.
    pub fn new(anchors: DraggableAnchors<K>, initial: K) -> Result<Self, AnchorsError> {
        let offset = anchors.offset(initial).ok_or(AnchorsError::Missing)?;
        let (min, max) = anchors.extent();
        let scroll = ScrollController::with_initial_scroll_offset(offset);
        scroll
            .update_dimensions(max - min, min, max)
            .expect("BUG: anchors are sorted, finite and within MAX_ANCHOR_OFFSET");
        let inner = Rc::new_cyclic(|this: &Weak<Inner<K>>| {
            let this = this.clone();
            let activity = scroll.position().add_activity_listener(Rc::new(move || {
                let Some(inner) = this.upgrade() else {
                    return;
                };
                let moving = inner.scroll.position().is_scrolling();
                if inner.moving.replace(moving) != moving {
                    inner.notifier.notify_listeners();
                }
            }));
            Inner {
                anchors: RefCell::new(anchors),
                current: Cell::new(initial),
                moving: Cell::new(false),
                scroll,
                slot: Rc::default(),
                notifier: Rc::new(ChangeNotifier::new()),
                activity: Cell::new(Some(activity)),
            }
        });
        Ok(Self { inner })
    }

    /// The anchors.
    #[must_use]
    pub fn anchors(&self) -> DraggableAnchors<K> {
        self.inner.anchors.borrow().clone()
    }

    /// The anchor the offset last arrived at.
    #[must_use]
    pub fn current(&self) -> K {
        self.inner.current.get()
    }

    /// The offset of the child along the axis, logical pixels.
    #[must_use]
    pub fn offset(&self) -> f64 {
        self.inner.scroll.pixels()
    }

    /// Whether a drag holds the child or a run moves it.
    #[must_use]
    pub fn is_moving(&self) -> bool {
        self.inner.scroll.position().is_scrolling()
    }

    /// Notifies when the anchors, [`current`](Self::current) or
    /// [`is_moving`](Self::is_moving) change.
    #[must_use]
    pub fn listenable(&self) -> Rc<dyn Listenable> {
        Rc::clone(&self.inner.notifier) as Rc<dyn Listenable>
    }

    /// Notifies whenever [`offset`](Self::offset) changes, as it changes:
    /// every step of a drag, every frame of a settle, every programmatic
    /// move. What follows the drag's progress (the actions a row reveals, a
    /// sheet's scrim) listens here.
    #[must_use]
    pub fn offset_listenable(&self) -> Rc<dyn Listenable> {
        self.inner.scroll.listenable()
    }

    /// Replace the anchors. The current anchor stays current when it is
    /// still one of them, else the anchor nearest the offset becomes
    /// current. A drag in progress goes on within the new extents and its
    /// release settles on the new anchors; otherwise the child moves onto
    /// the current anchor's offset.
    pub fn set_anchors(&self, anchors: DraggableAnchors<K>) {
        let inner = &self.inner;
        let pixels = inner.scroll.pixels();
        let current = inner.current.get();
        let key = if anchors.offset(current).is_some() {
            current
        } else {
            anchors.closest(pixels)
        };
        let target = anchors.offset(key).unwrap_or(pixels);
        let (min, max) = anchors.extent();
        let policy = Arc::clone(&anchors.policy);
        // Commit before anything that reaches user code: listeners of the
        // position and of this controller may read or replace the anchors.
        let outgoing = inner.anchors.replace(anchors);
        inner.current.set(key);
        let owner = inner.slot.owner();
        if let Some(owner) = &owner {
            owner.set_snap(Some(policy));
        }
        drop(outgoing);
        inner
            .scroll
            .update_dimensions(max - min, min, max)
            .expect("BUG: anchors are sorted, finite and within MAX_ANCHOR_OFFSET");
        let dragging = owner
            .as_ref()
            .is_some_and(|owner| owner.contact().is_some());
        if !dragging && inner.scroll.pixels() != target {
            if owner.is_some() {
                inner.scroll.animate_to(
                    target,
                    RESETTLE,
                    ArcCurve::new(flui_animation::curves::EASE_OUT),
                );
            } else {
                inner.scroll.jump_to(target);
            }
        }
        inner.notifier.notify_listeners();
    }

    /// Animate to `key`'s anchor over `duration`, easing through `curve`,
    /// as a programmatic scroll; a drag on the child takes over.
    ///
    /// # Errors
    ///
    /// [`AnchorsError::Missing`] when `key` is not one of the anchors.
    pub fn animate_to(
        &self,
        key: K,
        duration: Duration,
        curve: ArcCurve,
    ) -> Result<(), AnchorsError> {
        let target = self.inner.anchors.borrow().offset(key);
        let target = target.ok_or(AnchorsError::Missing)?;
        self.inner.scroll.animate_to(target, duration, curve);
        Ok(())
    }

    /// Jump to `key`'s anchor, as a programmatic scroll.
    ///
    /// # Errors
    ///
    /// [`AnchorsError::Missing`] when `key` is not one of the anchors.
    pub fn jump_to(&self, key: K) -> Result<(), AnchorsError> {
        let target = self.inner.anchors.borrow().offset(key);
        let target = target.ok_or(AnchorsError::Missing)?;
        let from = self.inner.scroll.pixels();
        self.inner.scroll.jump_to(target);
        // Without a mounted drag the jump moves the position directly, and
        // no owner reports the arrival.
        if self.inner.arrive(from, self.inner.scroll.pixels()) {
            self.inner.notifier.notify_listeners();
        }
        Ok(())
    }

    /// Put the child on the anchor nearest its offset, at once: the drag
    /// that was moving it is gone. A child resting on an anchor stays.
    fn rest_on_nearest(&self) {
        let inner = &self.inner;
        let from = inner.scroll.pixels();
        let anchors = self.anchors();
        let key = anchors.closest(from);
        let target = anchors.offset(key).unwrap_or(from);
        if target == from && !self.is_moving() && key == inner.current.get() {
            return;
        }
        inner.scroll.set_pixels(target);
        inner.current.set(key);
        inner.scroll.position().set_is_scrolling(false);
        inner.notifier.notify_listeners();
    }

    fn same(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

/// A one-dimensional drag of its child that comes to rest on anchors.
///
/// The base of swipe actions, dismissible rows and sheets. A drag along
/// `axis` moves the child; a release settles it on an anchor of the
/// controller, a fling on the next anchor in its direction. Arriving at
/// another anchor than the current one, by a drag or a settle, plays a
/// selection haptic with the frame that shows it; programmatic moves play
/// none.
///
/// In the nested scroll channel (ADR-0209 §9) the drag is a bounded leaf: it
/// takes its axis first and passes the remainder to the scrollables around
/// it, and an enclosing scrollable on the same axis loses the contact to it.
/// For scrollables inside it the drag is a participant: a list at its edge
/// hands the rest of a drag on the drag's axis to it, and the list's release
/// settles it.
///
/// A drag is a path: the constructor takes the drag's
/// [`GestureAlternatives`] (ADR-0216), whose commands name [`AnchorIntent`],
/// which the drag answers by moving to the next or the previous anchor by
/// offset. They make the drag a row of focus traversal, its one stop, with one
/// custom action per command, the commands' keys while focus is in the row,
/// and a control per command for one pointer, so a sheet opens and closes
/// without a drag. Focused by Tab or by a drag, the arrow keys along the axis
/// move to the next anchor the way they point.
///
/// Assistive technology also sees a slider: increment and decrement move to
/// the next and the previous anchor by offset. Named with
/// [`semantics`](Self::semantics), the slider reads its label and the
/// current anchor's description.
///
/// A [`VsyncScope`](crate::VsyncScope) above it runs the settle.
///
/// ```
/// use flui_animation::Vsync;
/// use flui_foundation::geometry::Axis;
/// use flui_widgets::{
///     Alternative, AnchorIntent, AnchoredDrag, AnchoredDragController, ControlSpec,
///     DraggableAnchors, GestureAlternatives, GestureCommand, SizedBox, Text, VsyncScope,
/// };
///
/// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// enum Row {
///     Closed,
///     Actions,
/// }
///
/// let anchors = DraggableAnchors::builder()
///     .at(Row::Closed, 0.0)
///     .at(Row::Actions, -96.0)
///     .build()
///     .expect("two finite anchors");
/// let controller = AnchoredDragController::new(anchors, Row::Closed).expect("a known anchor");
/// let alternatives = GestureAlternatives::new(Alternative::VisibleControl(
///     ControlSpec::new(GestureCommand::new("Show actions", AnchorIntent::Previous), Text::new("<"))
///         .control(GestureCommand::new("Hide actions", AnchorIntent::Next), Text::new(">")),
/// ))
/// .expect("two distinct labels");
/// let row = VsyncScope::new(
///     Vsync::new(),
///     AnchoredDrag::new(
///         controller.clone(),
///         Axis::Horizontal,
///         SizedBox::new(320.0, 56.0),
///         alternatives,
///     ),
/// );
/// assert_eq!(controller.current(), Row::Closed);
/// ```
#[derive(Clone)]
pub struct AnchoredDrag<K> {
    controller: AnchoredDragController<K>,
    axis: Axis,
    child: BoxedView,
    assistive: Assistive<K>,
    admission: Option<ContactAdmission>,
    cancelled: Option<Rc<dyn Fn()>>,
    /// `None` inside a widget whose own alternatives perform its outcomes.
    alternatives: Option<GestureAlternatives>,
}

/// What assistive technology sees of an [`AnchoredDrag`].
enum Assistive<K> {
    /// A slider without a name.
    Unnamed,
    /// A slider named `label`, its value `describe` of the current anchor.
    Named {
        label: Rc<str>,
        describe: Rc<dyn Fn(K) -> String>,
    },
    /// Nothing: the widget around the drag publishes what it does instead.
    Withheld,
}

impl<K> Clone for Assistive<K> {
    fn clone(&self) -> Self {
        match self {
            Self::Unnamed => Self::Unnamed,
            Self::Named { label, describe } => Self::Named {
                label: Rc::clone(label),
                describe: Rc::clone(describe),
            },
            Self::Withheld => Self::Withheld,
        }
    }
}

impl<K: fmt::Debug + Copy> fmt::Debug for AnchoredDrag<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match &self.assistive {
            Assistive::Named { label, .. } => Some(&**label),
            Assistive::Unnamed | Assistive::Withheld => None,
        };
        f.debug_struct("AnchoredDrag")
            .field("controller", &self.controller)
            .field("axis", &self.axis)
            .field("label", &label)
            .finish_non_exhaustive()
    }
}

impl<K: Copy + Eq + 'static> AnchoredDrag<K> {
    /// Drag `child` along `axis` between `controller`'s anchors, moved
    /// between them without a drag through `alternatives`, whose commands
    /// name [`AnchorIntent`].
    pub fn new(
        controller: AnchoredDragController<K>,
        axis: Axis,
        child: impl IntoView,
        alternatives: GestureAlternatives,
    ) -> Self {
        Self {
            controller,
            axis,
            child: child.into_view().boxed(),
            assistive: Assistive::Unnamed,
            admission: None,
            cancelled: None,
            alternatives: Some(alternatives),
        }
    }

    /// The drag of a widget that reaches its outcomes by assistive
    /// technology, the keyboard and one pointer itself (a swiped row): no
    /// slider and no alternatives of its own.
    pub(crate) fn inside(
        controller: AnchoredDragController<K>,
        axis: Axis,
        child: impl IntoView,
    ) -> Self {
        Self {
            controller,
            axis,
            child: child.into_view().boxed(),
            assistive: Assistive::Withheld,
            admission: None,
            cancelled: None,
            alternatives: None,
        }
    }

    /// Name the drag's slider for assistive technology: it reads `label`
    /// and, as its value, `describe` of the current anchor ("Half open").
    #[must_use]
    pub fn semantics(
        mut self,
        label: impl Into<String>,
        describe: impl Fn(K) -> String + 'static,
    ) -> Self {
        self.assistive = Assistive::Named {
            label: Rc::from(label.into()),
            describe: Rc::new(describe),
        };
        self
    }

    /// Take a contact only when `admission` accepts its Down; a refused
    /// contact goes to whatever else it hits.
    #[must_use]
    pub fn admission(mut self, admission: ContactAdmission) -> Self {
        self.admission = Some(admission);
        self
    }

    /// Call `cancelled` when a drag on the child is cancelled, after its
    /// settle began, so it can replace that settle.
    #[must_use]
    pub(crate) fn on_cancelled(mut self, cancelled: impl Fn() + 'static) -> Self {
        self.cancelled = Some(Rc::new(cancelled));
        self
    }
}

/// The participant an [`AnchoredDrag`] is in the chains of the scrollables
/// inside it: their remainder on its axis moves it, their release settles
/// it.
struct Ancestor {
    slot: Rc<LeafSlot>,
    pipeline: Rc<RefCell<Option<WeakPipelineCell>>>,
    /// The sequence that moved the drag and has not settled it yet.
    moved: Cell<Option<(InputSequenceId, ScrollDevice)>>,
    /// The sequence whose release settles the drag now.
    settling: Cell<Option<InputSequenceId>>,
}

impl Ancestor {
    fn device_pixel_ratio(&self) -> f64 {
        presentation_device_pixel_ratio(self.pipeline.borrow().as_ref())
    }

    /// Run `body` on the drag's owner as a pass through it: the chain that
    /// calls this participant does not hold the drag's owner, and a scroll
    /// a callback it reaches requests must wait for the pass, not re-enter
    /// it.
    fn on_owner<R>(
        &self,
        body: impl FnOnce(&Rc<ScrollOwner>, &mut PanicRecovery) -> R,
    ) -> Option<R> {
        let owner = self.slot.owner()?;
        let mut recovery = PanicRecovery::new();
        let result = owner.within_pass(&mut recovery, |recovery| body(&owner, recovery));
        recovery.finish();
        Some(result)
    }

    /// Settle the drag for `sequence` at `velocity`; true when a run started.
    fn settle(
        &self,
        owner: &Rc<ScrollOwner>,
        sequence: InputSequenceId,
        device: ScrollDevice,
        velocity: Vec2,
    ) -> bool {
        let started =
            owner.settle_from_inside(sequence, device, velocity, self.device_pixel_ratio());
        if started {
            self.settling.set(Some(sequence));
        } else if owner.contact().is_none() {
            owner.controller().position().set_is_scrolling(false);
        }
        started
    }
}

/// The drag as an ancestor is a connection, not a scrollable link of the
/// chains inside it: it takes `UserInput` remainders only and settles itself
/// from `released` (ADR-0209 §9), where the channel settles a scrollable
/// link it moved (§6a).
impl NestedScrollConnection for Ancestor {
    fn post_scroll(&self, step: &ScrollStep, _consumed_by_child: Vec2) -> Vec2 {
        // Only the user's own motion drags it; a fling settles it instead.
        if step.cause != ScrollCause::UserInput {
            return Vec2::ZERO;
        }
        self.on_owner(|owner, recovery| {
            if owner.contact().is_some() {
                return Vec2::ZERO;
            }
            let taken = owner.take_remainder(step, recovery);
            if taken != Vec2::ZERO {
                self.moved.set(Some((step.sequence, step.device)));
                owner.controller().position().set_is_scrolling(true);
            }
            taken
        })
        .unwrap_or(Vec2::ZERO)
    }

    fn released(&self, step: &FlingStep) {
        let Some((sequence, device)) = self.moved.get().filter(|(id, _)| *id == step.sequence)
        else {
            return;
        };
        self.moved.set(None);
        let velocity = step.available.pixels_per_second;
        let _ = self.on_owner(|owner, _| {
            self.settle(owner, sequence, device, Vec2::new(velocity.dx, velocity.dy))
        });
    }

    fn post_fling(&self, step: &FlingStep) -> FlingHandoff {
        if self.settling.get() == Some(step.sequence) {
            return FlingHandoff::Declined;
        }
        let velocity = step.available.pixels_per_second;
        let started = self.on_owner(|owner, _| {
            self.settle(
                owner,
                step.sequence,
                step.device,
                Vec2::new(velocity.dx, velocity.dy),
            )
        });
        if started == Some(true) {
            FlingHandoff::Accepted
        } else {
            FlingHandoff::Declined
        }
    }

    fn sequence_ended(&self, sequence: InputSequenceId, _reason: ScrollEndReason) {
        if self.settling.get() == Some(sequence) {
            self.settling.set(None);
        }
        let Some((_, device)) = self.moved.get().filter(|(id, _)| *id == sequence) else {
            return;
        };
        // Cancelled, or a contactless sequence: no release came.
        self.moved.set(None);
        let _ = self.on_owner(|owner, _| self.settle(owner, sequence, device, Vec2::ZERO));
    }
}

/// Persistent state for [`AnchoredDrag`]: the haptics capability, the hook
/// its scrollable reports moves to, and its place in the chains of the
/// scrollables inside it.
pub struct AnchoredDragState<K> {
    controller: AnchoredDragController<K>,
    haptics: Rc<RefCell<Option<HapticsHandle>>>,
    pipeline: Rc<RefCell<Option<WeakPipelineCell>>>,
    motion: MotionHook,
    ancestor: Rc<Ancestor>,
    /// The chain node the scrollables inside see, kept while the ancestor
    /// and the chain above stay the same.
    node: RefCell<Option<Rc<ChainNode>>>,
    /// The plane its connection takes remainders in: the box around the
    /// translated child, the drag's own scrollable's.
    plane: PlaneSource,
    /// The child's painted offset, which its transform follows; kept across
    /// controllers, its source replaced when the axis, the reading direction
    /// or the controller changes.
    offset: ProxyAnimation<Offset<f64>>,
    /// What `apply_view` resolved from the view and the ambient scopes, for
    /// `build` to read.
    placed: Option<Placed>,
}

/// How the drag sits in its surroundings: the reading direction its axis was
/// placed under, the scrollable's axis direction and the node it is in the
/// chains with.
#[derive(Clone)]
struct Placed {
    axis: Axis,
    rtl: bool,
    axis_direction: AxisDirection,
    node: Rc<ChainNode>,
}

/// The child's painted offset: the drag's offset along its axis, toward the
/// reading end on a horizontal one.
struct DragOffset {
    scroll: ScrollController,
    axis: Axis,
    rtl: bool,
}

impl fmt::Debug for DragOffset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DragOffset")
            .field("offset", &self.scroll.pixels())
            .field("axis", &self.axis)
            .field("rtl", &self.rtl)
            .finish()
    }
}

impl Listenable for DragOffset {
    fn add_observer(&self, observer: ListenerObserver) -> ListenerId {
        self.scroll.listenable().add_observer(observer)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.scroll.listenable().remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.scroll.listenable().remove_all_listeners();
    }
}

impl Animation<Offset<f64>> for DragOffset {
    fn value(&self) -> Offset<f64> {
        let offset = self.scroll.pixels();
        match (self.axis, self.rtl) {
            (Axis::Vertical, _) => Offset::new(0.0, offset),
            (Axis::Horizontal, true) => Offset::new(-offset, 0.0),
            (Axis::Horizontal, false) => Offset::new(offset, 0.0),
        }
    }

    fn status(&self) -> AnimationStatus {
        AnimationStatus::Forward
    }

    fn observe_status(&self, observer: StatusObserver) -> StatusSubscription {
        drop(observer);
        StatusSubscription::default()
    }

    fn is_animating(&self) -> bool {
        self.scroll.position().is_scrolling()
    }
}

impl<K> fmt::Debug for AnchoredDragState<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnchoredDragState").finish_non_exhaustive()
    }
}

impl<K: Copy + Eq + 'static> AnchoredDragState<K> {
    fn new(
        controller: &AnchoredDragController<K>,
        haptics: Rc<RefCell<Option<HapticsHandle>>>,
        pipeline: Rc<RefCell<Option<WeakPipelineCell>>>,
    ) -> Self {
        Self {
            motion: motion_hook(Rc::downgrade(&controller.inner), Rc::clone(&haptics)),
            ancestor: Rc::new(Ancestor {
                slot: Rc::clone(&controller.inner.slot),
                pipeline: Rc::clone(&pipeline),
                moved: Cell::new(None),
                settling: Cell::new(None),
            }),
            controller: controller.clone(),
            haptics,
            pipeline,
            node: RefCell::new(None),
            plane: PlaneSource::default(),
            offset: ProxyAnimation::new(Rc::new(ConstantAnimation::new(Offset::ZERO))),
            placed: None,
        }
    }

    fn acquire(&self, ctx: &dyn LifecycleContext) {
        *self.haptics.borrow_mut() = ctx.haptics();
        *self.pipeline.borrow_mut() = ctx.pipeline_owner().map(|cell| cell.downgrade());
        self.plane.set_pipeline(self.pipeline.borrow().clone());
    }

    /// The node the scrollables inside put this drag in their chains with.
    fn node(&self, parent: Option<Rc<ChainNode>>) -> Rc<ChainNode> {
        let connection =
            Rc::downgrade(&(Rc::clone(&self.ancestor) as Rc<dyn NestedScrollConnection>));
        let current = self.node.borrow().clone();
        if let Some(node) = current
            && node.is_connection(&connection)
            && node.has_parent(parent.as_ref())
        {
            return node;
        }
        let node = Rc::new(ChainNode::new(
            Participant::Connection(connection),
            parent,
            self.plane.clone(),
        ));
        *self.node.borrow_mut() = Some(Rc::clone(&node));
        node
    }

    /// Move to the anchor at `target` for an assistive action.
    fn assistive_move(slot: &LeafSlot, pipeline: &RefCell<Option<WeakPipelineCell>>, target: f64) {
        let Some(owner) = slot.owner() else {
            return;
        };
        let ratio = presentation_device_pixel_ratio(pipeline.borrow().as_ref());
        if owner.move_to(target, ScrollCause::UserInput, ratio) == MoveOutcome::Moved {
            owner.controller().position().set_is_scrolling(false);
        }
    }

    /// `drag` as assistive technology's slider: increment and decrement move
    /// to the next and the previous anchor by offset.
    fn slider(
        &self,
        controller: &AnchoredDragController<K>,
        assistive: &Assistive<K>,
        drag: BoxedView,
    ) -> BoxedView {
        let anchors = controller.anchors();
        let semantics = match assistive {
            Assistive::Named { label, describe } => Semantics::new()
                .label(label.to_string())
                .value(describe(controller.current())),
            Assistive::Unnamed | Assistive::Withheld => Semantics::new(),
        };
        let mut semantics = semantics.container(true).role(SemanticsRole::Slider);
        let from = anchors
            .offset(controller.current())
            .unwrap_or_else(|| controller.offset());
        if let Some(target) = anchors.after(from) {
            let slot = Rc::clone(&controller.inner.slot);
            let pipeline = Rc::clone(&self.pipeline);
            semantics = semantics.on_increment(move |_cx| {
                Self::assistive_move(&slot, &pipeline, target);
            });
        }
        if let Some(target) = anchors.before(from) {
            let slot = Rc::clone(&controller.inner.slot);
            let pipeline = Rc::clone(&self.pipeline);
            semantics = semantics.on_decrement(move |_cx| {
                Self::assistive_move(&slot, &pipeline, target);
            });
        }
        semantics.child(drag).boxed()
    }
}

/// Move the enclosing [`AnchoredDrag`] to the next or the previous anchor
/// by offset: down on a vertical drag, toward the reading end on a
/// horizontal one, or back.
///
/// The drag answers it for everything inside it: the commands of its
/// [`GestureAlternatives`] name it. Past the last or before the first anchor
/// it is not performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnchorIntent {
    /// The anchor after the current one by offset.
    Next,
    /// The anchor before the current one by offset.
    Previous,
}

impl Intent for AnchorIntent {}

/// Answers [`AnchorIntent`] for everything inside an [`AnchoredDrag`].
struct AnchorAction<K> {
    controller: AnchoredDragController<K>,
    pipeline: Rc<RefCell<Option<WeakPipelineCell>>>,
}

impl<K: Copy + Eq + 'static> AnchorAction<K> {
    /// The offset of the anchor `intent` moves to, if there is one.
    fn target(&self, intent: AnchorIntent) -> Option<f64> {
        let anchors = self.controller.anchors();
        let from = anchors
            .offset(self.controller.current())
            .unwrap_or_else(|| self.controller.offset());
        match intent {
            AnchorIntent::Next => anchors.after(from),
            AnchorIntent::Previous => anchors.before(from),
        }
    }
}

impl<K: Copy + Eq + 'static> Action<AnchorIntent> for AnchorAction<K> {
    /// Past the last or before the first anchor the intent is not enabled,
    /// so a key there bubbles on.
    fn is_enabled(&self, intent: &AnchorIntent) -> bool {
        self.target(*intent).is_some()
    }

    fn enablement_changes(&self) -> Option<ChangeNotifier> {
        Some(ChangeNotifier::clone(&self.controller.inner.notifier))
    }

    fn invoke(&self, _cx: &mut EventCx<'_>, intent: &AnchorIntent) -> ActionOutcome {
        let Some(target) = self.target(*intent) else {
            return ActionOutcome::NotPerformed;
        };
        AnchoredDragState::<K>::assistive_move(&self.controller.inner.slot, &self.pipeline, target);
        ActionOutcome::Performed
    }
}

/// The hook that tracks the current anchor of `inner` and plays the anchor
/// haptic (once per arrival at another anchor, for every cause but a
/// programmatic one).
fn motion_hook<K: Copy + Eq + 'static>(
    inner: Weak<Inner<K>>,
    haptics: Rc<RefCell<Option<HapticsHandle>>>,
) -> MotionHook {
    Rc::new(move |from, to, cause| {
        let Some(inner) = inner.upgrade() else {
            return;
        };
        if !inner.arrive(from, to) {
            return;
        }
        if cause != ScrollCause::Programmatic {
            let handle = haptics.borrow().clone();
            if let Some(handle) = handle {
                let _ = handle.perform(
                    HapticFeedback::SelectionClick,
                    HapticRequest::new(HapticTiming::FramePresented),
                );
            }
        }
        inner.notifier.notify_listeners();
    })
}

impl<K: Copy + Eq + 'static> StatefulView for AnchoredDrag<K> {
    type State = AnchoredDragState<K>;

    fn create_state(&self) -> Self::State {
        AnchoredDragState::new(&self.controller, Rc::default(), Rc::default())
    }
}

impl<K: Copy + Eq + 'static> AnimatedView for AnchoredDrag<K> {
    fn listenable(&self) -> Rc<dyn Listenable> {
        self.controller.listenable()
    }
}

impl<K: Copy + Eq + 'static> View for AnchoredDrag<K> {
    fn create_element(&self) -> ElementKind {
        ElementKind::animated(self)
    }
}

impl<K: Copy + Eq + 'static> ViewState<AnchoredDrag<K>> for AnchoredDragState<K> {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.acquire(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.acquire(ctx);
    }

    fn apply_view(&mut self, view: &AnchoredDrag<K>, ctx: &dyn BuildContext) {
        let controller = &view.controller;
        let axis = view.axis;
        let rtl =
            axis == Axis::Horizontal && Directionality::maybe_of(ctx) == Some(TextDirection::Rtl);
        // The opposite of the scroll convention: the content follows the
        // finger, so the scrollable's pixels are the child's offset.
        let axis_direction = match (axis, rtl) {
            (Axis::Vertical, _) => AxisDirection::BottomToTop,
            (Axis::Horizontal, false) => AxisDirection::RightToLeft,
            (Axis::Horizontal, true) => AxisDirection::LeftToRight,
        };
        let node = self.node(NestedScrollScope::parent_of(ctx));
        let unchanged = self
            .placed
            .as_ref()
            .is_some_and(|placed| (placed.axis, placed.rtl) == (axis, rtl));
        if !unchanged {
            self.offset.set_parent(Rc::new(DragOffset {
                scroll: controller.inner.scroll.clone(),
                axis,
                rtl,
            }));
        }
        self.placed = Some(Placed {
            axis,
            rtl,
            axis_direction,
            node,
        });
    }

    fn build(&self, view: &AnchoredDrag<K>, _ctx: &dyn BuildContext) -> impl IntoView {
        let controller = &view.controller;
        let anchors = controller.anchors();
        let axis = view.axis;
        let Placed {
            rtl,
            axis_direction,
            node,
            ..
        } = self
            .placed
            .clone()
            .expect("BUG: `apply_view` runs before every build");
        let offset = self.offset.clone();
        // The whole child is hit where it is painted, so the strip a drag
        // uncovers reaches what lies under the drag.
        let child = Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .child(view.child.clone())
            .boxed();
        let anchor = self.plane.anchor();
        let content = Rc::new(move |_position| {
            // A move patches the transform's layer: nothing rebuilds.
            AnchoredBox::new(
                anchor.clone(),
                NestedScrollScope::new(
                    Rc::clone(&node),
                    AnimatedTransformView {
                        motion: TransformMotion::Translate {
                            offset: offset.clone(),
                        },
                        transform_hit_tests: true,
                        text_direction: TextDirection::Ltr,
                        child: child.clone(),
                    },
                ),
            )
            .boxed()
        });
        let scrollable = Scrollable::new()
            .controller(controller.inner.scroll.clone())
            .scroll_direction(axis)
            .axis_direction(axis_direction)
            .snap_policy(Arc::clone(&anchors.policy))
            .viewport_builder(content)
            .anchored(AnchoredLeaf {
                motion: Rc::clone(&self.motion),
                slot: Rc::clone(&controller.inner.slot),
                admission: view.admission.clone(),
                cancelled: view.cancelled.clone(),
            });
        let drag = match &view.assistive {
            Assistive::Withheld => scrollable.boxed(),
            assistive => self.slider(controller, assistive, scrollable.boxed()),
        };
        let Some(alternatives) = &view.alternatives else {
            return drag;
        };
        // The row of the alternatives is the drag's stop of focus traversal:
        // there the arrow keys move to the next anchor the way they point,
        // as they do once a drag has focused the drag itself.
        let (next, previous) = anchor_keys(axis, rtl);
        let keys = Shortcuts::new(alternatives.clone().child(drag))
            .shortcut(SingleActivator::named(next), AnchorIntent::Next)
            .shortcut(SingleActivator::named(previous), AnchorIntent::Previous);
        Actions::new(keys)
            .action(AnchorAction {
                controller: controller.clone(),
                pipeline: Rc::clone(&self.pipeline),
            })
            .boxed()
    }

    fn did_update_view(&mut self, _old_view: &AnchoredDrag<K>, new_view: &AnchoredDrag<K>) {
        if self.controller.same(&new_view.controller) {
            return;
        }
        // The incoming controller gets its own hook and participant; the
        // outgoing one is no longer shown, so it rests.
        let incoming = Self::new(
            &new_view.controller,
            Rc::clone(&self.haptics),
            Rc::clone(&self.pipeline),
        );
        let mut outgoing = std::mem::replace(self, incoming);
        // The transform keeps its proxy: the next build points it at the
        // incoming controller.
        std::mem::swap(&mut self.offset, &mut outgoing.offset);
        outgoing.controller.rest_on_nearest();
    }

    fn dispose(&mut self) {
        self.haptics.borrow_mut().take();
        self.controller.rest_on_nearest();
    }
}

/// The keys that move a drag along `axis` to its next and previous anchor:
/// the arrows the way they point, mirrored right-to-left. The collision table
/// checks them on every platform.
pub(crate) fn anchor_keys(axis: Axis, rtl: bool) -> (NamedKey, NamedKey) {
    match (axis, rtl) {
        (Axis::Vertical, _) => (NamedKey::ArrowDown, NamedKey::ArrowUp),
        (Axis::Horizontal, false) => (NamedKey::ArrowRight, NamedKey::ArrowLeft),
        (Axis::Horizontal, true) => (NamedKey::ArrowLeft, NamedKey::ArrowRight),
    }
}
