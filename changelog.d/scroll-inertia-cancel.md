### Added

- `TrackpadScript::rest` replays fingers coming to rest on the trackpad.

### Changed

- `Listener::on_pointer_signal` also receives `PointerEvent::ScrollInertiaCancel`.

### Fixed

- A `Scrollable` stops its fling when fingers come to rest on the trackpad, as a press does, and settles through its physics; a notched wheel's easing goes on.
