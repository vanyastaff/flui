# Codebase map: values_foundation_tree — flui-geometry (L0), flui-types (L0), flui-foundation (L1), flui-macros (L1), flui-tree (L2)

_Raw output of the `map:values_foundation_tree` agent (read-only review of main @ cab06137d, 2026-09-25). Unedited; claims are the agent's and were only partly re-verified._

## Summary

The bottom of the stack is five published crates, about 61k non-test lines: geometry 19.3k, types 21.8k, foundation 11.3k, tree 6.9k, macros 0.9k. They respect the one-way layer DAG (geometry <- types; foundation and macros are leaves; tree -> foundation). Almost every crate above depends on them directly: 17 crates plus the facade depend on flui-types, and 21 on flui-foundation. How the split actually works today:

- **flui-geometry**: unit-typed values (`Point/Size/Rect/Offset<T: Unit = Pixels>`, `DevicePixels`, `Matrix4` over glam). It also carries a second, GPUI-derived vocabulary: `Bounds` (origin+size), `Vec2`, `Transform2D`, CSS-like `Length/Rems/Percentage`, `ScaleFactor<Src,Dst>`, its own Bezier types, text-path helpers and a kurbo bridge. Most of that second vocabulary has no consumer.
- **flui-types**: a Flutter `dart:ui` + `painting` + `services` grab-bag. Colors, `TextStyle`/`InlineSpan`, `Path`/`Paint`/`Shader`/`Image`, layout enums, gesture details, platform enums, `HapticFeedback`/`ImeEvent`. It also holds physics simulations and a `BoxConstraints` that duplicate flui-animation and flui-rendering, and a `MaterialColors` palette nobody uses.
- **flui-foundation**: it has the parts it should have: typed IDs, keys, callbacks, `ChangeNotifier`, the Flutter-style `Diagnosticable` tree, the tracing field vocabulary and the `observe` seam (ADR-0040). It has also become the "lowest common place" for runtime and platform protocol pieces:
  - frame and surface generation counters, `FrameStamp`, `PresentationAddress`, used by flui-layer, flui-engine and flui-app;
  - `OwnerAffinity` and `ClaimSlot`, used only by flui-platform and flui-app. These were moved there explicitly because flui-platform's test suite is not run in CI.
- **flui-tree**: provides `TreeRead`/`TreeNav`/`TreeWrite`, which LayerTree, RenderTree and SemanticsTree implement, but nothing is generic over them, and the central ElementTree does not implement them. The parts that carry weight are the compile-time `Arity` markers, `Slot`/`IndexedSlot` and `Depth`.
- **flui-macros**: small and healthy (StatelessView 135 uses, StatefulView 103, InheritedData 9, Animatable 6, Diagnosticable 1). It resolves runtime paths through proc-macro-crate, so it works with the facade and with renamed dependencies. It sits at layer 1 but emits code that targets flui-view (L5) and flui-animation (L3).

Cross-cutting findings:
- About 6-7k lines of public surface have zero production consumers.
- The vocabulary is duplicated in several places: two `Axis`, `Rect` vs `Bounds`, three transform types, and three ID schemes (`Id<M>`, `GenId<M>`, a bespoke `ElementId`).
- `Pixels` has an Eq/Hash/Ord contract violation that spreads into `Rect`, `Size` and `BoxConstraints`.
- `Color` is a fixed 8-bit sRGB struct.
- The docs and contracts are stale: AGENTS.md's ID-offset rule, docs/crates.md, and foundation's ARCHITECTURE.md all contradict the code and ADR-0074.
- The facade re-exports all three value crates wholesale, so ~2,500 public items, runtime plumbing included, would enter the H3 API freeze with no tiering.

Nothing here is structurally broken for H0. The costs land at H1 (plugin and theme-as-data vocabulary), H2 (ABA-safe layer identity, cache keys, colour depth) and above all H3 (freezing a bloated, duplicated bottom layer that every ecosystem crate will depend on).

## Responsibilities and boundaries

**What each crate should own vs what it owns today**

- **flui-geometry (L0)** should own unit-safe 2D math only. It does, plus about 3.5k lines of an unused GPUI-style second vocabulary: Length/Rems/Percentage, Transform2D, CubicBez/QuadBez, text_path, the kurbo bridge, ScaleFactor, Radians, QuarterTurns, GeometryError.
- **flui-types (L0)** should own the cross-crate value vocabulary that at least two layers share: Color, TextStyle, layout enums, HapticFeedback/ImeEvent (platform <-> interaction). Leaks:
  - physics simulations belong to (and are duplicated in) flui-animation;
  - `BoxConstraints` belongs to (and is duplicated in) flui-rendering;
  - `MaterialColors`, and arguably `MaterialType`/`Elevation`, are design-system residue in L0, against the spirit of ADR-0028;
  - gesture `*Details` belong in flui-interaction;
  - `MouseCursor` is orphaned, and the material and cupertino docs say the type does not exist.
