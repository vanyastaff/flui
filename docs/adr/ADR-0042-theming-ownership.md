# ADR-0042: Theming ownership — appearance is per-presentation, selection belongs to the design system

*OS appearance is a per-window signal carried by `MediaQueryData::platform_brightness`; theme *selection* (`ThemeMode`) belongs to the design system that defines the tokens, not to the application framework; the resolved theme is published through an in-tree inherited widget, separately in each window. There is no universal `ThemeData` abstraction over Material and Cupertino, and `WidgetsApp` works with neither. `flui-app` owns no design tokens — the parked `AppTheme`/`AppColorScheme` surface is removed rather than moved.*

---

- **Status:** Accepted (2026-08-02)
- **Date:** 2026-08-02
- **Deciders:** @vanyastaff
- **Scope:** removal of `crates/flui-app/src/theme/` (`AppTheme`, `AppThemeBuilder`, `AppColorScheme`, `ThemeMode`); the ownership rule recorded in `crates/flui-app/AGENTS.md`; the target app-shell split (`WidgetsApp` / `MaterialApp` / `CupertinoApp`) that implements it
- **Related:** [ADR-0028](ADR-0028-design-system-decoupling-contract.md) (design-system decoupling — Material and Cupertino are independent siblings above the widget catalog); [ADR-0027](ADR-0027-owner-affine-ui-realms.md) (one realm per window — why appearance is per-presentation, and why package/ownership topology is a sanctioned leapfrog zone); [ADR-0037](ADR-0037-presentation-ownership-domains.md) (per-presentation owner state); [ADR-0041](ADR-0041-workspace-topology-contract.md) (layer policy — `flui-app` is L9, the design systems are L7)
- **Issue:** [#569](https://github.com/vanyastaff/flui/issues/569) — public package surface cleanup, the third Runtime.1 pre-sprint structural task

---

## Context

`crates/flui-app/src/theme/` shipped `AppTheme`, `AppThemeBuilder`, `AppColorScheme`, and `ThemeMode`: a flat pre-tree configuration object bundling colours, font family and sizes, spacing, corner radius, and an animation duration. Its own module doc classified it as "parked, unwired": nothing in the workspace read it, and no runner, binding, or widget consumed it.

Zero consumers is a fact about cost, not about need. It proves the current API can be changed for free; it does not prove applications will never need theming. The moment a `MaterialApp`-shaped entry point lands, *something* has to answer "which theme, resolved how, published where" — and the parked surface answers it wrongly in five ways:

1. **It bundles unrelated axes.** Colours, typography, spacing, radius, and motion are separate design concerns with separate override points. One struct with one builder means an application that wants a different corner radius restates a colour scheme.
2. **It is Material-shaped but lives in a design-neutral crate.** `AppColorScheme`'s tokens (`primary`, `on_primary`, `surface`, `on_surface`, …) are Material 3 role names. `flui-app` is L9 application composition; the tokens belong to L7.
3. **It competes with the real thing.** `flui_material::ThemeData` and `flui_cupertino::CupertinoThemeData` already exist, are consumed by real widgets, and already resolve through the tree. A third, unwired theme type is a fork of the contract, not a layer above it.
4. **Its scope is wrong.** `AppTheme` is application-scoped configuration. Appearance is not application-scoped: two windows of one application can legitimately show different brightness, and an OS appearance change reaches one window at a time.
5. **`ThemeMode::System` was a lie.** `AppThemeBuilder::build` mapped `System` to `AppTheme::light()`. A "follow the system" mode that never reads a system signal is worse than no mode at all, because it silently satisfies the API and fails the behaviour.

The parked surface also extends badly to everything theming has to grow into: high contrast, other accessibility appearance signals, third-party design systems, and live OS theme changes.

## Decision

**1. OS appearance belongs to the presentation, not the application.**

The system's light/dark signal reaches the tree as `MediaQueryData::platform_brightness`, scoped to one window's UI tree. An OS theme change updates that window's `MediaQuery` and rebuilds its dependents; it does not mutate a process-global value. This follows directly from ADR-0027's one-realm-per-window model: a per-window signal delivered through a process-scoped configuration object would be unable to express two windows disagreeing, which they legitimately can.

**2. Theme *selection* belongs to the design system.**

`ThemeMode` (light / dark / follow-the-system) is Material's selection model. It goes in `flui-material`, next to `ThemeData`, and arrives together with `MaterialApp` — the widget that actually consumes it, resolving `theme` / `dark_theme` / `theme_mode` against the ambient `platform_brightness`. Cupertino models selection differently (`CupertinoThemeData` carries an *optional* brightness override and otherwise follows the ambient `MediaQuery`, which is Apple's model, not a `ThemeMode` enum), and a third-party design system may model it differently again. There is no framework-level selection enum.

This ADR deliberately does **not** relocate the existing `ThemeMode` into `flui-material` as part of the removal. An unused enum moved to a new crate is relocated debt, not progress; it lands with `MaterialApp`, shaped by that widget's needs.

**3. The theme lives in the tree.**

