//! The portable dedicated-owner entry and its explicit availability refusal.
use super::{AppWindowError, RuntimeWindowConfig, WindowRequest};
use flui_view::View;

/// Construct a window and UI runtime on a fresh permanent owner.
/// Only the construction factory crosses a thread; its output may hold `Rc`.
/// The receipt means mounted, wired and redraw requested, not displayed.
///
/// Native admission is unavailable until integrated native acceptance. The
/// headless testing kit and explicit native qualification entry exercise the
/// implementation without promoting platform support.
///
/// # Errors
/// Unsupported placement and exhausted capacity refuse before factory execution.
/// An admitted launch settles through its receipt, even if observation is dropped.
pub fn open_dedicated_window<V, F>(factory: F) -> Result<WindowRequest, AppWindowError>
where
    V: View + Clone + 'static,
    F: FnOnce() -> (V, RuntimeWindowConfig) + Send + 'static,
{
    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    {
        super::runner::dedicated::open(factory)
    }
    #[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
    {
        drop(factory);
        Err(AppWindowError::UnsupportedPolicy {
            reason: "dedicated ownership is unavailable on this host",
        })
    }
}
