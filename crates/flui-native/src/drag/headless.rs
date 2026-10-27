//! The headless backend's OS drag session (ADR-0211): a double that plays the
//! operating system's side of a drag, so a test can run a whole drag between
//! windows, or a drag arriving from another application, without a display.
//!
//! The double is the platform's one [`DataTransferSource`]. A window it
//! serves ([`HeadlessPlatform::with_os_drag`](crate::HeadlessPlatform::with_os_drag))
//! starts a session through its [`PlatformDragSource`]; the test then plays the
//! pointer with [`HeadlessDragAndDrop::hover`], [`drop_here`](HeadlessDragAndDrop::drop_here)
//! and [`cancel`](HeadlessDragAndDrop::cancel). Each step reaches the window under
//! the pointer as a [`DragDropEvent`], the target's answer is cached as drop
//! feedback exactly as a native backend caches it, and the drop's action is
//! resolved from that cache.
//!
//! What the simulated system keeps of drag and drop (the offers it holds open
//! and the session in flight) is the loop's, as the rest of that system is
//! ([`OsDrags`]); the source, a window's drag source and the test's side all
//! play over it.

use std::sync::{Arc, Weak};

use flui_foundation::DataTransferId;
use flui_foundation::geometry::Point;
use flui_platform::data_transfer::{
    DataTransferOffer, DataTransferSource, DropFeedback, RepresentationDescriptor,
    RepresentationIndex, TransferActions, TransferError, TransferFormat, TransferLimits,
    TransferPayload, TransferRequest,
};
use flui_platform::drag::{
    DragError, OsDragEnd, OsDragProvider, OsDragRequest, OsDragStart, PlatformDragSource,
};
use flui_platform::{DragDropEvent, PlatformInput, Unsupported, UnsupportedReason, WindowId};
use parking_lot::Mutex;

use crate::drag::offers::OfferTable;
use crate::host::{self, Host};
use crate::owner::headless::{
    DragOver, HeadlessLoop, HeadlessState, OsDragSession, OsDrags, SimulatedOffers,
};
use crate::owner::slot::OwnerSlot;
use crate::platform::HostPlatform;

/// The simulated system mints its offers in the table every source does.
impl SimulatedOffers for OfferTable {
    fn mint(&mut self, representations: Arc<[RepresentationDescriptor]>) -> DataTransferId {
        OfferTable::mint(self, representations)
    }

    fn retire(&mut self, id: DataTransferId) {
        // A stale id is a no-op, as the system's every retirement may be.
        let _ = OfferTable::retire(self, id);
    }
}

/// The headless host's drag-and-drop transport: the platform's one
/// data-transfer source, over the drag state of the simulated system, which
/// the loop holds.
pub struct DragDouble {
    drags: Arc<Mutex<OsDrags>>,
}

impl std::fmt::Debug for DragDouble {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragDouble").finish_non_exhaustive()
    }
}

impl host::Drag<HeadlessLoop> for DragDouble {
    fn data_transfer(owner: &HeadlessLoop) -> Arc<dyn DataTransferSource> {
        // The headless backend's one source: it serves the drags the test plays
        // through `HeadlessDragAndDrop`; it has no clipboard transport yet.
        // Every handle reads the one state, so an id one minted stays
        // redeemable at every other.
        owner.with_state(|state| {
            Arc::new(Self {
                drags: Arc::clone(&state.drags),
            }) as Arc<dyn DataTransferSource>
        })
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
        let provider = self.drags.lock().open.get(&id).cloned();
        let Some(provider) = provider else {
            return TransferRequest::ready(Err(TransferError::StaleOffer(id)));
        };
        // The provider holds owner-thread state; off the owner thread, or
        // asked from inside itself, it is not there.
        let Some(mut serve) = provider.take() else {
            return TransferRequest::ready(Err(TransferError::SourceGone));
        };
        // A source's failure is contained as the OS contains it: the target
        // sees the transfer fail, and the provider stays for its next request.
        let served =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| serve(representation)));
        if let Err(displaced) = provider.restore(serve) {
            // The slot is the session's own, so nothing else fills it.
            std::mem::forget(displaced);
        }
        let result = served.unwrap_or_else(|payload| {
            std::mem::forget(payload);
            crate::owner::panic_boundary::contain_owner_callback(|| {
                tracing::error!("the drag's provider panicked; the request fails");
            });
            Err(TransferError::SourceGone)
        });
        // The last clone of `provider` may be this one, once the target
        // concluded the drop meanwhile: it retires here, outside every lock.
        drop(provider);
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
        let mut inner = self.drags.lock();
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
        let concluded = {
            let mut inner = self.drags.lock();
            inner.table.retire(id);
            inner.open.remove(&id)
        };
        // After the session ended this is the provider's last clone: its
        // captures are user code, so they retire with no lock held.
        drop(concluded);
    }
}

