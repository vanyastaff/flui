//! Closed cross-thread application commands. UI factories never enter this state.
mod membership;
mod thread_retirement;
use super::owner_document::{
    DocumentData, DocumentRefusal, DocumentRequest, DocumentSendError, Edit, Envelope,
    SharedDocument,
};
use flui_foundation::{PresentationAddress, claim_slot};
use flui_native::PlatformProxy;
use membership::Membership;
pub(crate) use membership::OwnerId;
pub(crate) use membership::QuitTicket;
use parking_lot::Mutex;
use std::sync::{
    Arc, Weak,
    atomic::{AtomicU64, Ordering},
};
pub(crate) use thread_retirement::ThreadRetirement;

/// Failure to admit an application command.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum AppControlError {
    /// A never-reused request identity cannot be issued.
    #[error("application request identity space is exhausted")]
    IdentityExhausted,
    /// The application has committed to shutdown.
    #[error("application owner is closed")]
    OwnerGone,
    /// Too many outstanding result receivers.
    #[error("main-window request capacity {capacity} is exhausted")]
    Capacity {
        /// Maximum simultaneous result receivers.
        capacity: usize,
    },
    /// The operating system rejected the owner notification.
    #[error("application wake failed: {message}")]
    WakeFailed {
        /// Operating-system notification diagnostic.
        message: String,
    },
}

pub(crate) use super::window_request::{AppWindowError, Reply, WindowRequest};
const WAITER_CAPACITY: usize = 64;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Accepting,
    Quitting,
    Closed,
}
/// What the presentations answered a quit the application asked for.
pub(crate) enum QuitAnswer {
    /// A refusal recorded nothing to wait on: the quit is cancelled.
    Cancelled,
    /// The presentations whose recorded close the quit waits on; none, and
    /// the quit goes ahead.
    Waiting(Vec<PresentationAddress>),
}
struct State {
    documents: Vec<Weak<DocumentData>>,
    document_ids: flui_foundation::IdMint<std::num::NonZeroU64>,
    limits: super::OwnerLimits,
    launches: usize,
    owner_failure: Option<AppWindowError>,
    membership_changed: bool,
    phase: Phase,
    membership: Membership,
    intent: bool,
    active: bool,
    batch: Option<Arc<()>>,
    superseded: bool,
    closing: Option<PresentationAddress>,
    open_address: Option<PresentationAddress>,
    active_replies: Vec<MainReply>,
    outstanding: usize,
    main_receipts: usize,
    replies: Vec<MainReply>,
}

struct MainReply {
    reply: Reply,
    budget: Arc<ReceiptBudget>,
}

