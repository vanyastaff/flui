//! The headless backend's OS drag session (ADR-0211): a double that plays the
//! operating system's side of a drag, so a test can run a whole drag between
//! windows, or a drag arriving from another application, without a display.
//!
//! The double is the platform's one [`DataTransferSource`]. A window it
//! serves ([`HeadlessPlatform::with_os_drag`](super::HeadlessPlatform::with_os_drag))
//! starts a session through its [`PlatformDragSource`]; the test then plays the
//! pointer with [`HeadlessDragAndDrop::hover`], [`drop_here`](HeadlessDragAndDrop::drop_here)
//! and [`cancel`](HeadlessDragAndDrop::cancel). Each step reaches the window under
//! the pointer as a [`DragDropEvent`], the target's answer is cached as drop
//! feedback exactly as a native backend caches it, and the drop's action is
//! resolved from that cache.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use flui_foundation::DataTransferId;
use flui_foundation::geometry::Point;
use flui_platform_api::data_transfer::{
    DataTransferOffer, DataTransferSource, DropFeedback, OfferRecord, OfferTable,
    RepresentationDescriptor, RepresentationIndex, TransferActions, TransferError, TransferFormat,
    TransferLimits, TransferPayload, TransferRequest,
};
use flui_platform_api::drag::{
    DragError, OsDragEnd, OsDragEndHook, OsDragProvider, OsDragRequest, OsDragStart,
    PlatformDragSource,
};
use flui_platform_api::{DragDropEvent, PlatformInput, PlatformWindow, WindowId};
use parking_lot::Mutex;

use super::platform::HeadlessState;
use crate::shared::owner_slot::OwnerSlot;

/// One OS drag session in flight.
struct Session {
    representations: Arc<[RepresentationDescriptor]>,
    allowed: TransferActions,
    provider: Arc<OwnerSlot<OsDragProvider>>,
    /// Told once how the session ended; absent for a drag from another application.
    on_end: Option<OwnerSlot<OsDragEndHook>>,
    over: Option<Over>,
}

/// The window the simulated pointer is over.
struct Over {
    window: WindowId,
    offer: DataTransferId,
    feedback: DropFeedback,
    position: Point<f64>,
}

#[derive(Default)]
struct Inner {
    table: OfferTable,
    session: Option<Session>,
    /// The providers behind offers still open at the OS, by id, so a target
    /// may fetch after the drop until it concludes it.
    open: HashMap<DataTransferId, Arc<OwnerSlot<OsDragProvider>>>,
}

pub(crate) struct DragDouble {
    inner: Mutex<Inner>,
    windows: Mutex<Weak<Mutex<HeadlessState>>>,
    start: Mutex<Option<OsDragStart>>,
}

impl DragDouble {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            windows: Mutex::new(Weak::new()),
            start: Mutex::new(None),
        }
    }

    pub(super) fn attach(&self, platform: Weak<Mutex<HeadlessState>>) {
        *self.windows.lock() = platform;
    }

    pub(crate) fn enable(&self, timing: OsDragStart) {
        *self.start.lock() = Some(timing);
    }

    pub(crate) fn timing(&self) -> Option<OsDragStart> {
        *self.start.lock()
    }

    fn deliver(&self, window: WindowId, event: DragDropEvent) {
        let platform = self.windows.lock().upgrade();
        let window = platform.and_then(|platform| {
            platform
                .lock()
                .windows
                .iter()
                .find(|candidate| PlatformWindow::id(*candidate) == window)
                .cloned()
        });
        if let Some(window) = window {
            // No lock is held: the window's callback reenters the runtime.
            window.inject_event(PlatformInput::DragDrop(event));
        }
    }
}

impl DataTransferSource for DragDouble {
    fn clipboard_offer(&self) -> Option<DataTransferOffer> {
        None
    }

    fn request(
        &self,
        id: DataTransferId,
        representation: RepresentationIndex,
        limits: TransferLimits,
    ) -> TransferRequest {
        let provider = self.inner.lock().open.get(&id).cloned();
        let Some(provider) = provider else {
            return TransferRequest::ready(Err(TransferError::StaleOffer(id)));
        };
        // The provider holds owner-thread state; off the owner thread it is not there.
        let Some(mut serve) = provider.take() else {
            return TransferRequest::ready(Err(TransferError::SourceGone));
        };
        let result = serve(representation);
        let _ = provider.restore(serve);
        let result = result.and_then(|payload| {
            let actual = payload.byte_len();
            if actual > limits.max_bytes {
                Err(TransferError::TooLarge {
                    actual,
                    limit: limits.max_bytes,
                })
            } else {
                Ok(payload)
            }
        });
        TransferRequest::ready(result)
    }

