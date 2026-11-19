# flui-native

**The platform abstraction layer** — the boundary between FLUI's portable
pipeline and the operating system's windowing, input, and event-loop APIs.

`flui-native` defines the `Platform` trait family and ships one backend per
target. `current_platform()` selects in two stages: a runtime `FLUI_HEADLESS`
environment check first (unset, empty or `0` is off; any other value, `false` included, forces the headless backend — the CI/test
path), then compile-time `#[cfg]` picks the OS backend:

| Backend | Target | Status |
|---------|--------|--------|
| Win32 (`WindowsPlatform`) | Windows | Native, full featured |
| AppKit (`MacOSPlatform`) | macOS | Native, in progress |
| Headless (`HeadlessPlatform`, named with `scripted`) | any (`FLUI_HEADLESS=1`) | Simulated backend for CI and tests |
| X11 (`X11Platform`), Wayland (`WaylandPlatform`) | Linux | Native; Wayland when `WAYLAND_DISPLAY` names a compositor, X11 otherwise |

Part of the [FLUI](https://github.com/vanyastaff/flui) workspace — pre-release,
consumed by path (not published to crates.io).

## Surfaces

```text
platform.rs      Platform, AppHandler, and HostPlatform<H>, its one
                 implementation
host_window.rs   HostWindow: a PlatformWindow plus its accessibility bridge
host.rs, hosts/  a host's table of positions, one table per host
owner/           OwnerPlatform, PlatformProxy; the owner-turn
                 signal, the owner-thread slot, the panic boundary, the
                 delivery to the application's handler (AppDelivery), each
                 window's delivery to its
                 handler (WindowDelivery), and each
                 platform's loop
version/         the OS version floors and the gates above them (ADR-0244)
window/, input/, text/, display/, clipboard/, drag/, accessibility/,
haptics/, preferences/, lifecycle/
                 a feature each: its model beside one file per platform
                 (windows, apple, android, web, linux, headless)
storage/         where an application keeps its data
executor.rs      NativeExecutor (the tokio runtime the hosts run their IO on)
error.rs         PlatformError, BootstrapError
```

No public path names an OS type (ADR-0151 §8): each host's window is the
`Rc<dyn HostWindow>` the platform opens, a raw window or display handle comes
from its `SurfaceTarget` through `raw-window-handle`, and what one OS offers beyond the window contract is
an extension of `HostWindow` (`WindowsWindowExt`, `MacOSWindowExt`) that
answers `None` for another host's window. The Android host starts over the
Activity `android_main` is handed, through `android_platform(app)`.

- **Windows** — creation, bounds, appearance, fullscreen, each fact published
  in the window's state snapshot and announced by `WindowHandler::state_changed`.
- **Events** — native input converted to FLUI-owned pointer and keyboard values
  wrapped in `PlatformInput`. Named-key spellings and pointer fields map the
  applicable W3C vocabularies; native adapters keep their platform contracts
  (ADR-0143), with execution evidence distinct from type compatibility.
- **Executor** — `NativeExecutor` is the tokio runtime a host runs its IO on (the Linux
  session sources and the Wayland title fonts): the one its creator lends with `from_tokio`, which
  `CurrentHost::start_with` takes, or a small one of its own (`own`, what `start` uses).
  UI-owner commands stay private to each backend's bounded, wake-integrated control path.
- **Displays** — monitor enumeration, DPI/scale factors, device↔logical pixel
  conversion.

The headless backend provides headless windows (`HeadlessHostWindow`), an in-memory clipboard, and a
virtual 1920x1080 display so tests run without a display server or GPU —
set `FLUI_HEADLESS=1` or call `headless_platform()` directly. Its type, its windows, its doubles
and the controls a test drives it with are public with the `scripted` feature (which `testing`
includes); without it no public path names a test double.

## Sanctioned unsafe boundary

This crate is the workspace's sanctioned `unsafe` FFI island
(`#![allow(unsafe_code)]`, permanent): Win32 and AppKit calls live here so the
rest of the workspace stays `unsafe`-free. Every unsafe block carries a
`// SAFETY:` comment.

## Testing status

The suite runs under nextest in CI on Linux (the `test` job's dedicated step,
headless + Xvfb). Native Windows/macOS suites run locally through
`cargo nextest run -p flui-native --locked --no-fail-fast -E 'not (group(trybuild))'`,
with default features and with `--all-features`; on macOS with `FLUI_HEADLESS=1`, since a bare
test process cannot own an AppKit run loop (`nightly-platforms.yml` runs both). Run it with
nextest, not `cargo test`: the tests that drive a real event loop expect a
process of their own. macOS and the mobile backends are lint-only in CI
(`cross-typecheck`).

The Win32 clipboard is opened only by its own thread, with its own owner
window (ADR-0337): `OpenClipboard(NULL)` does not exclude other threads of the
same process, and a reader racing a writer on two threads was a use-after-free
that aborted the process with `STATUS_HEAP_CORRUPTION`.

## Documentation

Every public item is documented (`#![deny(missing_docs)]`); build locally with
`cargo doc -p flui-native --open`. Architecture context lives in
[`docs/architecture.md`](../../docs/architecture.md).

## License

MIT OR Apache-2.0, per the workspace license.
