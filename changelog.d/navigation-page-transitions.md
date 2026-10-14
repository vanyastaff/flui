### Added

- **Ambient page transitions** (`flui-widgets`): the `PageTransitionBuilder` trait and the
  `PageTransitions` view. A `PageRoute` or `PopupRoute` given no `.transitions(…)` adopts the
  nearest builder of its kind, with its durations, when its navigator installs it (ADR-0221).
  `PageRoute::transition_builder` and `PopupRoute::transition_builder` take a builder directly.
- **`ParallaxPageTransition`** (`flui-widgets`): the iOS parallax slide, moved out of
  `flui-cupertino` so every package shares one copy.
- **Material page transitions** (`flui-material`): `PageTransition` (fade-forwards, zoom,
  Cupertino, shared axis, fade through, none, custom), `PageTransitionsTheme`, and
  `ThemeData::platform` / `ThemeData::page_transitions_theme`. `MaterialApp` installs the
  platform's transition: fade-forwards on Android (450 ms), the parallax slide on iOS and macOS
  (500 ms), zoom elsewhere (300 ms).
- **`flui_sdk::platform::TargetPlatform`**.

### Changed

- **Material route pushes animate** (`flui-material`): routes pushed under a `MaterialApp` with
  no transitions of their own now run the theme's transition instead of a 300 ms jump cut, and
  `show_dialog` fades in over 150 ms.
- **`ThemeData` is no longer `Send`/`Sync`** (`flui-material`): `PageTransition::Custom` holds an
  `Rc<dyn PageTransitionBuilder>`.
