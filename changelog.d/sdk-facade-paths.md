### Added

- `flui::foundation::memory::MemoryPressureLevel`, the level `HostNotice::MemoryPressure` carries,
  and `flui::interaction::ContactEnd`, which `PrimaryContact::finish` takes: an application can
  now name both.

### Changed

- `flui-sdk` places every item at the facade's path. The painting values move from the root of
  `flui_sdk::painting` into `painting::paint`, `painting::styling` and `painting::typography`
  (`flui_sdk::painting::Color` is now `flui_sdk::painting::styling::Color`), and the text input
  options from the root of `flui_sdk::platform` into `platform::text_store`.

### Removed

- `flui_sdk::painting::TextDirection` and `flui_sdk::platform::MemoryPressureLevel`, second paths
  to `flui_sdk::foundation::TextDirection` and `flui_sdk::foundation::memory::MemoryPressureLevel`.
