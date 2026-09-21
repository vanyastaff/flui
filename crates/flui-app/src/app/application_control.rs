//! Closed cross-thread application commands. UI factories never enter this state.
use flui_foundation::{ClaimHandle, ClaimOutcome, ClaimSlot, PresentationAddress, claim_slot};
use flui_platform::PlatformProxy;
use parking_lot::Mutex;
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

/// Failure to admit an application command.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum AppControlError {
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

/// A main window could not be opened or shown.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum AppWindowError {
    /// Native creation failed; retains the original platform error.
    #[error("native window creation failed: {source}")]
    Native {
        /// Original error, shared across coalesced request replies.
        #[source]
        source: Arc<dyn std::error::Error + Send + Sync>,
    },
    /// GPU initialization failed.
    #[error("renderer initialization failed: {source}")]
    Renderer {
        /// Original renderer error.
        #[source]
        source: Arc<dyn std::error::Error + Send + Sync>,
    },
    /// Realm construction, registration or root mounting failed.
    #[error("root mount failed: {source}")]
    Mount {
        /// Original initialization error.
        #[source]
        source: Arc<dyn std::error::Error + Send + Sync>,
    },
    /// The user factory unwound; text follows the configured failure policy.
    #[error("root factory panicked: {message}")]
    FactoryPanicked {
        /// Materialized panic text; arbitrary payloads are never retained.
        message: super::PanicText,
    },
    /// Installation unwound after native creation.
    #[error("window installation panicked: {message}")]
    InstallerPanicked {
        /// Materialized panic text; arbitrary payloads are never retained.
        message: super::PanicText,
    },
    /// The existing window could not be revealed.
    #[error("window could not be shown: {source}")]
    Show {
        /// Original native reveal error.
        #[source]
        source: Arc<dyn std::error::Error + Send + Sync>,
    },
    /// Shutdown cancelled an admitted intent before installation completed.
    #[error("application shutdown cancelled the window request")]
    Cancelled,
    /// The request was refused at admission because the application is
    /// quitting, or because the loop it was made against has stopped or
    /// been replaced. Unlike [`Self::Cancelled`], nothing was admitted.
    #[error("window admission is closed: the application loop is quitting or gone")]
    AdmissionClosed,
    /// Called from a thread that hosts no running platform loop. Window
    /// entry points must run on the owner thread, from inside or after a
    /// running `Platform::run`'s `on_ready`.
    #[error("no application loop runs on this thread; call from the owner thread")]
    NoOwnerLoop,
    /// The requested [`WindowPolicy`](crate::app::WindowPolicy) is not
    /// supported by this entry point in the application's current state;
    /// `reason` says which precondition failed.
    #[error("window policy not supported here: {reason}")]
    UnsupportedPolicy {
        /// Why this policy was refused, in one sentence.
        reason: &'static str,
    },
}

type Reply = ClaimSlot<Result<PresentationAddress, AppWindowError>>;
const WAITER_CAPACITY: usize = 64;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Accepting,
    Quitting,
    Closed,
}
struct State {
    phase: Phase,
    intent: bool,
    active: bool,
    batch: Option<Arc<()>>,
    superseded: bool,
    closing: Option<PresentationAddress>,
    open_address: Option<PresentationAddress>,
    active_replies: Vec<Reply>,
    outstanding: usize,
    replies: Vec<Reply>,
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
    /// Admit a coalesced show request. Dropping its receiver does not cancel intent.
    pub fn request_show_main_window(&self) -> Result<MainWindowRequest, AppControlError> {
        self.ingress.request_show()
    }
    /// Fence new work immediately and request ordinary application shutdown.
    pub fn request_quit(&self) -> Result<(), AppControlError> {
        let mut state = self.ingress.state.lock();
        if state.phase == Phase::Closed {
            return Err(AppControlError::OwnerGone);
        }
        state.phase = Phase::Quitting;
        self.ingress.proxy.request_quit().map_err(control_error)
    }
}

/// Reply to a show request. Success means mounted/wired and redraw requested,
/// not that the compositor presented a frame or the OS granted focus.
#[derive(Debug)]
pub struct MainWindowRequest {
    reply: ClaimHandle<Result<PresentationAddress, AppWindowError>>,
}
impl MainWindowRequest {
    /// Take a completed result without blocking the owner thread.
    pub fn try_result(&mut self) -> Option<Result<PresentationAddress, AppWindowError>> {
        let mut cx = Context::from_waker(std::task::Waker::noop());
        match Pin::new(&mut *self).poll(&mut cx) {
            Poll::Ready(result) => Some(result),
            Poll::Pending => None,
        }
    }
}
impl Future for MainWindowRequest {
    type Output = Result<PresentationAddress, AppWindowError>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.reply).poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(ClaimOutcome::Delivered(result)) => Poll::Ready(result),
            Poll::Ready(ClaimOutcome::OwnerGone | ClaimOutcome::AlreadyClaimed) => {
                Poll::Ready(Err(AppWindowError::Cancelled))
            }
        }
    }
}