/// The drag source a headless window offers, once a test let the windows
/// start sessions ([`HeadlessPlatform::with_os_drag`](crate::HeadlessPlatform::with_os_drag)).
pub(crate) struct HeadlessDragSource {
    drags: Arc<Mutex<OsDrags>>,
    timing: OsDragStart,
}

impl HeadlessDragSource {
    /// The drag source of a window opened now in `state`'s system: none
    /// until a test let the windows start sessions.
    pub(crate) fn of(state: &HeadlessState) -> Option<Arc<dyn PlatformDragSource>> {
        let timing = state.os_drag_start?;
        Some(Arc::new(Self {
            drags: Arc::clone(&state.drags),
            timing,
        }))
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
        // The slots are filled, and dropped on a refusal, before the lock is
        // taken: their values are user code.
        let slot = Arc::new(OwnerSlot::new());
        let on_end_slot = OwnerSlot::new();
        if slot.replace(provider).is_err() || on_end_slot.replace(on_end).is_err() {
            return Err(Unsupported::new(UnsupportedReason::Unavailable).into());
        }
        let mut inner = self.drags.lock();
        if inner.session.is_some() {
            // One OS session at a time, as on a desktop.
            drop(inner);
            return Err(Unsupported::new(UnsupportedReason::Unavailable).into());
        }
        inner.session = Some(OsDragSession {
            representations: Arc::from(representations),
            allowed,
            provider: slot,
            on_end: Some(on_end_slot),
            over: None,
        });
        Ok(())
    }
}

/// The headless platform's drag and drop: whether its windows start OS drag
/// sessions, and the test's side of the simulated system.
impl<H: Host<Loop = HeadlessLoop>> HostPlatform<H> {
    /// Let this backend's windows start OS drag sessions, starting them at
    /// `timing` (desktop: `OnLeavingWindow`; iOS and the web: `AtStart`), and
    /// report outgoing drags `Native`. Without it its windows have no drag
    /// source, as a Linux/X11 window has none. The test plays the OS through
    /// [`drag_and_drop`](Self::drag_and_drop).
    #[must_use]
    pub fn with_os_drag(self, timing: OsDragStart) -> Self {
        self.owner().with_state(|state| {
            state.os_drag_start = Some(timing);
            state.input_availability = state
                .input_availability
                .to_builder()
                .outgoing_drag(flui_platform::Availability::Native)
                .build();
        });
        self
    }

    /// The simulated OS side of drag-and-drop, valid across [`Platform::run`](crate::Platform::run).
    #[must_use]
    pub fn drag_and_drop(&self) -> HeadlessDragAndDrop {
        HeadlessDragAndDrop::of(&self.owner().state)
    }
}

/// Plays the operating system's side of a drag on a [`HeadlessPlatform`](crate::HeadlessPlatform).
#[derive(Clone)]
pub struct HeadlessDragAndDrop {
    drags: Arc<Mutex<OsDrags>>,
    /// The system whose windows a step reaches.
    system: Weak<Mutex<HeadlessState>>,
}

impl std::fmt::Debug for HeadlessDragAndDrop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessDragAndDrop")
            .field("active", &self.is_active())
            .finish_non_exhaustive()
    }
}

impl HeadlessDragAndDrop {
    /// The OS side of the simulated system whose state is `system`.
    pub(crate) fn of(system: &Arc<Mutex<HeadlessState>>) -> Self {
        Self {
            drags: Arc::clone(&system.lock().drags),
            system: Arc::downgrade(system),
        }
    }

    /// Deliver `event` to `window`, when it is still open.
    fn deliver(&self, window: WindowId, event: DragDropEvent) {
        let system = self.system.upgrade();
        // The windows are the window feature's, which the loop holds without
        // naming them: their callbacks are what the drag reaches.
        let windows = system.map(|system| Arc::clone(&system.lock().windows));
        let callbacks = windows.and_then(|windows| windows.delivery_of(window));
        if let Some(callbacks) = callbacks {
            // No lock is held: the window's callback reenters the runtime.
            callbacks.dispatch_input(PlatformInput::DragDrop(event));
        }
    }

