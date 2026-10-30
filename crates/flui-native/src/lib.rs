//! Platform abstraction layer for FLUI
//!
//! This crate provides a complete platform abstraction for the FLUI framework,
//! enabling cross-platform UI development with a unified API.
//!
//! # Architecture
//!
//! The architecture is inspired by GPUI's platform layer:
//!
//! ```text
//! flui-platform         - Contracts: PlatformWindow, capability traits,
//!                             window/input and data-transfer vocabulary
//!                             (re-exported here)
//!
//! flui-native
//!   ├─ platform.rs          - Central Platform trait and HostPlatform<H>,
//!   │                         its one implementation
//!   ├─ host_window.rs       - HostWindow: PlatformWindow + accessibility
//!   ├─ host.rs              - A host's table: one type per position, and
//!   │                         Absent<F> for a position a platform lacks
//!   ├─ hosts/               - One table per host and its constructor
//!   ├─ owner/               - The owner thread (ADR-0039) and each platform's
//!   │                         loop: the capability, its signal and slot, the
//!   │                         panic boundary, the one call point for handlers
//!   ├─ version/             - The OS version floors and gates (ADR-0244)
//!   │
//!   ├─ window/              - A feature each (ADR-0151 §10): its model beside
//!   ├─ input/                 one file per platform (windows, apple,
//!   ├─ text/                  android, web, linux, winit, headless)
//!   ├─ display/
//!   ├─ clipboard/, drag/
//!   ├─ accessibility/
//!   ├─ haptics/
//!   ├─ preferences/
//!   ├─ lifecycle/
//!   │
//!   └─ storage/             - Where an application keeps its data
//! ```
//!
//! # Key Concepts
//!
//! ## Platform Trait
//!
//! The [`Platform`] trait is the central abstraction that all platform
//! implementations must provide. It covers:
//!
//! - **Lifecycle**: Event loop, quit
//! - **Windows**: Creation, management, events
//! - **Displays**: Monitor enumeration and information
//! - **Executor**: Thread-safe background task execution
//! - **Clipboard**: Read/write operations
//! - **Callbacks**: Event handler registration
//!
//! ## Platform Selection
//!
//! Use [`current_platform()`] to get the appropriate platform for the current
//! environment:
//!
//! ```rust,ignore
//! use flui_native::current_platform;
//!
//! let platform = current_platform()?;
//! // `App` implements `AppHandler`: its `ready` is the bootstrap.
//! platform.run(Rc::new(App::default()))?;
//! ```
//!
//! ## Testing with Headless Mode
//!
//! The [`HeadlessPlatform`] provides a mock implementation perfect for
//! CI/testing without requiring a display server, GPU, or OS windowing system.
//!
//! ### Direct Usage
//!
//! ```rust
//! use flui_native::{Platform, headless_platform};
//!
//! let platform = headless_platform();
//! assert_eq!(platform.name(), "Headless");
//! ```
//!
//! ### Environment Variable (Recommended for CI)
//!
//! Set `FLUI_HEADLESS=1` to force headless mode via [`current_platform()`]:
//!
//! ```bash
//! # Run tests in headless mode
//! FLUI_HEADLESS=1 cargo test
//!
//! # CI configuration
//! - name: Run tests
//!   run: cargo test
//!   env:
//!     FLUI_HEADLESS: 1
//! ```
//!
//! ```rust,ignore
//! use flui_native::current_platform;
//!
//! // Returns HeadlessPlatform when FLUI_HEADLESS=1
//! let platform = current_platform()?;
//! assert_eq!(platform.name(), "Headless");
//! ```
//!
//! ### What Headless Mode Provides
//!
//! - **Mock Windows**: `open_window()` returns mock windows (no OS windows
//!   created)
//! - **In-Memory Clipboard**: a clipboard that reads back what the application wrote
//! - **Mock Displays**: Single virtual display at 1920x1080
//! - **Background Executor**: Async task execution with tokio runtime
//! - **Fast Tests**: <100ms overhead, suitable for rapid test iteration
//! - **Parallel Safe**: Thread-safe, no race conditions in parallel test
//!   execution
//!
//! ### Example Test
//!
//! ```rust
//! use flui_native::headless_platform;
//! use flui_platform::WindowOptions;
//! use flui_foundation::geometry::Size;
//! use flui_testing::window_handler::RecordingWindowHandler;
//!
//! fn test_window_creation() {
//!     let platform = headless_platform();
//!
//!     let options = WindowOptions::default()
//!         .with_title("Test")
//!         .with_size(Size::new(800.0, 600.0))
//!         .with_visible(true);
//!
//!     let handler = RecordingWindowHandler::new();
//!     let window = platform
//!         .open_window(options, handler.clone())
//!         .expect("Failed to create window");
//!     // The handler's first input is the window's capability snapshot.
//!     assert!(!handler.inputs().is_empty());
//!     // Window is a mock, no actual OS resources allocated
//! }
//! ```
//!
//! # Feature Flags
//!
//! - `default` (`desktop`): the native desktop backend for the target OS, on
//!   Linux the Wayland and X11 hosts (`wayland`, `x11`). It does not include
//!   winit.
//! - `winit-backend`: the winit backend, which `current_platform` picks on
//!   Linux while it is built. It brings `wayland` and `x11`.
//! - `wayland`: on Linux, the native Wayland host (`WaylandPlatform`) and the
//!   client the display source reads the compositor's outputs with. `x11`: the
//!   native X11 host (`X11Platform`) and the display source's RandR client. On
//!   other targets they add nothing.
//! - `testing`: the `testing` module, test support for a native host's own
//!   windows (ADR-0243). Not part of the release API.
//!
//! Accessibility has no feature: Win32, AppKit, Wayland, X11 and Linux winit
//! windows always hand out their operating system's AccessKit adapter. The headless testing
//! platform needs no feature.
//!
//! # Input capabilities
//!
//! There is no per-platform capability constant (ADR-0213). A capability with
//! a handle answers through it (`PlatformWindow::haptics`, `text_input`); the
//! rest arrive per window as `PlatformInput::Capabilities`: first as a
//! snapshot, the first input of the handler the window was opened with,
//! delivered before the open returns, then as changes in order with the input
//! around them.

