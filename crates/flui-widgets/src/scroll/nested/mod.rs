//! The nested scroll channel (ADR-0209): every change of a scroll offset is a
//! delta one owner-local chain of participants shares.
//!
//! A scroll leaf — the [`Scrollable`](super::Scrollable) that won a contact's
//! arena, or the one under the wheel or focus for a contactless source —
//! dispatches each delta as a [`ScrollStep`] through the chain of
//! [`NestedScrollConnection`]s and scrollables above it: ancestors may take a
//! part first ([`pre_scroll`](NestedScrollConnection::pre_scroll), root to
//! leaf), the leaf takes what it can, and the remainder goes back up
//! ([`post_scroll`](NestedScrollConnection::post_scroll), leaf to root). A
//! fling is the same stream of deltas with [`ScrollCause::SideEffect`]; what
//! its leaf cannot carry past its edge is offered as a [`FlingStep`] to
//! [`post_fling`](NestedScrollConnection::post_fling), nearest first, and an
//! accepting scrollable runs it under the same sequence.
//!
//! # Vocabulary
//!
//! - **Sequence.** Every step names the [`InputSequenceId`] the binding minted
//!   for the input it came from: a contact, a wheel or trackpad latch, an
//!   assistive action, or a programmatic scroll. A participant sees
//!   [`sequence_started`](NestedScrollConnection::sequence_started) before the
//!   first step and [`sequence_ended`](NestedScrollConnection::sequence_ended)
//!   once, after the last run of the sequence.
//! - **Direction.** [`ScrollStep::available`] and [`FlingStep::available`]
//!   are in physical screen axes and point the way the content's offset
//!   grows for an unreversed scrollable: a finger moving up scrolls down, so
//!   it offers a positive `y`. Each scrollable projects a step onto its own
//!   axis and orientation.
//! - **Latch.** Per axis, the chain latches to the deepest participant that
//!   consumes a non-zero part of the sequence's first non-zero delta (the
//!   leaf included; a leaf at its edge does not latch). Under the default
//!   [`RemainderHandoff::NextSequence`](flui_interaction::RemainderHandoff)
//!   the remainder goes nowhere else for the rest of the sequence; an
//!   ancestor moves on the next one. A preset that says
//!   [`AtEdgeWithinSequence`](flui_interaction::RemainderHandoff::AtEdgeWithinSequence)
//!   re-latches to the nearest consuming ancestor once the latched
//!   participant consumes nothing at its edge, never back.
//!   [`pre_scroll`](NestedScrollConnection::pre_scroll) is offered regardless
//!   of the latch.
//! - **Containment.** A scrollable with [`OverscrollBehavior::Contain`] keeps
//!   the remainder and the fling residual of its subtree: nothing above it
//!   is offered either.
//!
//! # What is not a scroll
//!
//! Anchor correction (ADR-0051) keeps the content under the viewport still
//! while layout changes extents above it. The viewport applies it inside
//! layout: no participant sees it, and it neither ends nor latches a
//! sequence.
//!
//! # Threading
//!
//! Scrolling is owner-driven. The step, cause, device, end-reason, overscroll
//! and snap types are `Send` data; a connection is not `Send` and may capture
//! `Rc` state. A future compositor-side scroller would admit only chains
//! without user connections.

mod chain;
mod snap;

use std::rc::Rc;

use flui_foundation::geometry::Vec2;
use flui_interaction::{InputSequenceId, Velocity};
use flui_platform_api::pointer::{DeviceId, PointerKind};
use flui_view::prelude::StatefulView;
use flui_view::{BoxedView, BuildContext, IntoView, ViewExt, ViewState};

pub(crate) use chain::{
    Acceptance, ChainNode, Handoff, NestedScrollScope, Participant, ScrollSequence,
};
pub use snap::{
    AnchorSnap, AnchorSnapError, InvalidViewportFraction, PageSnap, SnapPolicy, ViewportFraction,
};

/// What moved the offset a step carries (ADR-0209 §2).
///
/// Participants decide by the cause, never by the size of a delta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ScrollCause {
    /// A contact, a wheel, a key or an assistive scroll action.
    UserInput,
    /// Inertia the platform delivers after the user let go (trackpad
    /// momentum phases): the platform's own fling.
    Momentum,
    /// Motion the framework drives on the user's behalf: a fling, a settle
    /// back into range.
    SideEffect,
    /// An application request: a controller jump or animation, a reveal.
    /// The default connection methods take none of it; a participant that
    /// wants to react opts in by checking the cause.
    Programmatic,
}