    fn update_drop_feedback(&self, id: DataTransferId, feedback: DropFeedback) {
        let mut inner = self.inner.lock();
        if let Some(over) = inner
            .session
            .as_mut()
            .and_then(|session| session.over.as_mut())
            .filter(|over| over.offer == id)
        {
            over.feedback = feedback;
        }
    }

    fn conclude_drop(&self, id: DataTransferId) {
        let mut inner = self.inner.lock();
        inner.open.remove(&id);
        let _ = inner.table.retire(id);
    }
}

/// The drag source a headless window offers.
pub(crate) struct HeadlessDragSource {
    double: Arc<DragDouble>,
    timing: OsDragStart,
}

impl HeadlessDragSource {
    pub(crate) fn new(double: Arc<DragDouble>, timing: OsDragStart) -> Self {
        Self { double, timing }
    }
}

impl PlatformDragSource for HeadlessDragSource {
    fn start_timing(&self) -> OsDragStart {
        self.timing
    }

    fn start(&self, request: OsDragRequest) -> Result<(), DragError> {
        let OsDragRequest {
            representations,
            allowed,
            provider,
            on_end,
            ..
        } = request;
        let mut inner = self.double.inner.lock();
        if inner.session.is_some() {
            // One OS session at a time, as on a desktop.
            return Err(DragError::Unavailable);
        }
        let slot = Arc::new(OwnerSlot::new());
        let on_end_slot = OwnerSlot::new();
        if slot.replace(provider).is_err() || on_end_slot.replace(on_end).is_err() {
            return Err(DragError::Unavailable);
        }
        inner.session = Some(Session {
            representations: Arc::from(representations),
            allowed,
            provider: slot,
            on_end: Some(on_end_slot),
            over: None,
        });
        Ok(())
    }
}

/// Plays the operating system's side of a drag on a [`HeadlessPlatform`](super::HeadlessPlatform).
#[derive(Clone)]
pub struct HeadlessDragAndDrop {
    double: Arc<DragDouble>,
}

impl std::fmt::Debug for HeadlessDragAndDrop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessDragAndDrop")
            .field("active", &self.is_active())
            .finish_non_exhaustive()
    }
}

impl HeadlessDragAndDrop {
    pub(crate) fn new(double: Arc<DragDouble>) -> Self {
        Self { double }
    }

