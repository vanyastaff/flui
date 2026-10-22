//! The positions of the iOS host's table that feature types fill.

use std::sync::Arc;

use objc2::MainThreadMarker;

use super::{IOSClipboard, IOSPreferences, IOSWindow, IosDisplays};
use crate::PlatformError;
use crate::host;
use crate::host_window::HostWindow;
use crate::owner::OpenWindowError;
use crate::owner::apple::IOSLoop;
use crate::owner::call_point::install;
use flui_foundation::geometry::Size;
use flui_platform::{
    Clipboard, DisplaySnapshot, PlatformWindow, SystemPreferences, WindowCapabilities, WindowEvent,
    WindowId, WindowOptions,
};

impl host::Window<IOSLoop> for IOSWindow {
    fn open(_: &IOSLoop, _options: WindowOptions) -> Result<Arc<dyn HostWindow>, OpenWindowError> {
        Err(OpenWindowError::Unavailable { message: "UIKit scene connection supplies the window through on_scene_event; arbitrary open_window is unsupported".into() })
    }

    fn active(owner: &IOSLoop) -> Option<WindowId> {
        MainThreadMarker::new().and_then(|_| owner.active().map(|window| window.id()))
    }

    fn capabilities(_: &IOSLoop) -> WindowCapabilities {
        WindowCapabilities::NONE
    }

    fn frame_inset(_: &IOSLoop, _: f64) -> Size<f64> {
        // A UIKit window has no non-client area.
        Size::new(0.0, 0.0)
    }

    fn on_event(owner: &IOSLoop, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        install(
            &owner.handler(|handlers| &mut handlers.window_event),
            callback,
        );
    }
}

impl host::Displays<IOSLoop> for IosDisplays {
    fn snapshot(owner: &IOSLoop) -> Result<DisplaySnapshot, PlatformError> {
        let marker = MainThreadMarker::new().ok_or_else(|| PlatformError::Displays {
            message: "UIKit display sampling must run on the main thread".into(),
        })?;
        owner.displays.snapshot(marker)
    }
}

impl host::Clipboard<IOSLoop> for IOSClipboard {
    fn clipboard(_: &IOSLoop) -> Arc<dyn Clipboard> {
        // Stateless: every handle reads and writes the general pasteboard.
        Arc::new(IOSClipboard::new())
    }
}

impl host::Preferences<IOSLoop> for IOSPreferences {
    fn sample(owner: &IOSLoop) -> Result<SystemPreferences, PlatformError> {
        let marker = MainThreadMarker::new().ok_or_else(|| PlatformError::Preferences {
            message: "UIKit preference sampling must run on the main thread".into(),
        })?;
        owner.preferences.get(marker).sample(marker)
    }
}
