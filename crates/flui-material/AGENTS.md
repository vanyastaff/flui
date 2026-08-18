# AGENTS.md — flui-material

Material Design theming foundation: `ColorScheme`, the M3 2021 type scale (`Typography`/`TextTheme`), `ThemeData`, and the `Theme` inherited widget that publishes it to a subtree.

## What lives here

- **`ColorScheme`** — the full Material 3 color-role palette (50 fields incl. `brightness`), `#[non_exhaustive]`. `light()`/`dark()` are verbatim ports of the oracle's `_colorSchemeLightM3`/`_colorSchemeDarkM3` const tables (`theme_data.dart`) — the same table `ThemeData()` defaults to, not the legacy M2 `ColorScheme.light()`/`.dark()` baseline constructors. `copy_with` takes a `ColorSchemeOverrides` patch struct (Rust has no optional named parameters).
- **`typography` module** — `english_like_2021()`, the M3 2021 English-like type scale (15 roles: `display_large`…`body_small`), geometry only (no color). Ported from `Typography.englishLike2021`.
- **`TextTheme`** — the 15 type-scale roles as `Option<TextStyle>`. `merge` composes a base theme with a patch (patch's non-`None` fields win, per role, via `flui_types::TextStyle::merge`). `black_mountain_view()`/`white_mountain_view()` are the oracle's color-only overlay tables; `apply_color` sets a single uniform color across every present role (the simplification M3's `Typography.material2021` factory reduces to — see `theme_data.rs`'s `default_text_theme` doc comment).
- **`ThemeData`** — `color_scheme` + `text_theme`, `#[non_exhaustive]` (component-theme slots land with their owning widgets). `light()`/`dark()` compose `ColorScheme::light()`/`dark()` with the derived default `TextTheme` (`englishLike2021` geometry recolored to `on_surface`). `brightness()` reads `color_scheme.brightness`.
- **`Theme`** — the `InheritedView` widget that publishes `ThemeData` to a subtree (`Theme::of`/`maybe_of`). Implements `flui-widgets`' `InheritedTheme` trait.
- **`MaterialApp` / `ThemeMode`** (`app.rs`) — the Material application shell (ADR-0042): composes `flui-widgets`' design-neutral `WidgetsApp`, resolves `theme`/`dark_theme`/`theme_mode` against the ambient `MediaQueryData::platform_brightness` (the oracle's `_themeBuilder`), and publishes the result through `Theme` with a `ScaffoldMessenger` band, per presentation. The caller's `builder` hook runs from its own child element below the published `Theme` (the oracle's double-`Builder`). Named deferrals (high-contrast themes, `AnimatedTheme`, `DefaultSelectionStyle`, scroll behavior, Material hero arc motion, `MaterialLocalizations`) are in the module docs.

## Key constraints

- **Constants-first, M3-only.** No `ColorScheme.fromSeed` (dynamic-color generation) — deferred to a follow-up unit gated on a spike validating a seed-color crate against `color_scheme_test.dart`'s literal table. No M2 fallback constructors, no `useMaterial3` toggle: this crate has only one mode.
- **Field order mirrors the oracle's primary `ColorScheme` constructor**, not the M3 const tables' generated order (which puts `background`/`onBackground` before the `surface*` group for historical script-generation reasons).
- **Deprecated-in-Flutter roles are still normal fields** (`background`, `on_background`, `surface_variant`) — the oracle's own default tables and parity tests still populate/assert them, so dropping them breaks parity for no gain.
- **No dense/tall type-scale geometries** — only `englishLike2021`. Both are identical to `englishLike2021` in the M3 spec itself (per the oracle's own doc comment) and have no localization consumer yet to resolve `ScriptCategory` against.
- **The default `TextTheme` is baked once, at `ThemeData::light`/`dark` construction** — the oracle recomputes it lazily per `Theme.of` read, keyed on the ambient locale's script category (`Theme.build`'s `ThemeData.localize` step). FLUI has no script-category-resolving localization consumer yet, so this is a documented, named simplification, not a silent gap.
- **`AnimatedTheme` / `ColorScheme`/`TextTheme` lerp are deferred** — no component consumes an interpolated theme yet.
- **`MaterialApp` composes downward only** — it builds on `flui-widgets`' `WidgetsApp` (ADR-0028/ADR-0042: the design-neutral shell never learns about this crate). Theme *selection* (`ThemeMode`) lives here, next to the tokens it selects between — never in `flui-app` (ADR-0042 §2/§6).

## Related crates

- `flui-widgets` — owns the `InheritedTheme` trait `Theme` implements, and the `BoxedView`/`IntoView` widget-authoring spine.
- `flui-view` — `InheritedView`, `impl_inherited_view!`, `BuildContext` — the element-tree primitives `Theme` is built on.
- `flui-types` — `Color`, `Brightness`, `TextStyle`, `FontWeight` — the value types every table in this crate is built from.
