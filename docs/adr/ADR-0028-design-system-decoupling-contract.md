# ADR-0028: Design-system decoupling contract

*Core (`flui-view`/`flui-widgets`/`flui-rendering`/`flui-objects`/`flui-animation`/`flui-interaction`/`flui-foundation`/`flui-types`/`flui-painting`/`flui-geometry`, and every other crate that is not a design system, the app crate, or an example) never depends on `flui-material` or `flui-cupertino` — enforced by `scripts/check-workspace-inventory.sh`, not left as a convention.*

---

- **Status:** Accepted (owner directive, 2026-07-16)
- **Date:** 2026-07-16
- **Deciders:** @vanyastaff
- **Scope:** workspace-wide dependency topology — `crates/*/Cargo.toml`, `scripts/check-workspace-inventory.sh`, `docs/FOUNDATIONS.md`
- **Related:** ADR-0027 (sanctioned leapfrog zones: multi-window ownership, runtime/scheduling topology, concurrency architecture, presentation architecture — this ADR adds **package/dependency topology** as one more); ADR-0022/ADR-0026 (Focus seam — an example of a design-agnostic mechanism already correctly homed in `flui-widgets`); ADR-0009 (flui-widgets as the configuration-object catalog design systems are built on)

---

## Context

Flutter announced decoupling `material`/`cupertino` from the framework core (`widgets`/`rendering`/`painting` stop assuming a design system exists). FLUI's own layer table (`docs/FOUNDATIONS.md`, Part IV) already draws design systems as **L7**, strictly above the **L6** widget catalog — `material --> widgets`, `cupertino --> widgets`, never the reverse. That direction has held so far, but only as a convention nobody has broken yet, not as something CI would catch if someone did. Two concrete near-misses motivate closing that gap now rather than after the fact:

1. **`WidgetState`/`WidgetStateProperty`/`WidgetStatesController`** (the interactive-state vocabulary `InkWell` and every future button style resolve against) was ported to `flui-widgets`, matching Flutter 3.44's own move of `widget_state.dart` out of `material` and into `widgets` — precisely the decoupling Flutter announced, already landed on the FLUI side before this ADR existed to name it.
2. `flui-material`'s `Material`/`InkWell` substrate (PR-1) depends downward on `flui-widgets` (`Focus`, `MouseRegion`, `GestureDetector`, `WidgetStateProperty`) with zero edges the other way — the correct shape happened by construction, not by a rule that would have caught it going wrong.

Without an enforced rule, the failure mode is gradual: a widget author reaches for a Material color constant "just this once," a core crate's dev-dependencies pull in `flui-material` for a convenience test fixture, and five PRs later `flui-cupertino` cannot exist without dragging Material along — the exact coupling Flutter spent years unwinding.

## Decision

**Core never depends on a design system. This is a Cargo-manifest contract, enforced in CI, not a documentation convention.**

