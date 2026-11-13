### Changed

- `flui_native::MacOSPlatform` is neither `Send` nor `Sync`: its loop belongs to the main thread, which the compiler now holds instead of an `unsafe impl` (ADR-0327 §5). Another thread reaches the platform through its proxy.
