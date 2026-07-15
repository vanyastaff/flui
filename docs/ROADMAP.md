[← Foundations](FOUNDATIONS.md) · [Port Methodology](PORT.md) · [Strategy](../STRATEGY.md) · [Back to README](../README.md)

# FLUI Port Roadmap

> The dependency-ordered construction plan for porting Flutter to Rust — from the current codebase to full Flutter parity. It is written **backward from the finished product**: released Flutter is the specification, full parity is the destination, and the phases are the dependency-correct path to it. Progress is measured as **parity against `.flutter/`**, not as crates touched.

This roadmap sits on top of [`FOUNDATIONS.md`](FOUNDATIONS.md) — the architecture contract. The foundations say *what* is built and *to what rules*; this document says *in what order*. Phases are ordered purely by dependency correctness and risk. There are **no calendar dates** — a phase is done when its exit criteria are objectively met.

---

## Status at a glance *(verified against the codebase 2026-07-14)*

| Phase | Status |
|---|---|
| Core.0 — spine to spec | ✅ **Complete.** Pipeline phases wired and tested, keyed reconciliation production-wired, contracts locked, gate green. |
| Core.1 — vertical slice | ✅ Slice widgets, contract validation, combined demo app + acceptance tests, parity ports, frame evidence, drag-to-scroll — all delivered. |
| Core.2 — render-object catalog | ✅ **79 of ~80** objects built with harness tests in `crates/flui-objects`, incl. `RenderAnimatedOpacity`/`RenderSliverAnimatedOpacity`; exit verification (scrolling test, intrinsics audit, coverage ≥80%) met. |
| Business.1 — widget catalog | ◐ Every named catalog widget implemented and integration-tested; **fidelity** (ported parity corpus) and named `Hero` gaps remain. |
| Catalog.1 — Material ∥ Cupertino | ✗ Not started — `flui-material`, `flui-cupertino`, `flui-localizations` do not exist yet. |
| App.1 — application integration | ◐ `run_app`, both bindings, and a wake-driven frame loop exist; true vsync pacing, IME, and the facade crate remain. |

---

## The destination

