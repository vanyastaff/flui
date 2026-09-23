# ADR-0028: Design-system decoupling contract

- **Status:** Accepted
- **Date:** 2026-07-16
- **Related:** ADR-0041 (the whole-workspace layer policy this rule is one part of)

## Context

Flutter spent years unwinding `material`/`cupertino` assumptions out of its framework core, and
froze its in-SDK design-system packages at 3.44 to continue them in `flutter/packages`. FLUI's
layer table already puts the design systems (L7) above the widget catalog (L6). Without an
enforced rule the failure mode is gradual: a widget author reaches for a Material constant
"just this once", a core crate pulls `flui-material` in as a test fixture, and a few changes
later `flui-cupertino` cannot exist without dragging Material along.

## Decision

**Core never depends on a design system.** No workspace crate other than `flui-material`,
`flui-cupertino`, `flui-localizations`, `flui-app` and the `flui` facade may declare a
dependency — normal, dev or build — on `flui-material` or `flui-cupertino`.
`check-workspace-inventory.sh` enforces it from `cargo metadata`. The `flui-localizations`
exception is one-way: ADR-0041 forbids either design system from depending back on it.
Material and Cupertino do not depend on each other.

**Shared substrate lives below both design systems.** Material and Cupertino share more than
the interaction mechanism: ink/splash, surface tint, elevation and shadow, and the
localizations interface are one substrate both resolve against. That substrate lives below
both (in `flui-widgets` or a lower crate) and has no opinion about which design system draws on
it. Design *opinion* — M3 token tables, `ColorScheme`/`TextTheme` literals, Cupertino's dynamic
colors — stays in the design crate that owns it.

**Design-agnostic mechanism goes down, not sideways.** `WidgetState`/`WidgetStateProperty`/
`WidgetStatesController`, `InheritedTheme`, and `Localizations`/`LocalizationsDelegate`/
`Directionality` live in `flui-widgets`, matching Flutter 3.44's own downward moves.

**Platform-adaptive behavior is a capability seam, not a branch.** Where behavior varies by
platform convention (scroll physics, selection handles, default cursor), a design system or
platform layer implements and injects a trait; a core widget never branches on
`cfg!(target_os)` or `theme.platform`.

**Raw primitives.** When a component's behavior is design-agnostic but Flutter only ships it
fused to a design system's paint, the behavior lands in `flui-widgets` as an unstyled "raw"
primitive and the design crate supplies the chrome — Flutter's own `RawMenuAnchor`/`RawRadio`
direction (flutter/flutter#101479). `InkWell` already follows it.

**Text-selection chrome is injected.** Selection handles, the selection menu and the toolbar
are where Flutter found the boundary hardest to hold (flutter/flutter#179591). `flui-widgets`'
text-editing core exposes a seam for those visuals and ships no default appearance; a design
crate or the app supplies one.

**No god-widget entry point.** The `WidgetsApp`-equivalent composition root works standalone.
A `MaterialApp`-equivalent is a thin layer on top that supplies theme and token defaults; no
core widget assumes it is present.

## Flutter divergence

Package topology is a leapfrog zone (ADR-0027): FLUI takes the shape Flutter is heading toward
— core has no opinion about Material or Cupertino — and enforces it now, instead of mirroring
Flutter's package split move for move. Widget behavior (`WidgetState` semantics, `Material`'s
clip/elevation/shadow, `InkWell`'s overlay resolution) stays Flutter's.

## Consequences

- A design-system coupling regression fails the inventory check with the offending crate
  named.
- The exemption set is hard-coded; a second application-tier crate that legitimately needs
  `flui-material` needs a script edit — intended friction.
- Shared substrate has to be designed as substrate: a feature one design system wants from the
  other moves down; it is never imported across.

## Alternatives rejected

- **A documented convention only.** A diagram does not fail a build.
- **A source-import grep for `use flui_material`.** Misses a declared-but-unused dependency and
  reads the dependency graph a second, driftable way.
- **`cargo-deny` bans.** They express "A must not depend on B", not a directional rule with
  exemptions; revisit if directional bans mature.
- **Design systems never share defaults.** Forces ink, tint, elevation and localizations to be
  implemented twice, and the two copies drift.
