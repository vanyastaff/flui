### Changed

- `flui_native::WindowsPlatform` is neither `Send` nor `Sync`: its loop and window set belong to the thread that built them, which the compiler now holds instead of an `unsafe impl` (ADR-0327 §5). Another thread reaches the platform through its proxy.
