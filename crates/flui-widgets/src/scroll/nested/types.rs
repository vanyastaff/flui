//! Data and authored participation in one owner-local scroll sequence.

use flui_animation::AnimationRunFuture;
use flui_foundation::{
    InputSequenceId,
    geometry::{Point, Vec2},
};
use flui_interaction::Velocity;

pub use flui_interaction::{ScrollCause, ScrollDevice, ScrollEndReason};

/// Motion offered in this participant's viewport coordinates.
///
/// The dispatcher projects the remaining presentation-space displacement into
/// each participant's committed plane. Returned consumption uses this same
/// local space. It must be finite, same-signed and bounded on each component.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct ScrollStep {
    /// Binding-owned identity, retained across release and framework inertia.
    pub sequence: InputSequenceId,
    /// Why this displacement was produced.
    pub cause: ScrollCause,
    /// Actual input source, with explicitly unknown backend identity allowed.
    pub device: ScrollDevice,
    /// Remaining local displacement in logical pixels.
    pub available: Vec2,
    /// The source's focal point in this participant's viewport.
    pub focal_point: Point,
    /// Presentation scale sampled for this motion; inertia retains release scale.
    pub device_pixel_ratio: f64,
}

/// Release or residual velocity offered to a participant in its own plane.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct FlingStep {
    /// The same identity that owned direct motion.
    pub sequence: InputSequenceId,
    /// Source of the released motion.
    pub device: ScrollDevice,
    /// Local logical pixels per second; projected as a tangent, not a chord.
    pub available: Velocity,
    /// Current local focal point.
    pub focal_point: Point,
    /// Presentation scale sampled at release.
    pub device_pixel_ratio: f64,
}

/// Whether a participant started the offered continuation.
///
/// An accepted run supplies a public completion observer. Dropping that future
/// does not stop the run: the participant cancels its actual animation when
/// `sequence_ended` reports cancellation or preemption. The dispatcher observes
/// this exact run, so a superseded run cannot end its successor's sequence.
#[derive(Debug)]
pub enum FlingHandoff {
    /// No continuation was started.
    Declined,
    /// A continuation was started, with its real completion observer.
    Accepted(AnimationRunFuture),
}

/// Whether an owner permits an unconsumed displacement to reach its ancestors.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum OverscrollBehavior {
    /// Follow the sequence's directional owner and platform transfer policy.
    #[default]
    Auto,
    /// Stop the remainder at this owner, including at an outward extent.
    Contain,
}

/// Synchronous authored participation before and after descendant scrolling.
///
/// Connections belong to the UI owner and may capture `Rc` state. No widget
/// state or dispatcher borrow is held during these calls. `pre_scroll` runs
/// from outermost to innermost, and `post_scroll` in the opposite order.
pub trait NestedScrollConnection {
    /// Called once before this connection sees the sequence's first step.
    fn sequence_started(&self, _sequence: InputSequenceId) {}

    /// Opt into controller commands and descendant reveal deliberately.
    fn accepts_programmatic(&self) -> bool {
        false
    }

    /// Consume before descendants, for example while collapsing a header.
    fn pre_scroll(&self, _step: &ScrollStep) -> Vec2 {
        Vec2::ZERO
    }

    /// Consume after descendants; `consumed_by_child` is in this same plane.
    fn post_scroll(&self, _step: &ScrollStep, _consumed_by_child: Vec2) -> Vec2 {
        Vec2::ZERO
    }

    /// The last direct contact released, before any framework fling starts.
    fn released(&self, _step: &FlingStep) {}

    /// Start a continuation of an unconsumed framework fling, nearest first.
    /// Platform momentum is already inertia and never invokes this hook.
    fn post_fling(&self, _step: &FlingStep) -> FlingHandoff {
        FlingHandoff::Declined
    }

    /// The sequence ended after its last accepted run, at most once per link.
    fn sequence_ended(&self, _sequence: InputSequenceId, _reason: ScrollEndReason) {}
}
