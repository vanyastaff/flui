### Changed

- `flui_platform` no longer depends on `tracing`: the contract reports nothing on its own and hands each condition to its caller.
- `Opened::new` is `Opened::bounded`, which returns what bounding dropped (`OpenedOverflow`) beside the value. The native host reports it where it bounds: a launch whose every item was dropped, and a second instance before it forwards the rest, are reported too.
- `commit_composition_in_place` and `project_ime_event` take a reporter for a refusal in a grant that runs after they returned, and `commit_composition_in_place` returns the store's refusal of the edit when it ran at once. Every native host and `TextInputOwner` report both.
- What launched the application is joined only up to `MAX_OPENED_ITEMS` items: a further cold report reaches `AppHandler::opened` after `ready` instead of being dropped.

### Removed

- `Platform::active_window` and `OwnerPlatform::active_window`, which nothing above the host read. A consumer that needs the focused window reads `WindowState::focused` from each window's state snapshot.
- `Platform::keyboard_layout`, `Platform::on_keyboard_layout_change` and `OwnerPlatform::keyboard_layout`: the keyboard layout stays inside the native host until the menu feature publishes it with its consumer.
- `TargetPlatform::is_mobile`, `is_desktop` and `is_touch_primary`: whether input is touch-first belongs to the device, not to the operating system; read `InputCapabilities` instead.
- `PlatformInput::as_drag_drop`: match `PlatformInput::DragDrop` instead.
