### Changed

- `OverlayHandle` and `OverlayEntry` are explicitly owner-local, backed by `Rc`, `RefCell` and `Cell` rather than thread-synchronization primitives.

### Fixed

- Overlay mutations and entry rebuild requests can re-enter from a frame-request hook without deadlocking; nested accepted work reaches the next frame.