/// Thread-safe application control. It never retains a widget tree or UI factory.
#[derive(Clone)]
pub struct AppHandle {
    pub(crate) ingress: Arc<Ingress>,
}
impl std::fmt::Debug for AppHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppHandle").finish_non_exhaustive()
    }
}
impl AppHandle {
    /// Create an explicitly versioned document service in this application.
    /// `max_bytes` bounds every value and reserves space for read/conflict replies.
    /// Capture the returned handle in factories and bind a port on each owner.
    pub fn create_document(
        &self,
        bytes: Vec<u8>,
        max_bytes: std::num::NonZeroUsize,
    ) -> Result<SharedDocument, DocumentSendError> {
        let mut state = self.ingress.state.lock();
        let refusal = if state.phase != Phase::Accepting {
            Some(DocumentRefusal::OwnerGone)
        } else if bytes.capacity() > max_bytes.get()
            || max_bytes.get() > state.limits.payload_bytes.get()
        {
            Some(DocumentRefusal::Oversize)
        } else {
            None
        };
        if let Some(reason) = refusal {
            return Err(DocumentSendError {
                reason,
                rejected: Some(bytes),
            });
        }
        state
            .documents
            .retain(|document| document.strong_count() != 0);
        if state.documents.len() >= state.limits.commands.get() {
            return Err(DocumentSendError {
                reason: DocumentRefusal::Full,
                rejected: Some(bytes),
            });
        }
        let Ok(id) = state.document_ids.next() else {
            return Err(DocumentSendError {
                reason: DocumentRefusal::IdentityExhausted,
                rejected: Some(bytes),
            });
        };
        let data = Arc::new(DocumentData::new(id, bytes, max_bytes.get()));
        state.documents.push(Arc::downgrade(&data));
        Ok(SharedDocument {
            data,
            run: Arc::downgrade(&self.ingress),
        })
    }
    /// Admit a coalesced show request. Dropping its receiver does not cancel intent.
    pub fn request_show_main_window(&self) -> Result<WindowRequest, AppControlError> {
        self.ingress.request_show()
    }
    /// Ask to quit the application.
    ///
    /// The quit is a close every presentation may refuse, as
    /// [`CloseReason::Program`](crate::app::CloseReason::Program). On the
    /// owner's next turn each one is asked: its close guard first, then its
    /// close-request handler. When none refuses, new work is fenced and the
    /// application shuts down as it always did. When some refuse, each
    /// records the close on its guard and the quit is deferred: it goes ahead
    /// once every record is carried out (the holds released, or
    /// `PendingClose::discard_and_close`), without asking again, and is
    /// cancelled when one is resolved with `PendingClose::stay_open`. A
    /// refusal that records nothing, a handler that panicked or a
    /// presentation without a guard, cancels the quit at once. A second call
    /// while the quit is deferred asks every presentation again.
    ///
    /// # Errors
    ///
    /// [`AppControlError::OwnerGone`] once the application has shut down,
    /// and [`AppControlError::WakeFailed`] when the owner could not be woken
    /// to ask.
    pub fn request_quit(&self) -> Result<(), AppControlError> {
        let mut state = self.ingress.state.lock();
        match state.phase {
            Phase::Closed => return Err(AppControlError::OwnerGone),
            Phase::Quitting => return Ok(()),
            Phase::Accepting => {}
        }
        state
            .membership
            .request_quit()
            .map_err(|_| AppControlError::IdentityExhausted)?;
        let proxies = state.membership.proxies();
        drop(state);
        // Admission precedes notification. A failed post cannot erase the
        // reserved consultation; the next owner turn still observes it.
        Ingress::notify(proxies, false)
    }
    /// Ask every window whose GPU device rebuild was given up to try again
    /// (ADR-0267).
    ///
    /// A device that keeps failing to rebuild is given up after ten failures
    /// at the one-second retry ceiling, about twelve seconds after the loss:
    /// the window reports
    /// [`DeviceRecovery::Abandoned`](crate::DeviceRecovery::Abandoned) through
    /// its [`FrameFailureHandler`](crate::FrameFailureHandler), attempts no
    /// more frames and stays as it was until this call, typically after the
    /// user installed a driver or reconnected a GPU. The rebuild then starts
    /// over at once, with a fresh streak. A window whose device is healthy or
    /// still being rebuilt ignores the request.
    ///
    /// Every rendering window of the application this handle controls follows
    /// the request.
    pub fn retry_render_device(&self) -> Result<(), AppControlError> {
        if self.ingress.state.lock().phase == Phase::Closed {
            return Err(AppControlError::OwnerGone);
        }
        self.ingress.device_retry.request()?;
        Ok(())
    }
}

/// The application's requests to rebuild a given-up GPU device (ADR-0267),
/// shared by [`AppHandle`] and the windows that render.
#[derive(Default)]
pub(crate) struct DeviceRetry {
    /// Requests so far; a window compares it with the count it last acted on.
    requests: AtomicU64,
    /// Each rendering window's frame wake, held weakly so a closed window's
    /// UI runtime is not kept alive.
    wakes: Mutex<Vec<Weak<dyn Fn() + Send + Sync>>>,
}
impl DeviceRetry {
    /// Follow the requests on behalf of a window that `wake` asks a frame of.
    pub(crate) fn watch(self: &Arc<Self>, wake: &Arc<dyn Fn() + Send + Sync>) -> DeviceRetryWatch {
        let mut wakes = self.wakes.lock();
        wakes.retain(|wake| wake.strong_count() > 0);
        wakes.push(Arc::downgrade(wake));
        drop(wakes);
        DeviceRetryWatch {
            seen: self.requests.load(Ordering::Acquire),
            retry: Arc::clone(self),
        }
    }
    /// Count a request and ask every live window for a frame, which acts on
    /// it. The wakes run without the list's lock held.
    fn request(&self) -> Result<(), AppControlError> {
        self.requests
            .try_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map_err(|_| AppControlError::IdentityExhausted)?;
        let wakes: Vec<_> = self.wakes.lock().iter().filter_map(Weak::upgrade).collect();
        let mut panics = flui_foundation::panic::FirstPanic::new();
        for wake in wakes {
            panics.contain(|| wake());
        }
        panics.resume();
        Ok(())
    }
}

