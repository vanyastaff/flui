//! Driving a window from a test with no command queue in front of it.
//!
//! A window takes only an [`AdmittedCommand`](flui_platform::AdmittedCommand)
//! (ADR-0254 §2); the application's queue admits each command against the
//! capabilities of the window's snapshot when it is submitted.
//! [`AdmitAndApply`] does the same at once, so a test of a window, a host or
//! a conformance check reads as the command it sends.

use flui_platform::{AdmissionError, ApplyError, PlatformWindow, WindowCommand};

/// Why a command a test sent did not take effect: its admission refused it,
/// or the window did.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandRefusal {
    /// The window's capabilities refuse the command, or its numbers describe
    /// no window.
    #[error(transparent)]
    Admission(#[from] AdmissionError),
    /// The window closed, its capabilities moved, or its window system
    /// failed the command.
    #[error(transparent)]
    Apply(#[from] ApplyError),
}

/// Admit a command against the capabilities of a window's snapshot now, and
/// apply it.
pub trait AdmitAndApply {
    /// Admit `command` against [`state`](PlatformWindow::state)'s
    /// capabilities and apply it to this window.
    ///
    /// # Errors
    /// [`CommandRefusal::Admission`] when admission refuses it,
    /// [`CommandRefusal::Apply`] when the window does.
    fn admit_and_apply(&self, command: WindowCommand) -> Result<(), CommandRefusal>;

    /// Close this window with [`WindowCommand::Close`], as a test's teardown
    /// does: a window that has closed already is closed.
    ///
    /// # Panics
    /// When the window refuses the close for any other reason: its
    /// capabilities do not take it, or its window system failed it.
    fn close_now(&self) {
        match self.admit_and_apply(WindowCommand::Close) {
            Ok(()) | Err(CommandRefusal::Apply(ApplyError::Closed)) => {}
            Err(refusal) => panic!("the window did not close: {refusal}"),
        }
    }
}

impl<W: PlatformWindow + ?Sized> AdmitAndApply for W {
    fn admit_and_apply(&self, command: WindowCommand) -> Result<(), CommandRefusal> {
        let admitted = command.admit(&self.state().capabilities)?;
        Ok(self.apply(admitted)?)
    }
}
