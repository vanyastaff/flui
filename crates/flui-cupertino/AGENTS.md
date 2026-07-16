# AGENTS.md — flui-cupertino

iOS-style (Cupertino) theming foundation: the brightness-adaptive `CupertinoDynamicColor`/`CupertinoColors` palette, `CupertinoTextThemeData`, `CupertinoThemeData`, the `CupertinoTheme` inherited widget, and `CupertinoButton`.

## What lives here

- **`colors.rs`** — `CupertinoDynamicColor` (the 8-variant brightness/contrast/elevation data struct) and `CupertinoColors` (the named palette — `LABEL`, `SYSTEM_BACKGROUND`, `SEPARATOR`, the `SYSTEM_FILL`/`SYSTEM_GREY` families, `SYSTEM_BLUE`/`ACTIVE_BLUE`, `DESTRUCTIVE_RED`, …). `CupertinoColor` (`Static(Color)` / `Dynamic(CupertinoDynamicColor)`) is this crate's Rust-native answer to Dart's `CupertinoDynamicColor implements Color` polymorphism — see the module doc for why a plain `Color`-typed field can't carry that trick in Rust.
- **`text_theme.rs`** — `CupertinoTextThemeData`: the 9 type-style roles (`text_style`, `action_text_style`, `nav_title_text_style`, …), each falling back through a `TextThemeDefaults` builder parameterized on `label`/`inactive_gray`/`primary_color` dynamic colors. Ships the oracle's exact `'CupertinoSystemText'`/`'CupertinoSystemDisplay'` font-family strings plus a `cosmic-text` fallback chain — metrics parity, not pixel parity (no bundled SF font).
- **`theme.rs`** — `CupertinoThemeData` (brightness/primary/contrasting/bar-background/scaffold-background + the derived `text_theme`) and `CupertinoTheme`, the `InheritedView` that publishes it. `CupertinoTheme::of` always resolves before returning (`CupertinoThemeData::resolve_from`) — consumers never see an unresolved dynamic color.
- **`button.rs`** — `CupertinoButton`, `CupertinoButton::tinted`, `CupertinoButton::filled`; `CupertinoButtonSize` (`Small`/`Medium`/`Large`) with the oracle's per-size padding/border-radius/minimum-size tables from `constants.dart`. Press-opacity fade via a persistent `AnimationController` on the ambient `VsyncScope`, composed over `flui-widgets`' `FadeTransition`.

## Key constraints

- **ADR-0028: no upward/sideways design-system dependency.** Depends only on `flui-widgets`/`flui-view`/`flui-types`/`flui-foundation`/`flui-animation`/`flui-interaction` — never `flui-material` (independent sibling design system) and never `flui-objects`/`flui-rendering` directly (v1 composes existing `flui-widgets` widgets; it paints nothing of its own).
- **Brightness-only dynamic-color resolution.** `CupertinoDynamicColor::resolve_from` implements full oracle parity for the brightness axis (`CupertinoTheme` ambient ?? `MediaQuery::platform_brightness`) but always resolves contrast/elevation as their base variant — FLUI's `MediaQueryData` has no `high_contrast` field yet and there is no `CupertinoUserInterfaceLevel` ambient. Every color's full 8-variant table is still stored verbatim, so wiring the other two axes later needs no data-table changes.
- **One component only.** `CupertinoButton` is the only widget this crate ships in V1 — `CupertinoNavigationBar`, `CupertinoTabScaffold`, pickers, `CupertinoTextField`, action sheets, and `CupertinoPageRoute`'s swipe-back transition are later increments on this same substrate.
- **`CupertinoButton` press tracking is reduced.** The oracle drives its fade off real `onTapDown`/`onTapMove`/`onTapUp`/`onTapCancel` callbacks; `flui-widgets::GestureDetector` exposes only `on_tap` (recognized-tap) and `on_long_press`. This port applies the oracle's fade *sequence* uniformly to the single `on_tap` event instead — see `button.rs`'s module doc for the full rationale and what's named-deferred (focus ring, `WidgetState`-resolved mouse cursor, `onFocusChange`/`autofocus`, icon theming).
- **No `CupertinoApp`.** This crate is the theming substrate an app root (`CupertinoTheme` wrapping the tree, or a future `CupertinoApp`) builds on.

## Related crates

- `flui-widgets` — `MediaQuery`, `FadeTransition`, `GestureDetector`, `VsyncScope`, `InheritedTheme` (the trait `CupertinoTheme` implements), and the layout/paint widgets `CupertinoButton` composes (`ConstrainedBox`, `Padding`, `Align`, `DecoratedBox`, `DefaultTextStyle`).
- `flui-view` — `InheritedView`, `impl_inherited_view!`, `BuildContext`, `StatefulView`/`ViewState` — the element-tree primitives everything here is built on.
- `flui-types` — `Color`, `Brightness`, `TextStyle`/`FontWeight`, `BoxDecoration`, `BorderRadius` — the value types every const table in this crate is built from.
- `flui-animation` — `AnimationController`, `Curves`, `Vsync` — `CupertinoButton`'s press-opacity fade.
- `flui-material` — the sibling design system. No dependency in either direction (ADR-0028); the future Material→Cupertino theme-injection seam belongs to `flui-material`, not here (see `theme.rs`'s module doc).

## Testing

- `src/*.rs` unit tests cover pure data-model behavior (construction, resolution logic, const-table spot checks) with no mounted context.
- `tests/colors.rs` — full const-table oracle-diff test against `cupertino/colors.dart` (tag `3.44.0`), asserting raw ARGB channels rather than re-deriving the source's own `Color::rgba(...)` calls.
- `tests/theme.rs` — resolve-chain integration tests through a real mounted `BuildContext`: `CupertinoTheme::of` reading the ancestor (not a default), and the brightness root flipping via both an explicit `CupertinoThemeData::brightness` and the `MediaQuery::platform_brightness` fallback.
- `tests/button.rs` — tap firing, the disabled no-handler swallow, per-size minimum geometry reaching the mounted render tree, and the press-opacity timeline ticked through `tests/common`'s vsync-driven harness (copied from `flui-material`'s).
