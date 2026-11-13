### Changed

- `flui_native::WebPlatform` is neither `Send` nor `Sync`: its loop lives on the page's one thread, and the owner signal's wake reaches it there (ADR-0327 §5).