// Platform FFI is the sanctioned `unsafe` boundary of the workspace (Win32 /
// AppKit / UIKit / Android NDK / wasm-bindgen — `headless` has none). The
// crate does not opt the whole tree out of `unsafe_code = "warn"`: each FFI
// island carries its own `expect(unsafe_code)` where it is declared — the
// loops `owner::windows`, `owner::apple`, `owner::web`, `owner::winit` and
// `owner::android::java_view`, and the platform files of the features that
// call the OS directly (`window::windows`, `input::windows`,
// `text::windows::tsf`, `display::apple::{macos, ios}`, `preferences::windows`
// and the like) — and the reading of the Windows build in `version::gate` is
// one item-level `expect`. A platform file's pure half forbids it. The Win32
// files are fully SAFETY-documented — every `unsafe` block there carries a
// `// SAFETY:` comment (or, for `unsafe fn`, a `# Safety` rustdoc section)
// stating the invariant that makes it sound. The AppKit, Android and web files
// are NOT yet at that bar; see the per-crate documentation coverage tracked in
// the root `Cargo.toml`'s `undocumented_unsafe_blocks` note — do not restate
// "every block" as a crate-wide claim until that note says so too.
// Ship bar (wave 4): every public item is documented; keep it that way.
#![deny(missing_docs)]

