### Changed

- `flui_native::Platform` and the host's `Loop` and `Window::Shared` are no longer `Send + Sync`: a platform stays on the thread that made it, and another thread reaches it through its `PlatformProxy` (ADR-0327 §5).
