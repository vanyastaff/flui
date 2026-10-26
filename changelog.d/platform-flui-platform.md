### Changed

- The contract crate `flui-platform-api` is now `flui-platform`
  ([ADR-0151](/docs/adr/ADR-0151-platform-layer-boundary-and-names.md)); the facade keeps
  `flui::platform`. A branch written before both renames applies the core host's map first
  (`flui-platform` to `flui-native`, `flui_platform` to `flui_native`, outside the contract's name),
  then this one (`flui-platform-api` to `flui-platform`, `flui_platform_api` to `flui_platform`).