/// One window's view of [`DeviceRetry`]: whether a request arrived since its
/// last frame.
pub(crate) struct DeviceRetryWatch {
    retry: Arc<DeviceRetry>,
    seen: u64,
}
impl DeviceRetryWatch {
    /// Whether the application asked for a rebuild since the last call.
    pub(crate) fn take_request(&mut self) -> bool {
        let requests = self.retry.requests.load(Ordering::Acquire);
        let requested = requests != self.seen;
        self.seen = requests;
        requested
    }
}
/// Retained until both installation and receipt observation/abandonment finish.
#[derive(Debug)]
pub(crate) enum ReceiptBudget {
    Launch(Weak<Ingress>),
    Main(Weak<Ingress>),
}
impl Drop for ReceiptBudget {
    fn drop(&mut self) {
        let run = match self {
            Self::Launch(run) | Self::Main(run) => run.upgrade(),
        };
        if let Some(run) = run {
            let mut state = run.state.lock();
            match self {
                Self::Launch(_) => {
                    state.launches = state
                        .launches
                        .checked_sub(1)
                        .expect("BUG: launch receipt released once");
                }
                Self::Main(_) => {
                    state.main_receipts = state
                        .main_receipts
                        .checked_sub(1)
                        .expect("BUG: main receipt released once");
                    state.membership.release_budget(OwnerId::INITIAL, 0);
                }
            }
        }
    }
}

pub(crate) struct Ingress {
    state: Mutex<State>,
    proxy: PlatformProxy,
    device_retry: Arc<DeviceRetry>,
    threads: Mutex<Vec<std::thread::JoinHandle<()>>>,
    services: Mutex<Option<Arc<super::runtime::RunServices>>>,
}

/// A stamped route to one owner in one application run. Cloning this carries
/// control authority only; no UI/native object or callback crosses the route.
#[derive(Clone)]
pub(crate) struct OwnerRoute {
    pub(crate) ingress: Arc<Ingress>,
    id: OwnerId,
}

impl OwnerRoute {
    pub(crate) fn submit_document(
        &self,
        data: Arc<DocumentData>,
        edit: Edit,
    ) -> Result<DocumentRequest, DocumentSendError> {
        let bytes = match edit.retained_bytes(data.max_bytes) {
            Ok(bytes) => bytes,
            Err(reason) => return Err(edit.rejected(reason)),
        };
        let mut state = self.ingress.state.lock();
        if state.phase != Phase::Accepting {
            return Err(edit.rejected(DocumentRefusal::OwnerGone));
        }
        let limits = state.limits;
        let sequence = match state.membership.reserve_command(self.id, bytes, limits) {
            Ok(sequence) => sequence,
            Err(reason) => return Err(edit.rejected(reason)),
        };
        let (command, request) = Envelope::new(
            sequence,
            data,
            edit,
            Arc::downgrade(&self.ingress),
            self.id,
            bytes,
        );
        state.membership.enqueue(self.id, command);
        drop(state);
        self.ingress.wake_owner();
        Ok(request)
    }
    pub(crate) fn is_initial(&self) -> bool {
        self.id == OwnerId::INITIAL
    }

    pub(crate) fn reserve_launch(&self) -> Result<LaunchReservation, AppWindowError> {
        self.ingress.reserve_owner_launch(self.id)
    }

    pub(crate) fn attach(&self, proxy: PlatformProxy) -> bool {
        let mut state = self.ingress.state.lock();
        state.phase == Phase::Accepting && state.membership.attach(self.id, proxy)
    }

    pub(crate) fn launched(&self) {
        let mut state = self.ingress.state.lock();
        state.membership.finish_launch(self.id);
        drop(state);
        contain(|| self.ingress.wake_all());
    }

    pub(crate) fn retired(&self) {
        let retired = {
            let mut state = self.ingress.state.lock();
            state.membership_changed = true;
            state.membership.retire(self.id)
        };
        let (proxy, commands) = retired;
        drop(proxy);
        for command in commands {
            command.execute(false);
        }
        self.ingress.wake_owner();
    }

