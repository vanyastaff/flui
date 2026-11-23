### Changed

- **`flui-material`**: a `Tab` is built from its content: `Tab::text("Home")`,
  `Tab::child(view)` or `Tab::icon_only(icon)`, with `.icon(icon)` adding an icon above a label
  and `.height(h)` as before. A tab with no content, or with both a text and a view label, no
  longer compiles; `Tab::new()`, `Tab::default()` and the `.text`/`.child` builders are gone.
- **`flui-material`**: `Material::new()` takes no color and paints the theme's
  `color_scheme.surface`, following a theme change; `.color(c)` sets a color of its own.
  Without a color and without a `Theme` ancestor, `build` panics as `Theme::of` does.
- **`flui-material`**: `Scaffold` no longer depends on the whole theme; its surface follows
  `color_scheme` alone.
- **`flui-objects`**: `RenderPhysicalModel` and `RenderPhysicalShape` store a negative, NaN or
  infinite elevation as `0.0` (no shadow) from `with_elevation` and `set_elevation`, in every
  build; `elevation()` is always finite and non-negative.

### Removed

- **`flui-material`**: `ColorScheme::copy_with`, `ColorSchemeOverrides`, `ThemeData::copy_with`
  and `ThemeDataOverrides`. Assign the fields instead:
  `let mut theme = ThemeData::light(); theme.color_scheme.primary = red;`.
- **`flui-material`**: the deprecated `ColorScheme` roles `background`, `on_background` and
  `surface_variant`; read `surface`, `on_surface` and `surface_container_highest`.

### Fixed

- **`flui-material`**: a `Drawer`, `SnackBar` or any `Material` given a negative or NaN
  elevation, or a `Drawer` given a negative or NaN width, no longer panics in a debug build and
  publishes finite geometry in a release one: the elevation casts no shadow and the width is
  `0.0`.
