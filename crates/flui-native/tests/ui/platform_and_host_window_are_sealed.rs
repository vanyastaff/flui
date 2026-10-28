//! `Platform` and `HostWindow` are sealed: an implementation outside
//! `flui-native` does not compile, and the seal says why. `Platform`'s writes
//! every item, so the seal is its only error; `HostWindow`'s cannot write the
//! items that take the owner thread's token, which it cannot name, so those
//! are missing too. `PlatformWindow` stays open, so the window below is one.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::geometry::Size;
use flui_native::storage::DataDirs;
use flui_native::{
    AppHandler, HostWindow, OpenWindowError, PathPromptOptions, Platform, PlatformError,
    PlatformExecutor, Task,
};
use flui_platform::data_transfer::DataTransferSource;
use flui_platform::{
    Clipboard, ClipboardItem, CursorError, CursorIcon, DisplaySnapshot, PlatformWindow,
    StorageName, SystemPreferences,
    AdmittedCommand, ApplyError, WindowCapabilities, WindowGeometry, WindowId,
    WindowHandler, WindowOptions, WindowState,
};
use flui_semantics::platform::PlatformAccessibility;

struct OutsidePlatform;

impl Platform for OutsidePlatform {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        unreachable!("the impl never compiles")
    }

    fn run(self: Box<Self>, _: Rc<dyn AppHandler>) -> Result<(), PlatformError> {
        Ok(())
    }

    fn quit(&self) {}

    fn request_exit_policy_reevaluation(&self) {}

    fn open_window(
        &self,
        _: WindowOptions,
        _: Rc<dyn WindowHandler>,
    ) -> Result<Rc<dyn HostWindow>, OpenWindowError> {
        Ok(Rc::new(OutsideWindow))
    }

    fn active_window(&self) -> Option<WindowId> {
        None
    }

    fn preferences(&self) -> Result<SystemPreferences, PlatformError> {
        unreachable!("the impl never compiles")
    }

    fn display_snapshot(&self) -> Result<DisplaySnapshot, PlatformError> {
        unreachable!("the impl never compiles")
    }

    fn clipboard(&self) -> Arc<dyn Clipboard> {
        unreachable!("the impl never compiles")
    }

    fn data_transfer(&self) -> Arc<dyn DataTransferSource> {
        unreachable!("the impl never compiles")
    }

    fn activate(&self, _: bool) {}

    fn write_to_clipboard(&self, _: ClipboardItem) {}

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        None
    }

    fn open_url(&self, _: &str) {}

    fn reveal_path(&self, _: &Path) {}

    fn open_path(&self, _: &Path) {}

    fn prompt_for_paths(
        &self,
        _: PathPromptOptions,
    ) -> Task<Result<Option<Vec<PathBuf>>, PlatformError>> {
        unreachable!("the impl never compiles")
    }

    fn prompt_for_new_path(
        &self,
        _: &Path,
        _: Option<&str>,
    ) -> Task<Result<Option<PathBuf>, PlatformError>> {
        unreachable!("the impl never compiles")
    }

    fn keyboard_layout(&self) -> String {
        String::new()
    }

    fn on_keyboard_layout_change(&self, _: Box<dyn FnMut() + Send>) {}

    fn release_session_end(&self) {}

    fn window_capabilities(&self) -> WindowCapabilities {
        WindowCapabilities::NONE
    }

    fn window_frame_inset(&self, _: f64) -> Size<f64> {
        Size::default()
    }

    fn name(&self) -> &'static str {
        "outside"
    }

    fn compositor_name(&self) -> &'static str {
        ""
    }

    fn app_path(&self) -> Result<PathBuf, PlatformError> {
        Ok(PathBuf::new())
    }

    fn data_dirs(&self, _: &StorageName) -> Option<DataDirs> {
        None
    }
}

struct OutsideWindow;

impl PlatformWindow for OutsideWindow {
    fn id(&self) -> WindowId {
        WindowId(7)
    }

    fn request_redraw(&self) {}

    fn state(&self) -> WindowState {
        WindowState::new(WindowGeometry::new(None, Size::default()), Size::default())
    }

    fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
        command.against(&WindowCapabilities::NONE)?;
        Ok(())
    }

    fn set_cursor(&self, _: CursorIcon) -> Result<(), CursorError> {
        Err(flui_platform::Unsupported::new(flui_platform::UnsupportedReason::NotOnThisPlatform).into())
    }

    flui_platform::window_takes_no_frame_verbs!();

    fn text_input(&self) -> Option<Arc<dyn flui_platform::PlatformTextInput>> {
        None
    }

    fn haptics(&self) -> Option<Arc<dyn flui_platform::PlatformHaptics>> {
        None
    }

    fn system_chrome(&self) -> Option<Arc<dyn flui_platform::PlatformSystemChrome>> {
        None
    }

    fn drag_source(&self) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
        None
    }

    fn redraw_requester(&self) -> Arc<dyn flui_platform::RedrawRequester> {
        Arc::new(|| {})
    }

    fn surface_target(&self) -> Arc<dyn flui_platform::SurfaceTarget> {
        Arc::new(flui_platform::NoNativeSurface)
    }
}

impl HostWindow for OutsideWindow {
    fn accessibility(&self) -> Option<Arc<dyn PlatformAccessibility>> {
        None
    }
}

fn main() {}