pub(crate) struct Ingress {
    state: Mutex<State>,
    proxy: PlatformProxy,
}
impl Ingress {
    pub(crate) fn new(proxy: PlatformProxy, startup_open: bool) -> Arc<Self> {
        Arc::new(Self {
            proxy,
            state: Mutex::new(State {
                phase: Phase::Accepting,
                intent: startup_open,
                active: false,
                batch: None,
                superseded: false,
                closing: None,
                open_address: None,
                active_replies: Vec::new(),
                outstanding: 0,
                replies: Vec::new(),
            }),
        })
    }
    pub(crate) fn handle(self: &Arc<Self>) -> AppHandle {
        AppHandle {
            ingress: Arc::clone(self),
        }
    }
    fn request_show(&self) -> Result<MainWindowRequest, AppControlError> {
        let (reply, receiver) = claim_slot(Arc::new(|| {}));
        let mut state = self.state.lock();
        if state.phase != Phase::Accepting {
            return Err(AppControlError::OwnerGone);
        }
        if state.outstanding == WAITER_CAPACITY {
            return Err(AppControlError::Capacity {
                capacity: WAITER_CAPACITY,
            });
        }
        // Notification is nonblocking and invokes no user code. Serialize it
        // with admission so another sender cannot acknowledge a failed post.
        self.proxy.wake().map_err(control_error)?;
        state.outstanding += 1;
        if state.active && !state.superseded && state.closing.is_none() {
            state.active_replies.push(reply);
        } else {
            state.intent = true;
            state.replies.push(reply);
        }
        Ok(MainWindowRequest { reply: receiver })
    }
    pub(crate) fn request_native_show(&self) {
        let mut state = self.state.lock();
        if state.phase != Phase::Accepting {
            return;
        }
        if let Err(error) = self.proxy.wake() {
            drop(state);
            tracing::error!(%error, "native reopen could not notify application owner");
            return;
        }
        if !state.active || state.superseded || state.closing.is_some() {
            state.intent = true;
        }
    }
    pub(crate) fn wake_owner(&self) {
        if let Err(error) = self.proxy.wake() {
            tracing::debug!(%error, "application owner wake unavailable");
        }
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
            state.phase == Phase::Accepting && state.intent
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
    fn deliver(replies: Vec<Reply>, result: Result<PresentationAddress, AppWindowError>) {
        for reply in replies {
            contain(|| {
                let _ = reply.deliver(result.clone());
            });
            contain(|| drop(reply));
        }
    }
    pub(crate) fn try_auto_quit(&self) -> bool {
        let mut state = self.state.lock();
        if state.phase != Phase::Accepting {
            return true;
        }
        if state.intent || state.active || state.outstanding != 0 {
            return false;
        }
        state.phase = Phase::Quitting;
        // The native admission fence is synchronous even if posting fails.
        let result = self.proxy.request_quit();
        drop(state);
        if let Err(error) = result {
            tracing::error!(%error, "automatic quit notification failed");
        }
        true
    }
    pub(crate) fn close(&self) {
        let replies = {
            let mut state = self.state.lock();
            state.phase = Phase::Closed;
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
}
fn control_error(error: flui_platform::ProxySendError<()>) -> AppControlError {
    match error {
        flui_platform::ProxySendError::OwnerGone { .. } => AppControlError::OwnerGone,
        other => AppControlError::WakeFailed {
            message: other.to_string(),
        },
    }
}
pub(crate) fn contain(body: impl FnOnce()) {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        std::mem::forget(payload);
        if let Err(payload) = std::panic::catch_unwind(|| {
            tracing::error!("contained application callback or capture cleanup panic");
        }) {
            std::mem::forget(payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_platform::{HeadlessPlatform, Platform};
    use std::{cell::RefCell, rc::Rc};

    fn ingress() -> (Arc<Ingress>, flui_platform::OwnerPlatform) {
        let saved = Rc::new(RefCell::new(None));
        let returned = Rc::clone(&saved);
        Box::new(HeadlessPlatform::new())
            .run(Box::new(move |owner| {
                returned.replace(Some(owner));
                Ok(())
            }))
            .expect("headless owner");
        let owner = saved.take().expect("owner was delivered");
        (Ingress::new(owner.proxy(), false), owner)
    }
    #[test]
    fn main_window_admission_capacity_receiver_drop_and_terminal_result() {
        let (ingress, _owner) = ingress();
        let handle = ingress.handle();
        let requests: Vec<_> = (0..WAITER_CAPACITY)
            .map(|_| handle.request_show_main_window().expect("within capacity"))
            .collect();
        assert!(matches!(
            handle.request_show_main_window(),
            Err(AppControlError::Capacity { .. })
        ));
        drop(requests);
        assert!(
            !ingress.try_auto_quit(),
            "receiver drop does not cancel intent"
        );
        assert!(ingress.begin());
        let mut requests: Vec<_> = Vec::new();
        ingress.settle(Err(AppWindowError::Cancelled));
        requests.push(
            handle
                .request_show_main_window()
                .expect("settlement releases capacity"),
        );
        ingress.close();
        assert!(matches!(
            requests[0].try_result(),
            Some(Err(AppWindowError::Cancelled))
        ));
        assert!(matches!(
            handle.request_show_main_window(),
            Err(AppControlError::OwnerGone)
        ));
    }
    #[test]
    fn main_window_failed_batch_releases_reservation_without_touching_its_successor() {
        let (ingress, _owner) = ingress();
        let mut first = ingress.handle().request_show_main_window().expect("first");
        assert!(ingress.begin());
        let previous = ingress.active_batch();
        assert!(ingress.fail_pending(&previous, AppWindowError::Cancelled));
        assert!(matches!(
            first.try_result(),
            Some(Err(AppWindowError::Cancelled))
        ));
        {
            let state = ingress.state.lock();
            assert_eq!(state.outstanding, 0);
            assert!(!state.active);
        }
        let mut next = ingress.handle().request_show_main_window().expect("next");
        assert!(ingress.begin());
        assert!(!ingress.fail_pending(&previous, AppWindowError::Cancelled));
        assert!(next.try_result().is_none());
        assert_eq!(ingress.state.lock().outstanding, 1);
        ingress.settle(Err(AppWindowError::Cancelled));
    }
    #[test]
    fn main_window_quit_cancels_success_settled_after_its_admission() {
        let (ingress, _owner) = ingress();
        let handle = ingress.handle();
        let mut request = handle.request_show_main_window().expect("admit");
        assert!(ingress.begin());
        handle.request_quit().expect("quit");
        ingress.settle(Ok(PresentationAddress {
            realm_id: flui_foundation::RealmId::new(1),
            presentation_id: flui_foundation::PresentationId::new(1),
        }));
        assert!(matches!(
            request.try_result(),
            Some(Err(AppWindowError::Cancelled))
        ));
    }
    #[test]
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
    #[test]
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
    fn main_window_reply_owner_gone_is_terminal_without_blocking() {
        let (reply, receiver) = claim_slot(Arc::new(|| {}));
        let mut request = MainWindowRequest { reply: receiver };
        assert!(request.try_result().is_none());
        drop(reply);
        assert!(matches!(
            request.try_result(),
            Some(Err(AppWindowError::Cancelled))
        ));
    }
    #[test]
    fn main_window_reply_waker_can_admit_the_next_batch_without_old_settlement_consuming_it() {
        struct Admit {
            handle: AppHandle,
            request: Mutex<Option<MainWindowRequest>>,
        }
        impl std::task::Wake for Admit {
            fn wake(self: Arc<Self>) {
                self.wake_by_ref();
            }
            fn wake_by_ref(self: &Arc<Self>) {
                let request = self
                    .handle
                    .request_show_main_window()
                    .expect("reentrant admission");
                let displaced = { self.request.lock().replace(request) };
                drop(displaced);
            }
        }
        let (ingress, _owner) = ingress();
        let mut first = ingress.handle().request_show_main_window().expect("first");
        let admitted = Arc::new(Admit {
            handle: ingress.handle(),
            request: Mutex::new(None),
        });
        let waker = std::task::Waker::from(Arc::clone(&admitted));
        assert!(
            Pin::new(&mut first)
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        assert!(ingress.begin());
        ingress.settle(Err(AppWindowError::Cancelled));
        let mut next = admitted
            .request
            .lock()
            .take()
            .expect("waker admitted next request");
        assert!(next.try_result().is_none());
        assert!(
            !ingress.try_auto_quit(),
            "next reservation survives callback delivery"
        );
        assert!(ingress.begin());
        ingress.settle(Err(AppWindowError::Cancelled));
        assert!(matches!(
            next.try_result(),
            Some(Err(AppWindowError::Cancelled))
        ));
    }

    #[test]
    fn main_window_panicking_reply_waker_does_not_skip_siblings() {
        struct Panics;
        impl std::task::Wake for Panics {
            fn wake(self: Arc<Self>) {
                panic!("reply wake");
            }
        }
        let (ingress, _owner) = ingress();
        let mut first = ingress.handle().request_show_main_window().expect("first");
        let mut second = ingress.handle().request_show_main_window().expect("second");
        let waker = std::task::Waker::from(Arc::new(Panics));
        assert!(
            Pin::new(&mut first)
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        assert!(ingress.begin());
        ingress.settle(Err(AppWindowError::Cancelled));
        assert!(matches!(
            second.try_result(),
            Some(Err(AppWindowError::Cancelled))
        ));
        assert!(ingress.handle().request_show_main_window().is_ok());
    }
}
