### Added

- **`flui-platform`**: one vocabulary for "not here" (ADR-0151 §4): `Unsupported`, built with
  `Unsupported::new(reason)` and never written as a literal, and its `UnsupportedReason`
  (`NotOnThisPlatform`, `NotOnThisDevice`, `NotDeclared { what }`, `Unavailable`,
  `NotRegistered`, `NoWindow`). No reason names an operating-system version. `flui-sdk`
  re-exports both from `platform`.

### Changed

- **`flui-platform`**: `Availability` is named at the crate root
  (`flui_platform::Availability`), not in `input_capabilities`; `flui-sdk` keeps it at
  `platform::input_capabilities::Availability` under `unstable-input`.
