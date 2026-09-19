# ADR-0009 — flui-widgets: the configuration-object widget catalog (Core.1 vertical slice)

- **Status:** Accepted (slice landed: box-layout + flex + Container + Text, 15 headless layout-parity tests green)
- **Date:** 2026-06-25
- **Scope:** new crate `flui-widgets` (L6, Business layer), consuming `flui-view` + `flui-objects` + `flui-rendering`/`flui-types`/`flui-geometry`/`flui-painting`
- **Relates:** realises [ROADMAP Core.1 — Vertical slice]; commits FOUNDATIONS contracts **C2** (heterogeneous children), **C3** (widget-authoring API), **C7** (`build()` infallible). Builds on [ADR-0008] (live `BuildContext`) and the `flui-objects` extraction (#316).

> Core.1's purpose is *risk reduction*: prove every locked contract and the whole build → layout → paint pipeline on **live widget code** before the 210k-LOC Material work leans on it. This ADR records the shape that proof took.

---

## Context

The render machine (layout/paint/compositing) and the View/Element spine were complete and at spec, but **no user-facing widget existed** — the layer an app author actually composes (`Container`, `Row`, `Text`, …) was 0%. The risk: an authoring-API or contract flaw discovered mid-catalog is a catalog-wide rewrite. Core.1 builds a thin end-to-end slice to flush that risk out cheaply.

Two facts shaped the design, both ground-truthed against the code, not assumed:

1. **The authoring API (C3) was already applied.** `StatelessView::build` and `ViewState::build` return `impl IntoView` (RPITIT); `RenderView` exposes `create_render_object`/`update_render_object`/`visit_child_views`; `ViewSeq` covers tuples `0..=16` and `Vec<BoxedView>`; the derives + `impl_render_view!` exist. So the slice builds *on* the spine, not *into* it.
2. **`flui-objects` has Leaf simplifications where Flutter has proxies.** `RenderSizedBox` and `RenderColoredBox` are `Leaf` (no child). A naïve `SizedBox`/`ColoredBox` over them would silently lose child support — a "MVP reported as parity" trap.

---

## Decision

**A widget is a small immutable configuration object over a render object (or a composition of widgets).** Three shapes, no inheritance simulation:

- **Render-object widget** — implements `RenderView` (+ `impl_render_view!`), wraps one `flui-objects` render box. Single child stored as `Child`; `.child(impl IntoView)`.
- **Multi-child render widget** — `Flex`/`Row`/`Column`, **generic over `C: ViewSeq`** with default `Vec<BoxedView>`. This is how **C2's two load-bearing paths** are served by one type: `column!`/`row!` produce a monomorphic tuple (`Flex<(A,B,C)>`), a `Vec<BoxedView>` carries a dynamic list. Generic widgets hand-write `impl View` via the crate-local `generic_render_view_element!` macro (the `impl_render_view!` macro can't express generic bounds).
- **Composition widget** — `Container` is a `StatelessView` whose `build` composes other widgets in Flutter's exact child-outward order. **(superseded — see "Amendment (2026-09-14): `Container` is a render-object widget, not a composition widget" below; `SafeArea` is the catalog's Composition-widget exemplar now.)**

**Parity over the available primitive, not the convenient one.** `SizedBox` → `RenderConstrainedBox` (tight constraints, exactly Flutter's implementation); `ColoredBox` → `RenderDecoratedBox` (color decoration, exactly `ColoredBox ≈ DecoratedBox(color)`). The Leaf `RenderSizedBox`/`RenderColoredBox` stay engine-demo primitives.

**Public surface (C3 ergonomics).** Constructor + chainable `#[must_use]` config methods (Flutter-like, discoverable); `f32` for dimensions at the call site, `Pixels` conversion internal; `bon` reserved for the widest future widgets. Children erased to `BoxedView`/`ViewSeq` at the widget boundary — the sanctioned C9 erasure points, keeping widget types non-generic (single child) or single-type-param (multi-child) and avoiding GPUI-style type explosion.

**The parity oracle is a headless view-level harness** (`tests/common/mod.rs`): mount a root widget → `build_scope` → locate the render-tree root as the parentless render node (uniform for `RenderView` and `StatelessView` roots) → real `run_frame` (the production `mem::take` + by-value typestate path) → assert `Size`/`Offset`. No `WidgetsBinding` singleton, so tests are parallel-safe — this is the Core.1 parity-oracle infrastructure the roadmap calls for.

---

## Consequences

- **Positive.** The whole pipeline is proven on live widget code (15 layout-parity tests: Padding/Align/Center/SizedBox/ColoredBox sizing+offset, Flex both C2 paths, Container composition incl. childless placeholder, Text headless shaping). C2/C3/C7 are exercised, not just asserted on paper. The authoring pattern is mechanical to extend — adding a render-object widget is ~60 lines + a test.
- **Negative / deferred (stated, not hidden).** `Container` does **not** fold a `BoxDecoration` border's thickness into layout padding (`flui-types`' `BoxDecoration` exposes no border insets) — documented on the type; set `padding` explicitly meanwhile. Parent-data widgets (`Flexible`/`Expanded`/`Positioned`), `Stack`, and the remaining single-child boxes (`AspectRatio`/`FittedBox`/`FractionallySizedBox`) are follow-on, not in this slice. Render objects with no setters are rebuilt wholesale in `update_render_object`; adding setters to `flui-objects` is a later optimisation.
- **Neutral.** The slice does not yet exercise C1 (`setState` stateful widget) or gestures end-to-end through a widget; those land with the stateful/interaction widgets.