The target is **full parity with released Flutter** — every framework package, adapted to Rust-native structure and improved where Rust permits ([`FOUNDATIONS.md` Part II](FOUNDATIONS.md#part-ii--where-flui-is-better-than-flutter)). Released Flutter is ~480k LOC of framework logic across 12 packages. FLUI today covers an estimated **~35–40% of Flutter's framework logic** (a coverage-weighted estimate of what is built and working — not raw LOC ÷ raw LOC; the raw FLUI workspace is ~292k Rust code LOC in `crates/` as of 2026-07-14, ~295k including `examples/` and `tools/`, but that includes tests and scaffolding). The estimate is sharply bimodal in a new place: the render *machine*, render-object *catalog*, and framework widget *catalog* are largely built; the design-system layer (`material`/`cupertino`) is ~0%.

**Parity scoreboard** — every Flutter package, its size, current coverage, and the phase that brings it to parity. Percentages are estimates unless marked as a mechanical count:

| Flutter package | Logic LOC | Today | Brought to parity by |
|---|---:|---:|---|
| `foundation` | 11.4k | ~95% | Core.0 ✅ |
| `physics` | 0.9k | **verified** — parity audit at [`research/2026-06-30-physics-parity-audit.md`](research/2026-06-30-physics-parity-audit.md); code in `flui-types/src/physics/` | Core.0 ✅ |
| `scheduler` | 2.2k | ~95% | Core.0 ✅ |
| `gestures` | 14.3k | ~95% | Cross.H |
| `semantics` | 7.9k | ~70% | Cross.H |
| `animation` | 5.3k | ~85% | Core.1 (active workspace member) |
| `painting` | 24.9k | ~60% | Core.0 → Core.2 |
| `rendering` | 52.1k | machine ~90%; catalog **79/~80 harness-tested** (`crates/flui-objects`) | Core.2 ✅ |
| `widgets` | 157.4k | spine ~85%; catalog built (est. ~70% coverage), **fidelity partial** | Business.1 (fidelity) |
| `services` | 30.2k | ~40% | App.1 + Cross.P (dissolved into `flui-platform`) |
| `material` | 210.8k | ~1% | Catalog.1 |
| `cupertino` | 48.3k | ~0% | Catalog.1 |

The shape of the work: the machinery and framework catalog are largely landed; the remaining mass is **fidelity** (the ported Flutter test corpus) **and the design-system catalog.** `material` alone (210k LOC) is the terminal node and the single largest body of work in the entire port — roughly twice the rest of the catalog combined.

**The critical path:** close Business.1 fidelity residues → `flui-localizations` + theming → `flui-material`. Everything else is a parallel tributary or hangs off the end.

---

## What parity means and how it is measured

"Full parity with released Flutter" is the destination. To prevent that destination from being an undefined "we're done when it feels done," parity is given a **falsifiable definition** and an **acceptance oracle** — without these "one march to 100%" has no forcing function and "no intermediate release" means no release ever.

**Definition.** A FLUI widget, render object, or subsystem is at **parity** with its Flutter counterpart when:
1. It implements the same behavior contract — the same `.flutter/` algorithm, lifecycle, and observable semantics (per [`FOUNDATIONS.md` Part I](FOUNDATIONS.md#part-i--the-target-architecture) "behavior loyal").
2. It passes the **adapted Flutter widget-test corpus** for that component (the oracle below).
3. It satisfies the FLUI-side quality bar: the coverage threshold for its crate category (Core ≥ 80%, Platform ≥ 70%, Widget ≥ 85% — checked locally via `just coverage`; there is currently **no CI coverage gate**), all `port-check.sh` refusal triggers green (22 numbered triggers plus the named guards as of 2026-07-14), no `unimplemented!()`/`todo!()` in its code path.

**The oracle.** Flutter ships an enormous widget-test corpus under `.flutter/flutter-master/packages/flutter/test/`. Each widget has a `_test.dart` companion exercising layout, paint, gestures, edge cases, accessibility. The parity oracle for FLUI is: **the corresponding Flutter `_test.dart` is ported (mechanically where possible, behavior-faithfully always) and passes against the FLUI widget**. This is the same "behavior loyal, structure Rust-native" rule applied to tests. The ported corpus lives at `crates/<crate>/tests/parity/` and runs in CI — it is live today in `crates/flui-widgets/tests/parity/`.

**Measurement.** The parity scoreboard at the top of this document reports a **coverage estimate** — what is built and working, weighted by component importance. With the ported test corpus online, the scoreboard's second dimension is **fidelity** = fraction of ported Flutter tests passing. A package is at full parity only when **both** reach 100% — coverage answers "how much is built," fidelity answers "and does it behave like Flutter." Coverage alone is not done. **Fidelity is now the roadmap's main open front for `widgets`:** coverage ran ahead of the ported corpus.

**Why this matters for "one march to 100%."** Without a definition, "100%" is unreachable — the last 5% is unbounded. With this definition, the last failing ported Flutter test is the gate; when none fail against the corresponding FLUI widget, the package is done. There is a forcing function, and "no intermediate release" no longer means "no forcing function."

---

## The four layers

FLUI's construction divides into **four architectural layers**. Cross supports all the others and runs continuously; the main vertical is **Core → Business → Catalog** with Application integration as the top.

```
                    ┌────────────────────────────┐
                    │      App. integration      │
                    │    flui-app + facade       │
                    └─────────────▲──────────────┘
                                  │
                    ┌─────────────┴──────────────┐
                    │          CATALOG           │
                    │  flui-material ∥           │
                    │  flui-cupertino            │
                    │  flui-localizations        │
                    └─────────────▲──────────────┘
                                  │
                    ┌─────────────┴──────────────┐
                    │         BUSINESS           │
                    │  flui-widgets              │
                    │  (~80-widget user-facing   │
                    │   framework catalog)       │
                    └─────────────▲──────────────┘
                                  │
                    ┌─────────────┴──────────────┐
                    │           CORE             │
                    │  Render machine —          │
                    │  rendering / view / engine │
                    │  / layer / painting        │
                    │  + flui-objects (the       │
                    │  ~80-object catalog)       │
                    │  + flui-binding (headless  │
                    │  frame driver / harness)   │
                    └─────────────▲──────────────┘
                                  │
        ┌─────────────────────────┴───────────────────────────┐
        │                       CROSS                         │
        │  Cross-cutting infrastructure (continuous):         │
        │  foundation / types / geometry / tree / macros /    │
        │  scheduler / platform / interaction (gestures) /    │
        │  semantics / animation / assets / DX tooling /      │
        │  refusal triggers / the standing quality discipline │
        └─────────────────────────────────────────────────────┘
```

- **Cross** — the substrate the rest stands on. Foundation hardening, platform backends, animation/physics, asset pipeline, DX tooling (devtools/build/cli/hot-reload), the refusal triggers, the standing audit-driven quality discipline. Not a phase — it runs the whole duration in tracks.
- **Core** — the render machine. The spine from `View` through `Element` reconciliation through `RenderObject` layout/paint to layered compositing, plus the concrete render-object catalog (`flui-objects`) and the deterministic headless frame driver (`flui-binding`). Includes the contract design docs (the *core* the widget catalog commits to).
- **Business** — `flui-widgets`. The ~80-widget framework catalog — `Container`/`Row`/`Text`/`Image`/`ListView`/`GestureDetector`/`Navigator`/`Focus`/`AnimatedContainer`/.... The thing an app author composes. Depends on Core + Cross.
- **Catalog** — design-system component libraries. `flui-material` ∥ `flui-cupertino` (independent siblings) + the `flui-localizations` they share. The thing app authors typically import.
- **App** — `flui-app` + the `flui` facade. The top-level binding wiring platform vsync → frame loop → Catalog. The phase that ships a real app.

One crate directory is deliberately **outside** the workspace: `flui-reactivity` (signals/hooks experiment) is excluded from `[workspace.members]` because signals are locked out by contract C1 — it re-enters only if signals are ever integrated into the view layer (rationale in the root `Cargo.toml` note).

The phases below sequence construction across these layers. Phase headings use the layer prefix so a reader sees at a glance which layer the work belongs to.

---

## Construction strategy — spine-first vertical slice

The phases follow the dependency graph, with one deliberate shape decision. After the spine was brought to target spec (Core.0), construction did **not** go straight to breadth. Core.1 built a thin **vertical slice** — a handful of widgets, one per render-object family, end-to-end — *before* the wide render-object and widget catalogs.

The reason is risk. `material` is 210k LOC; an architecture-contract flaw discovered mid-`material` is a catastrophe. The vertical slice exercises every locked contract ([`FOUNDATIONS.md` Part III](FOUNDATIONS.md#part-iii--the-locked-contracts)) and the whole build → layout → paint → composite → reconcile pipeline on **live widget code**, cheaply, before the expensive breadth. A paper contract is not proven; a contract a running app depends on is. This strategy has largely played out as designed — the slice landed, then the catalogs followed — with the combined slice demo app the one piece of it still owed (see Core.1).

---

## How to read a phase

Each phase states: **Goal**, **Status**, what was **Delivered** (for closed work) and what **Remains** (objective, testable criteria — a phase is not done until every item is verifiably true), and **Parity delta** (which `.flutter/` packages it advances). Phases on the critical path are strictly sequential; tracks run in parallel throughout.

---

## Core.0 — Spine to target spec  *(was Phase 0)* — ✅ COMPLETE

**Goal.** Bring the render spine up to its target specification ([`FOUNDATIONS.md` Part I](FOUNDATIONS.md#part-i--the-target-architecture)) and lock the architecture contracts the catalog commits to.

**Delivered** (each item verified in code, 2026-07-14):

- **Layout / compositing / paint phases wired to spec.** `run_layout` performs real per-node layout with parent→child constraint propagation (`flui-rendering/src/pipeline/owner/layout.rs` — it replaced the legacy no-op recursion); `run_compositing` implements the full Flutter subtree compositing-bits walk (`pipeline/owner/compositing.rs`); `run_paint` clears the dirty flag only on nodes it paints (`pipeline/owner/paint.rs`). Each has a dedicated integration-test file: `run_layout_wiring.rs`, `compositing_bits_walk.rs`, `paint_dirty_flag_discipline.rs`.
- **Keyed reconciliation production-wired.** The element node carries `key: Option<Box<dyn ViewKey>>`; variable-arity reconciliation routes through the keyed algorithm from the build owner. Positional matching survives only as the Flutter-correct fallback for *keyless* children inside the keyed algorithm — no separate legacy positional path remains. Reordering keyed children preserves element identity through the production build path (`flui-view/tests/production_reconcile_emits.rs`).
- **Core contracts locked** — heterogeneous children (C2, both `ViewSeq` paths), the widget-authoring API (C3), and the `View` trait / element storage / keyed reconciliation core (C4+C6). The contracts were designed as one unit because they cannot be locked independently (the `ViewSeq` shape forces the reconciler signature; authoring ergonomics couple C3 ↔ C4; element storage couples C2 ↔ C4). The contract design documents are merged into `docs/designs/`.
- **`RasterBackend` seam** in `flui-engine` (`src/raster.rs`; lyon stays the implementation; the seam makes a future Vello swap non-breaking). Design doc: `docs/designs/2026-06-30-rasterbackend-seam.md`.
- **`Scene` / `DrawCommand` contract frozen** with an explicit change protocol (`flui-painting/src/display_list/command.rs`; `docs/designs/2026-06-30-scene-drawcommand-contract.md`), so engine work parallelizes safely.
- **Physics parity-audited.** `flui-types/src/physics/` (spring/friction/gravity/tolerance) audited against Flutter's `physics` package: two bugs fixed, four intentional divergences documented, behavior tests passing. Report: [`research/2026-06-30-physics-parity-audit.md`](research/2026-06-30-physics-parity-audit.md).
- **Widget → render-object mapping checklist** delivered at [`research/widget-renderobject-map.md`](research/widget-renderobject-map.md) (it gated Core.2 entry).
- **Structural do-nows.** The `flui-geometry` split landed; standalone `flui-log` was removed from the workspace.
- **Standing discipline installed.** `port-check.sh` enforces 22 numbered refusal triggers plus the named guards (FR-033, FR-033/widgets, N-geom.U16, Cross.H2/H3/H7); the gate is green. New mechanically-detectable architecture rules must land in [`PORT.md`](PORT.md) and the script together.

**Exit verification.** `cargo build`/`clippy` green, `bash scripts/port-check.sh -v` exits 0 with all 22 triggers reporting green, zero `unimplemented!()`/`todo!()` in non-test code (grep gate in CI), and the four named integration-test areas exist (layout wiring, keyed-reorder identity, compositing-bits propagation, repaint-boundary dirty-clear). The exact 3-widget `Padding → Center → ColoredBox` tree named in the original exit is covered by equivalent-or-stronger tests (`flui-widgets/tests/layout.rs` per-widget size assertions; `flui-rendering/tests/pipeline_scenarios.rs` deep-chain constraint propagation) rather than one literal composed test.

**Parity delta.** `foundation`, `scheduler` → ~95%+; `rendering` *machine* → spec-complete; `widgets` *spine* → spec-complete; `physics` → verified.

---

## Core.1 — Vertical slice (Core × Business integration)  *(was Phase 1)* — ✅ COMPLETE

**Goal.** Prove the locked contracts and the full pipeline on live widget code: ~8–12 widgets — one per render-object family — end-to-end, into a running demo app.

**Delivered.**
- **All slice widgets implemented and integration-tested** in `flui-widgets`: `Container`/`Padding`/`Center` (box layout), `Column`/`Row` (flex, generic over children with `Vec<BoxedView>` as the default type parameter — the mandatory dynamic path), `Text` (`RenderParagraph` over the cosmic-text stack), `GestureDetector` (input/hit-testing), `SingleChildScrollView` (viewport/offset), dynamic `ListView` (keyed reconciliation on reorder; `tests/lazy_list.rs`), `AnimatedContainer` + `AnimatedOpacity` (`flui-animation` + the `memoize`/`can_update` short-circuit), and a `StatefulView` counter (C1 `setState`).
- **Contract-validation report** at [`research/2026-06-30-phase1-contract-validation.md`](research/2026-06-30-phase1-contract-validation.md), listing per contract the test that proved it. C8 (async edges) and C9 (type-erasure boundary) are framework invariants — validated by `port-check.sh` triggers, not per-widget tests.
- **The parity oracle is live**: `crates/flui-widgets/tests/parity/` (25 files) covers Container, Center, Column/Row, Text, ListView, and the stateful counter.
- **The combined demo app**, `examples/vertical_slice_demo/` — a stateful counter, a scrollable list, a gesture-responsive "+" button, and an `AnimatedContainer` box, all in one tree — with a shared-tree acceptance test at `tests/vertical_slice_demo.rs` (`#[path]`-includes the example's tree and mounts it headlessly, so the test exercises the exact tree the example runs).
- **Parity-test ports for the four remaining slice widgets** — `Padding`, `GestureDetector`, `SingleChildScrollView`, and the implicit-animation pair — ported into `tests/parity/` at Flutter tag `3.44.0`. Porting `SingleChildScrollView`'s cases exposed and fixed a missing `reverse` option.
- **Frame-time evidence for the 60 fps assertion** — measured on a real winit-backed Wayland window (2026-07-14): the demo's implicit animation over a 25 s run shows inter-tick cadence median 16.72 ms / p90 16.78 ms / max 16.81 ms across three post-warmup 300-sample windows — a locked ~59.8 fps with zero dropped ticks. The original "≤ 16 ms median" phrasing conflated the per-frame budget with tick cadence: a steady 60 Hz cadence sits at ~16.7 ms by definition, so the honest criterion — the animation sustains 60 fps on the real frame loop — is met. The loop is wake-driven; true vsync pacing (`ControlFlow::Wait`) remains App.1's exit criterion.
- **Drag-to-scroll for the demo list** (2026-07-14): pointer drags over the `ListView` scroll it through `GestureDetector` pan → `ScrollController`, with clamped extents, proven by a red→green drag acceptance test against the shared demo tree. **Updated 2026-07-15 (back on `ListView`):** the demo list is a `ListView` driven by `.position(controller.position())` — `ListView`'s own `.position(...)` passthrough (delivered under Business.1's "Scrollable composition + content-dimension feedback" item below) closed the gap that previously forced this demo onto a hand-composed `Viewport` + `SliverFixedExtentList` tree; `RenderViewport::perform_layout`'s committed content extents still flush back into the controller through the same loop, now reached through the ordinary widget.

**Parity delta.** `widgets` catalog seeded; `animation` exercised end-to-end; the pipeline is proven on live widget code.

---

## Core.2 — Render-object catalog  *(was Phase 2)* — ✅ COMPLETE

**Goal.** Build the ~80-object render catalog. Every widget is a thin configuration object over a render object — this was the hidden bottleneck under the widget catalog, and it has been substantially cleared.

**Where it lives.** The catalog is the dedicated **`crates/flui-objects`** crate (extracted out of `flui-rendering` — the machine and the catalog are now separate crates). `flui-rendering` keeps only machine types (`RenderTree`, `RenderState`, `RenderView`, `RenderTester`, …).

**Delivered.** **79 exported render-object types** (74 concrete structs plus type aliases such as the `RenderClipRect`/`RRect`/`Oval`/`Path` family over a generic clip base), enumerated in the `RENDER_OBJECT_TYPES` harness catalog (`crates/flui-objects/tests/render_object_harness.rs`) whose CI guard asserts the list matches the crate's `pub use` exports, with per-type `harness_*` tests. Every family the phase named is implemented:
- **Box layout** — `RenderStack`/`RenderIndexedStack`, `RenderConstrainedBox`/`RenderLimitedBox`, `RenderAspectRatio`, `RenderBaseline`, `RenderWrap`, `RenderFlow`, `RenderTable`, `RenderFractionallySizedBox`. (Flutter's `Positioned` is a `ParentDataWidget` over `RenderStack`, not a render object — nothing is missing there.)
- **Paint effects** — the clip family, `RenderDecoratedBox`, `RenderOpacity` (+ `RenderSliverOpacity`), `RenderAnimatedOpacity` (+ `RenderSliverAnimatedOpacity`), the `RenderTransform` family (+ `RenderFractionalTranslation`, `RenderRotatedBox`), `RenderCustomPaint`, `RenderRepaintBoundary`, `RenderPhysicalModel`/`Shape`.
- **Slivers** — `RenderViewport` + `RenderShrinkWrappingViewport`, `RenderSliverList`/`Grid` (each with lazy variants), `Padding`/`FillViewport`/`ToBoxAdapter`, `RenderSliverFixedExtentList`, three `FillRemaining` variants, four persistent-header variants, `Offstage`/`Opacity`/`AnimatedOpacity`/`IgnorePointer`.
- **Input / leaf** — `RenderParagraph` + `RenderEditable`, `RenderImage`, `RenderMouseRegion`, `RenderListBody`, and Flutter's `RenderPointerListener` ported as **`RenderListener`**.
- **`RenderAnimatedOpacity`/`RenderSliverAnimatedOpacity`** (the last named gap) ported the mixin's alpha-caching/dirty-marking contract with one **documented divergence**: Flutter's mixin is a retained-layer node — a tick calls `updateCompositedLayer` to blend the existing `OpacityLayer` in place, so it never repaints the child subtree, only re-composites. FLUI has no composited-layer-update machinery (`updateCompositedLayer`/`markNeedsCompositedLayerUpdate` do not exist anywhere in `flui-rendering`/`flui-objects`), so a tick here marks a full repaint whenever the effective alpha changes, plus a compositing-bits mark when it crosses the layered/unlayered boundary. This is a real, currently-open pipeline gap, not a hidden shortcut — tracked as a named Cross.H item below, not swept back into Core.2.

**Exit verification (all met).**
- **Scrolling.** `scrolling_lazy_sliver_keeps_materialized_band_bounded_and_windowed` (`crates/flui-objects/tests/harness_snapshot.rs`) scrolls a 1 000-item `RenderSliverListLazy` from the head to a mid offset (~item 500) to the tail, asserting at each stop that the materialized child band is both bounded (laziness — distant items are never attached) and correctly windowed (the band tracks the scroll position). Companion to the existing offset-0-only `snapshot_lazy_sliver_visible_band`.
- **Intrinsics audit.** Catalog families with non-trivial intrinsic-size logic: `flex` (harness-covered), `table` (column-width unit tests covered width; **added** a min/max-intrinsic-height test that also pins the oracle's own documented quirk — `computeMaxIntrinsicHeight` returns `computeMinIntrinsicHeight` verbatim), `wrap` (harness covered max-width only; **added** min-width and vertical-axis max-height tests), `aspect_ratio` (already covered by in-file unit tests), `list_body` (had zero intrinsic coverage; **added** width and height tests — the height test also pins a second oracle quirk: `computeMaxIntrinsicHeight`/`computeMinIntrinsicWidth` share the identical axis-keyed sum/max switch, so height does not independently reason about "am I the main or cross axis"), `paragraph` (already covered by in-file unit tests). 5 tests added (audit cap), 0 skipped as low-value.
- **Coverage** (`cargo llvm-cov --summary-only -p flui-objects -p flui-rendering`, 2026-07-14): `flui-objects` **81.41%** line coverage, `flui-rendering` **83.27%** line coverage — both ≥ the 80% Core threshold.

**Deliberately deferred (named, not silently dropped):**
- **Composited-layer pipeline gap** (no `updateCompositedLayer`/retained-layer alpha-blend-without-repaint path) — tracked under Cross.H below; it is a `flui-rendering` machine capability, not specific to the opacity pair, and would benefit any future retained-layer effect.
- **`AnimatedOpacity` widget rewiring** — delivered 2026-07-15: the widget now builds `RenderAnimatedOpacity` directly through an injected, hot-swappable `ProxyAnimation<f32>` (retarget = widget-side `set_parent`, the render object never sees controller or curve), and a probe test pins that animation ticks no longer rebuild the child subtree. Surfaced pre-existing gap, now named under Business.1: `ImplicitAnimation` ignores a changed `curve` on rebuild across all implicit widgets (Flutter re-creates the `CurvedAnimation`).

**Parity delta.** `rendering` catalog → ~95% coverage (mechanical count 79/~80); `painting` advanced correspondingly.

---

## Business.1 — Widget catalog  *(was Phase 3)* — ◐ BUILT, FIDELITY OPEN

**Goal.** Complete `flui-widgets` — the full ~80-widget user-facing catalog. The largest single new crate and the join point of every upstream phase.

**Delivered.** The named catalog beyond the slice is implemented with dedicated integration tests: full layout family, `RichText`/`DefaultTextStyle`, `Icon`, scrolling (`ListView`/`GridView`/`CustomScrollView`/`Scrollable` + scroll physics), input (`Listener`, `MouseRegion`, `Focus`/`FocusScope`, `Actions`/`Shortcuts`), `Navigator`/routing/`PageRoute`, the implicit-animation family, `Hero`, `MediaQuery`, `LayoutBuilder`, `FutureBuilder`/`StreamBuilder`, plus `TextField`/`EditableText`. `Hero` works end-to-end without cross-overlay `GlobalKey` reparenting: push/pop flights, divert, fade-out, default placeholder, `HeroControllerScope` auto-attach, the customization hooks (`create_rect_tween`, `flight_shuttle_builder`, FLUI's state-preserving `placeholder`, `curve`/`reverse_curve`), and `HeroMode` are public (2026-07-10).

**Remains.**
- **Fidelity** — porting the Flutter `_test.dart` corpus for the catalog into `tests/parity/`. This is the bulk of what separates "built" from "at parity." Scroll-family parity corpus ported (2026-07-15): `parity/scroll_controller_test.rs` (4 cases, incl. a `ScrollController::with_initial_scroll_offset` gap fix), `parity/scrollable_test.rs` (2 cases, min-boundary clamp/bounce symmetry), and 2 cases added to `parity/list_view_test.rs` — against `scrollable_test.dart`/`list_view_test.dart`/`scroll_controller_test.dart` (tag `3.44.0`), on top of the pre-existing `tests/scroll.rs` corpus (25 cases). Documented-out-of-scope: multi-position attach/detach, `animateTo`, `PageStorage`/`keepScrollOffset`, `isScrollingNotifier` — v1-deferred, no throw-on-ambiguous-read model in `ScrollController`. Flex/stack-family parity corpus ported (2026-07-15): 6 cases added to `parity/flex_test.rs` (`SpaceEvenly` exact positions, `Stretch` cross-axis fill, 2:1 flex-factor split, `CrossAxisAlignment::Baseline`, `MainAxisSize::Min` vs `Max`, and a documented divergence — FLUI degrades a flex child under an unbounded main axis to its natural size instead of raising Flutter's `FlutterError`) and a new `parity/stack_test.rs` (7 cases: non-positioned alignment, size-to-biggest-child, all three `StackFit` variants, `Positioned` left/top offsets, over-constrained all-four-edges stretch, `Positioned::fill`, and geometry-only overflow) — against `flex_test.dart`/`rendering/flex_test.dart`/`stack_test.dart` (tag `3.44.0`). Documented-out-of-scope: `Positioned.directional`/`PositionedDirectional` (no directional constructor exists on FLUI's `Positioned`) and `IndexedStack` visibility/offstage/focus-exclusion cases (out of scope for this geometry-only pass). Navigator/routes-family parity corpus ported (2026-07-15): new `parity/navigator_test.rs` (8 cases, on top of the pre-existing self-authored `tests/navigator_public.rs`/`tests/routes.rs`): route-level lifecycle callback order (`didAdd`/`didPush`/`didChangeNext` propagation on push, `didPop`/`didComplete`/`didPopNext`/dispose on pop), `pushReplacement`'s `didReplace` observer report and old-route disposal, `pushAndRemoveUntil`'s removal-set semantics, `RouteSettings`'s `Debug` contract, `remove_route` below the top completing its future without disturbing it, and observer route-*identity* (not just event-kind) across push/pop chains — against `navigator_test.dart`/`routes_test.dart` (tag `3.44.0`). Documented-out-of-scope: `popUntil`/`popUntilWithResult` (no `pop_until` on `NavigatorHandle`), `onGenerateRoute`/named-route generation/`RouteSettings.arguments` (no route-table mechanism, no `arguments` field), the declarative `Pages` API (`Navigator(pages:, onPopPage:)` — FLUI's `Navigator` is imperative-only), and `Navigator.of`/`maybePop` scoping cases already covered by `navigator_public.rs`.
- **Navigator API gaps** (surfaced by the parity port, 2026-07-15): `pop_until` and `RouteSettings.arguments` delivered 2026-07-15 (`NavigatorHandle::pop_until`, `RouteSettings::{with_arguments, arguments, argument}` — ADR-0024 §4.1's shape; §4.2's `GeneratedRoute`/generator-registration/`push_named` did not land, still gated). Still owed: `onGenerateRoute`/named-route generation, and the `Navigator` is imperative-only (no declarative `Pages` API).
- **Named `Hero` gaps** — user-gesture flights and cross-navigator flights, tracked in `ROADMAP-TRACKER.md` B1.4 / ADR-0021.
- **Scrollable composition + content-dimension feedback** — ✅ DELIVERED (2026-07-14). `ScrollPosition` (`flui-rendering`) is now a shared, `ViewportOffset`-and-`Listenable` handle: `Viewport`/`SingleChildScrollView::position(...)` inject it directly (Position mode), and `RenderViewport::perform_layout`'s `apply_viewport_dimension`/`apply_content_dimensions` write extents into it and flush a coalesced post-frame notification — no more manual `update_dimensions` hand-wiring. `ScrollController` rebased onto `ScrollPosition` (`ScrollController::position()`) with `update_dimensions` kept as the explicit out-of-frame path. `Scrollable::viewport_builder` (default `None` keeps the `SingleChildScrollView` fast path) lets a caller compose an arbitrary scrollable widget over the same shared position instead of a single child. Honest remainder, not silently dropped:
  - **`ListView`/`GridView` `.position(...)` passthrough** — ✅ DELIVERED (2026-07-15). Both widgets take `.position(ScrollPosition)` alongside the existing `.offset(f32)`, flowing to their composed `Viewport` exactly as `SingleChildScrollView::position` does, so they now participate in the feedback loop directly (a caller no longer has to drop to `Viewport` + the sliver types directly, or `Scrollable::viewport_builder`, just to inject a shared position).
  - **`ShrinkWrappingViewport` `.position(...)` passthrough** — ✅ DELIVERED (2026-07-15). `ShrinkWrappingViewport` (the widget) now mirrors `Viewport`'s `Pixels`-vs-`Position` `offset_source` mechanics 1:1, over `RenderShrinkWrappingViewport<ScrollPosition>` (which already had the `with_offset` injection constructor from the render-object symmetry work). `ListView`/`GridView`'s `shrink_wrap` arm passes the caller's offset source straight through instead of snapshotting `position.pixels()` once per rebuild — the prior limitation (documented in the previous entry) is closed: a shrink-wrapped list/grid built with `.position(...)` now joins the live content-dimension feedback loop the same as the non-shrink-wrap path, gesture/controller writes land without waiting for a rebuild, and `RenderShrinkWrappingViewport::perform_layout`'s committed extents flush back into the shared position.
  - **Listener-driven `markNeedsLayout` on the render side** — ✅ DELIVERED (2026-07-15). `RenderViewport`/`RenderShrinkWrappingViewport` now register a render-side `ViewportOffset` listener in `attach` (torn down in `detach`, re-registered on `set_offset` while attached) that calls `RepaintHandle::mark_needs_layout()` directly — Flutter parity: `RenderViewport`'s `offset.addListener(markNeedsLayout)` (`rendering/viewport.dart`). This closes a real bug, not just a perf gap: a bare `Viewport::position(...)` with no `Scrollable`/`AnimatedBuilder` anywhere had NO relayout path at all — `set_pixels` on the injected `ScrollPosition` was dead on arrival, since nothing observed the mutation (widget-rebuild dirty-marking was the only relayout trigger, and nothing was scheduling a rebuild). `Scrollable`/`ScrollController`-composed trees still rebuild via `AnimatedBuilder` as before (unchanged) — the render-side listener is a second, independent observer on the same shared `ScrollPosition`, not a replacement; loop-termination (the post-frame content-dimension flush now notifies both listeners) is bounded by `apply_content_dimensions`'s own epsilon-guarded dirty-marking, which only schedules a further flush on a REAL extent change.
- **`flui-assets` ↔ `Image` integration verification** — the crate is an active workspace member; confirm network + asset image and font loading through the `Image` widget end-to-end.
- ~~**Implicit-animation curve staleness**~~ — delivered: `ImplicitAnimation::retarget`/`ImplicitController::set_curve` now accept the curve and swap it onto the run in flight without restarting (Flutter parity: `implicit_animations.dart` `didUpdateWidget`/`_createCurve`), and `AnimatedOpacity` gates its proxy `set_parent` on the changed report so an unrelated rebuild no longer reallocates the tween/curved chain.
- Exit criteria to demonstrate: a non-trivial sample app built entirely from `flui-widgets` (no raw render objects) with a scrolling list, gesture button, implicit animation, and navigated route; `flui-widgets` coverage ≥ 85% via `just coverage`.

**Parity delta.** `widgets` catalog coverage largely in place; parity completes when the ported corpus passes.

---

## Catalog.1 — Material ∥ Cupertino  *(was Phase 4)* — ✗ NOT STARTED

**Goal.** The two design-system component libraries — `flui-material` and `flui-cupertino` — built **in parallel** (independent siblings; neither depends on the other).

**Builds.**
- **Create `flui-localizations`** — shared l10n infrastructure, a common ancestor both design systems need. (Catalog.1 prerequisite.)
- **Create `flui-material`** — Material Design 3: `ThemeData`/`ColorScheme`, the button family, `Scaffold`/`AppBar`/tabs, `TextField`, dialogs/sheets, `Card`, `Drawer`, `NavigationBar`, `DataTable`, `Chip`, `ListTile`, selection controls, `InkWell`/ripple. Internally phased by component family — **theming first** (it is the `InheritedWidget` foundation every other component reads), then buttons, inputs, navigation, data display. Material can ship usefully in increments.
- **Create `flui-cupertino`** — iOS components: `CupertinoApp`, scaffolds, `CupertinoNavigationBar`, buttons, pickers, `CupertinoTextField`, `CupertinoPageRoute` (the iOS swipe-back transition), action sheets.

**Entry.** Business.1 exit (`flui-widgets` complete and stable). The `BuildContext` inherited-data path must be fully wired (the Cross.H hardening item) — `Theme` is an `InheritedWidget`, needed by approximately Material widget #1.

**Exit.**
- A Material sample app (`Scaffold` + `AppBar` + `FloatingActionButton` + a `ListView` of `Card`s + a `Dialog`) renders and is interactive.
- A Cupertino sample app (`CupertinoTabScaffold` + `CupertinoNavigationBar` + a `CupertinoPageRoute` swipe-back) renders and is interactive.
- A `ThemeData` change in a tree of ≥1,000 widgets repaints exactly the dependents (the inherited-lookup dependent-set is touched, not the whole tree) — asserted by an integration test that counts rebuilds.

**Parity delta.** `material` → ~95%; `cupertino` → ~95%.

---

## App.1 — Application integration  *(was Phase 5)* — ◐ PARTIALLY BUILT

**Goal.** Bring `flui-app` to full parity as the top-level binding integrating the now-complete stack.

**Already in place.** A `runApp`-equivalent exists (`run_app`/`run_app_with_config`/`run_direct`, plus `run_app_android`), `WidgetsFlutterBinding` and `RendererBinding` are implemented, and a working **wake-driven** frame loop runs today (wake → `needs_redraw` → `RedrawRequested` → begin/draw frame) — `examples/animated_box_app.rs` runs on it.

**Remains.**
- **True vsync-driven, on-demand pacing.** Redraw requests currently carry no vsync (`ControlFlow::Wait` appears nowhere in `flui-app`/`flui-platform`); the exit criterion below is not yet met.
- `flui-platform` capability traits for the dissolved `services` responsibilities: `PlatformTextInput` (IME), `PlatformSystemChrome`, `PlatformHaptics`.
- Formalize the `flui` facade crate + `flui::prelude` (re-export the public surface of `flui-widgets`/`flui-material`/`flui-cupertino`).
- A full audit-and-repair pass on `flui-app` (it has had none).

**Entry.** Catalog.1 exit; at least one platform backend solid (Cross.P).

**Exit.**
- A full Material app runs on a native platform with a real vsync-driven, on-demand frame loop (`ControlFlow::Wait`).
- Text input via IME works (proves the `services` carve-out).
- Coverage thresholds met across the stack (`just coverage`).

**Parity delta.** `services` → ~90%; full-stack parity reached on the lead platform.

---

## Cross layer — continuous infrastructure

Cross is not a phase — it is the substrate that runs alongside Core / Business / Catalog for the whole duration. Four sub-tracks plus an upfront do-now batch.

### Cross.0 — Structural do-nows (one-time, upfront) — ✅ done

Bundled into Core.0 because they were cheap-now / catalog-wide-later: the `flui-geometry` split landed, standalone `flui-log` is absent from the workspace, and the refusal-trigger gate has grown to 22 numbered checks plus named guards in `port-check.sh`. Keep [`PORT.md`](PORT.md) and the script aligned as new mechanically-detectable rules land.

### Cross.A — Animation / assets / physics — ✅ mostly done

**Goal.** Keep the Cross-layer crates the Business catalog needs available and parity-tested. **Status:** `flui-animation` and `flui-assets` are active `[workspace.members]`; physics is parity-audited ([`research/2026-06-30-physics-parity-audit.md`](research/2026-06-30-physics-parity-audit.md)). **Remaining:** verify the `flui-assets` ↔ `Image` integration end-to-end (Business.1 item).

### Cross.P — Platform breadth

**Goal.** Complete `flui-platform` backends — finish Windows/macOS, add native **Android + iOS** (mobile-native is a `STRATEGY.md` first-class commitment); engine backend breadth (DX12/Metal/Vulkan/WebGPU surface management). The winit fallback is now routed as the Linux path (`current_platform()` → `WinitPlatform` via `flui-app`'s target-scoped `winit-backend` feature; real window + Vulkan surface verified 2026-07-14) — native Wayland/X11 backends remain open items. **Entry:** none beyond `flui-types`. **Exit:** a trivial app runs on Windows, macOS, Linux, Android, iOS, Web with per-platform smoke tests. Platform work gates only each phase's *final on-device demonstration*, never the headless construction of the widget/material layers.

### Cross.D — Developer tooling

**Goal.** `STRATEGY.md`'s DX track — complete `flui-devtools` (inspector, frame profiler), `flui-build` (Android/iOS/Desktop/Web builders), `flui-cli` (`flui new`/`build`/`run`); harden `flui-hot-reload`. All four crates are already active workspace members (and default-members) — the remaining work is **functionality**, not crate re-enablement. **Honest serialization:** several flagship deliverables are gated downstream — the **frame profiler** cannot complete until App.1 ships the full vsync-driven frame loop, and **`flui-build`** depends on Cross.P's mobile backends for the Android/iOS targets. `flui new` scaffolding and hot-reload hardening genuinely run in parallel; the headline DX features are partially serialized behind App.1 and Cross.P. **Exit:** `flui new`/`build`/`run` work; inspector + profiler functional; hot-reload preserves scene state. `STRATEGY.md`'s DX-day-1 ambition holds for the bookkeeping; the full-functionality bar lands post-App.1.

### Cross.H — Foundation hardening

**Goal.** The standing quality discipline — close the remaining systemic defects as the owning crates are next touched: the layer lifecycle protocol (gates App.1), parallel-type collapses, the `BuildContext` inherited-data hole (**gates Catalog.1** — `Theme` depends on it), the `TreeWrite::remove` cascade, Ticker lifecycle, and feature-gating of speculative scaffolding. Focus/tab navigation — originally on this list — has since landed in `flui-widgets` (`Focus`/`FocusScope`/`Actions`/`Shortcuts`, with tests). **Known gap:** gesture settings do not adapt to pointer type — `DragGestureRecognizer` always uses touch slop (18px) even for mouse input, where Flutter differentiates precise-pointer slop (surfaced by Core.1's drag-to-scroll work, 2026-07-14). **Known gap:** no composited-layer-update pipeline path — Flutter's `RenderObject.updateCompositedLayer`/`markNeedsCompositedLayerUpdate` let a retained layer (e.g. `OpacityLayer`) re-blend in place on a tick without repainting its subtree; FLUI has no equivalent anywhere in `flui-rendering`/`flui-objects`, so every such tick (currently `RenderAnimatedOpacity`/`RenderSliverAnimatedOpacity`, Core.2, 2026-07-14) pays a full repaint instead of a compositor-only blend — a documented, currently-accepted performance divergence, not a correctness one. **Entry:** continuous from Core.0. **Exit:** the foundation is declared stable — all critical-tier defects closed, second-tier addressed opportunistically. This is the audit-and-repair methodology as permanent discipline, not a bounded effort.

---

## Parallelism map

```
MAIN VERTICAL (sequential — Core → Business → Catalog → App):
  Core.0 ✅ ── Core.1 ✅ ── Core.2 ✅ ── Business.1 ◐ ── Catalog.1 ✗ ── App.1 ◐
                            (79/~80      (built;        (Material ∥     (partial:
                             objects;     fidelity +     Cupertino —     vsync, IME,
                             exit met)    Hero gaps)     not started)    facade left)

CROSS layer — continuous, with cross-track gates marked:
  Cross.P (platform)  ═════════════════════════════════════► joins App.1
       └──► Cross.P's mobile backends GATE Cross.D's flui-build

  Cross.D (DX tooling) ═════════════════════════════════════►
       inspector + frame-profiler BLOCKED until App.1
       ships the vsync-driven frame loop

  Cross.H (hardening) ══════════════════════════════════════►
       BuildContext inherited-data hole  ──► gates Catalog.1
       layer lifecycle protocol          ──► gates App.1

  Cross.A (animation/assets/physics) ✅ ──► assets↔Image verification joins Business.1
```

Within phases: Core.2's render-object families parallelize (box / sliver / paint-effect / input); Business.1's widget families parallelize (layout / scroll / input / animation / routing); Catalog.1 runs `flui-material` ∥ `flui-cupertino`.

---

## Ordering risks

| # | Risk | Mitigation |
|---|---|---|
| R1 | Widget catalog built on a spine not yet at target spec — defects compound across ~80 widgets, silently | **Realized as designed:** Core.0 was a hard gate with objective exit tests and is met; Core.1's vertical slice re-proved the spine under live load before breadth |
| R2 | Render-object catalog under-scoped — Business.1 stalls mid-widget on a missing render object | The widget → render-object checklist ([`research/widget-renderobject-map.md`](research/widget-renderobject-map.md)) was built before the catalog; Core.2 is complete — 79/~80 objects exist with harness tests |
| R3 | A contract flaw discovered inside `flui-material` (210k LOC) = catastrophic rework | Core.1's slice exercised every contract on live code; Catalog.1 starts only after the contract-validation report ([`research/2026-06-30-phase1-contract-validation.md`](research/2026-06-30-phase1-contract-validation.md)) is clean and Core.1's residues close |
| R4 | `flui-material` is one monolithic terminal phase | Phased internally by component family (theming → buttons → inputs → navigation → data); ships in increments; runs ∥ `flui-cupertino` |
| R5 | `Scene`/`DrawCommand` contract drift breaks the parallel engine track | The contract is frozen with a documented change protocol (`docs/designs/2026-06-30-scene-drawcommand-contract.md`); any later change is a coordinated cross-track change |
| R6 | Platform backends slip, blocking a phase's on-device demonstration | Cross.P started at Core.0 with the longest runway; phase exits can be met on desktop first, mobile as a follow-on demonstration |
| R7 | **Coverage outruns fidelity** — "built" reported as "at parity" while the ported Flutter test corpus lags | The scoreboard separates coverage from fidelity; a package is done only when the ported corpus passes (see "What parity means") |

---

## Governance — how a phase becomes work

This roadmap is the index, not the work. Each phase (and each large family within a phase) is decomposed **spec → plan → tasks → implement**, one directory per unit under `specs/` (name-based directories, e.g. `specs/animation-scheduling/`). Specs and plans are working artifacts, not durable authority: anything load-bearing they decide must be restated in the code, its tests, or a durable doc (`FOUNDATIONS.md`, `PORT.md`, this file) — a reader must never need a planning doc to understand why the code is shaped the way it is. The core contracts (C2, C3, C4+C6) were the first such units and are locked, with their design documents merged into `docs/designs/`.

Every phase exit is enforced by the standing discipline of [`FOUNDATIONS.md` Part VI](FOUNDATIONS.md#part-vi--the-standing-quality-discipline): `cargo build`/`clippy`/`test` green, `bash scripts/port-check.sh` green for all refusal triggers (22 numbered plus named guards today), coverage thresholds met via `just coverage`. A phase is not done because its code is written — it is done when its exit criteria are objectively verified.

Progress is reported as **parity against `.flutter/`** — the scoreboard at the top of this document is the live measure. The destination is the column on the right reading ~100% in both coverage and fidelity.

---

[← Foundations](FOUNDATIONS.md) · [Port Methodology](PORT.md) · [Strategy](../STRATEGY.md) · [Back to README](../README.md)