    pub(crate) fn quiesce(&self) {
        let (proxy, commands) = self.ingress.state.lock().membership.quiesce(self.id);
        drop(proxy);
        for command in commands {
            command.execute(false);
        }
    }

    pub(crate) fn thread_finished(&self, result: std::thread::Result<()>) {
        self.ingress
            .state
            .lock()
            .membership
            .thread_finished(self.id, result);
        // Only native notification here: the join helper must not run a tracing
        // subscriber, completion waker or application destructor. The initial
        // owner processes this reserved result even if posting a wake fails.
        let _ = self.ingress.proxy.wake();
    }

    pub(crate) fn begin_quit_consult(&self) -> Option<QuitTicket> {
        let mut state = self.ingress.state.lock();
        if state.phase != Phase::Accepting || (self.is_initial() && (state.active || state.intent))
        {
            return None;
        }
        state.membership.begin_consult(self.id)
    }

    pub(crate) fn quit_waiting(&self) -> Vec<PresentationAddress> {
        self.ingress.state.lock().membership.waiting(self.id)
    }

    pub(crate) fn quit_consented(&self, address: PresentationAddress) {
        self.ingress
            .state
            .lock()
            .membership
            .consent(self.id, address);
        self.ingress.wake_owner();
    }

    pub(crate) fn quit_declined(&self, address: PresentationAddress) {
        self.ingress
            .state
            .lock()
            .membership
            .decline(self.id, address);
    }
}

/// Accepted publication, held until installation or rollback has completed.
pub(crate) struct LaunchReservation {
    ingress: Arc<Ingress>,
    owner: OwnerId,
}

impl Drop for LaunchReservation {
    fn drop(&mut self) {
        self.ingress
            .state
            .lock()
            .membership
            .finish_launch(self.owner);
        self.ingress.wake_owner();
    }
}

impl Ingress {
    #[cfg(feature = "testing")]
    pub(crate) fn retained_payload_bytes(&self) -> u128 {
        self.state.lock().membership.retained_payload_bytes()
    }
    pub(crate) fn finish_threads(&self) {
        let finished = {
            let mut state = self.state.lock();
            let finished = state.membership.take_finished();
            state.membership_changed |= !finished.is_empty();
            finished
        };
        for finished in finished {
            drop(finished.proxy);
            for command in finished.commands {
                command.execute(false);
            }
            if let Err(payload) = finished.result {
                let detail = self
                    .services
                    .lock()
                    .as_ref()
                    .map_or(super::FrameFailureDetail::Redacted, |services| {
                        services.failure_detail
                    });
                let message = flui_runtime::frame_failure::panic_text(detail, payload.as_ref());
                flui_foundation::panic::discard_payload(payload);
                self.record_owner_failure(AppWindowError::InstallerPanicked { message });
            }
        }
    }
    pub(crate) fn take_membership_change(&self) -> bool {
        std::mem::take(&mut self.state.lock().membership_changed)
    }
    pub(crate) fn record_owner_failure(&self, error: AppWindowError) {
        let refused = {
            let mut state = self.state.lock();
            if state.owner_failure.is_none() {
                state.owner_failure = Some(error);
                None
            } else {
                Some(error)
            }
        };
        contain(|| drop(refused));
        self.wake_owner();
    }

    pub(crate) fn take_owner_failure(&self) -> Option<AppWindowError> {
        self.state.lock().owner_failure.take()
    }

    pub(crate) fn release_services(&self) {
        let services = self.services.lock().take();
        drop(services);
    }
    pub(crate) fn release_document_budget(&self, owner: OwnerId, bytes: usize) {
        self.state.lock().membership.release_budget(owner, bytes);
    }

    pub(crate) fn serve_documents(&self) {
        let commands = self.state.lock().membership.drain_commands();
        for command in commands {
            command.execute(self.accepting());
        }
    }
    #[cfg(feature = "testing")]
    pub(crate) fn diagnostics(&self) -> String {
        let state = self.state.lock();
        format!(
            "phase={:?}, active={}, intent={}, outstanding={}, membership={:?}",
            state.phase, state.active, state.intent, state.outstanding, state.membership
        )
    }
    pub(crate) fn install_services(&self, services: super::runtime::RunServices) {
        let mut slot = self.services.lock();
        assert!(slot.is_none(), "BUG: application services installed once");
        *slot = Some(Arc::new(services));
    }

