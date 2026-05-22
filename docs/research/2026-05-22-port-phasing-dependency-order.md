---
title: "Port Phasing & Dependency-Order Analysis — Flutter → FLUI"
date: 2026-05-22
status: research
research_type: roadmap-input (port phasing, construction order, critical path)
crates_analyzed: all 21 (15 active, 6 disabled) + 5 missing crates proposed
reference_sources:
  - .flutter/flutter-master/packages/flutter/lib/src/ (12 packages, foundation→material)
  - cargo metadata --format-version 1 (verified crate DAG)
  - docs/research/2026-05-22-flui-foundation-tree-audit.md (Cycle 3)
  - docs/research/2026-05-22-flui-layer-semantics-audit.md (Cycle 2)
  - docs/research/2026-05-22-flui-painting-view-audit.md (Cycle 5)
  - docs/research/2026-05-22-flui-rendering-engine-audit.md (Cycle 4)
  - docs/research/2026-05-21-flui-interaction-scheduler-audit.md (Cycle 1)
  - docs/research/2026-05-21-view-tree-foundation-audit.md
  - STRATEGY.md (Tracks model), docs/PORT.md (Mythos methodology)
authors:
  - System Architect (via Claude Opus 4.7)
feeds: master Flutter→Rust port ROADMAP (one of four parallel research inputs)
---

# Port Phasing & Dependency-Order Analysis

> **Scope.** Determine the dependency-correct construction order from FLUI's current state to full Flutter parity. This is a build-systems analysis: critical path, topological order, what gates what. It is NOT time-boxed — phases are ordered purely by dependency correctness and risk. No calendar dates, no artificial "v1.0".

---

## 1. Intro & methodology

### 1.1 The question

FLUI is a Flutter→Rust port at ~236k LOC across 21 crates. The render machine (build → layout → paint → composite) is largely built and being hardened by the in-flight "Mythos" cycle. The **user-facing widget catalog is ≈0%**: `crates/flui-rendering/src/objects/` holds exactly **7 render objects** (`center`, `colored_box`, `flex`, `opacity`, `padding`, `sized_box`, `transform`) against Flutter's ~80+; there is no `flui-widgets` crate at all. The roadmap question is: **in what order do we build from here to full parity, and what is the longest dependency chain that gates everything else?**

### 1.2 How this analysis was produced

1. **Real crate DAG** extracted via `cargo metadata --format-version 1 --no-deps` — every `flui-*` edge, not the aspirational layering in `Cargo.toml` comments.
2. **Flutter package DAG** — the canonical Dart layering (`foundation → painting/gestures/scheduler/semantics → rendering → widgets → material/cupertino`), confirmed against `.flutter/flutter-master/packages/flutter/lib/src/`.
3. **Foundation solidity** — read all five Mythos cycle audits in `docs/research/` plus the four in-flight repair plans in `docs/plans/`. Each active crate is classified SOLID vs KNOWN-FRAGILE with the backing audit cited.
4. **Sizing** — Flutter `.dart` LOC per package counted directly (`wc -l`) as the parity-target proxy; FLUI crate `src/` LOC counted for current state.

### 1.3 Flutter package sizes (the parity target)

Measured from `.flutter/flutter-master/packages/flutter/lib/src/`:

| Flutter package | `.dart` LOC | files | FLUI crate | FLUI `src/` LOC | Coverage estimate |
|---|---:|---:|---|---:|---|
| foundation | 11,420 | 42 | flui-foundation + flui-types | 4,799 + 36,213 | High (types over-built) |
| gestures | 14,330 | 27 | flui-interaction | 19,137 | High |
| scheduler | 2,192 | 5 | flui-scheduler | 7,902 | High |
| painting | 24,890 | 48 | flui-painting | 8,341 | Partial (text/tess in engine) |
| semantics | 7,865 | 5 | flui-semantics | 6,619 | High |
| rendering | 52,118 | 48 | flui-rendering + flui-layer + flui-engine | 23,578 + 10,718 + 19,004 | Render-object **catalog ~10%** |
| animation | 5,283 | 8 | flui-animation (disabled) | 7,475 | High (disabled) |
| physics | 893 | 7 | *(missing)* | 0 | **0%** |
| widgets | 157,402 | 186 | flui-view + *(flui-widgets missing)* | 14,261 + 0 | **Framework spine only; catalog 0%** |
| material | 210,800 | 198 | *(missing)* | 0 | **0%** |
| cupertino | 48,253 | 52 | *(missing)* | 0 | **0%** |
| services | 30,226 | 52 | flui-platform + flui-assets (disabled) | 18,970 + 4,607 | Partial |

**The headline number:** the three packages that are 0% — `material` (210k), `widgets` *catalog* (~140k of the 157k is the widget catalog, not the framework spine FLUI has), `cupertino` (48k) — are **~400k LOC of Dart**, roughly **1.7× the entire current FLUI workspace**. The port is *front-loaded* in machinery and *back-loaded in catalog*. Everything below sequences that reality.

---

## 2. The dependency graph

### 2.1 Flutter's internal package DAG (the proven order)

Flutter's packages form a strict DAG. This is the order the port must respect at the *layer* level even where FLUI splits a package into several crates.

```
foundation
   │
   ├──────────────┬───────────────┬──────────────┐
   ▼              ▼               ▼              ▼
 gestures      scheduler       painting       (physics)
   │              │               │              │
   │              │               ▼              │
   │              │           semantics           │
   │              │               │              │
   └──────────────┴───────┬───────┴──────────────┘
                          ▼
                      rendering
                          │
                          ▼
                       animation ──► widgets
                          ▲             │
                          └─────────────┤
                          ┌─────────────┴─────────────┐
                          ▼                           ▼
                       material                   cupertino
                          (both depend on widgets + animation + gestures)

services ── cross-cuts: depended on by painting, rendering, widgets
            (platform channels, asset bundle, text input, system chrome)
```

Key Flutter facts that constrain FLUI:
- **`rendering` depends on `painting`, `gestures`, `semantics`, `scheduler`, `services`.** It is the convergence point — five upstream packages gate it.
- **`widgets` depends on `rendering` + `animation`.** The widget layer cannot exist before the render layer and the animation layer are both real.
- **`material` and `cupertino` are siblings**, both on top of `widgets`; `material` is 4.4× `cupertino` by LOC.
- **`physics` is tiny (893 LOC)** and feeds only `widgets` (scroll simulations) and `animation` (spring curves). It is a leaf input, not a gate.

### 2.2 FLUI's actual crate DAG (verified via `cargo metadata`)

The real edges among active + disabled crates. `*` marks a disabled crate (source exists, commented out of `[workspace.members]`).

