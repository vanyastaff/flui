//! The window stays on its owner thread; its surface target crosses
//! (ADR-0254 §5).

use std::sync::Arc;

use flui_platform::testing::NoNativeSurface;
use flui_platform::{PlatformWindow, SurfaceTarget};
use raw_window_handle::{HandleError, HasDisplayHandle, HasWindowHandle};
use static_assertions::{assert_impl_all, assert_not_impl_any};

// `a_platform_window_cannot_cross_a_thread`, the type-level half: no window,
// nor any pointer to one, is `Send` or `Sync`.
assert_not_impl_any!(dyn PlatformWindow: Send, Sync);
assert_not_impl_any!(Arc<dyn PlatformWindow>: Send, Sync);
assert_not_impl_any!(&'static dyn PlatformWindow: Send, Sync);

// `a_surface_target_is_send_sync_and_unavailable_after_close`, the const
// half: the target is what a renderer may own on any thread.
assert_impl_all!(
    Arc<dyn SurfaceTarget>: HasWindowHandle, HasDisplayHandle, Send, Sync
);

#[test]
fn a_window_with_no_native_surface_hands_out_no_handle() {
    let target: Arc<dyn SurfaceTarget> = Arc::new(NoNativeSurface);
    assert!(matches!(
        target.window_handle(),
        Err(HandleError::Unavailable)
    ));
    assert!(matches!(
        target.display_handle(),
        Err(HandleError::Unavailable)
    ));
    target.pre_present_notify();
}
