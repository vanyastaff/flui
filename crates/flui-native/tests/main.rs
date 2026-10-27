//! Single-binary consolidation of flui-native's root integration tests.
//!
//! Each former standalone test target linked the full dependency stack
//! separately; compiling them as modules of one `platform_it` binary cuts
//! link time and `target/` disk. Source files stay in place (see
//! `autotests = false` + `[[test]]` in `Cargo.toml`), so file-relative
//! and manifest-relative paths keep working unchanged.
//!
//! Convention: tests that WRITE process-global state (e.g. the
//! `FLUI_HEADLESS` env var) live in their own [[test]] target instead —
//! process isolation beats opt-in locking. See headless.
//!
//! macOS invariant: any `platform_it` test that constructs the platform
//! through `current_platform()` (or a helper reaching it) must carry
//! `#[cfg_attr(target_os = "macos", ignore = "requires an AppKit-run-loop-
//! pumping test process (ADR-0039): …")]` — the macOS platform surface
//! asserts the AppKit main thread, which a bare `cargo test` cannot host
//! unbundled (ADR-0039; unbundled NSWindow construction aborts the process).

#[path = "absent.rs"]
mod absent;
#[path = "android_exit_path.rs"]
mod android_exit_path;
#[path = "backend_conformance.rs"]
mod backend_conformance;
#[path = "contract.rs"]
mod contract;
#[path = "displays.rs"]
mod displays;
#[cfg(not(target_arch = "wasm32"))]
#[path = "executor.rs"]
mod executor;
#[path = "file_store.rs"]
mod file_store;
#[path = "handler_reentry.rs"]
#[cfg(not(target_arch = "wasm32"))]
mod handler_reentry;
#[path = "headless_drag.rs"]
#[cfg(not(target_arch = "wasm32"))]
mod headless_drag;
#[path = "headless_redraw.rs"]
#[cfg(not(target_arch = "wasm32"))]
mod headless_redraw;
#[path = "input_capabilities.rs"]
mod input_capabilities;
#[path = "input_regions.rs"]
mod input_regions;
#[path = "native_recording/mod.rs"]
mod native_recording;
#[path = "preferences.rs"]
mod preferences;
#[path = "window_callback_unwind.rs"]
#[cfg(not(target_arch = "wasm32"))]
mod window_callback_unwind;
#[path = "window_installation.rs"]
mod window_installation;
#[path = "window_state.rs"]
mod window_state;
#[path = "windows_host_events.rs"]
#[cfg(target_os = "windows")]
mod windows_host_events;
#[path = "windows_window_commands.rs"]
#[cfg(target_os = "windows")]
mod windows_window_commands;

/// The Win32 handle of a window the Windows host opened, read the way an
/// embedder reads it: through `raw-window-handle` (ADR-0089 §2), since the
/// host's window type and its `HWND` are not public (ADR-0151 §8).
#[cfg(target_os = "windows")]
fn win32_handle(window: &dyn flui_native::HostWindow) -> windows::Win32::Foundation::HWND {
    let surface = flui_platform::PlatformWindow::surface_target(window);
    let handle = raw_window_handle::HasWindowHandle::window_handle(&*surface)
        .expect("a live Win32 window has a handle");
    let raw_window_handle::RawWindowHandle::Win32(handle) = handle.as_raw() else {
        panic!("not a Win32 window");
    };
    windows::Win32::Foundation::HWND(handle.hwnd.get() as *mut std::ffi::c_void)
}

/// Runs every case even after one fails, then panics listing the failing case names.
fn run_table(table: &str, cases: &[(&str, fn())]) {
    let failed: Vec<&str> = cases
        .iter()
        .filter(|(_, case)| std::panic::catch_unwind(*case).is_err())
        .map(|(name, _)| *name)
        .collect();
    assert!(failed.is_empty(), "{table}: failing cases: {failed:?}");
}

/// A window and the recording handler it was opened with: a test drives the
/// window and sets what each handler method does.
struct Opened {
    window: std::rc::Rc<dyn flui_native::HostWindow>,
    handler: std::rc::Rc<flui_testing::window_handler::RecordingWindowHandler>,
}

impl Opened {
    /// A window `platform` opens with `options`, delivering to a fresh
    /// handler.
    fn on(
        platform: &(impl flui_native::Platform + ?Sized),
        options: flui_platform::WindowOptions,
    ) -> Result<Self, flui_native::OpenWindowError> {
        let handler = flui_testing::window_handler::RecordingWindowHandler::new();
        let window = platform.open_window(options, handler.clone())?;
        Ok(Self { window, handler })
    }

    /// A window `owner` opens at once with `options`, delivering to a fresh
    /// handler.
    #[cfg(target_os = "windows")]
    fn ready(owner: &flui_native::OwnerPlatform, options: flui_platform::WindowOptions) -> Self {
        let handler = flui_testing::window_handler::RecordingWindowHandler::new();
        let window = owner
            .open_window(options, handler.clone())
            .expect("the window opens")
            .try_ready()
            .expect("the window opens at once");
        Self { window, handler }
    }
}

impl std::ops::Deref for Opened {
    type Target = std::rc::Rc<dyn flui_native::HostWindow>;

    fn deref(&self) -> &Self::Target {
        &self.window
    }
}