- **flui-foundation (L1)** should own framework-wide primitives: IDs, keys, notifiers, diagnostics vocabulary, observe seam, clock, panic helpers. It also owns things that belong elsewhere:
  - window-runtime and raster-boundary protocol (epoch.rs, frame_stamp.rs, `PresentationId`/`PresentationAddress`, `DataTransferId`) — consumers are only L3+ (layer, engine, app, platform);
  - platform owner-lane primitives (affinity.rs, claim_slot.rs). These are placed here for CI reasons, not layering reasons (`crates/flui-foundation/src/affinity.rs:11-13`).
  - View-layer semantics also leak down: the `ViewKey::is_global_key` doc references the BuildOwner registry (`key.rs:364-405`), and `RebuildReason`/`AsyncSnapshot` are consumed only by flui-view, flui-app and flui-devtools.
- **flui-tree (L2)** is labelled "generic tree abstractions". In reality it is:
  - the compile-time arity markers used by rendering, objects and view;
  - `Slot`/`Depth`;
  - a method-mixin trait trio that no code consumes generically.
  It re-exports `ElementId` although the element tree does not use its traits.
- **flui-macros (L1)** owns derive expansion. Its real semantic dependencies point upward (L3/L5), which the layer checker cannot see. That is acceptable for a proc-macro, but it means the layer number carries no information here.

## Key types and contracts