---

## Alternatives rejected

- **Generic-over-child single-child widgets (`Padding<W: View>`).** Rejected: deeply nested concrete types (`Padding<Center<ColoredBox<…>>>`) explode compile times and type names — the exact GPUI/type-explosion failure FOUNDATIONS C9 erases at the slab boundary to avoid. Flutter erases here too; `Child` (a `BoxedView`) is the sanctioned point.
- **`SizedBox`/`ColoredBox` over the Leaf `RenderSizedBox`/`RenderColoredBox`.** Rejected: no child support — a parity regression masquerading as done.
- **Testing widgets at the render-object level (`RenderTester`).** Rejected: that re-tests `flui-objects`, not the widgets. The view-level harness is what proves *the widget* wires its render object and attaches its child.
- **Driving tests through `WidgetsBinding`.** Rejected: a process singleton (the CI `--test-threads=1` flake source). The direct `ElementTree` + `PipelineOwner` mount is parallel-safe.

---

## Amendment (2026-09-14): `Container` is a render-object widget, not a composition widget

§Decision named `Container` as the catalog's **Composition widget** exemplar — a `StatelessView` whose `build` composes other widgets in Flutter's exact child-outward order. That is no longer what `Container` is: it is now a `RenderView` over one `RenderContainer` (`flui-objects`), which carries every optional layer (alignment, padding, margin, color, decoration, additional constraints, transform) as a field instead of a conditionally-present widget level. `SafeArea` is the catalog's Composition-widget exemplar now — the crate table that cited this ADR (`crates/flui-widgets/README.md`) already points to it.

**Why.** Flutter's `Container.build` (`widgets/container.dart`) is the conditional stack this ADR described: `Align` → `Padding` → `ColoredBox` → `DecoratedBox` → `ConstrainedBox` → `Padding` (margin) → `Transform`, each level present only while its field is set. Toggling a field inserts or removes a level between the parent and the caller's child, so reconciliation diverges at that level and everything below it — including an unkeyed stateful child — is rebuilt from scratch instead of updated in place. Flutter has never fixed this (flutter/flutter#161698); a `Container` composed the way this ADR originally specified inherits the same hazard verbatim. Folding the optional levels into `RenderContainer` fields makes the child's slot structurally fixed: no level can appear or disappear, so no unkeyed child ever loses state from a cosmetic option changing. See `crates/flui-widgets/ARCHITECTURE.md` mapping decision 15 and issue #1096.

**The rule for the next case.** Composing widgets is still the default shape (§Decision's three shapes stand); this is a narrow exception, not a precedent to reach for casually. A composition widget may collapse into a single render object only when:

(a) its optional levels are conditional on caller-visible properties **and** its child slot is the caller's — i.e. toggling a property would otherwise insert or remove an element between the widget and an unkeyed caller-supplied child; **and**

(b) every level absorbed is pure box-protocol geometry/paint, with no build, no parent data, and no element identity of its own.

`Container` satisfies both: every level it absorbs (`Align`, `Padding`, `ColoredBox`, `DecoratedBox`, `ConstrainedBox`, `Transform`) is geometry/paint over the same caller-supplied child, none of them builds or owns parent-data, and the whole point is the reconciliation hazard in (a).

`SafeArea` and `Card` both fail (a), before (b) is ever reached: `SafeArea` is fixed-depth (always exactly one `Padding`), and `Card::build` (`crates/flui-material/src/card.rs`) is `Padding(margin).child(Material::new(color)...)` — also fixed-depth, since `margin` resolves through the card theme to a hard default and is never absent. Neither widget's structure ever changes shape under the caller, so there is no identity hazard in (a) to fix by collapsing either one. `ListTile` matches the reasoning as stated: it builds an icon/text `Row` whose members are genuinely conditional on which of `ListTile`'s optional slots (`leading`, `trailing`, …) are set, so it fails (b) instead — it builds child widgets with their own element identity, not box-protocol geometry/paint over the caller's child.

`Material` (`crates/flui-material/src/material.rs`) is not a composition widget at all — it is a `RenderView` (`type RenderObject = RenderPhysicalShape`) with no `build` — so neither clause applies to it; it is already the single render object this rule is about, not a candidate for becoming one.
