//! Driving a window from a test with no command queue in front of it.
//!
//! A window takes only an [`AdmittedCommand`](flui_platform::AdmittedCommand)
//! (ADR-0254 §2). [`AdmitAndApply`] reads as the command a test sends and runs
//! the one path for applying at once,
//! [`WindowCommand::admit_and_apply`].

use flui_platform::{ApplyError, CommandRefusal, PlatformWindow, WindowCommand};

/// Admit a command against the capabilities of a window's snapshot now, and
/// apply it.
pub trait AdmitAndApply {
    /// [`WindowCommand::admit_and_apply`] on this window.
    ///
    /// # Errors
    /// [`CommandRefusal::Admission`] when admission refuses the command,
    /// [`CommandRefusal::Apply`] when the window does.
    fn admit_and_apply(&self, command: WindowCommand) -> Result<(), CommandRefusal>;

    /// Close this window with [`WindowCommand::Close`] where a refusal fails
    /// the test: a window that has closed already is closed.
    ///
    /// Never call it from `Drop` or from cleanup after a caught panic: a
    /// second panic while unwinding aborts the test binary and loses the first
    /// failure. There, call [`close_for_cleanup`](Self::close_for_cleanup), or
    /// the fallible [`admit_and_apply`](Self::admit_and_apply) and report its
    /// error.
    ///
    /// # Panics
    /// When the window refuses the close for any other reason: its
    /// capabilities do not take it, or its window system failed it.
    fn close_now(&self) {
        if let Some(refusal) = close_refusal(self.admit_and_apply(WindowCommand::Close)) {
            panic!("the window did not close: {refusal}");
        }
    }

    /// Close this window from `Drop` or from cleanup: a refusal is logged and
    /// never panics, so a test already unwinding keeps its first failure.
    fn close_for_cleanup(&self) {
        if let Some(refusal) = close_refusal(self.admit_and_apply(WindowCommand::Close)) {
            tracing::warn!(%refusal, "a window the test cleans up did not close");
        }
    }
}

/// What a close answered, unless the window is closed: a window that had
/// closed already is closed.
fn close_refusal(outcome: Result<(), CommandRefusal>) -> Option<CommandRefusal> {
    match outcome {
        Ok(()) | Err(CommandRefusal::Apply(ApplyError::Closed)) => None,
        Err(refusal) => Some(refusal),
    }
}

impl<W: PlatformWindow + ?Sized> AdmitAndApply for W {
    fn admit_and_apply(&self, command: WindowCommand) -> Result<(), CommandRefusal> {
        command.admit_and_apply(self)
    }
}
