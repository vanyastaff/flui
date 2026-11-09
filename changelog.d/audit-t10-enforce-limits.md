### Added

- `TransferLimits::enforce(actual)` in `flui-platform::data_transfer`: the transport's one size
  predicate, `Ok(())` up to and including `max_bytes` and `Err(TransferError::TooLarge)` past it.
  The headless, Android, web, Windows, X11 and Wayland drag readers refuse an oversized payload
  through it.

### Fixed

- A `DragFetch` from `DragView::request` resolves with `TransferError::TooLarge` for a payload
  past the requested limit on the cross-process path too, not only in process, so a backend that
  delivers more than was asked can no longer hand it to the target.
