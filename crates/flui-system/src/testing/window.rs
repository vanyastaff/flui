//! Windows a provider's test acquires its handles in, on every host: a
//! [`WindowRef`] over a window that is only its id and the surface target a
//! test gives it, open until its [`WindowCloser`] closes it.

use std::rc::Rc;
use std::sync::Arc;

use flui_platform::capability::{WindowCloser, WindowRef};
use flui_platform::haptics::PlatformHaptics;
use flui_platform::{
    AdmittedCommand, ApplyError, Cursor, CursorError, GestureGeometry, PlatformSystemChrome,
    PlatformTextInput, PlatformWindow, PreferenceQueryError, RedrawRequester, SurfaceTarget,
    SwapChainStatistics, WindowAt, WindowExtent, WindowGeometry, WindowId, WindowIdMint,
    WindowState,
};
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};

/// A window with no native handle: what a provider sees of a window it
/// cannot own a dialog with.
struct Handleless;

impl HasWindowHandle for Handleless {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        Err(HandleError::Unavailable)
    }
}

impl HasDisplayHandle for Handleless {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Err(HandleError::Unavailable)
    }
}

impl SurfaceTarget for Handleless {
    fn pre_present_notify(
        &self,
        _: Option<flui_sdk::foundation::FrameStamp>,
        _: SwapChainStatistics,
    ) {
    }

    fn keep_past_close(&self) {}
}

/// A window with no native handle, open until its closer closes it.
pub(crate) fn window() -> (WindowRef, WindowCloser) {
    open(Arc::new(Handleless))
}

/// A window whose surface is `target`, open until its closer closes it.
pub(crate) fn open(target: Arc<dyn SurfaceTarget>) -> (WindowRef, WindowCloser) {
    thread_local! {
        // One mint per test thread: two windows of one test are two ids.
        static WINDOW_IDS: WindowIdMint = const { WindowIdMint::new() };
    }
    let (closer, lifetime) = WindowCloser::new();
    let id = WINDOW_IDS
        .with(WindowIdMint::next)
        .expect("BUG: a test opens far fewer windows than a mint issues");
    let window: Rc<dyn PlatformWindow> = Rc::new(TestWindow { id, target });
    (WindowRef::new(&window, lifetime), closer)
}

/// A window that is its id and its surface: what a [`WindowRef`] reads of
/// it. Every verb answers that it does nothing.
struct TestWindow {
    id: WindowId,
    target: Arc<dyn SurfaceTarget>,
}

impl PlatformWindow for TestWindow {
    flui_platform::window_takes_no_frame_verbs!();

    fn id(&self) -> WindowId {
        self.id
    }
    fn gesture_geometry(&self) -> Result<Option<GestureGeometry>, PreferenceQueryError> {
        Ok(None)
    }
    fn request_redraw(&self) {}
    fn redraw_requester(&self) -> Arc<dyn RedrawRequester> {
        Arc::new(|| {})
    }
    fn surface_target(&self) -> Arc<dyn SurfaceTarget> {
        Arc::clone(&self.target)
    }
    fn state(&self) -> WindowState {
        WindowState::new(WindowGeometry::EMPTY, WindowExtent::ZERO)
    }
    fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
        command.against(&self.state().capabilities)?;
        Ok(())
    }
    fn set_cursor(&self, _: &Cursor) -> Result<(), CursorError> {
        Ok(())
    }
    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        None
    }
    fn haptics(&self) -> Option<Arc<dyn PlatformHaptics>> {
        None
    }
    fn system_chrome(&self) -> Option<Arc<dyn PlatformSystemChrome>> {
        None
    }
    fn drag_source(&self) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
        None
    }
    fn window_at(&self, _: flui_sdk::foundation::geometry::Point<f64>) -> WindowAt {
        WindowAt::Unknown
    }
    fn reveal_after_first_frame(&self) {}
}