Shared application state may hold the user's *preference* (a persisted "prefers dark" setting). The resolved `ThemeData` is published through the inherited `Theme` widget, separately in each window's tree. Two windows of one application therefore render different themes without any special mechanism, and a theme change is an ordinary rebuild of the `Theme`'s dependents.

**4. There is no universal `ThemeData` trait.**

Abstracting Material and Cupertino behind `dyn Theme`, `Any`, or a shared token set produces a lowest-common-denominator API: the intersection of two design systems' tokens is close to empty, and every consumer downcasts back to the concrete type. What Material, Cupertino, and a third-party design system share is the *mechanism* — an inherited widget publishing an immutable value, resolved against the ambient `MediaQuery` — never the tokens. The mechanism already exists in `flui-view`/`flui-widgets` and needs no design-system-aware abstraction.

**5. `WidgetsApp` stays fully independent.**

An application with no Material and no Cupertino must still get navigation, localization, focus, and media information. The target split is:

| Widget | Crate | Provides |
|---|---|---|
| `WidgetsApp` | `flui-widgets` | navigator, localizations, focus root, `MediaQuery`, vsync scope — design-neutral |
| `MaterialApp` | `flui-material` | `WidgetsApp` + `ThemeMode` resolution + `Theme` publication + Material defaults |
| `CupertinoApp` | `flui-cupertino` | `WidgetsApp` + `CupertinoTheme` publication + Cupertino defaults |

`MaterialApp` and `CupertinoApp` compose `WidgetsApp`; `WidgetsApp` knows about neither. This is the same direction ADR-0028 already locks for the catalogs, applied to the app shell.

**6. `flui-app` owns no design tokens.**

`AppTheme` and `AppColorScheme` are deleted outright — no compatibility alias, no re-export, no move to another crate. They were an incorrect public contract; carrying a shim forward would preserve exactly the confusion the removal exists to end. `crates/flui-app/AGENTS.md` records the rule so the surface cannot drift back: colours, typography, spacing, radius, motion, and any other design token are L7 concerns, and a token type appearing under `crates/flui-app/src/` is a review failure regardless of what it is named.

## Consequences

- **Positive.** The removal happens while the cost is zero — no consumer, no migration, no deprecation window. Breaking changes are cheap today and ossify once consumers exist. The replacement architecture is recorded before the deletion, so the need it served has a named home rather than being quietly dropped.
- **Positive.** `flui-app` loses its only design-system-shaped surface, which makes the L9/L7 split in ADR-0041 true in the source and not only in the manifest.
- **Negative / accepted cost.** FLUI has, for now, *no* application-level theming entry point at all: an author wires `Theme`/`CupertinoTheme` into their own tree. That is the honest state — it was equally true before, since `AppTheme` was never read; what changes is that the API no longer implies otherwise.
- **Neutral.** No behaviour changes. Nothing consumed the removed types.

## What is untouched

Prime Directive #1 is not amended. The three-tree model, lifecycle, the layout/paint/hit-test protocol, and reconciliation stay ported 1:1 from `.flutter/`. `flui_material::ThemeData`, `flui_material::Theme`, `flui_cupertino::CupertinoThemeData`, and `flui_cupertino::CupertinoTheme` are unchanged by this ADR; it decides where theme *ownership* sits, not what the tokens are.

Ownership topology is the sanctioned leapfrog category ADR-0027 opened: Flutter is the behavioural reference for widget-tree semantics, not for which package owns which configuration object. In substance this lands where Flutter already is — `ThemeMode` in `material`, `platform_brightness` on `MediaQueryData`, `Theme` as an inherited widget, no shared theme supertype — with the per-window scoping made explicit rather than left to a process-global `WidgetsBinding`.

## Alternatives rejected

- **Keep `AppTheme` and wire it up.** It would have to be wired to something: either a process-global default (wrong scope — see §1) or a per-window value, at which point it is a worse `ThemeData` living two layers away from its tokens. Wiring a wrong contract makes it expensive to remove instead of free.
- **Move `AppTheme`/`AppColorScheme` into `flui-material`.** Material already has `ThemeData` and `ColorScheme` with real consumers. Importing a second, parallel, unwired pair would create the name collision the workspace deleted `flui_app::theme::colors::Color` to avoid.
- **Introduce a `Theme` trait both design systems implement.** Rejected in §4: the shared token set is empty, so the trait degenerates to `Any` plus downcasts, and it would create a compile-time coupling point between two crates ADR-0028 keeps independent.
- **Move `ThemeMode` to `flui-material` now, keep the rest deleted.** Rejected in §2: an enum with no consumer in its new home is debt with a new address. It lands with `MaterialApp`.
- **Deprecate rather than delete.** A `#[deprecated]` type with zero consumers warns nobody and still has to be maintained through the `WidgetsApp`/`MaterialApp` work. Deprecation buys migration time; there is no one to migrate.

## Follow-up

The app-shell work this ADR specifies — design-neutral `WidgetsApp` in `flui-widgets`, `MaterialApp` + `ThemeMode` in `flui-material`, `CupertinoApp` in `flui-cupertino`, with tests for theme switching, OS brightness changes, and multiple windows showing different themes — is tracked as its own dependency-ordered issue: [#573](https://github.com/vanyastaff/flui/issues/573).
