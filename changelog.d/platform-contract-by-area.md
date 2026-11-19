### Changed

- **`flui-platform`**: the crate is grouped by area (ADR-0151 §2), and its public modules moved
  under their areas: `pointer`, `keyboard`, `input_capabilities` and `input_regions` to
  `flui_platform::input::*`, `text_store` to `flui_platform::text::text_store`, `data_transfer`
  and `drag` to `flui_platform::transfer::*`, and `os_options` to
  `flui_platform::window::os_options`. The names re-exported at the crate root keep their paths
  (`flui_platform::WindowOptions`), and the facade's `flui::platform` paths and `flui-sdk`'s are
  unchanged.
- **`flui-platform`**: `Utf16Offset`, `Utf16Range` and `OffsetError` have one path,
  `flui_platform::text::text_store::utf16::*`; `text_store` no longer re-exports them.
