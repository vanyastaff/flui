### Added

- `flui_cupertino::ResolvedCupertinoTheme` and `ResolvedCupertinoTextTheme`: the theme a Cupertino widget reads, every colour concrete and every text role a `TextStyle`, from `CupertinoTheme::of`.
- `CupertinoThemeData::resolve(ctx)` resolves a theme that is not installed.
- Every one of the nine text roles can be set through `CupertinoThemeData::text_theme`; `tab_label_text_style`, `nav_large_title_text_style`, `nav_action_text_style`, `picker_text_style` and `date_time_picker_text_style` could not be set before.

### Changed

- `CupertinoThemeData` and `CupertinoTextThemeData` are `#[non_exhaustive]` structs read and changed by their `pub` fields, each `None` the iOS default: start from `Default::default()` and assign the fields to change. `CupertinoThemeData::text_theme` is a `CupertinoTextThemeData`, no longer an `Option`, and an unset `CupertinoTextThemeData::primary_color` follows the theme's resolved primary colour.
- `CupertinoTheme::of` returns an `Rc<ResolvedCupertinoTheme>` shared by every reader under the same brightness, contrast and system accent, instead of a fresh `CupertinoThemeData` copy per call.
- `CupertinoApp` publishes its authored theme rather than a copy resolved in its own context, so a nested `MediaQuery` that changes brightness or contrast changes the theme below it.
- `CupertinoTabBar::opaque` takes the resolved theme the caller already holds: `opaque(&theme, ctx)`.

### Removed

- `CupertinoThemeData::new`, `CupertinoTextThemeData::new`, their `with_*` builders, their getters and their `resolve_from`: set the fields, and read the resolved theme's fields.
