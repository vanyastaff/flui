# flui-native

**The platform abstraction layer** — the boundary between FLUI's portable
pipeline and the operating system's windowing, input, and event-loop APIs.

`flui-native` defines the `Platform` trait family and ships one backend per
target. `current_platform()` selects in two stages: a runtime `FLUI_HEADLESS`
environment check first (any value forces the headless backend — the CI/test
path), then compile-time `#[cfg]` picks the OS backend:

| Backend | Target | Status |
|---------|--------|--------|
| Win32 (`WindowsPlatform`) | Windows | Native, full featured |
| AppKit (`MacOSPlatform`) | macOS | Native, in progress |
| Headless (`HeadlessPlatform`) | any (`FLUI_HEADLESS=1`) | Mock backend for CI/tests |
| winit (`WinitPlatform`) | cross-platform fallback | Primary on Linux; optional elsewhere via `winit-backend` |

Part of the [FLUI](https://github.com/vanyastaff/flui) workspace — pre-release,
consumed by path (not published to crates.io).

## Surfaces

```text
platform.rs      Platform, PlatformExecutor, and HostPlatform<H>, its one implementation
host_window.rs   HostWindow: a PlatformWindow plus its accessibility bridge
host.rs, hosts/  a host's table of positions, one table per host
owner/           OwnerPlatform, SharedPlatform, PlatformProxy; the owner-turn
                 signal, the owner-thread slot, the panic boundary, the
                 PlatformHandlers / WindowCallbacks registries, and each
                 platform's loop
version/         the OS version floors and the gates above them (ADR-0244)
window/, input/, text/, display/, clipboard/, drag/, accessibility/,
haptics/, preferences/, lifecycle/
                 a feature each: its model beside one file per platform
                 (windows, apple, android, web, linux, winit, headless)
storage/         where an application keeps its data
executor.rs      BackgroundExecutor (tokio pool)
error.rs         PlatformError, BootstrapError
```

No public path names an OS type (ADR-0151 §8): each host's window is the
`Arc<dyn HostWindow>` the platform opens, a raw window or display handle comes
from `raw-window-handle`, and what one OS offers beyond the window contract is
an extension of `HostWindow` (`WindowsWindowExt`, `MacOSWindowExt`) that
answers `None` for another host's window. The Android host starts over the
Activity `android_main` is handed, through `android_platform(app)`.

- **Windows** — creation, bounds, appearance, fullscreen, each fact published
  in the window's state snapshot and announced by `on_state_changed`.
- **Events** — native input converted to FLUI-owned pointer and keyboard values
  wrapped in `PlatformInput`. Named-key spellings and pointer fields map the
  applicable W3C vocabularies; native adapters keep their platform contracts
  (ADR-0143), with execution evidence distinct from type compatibility.
- **Executor** — `BackgroundExecutor` runs thread-safe work on a multi-threaded
  tokio runtime. UI-owner commands stay private to each backend's bounded,
  wake-integrated control path.
- **Displays** — monitor enumeration, DPI/scale factors, device↔logical pixel
  conversion.

The headless backend provides mock windows, an in-memory clipboard, and a
virtual 1920x1080 display so tests run without a display server or GPU —
set `FLUI_HEADLESS=1` or call `headless_platform()` directly.

## Sanctioned unsafe boundary

This crate is the workspace's sanctioned `unsafe` FFI island
(`#![allow(unsafe_code)]`, permanent): Win32 and AppKit calls live here so the
rest of the workspace stays `unsafe`-free. Every unsafe block carries a
`// SAFETY:` comment.

## Testing status

The suite runs under nextest in CI on Linux (the `test` job's dedicated step,
headless + Xvfb). Native Windows/macOS suites run locally through
`cargo xtask platform-test`, with default and all features. Run it with nextest, not `cargo test`: the winit tests that drive a
real event loop need one process per test, because winit allows one
`EventLoop` per process. macOS and the mobile backends are lint-only in CI
(`cross-typecheck`).

The Win32 clipboard is opened only under a process-wide lock
(`clipboard::lock`): `OpenClipboard(NULL)` does not exclude other
threads of the same process, and a reader racing a writer on two threads was a
use-after-free that aborted the process with `STATUS_HEAP_CORRUPTION`.

## Documentation

Every public item is documented (`#![deny(missing_docs)]`); build locally with
`cargo doc -p flui-native --open`. Architecture context lives in
[`docs/architecture.md`](../../docs/architecture.md).

## License

MIT OR Apache-2.0, per the workspace license.
