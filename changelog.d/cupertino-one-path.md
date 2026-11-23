### Changed

- `flui-cupertino` publishes every type once, at the crate root: its modules are private, so `flui_cupertino::button::CupertinoButton` and the other module paths are gone.

### Removed

- The constants `NAV_BAR_PERSISTENT_HEIGHT`, `TAB_BAR_HEIGHT`, `HAIRLINE_BORDER_WIDTH`, `K_FADE_OUT_DURATION` and `K_FADE_IN_DURATION` are no longer public. Flutter keeps the same values private.