    /// Whether an OS drag session is running.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.drags.lock().session.is_some()
    }

    /// The representations the running session offers.
    #[must_use]
    pub fn offered(&self) -> Option<Vec<RepresentationDescriptor>> {
        self.drags
            .lock()
            .session
            .as_ref()
            .map(|session| session.representations.to_vec())
    }

    /// The actions the running session advertises to its targets.
    #[must_use]
    pub fn advertised(&self) -> Option<TransferActions> {
        self.drags
            .lock()
            .session
            .as_ref()
            .map(|session| session.allowed)
    }

    /// How many offers the OS still holds open: the one under the pointer, and
    /// every dropped one whose target has not concluded it yet.
    #[must_use]
    pub fn open_offers(&self) -> usize {
        self.drags.lock().open.len()
    }

    /// The answer the window under the pointer cached for the OS.
    #[must_use]
    pub fn cached_feedback(&self) -> Option<DropFeedback> {
        self.drags
            .lock()
            .session
            .as_ref()
            .and_then(|session| session.over.as_ref())
            .map(|over| over.feedback)
    }

    /// Start a drag that comes from another application, offering `payloads` in
    /// order and allowing `allowed`. Its windows see an offer with no FLUI identity.
    ///
    /// # Errors
    ///
    /// [`DragError::Unsupported`] (`Unavailable`) while another session runs: the OS runs one
    /// at a time.
    pub fn start_foreign(
        &self,
        payloads: Vec<(TransferFormat, TransferPayload)>,
        allowed: TransferActions,
    ) -> Result<(), DragError> {
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
        if slot.replace(served).is_err() {
            // The slot was made on this thread: only exhausted slot keys refuse it.
            return Err(Unsupported::new(UnsupportedReason::Unavailable).into());
        }
        let mut inner = self.drags.lock();
        if inner.session.is_some() {
            return Err(Unsupported::new(UnsupportedReason::Unavailable).into());
        }
        inner.session = Some(OsDragSession {
            representations: Arc::from(representations),
            allowed,
            provider: slot,
            on_end: None,
            over: None,
        });
        Ok(())
    }

    /// The simulated pointer is over `window` at `position`: the first call
    /// enters it, later ones move, and moving to another window leaves the last.
    ///
    /// # Errors
    ///
    /// [`DragError::SessionClosed`] when no session is running.
    pub fn hover(&self, window: WindowId, position: Point<f64>) -> Result<(), DragError> {
        let (left, event) = {
            let mut inner = self.drags.lock();
            let OsDrags {
                table,
                session,
                open,
            } = &mut *inner;
            let session = session.as_mut().ok_or(DragError::SessionClosed)?;
            let allowed = session.allowed;
            let mut left = None;
            let event =
                if let Some(over) = session.over.as_mut().filter(|over| over.window == window) {
                    over.position = position;
                    DragDropEvent::Moved {
                        id: over.offer,
                        allowed,
                        position,
                    }
                } else {
                    if let Some(previous) = session.over.take() {
                        table.retire(previous.offer);
                        // Not the provider's last clone: the session holds one.
                        open.remove(&previous.offer);
                        left = Some((previous.window, previous.offer));
                    }
                    let id = table.mint(Arc::clone(&session.representations));
                    open.insert(id, Arc::clone(&session.provider));
                    session.over = Some(DragOver {
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
                };
            (left, event)
        };
        if let Some((window, id)) = left {
            self.deliver(window, DragDropEvent::Exited { id });
        }
        self.deliver(window, event);
        Ok(())
    }

    /// The pointer is over no window of the platform.
    pub fn leave(&self) {
        let left = {
            let mut inner = self.drags.lock();
            let OsDrags {
                table,
                session,
                open,
            } = &mut *inner;
            let over = session.as_mut().and_then(|session| session.over.take());
            over.map(|over| {
                table.retire(over.offer);
                // Not the provider's last clone: the session holds one.
                open.remove(&over.offer);
                (over.window, over.offer)
            })
        };
        if let Some((window, id)) = left {
            self.deliver(window, DragDropEvent::Exited { id });
        }
    }

    /// The user releases the pointer. Over a window, the action is resolved
    /// from the answer that window cached, as a native backend resolves it:
    /// one action the source permits ([`DropFeedback::resolve`]). Over no
    /// window, or when the window accepted nothing, the drag is cancelled
    /// instead. Returns the action, `None` when cancelled.
    ///
    /// # Errors
    ///
    /// [`DragError::SessionClosed`] when no session is running.
    pub fn drop_here(&self) -> Result<Option<TransferActions>, DragError> {
        let over = {
            let inner = self.drags.lock();
            let session = inner.session.as_ref().ok_or(DragError::SessionClosed)?;
            session.over.as_ref().and_then(|over| {
                let action = over.feedback.resolve(session.allowed)?;
                Some((over.window, over.offer, action, over.position))
            })
        };
        let Some((window, id, action, position)) = over else {
            self.cancel();
            return Ok(None);
        };
        self.deliver(
            window,
            DragDropEvent::Dropped {
                id,
                action,
                position: Some(position),
            },
        );
        self.finish(OsDragEnd::Dropped(action));
        Ok(Some(action))
    }

    /// The user cancels (Escape): the window under the pointer is left and the
    /// source is told.
    pub fn cancel(&self) {
        self.leave();
        self.finish(OsDragEnd::Cancelled);
    }

    fn finish(&self, end: OsDragEnd) {
        let session = self.drags.lock().session.take();
        let Some(OsDragSession { on_end, .. }) = session else {
            return;
        };
        if let Some(hook) = on_end.and_then(|slot| slot.take()) {
            hook(end);
        }
    }
}
