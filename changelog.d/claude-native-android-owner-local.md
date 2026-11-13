### Changed

- `flui_native::AndroidPlatform` is no longer `Sync`: the loop's window slot and stop flag are cells of the thread that runs `android_main` (ADR-0327 §5). Another thread reaches the platform through its proxy.
