### Removed

- The `flui_native::traits` module
  ([ADR-0151](/docs/adr/ADR-0151-platform-layer-boundary-and-names.md) §10). Its public items
  keep their crate-root paths: `flui_native::{Platform, PlatformExecutor, PlatformReadyCallback,
  PathPromptOptions, HostWindow, OwnerPlatform, SharedPlatform, PlatformProxy, PendingWindow,
  WindowOpen, OpenWindowError, ProxySendError, WaitError, WakeRegistrationError}`.
