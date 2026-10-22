### Changed

- `flui_native::Platform` and `flui_native::HostWindow` are sealed
  ([ADR-0151](/docs/adr/ADR-0151-platform-layer-boundary-and-names.md) §5): only the core host
  implements them, `Platform` as `HostPlatform<H>` over a host's table and `HostWindow` on the
  windows its hosts build. An outside implementation is a compile error that names the seal; a
  test double implements `flui_platform::PlatformWindow`, which stays open.

### Removed

- The `flui_native::traits` module
  ([ADR-0151](/docs/adr/ADR-0151-platform-layer-boundary-and-names.md) §10). Its public items
  keep their crate-root paths: `flui_native::{Platform, PlatformExecutor, PlatformReadyCallback,
  PathPromptOptions, HostWindow, OwnerPlatform, SharedPlatform, PlatformProxy, PendingWindow,
  WindowOpen, OpenWindowError, ProxySendError, WaitError, WakeRegistrationError}`.
