//! Exact-contact logical routing capture (ADR-0164).

use std::{
    cell::Cell,
    fmt,
    rc::{Rc, Weak},
    sync::Weak as SharedWeak,
};

use flui_platform_api::{PlatformWindow, pointer::PointerInfo};

use super::PointerTarget;

/// Why a pointer dispatch could not acquire explicit capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PointerCaptureError {
    /// Only a Down callback may acquire capture.
    #[error("pointer capture requires a Down dispatch")]
    NotDown,
    /// This dispatch carries no admitted contact route.
    #[error("pointer dispatch has no capture authority")]
    Unavailable,
    /// An earlier target already claimed this Down transaction.
    #[error("pointer capture has already been claimed")]
    AlreadyClaimed,
    /// This contact ended or was replaced by a newer Down.
    #[error("pointer capture contact is stale")]
    StaleSequence,
    /// The presentation that owned the contact has closed.
    #[error("pointer capture owner is closed")]
    ClosedOwner,
}

#[derive(Clone, Copy)]
enum CaptureStatus {
    Unclaimed,
    Claimed(PointerTarget),
    Released(PointerTarget),
    Ended(Option<PointerTarget>),
    Closed(Option<PointerTarget>),
}

/// One binding-admitted generation. All fields are framework-owned values;
/// releasing a token never retires a handler or invokes an event callback.
pub(crate) struct ContactCapture {
    pointer: PointerInfo,
    sequence: u64,
    status: Cell<CaptureStatus>,
    wake: Option<SharedWeak<dyn PlatformWindow>>,
}

impl ContactCapture {
    pub(crate) fn new(
        pointer: PointerInfo,
        sequence: u64,
        wake: Option<SharedWeak<dyn PlatformWindow>>,
    ) -> Rc<Self> {
        Rc::new(Self {
            pointer,
            sequence,
            status: Cell::new(CaptureStatus::Unclaimed),
            wake,
        })
    }

    pub(crate) fn target(&self) -> Option<PointerTarget> {
        match self.status.get() {
            CaptureStatus::Unclaimed => None,
            CaptureStatus::Claimed(target) | CaptureStatus::Released(target) => Some(target),
            CaptureStatus::Ended(target) | CaptureStatus::Closed(target) => target,
        }
    }

    pub(crate) fn release_requested(&self) -> bool {
        matches!(self.status.get(), CaptureStatus::Released(_))
    }

    pub(crate) fn end(&self) {
        self.status.set(CaptureStatus::Ended(self.target()));
    }

    pub(crate) fn close(&self) {
        self.status.set(CaptureStatus::Closed(self.target()));
    }

    fn claim(
        self: &Rc<Self>,
        pointer: PointerInfo,
        target: PointerTarget,
    ) -> Result<PointerCapture, PointerCaptureError> {
        if pointer != self.pointer {
            return Err(PointerCaptureError::StaleSequence);
        }
        match self.status.get() {
            CaptureStatus::Unclaimed => {
                self.status.set(CaptureStatus::Claimed(target));
                Ok(PointerCapture {
                    contact: Rc::downgrade(self),
                    pointer,
                    sequence: self.sequence,
                    target,
                })
            }
            CaptureStatus::Claimed(_) | CaptureStatus::Released(_) => {
                Err(PointerCaptureError::AlreadyClaimed)
            }
            CaptureStatus::Ended(_) => Err(PointerCaptureError::StaleSequence),
            CaptureStatus::Closed(_) => Err(PointerCaptureError::ClosedOwner),
        }
    }

    fn release(&self, pointer: PointerInfo, sequence: u64, target: PointerTarget) {
        if pointer != self.pointer
            || sequence != self.sequence
            || !matches!(self.status.get(), CaptureStatus::Claimed(current) if current == target)
        {
            return;
        }
        // Debt is committed before platform code can reenter or fail. There
        // is no contact-map borrow and no handler ownership in this state.
        self.status.set(CaptureStatus::Released(target));
        let Some(window) = self.wake.as_ref().and_then(SharedWeak::upgrade) else {
            return;
        };
        let mut failure = crate::__runtime::ClosePanic::new();
        failure.invoke(|| window.request_redraw());
        failure.retire(window);
        failure.finish();
    }
}

#[derive(Clone, Copy)]
pub(crate) struct CaptureRequest<'a> {
    pub(crate) contact: &'a Rc<ContactCapture>,
    pub(crate) target: PointerTarget,
}

impl CaptureRequest<'_> {
    pub(crate) fn claim(self, pointer: PointerInfo) -> Result<PointerCapture, PointerCaptureError> {
        self.contact.claim(pointer, self.target)
    }
}

impl fmt::Debug for CaptureRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CaptureRequest").finish_non_exhaustive()
    }
}

/// Exclusive logical delivery to the target that claimed a contact's Down.
///
/// Retain the token for as long as that target should receive the contact.
/// Dropping it, or calling [`Self::release`], commits one
/// `Cancel(CaptureLost)` for delivery on the owner's next input entry or
/// frame. Already accepted motion is delivered before that cancellation;
/// release invokes no event callbacks inline. Native Up/Cancel, replacement
/// Down, and owner close make an older token inert.
///
/// The token is owner-affine and weak: it cannot keep its presentation alive.
/// This controls framework routing; existing native automatic capture is
/// managed by the platform backend.
#[must_use = "retain the capture token until the contact should be released"]
pub struct PointerCapture {
    contact: Weak<ContactCapture>,
    pointer: PointerInfo,
    sequence: u64,
    target: PointerTarget,
}

impl PointerCapture {
    /// Release this capture through the same deferred cancellation as Drop.
    pub fn release(self) {
        drop(self);
    }
}

impl fmt::Debug for PointerCapture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PointerCapture").finish_non_exhaustive()
    }
}

impl Drop for PointerCapture {
    fn drop(&mut self) {
        if let Some(contact) = self.contact.upgrade() {
            contact.release(self.pointer, self.sequence, self.target);
        }
    }
}