    pub(crate) fn services(&self) -> Arc<super::runtime::RunServices> {
        Arc::clone(
            self.services
                .lock()
                .as_ref()
                .expect("BUG: application services precede dedicated admission"),
        )
    }
    fn reserve_owner_launch(
        self: &Arc<Self>,
        owner: OwnerId,
    ) -> Result<LaunchReservation, AppWindowError> {
        let mut state = self.state.lock();
        if state.phase != Phase::Accepting {
            return Err(AppWindowError::AdmissionClosed);
        }
        state
            .membership
            .begin_launch(owner)
            .map_err(|_| AppWindowError::IdentityExhausted)?;
        Ok(LaunchReservation {
            ingress: Arc::clone(self),
            owner,
        })
    }

    pub(crate) fn initial_owner(self: &Arc<Self>) -> OwnerRoute {
        OwnerRoute {
            ingress: Arc::clone(self),
            id: OwnerId::INITIAL,
        }
    }

    pub(crate) fn reserve_owner(
        self: &Arc<Self>,
    ) -> Result<(OwnerRoute, Arc<ReceiptBudget>), AppWindowError> {
        self.reap_threads();
        let mut state = self.state.lock();
        if state.phase != Phase::Accepting {
            return Err(AppWindowError::AdmissionClosed);
        }
        let capacity = state.limits.owners.get();
        if state.membership.count() > capacity {
            return Err(AppWindowError::OwnerCapacity { capacity });
        }
        if state.launches == state.limits.launches.get() {
            return Err(AppWindowError::LaunchCapacity {
                capacity: state.limits.launches.get(),
            });
        }
        let id = state
            .membership
            .reserve_owner()
            .map_err(|_| AppWindowError::IdentityExhausted)?;
        state.launches += 1;
        Ok((
            OwnerRoute {
                ingress: Arc::clone(self),
                id,
            },
            Arc::new(ReceiptBudget::Launch(Arc::downgrade(self))),
        ))
    }

    pub(crate) fn keep_thread(&self, thread: std::thread::JoinHandle<()>) {
        self.threads.lock().push(thread);
    }

    fn reap_threads(&self) {
        let finished = {
            let mut threads = self.threads.lock();
            let mut finished = Vec::new();
            let mut index = 0;
            while index < threads.len() {
                if threads[index].is_finished() {
                    finished.push(threads.swap_remove(index));
                } else {
                    index += 1;
                }
            }
            finished
        };
        // These are join helpers, never UI owners. Their completed bodies have
        // already joined the UI thread (including TLS) and acknowledged retirement.
        // Do not join here: even is_finished() does not include TLS destruction.
        drop(finished);
    }