/// The device a sequence runs on (ADR-0209 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ScrollDevice {
    /// A pointer: a finger, a mouse wheel, a trackpad, a pen.
    Pointer {
        /// The kind of pointer.
        kind: PointerKind,
        /// The hardware behind it, when the platform reports it.
        device: Option<DeviceId>,
    },
    /// Keys scrolling the focused scrollable.
    Keyboard,
    /// An assistive technology's scroll action.
    Assistive,
    /// The application itself.
    Programmatic,
}

/// Why a sequence ended (ADR-0209 §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ScrollEndReason {
    /// The input finished: the contact lifted, or a contactless sequence was
    /// followed by the next one, and every run it started came to rest.
    Released,
    /// The input was cancelled, or its scrollable went away.
    Cancelled,
    /// New user input took over the motion the sequence still drove.
    Preempted,
    /// The arena gave the contact to a gesture outside this scrollable after
    /// the sequence had started: a press that stopped a moving scrollable
    /// became another scrollable's drag.
    ArenaLost,
}

/// A participant's answer to a fling residual.
///
/// Exhaustive: the answer is two-way, and growing it would change the
/// contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlingHandoff {
    /// The residual goes on to the next participant up the chain.
    Declined,
    /// The participant runs the residual itself. The channel offers it to
    /// no one else, and its own runs for the sequence are over.
    Accepted,
}

/// Where the remainder of a scrollable's subtree may go (CSS
/// `overscroll-behavior`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum OverscrollBehavior {
    /// The remainder and the fling residual go on up the chain.
    #[default]
    Auto,
    /// The remainder and the fling residual stop at this scrollable.
    Contain,
}

/// One delta a participant is offered (ADR-0209 §1).
///
/// Only the channel builds steps.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct ScrollStep {
    /// The input sequence the delta belongs to.
    pub sequence: InputSequenceId,
    /// What moved the offset.
    pub cause: ScrollCause,
    /// The device the sequence runs on.
    pub device: ScrollDevice,
    /// What is left of the delta when this participant is asked, in
    /// physical axes (see the [module](self) documentation).
    pub available: Vec2,
}

#[cfg(any(test, feature = "testing"))]
impl ScrollStep {
    /// A step for exercising a connection alone, in a test; the channel
    /// builds every other one.
    #[must_use]
    pub fn for_test(
        sequence: InputSequenceId,
        cause: ScrollCause,
        device: ScrollDevice,
        available: Vec2,
    ) -> Self {
        Self {
            sequence,
            cause,
            device,
            available,
        }
    }
}

#[cfg(any(test, feature = "testing"))]
impl FlingStep {
    /// A fling step for exercising a connection alone, in a test; the
    /// channel builds every other one.
    #[must_use]
    pub fn for_test(sequence: InputSequenceId, device: ScrollDevice, available: Velocity) -> Self {
        Self {
            sequence,
            device,
            available,
        }
    }
}

/// A fling residual a participant is offered, or the release velocity of a
/// sequence (ADR-0209 §1).
///
/// Only the channel builds steps.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct FlingStep {
    /// The input sequence the motion belongs to.
    pub sequence: InputSequenceId,
    /// The device the sequence runs on.
    pub device: ScrollDevice,
    /// The velocity, logical pixels per second in physical axes.
    pub available: Velocity,
}

/// A participant in the nested scroll channel (ADR-0209 §1).
///
/// Every method has a default that takes nothing. A participant answers
/// through `&self` and keeps its state in cells: the channel holds no
/// borrow across a call, and a scroll dispatched from inside one into the
/// same chain is refused with a diagnostic. A result that is not finite,
/// points the other way, or exceeds what was offered is clamped with a
/// diagnostic. A participant that panics counts as consuming everything;
/// its link closes, it receives
/// [`sequence_ended`](Self::sequence_ended) with
/// [`ScrollEndReason::Cancelled`], and the first panic resumes once the step
/// has finished.
///
/// The trait carries no `Send` bound: a connection lives on its owner's
/// thread and may capture `Rc` state.
pub trait NestedScrollConnection {
    /// A sequence reached this participant, before its first step.
    fn sequence_started(&self, _sequence: InputSequenceId) {}

