//! `Platform` and `HostWindow` are sealed: an implementation outside
//! `flui-native` that writes every item still does not compile, and the only
//! error is the seal. `PlatformWindow` stays open, so the window below is one.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use flui_foundation::geometry::{DeviceSize, Size};
use flui_native::storage::DataDirs;
use flui_native::{
    HostWindow, OpenWindowError, PathPromptOptions, Platform, PlatformError, PlatformExecutor,
    PlatformReadyCallback, Task,
};
use flui_platform::data_transfer::DataTransferSource;
use flui_platform::{
    Clipboard, ClipboardItem, CursorError, CursorIcon, DisplaySnapshot, PlatformWindow,
    SessionEndAnswer, SessionEndPhase, StorageName, SystemNotice, SystemPreferences,
    WindowCapabilities, WindowCommand, WindowCommandError, WindowEvent, WindowGeometry, WindowId,
    WindowOptions, WindowState,
};
use flui_semantics::platform::PlatformAccessibility;

struct OutsidePlatform;

impl Platform for OutsidePlatform {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        unreachable!("the impl never compiles")
    }

    fn run(self: Box<Self>, _: PlatformReadyCallback) -> Result<(), PlatformError> {
        Ok(())
    }

    fn quit(&self) {}

    fn set_exit_policy_hook(&self, _: Box<dyn Fn() -> bool + Send>) {}

    fn request_exit_policy_reevaluation(&self) {}

    fn set_wake_deadline_hook(&self, _: Box<dyn Fn() -> Option<Instant> + Send + Sync>) {}

    fn open_window(&self, _: WindowOptions) -> Result<Arc<dyn HostWindow>, OpenWindowError> {
        Ok(Arc::new(OutsideWindow))
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

    fn on_quit(&self, _: Box<dyn FnMut() + Send>) {}

    fn on_session_end(&self, _: Box<dyn FnMut(SessionEndPhase) -> SessionEndAnswer + Send>) {}

    fn on_host_notice(&self, _: Box<dyn FnMut(SystemNotice) + Send>) {}

    fn release_session_end(&self) {}

    fn window_capabilities(&self) -> WindowCapabilities {
        WindowCapabilities::NONE
    }

    fn window_frame_inset(&self, _: f64) -> Size<f64> {
        Size::default()
    }

    fn on_reopen(&self, _: Box<dyn FnMut() + Send>) {}

    fn on_window_event(&self, _: Box<dyn FnMut(WindowEvent) + Send>) {}

    fn on_open_urls(&self, _: Box<dyn FnMut(Vec<String>) + Send>) {}

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

    fn physical_size(&self) -> DeviceSize {
        Size::default()
    }

    fn logical_size(&self) -> Size {
        Size::default()
    }

    fn scale_factor(&self) -> f64 {
        1.0
    }

    fn request_redraw(&self) {}

    fn is_focused(&self) -> bool {
        false
    }

    fn is_visible(&self) -> bool {
        false
    }

    fn state(&self) -> WindowState {
        WindowState::new(WindowGeometry::new(None, Size::default()), Size::default())
    }

    fn capabilities(&self) -> WindowCapabilities {
        WindowCapabilities::NONE
    }

    fn apply(&self, command: WindowCommand) -> Result<(), WindowCommandError> {
        command.admit(&WindowCapabilities::NONE)?;
        Ok(())
    }

    fn set_cursor(&self, _: CursorIcon) -> Result<(), CursorError> {
        Err(flui_platform::Unsupported::new(flui_platform::UnsupportedReason::NotOnThisPlatform).into())
    }

    fn text_input(&self) -> Option<Arc<dyn flui_platform::PlatformTextInput>> {
        None
    }

    fn haptics(&self) -> Option<Arc<dyn flui_platform::PlatformHaptics>> {
        None
    }

    fn system_chrome(&self) -> Option<Arc<dyn flui_platform::PlatformSystemChrome>> {
        None
    }
}

impl HostWindow for OutsideWindow {
    fn accessibility(&self) -> Option<Arc<dyn PlatformAccessibility>> {
        None
    }
}

fn main() {}
