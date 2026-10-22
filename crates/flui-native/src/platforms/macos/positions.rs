//! The positions of the macOS host's table that feature types fill.

use std::sync::Arc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;

use super::window::{decorated_frame_inset, window_capabilities};
use super::{MacOSWindow, PreferenceSource};
use crate::PlatformError;
use crate::host;
use crate::host_window::HostWindow;
use crate::owner::OpenWindowError;
use crate::owner::apple::MacOSLoop;
use crate::owner::apple::macos::debug_assert_appkit_main_thread;
use crate::owner::call_point::install;
use flui_foundation::geometry::Size;
use flui_platform::{SystemPreferences, WindowCapabilities, WindowEvent, WindowId, WindowOptions};

impl host::Window<MacOSLoop> for MacOSWindow {
    fn open(
        owner: &MacOSLoop,
        mut options: WindowOptions,
    ) -> Result<Arc<dyn HostWindow>, OpenWindowError> {
        owner
            .affinity
            .debug_assert_owner("MacOSPlatform::open_window");
        debug_assert_appkit_main_thread("MacOSPlatform::open_window");
        if !owner.loop_control.accepts_windows() {
            return Err(OpenWindowError::OwnerGone {
                rejected: Some(Box::new(options)),
            });
        }
        owner.measure_placement_globally(&mut options);
        let window = MacOSWindow::new(options, Arc::clone(&owner.windows), owner.config.clone())?;
        owner.notices.mark_stale_on_wake(window.state_cell());
        // Installed immediately after construction, before the window can be
        // handed to anyone who might redraw it — the slot is a `OnceLock`, so
        // a later install would be silently ignored rather than racing.
        window.install_loop_control(Arc::downgrade(&owner.loop_control));
        window.install_wake_pump(Arc::downgrade(&owner.wake_pump));
        window.bind_displays(Arc::clone(&owner.displays));
        let callbacks = window.callbacks();
        callbacks.set_input_capabilities(crate::shared::input_availability::macos());
        callbacks.set_input_wake(crate::owner::signal::OwnerSignal::callbacks_wake(
            &owner.loop_control.owner_signal,
            callbacks,
        ));

        Ok(window)
    }

    fn active(owner: &MacOSLoop) -> Option<WindowId> {
        owner
            .affinity
            .debug_assert_owner("MacOSPlatform::active_window");
        debug_assert_appkit_main_thread("MacOSPlatform::active_window");
        // `keyWindow` answers `None` when no window is key. The window id is
        // the native `NSWindow` pointer, the same identity `MacOSWindow`
        // reports, so a caller can compare them.
        let key_window = owner.app.keyWindow()?;
        let ptr = Retained::as_ptr(&key_window) as u64;
        Some(WindowId(ptr))
    }

    fn capabilities(_: &MacOSLoop) -> WindowCapabilities {
        window_capabilities()
    }

    fn frame_inset(owner: &MacOSLoop, _scale_factor: f64) -> Size<f64> {
        owner
            .affinity
            .debug_assert_owner("MacOSPlatform::window_frame_inset");
        debug_assert_appkit_main_thread("MacOSPlatform::window_frame_inset");
        // AppKit measures a frame in points, so the scale does not enter.
        let main = MainThreadMarker::new().expect(
            "BUG: the owner thread is the AppKit main thread, which the assertions above check",
        );
        decorated_frame_inset(main)
    }

    fn on_event(owner: &MacOSLoop, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        install(
            &owner.handler(|handlers| &mut handlers.window_event),
            callback,
        );
    }
}

impl host::Preferences<MacOSLoop> for PreferenceSource {
    fn sample(owner: &MacOSLoop) -> Result<SystemPreferences, PlatformError> {
        owner.preferences.read()
    }
}
