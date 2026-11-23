### Added

- `flui_foundation::panic::FirstPanic::retire`, dropping a value under the collector's containment:
  a destructor that panics is held as the first failure unless an earlier one is.
