### Changed

- `CursorImage` is one shared pointer, so a `Cursor` and a hit target's `CursorRequest` are 16
  bytes rather than 32; `CursorImage::width`, `height` and `hotspot` are no longer `const fn`.

### Fixed

- A `RawButton` over a `Text` leaves 4349 bytes live per button again under the perf baseline
  (`perf_raw_button_over_text`), after an image-capable cursor in every mouse region had raised
  it to 4365.