    /// Whether an OS drag session is running.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.double.inner.lock().session.is_some()
    }

    /// The representations the running session offers.
    #[must_use]
    pub fn offered(&self) -> Option<Vec<RepresentationDescriptor>> {
        self.double
            .inner
            .lock()
            .session
            .as_ref()
            .map(|session| session.representations.to_vec())
    }

    /// The actions the running session advertises to its targets.
    #[must_use]
    pub fn advertised(&self) -> Option<TransferActions> {
        self.double
            .inner
            .lock()
            .session
            .as_ref()
            .map(|session| session.allowed)
    }

    /// How many offers the OS still holds open: the one under the pointer, and
    /// every dropped one whose target has not concluded it yet.
    #[must_use]
    pub fn open_offers(&self) -> usize {
        self.double.inner.lock().open.len()
    }

    /// The answer the window under the pointer cached for the OS.
    #[must_use]
    pub fn cached_feedback(&self) -> Option<DropFeedback> {
        self.double
            .inner
            .lock()
            .session
            .as_ref()
            .and_then(|session| session.over.as_ref())
            .map(|over| over.feedback)
    }

    /// Start a drag that comes from another application, offering `payloads` in
    /// order and allowing `allowed`. Its windows see an offer with no FLUI identity.
    ///
    /// # Panics
    ///
    /// When a session is already running.
    pub fn start_foreign(
        &self,
        payloads: Vec<(TransferFormat, TransferPayload)>,
        allowed: TransferActions,
    ) {
        let representations: Vec<RepresentationDescriptor> = payloads
            .iter()
            .map(|(format, payload)| RepresentationDescriptor {
                format: format.clone(),
                declared_len: Some(payload.byte_len()),
            })
            .collect();
        let slot = Arc::new(OwnerSlot::new());
        let payloads: Vec<TransferPayload> =
            payloads.into_iter().map(|(_, payload)| payload).collect();
        let served: OsDragProvider = Box::new(move |RepresentationIndex(index)| {
            payloads
                .get(usize::from(index))
                .cloned()
                .ok_or(TransferError::Cancelled)
        });
        assert!(
            slot.replace(served).is_ok(),
            "BUG: the double is driven from its owner thread"
        );
        let mut inner = self.double.inner.lock();
        assert!(
            inner.session.is_none(),
            "an OS drag session is already running"
        );
        inner.session = Some(Session {
            representations: Arc::from(representations),
            allowed,
            provider: slot,
            on_end: None,
            over: None,
        });
    }

    /// The simulated pointer is over `window` at `position`: the first call
    /// enters it, later ones move, and moving to another window leaves the last.
    ///
    /// # Panics
    ///
    /// When no session is running.
    pub fn hover(&self, window: WindowId, position: Point<f64>) {
        let (left, event) = {
            let mut inner = self.double.inner.lock();
            let Inner {
                table,
                session,
                open,
            } = &mut *inner;
            let session = session.as_mut().expect("an OS drag session is running");
            let allowed = session.allowed;
            let mut left = None;
            let same = session
                .over
                .as_ref()
                .is_some_and(|over| over.window == window);
            let event = if same {
                let over = session.over.as_mut().expect("checked just above");
                over.position = position;
                DragDropEvent::Moved {
                    id: over.offer,
                    allowed,
                    position,
                }
            } else {
                {
                    if let Some(previous) = session.over.take() {
                        let _ = table.retire(previous.offer);
                        open.remove(&previous.offer);
                        left = Some((previous.window, previous.offer));
                    }
                    let id = table.mint(OfferRecord::new(Arc::clone(&session.representations)));
                    open.insert(id, Arc::clone(&session.provider));
                    session.over = Some(Over {
                        window,
                        offer: id,
                        feedback: DropFeedback::default(),
                        position,
                    });
                    DragDropEvent::Entered {
                        offer: DataTransferOffer::new(id, Arc::clone(&session.representations)),
                        allowed,
                        position: Some(position),
                    }
                }
            };
            (left, event)
        };
        if let Some((window, id)) = left {
            self.double.deliver(window, DragDropEvent::Exited { id });
        }
        self.double.deliver(window, event);
    }

    /// The pointer is over no window of the platform.
    pub fn leave(&self) {
        let left = {
            let mut inner = self.double.inner.lock();
            let Inner {
                table,
                session,
                open,
            } = &mut *inner;
            let over = session.as_mut().and_then(|session| session.over.take());
            over.map(|over| {
                let _ = table.retire(over.offer);
                open.remove(&over.offer);
                (over.window, over.offer)
            })
        };
        if let Some((window, id)) = left {
            self.double.deliver(window, DragDropEvent::Exited { id });
        }
    }

    /// The user releases over the window under the pointer. The action is
    /// resolved from the answer that window cached, as a native backend does;
    /// when it accepted nothing the drag is cancelled instead. Returns the action.
    ///
    /// # Panics
    ///
    /// When no session is running.
    pub fn drop_here(&self) -> Option<TransferActions> {
        let (window, id, action, position) = {
            let inner = self.double.inner.lock();
            let session = inner
                .session
                .as_ref()
                .expect("an OS drag session is running");
            let over = session.over.as_ref()?;
            let action = over.feedback.accept.map(|accept| accept & session.allowed);
            (
                over.window,
                over.offer,
                action.filter(|action| !action.is_empty()),
                over.position,
            )
        };
        let Some(action) = action else {
            self.cancel();
            return None;
        };
        self.double.deliver(
            window,
            DragDropEvent::Dropped {
                id,
                action,
                position: Some(position),
            },
        );
        self.finish(OsDragEnd::Dropped(action));
        Some(action)
    }

    /// The user cancels (Escape): the window under the pointer is left and the
    /// source is told.
    pub fn cancel(&self) {
        self.leave();
        self.finish(OsDragEnd::Cancelled);
    }

    fn finish(&self, end: OsDragEnd) {
        let session = self.double.inner.lock().session.take();
        let Some(Session { on_end, .. }) = session else {
            return;
        };
        if let Some(hook) = on_end.and_then(|slot| slot.take()) {
            hook(end);
        }
    }
}