- **The guard.** `scripts/check-workspace-inventory.sh` (run by `just inventory-check`, part of `just ci`) now parses `cargo metadata`'s per-package `dependencies` list (normal + dev + build — the same source of truth the script already uses for its path/version-consistency check) and fails if any active workspace crate other than `flui-material`, `flui-cupertino`, `flui-app`, or `flui` (the facade) declares a dependency named `flui-material` or `flui-cupertino`. `flui-cupertino` does not exist yet; it is guarded pre-emptively so the rule is live the instant the crate is created, not retrofitted after its first accidental core dependency.
- **Why `check-workspace-inventory.sh`, not `port-check.sh`.** `port-check.sh`'s triggers (the sanctioned-`dyn`-boundary allowlist, N-geom.U16's `glam` confinement, Cross.H2/H3/H7) audit **usage** — they `rg` over `.rs` source for patterns. This contract is a **declared-dependency-graph** fact, checkable straight from `Cargo.toml`/`cargo metadata`, with no source pattern to grep. `check-workspace-inventory.sh` is the one script already parsing `cargo metadata`'s package dependency lists (for the path/version-requirement check), so the new rule extends existing, proven parsing rather than teaching `port-check.sh` a second way to read dependency graphs.
- **Where design-agnostic mechanism lives.** Anything a design system needs but that carries no design opinion belongs in `flui-widgets` (or lower), matching Flutter 3.44's own downward migration:
  - `WidgetState`/`WidgetStates`/`WidgetStateProperty`/`WidgetStatesController` — already landed in `flui-widgets` (this ADR ratifies the placement, doesn't newly decide it).
  - `InheritedTheme` (the trait a theme widget implements to publish itself, e.g. `flui_material::Theme`) — already in `flui-widgets::app`.
  - `Localizations`/`LocalizationsDelegate`/`Directionality` — already in `flui-widgets::localization` (see the `l10n --> widgets` FOUNDATIONS note, 2026-07-16).
- **Where design opinion lives.** M3 token defaults, color/typography constants, and any Material-specific numeric table (elevation-to-shadow, state-overlay opacities, `ColorScheme`/`TextTheme` literals) stay a **separate data module inside `flui-material`**, never inlined into a core widget. A future `flui-cupertino` gets its own, unrelated token module — the two design systems never share defaults, only the mechanism (`WidgetStateProperty` et al.) they resolve against.
- **Platform-adaptive behavior is a capability seam, not a branch.** Where a widget's behavior must vary by platform convention (scroll physics, text-selection handles, a default `MouseCursor` shape), the seam is a trait/capability a design system or platform layer implements and injects — never `if cfg!(target_os = ...)` or `if theme.platform == ...` inside a core widget. This is the same shape ADR-0027 already sanctions for runtime/presentation topology; this ADR extends the **leapfrog-zone list** (ADR-0027, "sanctioned leapfrog zones") to include **package/dependency topology and platform-adaptive capability seams** — Flutter's *package* structure and its *if-platform* idioms are not the behavioral reference here, only its widget-tree semantics are.

## Consequences

- **Positive.** A design-system-coupling regression fails `just inventory-check` immediately, with the offending crate/dependency named in the error — the same fail-fast guarantee port-check gives source-pattern violations, now covering the dependency graph too. `flui-cupertino` can be scaffolded later with zero risk of inheriting an accidental Material dependency, because the guard already exists.
- **Negative / accepted cost.** The exemption list (`flui-material`, `flui-cupertino`, `flui-app`, `flui`) is a hardcoded set in the script; adding a second application-tier crate (an embedder, say) that legitimately needs `flui-material` requires a script edit, not just a `Cargo.toml` change — an intentional friction point, not an oversight.
- **Neutral.** No crate's actual dependencies changed — the guard formalizes a shape the workspace already has (see Context: PR-1's `Material`/`InkWell` substrate already depends only downward). This ADR is a contract for the *next* change, not a migration of the current one.

## What is untouched

**Prime Directive #1 (behavior loyalty) is not amended.** The three-tree model, lifecycle, layout/paint/hit-test protocol, and reconciliation stay ported 1:1 from `.flutter/`; `WidgetState`'s semantics, `Material`'s clip/elevation/shadow behavior, and `InkWell`'s state-overlay resolution all remain loyal to the oracle (see `crates/flui-widgets/src/widget_state.rs` and `crates/flui-material/src/{material,ink_well}.rs` module docs for the per-behavior citations). **Only package topology leapfrogs** — same category as ADR-0027's threading/runtime topology, not a new category of "improve on Flutter's actual UI behavior." Flutter announcing a `material`/`cupertino` decoupling it had not yet shipped is exactly the kind of "no strong contract" edge ADR's Prime Directive #2 already invites FLUI to get ahead of, by making the target shape a compile-time-adjacent guard from the start instead of retrofitting it after the coupling exists.

## Alternatives rejected

- **Leave it as a documented convention (FOUNDATIONS.md's DAG + prose).** Rejected: a DAG diagram does not fail a build. The whole point of "contract, not convention" is that violating it produces a red CI check, not a hoped-for code-review catch.
- **A `port-check.sh` regex trigger grepping `.rs` files for `use flui_material`/`use flui_cupertino`.** Rejected: source-import greps miss a dependency declared but unused (still a coupling smell Cargo would resolve and lock), and duplicate the dependency-graph parsing `check-workspace-inventory.sh` already does correctly via `cargo metadata` — two mechanisms reading the same fact two different, driftable ways.
- **`cargo-deny` bans list.** Considered: `deny.toml` already gates advisories/licenses/sources workspace-wide, and a `[bans]` deny-list keyed on crate name pairs could express this. Rejected for now because `cargo-deny`'s ban graph is workspace-global (crate A cannot depend on crate B), not *directional-with-exceptions* (core cannot depend on material, but `flui-app` can) without per-crate `skip`/`wrappers` configuration that is harder to read at a glance than the exemption set in one Python block. Revisit if `cargo-deny`'s directional-ban support matures.
