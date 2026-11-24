### Added

- `CupertinoColors` carries the whole iOS palette Flutter has, 47 constants with Flutter's values (`flui-cupertino`): `SYSTEM_GREEN`, `SYSTEM_MINT`, `SYSTEM_INDIGO`, `SYSTEM_ORANGE`, `SYSTEM_PINK`, `SYSTEM_BROWN`, `SYSTEM_PURPLE`, `SYSTEM_TEAL`, `SYSTEM_CYAN`, `SYSTEM_YELLOW`, `QUATERNARY_LABEL`, `PLACEHOLDER_TEXT`, `TERTIARY_SYSTEM_BACKGROUND`, `SYSTEM_GROUPED_BACKGROUND`, `SECONDARY_SYSTEM_GROUPED_BACKGROUND`, `TERTIARY_SYSTEM_GROUPED_BACKGROUND`, `LINK`, the fixed `LIGHT_BACKGROUND_GRAY`, `EXTRA_LIGHT_BACKGROUND_GRAY` and `DARK_BACKGROUND_GRAY`, and the aliases `ACTIVE_GREEN` and `ACTIVE_ORANGE`.
- `CupertinoUserInterfaceLevel` publishes a `CupertinoUserInterfaceLevelData` (`Base` or `Elevated`) to its subtree, read with `of(ctx)` (`Base` without an ancestor) and `maybe_of(ctx)`. A dynamic colour resolved below an elevated level, the theme's colours included, takes its elevated variant, so a dark `SYSTEM_BACKGROUND` there is `rgb(28, 28, 30)` instead of black. Nothing in the package installs an elevated level; an application wraps what it presents above its main content in one.

### Changed

- `CupertinoTheme::of` keeps one resolution per brightness, interface level, contrast and system accent, so a reader below an elevated level gets a resolution of its own.

### Removed

- `CupertinoDynamicColor::new`, whose eight colours were positional: write a `CupertinoDynamicColor { color, dark_color, … }` struct literal naming the eight variants, or use `with_brightness` / `with_brightness_and_contrast`.
- `CupertinoDynamicColor::resolve(color, ctx)` and `CupertinoDynamicColor::maybe_resolve(color, ctx)`: call `CupertinoColor::resolve(ctx)` (with `Option::map` for an optional colour).