pub(crate) mod accessibility;
pub(crate) mod clipboard;
mod display;
pub(crate) mod drag;
pub mod error;
// Reachable for the backend conformance suite, which reads every backend's
// haptics degradation table (`tests/backend_conformance.rs`); not API.
#[cfg(not(target_arch = "wasm32"))]
pub mod executor;
#[doc(hidden)]
pub mod haptics;
// Reachable so a crate outside can show that a table missing a position does
// not compile (`tests/ui/host_table_names_every_position.rs`); not an embedding
// API: nothing outside the crate can build a `HostPlatform`.
#[doc(hidden)]
pub mod host;
mod host_window;
mod hosts;
// Reachable for the backend conformance table and the native recordings in
// `tests/`, which decode through the pointer assembly and the key rules, and
// for the allocation test, its own binary; not an embedding API. The event
// timeline, which the composition root stamps frames on, is named at the root.
#[doc(hidden)]
pub mod input;
pub use input::event_timestamp_ns;
// The native input journal (ADR-0215 §4): reachable for the native
// recordings in `tests/`, which read, write and replay recordings through its
// codec and capture sessions with its recorder; not an embedding API.
#[doc(hidden)]
pub mod journal;
mod lifecycle;
pub mod os;
mod owner;
mod platform;
mod preferences;
#[cfg(any(windows, unix))]
mod process;
pub mod storage;
#[cfg(test)]
mod table_test;
pub mod task;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
// Reachable for the text position's trait, which bounds a host's table; not
// an embedding API.
#[doc(hidden)]
pub mod text;
pub use text::sizing::{
    CapturedTextSizing, TextSizingCaptureError, TextSizingCaptureState, capture_text_sizing,
};
mod version;
mod window;

// The acknowledgement a host waits for before it publishes a window it
// attached (a UIKit scene's), which the composition root completes.
pub use window::installation::{
    InstallationCancelled, PendingWindowInstallation, WindowInstallation,
};
// The typed lifecycle/service error taxonomy — `anyhow` is retired from
// this crate's public API (the window-open family lives with its
// capability types in `owner`).
pub use error::{BootstrapError, PlatformError, PlatformResult};
pub use host::NativeClipboard;
// Re-export executor types
#[cfg(not(target_arch = "wasm32"))]
pub use executor::BackgroundExecutor;
// Each host's platform: the one implementation of `Platform` over its table.
pub use hosts::HeadlessPlatform;
#[cfg(target_os = "ios")]
pub use hosts::IOSPlatform;
#[cfg(target_os = "macos")]
pub use hosts::MacOSPlatform;
pub use platform::HostPlatform;
// Web platform
#[cfg(target_arch = "wasm32")]
pub use hosts::WebPlatform;
// Desktop platforms. The native Linux hosts are built with the `wayland` and
// `x11` features (both in `desktop`); `current_platform` picks the session's.
#[cfg(wayland_host)]
pub use hosts::WaylandPlatform;
#[cfg(windows)]
pub use hosts::WindowsPlatform;
#[cfg(x11_host)]
pub use hosts::X11Platform;
// winit fallback backend — primary on Linux until native Wayland/X11 lands
// (roadmap Cross.P)
#[cfg(all(
    feature = "winit-backend",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
pub use hosts::WinitPlatform;
// The headless host's test controls and doubles, each its feature's.
pub use accessibility::headless::FakeAccessibility;
pub use display::headless::{
    DisplayChange, DisplayChangeError, DisplaySpec, HeadlessDisplayControls,
};
pub use drag::headless::HeadlessDragAndDrop;
pub use input::headless::HeadlessInputDevices;
pub use lifecycle::headless::HeadlessHostEvents;
pub use owner::headless::{
    HeadlessDeferredWindowOpens, HeadlessExitReevaluation, HeadlessOwnerTurns,
};
pub use preferences::headless::HeadlessPreferences;
pub use text::headless::FakeTextInput;
pub use window::headless::{
    HeadlessOpenWindows, HeadlessWindowPolicy, HeadlessWindowSystem, MockWindow, UnknownDisplay,
};
// What a UIKit scene tells the composition root.
#[cfg(target_os = "ios")]
pub use owner::apple::ios::{IOSSceneAttachmentId, IOSSceneSessionId};
#[cfg(target_os = "ios")]
pub use window::apple::ios::IOSSceneEvent;
// The frame request a host stamps on the event clock (`owner/handlers.rs`).
pub use owner::handlers::{frame_request_aged, frame_request_now, frame_request_timed};
// Re-export task types
pub use task::{Task, TaskLabel};
// The core host's own traits; the contract's names are imported from
// `flui_platform`, the accessibility capability from `flui_semantics::platform`.
// Each has this one path (ADR-0151 §9).
pub use host_window::HostWindow;
pub use platform::{AppHandler, ExitAnswer, PathPromptOptions, Platform, PlatformExecutor};
#[cfg(any(windows, unix))]
pub use process::end_process;
// The owner-thread capability (ADR-0039 §1): minted only by a backend,
// handed to `on_ready`, never re-exported with a public minting seam.
pub use owner::{
    OpenWindowError, OpenedWindow, OpenedWindowReach, OwnerPlatform, PendingWindow, PlatformProxy,
    ProxySendError, SharedPlatform, WaitError, WakeRegistrationError, WindowHandlerFactory,
    WindowOpen, WindowRequest,
};

