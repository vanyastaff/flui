### Changed

- **`HitTestEntry::cursor`** (`flui-interaction`) is now `Option<CursorIcon>`: `None` defers to the
  entries further out, and `Some(CursorIcon::Default)` is an explicit arrow that wins over an
  ancestor's cursor. `RenderObject::mouse_cursor` / `RenderBox::mouse_cursor` (`flui-rendering`)
  and `RenderMouseRegion::{cursor, set_cursor}` (`flui-objects`) follow; a `MouseRegion` without
  `.cursor(..)` now defers instead of forcing the arrow.
- **`HitTestResult::with_paint_offset`** returns `Option<R>` and refuses a NaN or infinite offset
  without running the subtree, like `with_paint_transform` (ADR-0113).

### Fixed

- A region hovered by two devices (mouse and pen) now delivers `on_exit` to each device; the first
  device to leave no longer swallows the other's exit.
- `MouseTracker::update_all_devices` delivers every device's enter/exit even when another
  device's hit test or callback panics, then resumes the first panic; a device whose hit test
  panicked is retried on the next refresh.
- A mouse region's callbacks released by the tracker are destroyed outside its borrow, so a
  capture destructor can use the tracker.
- Pointer events delivered to a transformed hit entry localize coalesced and predicted samples
  and the scroll delta (as a vector: rotated and scaled, not translated); `dispatch_scroll`
  localizes `ScrollEventData::delta` the same way.
