//! The positions of the iOS host's table that feature types fill.

use std::sync::Arc;

use objc2::MainThreadMarker;

use super::{IOSClipboard, IOSWindow};
use crate::host;
use crate::host_window::HostWindow;
use crate::owner::OpenWindowError;
use crate::owner::apple::IOSLoop;
use crate::owner::call_point::install;
use flui_foundation::geometry::Size;
use flui_platform::{
    Clipboard, PlatformWindow, WindowCapabilities, WindowEvent, WindowId, WindowOptions,
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

impl host::Clipboard<IOSLoop> for IOSClipboard {
    fn clipboard(owner: &IOSLoop) -> Arc<dyn Clipboard> {
        owner.clipboard.clone()
    }
}