/// Get the current platform implementation
///
/// Automatically selects the correct platform based on the target OS at compile
/// time. This is the recommended way to obtain a platform instance in
/// cross-platform code.
///
/// # Detection Logic
///
/// The platform selection follows a two-stage process:
///
/// 1. **Runtime Environment Check** (executed first):
///    - Checks `FLUI_HEADLESS` environment variable
///    - If set (any value), returns `HeadlessPlatform` immediately
///    - Bypasses all compile-time OS detection
///    - Used for CI/testing without GPU or display server
///
/// 2. **Compile-Time OS Detection** (if not headless):
///    - Uses Rust's `#[cfg]` attributes to select platform at compile time
///    - Selection order (first match wins):
///      - `cfg(windows)` → `WindowsPlatform`
///      - `cfg(target_os = "macos")` → `MacOSPlatform`
///      - `cfg(target_os = "linux")` → chosen at run time: `WinitPlatform`
///        while the `winit-backend` feature is built, else `WaylandPlatform`
///        for a Wayland session and `X11Platform` for an X11 one
///      - `cfg(target_os = "android")` → an error: the Android host starts
///        over the Activity `android_main` is handed (`android_platform`)
///      - `cfg(target_os = "ios")` → `IOSPlatform`
///      - `cfg(target_arch = "wasm32")` → `WebPlatform`
///    - Conditional guards prevent multiple platforms being compiled
///    - Results in zero runtime overhead (selection happens at compile time)
///
/// # Environment Variables
///
/// - **FLUI_HEADLESS=1**: Forces headless mode for CI/testing (overrides OS
///   detection)
///
/// # Platform Selection
///
/// - **Headless** (if `FLUI_HEADLESS=1`): Returns `HeadlessPlatform` - testing
///   mode
/// - **Windows**: Returns `WindowsPlatform` - fully implemented with Win32 API
/// - **macOS**: Returns `MacOSPlatform` - the native AppKit backend: windows,
///   input, clipboard, IME, displays, and a vsync-paced frame loop
/// - **Linux**: Returns `WinitPlatform` while the `winit-backend` feature is
///   built (`flui-app` builds it on Linux until the native hosts carry input,
///   ADR-0151 section 11); otherwise `WaylandPlatform` when `WAYLAND_DISPLAY`
///   names a compositor, `X11Platform` when `DISPLAY` names an X server, and
///   an error for no session.
/// - **Android**: Returns an error: the host needs the Activity, which the
///   system hands only to `android_main`; `android_platform(app)` starts it
/// - **iOS**: Returns `IOSPlatform` - the native UIKit backend: windows, touch
///   input, clipboard, displays, a `CADisplayLink` frame source, and the
///   background/foreground lifecycle (verified on a real simulator)
/// - **Web/WASM**: Returns `WebPlatform` - stub (unimplemented, roadmap
///   available)
///
/// # Platform Status
///
/// | Platform | Status | Quality | Features |
/// |----------|--------|---------|----------|
/// | Windows | ✅ Production | 10/10 | Full featured; no executing coverage in CI (lint-only via `cross-typecheck`) |
/// | macOS | ✅ Native AppKit | 9/10 | Windowing, input, clipboard, IME, displays, vsync-paced frame loop; lint-only + bundled probes locally, no CI test job |
/// | Linux | 🪟 winit fallback (`winit-backend`) | 5/10 | Windowing + input; the native Wayland and X11 hosts have windows, displays, preferences, lifecycle and AT-SPI, no input yet |
/// | Android | 📋 Stub | 2/10 | Roadmap complete |
/// | iOS | ✅ Native UIKit | 7/10 | Windowing, touch, clipboard, displays, `CADisplayLink` frame source; lint-only in CI (simulator verified) |
/// | Web | 📋 Stub | 2/10 | Roadmap complete |
///
/// # Errors
///
/// Returns [`PlatformError::Init`] if:
/// - Platform initialization fails (e.g., COM failure on Windows)
/// - Platform is not supported (should not happen with cfg guards)
/// - It is called on Android, whose host starts over the Activity
///   (`android_platform`)
/// - Linux is reached without `winit-backend` and with no display server its
///   native hosts can connect to
///
/// # Examples
///
/// ```rust,ignore
/// use flui_native::current_platform;
///
/// // Get platform and run event loop
/// let platform = current_platform()?;
/// println!("Running on: {}", platform.name());
///
/// // `App` implements `AppHandler`: its `ready` is the bootstrap.
/// platform.run(Rc::new(App::default()))?;
/// ```
///
/// # Platform-Specific Code
///
/// What one OS offers beyond the window contract is an extension of
/// [`HostWindow`], which answers `None` for a window of another host:
///
/// ```rust,ignore
/// let platform = current_platform()?;
/// let window = platform.open_window(WindowOptions::default(), handler)?;
///
/// #[cfg(windows)]
/// {
///     use flui_native::os::windows::{WindowsBackdrop, WindowsWindowExt};
///     // A Win32 window's view; `None` under `FLUI_HEADLESS`.
///     if let Some(win32) = window.win32() {
///         win32.set_backdrop(WindowsBackdrop::Mica)?;
///     }
/// }
/// ```
pub fn current_platform() -> Result<Box<dyn Platform>, PlatformError> {
    // Check for headless mode via environment variable (CI/testing)
    if std::env::var("FLUI_HEADLESS").is_ok() {
        tracing::info!("FLUI_HEADLESS detected, using headless platform");
        return Ok(Box::new(HeadlessPlatform::new()));
    }

    #[cfg(windows)]
    {
        Ok(Box::new(WindowsPlatform::new()?))
    }

    #[cfg(all(target_os = "macos", not(windows)))]
    {
        Ok(Box::new(MacOSPlatform::new()?))
    }

    #[cfg(all(target_os = "linux", not(any(windows, target_os = "macos"))))]
    {
        let built = LinuxHosts {
            wayland: cfg!(feature = "wayland"),
            x11: cfg!(feature = "x11"),
            winit: cfg!(feature = "winit-backend"),
        };
        match choose_linux_host(&display::linux::protocol::Environment::current(), built) {
            Ok(LinuxHost::Winit) => start_winit(),
            Ok(LinuxHost::Wayland) => start_wayland(),
            Ok(LinuxHost::X11) => start_x11(),
            Err(message) => Err(PlatformError::Init { message }),
        }
    }

    #[cfg(all(
        target_os = "android",
        not(any(windows, target_os = "macos", target_os = "linux"))
    ))]
    {
        // The Android host needs the Activity, which the system hands only to
        // `android_main`: `android_platform(app)` starts it.
        Err(PlatformError::Init {
            message: "On Android, start the platform with `flui_native::android_platform(app)` \
                      from android_main() instead of current_platform()"
                .to_string(),
        })
    }

    #[cfg(all(
        target_os = "ios",
        not(any(
            windows,
            target_os = "macos",
            target_os = "linux",
            target_os = "android"
        ))
    ))]
    {
        Ok(Box::new(IOSPlatform::new()?))
    }

    #[cfg(all(
        target_arch = "wasm32",
        not(any(
            windows,
            target_os = "macos",
            target_os = "linux",
            target_os = "android",
            target_os = "ios"
        ))
    ))]
    {
        Ok(Box::new(WebPlatform::new()?))
    }

    #[cfg(not(any(
        windows,
        target_os = "macos",
        target_os = "linux",
        target_os = "android",
        target_os = "ios",
        target_arch = "wasm32"
    )))]
    {
        Err(PlatformError::Init {
            message: "Unsupported platform - no platform implementation available for this target"
                .to_string(),
        })
    }
}

