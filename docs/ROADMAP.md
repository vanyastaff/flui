[← Foundations](FOUNDATIONS.md) · [Port Methodology](PORT.md) · [Strategy](../STRATEGY.md) · [Back to README](../README.md)

# FLUI Port Roadmap

> The dependency-ordered construction plan for porting Flutter to Rust — from the current codebase to full Flutter parity. It is written **backward from the finished product**: released Flutter is the specification, full parity is the destination, and the phases are the dependency-correct path to it. Progress is measured as **parity against `.flutter/`**, not as crates touched.

This roadmap sits on top of [`FOUNDATIONS.md`](FOUNDATIONS.md) — the architecture contract. The foundations say *what* is built and *to what rules*; this document says *in what order*. Phases are ordered purely by dependency correctness and risk. There are **no calendar dates** — a phase is done when its exit criteria are objectively met.

---

## The destination

The target is **full parity with released Flutter** — every framework package, adapted to Rust-native structure and improved where Rust permits ([`FOUNDATIONS.md` Part II](FOUNDATIONS.md#part-ii--where-flui-is-better-than-flutter)). Released Flutter is ~480k LOC of framework logic across 12 packages. FLUI today covers an estimated **~22% of Flutter's framework logic** (a coverage-weighted estimate of what is built and working — not raw LOC ÷ raw LOC; the raw current FLUI workspace is ~236k LOC, but that includes tests, scaffolding, and code with known defects). The 22% is sharply bimodal: the render *machine* is 60–95% built, the user-facing layer is ~0%.

**Parity scoreboard** — every Flutter package, its size, current coverage, and the phase that brings it to parity:

| Flutter package | Logic LOC | Today | Brought to parity by |
|---|---:|---:|---|
| `foundation` | 11.4k | ~95% | Core.0 (hardening) |
| `physics` | 0.9k | **present, unaudited** | parity-audit in Core.0; code in `flui-types/src/physics/` |
| `scheduler` | 2.2k | ~95% | Core.0 |
| `gestures` | 14.3k | ~95% | Cross.H |
| `semantics` | 7.9k | ~70% | Cross.H |
| `animation` | 5.3k | ~85% (disabled) | Core.1 (re-enable) |
| `painting` | 24.9k | ~60% | Core.0 → Core.2 |
| `rendering` | 52.1k | machine ~90%, catalog ~12% | Core.0 (machine) + Core.2 (catalog) |
| `widgets` | 157.4k | spine ~85%, catalog ~2% | Core.0 (spine) + Core.1 / Business.1 (catalog) |
| `services` | 30.2k | ~40% | App.1 + Cross.P (dissolved into `flui-platform`) |
| `material` | 210.8k | ~1% | Catalog.1 |
| `cupertino` | 48.3k | ~0% | Catalog.1 |

The shape of the work: **front-loaded in machinery, back-loaded in catalog.** `material` alone (210k LOC) is the terminal node and the single largest body of work in the entire port — roughly twice the rest of the catalog combined.

**The critical path:** spine to target spec → render-object catalog → `flui-widgets` → `flui-material`. Everything else is a parallel tributary or hangs off the end.

---

## What parity means and how it is measured

"Full parity with released Flutter" is the destination. To prevent that destination from being an undefined "we're done when it feels done," parity is given a **falsifiable definition** and an **acceptance oracle** — without these "one march to 100%" has no forcing function and "no intermediate release" means no release ever.

**Definition.** A FLUI widget, render object, or subsystem is at **parity** with its Flutter counterpart when:
1. It implements the same behavior contract — the same `.flutter/` algorithm, lifecycle, and observable semantics (per [`FOUNDATIONS.md` Part I](FOUNDATIONS.md#part-i--the-target-architecture) "behavior loyal").
2. It passes the **adapted Flutter widget-test corpus** for that component (the oracle below).
3. It satisfies the FLUI-side quality bar: Constitution coverage threshold for its crate category, all 13 `port-check.sh` refusal triggers green, no `unimplemented!()`/`todo!()` in its code path.

**The oracle.** Flutter ships an enormous widget-test corpus under `.flutter/flutter-master/packages/flutter/test/`. Each widget has a `_test.dart` companion exercising layout, paint, gestures, edge cases, accessibility. The parity oracle for FLUI is: **the corresponding Flutter `_test.dart` is ported (mechanically where possible, behavior-faithfully always) and passes against the FLUI widget**. This is the same "behavior loyal, structure Rust-native" rule applied to tests. The ported corpus lives at `crates/<crate>/tests/parity/` and runs in CI.

**Measurement.** The parity scoreboard at the top of this document reports a single **coverage estimate** today — what is built and working, weighted by component importance. As the ported test corpus comes online (Core.1 starts it for the slice widgets; subsequent phases extend it), the scoreboard gains a second column: **fidelity** = fraction of ported Flutter tests passing. A package is at full parity only when **both** columns reach 100% — coverage answers "how much is built," fidelity answers "and does it behave like Flutter." Coverage alone is not done.

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
                    │  / layer / paint           │
                    │  + ~73 render objects      │
                    └─────────────▲──────────────┘
                                  │
        ┌─────────────────────────┴───────────────────────────┐
        │                       CROSS                         │
        │  Cross-cutting infrastructure (continuous):         │
        │  foundation / types / geometry / scheduler /        │
        │  platform / gestures / semantics / animation /      │
        │  assets / DX tooling / refusal triggers /           │
        │  the standing quality discipline                    │
        └─────────────────────────────────────────────────────┘
```

- **Cross** — the substrate the rest stands on. Foundation hardening, platform backends, animation/physics, asset pipeline, DX tooling (devtools/build/cli/hot-reload), the 13 refusal triggers, the standing Mythos-derived discipline. Not a phase — it runs the whole duration in tracks.
- **Core** — the render machine. The spine from `View` through `Element` reconciliation through `RenderObject` layout/paint to layered compositing. Includes the contract design docs (the *core* the widget catalog commits to). Must reach target spec before Business leans on it; the spine-first vertical slice validates Core's contracts on live code before catalog breadth.
- **Business** — `flui-widgets`. The ~80-widget framework catalog — `Container`/`Row`/`Text`/`Image`/`ListView`/`GestureDetector`/`Navigator`/`Focus`/`AnimatedContainer`/.... The thing an app author composes. Depends on Core + Cross.
- **Catalog** — design-system component libraries. `flui-material` ∥ `flui-cupertino` (independent siblings) + the `flui-localizations` they share. The thing app authors typically import.
- **App** — `flui-app` + the `flui` facade. The top-level binding wiring platform vsync → frame loop → Catalog. The phase that ships a real app.

The phases below sequence construction across these layers. Phase headings use the layer prefix so a reader sees at a glance which layer the work belongs to.

---

## Construction strategy — spine-first vertical slice

The phases follow the dependency graph, with one deliberate shape decision. After the spine is brought to target spec (Core.0), construction does **not** go straight to breadth. Core.1 builds a thin **vertical slice** — a handful of widgets, one per render-object family, end-to-end through to a running demo app — *before* the wide render-object and widget catalogs.

The reason is risk. `material` is 210k LOC; an architecture-contract flaw discovered mid-`material` is a catastrophe. The vertical slice exercises every locked contract ([`FOUNDATIONS.md` Part III](FOUNDATIONS.md#part-iii--the-locked-contracts)) and the whole build → layout → paint → composite → reconcile pipeline on **live widget code**, cheaply, before the expensive breadth. A paper contract is not proven; a contract a running app depends on is.

---

## How to read a phase

Each phase states: **Goal**, **Builds** (crates/subsystems), **Entry** (objective preconditions), **Exit** (objective, testable criteria — a phase is not done until every item is verifiably true), and **Parity delta** (which `.flutter/` packages it advances). Phases on the critical path are strictly sequential; tracks run in parallel throughout.

---

## Core.0 — Spine to target spec  *(was Phase 0)*

**Goal.** Bring the render spine from its current state up to its target specification ([`FOUNDATIONS.md` Part I](FOUNDATIONS.md#part-i--the-target-architecture)) and lock the architecture contracts. The render *machine* is already gold-standard; this phase finishes the *phases of that machine that were never wired*, and settles the contracts the catalog will commit to. It is the first stretch of construction — not a repair detour.

**Builds / completes.** The work splits into two groups. **Closing planned Mythos work does NOT close the NEW items** — Cycle 4 closed its own audit by deleting the layout/composite/paint stubs subtractively, leaving the hole; the new items below are named here for the first time and are unowned by any existing Mythos plan. Both groups must complete.

**NEW — construction unowned by any prior plan:**
- **Layout / compositing / paint phases wired to spec (D-1, D-3, D-4).** Wire `layout_node_with_children` to invoke per-node `RenderEntry::layout` with constraints propagated parent→child; implement `run_compositing` (the subtree compositing-bits walk); fix `run_paint` to clear the dirty flag only on nodes it paints. **The single most important new work in Core.0** — audited but on no Mythos schedule, per [`research/2026-05-22-architecture-correction-plan.md`](research/2026-05-22-architecture-correction-plan.md) §6.
- **Keyed reconciliation (D-2 / Contract C6).** Add `key: Option<Key>` to `ElementNode`; route variable-arity reconciliation through the keyed algorithm (already written, tested, with zero production callers); delete the positional path.
- **Core contracts spec** ([`specs/004-view-element-core/`](../specs/004-view-element-core/)) — a unified `/ce-plan` covering **C2** (heterogeneous children — **both** `ViewSeq` paths), **C3** (widget-authoring API), and the **C4+C6** core (`View` trait / element storage / keyed reconciliation). Originally scoped as three separate plans; unified after the 2026-05-22 doc-review found the four contracts cannot be locked independently — the `ViewSeq` shape forces the reconciler signature (C6); `impl IntoView` ergonomics force the authoring surface (C3 ↔ C4); element storage couples to the heterogeneous-children boundary (C2 ↔ C4); locking any one in isolation re-opens the others. **Round-5 implementation sequence: 4 PRs** — Phase 0 (3-day spec-validation benchmarks; S1 `KeyId` interning prototype + S2 static-path algorithm sketch; gates Phase 1 by re-opening FR-022 / FR-016 if benchmarks invert), Phase 1 (storage shape + key field + self-validation round-trip tests), Phase 2 (keyed reconciler completion + `ElementCore` rewiring + `ReconcileEvent` trace stream), Phase 3 (`IntoView` surface + `downcast_ref` elimination + derive macros + `port-check.sh` triggers). See `specs/004-view-element-core/spec.md` Implementation Sequence section for the full phase-by-phase FR map.
- **Standing discipline installed.** Write refusal triggers #8–#13 into [`PORT.md`](PORT.md); make the mechanically-detectable ones `port-check.sh` gates.
- **Structural do-nows.** Merge `flui-log` → `flui-foundation`; split `flui-geometry` out of `flui-types`; amend the Constitution layer table + edition/Rust-version line. *(Trivial now, catalog-wide later — [`FOUNDATIONS.md` Part IV](FOUNDATIONS.md#part-iv--the-target-crate-decomposition).)*
- **`RasterBackend` seam** in `flui-engine` (lyon stays the implementation; the seam makes a future Vello swap non-breaking).
- **Freeze the `Scene` / `DrawCommand` contract** so engine work parallelizes safely.
- **Widget → render-object mapping checklist** at `docs/research/widget-renderobject-map.md` — every planned `flui-widgets` widget mapped to its render object. Core.2 cannot start without it (mitigates Risk R2).
- **Parity-audit `flui-types/src/physics/`** against Flutter's `physics` package (Spring / Friction / Gravity simulation behavior) — code is present but unaudited; the scoreboard's "present, unaudited" becomes "verified" only after this.

**Closing already-planned Mythos work:**
- **Cycle 4** (rendering × engine) and **Cycle 5** (painting × view) close their remaining audit findings. **Sequence dependency** (round-5 alignment-check finding): Cycle 5's wrap-up MUST land before the unified contracts spec's Phase 0 starts — both touch `crates/flui-view/src/element/` and `crates/flui-view/src/tree/` files; overlapping work creates merge conflicts between Cycle 5 audit-finding closures and Phase 1's `ElementKind` migration. Cycle 5 owns the `flui-view` element/tree files until its closure, then hands off to the contracts-spec Phase 0.
- The queued **layer / semantics repair plan** ([`plans/2026-05-22-004-feat-layer-semantics-repair-plan.md`](plans/2026-05-22-004-feat-layer-semantics-repair-plan.md)) lands.

**Entry.** None — this is the current state.

**Exit.** Every item is a command that exits 0/1 or a test that passes/fails — no prose-only criteria.
- `cargo build --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` exit 0; `bash scripts/port-check.sh -v` exits 0 with all 13 triggers reporting green.
- Zero `unimplemented!()`/`todo!()` in non-test code (grep gate in CI).
- Integration test: a `Padding → Center → ColoredBox` 3-level tree lays out with correct constraints and computed sizes (gates D-1).
- Integration test: `[A(key=1), B(key=2)]` reordered to `[B, A]` preserves element identity (no remount) under a `Variable`-arity element (gates D-2).
- Integration test: a layer subtree marked dirty triggers compositing-bits propagation (gates D-3); a `RepaintBoundary`-isolated repaint clears `needs_paint` only on painted nodes (gates D-4).
- The three contract design documents (C2, C3, C4+C6) are merged into `docs/designs/`, each with explicit sign-off recorded in its frontmatter against the contract checklist in [`FOUNDATIONS.md` Part III](FOUNDATIONS.md#part-iii--the-locked-contracts).
- `flui-geometry` crate is in `[workspace.members]`; `flui-log` is removed from it; constitution version is bumped and the layer table matches [`FOUNDATIONS.md` Part IV](FOUNDATIONS.md#part-iv--the-target-crate-decomposition).
- The widget → render-object mapping checklist exists at `docs/research/widget-renderobject-map.md` (gates Core.2 entry).
- The `flui-types::physics` parity-audit report exists and reports all Spring / Friction / Gravity behavior tests passing against Flutter.

**Parity delta.** `foundation`, `scheduler` → ~95%+; `rendering` *machine* → spec-complete; `widgets` *spine* → spec-complete.

---

## Core.1 — Vertical slice (Core × Business integration)  *(was Phase 1)*

**Goal.** Prove the locked contracts and the full pipeline on live widget code. Build ~8–12 widgets — one per render-object family — end-to-end, into a running demo app.

**Builds.**
- **Create `flui-widgets`** (skeleton crate, L6) — populated with the slice only.
- **Re-enable `flui-animation`** — it is a near-complete port (~7.5k LOC) sitting disabled; the slice needs an animated widget.
- The slice widgets, each exercising a distinct render-object family and a distinct contract:
  - `Container` / `Padding` / `Center` — box layout (existing render objects).
  - `Column` / `Row` — flex, `Variable` arity → exercises C2 (`ViewSeq`) and C6 (keyed reconciliation).
  - `Text` — leaf + paint → forces `RenderParagraph` over the cosmic-text stack.
  - `GestureDetector` — input / hit-testing.
  - `SingleChildScrollView` — scroll → exercises the viewport/offset path.
  - A dynamic-count `ListView` (`Vec`-driven children) — exercises **C2's dynamic `Vec<BoxedView>` path**, the primary path for the entire scrolling/data-display half of the catalog (`ListView`/`GridView`/`DataTable`/etc.), and keyed reconciliation on reorder. **Mandatory** — without it Core.1 validates `ViewSeq` only where it works (tuple `Column`/`Row`) and skips where Material actually lives.
  - An implicitly-animated widget (`AnimatedContainer` or `AnimatedOpacity`) → exercises `flui-animation` + the `memoize`/`can_update` short-circuit.
  - A `StatefulView` counter → exercises C1 (`setState`).
- A demo app assembled entirely from slice widgets: a stateful counter, a scrollable list, a gesture-responsive button, an animated box.

**Entry.** Core.0 exit met — contracts locked, spine to spec.

**Exit.** Every item is a passing test or a verifiable artifact — not "approved" or "proven at scale" in prose.
- The demo app builds and runs on one desktop platform with a real frame loop.
- Each code-exercisable contract has a **passing test** that the corresponding slice widget activates: C1 (`setState`), C2 (**both** tuple and dynamic `Vec<BoxedView>` paths), C3 (`impl IntoView` + derive + `bon`), C4 (element storage / `enum ElementNode`), C5 (`BuildContext.depend_on::<Theme>`), C6 (keyed reconciliation under dynamic-list reorder), C7 (`build()` infallible + `catch_unwind` substitutes `ErrorView`). C8 (async edges) and C9 (type-erasure boundary) are framework invariants — validated by `port-check.sh` triggers, not per-widget tests.
- An implicit animation runs at 60fps driven by a real `Ticker` (assertion: frame-time histogram ≤ 16ms median over a 5-second run).
- **Contract-validation report** at `docs/research/2026-XX-XX-phase1-contract-validation.md` lists, per contract, the test that proved it and the test's status. The phase exits only when **every listed test passes** — the report's existence alone is not the gate.
- The ported Flutter test scaffolding for each slice widget exists at `crates/flui-widgets/tests/parity/` (the parity oracle infrastructure from "What parity means" goes live here).

**Parity delta.** `widgets` catalog → ~8%; `animation` → ~95%; the pipeline is proven end-to-end.

---

## Core.2 — Render-object catalog  *(was Phase 2)*

**Goal.** Build the ~73 missing render objects in `flui-rendering/src/objects/` (7 of ~80 exist today). Every widget is a thin configuration object over a render object — this is the hidden bottleneck under the widget catalog.

**Builds.** The Flutter `rendering/` render-object set, grouped into arity-correct, independently-parallelizable families:
- **Box layout** — `RenderStack`/`RenderPositioned`, `RenderConstrainedBox`/`RenderLimitedBox`, `RenderAspectRatio`, `RenderBaseline`, `RenderWrap`, `RenderFlow`, `RenderTable`, `RenderFractionallySizedBox`.
- **Paint effects** — `RenderClipRect`/`RRect`/`Path`/`Oval`, `RenderDecoratedBox`, `RenderOpacity` variants, `RenderTransform` family, `RenderCustomPaint`, `RenderRepaintBoundary`.
- **Slivers** — `RenderViewport`, `RenderSliverList`/`Grid`/`Padding`/`FillViewport`/`ToBoxAdapter` (the sliver constraint protocol is already typed in `flui-rendering` — this is "finish the job," not greenfield).
- **Input / leaf** — `RenderParagraph` (if not already done in Core.1), `RenderImage`, `RenderMouseRegion`, `RenderPointerListener`, `RenderListBody`.

**Entry.** Core.1 exit — the layout/paint pipeline is proven on live code.

**Exit.**
- A checklist mapping every planned `flui-widgets` widget to its render object, all present.
- Per-render-object layout + paint tests; intrinsic-size tests where applicable.
- A sliver integration test scrolls a 1,000-item list with correct lazy layout.
- `flui-rendering` coverage ≥ 80% (Constitution Core requirement).

**Parity delta.** `rendering` catalog → ~95%; `painting` → ~90%.

---

## Business.1 — Widget catalog  *(was Phase 3)*

**Goal.** Complete `flui-widgets` — the full ~80-widget user-facing catalog. The largest single new crate and the join point of every upstream phase.

**Builds.**
- The Flutter `widgets/` catalog beyond the slice: full layout family, `RichText`/`DefaultTextStyle`, `Icon`, scrolling (`ListView`/`GridView`/`CustomScrollView`/`Scrollable` + scroll physics), input (`Listener`, `MouseRegion`, `Focus`/`FocusScope`, `Actions`/`Shortcuts`), `Navigator`/routing/`PageRoute`, the implicit-animation family, `Hero`, `MediaQuery`, `LayoutBuilder`, `FutureBuilder`/`StreamBuilder`.
- **Re-enable `flui-assets`** — required for the `Image` widget (network + asset image, font loading).

**Entry.** Core.2 exit (render-object catalog complete).

**Exit.**
- A non-trivial sample app builds entirely from `flui-widgets` (no raw render objects): scrolling list, gesture button, implicit animation, navigated route.
- `Hero` + `GlobalKey` reparenting works end-to-end (keyed reconciliation under real load).
- `flui-widgets` coverage ≥ 85% (Constitution Widget requirement).

**Parity delta.** `widgets` catalog → ~95%.

---

## Catalog.1 — Material ∥ Cupertino  *(was Phase 4)*

**Goal.** The two design-system component libraries — `flui-material` and `flui-cupertino` — built **in parallel** (independent siblings; neither depends on the other).

**Builds.**
- **Create `flui-localizations`** — shared l10n infrastructure, a common ancestor both design systems need. (Catalog.1 prerequisite.)
- **Create `flui-material`** — Material Design 3: `ThemeData`/`ColorScheme`, the button family, `Scaffold`/`AppBar`/tabs, `TextField`, dialogs/sheets, `Card`, `Drawer`, `NavigationBar`, `DataTable`, `Chip`, `ListTile`, selection controls, `InkWell`/ripple. Internally phased by component family — **theming first** (it is the `InheritedWidget` foundation every other component reads), then buttons, inputs, navigation, data display. Material can ship usefully in increments.
- **Create `flui-cupertino`** — iOS components: `CupertinoApp`, scaffolds, `CupertinoNavigationBar`, buttons, pickers, `CupertinoTextField`, `CupertinoPageRoute` (the iOS swipe-back transition), action sheets.

**Entry.** Business.1 exit (`flui-widgets` complete and stable). The `BuildContext` inherited-data path must be fully wired (Cross.H item D-9) — `Theme` is an `InheritedWidget`, needed by approximately Material widget #1.

**Exit.**
- A Material sample app (`Scaffold` + `AppBar` + `FloatingActionButton` + a `ListView` of `Card`s + a `Dialog`) renders and is interactive.
- A Cupertino sample app (`CupertinoTabScaffold` + `CupertinoNavigationBar` + a `CupertinoPageRoute` swipe-back) renders and is interactive.
- A `ThemeData` change in a tree of ≥1,000 widgets repaints exactly the dependents (the inherited-lookup dependent-set is touched, not the whole tree) — asserted by an integration test that counts rebuilds.

**Parity delta.** `material` → ~95%; `cupertino` → ~95%.

---

## App.1 — Application integration  *(was Phase 5)*

**Goal.** Bring `flui-app` to full parity as the top-level binding integrating the now-complete stack.

**Builds.**
- `flui-app` — `WidgetsBinding`/`RendererBinding` integration completion, `runApp`-equivalent, the full frame loop wired from platform vsync → build → layout → paint → composite → present.
- `flui-platform` capability traits for the dissolved `services` responsibilities: `PlatformTextInput` (IME), `PlatformSystemChrome`, `PlatformHaptics`.
- Formalize the `flui` facade crate + `flui::prelude` (re-export the public surface of `flui-widgets`/`flui-material`/`flui-cupertino`).
- A Mythos cycle on `flui-app` (it has had none).

**Entry.** Catalog.1 exit; at least one platform backend solid (Cross.P).

**Exit.**
- A full Material app runs on a native platform with a real vsync-driven, on-demand frame loop (`ControlFlow::Wait`).
- Text input via IME works (proves the `services` carve-out).
- Constitution coverage gates met across the stack.

**Parity delta.** `services` → ~90%; full-stack parity reached on the lead platform.

---

## Cross layer — continuous infrastructure

Cross is not a phase — it is the substrate that runs alongside Core / Business / Catalog for the whole duration. Four sub-tracks plus an upfront do-now batch.

### Cross.0 — Structural do-nows (one-time, upfront)

Bundled into Core.0 because they are cheap-now / catalog-wide-later: merge `flui-log` → `flui-foundation`; split `flui-geometry` out of `flui-types`; amend the Constitution layer table + edition/Rust-version; install refusal triggers #8–#13 into [`PORT.md`](PORT.md) and make the mechanically-detectable ones `port-check.sh` gates. See Core.0 builds.

### Cross.A — Animation / assets / physics re-entry

**Goal.** Re-enable the Cross-layer crates the Business catalog needs. **Builds:** Re-enable `flui-animation` (the slice in Core.1 needs an animated widget — re-entry begins there); re-enable `flui-assets` before the `Image` widget in Business.1; parity-audit `flui-types/src/physics/` against Flutter (Core.0 deliverable). **Exit:** all three in `[workspace.members]`, parity-tested.

### Cross.P — Platform breadth

**Goal.** Complete `flui-platform` backends — finish Windows/macOS, complete the winit fallback, add native **Android + iOS** (mobile-native is a `STRATEGY.md` first-class commitment) and Wayland; engine backend breadth (DX12/Metal/Vulkan/WebGPU surface management). **Entry:** none beyond `flui-types`. **Exit:** a trivial app runs on Windows, macOS, Linux, Android, iOS, Web with per-platform smoke tests. Platform work gates only each phase's *final on-device demonstration*, never the headless construction of the widget/material layers.

### Cross.D — Developer tooling

**Goal.** `STRATEGY.md`'s DX track — re-enable and complete `flui-devtools` (inspector, frame profiler), `flui-build` (Android/iOS/Desktop/Web builders), `flui-cli` (`flui new`/`build`/`run`); harden `flui-hot-reload`. **Entry:** Core.0 (stable engine render path + element-tree introspection). **Honest serialization:** Cross.D *starts* after Core.0 but several flagship deliverables are gated downstream — the **frame profiler** cannot complete until App.1 ships the full vsync-driven frame loop, and **`flui-build`** depends on Cross.P's mobile backends for the Android/iOS targets. Re-enabling crates, `flui new` scaffolding, and hot-reload hardening genuinely run in parallel; the headline DX features are partially serialized behind App.1 and Cross.P. **Exit:** `flui new`/`build`/`run` work; inspector + profiler functional; hot-reload preserves scene state. `STRATEGY.md`'s DX-day-1 ambition holds for the bookkeeping; the full-functionality bar lands post-App.1.

### Cross.H — Foundation hardening

**Goal.** The standing quality discipline — close the P1/P2 systemic-defect tiers ([`research/2026-05-22-architecture-correction-plan.md`](research/2026-05-22-architecture-correction-plan.md)) as the owning crates are next touched: layer lifecycle protocol (D-7), parallel-type collapses (D-8), the `BuildContext` `new_minimal` hole (D-9 — **gates Catalog.1**), focus/tab navigation (D-10), `TreeWrite::remove` cascade (D-11), Ticker lifecycle (D-12), and the speculative-scaffolding feature-gating. **Entry:** continuous from Core.0. **Exit:** the foundation is declared stable — all P1 defects closed, P2 addressed opportunistically. This is the Mythos methodology as permanent discipline, not a bounded effort.

---

## Parallelism map

```
MAIN VERTICAL (sequential — Core → Business → Catalog → App):
  Core.0 ── Core.1 ── Core.2 ── Business.1 ── Catalog.1 ── App.1
            (slice)   (~73       (~80 widgets) (Material ∥
                       render                    Cupertino)
                       objects)

CROSS layer — continuous, with cross-track gates marked:
  Cross.P (platform)  ═════════════════════════════════════► joins App.1
       └──► Cross.P's mobile backends GATE Cross.D's flui-build

  Cross.D (DX tooling) ═════════════════════════════════════►
       starts after Core.0; inspector + frame-profiler
       BLOCKED until App.1 ships the wired frame loop

  Cross.H (hardening) ══════════════════════════════════════►
       D-9 (BuildContext new_minimal hole)  ──► gates Catalog.1
       D-7 (layer lifecycle protocol)       ──► gates App.1

  Cross.A (animation/assets/physics) ───► joins Core.1 (anim), Business.1 (assets)
```

Within phases: Core.2's render-object families parallelize (box / sliver / paint-effect / input); Business.1's widget families parallelize (layout / scroll / input / animation / routing); Catalog.1 runs `flui-material` ∥ `flui-cupertino`.

---

## Ordering risks

| # | Risk | Mitigation |
|---|---|---|
| R1 | Widget catalog built on a spine not yet at target spec — keyless reconciliation and an unwired layout phase compound across ~80 widgets, silently | Core.0 is a hard gate with objective exit tests; Core.1's vertical slice re-proves the spine under live load before breadth |
| R2 | Render-object catalog under-scoped — Business.1 stalls mid-widget on a missing render object | Core.2's exit is a checklist mapping every planned widget to its render object; build the checklist as a Core.0 deliverable |
| R3 | A contract flaw discovered inside `flui-material` (210k LOC) = catastrophic rework | Core.1's vertical slice exercises every contract on live code; Catalog.1 starts only after the slice's contract-validation report is clean |
| R4 | `flui-material` is one monolithic terminal phase | Phased internally by component family (theming → buttons → inputs → navigation → data); ships in increments; runs ∥ `flui-cupertino` |
| R5 | `Scene`/`DrawCommand` contract drift breaks the parallel engine track | The contract is frozen at Core.0 exit; any later change is a coordinated cross-track change |
| R6 | Platform backends slip, blocking a phase's on-device demonstration | Cross.P starts at Core.0 with the longest runway; phase exits can be met on desktop first, mobile as a follow-on demonstration |

---

## Governance — how a phase becomes work

This roadmap is the index, not the work. Each phase (and each large family within a phase) is decomposed through the [Speckit workflow](../CLAUDE.md#speckit-workflow): `spec → plan → tasks → implement`, one `specs/NNN-*` directory per unit. The three open contracts (C2, C3, C4+C6) are the **first** Speckit units — they are Core.0 deliverables and block Core.1.

Every phase exit is enforced by the standing discipline of [`FOUNDATIONS.md` Part VI](FOUNDATIONS.md#part-vi--the-standing-quality-discipline): `cargo build`/`clippy`/`test` green, `bash scripts/port-check.sh` green for all 13 refusal triggers, Constitution coverage thresholds met. A phase is not done because its code is written — it is done when its exit criteria are objectively verified.

Progress is reported as **parity against `.flutter/`** — the scoreboard at the top of this document is the live measure. The destination is the column on the right reading ~100%.

---

[← Foundations](FOUNDATIONS.md) · [Port Methodology](PORT.md) · [Strategy](../STRATEGY.md) · [Back to README](../README.md)
