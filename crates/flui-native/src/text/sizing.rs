//! Owner-acquired, detached native numeric text sizing.
//!
//! The composition root acquires a presentation's converter on the owner
//! thread ([`capture_text_sizing`]). A platform whose system text size is not
//! one linear scale captures it natively and provides it as a
//! [`NativeTextSize`]: Android a copy of its `DisplayMetrics`, UIKit its trait
//! collection.

use flui_foundation::{TextSize, TextSizeRequest};
use std::{
    marker::PhantomData,
    rc::Rc,
    sync::Arc,
    sync::atomic::{AtomicBool, Ordering},
};

use crate::host_window::{HostWindow, OwnerThreadToken};
use crate::owner::OwnerPlatform;

/// Acquire a detached numeric text converter for `window`, a native
/// presentation, on the owner thread `_owner` proves.
///
/// # Errors
/// Refuses unavailable native contexts and failed capture/query admission.
pub fn capture_text_sizing(
    _owner: &OwnerPlatform,
    window: &Arc<dyn HostWindow>,
) -> Result<TextSizingCaptureState, TextSizingCaptureError> {
    window.capture_text_sizing(OwnerThreadToken::new())
}

/// Readiness of one presentation's detached conversion context.
#[derive(Debug)]
pub enum TextSizingCaptureState {
    /// This backend has no nonlinear native conversion.
    Unsupported,
    /// Acquisition owns an outstanding completion receipt. The composition
    /// root must also schedule one status check after this remaining delay;
    /// the native dispatcher does not acknowledge its wake delivery.
    Pending {
        /// Time until the original flight's bounded status check.
        check_after: std::time::Duration,
    },
    /// The original capture has not completed by its check deadline. Its
    /// receipt remains outstanding and can still deliver; do not post another
    /// job or schedule a hot retry. A matching late completion wakes the owner.
    Stalled,
    /// No capture is in flight. The composition root must schedule an owner
    /// turn after this remaining delay and retry acquisition.
    RetryAfter(std::time::Duration),
    /// An immutable conversion context is ready.
    Ready(CapturedTextSizing),
}

/// Failure to acquire or use a native numeric conversion context.
#[derive(Clone, Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TextSizingCaptureError {
    /// Acquisition or conversion was called off the owner lane.
    #[error("native text sizing requires its owner thread")]
    WrongThread,
    /// The native presentation or capture has been withdrawn.
    #[error("native text sizing context is unavailable")]
    Unavailable,
    /// The backend cannot map this numeric growth profile.
    #[error("unsupported native text growth profile")]
    UnsupportedProfile,
    /// A native call failed.
    #[error("native text sizing failed: {message}")]
    Native {
        /// Portable diagnostic; no native exception leaves its owner.
        message: String,
    },
    /// A native result cannot produce finite positive shaping geometry.
    #[error(transparent)]
    InvalidSize(#[from] flui_foundation::TextSizeError),
}

/// A platform's frozen native converter: the size an authored size and growth
/// profile take under the system text setting it captured.
pub(crate) trait NativeTextSize {
    /// The size `request` takes under the captured setting.
    ///
    /// # Errors
    /// The profile the platform cannot map, a failed native call, or a native
    /// answer that is no valid size.
    fn query(&self, request: TextSizeRequest) -> Result<TextSize, TextSizingCaptureError>;
}

/// A frozen native converter, independent of live Resources or trait lookup.
/// Clones retain one identity. Publication authority is withdrawn separately
/// from the lifetime of its native resources.
#[derive(Clone)]
pub struct CapturedTextSizing {
    native: Arc<dyn NativeTextSize>,
    valid: Arc<AtomicBool>,
    owner: std::thread::ThreadId,
    _owner_local: PhantomData<Rc<()>>,
}

impl std::fmt::Debug for CapturedTextSizing {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CapturedTextSizing")
            .field("owner", &self.owner)
            .field("current", &self.is_current())
            .finish_non_exhaustive()
    }
}

impl CapturedTextSizing {
    #[cfg(any(target_os = "android", target_os = "ios", test))]
    pub(crate) fn new(
        native: Arc<dyn NativeTextSize>,
        valid: Arc<AtomicBool>,
        owner: std::thread::ThreadId,
    ) -> Self {
        Self {
            native,
            valid,
            owner,
            _owner_local: PhantomData,
        }
    }

    /// Whether both leases refer to exactly the same frozen conversion context.
    #[must_use]
    pub fn same_capture(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.native, &other.native)
    }

    /// Whether this capture still has its native presentation authority.
    #[must_use]
    pub fn is_current(&self) -> bool {
        self.valid.load(Ordering::Acquire)
    }

    /// Resolve one exact authored size/profile without sampling live settings.
    /// Native calls occur here, outside Painting and TextContext loans.
    ///
    /// # Errors
    /// Refuses wrong-thread or withdrawn captures and invalid native output.
    pub fn query(&self, request: TextSizeRequest) -> Result<TextSize, TextSizingCaptureError> {
        if self.owner != std::thread::current().id() {
            return Err(TextSizingCaptureError::WrongThread);
        }
        if !self.is_current() {
            return Err(TextSizingCaptureError::Unavailable);
        }
        let result = self.native.query(request);
        if !self.is_current() {
            return Err(TextSizingCaptureError::Unavailable);
        }
        result
    }
}

/// A converter of a test's own, which answers through its closure.
#[cfg(test)]
pub(crate) struct QueryWith<F>(pub(crate) F);

#[cfg(test)]
impl<F> NativeTextSize for QueryWith<F>
where
    F: Fn(TextSizeRequest) -> Result<TextSize, TextSizingCaptureError>,
{
    fn query(&self, request: TextSizeRequest) -> Result<TextSize, TextSizingCaptureError> {
        (self.0)(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn rejects_withdrawn_queries_and_results() {
        let valid = Arc::new(AtomicBool::new(true));
        let calls = Arc::new(AtomicUsize::new(0));
        let native_valid = Arc::clone(&valid);
        let native_calls = Arc::clone(&calls);
        let capture = CapturedTextSizing::new(
            Arc::new(QueryWith(move |request: TextSizeRequest| {
                native_calls.fetch_add(1, Ordering::Relaxed);
                native_valid.store(false, Ordering::Release);
                Ok(request.size)
            })),
            valid,
            std::thread::current().id(),
        );
        let request = TextSizeRequest {
            size: TextSize::new(16.0).expect("valid authored size"),
            profile: flui_foundation::TextScaleProfile::Body,
        };
        assert!(matches!(
            capture.query(request),
            Err(TextSizingCaptureError::Unavailable)
        ));
        assert!(matches!(
            capture.query(request),
            Err(TextSizingCaptureError::Unavailable)
        ));
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "a withdrawn lease must not enter native code again"
        );
    }

    #[test]
    fn captured_text_sizing_authority() {
        crate::table_test::run_table(
            "captured_text_sizing_authority",
            &[(
                "rejects_withdrawn_queries_and_results",
                rejects_withdrawn_queries_and_results,
            )],
        );
    }
}