/// The winit host, as [`current_platform`] starts it on Linux.
#[cfg(all(target_os = "linux", feature = "winit-backend"))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the signature a build without the winit host answers with an error"
)]
fn start_winit() -> Result<Box<dyn Platform>, PlatformError> {
    Ok(Box::new(WinitPlatform::new()))
}

/// A build without the winit host never chooses it.
#[cfg(all(target_os = "linux", not(feature = "winit-backend")))]
fn start_winit() -> Result<Box<dyn Platform>, PlatformError> {
    Err(PlatformError::Init {
        message: "the winit host is not built".to_owned(),
    })
}

/// The native Wayland host, as [`current_platform`] starts it.
#[cfg(all(target_os = "linux", feature = "wayland"))]
fn start_wayland() -> Result<Box<dyn Platform>, PlatformError> {
    Ok(Box::new(WaylandPlatform::new()?))
}

/// A build without the Wayland host never chooses it.
#[cfg(all(target_os = "linux", not(feature = "wayland")))]
fn start_wayland() -> Result<Box<dyn Platform>, PlatformError> {
    Err(PlatformError::Init {
        message: "the Wayland host is not built".to_owned(),
    })
}

/// The native X11 host, as [`current_platform`] starts it.
#[cfg(x11_host)]
fn start_x11() -> Result<Box<dyn Platform>, PlatformError> {
    Ok(Box::new(X11Platform::new()?))
}