```
flui-types        (no flui deps)              flui-log   (no flui deps)
flui-foundation ──► flui-types
flui-tree       ──► flui-foundation
flui-platform   ──► flui-types
flui-scheduler  ──► flui-foundation
flui-painting   ──► flui-foundation, flui-types
flui-interaction──► flui-foundation, flui-types
flui-semantics  ──► flui-foundation, flui-tree, flui-types
flui-layer      ──► flui-foundation, flui-painting, flui-tree, flui-types
flui-engine     ──► flui-foundation, flui-layer, flui-painting, flui-types,
                    flui-assets*, flui-devtools*        ◄── edges into DISABLED crates
flui-rendering  ──► flui-foundation, flui-interaction, flui-layer,
                    flui-painting, flui-semantics, flui-tree, flui-types
flui-view       ──► flui-foundation, flui-interaction, flui-log,
                    flui-rendering, flui-tree, flui-types
flui-hot-reload ──► flui-layer, flui-rendering, flui-types, flui-view
flui-app        ──► flui-engine, flui-foundation, flui-hot-reload,
                    flui-interaction, flui-layer, flui-log, flui-painting,
                    flui-platform, flui-rendering, flui-scheduler,
                    flui-semantics, flui-types, flui-view

DISABLED:
flui-animation* ──► flui-foundation, flui-scheduler, flui-types
flui-reactivity*──► (NO flui deps — pure standalone signal library)
flui-assets*    ──► flui-types
flui-build*     ──► (NO flui deps — async build pipeline, std crates only)
flui-cli*       ──► flui-build*, flui-log, flui-devtools* (optional)
flui-devtools*  ──► flui-engine
```

As a layered diagram (the construction layers, top = leaf, bottom = app):

```
L0  flui-types          flui-log          flui-build*        flui-reactivity*
L1  flui-foundation
L2  flui-tree   flui-platform   flui-scheduler   flui-painting   flui-interaction
                                flui-assets*
L3  flui-semantics   flui-layer   flui-animation*
L4  flui-engine   flui-rendering
L5  flui-view   flui-devtools*
L6  flui-hot-reload   flui-cli*
L7  flui-app
```

### 2.3 Where FLUI's DAG diverges from Flutter's package order

| Divergence | Flutter | FLUI | Why it matters for ordering |
|---|---|---|---|
| **Foundation split** | one `foundation` package | `flui-types` (36k LOC!) + `flui-foundation` (4.8k) | `flui-types` is **over-built** — it is 36k LOC, 3× the entire Flutter `foundation`. It carries geometry, painting value-types, styling, platform enums. This is fine for ordering (it is a stable leaf) but it is the workspace's biggest single crate and a compile-time tax on everything. |
| **Rendering split into 3** | one `rendering` package (52k) | `flui-rendering` (render objects + pipeline) + `flui-layer` (compositing) + `flui-engine` (wgpu GPU backend) | Flutter's `rendering` package + the C++ engine's compositor are merged conceptually; FLUI separates the layer tree (`flui-layer`) and the GPU backend (`flui-engine`) as **siblings** of `flui-rendering`. `flui-engine` and `flui-rendering` do not depend on each other — they communicate via the `Scene` produced by paint. This is a *good* divergence: the GPU backend can be built/tested independently. |
| **`flui-tree` — a non-Flutter crate** | Flutter has 4 parallel tree impls (Element/RenderObject/Layer/Semantics), each bespoke | `flui-tree` is a *unified* trait surface (`TreeRead`/`TreeNav`/`TreeWrite` + arity markers + `IndexedSlot`) all four trees are meant to build on | This is the central Rust-native bet (per memory `flui-tree-unified-interface-intent`). It sits at L2 and gates `flui-semantics`, `flui-layer`, `flui-rendering`, `flui-view`. **Cycle 3 found it ~58% zombie** — migration gap, not deletion signal. Ordering consequence: `flui-tree` must be load-bearing-solid before the widget catalog leans on it. |
| **`flui-view` ≠ Flutter `widgets`** | `widgets` = framework spine (Widget/Element/BuildContext/BuildOwner) **+** the ~80-widget catalog (`Padding`, `Row`, `Text`, `ListView`, `Navigator`, ...) | `flui-view` is **only the spine**. The catalog has no home — `flui-widgets` does not exist. | This is the single most important structural gap. Flutter's `widgets/framework.dart` (~7k LOC) is the spine; the other ~150k LOC is the catalog. FLUI has the spine (`flui-view`, 14k LOC) and **none of the catalog**. |
| **`flui-engine` → disabled crates** | n/a | `flui-engine` declares deps on `flui-assets*` and `flui-devtools*`, both disabled | `cargo metadata` shows these edges, but the crates are commented out of `[workspace.members]`. The edges are feature-gated / optional. Ordering consequence: `flui-engine`'s asset-loading and devtools-overlay paths are **latently broken** until those crates re-enter — but `flui-engine` builds today because the deps are optional. |
| **`flui-reactivity` is Flutter-foreign** | Flutter has no signals; state flows via `setState` + `InheritedWidget` | `flui-reactivity` is a standalone React-Hooks/signal library (`Signal`/`Memo`/`Effect`), **zero `flui-` deps** | Per `STRATEGY.md` "Not working on: реинвент Flutter widget tree mental model" and the Flutter-primacy rule, `flui-reactivity` is **off the parity critical path**. It is an optional ergonomics layer. Its placement (L0, no deps) means it can be re-enabled any time without blocking anything — but nothing in the parity port *requires* it. |
| **`flui-physics` missing** | `physics` package (893 LOC) | no crate | Small. Feeds scroll simulations and spring animations. Belongs at L2 (deps: `flui-types` only). |
| **`flui-services` has no single home** | `services` package (30k LOC) — platform channels, text input, asset bundle, haptics, clipboard | spread across `flui-platform` (window/input/clipboard) + `flui-assets*` (asset bundle) | FLUI deliberately dissolved Flutter's `services` into `flui-platform` + `flui-assets` (per `docs/PORT.md` "binding-deletion carve-out" — `PlatformTextSystem` was deleted because cosmic-text + glyphon + flui-assets own text end-to-end). **Verdict: do NOT create `flui-services`.** The carve-out is a deliberate, documented divergence. Remaining `services` responsibilities (text input / IME, system chrome, haptics) attach to `flui-platform`. This is addressed as a *capability gap in `flui-platform`*, not a missing crate.

---

## 3. Critical-path analysis

### 3.1 The longest dependency chain

The chain that gates full parity, longest first:

```
flui-types / flui-foundation         [SOLID — Cycle 3 hardened]
        │
        ▼
flui-tree                            [FRAGILE — Cycle 3: ~58% zombie, TreeWrite cascade fixed PR#103]
        │
        ▼
flui-painting + flui-interaction + flui-semantics + flui-scheduler   [mixed — see §5]
        │
        ▼
flui-layer                           [FRAGILE — Cycle 2: no layer lifecycle protocol]
        │
        ▼
flui-rendering                       [FRAGILE — Cycle 4: 3× unimplemented!(), run_compositing stub]
        │
        ▼
flui-rendering::objects/  (the render-object CATALOG — ~80 objects, 7 exist)   ◄── THE BOTTLENECK
        │
        ▼
flui-view  (framework spine — depends on rendering)   [FRAGILE — Cycle 5: keyless reconciliation, dead O(1) registry]
        │
        ▼
flui-animation  (re-enabled)         [disabled — depends on scheduler]
        │
        ▼
flui-widgets  (NEW — the user-facing widget catalog, ~80 widgets)   ◄── THE LARGEST SINGLE BUILD
        │
        ├──────────────────────┐
        ▼                      ▼
flui-material  (NEW, 210k)   flui-cupertino  (NEW, 48k)
```

### 3.2 The critical path in one line

**`flui-tree` (repair) → `flui-rendering` (close `unimplemented!()` + render-object catalog) → `flui-view` (keyed reconciliation) → `flui-animation` (re-enable) → `flui-widgets` (NEW, the ~80-widget catalog) → `flui-material` (NEW, 210k LOC).**

