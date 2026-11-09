//! The composition-root seam of ADR-0081 §4: the frame drive.
//!
//! [`DriveFrame`] lets the crates that own a scheduler's owner frame
//! (`flui-runtime`, `flui-testing`) drive a complete frame from outside
//! `flui-scheduler`. The method is deliberately **not** an inherent method on
//! [`OwnerFrame`](crate::OwnerFrame): it resolves only where this trait is
//! imported, so a substrate crate cannot reach the frame entry by name.
//!
//! **No semver promise.** Anything here may change or disappear in any release.
//! The module is always compiled and `#[doc(hidden)]`.

use crate::{ExecutionError, IdleDeadline, Instant, OwnerFrame};

/// Drives a complete frame on an [`OwnerFrame`], off the public surface
/// (ADR-0081 §4). A frame runs the scheduler's begin, persistent-callback,
/// end-frame and idle phases around `prepare` and `pipeline`.
pub trait DriveFrame {
    /// Run one frame at `timestamp` with `deadline` as the idle-slice
    /// bound, `prepare` before and `pipeline` in the persistent slot.
    ///
    /// # Errors
    /// Refuses a recursive, retired or closed execution, as the sealed
    /// inherent method does.
    fn drive_frame<R>(
        &self,
        timestamp: Instant,
        deadline: IdleDeadline,
        prepare: impl FnMut(),
        pipeline: impl FnMut() -> R,
    ) -> Result<R, ExecutionError>;
}

impl DriveFrame for OwnerFrame {
    fn drive_frame<R>(
        &self,
        timestamp: Instant,
        deadline: IdleDeadline,
        prepare: impl FnMut(),
        pipeline: impl FnMut() -> R,
    ) -> Result<R, ExecutionError> {
        crate::OwnerFrame::drive_frame(self, timestamp, deadline, prepare, pipeline)
    }
}
