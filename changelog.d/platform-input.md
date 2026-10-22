### Changed

- **`flui-native`**: the input timestamp source is named at the crate root,
  `flui_native::event_timestamp_ns`; `flui_native::shared::events` is gone.

### Removed

- **`flui-native`**: the public `flui_native::shared` module, with
  `shared::events::primary_mouse_info`, which no consumer reached, and the empty
  `flui_native::platforms::web` module. The rule modules the crate's own tests decode
  with, hidden from the documentation and not API, moved under `flui_native::input`
  (`input::{assembly, availability, keys}`, `input::apple::keys`,
  `input::android::keys`, `input::web::keys`,
  `input::windows::direct_manipulation::manipulation`; a touchpad viewport's readings
  are `input::assembly`'s); the wheel normalization (`shared::scroll`) is no longer
  reachable, and `text::android::keys` and `text::web::ime` went with them.