Everything else — `flui-engine` GPU work, `flui-layer` lifecycle, `flui-interaction` recognizers, `flui-cupertino`, `flui-devtools` — either feeds this chain as a parallel tributary or hangs off its end. **`flui-material` is the terminal node and the single largest body of work in the entire port** (210k LOC of Dart, ~2× the rest of the catalog work combined).

### 3.3 The two true bottlenecks

The critical path has two nodes that are *bottlenecks* in the build-systems sense — work that cannot be parallelized away and gates a disproportionate amount downstream:

1. **The render-object catalog inside `flui-rendering`.** Every widget in `flui-widgets` is, in Flutter terms, a thin configuration object over a `RenderObject`. `Text` needs `RenderParagraph`; `ListView` needs `RenderViewport` + `RenderSliverList`; `Stack` needs `RenderStack`; `Opacity` needs `RenderOpacity` (exists). FLUI has 7 of ~80. **You cannot build the widget catalog faster than you build the render objects under it.** This is the hidden bottleneck — it looks like "finish `flui-rendering`" but it is really "build 73 more render objects," and that is sequenced *before* the widgets that wrap them.

2. **`flui-widgets` itself.** ~80 user-facing widgets. Even with render objects done, the widget catalog is the largest single new crate. It gates *both* `flui-material` and `flui-cupertino`.

### 3.4 What is NOT on the critical path (parallelizable)

- **`flui-engine`** — GPU backend hardening (Cycle 4 found ~2,800 LOC dead, `clip_path` no-op, backdrop-filter stub). The engine consumes `Scene`; as long as the `Scene`/`DrawCommand` contract is stable, engine work runs fully parallel to the rendering/view/widget chain. It is a *sibling*, not an ancestor, of `flui-rendering`.
- **`flui-interaction`** — recognizer FSM consolidation (Cycle 1). Gestures feed `flui-rendering` hit-testing, but the hit-test *contract* is stable; recognizer internals can harden in parallel.
- **`flui-cupertino`** — depends on `flui-widgets`, not on `flui-material`. Once `flui-widgets` is real, Cupertino and Material build in parallel.
- **`flui-devtools`, `flui-cli`, `flui-hot-reload`** — the DX track. Per `STRATEGY.md` this is a first-class track, but it does not gate parity construction.
- **`flui-physics`** — tiny leaf; can be built any time after `flui-types`.

---

## 4. Disabled-crate re-entry analysis

Six crates have source but are commented out of `[workspace.members]`. For each: why it was disabled (inferred), preconditions to re-enter, what it unblocks.

| Crate | LOC | Disabled because (inferred) | Preconditions to re-enter `[workspace.members]` | Unblocks |
|---|---:|---|---|---|
| **flui-animation** | 7,475 | Depends on `flui-scheduler`'s `Ticker`; `flui-scheduler` was mid-hardening (Cycle 1). The repair plan `2026-05-21-003-feat-input-frame-loop-repair-plan.md` explicitly does "source-only migration of disabled flui-animation" — so it is being kept in sync *as a disabled crate* while `Ticker`/`TickerProvider` stabilize. | (a) `flui-scheduler` `Ticker` + `TickerProvider::create_ticker` API frozen (Cycle 1 repair landed, PRs #85-#98). (b) `Animation<T>` trait verified against `flui-foundation::Listenable` (Cycle 3 confirms `flui-animation` is the canonical `Listenable` consumer). (c) `flui-view`'s `AnimationBehavior` un-feature-gated and given a real consumer (Cycle 5 V-9 found it test-only). | The `AnimatedView`/`AnimationController` path; **every animated widget** in `flui-widgets` (implicit animations, `AnimatedContainer`, transitions); `flui-physics` spring curves consumers. **On the critical path.** |
| **flui-reactivity** | 8,078 | Standalone signal library, **zero `flui-` deps**. Disabled likely because it is an *optional ergonomics layer* not required for Flutter parity, and per `STRATEGY.md` "not working on: реинвент Flutter widget tree mental model" it is deliberately deferred to avoid signalling a non-Flutter direction. | (a) A decision that signals are an *additive* opt-in, not a replacement for `setState`/`InheritedWidget`. (b) An integration seam in `flui-view` (a `Signal`-aware element or hook bridge) — currently none exists. | Optional reactive ergonomics. **NOT on the parity critical path** — it can stay disabled through full parity and re-enter as a post-parity ergonomics feature. |
| **flui-assets** | 4,607 | Async IO crate (`tokio fs`, `reqwest`, `moka`, `image`). `flui-engine` declares an *optional* edge to it. Disabled because the asset-loading integration into `flui-engine` (texture upload from decoded images) and `flui-painting` (font bytes) is not wired. | (a) `flui-engine` texture-pool API stable enough to accept decoded `image` buffers. (b) Font-loading path decided — cosmic-text owns shaping (per the `PlatformTextSystem` carve-out), so `flui-assets` supplies font *bytes* only. (c) An `AssetBundle` abstraction agreed (Flutter `services/asset_bundle.dart` parity). | `Image` widget (network + asset), font loading, any `flui-widgets` widget that displays an image or custom font. Needed **before the widget catalog ships an `Image` widget** — so it joins the critical path at the `flui-widgets` phase. |
| **flui-build** | 4,003 | Async cross-platform build pipeline (Android/iOS/Desktop/Web builders). **Zero `flui-` deps** — pure tooling. Disabled because it is DX-track tooling, not framework code, and the platform builders need `flui-platform` mobile backends to actually target. | (a) `flui-platform` has working Android + iOS backends (mobile-native is a `STRATEGY.md` track). (b) `flui-cli` re-enabled (its consumer). | `flui-cli` build commands; mobile/web packaging. **DX track — does not gate parity.** |
| **flui-cli** | 7,338 | Depends on `flui-build*` (disabled) and optionally `flui-devtools*` (disabled). Cannot compile while its deps are out. Has an active spec (`specs/001-cli-completion`). | (a) `flui-build` re-enabled. (b) `flui-devtools` re-enabled (optional dep — or drop the optional feature). | End-user `flui new`/`flui build`/`flui run` CLI. **DX track — does not gate parity.** |
| **flui-devtools** | 2,563 | Depends on `flui-engine`. Disabled because the devtools overlay (frame timeline, inspector) needs a stable engine render path + a stable element tree to inspect. `flui-engine` itself declares an optional edge back to `flui-devtools` (the in-engine overlay) — a near-cyclic feature relationship managed by Cargo features. | (a) `flui-engine` render path stable (Cycle 4 closes the `unimplemented!()`s). (b) `flui-view` element tree introspection API (the inspector needs to walk the element tree). (c) Resolve the `flui-engine ⇄ flui-devtools` optional-feature relationship cleanly. | Widget inspector, frame profiler, the DX track's flagship. **DX track — does not gate parity, but `STRATEGY.md` wants it day-1-class.** |

**Re-entry ordering:** `flui-animation` first (critical path). Then `flui-assets` (joins at `flui-widgets`). Then the DX cluster `flui-devtools` → `flui-build` → `flui-cli` as a parallel track. `flui-reactivity` last / optional.

---

## 5. Missing-crate placement analysis

Five crates that do not exist and must be created for full parity. Exact DAG placement, deps, dependents, sizing.

