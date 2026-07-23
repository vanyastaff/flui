# flui-platform

**The platform abstraction layer** — the boundary between FLUI's portable
pipeline and the operating system's windowing, input, and event-loop APIs.

`flui-platform` defines the `Platform` trait family and ships one backend per
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
traits/          Platform, PlatformWindow, PlatformDisplay, PlatformExecutor,
                 PlatformCapabilities, PlatformEmbedder,
                 input (PlatformInput, velocity tracking)
shared/          PlatformHandlers / WindowCallbacks — callback registries
platforms/       windows, macos, headless, winit (+ linux/android/ios/web stubs)
executor.rs      BackgroundExecutor (tokio pool)
window.rs        Window API surface — binding entry for platform integrators
config.rs        WindowConfiguration, fullscreen/monitor selection
```

- **Windows** — creation, bounds, appearance, fullscreen, and `WindowEvent`
  dispatch (`Resized`, `ScaleFactorChanged`, `RedrawRequested`, ...).
- **Events** — native input converted to W3C-compliant `ui-events` types
  (`PointerEvent`, `KeyboardEvent`) wrapped in `PlatformInput`.
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

Tests are currently **excluded from the CI nextest run** pending the
Windows-only `STATUS_HEAP_CORRUPTION` investigation (tracked as Cross.P /
item H9 in [`docs/ROADMAP-TRACKER.md`](../../docs/ROADMAP-TRACKER.md)). The
crash does not reproduce on Linux checkouts, where only the headless backend
compiles; lib tests pass locally on the headless backend.

## Documentation

Every public item is documented (`#![deny(missing_docs)]`); build locally with
`cargo doc -p flui-platform --open`. Architecture context lives in
[`docs/FOUNDATIONS.md`](../../docs/FOUNDATIONS.md).

## License

MIT OR Apache-2.0, per the workspace license.
