### Changed

- **`flui-platform`**: the crate is grouped by area (ADR-0151 §2), and its public modules moved
  under their areas: `pointer` to `flui_platform::input::pointer`, `keyboard` to
  `flui_platform::input::keyboard`, `input_capabilities` to `flui_platform::input::capabilities`,
  `input_regions` to `flui_platform::input::regions`, `text_store` to
  `flui_platform::text::store`, `data_transfer` to `flui_platform::transfer::data`, `drag` to
  `flui_platform::transfer::drag` and `os_options` to `flui_platform::window::os_options`. The
  names re-exported at the crate root keep their paths (`flui_platform::WindowOptions`), and the
  facade's `flui::platform` paths and `flui-sdk`'s are unchanged.
- **`flui-platform`**: `Utf16Offset`, `Utf16Range` and `OffsetError` have one path,
  `flui_platform::text::store::utf16::*`; the store module no longer re-exports them.
