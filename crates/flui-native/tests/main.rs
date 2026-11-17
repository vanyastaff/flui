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

#![allow(
    clippy::disallowed_methods,
    reason = "test code catches a panic to assert on it (ADR-0303 §5)"
)]
#[path = "absent.rs"]
mod absent;
#[path = "android_exit_path.rs"]
mod android_exit_path;
use flui_native::testing::app;
#[path = "app_handler.rs"]
#[cfg(not(target_arch = "wasm32"))]
mod app_handler;
#[path = "backend_conformance.rs"]
mod backend_conformance;
#[path = "clipboard.rs"]
#[cfg(not(target_arch = "wasm32"))]
mod clipboard;
#[path = "contract.rs"]
mod contract;
#[path = "displays.rs"]
mod displays;
#[path = "file_store.rs"]
mod file_store;
#[path = "gaps.rs"]
mod gaps;
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
#[cfg(not(target_arch = "wasm32"))]
#[path = "launch_values.rs"]
mod launch_values;
#[path = "native_recording/mod.rs"]
mod native_recording;
#[path = "preferences.rs"]
mod preferences;
#[path = "proxy_open_window.rs"]
#[cfg(not(target_arch = "wasm32"))]
mod proxy_open_window;
#[path = "wayland_window.rs"]
#[cfg(all(target_os = "linux", feature = "wayland"))]
mod wayland_window;
#[path = "window_callback_unwind.rs"]
#[cfg(not(target_arch = "wasm32"))]
mod window_callback_unwind;
#[path = "window_installation.rs"]
mod window_installation;
#[path = "window_state.rs"]
mod window_state;
#[path = "windows_foreground.rs"]
#[cfg(target_os = "windows")]
mod windows_foreground;
#[path = "windows_host_events.rs"]
#[cfg(target_os = "windows")]
mod windows_host_events;
#[path = "windows_test_desktop.rs"]
#[cfg(target_os = "windows")]
mod windows_test_desktop;
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

/// Whether this test runs on the desktop that takes the user's input, the one
/// the foreground, the cursor and `SendInput` belong to. nextest starts a test
/// on a desktop of its own, which does not (docs/testing.md, "The Windows test
/// desktop").
#[cfg(target_os = "windows")]
fn on_the_input_desktop() -> bool {
    this_threads_desktop().takes_input
}

/// What the calling thread's desktop is: its name and whether it takes input.
#[cfg(target_os = "windows")]
struct ThreadDesktop {
    name: String,
    takes_input: bool,
}

#[cfg(target_os = "windows")]
#[expect(
    unsafe_code,
    reason = "reads the thread's desktop, its name and whether it takes input"
)]
fn this_threads_desktop() -> ThreadDesktop {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::StationsAndDesktops::{
        GetThreadDesktop, GetUserObjectInformationW, UOI_IO, UOI_NAME,
    };
    use windows::Win32::System::Threading::GetCurrentThreadId;
    let unread = ThreadDesktop {
        name: String::new(),
        takes_input: false,
    };
    // SAFETY: the calling thread's own desktop, which is not closed.
    let Ok(desktop) = (unsafe { GetThreadDesktop(GetCurrentThreadId()) }) else {
        return unread;
    };
    let mut takes_input = windows::core::BOOL(0);
    // SAFETY: `takes_input` is a writable `BOOL`, the size passed.
    let io = unsafe {
        GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_IO,
            Some((&raw mut takes_input).cast()),
            u32::try_from(std::mem::size_of::<windows::core::BOOL>()).expect("a BOOL's size"),
            None,
        )
    };
    let mut name = [0u16; 256];
    // SAFETY: `name` is a writable buffer of the size passed.
    let named = unsafe {
        GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(name.as_mut_ptr().cast()),
            u32::try_from(std::mem::size_of_val(&name)).expect("512 fits a u32"),
            None,
        )
    };
    let len = if named.is_ok() {
        name.iter()
            .position(|&unit| unit == 0)
            .unwrap_or(name.len())
    } else {
        0
    };
    ThreadDesktop {
        name: String::from_utf16_lossy(&name[..len]),
        takes_input: io.is_ok() && takes_input.as_bool(),
    }
}

/// Whether a case that needs the desktop taking input (`what`) runs here. Elsewhere it says it
/// cannot verify and the case returns, or, under `FLUI_REQUIRE_INPUT_DESKTOP=1`, fails.
#[cfg(target_os = "windows")]
fn input_desktop_for(what: &str) -> bool {
    if on_the_input_desktop() {
        return true;
    }
    assert!(
        std::env::var_os("FLUI_REQUIRE_INPUT_DESKTOP").is_none_or(|required| required != "1"),
        "{what} needs the desktop that takes the user's input, which this test does not run on \
         (FLUI_REQUIRE_INPUT_DESKTOP=1)"
    );
    eprintln!(
        "CANNOT_VERIFY {what}: it needs the desktop that takes the user's input \
         (FLUI_TEST_VISIBLE_DESKTOP=1 runs it there)"
    );
    false
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