/// A build without the X11 host never chooses it.
#[cfg(all(target_os = "linux", not(feature = "x11")))]
fn start_x11() -> Result<Box<dyn Platform>, PlatformError> {
    Err(PlatformError::Init {
        message: "the X11 host is not built".to_owned(),
    })
}

/// The Linux hosts a build carries.
#[cfg(any(target_os = "linux", test))]
#[derive(Debug, Clone, Copy)]
struct LinuxHosts {
    /// The native Wayland host (`wayland`).
    wayland: bool,
    /// The native X11 host (`x11`).
    x11: bool,
    /// The winit host (`winit-backend`).
    winit: bool,
}

/// A Linux host [`current_platform`] can start.
#[cfg(any(target_os = "linux", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinuxHost {
    /// The native Wayland host.
    Wayland,
    /// The native X11 host.
    X11,
    /// The winit host.
    Winit,
}

/// The host [`current_platform`] starts on Linux, chosen at run time from the
/// session `environment` names, by the one rule every Linux connection to a
/// compositor follows (`display::linux::protocol`), and the hosts the build
/// carries.
///
/// The winit host is chosen whenever it is built: the native Wayland host
/// carries pointer and key input but no text input, clipboard or drag and
/// drop yet, the native X11 host no input at all, and `flui-app` builds winit
/// on Linux until they do (ADR-0151 §11). Without it, a Wayland session gets the native Wayland
/// host and an X11 session the native X11 host; a Wayland session that also
/// names an X server gets the X11 host when only that one is built.
#[cfg(any(target_os = "linux", test))]
fn choose_linux_host(
    environment: &display::linux::protocol::Environment,
    built: LinuxHosts,
) -> Result<LinuxHost, String> {
    use display::linux::protocol::Protocol;

    if built.winit {
        return Ok(LinuxHost::Winit);
    }
    let set = |value: &Option<String>| value.as_deref().is_some_and(|text| !text.is_empty());
    match Protocol::from_environment(environment) {
        Some(Protocol::Wayland) if built.wayland => Ok(LinuxHost::Wayland),
        // A Wayland session that serves X11 clients too (Xwayland), with only
        // the X11 host built.
        Some(Protocol::Wayland) if built.x11 && set(&environment.display) => Ok(LinuxHost::X11),
        Some(Protocol::X11) if built.x11 => Ok(LinuxHost::X11),
        None if set(&environment.wayland_socket) => Err(
            "only WAYLAND_SOCKET is set: the Wayland host connects to the socket WAYLAND_DISPLAY \
             names, as its display source does"
                .to_owned(),
        ),
        Some(Protocol::Wayland) => Err(
            "the session is Wayland and the Wayland host is not built: enable the `wayland` \
             feature (part of `desktop`)"
                .to_owned(),
        ),
        Some(Protocol::X11) => Err(
            "the session is X11 and the X11 host is not built: enable the `x11` feature (part of \
             `desktop`)"
                .to_owned(),
        ),
        None => Err(
            "neither WAYLAND_DISPLAY names a socket nor DISPLAY is set: there is no display server \
             to open windows on (FLUI_HEADLESS=1 selects the headless host)"
                .to_owned(),
        ),
    }
}

