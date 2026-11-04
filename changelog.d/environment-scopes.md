### Changed

- **`flui-view`**: `GestureArenaScope` and `VsyncScope` join `MediaQuery`/`MediaQueryData`
  in `flui_view::environment`, with `GesturePolicy` beside them — the inherited scopes are
  views the presentation installs above one root, and the widget catalog imports them from
  there. Package authors reach them as `flui_sdk::view::environment::*`; the facade as
  `flui::view::environment::*`. `VsyncScope::same_registry` and the scope's read methods
  (`of`, `maybe_of`, `settings_of`, `settings_or_default_of`, `wheel_preferences_of`,
  `dependent_of`) are public at the new home.

### Removed

- **`flui-widgets`**: the `animated::VsyncScope` and `interaction::GestureArenaScope`
  module paths and their re-exports; use `flui_view::environment`.