| New crate | DAG layer | Depends on | Depended on by | Flutter source | Rough size | Notes |
|---|---|---|---|---|---:|---|
| **flui-physics** | L2 (sibling of `flui-painting`) | `flui-types` only | `flui-animation` (spring curves), `flui-widgets` (scroll simulations, `BouncingScrollPhysics`) | `physics/` — 893 LOC, 7 files | **~1.5k LOC** | Smallest new crate. Pure math: `Simulation`, `SpringSimulation`, `FrictionSimulation`, `GravitySimulation`, `Tolerance`. No GPU, no platform. Build it early — it is a trivial leaf and unblocks `flui-animation`'s spring path. |
| **flui-widgets** | **L6** (new layer between `flui-view` and `flui-app`) | `flui-view`, `flui-rendering`, `flui-animation`, `flui-painting`, `flui-interaction`, `flui-types`, `flui-foundation`, `flui-physics`, `flui-assets` | `flui-material`, `flui-cupertino`, `flui-app`, end users | `widgets/` catalog — ~140k of the 157k LOC (the spine ~7k is already `flui-view`) | **~50-80k LOC** | The user-facing widget catalog: `Padding`, `Center`, `Row`/`Column`/`Stack`, `Container`, `Text`, `Image`, `Icon`, `ListView`/`GridView`/slivers, `Scaffold`-precursors, `Navigator`/routing, `GestureDetector`, `Scrollable`, `FocusScope` widgets, implicit-animation widgets, `Table`, `Wrap`, `Flow`. **The single largest new build.** Each widget is a `View` impl over a render object. Gates both design-system crates. |
| **flui-material** | L7 (sibling of `flui-cupertino`, parallel) | `flui-widgets` + all its deps | `flui-app`, end users | `material/` — 210,800 LOC, 198 files | **~80-120k LOC** | Material Design 3 component library: `Scaffold`, `AppBar`, `Button` family, `TextField`, `Card`, `Dialog`, `Drawer`, `BottomNavigationBar`, `TabBar`, `DataTable`, `Chip`, theming (`ThemeData`, `ColorScheme`), `InkWell`/ripple. **The terminal node of the critical path and the largest crate in the workspace.** Note: `flui-app/src/theme/colors.rs` already has a *parallel* `Color`/`ColorScheme` (Cycle 5 V-25) — that must be deleted in favor of `flui-types::Color` before `flui-material` defines theming. |
| **flui-cupertino** | L7 (sibling of `flui-material`, parallel) | `flui-widgets` + all its deps | `flui-app`, end users | `cupertino/` — 48,253 LOC, 52 files | **~25-40k LOC** | iOS-style component library: `CupertinoApp`, `CupertinoNavigationBar`, `CupertinoButton`, `CupertinoPicker`, `CupertinoTabScaffold`, `CupertinoPageRoute` (the iOS swipe-back transition). Independent of `flui-material` — builds in parallel once `flui-widgets` exists. |
| **flui-services-equiv** — **DO NOT CREATE** | — | — | — | `services/` — 30,226 LOC | **0** (absorbed) | Per `docs/PORT.md` binding-deletion carve-out, Flutter's `services` is deliberately dissolved: window/input/clipboard → `flui-platform`; asset bundle → `flui-assets`; text shaping → cosmic-text/glyphon (the `PlatformTextSystem` deletion precedent). The remaining `services` responsibilities — **text input / IME, system chrome, haptic feedback, platform method channels** — should be added as **capabilities of `flui-platform`** (new traits: `PlatformTextInput`, `PlatformSystemChrome`, `PlatformHaptics`), not a new crate. Creating `flui-services` would re-introduce a Flutter abstraction the project chose to delete. |

**Resulting layer table after all crates exist** (new crates in **bold**):

```
L0  flui-types   flui-log   flui-build*   flui-reactivity*
L1  flui-foundation
L2  flui-tree  flui-platform  flui-scheduler  flui-painting  flui-interaction
    flui-assets   **flui-physics**
L3  flui-semantics   flui-layer   flui-animation
L4  flui-engine   flui-rendering
L5  flui-view   flui-devtools
L6  flui-hot-reload   flui-cli   **flui-widgets**
L7  **flui-material**   **flui-cupertino**
L8  flui-app
```

`flui-widgets` slots at L6 alongside `flui-hot-reload`. `flui-material`/`flui-cupertino` form a new L7. `flui-app` moves to L8 as the true top.

---

## 6. Solid vs fragile foundation

The central roadmap risk: **the widget catalog will be built on `flui-view` and `flui-rendering`. If those are fragile, every widget inherits the fragility, and the cost compounds across ~80 widgets.** Each active crate classified below, with the backing audit.

