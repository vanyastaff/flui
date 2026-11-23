### Changed

- `flui-material` publishes every type once, at the crate root: its modules are private, so `flui_material::chip::Chip` and the other module paths are gone. `DEFAULT_TOOLBAR_HEIGHT` and `FloatingActionButtonState` join the root; the other component token constants (`CHIP_ICON_SIZE`, `FAB_SIZE`, `SWITCH_TRACK_WIDTH` and the rest) are no longer public.
- `PageTransitionsTheme::with` is renamed `transition`. Two themes with the same overrides compare equal whatever order the overrides were given in.
- `ScaffoldMessengerHandle` compares by identity with `==`, which replaces `ptr_eq`.

### Removed

- `ScaffoldMessengerHandle::registered_scaffold_count` and `TabController::listener_count`, test seams nothing called.