/// Create a headless platform for testing
///
/// This is a convenience function for tests. Prefer using this over
/// `current_platform()` in test code for clarity.
///
/// # Example
///
/// ```rust
/// use flui_native::headless_platform;
///
/// let platform = headless_platform();
/// assert_eq!(platform.name(), "Headless");
/// ```
pub fn headless_platform() -> Box<dyn Platform> {
    Box::new(HeadlessPlatform::new())
}

/// The Android platform over the Activity the system handed `android_main`.
///
/// The one public item of the core host that names an `android-activity`
/// type: the system hands the Activity only to the application's
/// `android_main`, and the composition root passes it on here, as the
/// facade's `run_app_android` takes it (ADR-0151 §8). Nothing the platform
/// hands back names it.
///
/// ```rust,ignore
/// #[unsafe(no_mangle)]
/// fn android_main(app: android_activity::AndroidApp) {
///     let platform = flui_native::android_platform(app);
///     // `App` implements `AppHandler`: its `ready` is the bootstrap.
///     let _ = platform.run(Rc::new(App::default()));
/// }
/// ```
#[cfg(target_os = "android")]
#[must_use]
pub fn android_platform(app: android_activity::AndroidApp) -> Box<dyn Platform> {
    Box::new(HostPlatform::<hosts::android::Android>::new(app))
}

#[cfg(test)]
mod linux_host_tests {
    use super::{LinuxHost, LinuxHosts, choose_linux_host};
    use crate::display::linux::protocol::Environment;

    fn session(wayland: &str, socket: &str, display: &str) -> Environment {
        let some = |text: &str| Some(text.to_owned());
        Environment {
            wayland_display: some(wayland),
            wayland_socket: some(socket),
            display: some(display),
            runtime_dir: some("/run/user/1000"),
        }
    }

    /// The Linux host is chosen at run time from the session: winit while it
    /// is built, otherwise the native host of the session's protocol (the X11
    /// host for a Wayland session that names an X server too, when only that
    /// one is built), and an error that says what is missing where none fits.
    /// An inherited descriptor (`WAYLAND_SOCKET`) is no socket the host and
    /// its display source can both connect to.
    #[test]
    fn the_linux_host_follows_the_session() {
        let hosts = |wayland, x11, winit| LinuxHosts {
            wayland,
            x11,
            winit,
        };
        let native = hosts(true, true, false);
        let only_wayland = hosts(true, false, false);
        let only_x11 = hosts(false, true, false);
        let all = hosts(true, true, true);
        let none = hosts(false, false, false);
        let wayland = session("wayland-0", "", ":0");
        let wayland_alone = session("wayland-0", "", "");
        let x11 = session("", "", ":0");
        for (case, environment, built, chosen) in [
            ("Wayland", &wayland, native, LinuxHost::Wayland),
            (
                "Wayland, X11 alone built",
                &wayland,
                only_x11,
                LinuxHost::X11,
            ),
            ("X11", &x11, native, LinuxHost::X11),
            ("X11, X11 alone built", &x11, only_x11, LinuxHost::X11),
            ("winit built", &wayland, all, LinuxHost::Winit),
            (
                "winit built, no session",
                &session("", "", ""),
                all,
                LinuxHost::Winit,
            ),
        ] {
            assert_eq!(choose_linux_host(environment, built), Ok(chosen), "{case}");
        }
        for (case, environment, built) in [
            ("X11, the host not built", &x11, only_wayland),
            (
                "Wayland with no X server, X11 alone built",
                &wayland_alone,
                only_x11,
            ),
            ("no session", &session("", "", ""), native),
            (
                "only an inherited descriptor",
                &session("", "7", ""),
                native,
            ),
            ("Wayland, no host built", &wayland, none),
        ] {
            assert!(choose_linux_host(environment, built).is_err(), "{case}");
        }
    }
}