| Crate | Verdict | Backing audit | Rationale |
|---|---|---|---|
| **flui-types** | **SOLID** | (no dedicated audit; stable leaf) | 36k LOC, over-built but a stable value-type leaf. Risk is compile-time tax, not correctness. Safe to build on. |
| **flui-foundation** | **SOLID** | `2026-05-22-flui-foundation-tree-audit.md` (Cycle 3, closed PRs #102-#106) | Audit's "three best things" are all here: `ChangeNotifier::dispose`, `Id<T: Marker>` generic ID system, `BindingBase` singleton macro. Cycle 3 closed ~22 findings. Templated `ARCHITECTURE.md`. **The most solid crate in the workspace.** |
| **flui-tree** | **KNOWN-FRAGILE** | `2026-05-22-flui-foundation-tree-audit.md` (Cycle 3) | Audit verdict: **~58% zombie surface** (~11,400 LOC zero-consumer). `TreeWrite::remove` cascade footgun *was* fixed (PR #103). But the unified-tree intent is half-migrated: `LayerTree`/`SemanticsTree` still carry parallel mutation APIs; `flui-view`'s `ElementTree` does not implement `TreeRead`/`TreeWrite` at all (Cycle 5 V-7). Quadruple depth-constant drift. **Fragile not because it is wrong but because it is unfinished — the widget catalog will lean on `arity` markers and `IndexedSlot`, which ARE solid; the visitor/diff/cursor surface is the zombie part and can be feature-gated away.** |
| **flui-platform** | **PARTIAL / IN-FLIGHT** | spec `002-platform-mvp`; no Mythos cycle yet | 18,970 LOC. Active MVP development (Windows/macOS/Headless backends; Winit fallback in progress). Mobile (Android/iOS) and text-input/IME not done. Not audited by Mythos. **Treat as in-flight, not load-bearing for the widget catalog** (widgets need `flui-view`, not `flui-platform` directly — only `flui-app` does). |
| **flui-painting** | **MOSTLY SOLID** | `2026-05-22-flui-painting-view-audit.md` (Cycle 5) | `Canvas`/`DisplayList`/`DrawCommand` are confirmed parity-clean and consumed correctly by engine. `forbid(unsafe_code)`. **But ~31% of the crate is zero-consumer**: `tessellation` (dup of engine's), `TextPainter` (test-only), fallback `TextLayout`, `canvas/sugar`. These are dead weight, not a correctness risk — feature-gate or delete (Cycle 5 P-1..P-9). The *load-bearing* part is solid. |
| **flui-semantics** | **SOLID-ish / repairable** | `2026-05-22-flui-layer-semantics-audit.md` (Cycle 2, repair plan `2026-05-22-004`) | `SemanticsHandle` ref-counting is the audit's only "fully Flutter-faithful FSM." `SemanticsService::send_event` is a stub (the one platform bridge). `SemanticsTree` does not implement `flui-tree` traits (asymmetry with `LayerTree`). Repair plan `2026-05-22-004` is ready/queued. **Not on the critical path for the widget catalog** — semantics is additive; widgets can ship before semantics is perfect. |
| **flui-scheduler** | **SOLID** (post-Cycle-1) | `2026-05-21-flui-interaction-scheduler-audit.md` (Cycle 1, closed PRs #85-#98) | Cycle 1 hardened `Ticker` (adopted the `ChangeNotifier::dispose` template), realigned `Priority` to Flutter's numeric values. Frame-loop is sound. Safe to build `flui-animation` on. |
| **flui-layer** | **KNOWN-FRAGILE** | `2026-05-22-flui-layer-semantics-audit.md` (Cycle 2) | Audit verdict: **"fundamentally absent Layer lifecycle protocol"** — no `Drop`, no ref-counted `LayerHandle`, no `needs_add_to_scene` dirty-bit, no `engine_layer` retention. Consequence: **every frame rebuilds the GPU layer from scratch** — retained rendering is lost. The 19-variant `Layer` enum is 360+ bytes on every node. Repair plan `2026-05-22-004` (Waves 1-4) addresses lifecycle phases 1+2 + enum boxing. **Must be repaired before it is load-bearing for a real app** (performance), but does not block widget *correctness*. |
| **flui-engine** | **KNOWN-FRAGILE** | `2026-05-22-flui-rendering-engine-audit.md` (Cycle 4, in flight) | ~2,800 LOC verified-dead; `WgpuPainter::clip_path` is a silent no-op; `Backend::render_backdrop_filter` is a fallback stub; `PipelineManager`/`PipelineHandle` are body-less zombies. Cycle 4 is *in flight* — not yet closed. **Parallelizable** (sibling of `flui-rendering`), but genuinely fragile until Cycle 4 lands. |
| **flui-rendering** | **KNOWN-FRAGILE (critical)** | `2026-05-22-flui-rendering-engine-audit.md` (Cycle 4, in flight) | **The most consequential fragility.** Three `unimplemented!()` in production paths (`run_semantics`, `perform_semantics_action`, `SemanticsBuilder::new`). `run_compositing` is a trace-and-clear stub returning `Ok(())` while doing nothing. `propagate_constraints_to_child` + `sync_child_size_to_parent` are **empty-body methods called every frame** — constraint propagation and child-size sync are not happening. `paint_node_recursive` does not depth-sort the dirty list — nodes not reachable from root get their dirty flag cleared without painting. Four parallel public types (`HitTestResult`, `MouseTrackerAnnotation`, `RenderError`, `ParentData`) ×2. **The good news:** typestate pipeline, `AtomicRenderFlags`, `PipelineOwnerHandle` are gold-standard. The render *machine* is sound; specific *phases* are stubbed. **The widget catalog cannot be built on a render layer whose layout phase has empty-body constraint propagation.** |
| **flui-view** | **KNOWN-FRAGILE (critical)** | `2026-05-22-flui-painting-view-audit.md` (Cycle 5); prior `view-tree-foundation-audit` (closed PR #84 — framework spine repair) | The framework spine was repaired in PR #84 (`BuildContext` wired, `GlobalKey` registry, `ElementOwner` split-borrow — all now SOLID). **But Cycle 5 found the catalog-critical gap:** `VariableChildStorage::update_with_views` matches children **by index, not by key** — the 325-LOC keyed `reconcile_children` exists but **nothing calls it**. Keyless reconciliation breaks `GlobalKey` reparenting, `Hero`, `Reorderable`, and `ListView` state preservation. The `BuildOwner::inherited_elements` O(1) registry is built, costed, and **never populated by production** — `depend_on_inherited` walks ancestors O(depth). **These two must be fixed before the widget catalog** — every list/animated/themed widget depends on keyed reconciliation and fast inherited lookup. |
| **flui-hot-reload** | **PARTIAL** | (no Mythos cycle) | 1,066 LOC, small. dlopen-based scene plugin reload. Functional for examples. DX-track; not load-bearing. |
| **flui-app** | **PARTIAL / IN-FLIGHT** | (no Mythos cycle; "Migration" status in `Cargo.toml`) | The top-level binding integration. Carries a parallel `Color`/`ColorScheme` (Cycle 5 V-25) that must be deleted. Inherits every fragility below it. Hardens last. |
| **flui-log** | **SOLID** | (trivial leaf) | 983 LOC, tracing wrapper. No risk. |

### 6.1 The central-risk summary

**Building the widget catalog on today's `flui-view` + `flui-rendering` would fail.** Three concrete blockers:
1. `flui-rendering` layout has **empty-body constraint propagation** — children would not receive constraints correctly.
2. `flui-view` reconciliation is **keyless** — `ListView`, `Hero`, any keyed widget would lose state on rebuild.
3. `flui-rendering` has **7 of ~80 render objects** — there is nothing for most widgets to wrap.

The roadmap MUST sequence the `flui-rendering` phase-closure + render-object catalog + `flui-view` keyed reconciliation **before** `flui-widgets`. This is non-negotiable dependency correctness, and it is exactly what Phases 1-3 below do.

---

## 7. The Phase 0..7 breakdown (core deliverable)

Phases are **dependency-ordered, not time-boxed**. Each phase has: a goal, the crates/subsystems it builds or repairs, entry preconditions, and an **objective + testable exit criterion**. Parallelism is marked explicitly.

---

### Phase 0 — Foundation hardening closure

**Goal.** Bring the bottom of the DAG to provably SOLID so nothing above inherits a foundation defect. This is mostly *finishing the in-flight Mythos cycles*, not new construction.

**Builds / repairs.**
- `flui-rendering` + `flui-engine`: close **Cycle 4** (in flight). Specifically: remove the three `unimplemented!()`, replace with `RenderError::SemanticsNotEnabled` / feature-gate; **implement `run_compositing` and the empty-body `propagate_constraints_to_child` / `sync_child_size_to_parent`**; fix `paint_node_recursive` depth-sort; collapse the four parallel public types to one canonical home each; delete ~2,800 LOC engine dead code.
- `flui-painting` + `flui-view`: close **Cycle 5**. Specifically: **wire `BuildOwner::inherited_elements` registry** (V-3) so `depend_on_inherited` is O(1); **hoist `reconcile_children` into `VariableChildStorage`** (V-4 — keyed reconciliation); feature-gate/delete painting + view zombies.
- `flui-layer` + `flui-semantics`: land repair plan `2026-05-22-004` (layer lifecycle phases 1+2, enum boxing, `send_event` wiring).
- `flui-tree`: resolve the unified-tree migration gap — either feature-gate the zombie 58% behind `unstable-devtools`, or migrate `LayerTree`/`SemanticsTree`/`ElementTree` onto `TreeWrite`. Unify the four depth constants.

**Entry preconditions.** None — this is the current state. Mythos Cycles 1-3 already closed; 4 in flight, 5 audited.

**Exit criterion (objective + testable).**
- `cargo build --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` green.
- `bash scripts/port-check.sh -v` green (all 7 refusal triggers).
- **Zero `unimplemented!()` / `todo!()` in non-test code across the active workspace** (grep gate).
- A render-tree integration test proves a 3-level nested layout (`Padding` → `Center` → `ColoredBox`) lays out with correct constraints and sizes — i.e. `propagate_constraints_to_child` actually works.
- A `flui-view` test proves keyed reconciliation: a `Variable`-arity element with children `[A(key=1), B(key=2)]` reordered to `[B(key=2), A(key=1)]` **preserves element identity** (no remount).

---

### Phase 1 — Render-object catalog

**Goal.** Build the ~73 missing render objects in `flui-rendering/src/objects/`. This is the **first true bottleneck** — every widget wraps a render object.

**Builds / repairs.**
- `flui-rendering::objects/` — port the Flutter `rendering/` render-object set: `RenderParagraph` (text), `RenderImage`, `RenderStack`/`RenderPositioned`, `RenderConstrainedBox`/`RenderLimitedBox`, `RenderAspectRatio`, `RenderBaseline`, `RenderClip*` (rect/rrect/path/oval), `RenderDecoratedBox`, `RenderTransform` family, `RenderFractionallySizedBox`, `RenderWrap`, `RenderFlow`, `RenderTable`, the **sliver family** (`RenderViewport`, `RenderSliverList`, `RenderSliverGrid`, `RenderSliverPadding`, `RenderSliverFillViewport`, `RenderSliverToBoxAdapter`), `RenderListBody`, `RenderCustomPaint`, `RenderMouseRegion`, `RenderPointerListener`, `RenderRepaintBoundary`, `RenderOpacity` variants.
- Group into arity-correct families using the existing `Leaf`/`Single`/`Optional`/`Variable` markers (these are SOLID per Cycle 3).
- This phase also *exercises* the Phase-0 layout fixes at scale — first real stress on `propagate_constraints_to_child`.

**Entry preconditions.** Phase 0 exit met — specifically the layout-phase constraint propagation must be real, and the typestate pipeline solid.

**Exit criterion.**
- The render-object set covers every render object that the Phase-3 widget catalog will wrap (concretely: a checklist mapping each planned `flui-widgets` widget to its render object, all present).
- Per-render-object layout + paint unit tests; intrinsic-size tests where applicable.
- A sliver integration test: `RenderViewport` + `RenderSliverList` scrolls a 1,000-item list with correct lazy layout.
- `flui-rendering` coverage ≥ 80% (Constitution requirement for Core).

**Parallelism.** Render objects are largely independent of each other — within this phase, the catalog can be split into parallel families (box-layout objects, sliver objects, paint-effect objects, input objects). Text (`RenderParagraph`) depends on the `flui-painting` text path being settled.

---

### Phase 2 — Animation re-entry + physics

**Goal.** Re-enable `flui-animation` and create `flui-physics` so the widget catalog can have animated and physics-driven widgets.

**Builds / repairs.**
- **Create `flui-physics`** (L2, deps: `flui-types` only) — `Simulation`, `SpringSimulation`, `FrictionSimulation`, `GravitySimulation`, `Tolerance`. ~1.5k LOC. Trivial leaf.
- **Re-enable `flui-animation`** — add back to `[workspace.members]`. Verify `Ticker`/`TickerProvider` integration (Cycle 1 froze that API). Wire `flui-animation` to `flui-physics` for spring curves.
- Un-feature-gate `flui-view`'s `AnimationBehavior` / `AnimatedView` (Cycle 5 V-9 had feature-gated it as test-only) and give it a real consumer path.

**Entry preconditions.** `flui-scheduler` SOLID (done, Cycle 1). `flui-view` framework spine SOLID (done, PR #84) + Phase-0 keyed reconciliation. `flui-types` SOLID.

**Exit criterion.**
- `flui-animation` + `flui-physics` in `[workspace.members]`, `cargo build --workspace` green.
- An `AnimationController` driven by a real `Ticker` produces a 0.0→1.0 ramp over a frame sequence (integration test).
- A `SpringSimulation` settles to rest within tolerance (physics test).
- `flui-view`'s `AnimationBehavior` has a non-test consumer.

**Parallelism.** Fully parallel with Phase 1 — animation/physics have no dependency on the render-object catalog. **Phases 1 and 2 run concurrently.**

---

### Phase 3 — `flui-widgets`: the user-facing catalog

**Goal.** Create the `flui-widgets` crate — the ~80-widget catalog. **The second bottleneck and the largest single new build.**

**Builds / repairs.**
- **Create `flui-widgets`** at L6 (deps: `flui-view`, `flui-rendering`, `flui-animation`, `flui-painting`, `flui-interaction`, `flui-physics`, `flui-assets`, `flui-types`, `flui-foundation`).
- Port the Flutter `widgets/` catalog (the ~140k LOC that is *not* the framework spine): layout widgets (`Padding`, `Center`, `Align`, `Row`/`Column`/`Stack`/`Wrap`/`Flow`, `Container`, `SizedBox`, `Expanded`/`Flexible`, `AspectRatio`, `Table`), `Text`/`RichText`/`DefaultTextStyle`, `Image`/`Icon`, scrolling (`Scrollable`, `ListView`/`GridView`/`CustomScrollView`/`SliverList`, scroll physics integration), input (`GestureDetector`, `Listener`, `MouseRegion`, `FocusScope`/`Focus`), `Navigator`/routing/`PageRoute`, implicit-animation widgets (`AnimatedContainer`, `AnimatedOpacity`, `AnimatedAlign`, transitions), `Hero`, `Visibility`, `Opacity`/`ClipRRect`/`DecoratedBox` widget wrappers.
- **Re-enable `flui-assets`** as part of this phase (needed for the `Image` widget — network + asset image, font loading). Decide the `AssetBundle` abstraction.

**Entry preconditions.** Phase 1 exit (render-object catalog complete) **and** Phase 2 exit (animation/physics available). This phase **cannot start** until both upstream phases are done — it is the convergence point.

**Exit criterion.**
- `flui-widgets` in `[workspace.members]`, builds clean.
- A sample app builds entirely from `flui-widgets` widgets (no raw render objects) and renders: a scrolling list, a gesture-responsive button, an implicitly-animated container, a navigated route.
- `flui-widgets` coverage ≥ 85% (Constitution requirement for Widget layer).
- `Hero` + `GlobalKey` reparenting works end-to-end (proves Phase-0 keyed reconciliation under real load).

**Parallelism.** Within the phase, widget families are independent and can be split (layout family, scrolling family, input family, animation family, routing family). The crate itself is one unit but the work inside parallelizes heavily.

---

### Phase 4 — `flui-material` and `flui-cupertino` (parallel)

**Goal.** The two design-system component libraries. These run **fully in parallel** with each other.

**Builds / repairs.**
- **Create `flui-material`** (L7) — Material Design 3: `Scaffold`, `AppBar`, button family (`ElevatedButton`/`TextButton`/`FilledButton`/`IconButton`/`FloatingActionButton`), `TextField`/`TextFormField`, `Card`, `Dialog`/`AlertDialog`/`BottomSheet`, `Drawer`/`NavigationDrawer`, `BottomNavigationBar`/`NavigationBar`, `TabBar`/`TabBarView`, `DataTable`, `Chip` family, `ListTile`, `Switch`/`Checkbox`/`Radio`/`Slider`, theming (`ThemeData`, `ColorScheme`, `Typography`), `InkWell`/`InkResponse` ripple effects, `Material` surface. **~80-120k LOC — the largest crate.**
  - Prerequisite cleanup: delete `flui-app/src/theme/colors.rs`'s parallel `Color`/`ColorScheme` (Cycle 5 V-25) before defining Material theming on `flui-types::Color`.
- **Create `flui-cupertino`** (L7) — iOS components: `CupertinoApp`, `CupertinoPageScaffold`/`CupertinoTabScaffold`, `CupertinoNavigationBar`, `CupertinoButton`, `CupertinoPicker`/`CupertinoDatePicker`, `CupertinoSwitch`/`CupertinoSlider`, `CupertinoTextField`, `CupertinoPageRoute` (the iOS swipe-back transition + parallax), `CupertinoActionSheet`/`CupertinoAlertDialog`. **~25-40k LOC.**

**Entry preconditions.** Phase 3 exit — `flui-widgets` complete and stable. Both crates depend only on `flui-widgets`, not on each other.

**Exit criterion.**
- Both crates in `[workspace.members]`, build clean.
- A Material sample app (`Scaffold` + `AppBar` + `FloatingActionButton` + `ListView` of `Card`s + a `Dialog`) renders and is interactive.
- A Cupertino sample app (`CupertinoTabScaffold` + `CupertinoNavigationBar` + `CupertinoButton` + a `CupertinoPageRoute` swipe-back) renders and is interactive.
- Theming round-trips: a `ThemeData` change repaints affected widgets (proves Phase-0 O(1) inherited lookup at scale — `Theme` is the canonical `InheritedWidget`).

**Parallelism.** `flui-material` and `flui-cupertino` are **independent siblings** — run them as two concurrent sub-tracks. `flui-material` is ~3× the size, so it is the longer of the two.

---

### Phase 5 — Application layer + `flui-app` parity hardening

**Goal.** Bring `flui-app` to full parity as the top-level binding, integrating the now-complete stack.

**Builds / repairs.**
- `flui-app` — `WidgetsBinding`/`RendererBinding` integration completion, `runApp`-equivalent, the full frame loop wired from platform vsync → build → layout → paint → composite → present.
- Move `flui-app` to L8 (above `flui-material`/`flui-cupertino`).
- `flui-platform` capability additions for the absorbed `services` responsibilities: `PlatformTextInput` (IME), `PlatformSystemChrome`, `PlatformHaptics`. (See §5 — these replace a `flui-services` crate.)
- A Mythos cycle on `flui-app` (it has had none).

**Entry preconditions.** Phase 4 exit. `flui-platform` MVP backends working (its own track — see Phase 6).

**Exit criterion.**
- A full app (`flui-material` UI) runs on at least one native platform with a real vsync-driven frame loop and on-demand rendering (`ControlFlow::Wait`).
- Text input via IME works (proves the `flui-platform` `services` carve-out).
- Constitution coverage gates met across the stack.

---

### Phase 6 — Platform breadth (parallel track, not on critical path)

**Goal.** Complete `flui-platform` backends. This is `STRATEGY.md`'s "Platform foundation" track — it runs **as a parallel track from the start**, not as a sequenced phase.

**Builds / repairs.**
- `flui-platform` — finish Windows/macOS, complete Winit fallback, **add Android + iOS native backends** (mobile-native is a `STRATEGY.md` first-class commitment), Wayland.
- `flui-engine` — backend breadth (the Cycle-4 hardening is in Phase 0; this is *additional* backend work: DX12/Metal/Vulkan/WebGPU surface management).

**Entry preconditions.** None beyond `flui-types`. Runs concurrently with Phases 0-5.

**Exit criterion.** A trivial app runs on Windows, macOS, Linux, Android, iOS, and Web. Per-platform smoke tests.

**Parallelism.** **Entirely parallel.** Platform work gates only the *final demonstration* of each phase on each OS — it does not gate the construction of the widget/material layers (those are tested headless).

---

### Phase 7 — Developer tooling (parallel DX track, not on critical path)

**Goal.** `STRATEGY.md`'s "Developer tooling (DX)" track — re-enable and complete `flui-devtools`, `flui-build`, `flui-cli`, harden `flui-hot-reload`.

**Builds / repairs.**
- Re-enable `flui-devtools` (needs stable engine render path from Phase 0 + element-tree introspection API from `flui-view`).
- Re-enable `flui-build` (Android/iOS/Desktop/Web builders) — needs `flui-platform` mobile backends (Phase 6).
- Re-enable `flui-cli` (depends on `flui-build`).
- Harden `flui-hot-reload`.

**Entry preconditions.** Phase 0 (stable engine + element tree). `flui-build` additionally needs Phase 6 mobile backends.

**Exit criterion.** `flui new` / `flui build` / `flui run` work; widget inspector + frame profiler functional; hot-reload preserves state.

**Parallelism.** Parallel track. `STRATEGY.md` wants DX "day-1-class" — so this track should *start* early (right after Phase 0) and run alongside Phases 1-5, even though it does not gate them.

**Optional / post-parity:** `flui-reactivity` re-enters here or later as additive signal ergonomics — it is never on the parity critical path.

---

## 8. Parallelism map

```
TRACK A (CRITICAL PATH — strictly sequential):
  Phase 0 ──► Phase 1 ──┐
              Phase 2 ──┴──► Phase 3 ──► Phase 4 ──► Phase 5
  (Phase 1 & Phase 2 run concurrently; Phase 3 is their join)
  (within Phase 4, flui-material ∥ flui-cupertino)

TRACK B (PLATFORM — parallel from start, joins at Phase 5):
  Phase 6 ════════════════════════════════════════════► (joins Phase 5)

TRACK C (DX TOOLING — parallel, starts after Phase 0):
        Phase 0 ──► Phase 7 ════════════════════════════►

  Timeline (dependency, not calendar):
  ──┬── Phase 0 ──┬── Phase 1 ─┬─ Phase 3 ── Phase 4 ── Phase 5
    │             └── Phase 2 ─┘
    └── Phase 6 (platform) ───────────────────────────────┘
                  └── Phase 7 (DX) ───────────────────────┘
```

Concurrency summary:
- **Phase 1 ∥ Phase 2** — render-object catalog and animation/physics are independent; both feed Phase 3.
- **Within Phase 1** — render-object families (box / sliver / paint-effect / input) parallelize.
- **Within Phase 3** — widget families (layout / scroll / input / animation / routing) parallelize.
- **Within Phase 4** — `flui-material` ∥ `flui-cupertino`.
- **Track B (platform)** and **Track C (DX)** run alongside the whole of Track A; Track B joins at Phase 5, Track C is independent.
- **Strictly sequential** — Phase 0 before everything; Phase 3 after both 1 and 2; Phase 4 after 3; Phase 5 after 4.

---

## 9. Ordering risks & mitigations

| # | Risk | Severity | Mitigation |
|---|---|---|---|
| **R1** | **Widget catalog built on fragile `flui-rendering`/`flui-view`.** If Phase 0 is skipped or rushed, ~80 widgets inherit keyless reconciliation + empty-body constraint propagation. The cost compounds — every list/animated/themed widget is silently broken, and the bug surfaces only under real use. | **CRITICAL** | Phase 0 is a hard gate with *objective* exit tests (the keyed-reconciliation test, the 3-level layout test). No Phase-1/2 work starts until they pass. The Mythos cycle already produces audits — Phase 0 is mostly *closing* known findings, not discovering new ones. |
| **R2** | **Render-object catalog under-scoped.** Phase 1 builds render objects; if the set is incomplete, Phase 3 stalls mid-widget waiting for a missing render object — a serialization failure disguised as a parallel phase. | **HIGH** | Phase 1's exit criterion is a *checklist* mapping every planned `flui-widgets` widget to its render object. Build the checklist *before* Phase 1 starts (it is a Phase-0 deliverable). The widget→render-object mapping is 1:1 and well-known from Flutter — this is enumerable. |
| **R3** | **`flui-tree` zombie surface treated as load-bearing.** The widget catalog might be designed against the visitor/diff/cursor APIs that Cycle 3 found are 58% zero-consumer — building on scaffolding that was never finished. | **MEDIUM** | Phase 0 forces a decision: feature-gate the zombie 58% behind `unstable-devtools` OR finish the migration. The widget catalog must only use the *confirmed-solid* `flui-tree` surface (`arity` markers, `IndexedSlot`, `TreeRead`/`TreeNav` core). Make that the documented contract. |
| **R4** | **`flui-engine` Cycle 4 not closed before it is needed.** `flui-engine` is parallelizable, but `flui-app` (Phase 5) needs a working GPU path. If `clip_path` is still a no-op and backdrop-filter still stubbed, real apps render incorrectly. | **MEDIUM** | Cycle 4 closure is *inside* Phase 0. The engine is a sibling so Phase 1-4 do not block on it, but Phase 0's exit gate includes the engine `unimplemented!()` sweep. Track B (platform) carries *additional* engine backend work but not the correctness fixes. |
| **R5** | **`flui-material` (210k LOC) is one monolithic terminal phase.** The largest crate is last; if it slips, the whole port's "done" slips, and there is no parallel relief because nothing comes after it. | **HIGH** | (a) `flui-material` ∥ `flui-cupertino` already splits Phase 4. (b) Within `flui-material`, sequence by component-family sub-phases (theming first — it is the `InheritedWidget` foundation everything else reads — then buttons, then inputs, then navigation, then data display). (c) Material can ship *incrementally*: a usable subset (`Scaffold` + buttons + `TextField` + `Card`) is a real milestone even before the full 198-file catalog. The roadmap should treat `flui-material` as internally phased. |
| **R6** | **`Scene`/`DrawCommand` contract drift.** `flui-engine` parallelism depends on the `Scene` contract being stable. If `flui-rendering`'s paint phase changes the `DrawCommand` enum mid-flight, the parallel engine track breaks. | **MEDIUM** | Freeze the `DrawCommand` enum + `Scene` shape as an explicit contract at end of Phase 0. Cycle 4 already confirmed `DrawCommand` parity with the engine's `CommandRenderer`. Treat any post-Phase-0 `DrawCommand` change as a coordinated cross-track change. |
| **R7** | **`flui-types` compile-time tax.** At 36k LOC it is 3× Flutter's whole `foundation` and a dependency of *everything*. As the workspace grows to ~700k LOC, a `flui-types` edit triggers a full-workspace rebuild. | **LOW-MEDIUM** | Not a correctness risk — a velocity risk. Mitigation: after Phase 3, consider splitting `flui-types` along the seams Flutter uses (geometry / painting-values / styling) so an edit to one does not rebuild all. `STRATEGY.md` already flags a post-MVP "diet" (`cargo bloat`, `cargo tree --duplicates`). Defer the split; do not block on it. |
| **R8** | **`flui-platform` mobile backends slip, blocking Phase 5 demonstration.** Track B is parallel, but Phase 5's exit ("runs on a native platform") and Phase 7's `flui-build` both need real backends. | **MEDIUM** | Track B starts at Phase 0 and runs the entire duration — it has the longest runway of any track. Phase 5's exit can be met on *desktop* first (Windows/macOS are furthest along); mobile is a follow-on demonstration, not a Phase-5 blocker. |

---

## 10. Mythos-track recommendation

**Question.** Does the in-flight Mythos hardening block forward construction, or run as a parallel track?

**Answer: Mythos is Phase 0, then it retires — it does not run forever as a parallel track.**

The reasoning, fitted to `STRATEGY.md`'s existing three-track model:

- Mythos is an **audit-then-repair cycle that hardens existing crates pair-by-pair** (`docs/research/` audits → `docs/designs/` → `docs/plans/` → atomic-commit PRs). It is not a *track* in the `STRATEGY.md` sense (a track is a permanent stream of work serving the approach). Mythos is a **bounded remediation effort** — it has a finite scope: the existing 15 active crates. Cycles 1-3 are closed; Cycle 4 is in flight; Cycle 5 is audited. After Cycle 5 + the layer-semantics repair plan, **the existing crates are hardened and Mythos has nothing left to audit** — the widget/material crates do not exist yet.

- **Mythos blocks forward construction by design, and that is correct.** You cannot build the widget catalog on a render layer with empty-body constraint propagation and keyless reconciliation. The right move is not to run Mythos forever in parallel — it is to **fold the remaining Mythos work into Phase 0 as the explicit foundation-hardening gate** and let it complete. Phase 0 *is* "finish Mythos."

- **Recommendation for the roadmap's track model.** Keep `STRATEGY.md`'s three tracks (Platform foundation, Render pipeline, Developer tooling) — they map cleanly: Render pipeline = Track A Phases 0-5, Platform foundation = Track B Phase 6, Developer tooling = Track C Phase 7. **Do not add a "Mythos track."** Instead:
  - Phase 0 = "close the remaining Mythos cycles (4, 5) + the queued repair plans." Bounded, with an objective exit gate.
  - After Phase 0, the *Mythos methodology* (audit → design → plan → atomic commits, the `docs/PORT.md` refusal triggers, `port-check.sh`) **continues to apply to new construction** — every render object, every widget, every Material component is built to the same bar and `port-check.sh` runs on every PR. But it is no longer a separate *audit-existing-crates* effort; it is the *standing quality discipline* baked into Phases 1-7.
  - In one line: **Mythos-as-remediation = Phase 0; Mythos-as-discipline = permanent, embedded in every later phase's exit gate.**

This respects `STRATEGY.md` ("the proven three tracks stay") while giving the roadmap a clean answer: the audits do not become a fourth perpetual track, and forward construction does not start until the foundation they are repairing is provably solid.

---

## 11. Summary

- **Critical path (one line):** `flui-tree`/`flui-rendering`/`flui-view` hardening (Phase 0) → render-object catalog (Phase 1) → `flui-widgets` (Phase 3) → `flui-material` (Phase 4). The render-object catalog inside `flui-rendering` and `flui-widgets` itself are the two true bottlenecks; `flui-material` (210k LOC) is the terminal node.
- **8 phases**, dependency-ordered: **P0** foundation-hardening closure (finish Mythos), **P1** render-object catalog (~73 objects), **P2** animation re-entry + new `flui-physics`, **P3** new `flui-widgets` catalog (~80 widgets), **P4** new `flui-material` ∥ `flui-cupertino`, **P5** `flui-app` parity + `flui-platform` text-input, **P6** platform breadth (parallel track), **P7** DX tooling re-enable (parallel track).
- **Parallelism:** P1 ∥ P2; within P4 material ∥ cupertino; Tracks B (platform) and C (DX) run the whole duration alongside Track A.
- **Single biggest ordering risk:** building the widget catalog on a still-fragile `flui-rendering`/`flui-view` — `flui-rendering` has empty-body constraint propagation and `flui-view` has keyless reconciliation today. Phase 0 must close these with objective exit tests before any of the ~80 widgets are built, or the fragility compounds across the entire catalog. Mitigation: Phase 0 is a hard gate; it is mostly *closing already-audited Mythos findings*, not new discovery.
- **Two structural verdicts for the roadmap:** (1) create `flui-widgets`, `flui-material`, `flui-cupertino`, `flui-physics`; (2) do **not** create a `flui-services` crate — Flutter's `services` was deliberately dissolved into `flui-platform` + `flui-assets` (the documented `PlatformTextSystem` carve-out), and its remaining responsibilities attach to `flui-platform` as new capability traits.