- flui_geometry::{Point,Size,Rect,Offset}<T: Unit = Pixels> — Rect is min/max (crates/flui-geometry/src/rect.rs:46); Pixels(pub f32) with derived PartialEq but manual Eq/Hash(to_bits)/Ord(total_cmp) (units.rs:91-94, 575-596)
- flui_geometry::Bounds<T> origin+size (bounds.rs:64) — parallel to Rect; used by flui-platform/interaction/app/widgets
- flui_geometry::Matrix4 (glam-backed), Transform enum (transform.rs:189), Transform2D (unused)
- flui_geometry::traits::Axis (traits.rs:19) vs flui_types::layout::Axis (layout/axis.rs:13)
- flui_types::styling::Color { r,g,b,a: u8 } (color.rs:25) — 8-bit sRGB, no color space; Color32 unused; Oklab helpers
- flui_types::{typography::TextStyle, InlineSpan, painting::{Path,Paint,Shader,Image}, layout::*, gestures::*, HapticFeedback, ImeEvent}
- flui_foundation::id — Id<M: Marker> plain 1-based NonZeroUsize (ViewId dead, LayerId, SemanticsId, ListenerId, ObserverId, FrameCallbackId, FrameId, TaskId, TickerId); GenId<M> generational (RenderId, RealmId, DataTransferId); bespoke ElementId(NonZeroU64) duplicating GenId (id.rs:614-760, 796, 1163)
- flui_foundation::key — Key(NonZeroU64) incl. const FNV-1a Key::from_str; dyn ViewKey trait with is_global_key; ValueKey/UniqueKey/SaltedKey; GlobalKey/ObjectKey live in flui-view
- flui_foundation::{ChangeNotifier, ValueNotifier, Listenable: Send+Sync} over Notifier<()> = Arc<Mutex<HashMap<ListenerId,_>>> (notifier_generic.rs:41-45); ListenerRegistry<S> unused
- flui_foundation::{Diagnosticable, DiagnosticsNode, DiagnosticsBuilder} (debug.rs) + diagnostics field-name vocabulary + observe (ADR-0040) + RebuildReason
- flui_foundation::{FrameEpoch, SurfaceGeneration, GpuResourceGeneration, FrameStamp, PresentationId, PresentationAddress, RealmId, OwnerAffinity, ClaimSlot}
- flui_tree::{TreeRead, TreeNav, TreeWrite} (impl'd by LayerTree, RenderTree, SemanticsTree only); Arity markers {Leaf, Single, Optional, Variable, Exact, AtLeast, Range, Never}; Slot/IndexedSlot; Depth
- flui_macros derives: StatelessView, StatefulView, InheritedData, Animatable, Diagnosticable — runtime path resolution via proc-macro-crate with facade fallback modules 'view'/'foundation'/'animation' (runtime_path.rs:15-37)

## Dependencies

**Outgoing (external crates)**
- geometry: thiserror, glam (mint), bytemuck; optional serde and kurbo. Nothing enables the `kurbo` feature, and the `mint` feature is a no-op.
- types: flui-geometry, thiserror; optional serde. The `simd` feature gates SSE/NEON colour lerp that nothing enables.
- foundation: parking_lot, web-time, smallvec, tracing; optional serde/serde_json.
- macros: syn 3, quote, proc-macro2, proc-macro-crate.
- tree: flui-foundation, thiserror, tracing, smallvec, and bon (a proc-macro builder used for a single `Slot` builder, iter/slot.rs:84-139).

**Incoming (direct `[dependencies]`)**
- flui-types: animation, app, assets, cupertino, engine, interaction, layer, localizations, material, objects, painting, platform, rendering, semantics, testing, view, widgets and the facade.
- flui-geometry directly: only flui-widgets (31 files use `flui_geometry::` while others use `flui_types::geometry::`) and the facade.
- flui-foundation: 21 crates.
- flui-tree: layer, objects, rendering, semantics, view and the facade.
- flui-macros: animation, view and the facade.

**Facade** re-exports whole modules: `pub use flui_foundation as foundation`, `flui_geometry as geometry`, `flui_types as types` (src/lib.rs:132-150), plus `flui_tree` arity (src/rendering.rs:36).

**Hidden semantic edges** (invisible to `cargo xtask workspace`): flui-macros -> flui-view/flui-animation. Foundation's runtime-protocol types have only L3+ consumers.

## Fit with the plan

**H0 (beta)**: these crates do not block H0. Their risk is the cost of publishing: all five are `publish = ["crates-io"]`, which puts ~2,500 public items on crates.io with dead and duplicated surface. Principle 3 (no global state) is mostly honoured: the only statics are monotonic counters in key.rs:59,64 and global_key.rs:74.

**H1**
- PlatformCapability plugins need a small, stable vocabulary crate for values like HapticFeedback and ImeEvent. Today a plugin must link all 22k lines of flui-types (paths, spans, images) to get them.
- Themes-as-data and A2UI need serde-stable Color/TextStyle. Color is u8 sRGB with no colour space, so freezing its serde form now locks in 8-bit colour.
- MaterialColors in L0 contradicts the Material-as-official-package delivery layer.

**H2 (perf and scale)**
- Damage and layer caching (ADR-0061 "damage needs layer identity") want ABA-safe layer and semantics ids, but `LayerId`/`SemanticsId` are still plain reusable slab indices.
- Layout caches key on `BoxConstraints`/`Size` Hash, which is inconsistent with `==` for ±0.0.
- `ChangeNotifier` takes a Mutex per notify inside a single-writer realm.

**H3 (1.0 freeze, stability tiers)**: this is the biggest misfit. The facade exposes foundation, geometry and types wholesale, and there is no Stable/Evolving/Experimental split at the bottom. Freezing would lock in:
- two Axis enums, Rect vs Bounds, three ID schemes;
- ClaimSlot and SurfaceGeneration as "stable";
- a Flutter-port Diagnostics tree that the agent/devtools protocol does not use (principle 4 says one machine-readable protocol).

**H4 (ecosystem)**: every community crate (custom render objects, the H0 extension point) will depend on geometry/types/tree. Their shape becomes the ecosystem's lingua franca, so trimming it now is cheap and trimming it later is not.

**Delivery layers**: the bottom crates are correctly "core". The design-system residue (MaterialColors, MaterialType) should move to flui-material.

## Strengths

- Strict one-way DAG, and these crates have zero upward runtime dependencies. The layer metadata is checked by `cargo xtask workspace`.
- The unit-typed geometry has compile_fail guards against lossy conversions and unit mixing (units.rs:86-90; flui-types tests/unit_mixing_compile_fail.rs), plus const size assertions that Point/Rect/Size stay f32-packed (flui-types/src/lib.rs size_assertions).
- The generational IDs (GenId, ElementId) have niche optimisation, and compile_fail doctests prove there is no bare `get()` that would bring ABA back (id.rs:776-790, 1150-1157).
- Foundation holds no ambient singletons: BindingBase and HasInstance were deleted, and bindings are owned per realm (foundation ARCHITECTURE.md §4). The tracing subscriber backends are kept out of foundation, in flui-log.
- The diagnostics field-vocabulary rule is well reasoned: a constant exists only when something emits it, and there is deliberately no closed enumeration (diagnostics.rs:1-28). That is a good seed for the machine-readable protocol.
- Notifier firing order is deterministic: the snapshot is sorted by ListenerId (notifier_generic.rs:253-259), each listener is isolated with catch_unwind, and the re-entrancy semantics are documented.
- flui-macros is small (935 LOC), heavily used (about 250 derives) and resolves runtime paths robustly: facade-only manifests, renamed dependencies and `Itself` via an `extern crate self` alias.
- Compile-time child arity (flui_tree::Arity) is a real Rust-shaped improvement over Flutter, and rendering, objects and view share it.
- deny(missing_docs) and clippy pedantic apply across the bottom crates, with almost no unsafe (geometry 2, types 11, foundation 7 textual hits, tree 0).

## Problems

### flui-foundation is the dumping ground for runtime/platform protocol that has no lower home

- **Kind:** layering · **Severity:** high
- **Evidence:** lib.rs re-exports FrameEpoch, SurfaceGeneration, GpuResourceGeneration, GenerationGate, FrameStamp, PresentationId, PresentationAddress, RealmId, DataTransferId, OwnerAffinity, ClaimSlot. Consumers by grep: ClaimSlot only in flui-app and flui-platform; OwnerAffinity only in flui-platform; FrameStamp, SurfaceGeneration, PresentationAddress only in flui-app, flui-engine, flui-layer; GenerationGate has zero. crates/flui-foundation/src/affinity.rs:11-13 says it 'lives in flui-foundation — not flui-platform — so its behavior is exercised by a test suite CI actually runs (flui-platform's suite is excluded from the CI gate)'. The lib.rs comment on claim_slot gives the same rationale.
- **Impact:** A CI gap decides crate placement. Foundation, the most depended-on framework crate, gains window-runtime concepts, and every change to raster or presentation protocol recompiles 21 crates. At H3 this plumbing becomes 'stable' API through `flui::foundation`. It also works against the H1 plugin story, where plugins should see a small vocabulary.
- **Direction:** Separate 'framework vocabulary' (tree ids, keys, callbacks, notifier, diagnostics names, observe) from 'runtime protocol' (realm/presentation/frame/surface identity, generations, claim slot, affinity). Either put the runtime protocol in a `runtime` module that is #[doc(hidden)] or unstable and not re-exported by the facade, or give it a dedicated low-layer home that flui-layer, flui-engine and flui-platform share. Move OwnerAffinity and ClaimSlot back to flui-platform once its suite runs on the Linux/headless CI path. Record the decision in ADR-0041.

