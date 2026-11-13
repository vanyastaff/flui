### Changed

- `flui_native::IOSPlatform` is neither `Send` nor `Sync`: its loop keeps its stop flag and owner deadline in `Cell`/`RefCell` on the main thread, which the compiler now holds (ADR-0327 §5). Another thread reaches the platform through its proxy.
