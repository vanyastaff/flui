### Fixed

- **`flui-material`**: `Material` now gives its subtree the theme's
  `text_theme.body_medium` as the default text style, so plain `Text` in a
  `Scaffold` body or a `Card` under `MaterialApp` reads as body copy instead of
  the app's red 48 px fallback. Components that style their own text keep it.