    pub(crate) fn wake_all(&self) {
        let proxies = self.state.lock().membership.proxies();
        if let Err(error) = Self::notify(proxies, false) {
            tracing::debug!(%error, "owner notification failed; control remains pending");
        }
    }
    pub(crate) fn new(
        proxy: PlatformProxy,
        startup_open: bool,
        limits: super::OwnerLimits,
    ) -> Arc<Self> {
        Arc::new(Self {
            services: Mutex::new(None),
            threads: Mutex::default(),
            device_retry: Arc::default(),
            proxy: proxy.clone(),
            state: Mutex::new(State {
                documents: Vec::new(),
                document_ids: flui_foundation::IdMint::new(),
                limits,
                launches: 0,
                owner_failure: None,
                membership_changed: false,
                phase: Phase::Accepting,
                membership: Membership::new(proxy),
                intent: startup_open,
                active: false,
                batch: None,
                superseded: false,
                closing: None,
                open_address: None,
                active_replies: Vec::new(),
                outstanding: 0,
                main_receipts: 0,
                replies: Vec::new(),
            }),
        })
    }
    pub(crate) fn device_retry(&self) -> &Arc<DeviceRetry> {
        &self.device_retry
    }
    pub(crate) fn handle(self: &Arc<Self>) -> AppHandle {
        AppHandle {
            ingress: Arc::clone(self),
        }
    }
    fn request_show(self: &Arc<Self>) -> Result<WindowRequest, AppControlError> {
        let (reply, receiver) = claim_slot(Arc::new(|| {}));
        let mut state = self.state.lock();
        if state.phase != Phase::Accepting {
            return Err(AppControlError::OwnerGone);
        }
        let capacity = WAITER_CAPACITY.min(state.limits.commands.get());
        if state.main_receipts >= capacity
            || state.membership.ordinary_count(OwnerId::INITIAL) >= state.limits.commands.get()
        {
            return Err(AppControlError::Capacity { capacity });
        }
        state
            .membership
            .changed(OwnerId::INITIAL)
            .map_err(|_| AppControlError::IdentityExhausted)?;
        let limits = state.limits;
        state
            .membership
            .reserve_command(OwnerId::INITIAL, 0, limits)
            .map_err(|reason| match reason {
                DocumentRefusal::IdentityExhausted => AppControlError::IdentityExhausted,
                DocumentRefusal::OwnerGone | DocumentRefusal::WrongRun => {
                    AppControlError::OwnerGone
                }
                DocumentRefusal::Full | DocumentRefusal::Oversize => {
                    AppControlError::Capacity { capacity }
                }
            })?;
        state.main_receipts += 1;
        let budget = Arc::new(ReceiptBudget::Main(Arc::downgrade(self)));
        let reply = MainReply {
            reply,
            budget: Arc::clone(&budget),
        };
        state.outstanding += 1;
        if state.active && !state.superseded && state.closing.is_none() {
            state.active_replies.push(reply);
        } else {
            state.intent = true;
            state.replies.push(reply);
        }
        drop(state);
        self.wake_owner();
        Ok(WindowRequest {
            reply: receiver,
            budget: Some(budget),
        })
    }
    pub(crate) fn request_native_show(&self) {
        let mut state = self.state.lock();
        if state.phase != Phase::Accepting {
            return;
        }
        if let Err(error) = state.membership.changed(OwnerId::INITIAL) {
            drop(state);
            tracing::error!(%error, "native reopen identity exhausted");
            return;
        }
        if !state.active || state.superseded || state.closing.is_some() {
            state.intent = true;
        }
        drop(state);
        self.wake_owner();
    }
    pub(crate) fn wake_owner(&self) {
        contain(|| {
            if let Err(error) = self.proxy.wake() {
                tracing::debug!(%error, "application owner wake unavailable");
            }
        });
    }
    pub(crate) fn accepting(&self) -> bool {
        self.state.lock().phase == Phase::Accepting
    }
    pub(crate) fn begin(&self) -> bool {
        let mut state = self.state.lock();
        if state.phase != Phase::Accepting
            || state.active
            || state.closing.is_some()
            || !state.intent
        {
            return false;
        }
        state.intent = false;
        state.active = true;
        state.batch = Some(Arc::new(()));
        state.superseded = false;
        state.active_replies = std::mem::take(&mut state.replies);
        true
    }
    /// Fence a closing generation even when its controller is checked out.
    pub(crate) fn begin_close(&self, address: PresentationAddress) {
        let mut state = self.state.lock();
        if state.open_address == Some(address) || (state.open_address.is_none() && state.active) {
            state.closing = Some(address);
            state.superseded = true;
        }
    }
    pub(crate) fn finish_close(&self, address: PresentationAddress) {
        let mut state = self.state.lock();
        if state.closing == Some(address) {
            state.closing = None;
            state.open_address = None;
            // Superseded remains latched until the old active batch settles.
        }
    }
    pub(crate) fn settle(&self, result: Result<PresentationAddress, AppWindowError>) {
        let (replies, result) = {
            let mut state = self.state.lock();
            let result = if state.phase != Phase::Accepting || !state.active {
                Err(AppWindowError::Cancelled)
            } else {
                result
            };
            state.active = false;
            state.batch = None;
            state.superseded = false;
            if state.phase == Phase::Accepting
                && let Ok(address) = &result
            {
                state.open_address = Some(*address);
            }
            let replies = std::mem::take(&mut state.active_replies);
            state.outstanding = state
                .outstanding
                .checked_sub(replies.len())
                .expect("BUG: active reply count exceeds admitted requests");
            (replies, result)
        };
        Self::deliver(replies, result);
        // A close/dispose callback may have reserved the next generation.
        let pending = {
            let state = self.state.lock();
            state.phase == Phase::Accepting && (state.intent || state.membership.has_quit())
        };
        if pending {
            self.wake_owner();
        }
    }
    pub(crate) fn active_batch(&self) -> Arc<()> {
        self.state
            .lock()
            .batch
            .clone()
            .expect("BUG: pending creation has an active batch")
    }
    /// Settle exactly the failed pending generation, never a replacement request.
    pub(crate) fn fail_pending(&self, batch: &Arc<()>, error: AppWindowError) -> bool {
        let replies = {
            let mut state = self.state.lock();
            if !state
                .batch
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, batch))
            {
                return false;
            }
            state.active = false;
            state.batch = None;
            let replies = std::mem::take(&mut state.active_replies);
            state.outstanding = state
                .outstanding
                .checked_sub(replies.len())
                .expect("BUG: failed batch exceeds admitted requests");
            replies
        };
        Self::deliver(replies, Err(error));
        true
    }
    fn deliver(replies: Vec<MainReply>, result: Result<PresentationAddress, AppWindowError>) {
        for MainReply { reply, budget } in replies {
            drop(budget);
            contain(|| {
                let _ = reply.deliver(result.clone());
            });
            contain(|| drop(reply));
        }
    }
    /// Quit now, past every presentation: the application's own failure
    /// (its initial window, a service that would not start, `on_ready`
    /// panicking), which no presentation may refuse. New work is fenced at
    /// once.
    pub(crate) fn quit_now(&self) -> Result<(), AppControlError> {
        let mut state = self.state.lock();
        if state.phase == Phase::Closed {
            return Err(AppControlError::OwnerGone);
        }
        let proxies = Self::commit_quit(&mut state);
        drop(state);
        Self::notify(proxies, true)
    }

    fn commit_quit(state: &mut State) -> Vec<PlatformProxy> {
        state.phase = Phase::Quitting;
        state.membership.cancel_quit();
        state.membership.retiring_proxies()
    }

    fn notify(proxies: Vec<PlatformProxy>, quitting: bool) -> Result<(), AppControlError> {
        let mut first = None;
        let mut panics = flui_foundation::panic::FirstPanic::new();
        for proxy in proxies {
            panics.contain(|| {
                let result = if quitting {
                    proxy.request_quit()
                } else {
                    proxy.wake()
                };
                if let Err(error) = result
                    && first.is_none()
                {
                    first = Some(control_error(error));
                }
            });
        }
        panics.resume();
        first.map_or(Ok(()), Err)
    }

    /// End the consultation [`OwnerRoute::begin_quit_consult`]
    /// started with what the presentations answered. A quit asked again
    /// while they were asked stays asked for the next turn.
    pub(crate) fn finish_quit_consult(&self, ticket: QuitTicket, answer: QuitAnswer) {
        self.state.lock().membership.answer(ticket, answer);
        self.wake_owner();
    }

    /// Run only on the initial owner, after replies and membership changes.
    pub(crate) fn advance_quit(&self) {
        let proxies = {
            let mut state = self.state.lock();
            if state.phase == Phase::Quitting {
                state.membership.retiring_proxies()
            } else {
                Self::commit_consented(&mut state)
            }
        };
        Self::notify_committed(proxies);
    }

    fn commit_consented(state: &mut State) -> Vec<PlatformProxy> {
        if state.phase == Phase::Accepting
            && !state.active
            && !state.intent
            && state.membership.may_commit()
        {
            Self::commit_quit(state)
        } else {
            Vec::new()
        }
    }

    fn notify_committed(proxies: Vec<PlatformProxy>) {
        if let Err(error) = Self::notify(proxies, true) {
            tracing::error!(%error, "quit notification failed; admission remains fenced");
        }
    }

    pub(crate) fn try_auto_quit(&self) -> bool {
        let mut state = self.state.lock();
        if state.phase == Phase::Closed {
            return true;
        }
        if state.phase == Phase::Quitting {
            return state.membership.count() == 1;
        }
        if state.intent || state.active || state.outstanding != 0 || state.membership.count() != 1 {
            return false;
        }
        let proxies = Self::commit_quit(&mut state);
        drop(state);
        Self::notify_committed(proxies);
        true
    }
    pub(crate) fn close(&self) {
        self.cancel_main();
        self.state.lock().phase = Phase::Closed;
        self.serve_documents();
    }

    /// Retiring the initial window controller does not retire the application
    /// coordinator: workers may still owe their native-resource acknowledgements.
    pub(crate) fn cancel_main(&self) {
        // Terminal initial-owner failure also retires workers before its shared
        // services. No join runs under ingress/registry locks.
        let _ = self.quit_now();
        let replies = {
            let mut state = self.state.lock();
            state.batch = None;
            state.active = false;
            state.intent = false;
            state.outstanding = 0;
            state.closing = None;
            state.open_address = None;
            let mut replies = std::mem::take(&mut state.active_replies);
            replies.append(&mut state.replies);
            replies
        };
        Self::deliver(replies, Err(AppWindowError::Cancelled));
    }

    /// After the initial native loop returned, under its exit watchdog, never
    /// from a native callback. Shared-service shutdown follows these joins.
    pub(crate) fn join_owners(&self) {
        let threads = std::mem::take(&mut *self.threads.lock());
        for thread in threads {
            if let Err(payload) = thread.join() {
                flui_foundation::panic::discard_payload(payload);
            }
        }
        self.finish_threads();
    }
}
fn control_error(error: flui_native::ProxySendError<()>) -> AppControlError {
    match error {
        flui_native::ProxySendError::OwnerGone { .. } => AppControlError::OwnerGone,
        other @ (flui_native::ProxySendError::WakeFailed { .. }
        | flui_native::ProxySendError::Full { .. }
        | flui_native::ProxySendError::Unsupported { .. }) => AppControlError::WakeFailed {
            message: other.to_string(),
        },
    }
}
pub(crate) fn contain(body: impl FnOnce()) {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        flui_foundation::panic::discard_payload(payload);
        if let Err(payload) = std::panic::catch_unwind(|| {
            tracing::error!("contained application callback or capture cleanup panic");
        }) {
            flui_foundation::panic::discard_payload(payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_native::{HeadlessPlatform, Platform};
    use std::{cell::RefCell, rc::Rc};

    fn ingress() -> (Arc<Ingress>, flui_native::OwnerPlatform) {
        let saved = Rc::new(RefCell::new(None));
        let returned = Rc::clone(&saved);
        Box::new(HeadlessPlatform::new())
            .run(crate::app::runner::app_handler(move |owner| {
                returned.replace(Some(owner));
                Ok(())
            }))
            .expect("headless owner");
        let owner = saved.take().expect("owner was delivered");
        (
            Ingress::new(owner.proxy(), false, crate::app::OwnerLimits::default()),
            owner,
        )
    }
    fn main_window_exit_and_admission_linearize_on_same_ingress() {
        let (ingress, _owner) = ingress();
        let handle = ingress.handle();
        let first = handle.request_show_main_window().expect("admitted first");
        assert!(!ingress.try_auto_quit());
        assert!(ingress.begin());
        assert!(
            !ingress.try_auto_quit(),
            "checked-out intent remains reserved"
        );
        ingress.settle(Err(AppWindowError::Cancelled));
        drop(first);
        assert!(ingress.try_auto_quit());
        assert!(matches!(
            handle.request_show_main_window(),
            Err(AppControlError::OwnerGone)
        ));
    }
    fn main_window_worker_admission_races_autoexit_without_lost_accepted_intent() {
        for _ in 0..24 {
            let (ingress, _owner) = ingress();
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let worker_barrier = Arc::clone(&barrier);
            let handle = ingress.handle();
            let worker = std::thread::spawn(move || {
                worker_barrier.wait();
                handle.request_show_main_window()
            });
            barrier.wait();
            let exited = ingress.try_auto_quit();
            let admitted = worker.join().expect("worker did not panic");
            assert!(
                !(exited && admitted.is_ok()),
                "exit and show cannot both win"
            );
            assert!(exited || admitted.is_ok(), "one serialized operation wins");
            ingress.close();
        }
    }

    #[test]
    fn application_control_admission_matrix() {
        crate::table_test::run_table(
            "application_control_admission_matrix",
            &[
                (
                    "main_window_exit_and_admission_linearize_on_same_ingress",
                    main_window_exit_and_admission_linearize_on_same_ingress as fn(),
                ),
                (
                    "main_window_worker_admission_races_autoexit_without_lost_accepted_intent",
                    main_window_worker_admission_races_autoexit_without_lost_accepted_intent
                        as fn(),
                ),
            ],
        );
    }
}
