### Added

- `flui_platform::Opened::overflow` (an `OpenedOverflow`) counts the URLs and files `Opened::new` dropped to stay within its bounds; the native host logs it once it accepts the report.

### Changed

- `flui_platform` no longer depends on `tracing`: the contract reports nothing on its own. `commit_composition_in_place` now returns the store's refusal of the edit that ends the composition when it ran at once, as `project_ime_event` already did; an edit a store queued and then refuses stays with the store.
- What launched the application is joined only up to `MAX_OPENED_ITEMS` items: a further cold report reaches `AppHandler::opened` after `ready` instead of being dropped.

### Removed

- `Platform::active_window` and `OwnerPlatform::active_window`, which nothing above the host read.
- `Platform::keyboard_layout`, `Platform::on_keyboard_layout_change` and `OwnerPlatform::keyboard_layout`: the keyboard layout stays inside the native host until the menu feature publishes it with its consumer.
- `TargetPlatform::is_mobile`, `is_desktop` and `is_touch_primary`, and `PlatformInput::as_drag_drop`, which nothing read: whether input is touch-first belongs to the device, not to the operating system.