### flui-types carries dead and duplicate subsystems (physics, BoxConstraints, MaterialColors, orphan types)

- **Kind:** tech_debt · **Severity:** high
- **Evidence:** flui-types/src/physics/* (1,797 lines: Spring/Friction/Gravity/Tolerance) has zero `flui_types::physics` users, while flui-animation/src/simulation.rs:31-950 defines Tolerance, SpringDescription, SpringSimulation, FrictionSimulation, GravitySimulation and BoundedFrictionSimulation again, and widgets' scroll_physics uses the animation copy. types/layout/constraints.rs `BoxConstraints` (393 lines) has no importer; rendering uses crates/flui-rendering/src/constraints/box_constraints.rs. styling/material_colors.rs `MaterialColors` (570 lines) is referenced nowhere outside types. These have zero non-test consumers: PointerData, NotchedShape/CircularNotchedRectangle/AutomaticNotchedShape, InlineSpanTrait, ImageShader, MaskFilter, HSLColor/HSVColor, Color32 (despite being in the root re-export and prelude), FractionalOffset, DeviceOrientation, StrutStyle, ShadowQuality, BorderDirectional, TextDecorationConfig, GlyphInfo, PlaceholderSpan. MouseCursor exists (typography/text_spans.rs:402), yet flui-material/src/button_style.rs:57 says 'FLUI has no MouseCursor type yet'.
- **Impact:** Roughly 3k+ lines of public API that would be frozen at H3 and has to be reviewed at every publish. Duplicate simulations can diverge silently, and two BoxConstraints types invite the wrong import. A Material palette in L0 undermines ADR-0028's decoupling and the plan to ship Material as an official package. When a grab-bag crate's own consumers cannot find what is in it, the crate has no clear ownership.
- **Direction:** Delete physics (flui-animation owns it), the types BoxConstraints and MaterialColors (move to flui-material if it is ever needed), and the orphan types. Adopt a rule that a flui-types item needs consumers in at least two crates, otherwise it lives with its single owner. Enforce it with `cargo xtask` or a pub-item consumer report, the same way the 'Unwired surface' review rule does.

### flui-geometry ships a second, unused GPUI-style vocabulary

- **Kind:** tech_debt · **Severity:** medium
- **Evidence:** Zero consumers outside geometry and types tests for: length.rs (1,160 lines: Length, DefiniteLength, AbsoluteLength, Percentage, Rems), transform2d.rs (420), bezier.rs CubicBez/QuadBez (606), text_path.rs (230: wave_offset, spiral_position, …), bridges/kurbo.rs (236, behind a `kurbo` feature no manifest enables), ScaleFactor<Src,Dst>, Radians, QuarterTurns, GeometryError (error.rs 347), and the traits GeometryOps, NumericUnit, FloatUnit, Along, ApproxEq, MaybeLerp. The `mint` feature is a documented no-op (Cargo.toml:47-50). The types `simd` feature gates unsafe SSE/NEON colour paths that nothing enables (color.rs:252-270).
- **Impact:** About 18% of the crate is speculative surface that becomes semver-bound at crates.io publish or H3. Code behind a never-enabled feature (kurbo, simd) is never built by CI's default feature set, so it rots unseen.
- **Direction:** Remove the unused modules and no-op features. Re-add CSS-like lengths only when a layout feature needs them, backed by an ADR. If kurbo interop is wanted for Parley or Vello (ADR-0077), wire it on the path that uses it and build it in CI.

### Parallel geometry vocabularies: Rect vs Bounds, two Axis enums, three transform types

- **Kind:** api_dx · **Severity:** medium
- **Evidence:** Rect<T>{min,max} (rect.rs:46) and Bounds<T>{origin,size} (bounds.rs:64, doc: 'Following GPUI's convention'): Bounds< is used by flui-platform, flui-interaction and flui-widgets, and geometry::Bounds by flui-app and flui-platform, while the render stack uses Rect. flui_geometry::traits::Axis (traits.rs:19) and flui_types::layout::Axis (layout/axis.rs:13) are distinct enums with the same variants. Transform (enum, transform.rs:189), Transform2D and Matrix4 coexist. Offset, Point and Vec2 all exist, and Offset<Pixels> is re-exported under the aliases PixelOffset and EdgeInsets = Edges<Pixels>. flui-semantics also defines its own TextDirection (properties.rs:16) although it depends on flui-types, which has one.
- **Impact:** Conversions are needed at every seam between platform/interaction and rendering. Users meet two same-named types (`Axis`) through the prelude or facade with no guidance. Flutter-migration docs (a product bet) get harder. Once frozen at H3, deduplication becomes a major-version break for every community render object (the H0 extension point).
- **Direction:** Pick one rectangle (keep Rect, and make Bounds a constructor or view if origin+size is needed), one Axis (in geometry, re-exported by types), and one 2D position/displacement story (Point + Offset, drop Vec2 or make it an alias). Record the decision in a flui-geometry ARCHITECTURE.md `## Mapping decisions`.

### Pixels (and so Rect/Size/Offset/BoxConstraints) violates the Eq/Hash/Ord consistency contract

- **Kind:** safety · **Severity:** medium
- **Evidence:** crates/flui-geometry/src/units.rs:91 has `#[derive(Copy, Clone, Default, PartialEq)] pub struct Pixels(pub f32);` (IEEE ==). Line 575 has `impl Eq for Pixels {}`, 584-588 have Ord via `f32::total_cmp`, and 591-595 have Hash via `to_bits`. Rect derives `PartialEq, Eq, Hash` (rect.rs:43). flui-rendering's BoxConstraints docs recommend using it as a cache key (box_constraints.rs 'Implements Hash and Eq for use as cache keys').
- **Impact:** Failure scenario: `px(0.0) == px(-0.0)` is true, but their hashes differ (to_bits), so a HashMap<Size,_> layout or intrinsics cache misses a key that compares equal. `px(NaN) != px(NaN)` although the type claims Eq, so HashMap lookups for NaN-bearing constraints never hit and dedupe fails. Ord puts -0.0 < 0.0 while == says they are equal, so a BTreeMap and == disagree. This is a latent correctness hazard in H2's caching work (layout cache, sliver cache #1199, damage).
- **Direction:** Make equality consistent with Hash/Ord: implement PartialEq via total_cmp or bits after canonicalising -0.0 to 0.0, or drop Eq/Hash/Ord from float units and give cache keys an explicit quantised key type (as `round_for_cache` already hints). Add a property test that `a == b ⇒ hash(a) == hash(b)`.

### Three ID schemes, ABA-unsafe Layer/Semantics ids, and a stale ID contract in AGENTS.md

- **Kind:** runtime_architecture · **Severity:** medium
- **Evidence:** id.rs:614-760 defines plain `Id<M>` (1-based NonZeroUsize with get()) for ViewId, LayerId, SemanticsId and scheduler ids; `GenId<M>` for RenderId, RealmId, DataTransferId; a bespoke `ElementId(NonZeroU64)` at 1163 that re-implements the GenId packing; and PresentationId wrapping GenId (984). The GenId docs name 'LayerId/SemanticsId candidates' (id.rs:800-803). ViewId has no production user (only a flui-tree test and a flui-app doc comment). AGENTS.md:138 still states ElementId/RenderId are 1-based NonZeroUsize with `id.get() - 1`, which compile_fail doctests now forbid. docs/crates.md:18 says IDs are 'all NonZeroUsize-backed'.
- **Impact:** H2 damage tracking and layer caching (ADR-0061 'damage needs layer identity') would key retained caches on a reusable slab index, which is the exact ABA bug RenderId was converted to avoid. The agent protocol (principle 4: 'stable IDs') would also expose SemanticsId that can alias after reuse. Agents and contributors following AGENTS.md get the wrong rule.
- **Direction:** Converge on one generational `GenId<M>`: make ElementId `GenId<Element>`, move LayerId and SemanticsId to it before H2, and delete ViewId. Update AGENTS.md's 'ID offset' row and docs/crates.md in the same change, and make the rule 'generational ids expose no index outside their owning tree'.

### The key system is split across layers, and the View concept leaks into foundation

- **Kind:** layering · **Severity:** medium
- **Evidence:** foundation/src/key.rs defines Key(NonZeroU64), the dyn `ViewKey` trait, ValueKey, UniqueKey and SaltedKey. `ViewKey::is_global_key` documents BuildOwner registry and ElementTree behaviour (key.rs:388-405). GlobalKey and ObjectKey live in flui-view/src/key/, each with its own process-global counter (global_key.rs:74, key.rs:59,64). `Key::from_str` uses a const FNV-1a 64-bit hash (key.rs:94-120).
- **Impact:** Reconciliation identity, a Stable-tier concept, is spread over L1 and L5, so foundation must change whenever reconciliation semantics change. Hypothesis: two distinct string keys that collide under FNV-1a compare equal and would silently reconcile the wrong child. The probability is low per pair, but because it is a silent-failure mode, a debug-time check is warranted. Process-global key counters make key values depend on test order within one process, which matters for record/replay (G7) if keys are ever serialised.
- **Direction:** Keep only a plain identity value in foundation, or move all reconciliation keys (ViewKey/ValueKey/UniqueKey/GlobalKey/ObjectKey) into flui-view. In debug builds, detect key collisions by storing the original string. Consider realm-scoped counters to support deterministic replay.

### Several overlapping change-notification mechanisms, and the foundation docs contradict ADR-0074

- **Kind:** plan_misfit · **Severity:** medium
- **Evidence:** foundation has ChangeNotifier/ValueNotifier (a Notifier<()> of `Arc<Mutex<HashMap<ListenerId,_>>>` + Arc<AtomicUsize> + Arc<AtomicBool>, notifier_generic.rs:41-45, Listenable: Send + Sync, notifier.rs:78), a generic Notifier<Arg>, and ListenerRegistry<S> (523 lines, zero consumers outside foundation). flui-view has realm-scoped signals (ADR-0074 Accepted). crates/flui-foundation/ARCHITECTURE.md 'Outstanding refactors' says the signals crate was removed and 'contract C1 locks the catalog to the setState/Inherited model', and calls itself 'a Layer 0 crate' (it is layer 1).
- **Impact:** The plan makes signals canonical, with setState/ValueNotifier as the low-level layer. Foundation still presents ChangeNotifier as 'the' mechanism, and each ChangeNotifier takes a lock per add/remove/notify inside a single-writer realm. That goes against AGENTS.md's 'locks guard shared infrastructure only', and it is used by rendering and objects (ViewportOffset-style objects). An unused third registry adds freeze surface.
- **Direction:** Delete ListenerRegistry, or wire it with a named follow-up. Decide whether ChangeNotifier needs Send+Sync; a realm-owned `!Send` notifier would drop the Mutex. Rewrite foundation's ARCHITECTURE.md from its current Flutter reference dump into the current contract and point it at ADR-0074/0075.

### The Flutter-port Diagnostics tree is disconnected from the machine-readable protocol

- **Kind:** plan_misfit · **Severity:** medium
- **Evidence:** foundation/src/debug.rs (1,969 lines: DiagnosticsNode, DiagnosticsBuilder, Diagnosticable, DiagnosticsTreeStyle with 0 external users, DebugPaintConfig with 0 external users) backs 104 `debug_fill_properties` impls across engine, layer, objects, painting and rendering. The dumps are consumed only by flui-app tests. flui-devtools uses `observe`/RebuildReason (devtools/src/inspector.rs:12-13), and tools/desktop-mcp uses AccessKit/UIA. None of them reads DiagnosticsNode.
- **Impact:** Principle 4 ('DevTools is a client of the same protocol as the agent'; stable IDs; structured data) is contradicted by a third inspection format that is maintained at every render object but feeds no consumer. It will be frozen at H3 and duplicated again when G-track inspectors need render-tree properties.
- **Direction:** Either make DiagnosticsNode the serialisable property payload of the agent/devtools protocol (serde, stable field names from `diagnostics`), or shrink it to Debug-only output and drop the dead styles and configs. Decide in the ADR-0080 follow-up.

### The flui-tree trait trio has no generic consumer, and the central ElementTree does not use it

- **Kind:** extension_point · **Severity:** medium
- **Evidence:** The only `impl TreeRead/TreeNav/TreeWrite` blocks are in flui-layer/src/tree/tree_traits.rs:18,45, flui-rendering/src/storage/tree.rs:953,980,1028 and flui-semantics/src/tree.rs:643,676,737. A grep for `: TreeNav<`, `impl TreeNav`-bounded generics and `dyn Tree*` outside flui-tree finds none; call sites only import the traits for method syntax. flui-view's ElementTree (tree/element_tree.rs:394) does not implement them. Zero external users: TreeWriteNav, AtomicDepth, DepthAware, SlotBuilder, TreeError, TreeResult. flui-tree pulls the `bon` proc-macro just for Slot's builder (iter/slot.rs:84-139). lib.rs re-exports ElementId and has `crate_summary()`/`VERSION` boilerplate.
- **Impact:** A published L2 crate whose headline abstraction delivers no polymorphism. It adds a third-party trait surface to freeze (H3) and a second place to change tree invariants, such as the cascade-remove semantics in write.rs. The pieces that do carry weight (Arity, Slot, Depth) are render/element protocol concepts, and they sit in a crate named for something else.
- **Direction:** Keep Arity, Slot and Depth, but decide whether the trait trio earns its keep: either make ElementTree implement it and write one real generic consumer (inspector/agent tree walker, test harness), or demote the traits to inherent methods and fold flui-tree into flui-foundation (arity) plus flui-rendering. Drop bon and the unused items.

### Colour is fixed at 8-bit sRGB in the most-shared value type

- **Kind:** flutter_divergence · **Severity:** medium
- **Evidence:** crates/flui-types/src/styling/color.rs:25-34 is `pub struct Color { r: u8, g: u8, b: u8, a: u8 }`. Color32([u8;4]) exists but has zero users. The size assertion text says 'Color should be ≤16 bytes (4×f32 RGBA)' (lib.rs size_assertions), which does not match the struct. No colour-space type exists anywhere in the workspace (grep ColorSpace/DisplayP3 hits only wgpu surface selection in engine/renderer.rs:1562-1577).
- **Impact:** Tweened colours quantise per frame (visible banding on slow gradients or animations). There is no wide-gamut or HDR path, and linear/coverage-correct blending (ADR-0057) has to upconvert everywhere. Themes-as-data (H1) and M3 Expressive dynamic colour will serialise the u8 form. Flutter itself moved Color to f32 components with a colorSpace (Flutter 3.27). Changing this after H3 breaks every crate and theme file. The divergence is unrecorded: there is no flui-types ARCHITECTURE.md.
- **Direction:** Decide before the crates.io publish. Options: f32 linear or sRGB components plus a color-space tag, keeping a packed u8 form as a GPU/storage type; or record the u8 choice as a deliberate divergence with the upgrade path. Delete or unify Color32.

### No stability tiering at the bottom: the facade re-exports ~2,500 public items wholesale

- **Kind:** api_dx · **Severity:** high
- **Evidence:** src/lib.rs:132-150 re-exports `flui_foundation as foundation`, `flui_geometry as geometry`, `flui_types as types`. The public-item count (a regex count of `pub fn|struct|enum|trait|type|const`) is about 816 for geometry, 1,226 for types, 322 for foundation and 145 for tree. All five manifests are `publish = ["crates-io"]`. The plan (Управление, H3) requires Stable/Evolving/Experimental tiers and cargo-semver-checks green for 3 minors.
- **Impact:** At H3 everything reachable becomes semver-stable, including runtime plumbing (ClaimSlot, SurfaceGeneration), dead modules and duplicate vocabulary. Every later cleanup becomes a major version. This is the costliest item for H3 and H4, since community crates bind to these paths first.
- **Direction:** Before first publish, define the Stable vocabulary explicitly: a curated facade `prelude`/modules instead of `pub use crate as module`, with internal protocol behind #[doc(hidden)] or an `unstable` feature. Add a public-API snapshot (cargo-public-api) gate for the bottom crates so that growth is a reviewed diff.

### Missing and stale architecture docs for the bottom crates, so Flutter divergences go unrecorded

- **Kind:** docs · **Severity:** medium
- **Evidence:** No ARCHITECTURE.md exists for flui-geometry, flui-types or flui-tree (16 of 27 crates have one). foundation's ARCHITECTURE.md is a Flutter reference walk (693 lines) with stale claims: §18 says keys live in flui-view/src/key/*.rs, 'IDs: Plain integers → Id<T: Marker>' ignores GenId/ElementId, it calls itself 'Layer 0', and it says signals were removed. docs/crates.md:18 mentions 'px, dp' (no dp type exists) and 'all NonZeroUsize-backed' ids. flui-types/Cargo.toml comments mention 'mint and glam feature flags' although only mint exists, and describe `simd` as a forwarded geometry feature that no longer exists.
- **Impact:** AGENTS.md's Definition of Done treats an unrecorded divergence as a regression. Rect min/max vs LTRB, u8 Color, generational ids, the dual Rect/Bounds and the physics duplication have no recorded rationale. Agents (the main contributors, bus factor 1) are steered by stale docs.
- **Direction:** Write short contract-style ARCHITECTURE.md files (owns / does not own / mapping decisions / invariants) for geometry, types and tree. Replace foundation's Flutter dump with the same shape and move the Flutter walk to docs/archive.

### flui-types is a 22k-line monolith at the root of almost every crate

- **Kind:** workspace_topology · **Severity:** medium
- **Evidence:** 17 crates plus the facade depend on flui-types directly. Its modules serve disjoint consumers: gestures only interaction and widgets; typography mostly widgets, material and painting; painting (Path 1,489 lines, Image 884, Shader 841) engine, widgets, painting and layer; haptics and ime only platform, interaction and app. Geometry's own integration tests and benches still live in flui-types (tests/geometry_property_tests.rs, device_pixels_geometry_tests.rs, rems_unit_tests.rs, unit_mixing_compile_fail.rs, benches/geometry_bench.rs), while flui-geometry has no tests/ directory.
- **Impact:** Any edit to flui-types (for example a text-span field for Parley, ADR-0077) invalidates nearly the whole workspace on a memory-limited, single-worker dev host. An H1 PlatformCapability plugin that needs only HapticFeedback-style vocabulary links spans, paths and images. Geometry behaviour is verified by another crate's test target, which is post-split residue.
- **Direction:** Keep 'crates are layers' (a decision already taken), but shrink flui-types to values shared by at least two layers. Move single-owner modules to their owner: gesture details to flui-interaction, `Path`/`Paint`/`Shader` possibly to flui-painting (L2, below layer and engine). Move geometry tests and benches into flui-geometry. Measure the rebuild fan-out before and after rather than assuming.

### flui-macros' real dependencies are invisible to the layer check, and fallback modules are hard-coded

- **Kind:** layering · **Severity:** low
- **Evidence:** flui-macros is layer 1, but its expansions target flui-view (L5) and flui-animation (L3) (runtime_path.rs:8-20), with facade fallbacks hard-coded as `flui::view`, `flui::foundation`, `flui::animation`. The macro crate's own tests cover only Diagnosticable (tests/diagnosticable_derive.rs); the View derive behaviour is tested in flui-view/tests.
- **Impact:** Renaming or moving a facade module or a runtime trait breaks every derive only at the consumer's compile time. `cargo xtask workspace` cannot catch it, so the layer number misleads anyone reading the topology.
- **Direction:** Document the semantic edge in docs/crates.md and ADR-0041 as an allowed proc-macro exception. Add a facade-only trybuild test per derive (the ARCHITECTURE.md mentions consumer integration tests; make sure one runs in the fast lane).

## Unwired or dead surface

- flui-types physics module (friction, gravity, spring, tolerance; 1,797 lines), duplicated by flui-animation::simulation
- flui_types::layout::BoxConstraints (393 lines), duplicated by flui-rendering::constraints::BoxConstraints
- flui_types::styling::MaterialColors (570 lines)
- flui_types: PointerData, NotchedShape, CircularNotchedRectangle, AutomaticNotchedShape, InlineSpanTrait, ImageShader, MaskFilter, HSLColor, HSVColor, Color32 (in the root re-export and prelude), FractionalOffset, DeviceOrientation, StrutStyle, ShadowQuality, BorderDirectional, TextDecorationConfig, GlyphInfo, PlaceholderSpan; MouseCursor is unused and its intended consumers say it does not exist
- flui-types `simd` feature, which no manifest enables (color.rs:252-300 SSE/NEON paths)
- flui_geometry: Length, DefiniteLength, AbsoluteLength, Percentage, Rems (length.rs 1,160 lines), Transform2D (420), CubicBez/QuadBez (bezier.rs 606), text_path helpers (230), bridges/kurbo (236, `kurbo` feature never enabled), ScaleFactor, Radians, QuarterTurns, GeometryError (347), traits GeometryOps/NumericUnit/FloatUnit/Along/ApproxEq/MaybeLerp; the `mint` feature is a no-op
- flui_foundation: ViewId (no production user), GenerationGate, DebugPaintConfig, DiagnosticsTreeStyle, consts IS_DESKTOP/IS_MOBILE/IS_WEB/DEBUG_MODE/RELEASE_MODE (exported in the prelude, zero users), WasmNotSendSync (external), generic Notifier<Arg> (external), ListenerRegistry/ListenerSubscription (523 lines), RawId (external)
- flui_tree: TreeWriteNav, AtomicDepth, DepthAware, SlotBuilder, TreeError/TreeResult, crate_summary()/VERSION; the TreeRead/TreeNav/TreeWrite trio is implemented three times but never used generically
- Diagnosticable/DiagnosticsNode dumps: 104 debug_fill_properties impls, consumed only by flui-app tests, not by devtools or MCP

## Open questions

- Where should runtime-protocol identity (RealmId, PresentationId/Address, FrameEpoch, SurfaceGeneration, FrameStamp) live, given that flui-layer (L3), flui-engine (L4), flui-platform (L2) and flui-app all need it? Options: a foundation module that is unstable or doc(hidden) (no new crate), or a dedicated low layer. Either way the answer belongs in ADR-0041.
- Can the flui-platform suite be run on the headless/Linux CI path, so that OwnerAffinity and ClaimSlot can move back to their owner?
- Colour model before crates.io publish: f32 + colour space (Flutter 3.27 shape) vs u8 sRGB as a recorded divergence. Who owns the migration of themes-as-data (H1)?
- Should flui-tree survive as a crate? If the trait trio stays, the ElementTree should implement it and a real generic consumer should exist (the agent/inspector tree walker is a natural candidate). Otherwise, fold Arity/Slot/Depth into foundation or rendering.
- Should DiagnosticsNode become the property payload of the ADR-0080 agent/devtools protocol, or be reduced to Debug output?
- Which of Rect/Bounds, the two Axis enums, and Point/Offset/Vec2 is canonical? This is a one-time breaking cleanup that should precede the H0 crates.io publish.
- Is a cargo-public-api snapshot gate for L0/L1 acceptable as the tiering mechanism before H3, or should an `unstable` feature carry internal items?
- Hypothesis to measure: how much of a full rebuild does a one-line flui-types change trigger today on the dev host, and how much would splitting single-owner modules out save?

