### Added

- **`flui-widgets`**: `Wrap::vertical_direction` sets the vertical sense of a wrap: with
  `VerticalDirection::Up`, horizontal runs start at the bottom and a vertical run fills from the
  bottom. `VerticalDirection` is now re-exported from `flui_widgets`, so `Flex::vertical_direction`
  is usable without depending on `flui-objects`.
- **`flui-objects`**: `RenderWrap::set_vertical_direction` updates a laid-out wrap, and its
  inspector description lists `text_direction` and `vertical_direction`.
