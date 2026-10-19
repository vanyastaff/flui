//! Gesture recognition: the arena that arbitrates competing recognizers, the
//! recognizers themselves, their settings and callback details, and the
//! velocity estimation and smoothing they measure motion with.
//!
//! - [`GestureArena`] - Conflict resolution between recognizers
//! - [`GestureRecognizer`] - The recognizer contract, with [`TapGestureRecognizer`],
//!   [`DragGestureRecognizer`], [`ScaleGestureRecognizer`] and the rest
//! - [`GestureSettings`] - Slops, timeouts and fling limits
//! - [`VelocityTracker`] - Velocity estimation from pointer movement
//! - [`OneEuroFilter`] - Smoothing of a noisy signal; recognizers apply it to
//!   reported positions after recognition when settings select a [`OneEuroTuning`]
//!
//! # Example
//!
//! ```rust
//! use flui_interaction::{PointerKind, gesture::VelocityTracker};
//! use flui_foundation::geometry::Offset;
//! use web_time::Instant;
//!
//! let mut tracker = VelocityTracker::with_kind(PointerKind::Touch);
//! let now = Instant::now();
//! tracker.add_position(now, Offset::ZERO);
//! let velocity = tracker.velocity_at(now);
//! ```

pub(crate) mod arena;
mod details;
mod lsq_solver;
mod observability;
mod observer;
mod one_euro;
pub(crate) mod recognizers;
pub(crate) mod settings;
mod traits;
mod velocity;
mod velocity_tracker;

pub use arena::{
    CompositionBranch, CompositionError, CompositionKind, GestureArena, GestureArenaEntry,
    GestureArenaMember, GestureBranches, GestureComposition, GestureDeadlineError,
    GestureDeadlineRegistration, GestureDeadlineUpdate, GestureDisposition, MotionRole, PanClaim,
    ScrollLatchEnd, SequencedBranch, SweepModel,
};
pub use details::{
    ForcePressDetails, LongPressEndDetails, LongPressMoveUpdateDetails, TapDownDetails,
    TapUpDetails,
};
pub use observability::{GestureEvent, SPAN_ARENA, SPAN_RECOGNIZER, pointer_event_kind};
pub use observer::{
    ContestCancel, GestureContest, GestureObservation, GestureObserver, ObservedComposition,
    ObservedMember, ObserverHandle, RejectionReason,
};
pub use one_euro::{OneEuroFilter, OneEuroFilter2D, OneEuroFilterError, OneEuroTuning};
pub(crate) use one_euro::{PositionSmoother, PublishedPosition};
pub use recognizers::*;
pub use settings::*;
pub(crate) use traits::on_rail;
pub use traits::{AxisLock, DragAxis, TransformLock};
pub use velocity::{Velocity, VelocityEstimate};
pub use velocity_tracker::{VelocityEstimator, VelocityTracker};
