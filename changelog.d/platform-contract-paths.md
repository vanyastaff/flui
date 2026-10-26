### Changed

- `flui-native` no longer re-exports the contract
  ([ADR-0151](/docs/adr/ADR-0151-platform-layer-boundary-and-names.md) §9): the window, input,
  display, data-transfer, preference, clipboard and cursor names are imported from
  `flui_platform`, the accessibility capability (`PlatformAccessibility` and its listeners) from
  `flui_semantics::platform`. `flui_native::WindowOptions`, `flui_native::traits::PlatformWindow`
  and `flui_native::data_transfer` are gone; `flui_native` keeps `Platform`, `HostWindow`, the
  owner-thread types, the hosts and its errors.
