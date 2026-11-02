//! Portable window-launch results; unavailable hosts refuse before construction.
#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
use flui_foundation::ClaimSlot;
#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
use flui_foundation::claim_slot;
use flui_foundation::{ClaimHandle, ClaimOutcome, PresentationAddress};
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

/// A main window could not be opened or shown.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum AppWindowError {
    /// The immutable application provider policy no longer names this launch.
    #[error("application capability registry changed during dedicated launch")]
    CapabilityGenerationChanged,
    /// An application capability policy was not recreated on the destination.
    #[error("dedicated owner omitted capability `{name}` at tier {tier}")]
    MissingCapability {
        /// Capability diagnostic name.
        name: &'static str,
        /// Required registration tier.
        tier: flui_runtime::capability::ProviderTier,
    },
    /// Local reconstruction registered conflicting providers.
    #[error(transparent)]
    CapabilityConflict(#[from] flui_runtime::capability::CapabilityConflict),
    /// The configured number of starting, live and retiring owners is reached.
    #[error("dedicated owner capacity {capacity} is exhausted")]
    OwnerCapacity {
        /// Maximum simultaneous dedicated owners.
        capacity: usize,
    },
    /// The application already has its configured number of unsettled launches.
    #[error("dedicated launch receipt capacity {capacity} is exhausted")]
    LaunchCapacity {
        /// Maximum simultaneous unsettled launches.
        capacity: usize,
    },
    /// A never-reused launch identity cannot be issued.
    #[error("window launch identity space is exhausted")]
    IdentityExhausted,
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
    /// Runtime construction, registration or root mounting failed.
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
    /// The requested window policy is not
    /// supported by this entry point in the application's current state;
    /// `reason` says which precondition failed.
    #[error("window policy not supported here: {reason}")]
    UnsupportedPolicy {
        /// Why this policy was refused, in one sentence.
        reason: &'static str,
    },
}

#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
pub(crate) type Reply = ClaimSlot<Result<PresentationAddress, AppWindowError>>;
/// Receipt for opening or showing a window. Success means mounted/wired and redraw requested,
/// not that the compositor presented a frame or the OS granted focus.
/// Dropping the receipt abandons observation, without cancelling admitted work.
#[derive(Debug)]
#[must_use = "observe window installation success or failure"]
pub struct WindowRequest {
    pub(super) reply: ClaimHandle<Result<PresentationAddress, AppWindowError>>,
    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    pub(crate) budget: Option<Arc<super::application_control::ReceiptBudget>>,
}
impl WindowRequest {
    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    pub(crate) fn new() -> (Arc<Reply>, Self) {
        let (reply, receiver) = claim_slot(Arc::new(|| {}));
        (
            Arc::new(reply),
            Self {
                reply: receiver,
                budget: None,
            },
        )
    }
    /// Take a completed result without blocking the owner thread.
    pub fn try_result(&mut self) -> Option<Result<PresentationAddress, AppWindowError>> {
        let mut cx = Context::from_waker(std::task::Waker::noop());
        match Pin::new(&mut *self).poll(&mut cx) {
            Poll::Ready(result) => Some(result),
            Poll::Pending => None,
        }
    }
}
impl Future for WindowRequest {
    type Output = Result<PresentationAddress, AppWindowError>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let outcome = Pin::new(&mut self.reply).poll(cx);
        #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
        if outcome.is_ready() {
            self.budget.take();
        }
        match outcome {
            Poll::Pending => Poll::Pending,
            Poll::Ready(ClaimOutcome::Delivered(result)) => Poll::Ready(result),
            Poll::Ready(ClaimOutcome::OwnerGone | ClaimOutcome::AlreadyClaimed) => {
                Poll::Ready(Err(AppWindowError::Cancelled))
            }
        }
    }
}
