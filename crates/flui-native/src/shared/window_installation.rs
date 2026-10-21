//! One-shot acknowledgement before a backend publishes a native attachment.

use std::cell::Cell;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use crate::{PlatformProxy, ProxySendError};

/// Completion authority for one native attachment. Dropping it without
/// completing aborts that installation. It does not authorize another attachment.
#[derive(Debug)]
#[must_use = "complete the installation or drop it to abort native publication"]
pub struct WindowInstallation {
    sender: Option<Sender<()>>,
    wake: PlatformProxy,
}

/// Backend-owned observation of an installation acknowledgement.
#[derive(Debug)]
pub struct PendingWindowInstallation {
    receiver: Receiver<()>,
    result: Cell<Option<Result<(), InstallationCancelled>>>,
}

/// The installer released its completion authority before acknowledging readiness.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("window installation was cancelled")]
pub struct InstallationCancelled;

impl WindowInstallation {
    /// Create a unique completion authority and its backend observation.
    /// The proxy wakes the backend's owner after the outcome has been committed.
    pub fn channel(wake: PlatformProxy) -> (Self, PendingWindowInstallation) {
        let (sender, receiver) = channel();
        (
            Self {
                sender: Some(sender),
                wake,
            },
            PendingWindowInstallation {
                receiver,
                result: Cell::new(None),
            },
        )
    }

    /// Acknowledge readiness and notify the native owner. An attachment already
    /// retired by the backend ignores this late acknowledgement.
    ///
    /// # Errors
    /// A failed native wake is returned after readiness is committed. A later
    /// owner opportunity can still observe it; it is never changed to cancellation.
    pub fn complete(mut self) -> Result<(), ProxySendError<()>> {
        let sender = self
            .sender
            .take()
            .expect("BUG: installation completes once");
        if sender.send(()).is_err() {
            return Ok(());
        }
        self.wake.wake()
    }
}

impl Drop for WindowInstallation {
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take() {
            drop(sender);
            // Drop can run during another failure. The committed cancellation
            // remains observable even if the platform cannot post this wake.
            super::panic_boundary::contain_owner_callback(|| {
                let _ = self.wake.wake();
            });
        }
    }
}

impl PendingWindowInstallation {
    /// Read the committed result without waiting or consuming it. `None` means
    /// the native attachment must remain unpublished.
    pub fn outcome(&self) -> Option<Result<(), InstallationCancelled>> {
        if let Some(result) = self.result.get() {
            return Some(result);
        }
        let result = match self.receiver.try_recv() {
            Ok(()) => Ok(()),
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err(InstallationCancelled),
        };
        self.result.set(Some(result));
        Some(result)
    }
}