    /// Take part of `step` before the leaf does. Called root to leaf.
    /// Returns what this participant consumed.
    fn pre_scroll(&self, _step: &ScrollStep) -> Vec2 {
        Vec2::ZERO
    }

    /// Take part of what the leaf and the participants below left. Called
    /// leaf to root; `consumed_by_child` is what everything below took of
    /// this step. Returns what this participant consumed.
    fn post_scroll(&self, _step: &ScrollStep, _consumed_by_child: Vec2) -> Vec2 {
        Vec2::ZERO
    }

    /// The sequence's last user contact lifted, with the release velocity,
    /// before any fling runs. Release decisions — a refresh past its
    /// threshold, a snap — belong here.
    fn released(&self, _step: &FlingStep) {}

    /// Take over the residual of a fling the participants below could not
    /// carry. Called nearest first, never for platform momentum.
    fn post_fling(&self, _step: &FlingStep) -> FlingHandoff {
        FlingHandoff::Declined
    }

    /// The sequence is over: its last run came to rest, or it ended for
    /// `reason`. Called once per sequence that started here.
    fn sequence_ended(&self, _sequence: InputSequenceId, _reason: ScrollEndReason) {}
}

/// Puts a [`NestedScrollConnection`] into the scroll chain of every
/// scrollable beneath it.
///
/// The connection is a participant of each sequence those scrollables
/// dispatch, above them and below any scrollable that encloses this widget.
///
/// ```rust
/// use std::cell::Cell;
/// use std::rc::Rc;
///
/// use flui_foundation::geometry::Vec2;
/// use flui_widgets::{NestedScroll, NestedScrollConnection, Scrollable, ScrollStep, SizedBox};
///
/// /// Takes the vertical remainder a list leaves at its edge, and counts it.
/// #[derive(Default)]
/// struct Overscroll(Cell<f64>);
///
/// impl NestedScrollConnection for Overscroll {
///     fn post_scroll(&self, step: &ScrollStep, _consumed_by_child: Vec2) -> Vec2 {
///         self.0.set(self.0.get() + step.available.y);
///         Vec2::new(0.0, step.available.y)
///     }
/// }
///
/// let overscroll = Rc::new(Overscroll::default());
/// let view = NestedScroll::new(
///     overscroll.clone(),
///     Scrollable::new().child(SizedBox::new(300.0, 1400.0)),
/// );
/// ```
#[derive(Clone, StatefulView)]
pub struct NestedScroll {
    connection: Rc<dyn NestedScrollConnection>,
    child: BoxedView,
}

impl std::fmt::Debug for NestedScroll {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NestedScroll").finish_non_exhaustive()
    }
}

impl NestedScroll {
    /// `connection` joins the scroll chain of the scrollables in `child`.
    pub fn new(connection: Rc<dyn NestedScrollConnection>, child: impl IntoView) -> Self {
        Self {
            connection,
            child: child.into_view().boxed(),
        }
    }
}

/// The chain node a [`NestedScroll`] publishes, kept while its connection
/// and its own parent stay the same, so the scrollables below keep theirs.
#[derive(Default)]
pub struct NestedScrollState {
    node: std::cell::RefCell<Option<Rc<ChainNode>>>,
}

impl std::fmt::Debug for NestedScrollState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NestedScrollState").finish_non_exhaustive()
    }
}

impl StatefulView for NestedScroll {
    type State = NestedScrollState;

    fn create_state(&self) -> Self::State {
        NestedScrollState::default()
    }
}

impl ViewState<NestedScroll> for NestedScrollState {
    fn build(&self, view: &NestedScroll, ctx: &dyn BuildContext) -> impl IntoView {
        let parent = NestedScrollScope::parent_of(ctx);
        let connection = Rc::downgrade(&view.connection);
        let current = self.node.borrow().clone();
        let node = match current {
            Some(node) if node.is_connection(&connection) && node.has_parent(parent.as_ref()) => {
                node
            }
            _ => {
                let node = Rc::new(ChainNode::new(Participant::Connection(connection), parent));
                *self.node.borrow_mut() = Some(Rc::clone(&node));
                node
            }
        };
        NestedScrollScope::new(node, view.child.clone())
    }
}
