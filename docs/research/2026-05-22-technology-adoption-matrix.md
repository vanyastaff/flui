# Technology & Pattern Adoption Matrix — Flutter → FLUI Port

**Date:** 2026-05-22
**Author:** Principal UI-framework architect (consultant)
**Status:** research — changes nothing; one of three parallel **architecture-foundations** inputs (siblings: an architecture-correction plan, a crate-decomposition redesign). The orchestrator synthesizes the three into a FOUNDATIONS decision doc; the master ROADMAP is built on top.

---

## 1. Intro & methodology

### 1.1 The question this answers

The prior research wave produced four documents: a Flutter↔FLUI gap matrix, a port-phasing dependency order, an architectural-contracts audit, and a Rust-UI-ecosystem lessons doc. Together they answer *what to build*, *in what order*, and *which public contracts gate the catalog*. They do **not** answer, subsystem by subsystem, the question this document owns:

> For each major FLUI subsystem — given that **behavior is always Flutter's** (the port mandate) — *whose structural pattern* should FLUI adopt, *which concrete Rust idioms and crates* implement it, and *does the current code already match or must it change direction*?

This is a build-vs-borrow analysis at the pattern level. Every subsystem gets an explicit, grounded **adoption decision**.

### 1.2 The two-axis frame (the core idea)

The port mandate splits cleanly into two independent axes, and the whole document hangs off keeping them separate:

- **Behavior axis** — *what the subsystem does*. Always sourced from Flutter (`STRATEGY.md`: "Behavior loyal … алгоритмы портируются 1:1 из `.flutter/`"). Not a decision — a constraint. Each section names the specific `.flutter/` algorithm.
- **Structure axis** — *how it is shaped in Rust*: ownership, dispatch mechanism, type-system tools, crate placement. This **is** a decision, and `STRATEGY.md` ("structure Rust-native") explicitly invites divergence from Flutter's structure. The candidate structural sources are Flutter itself, GPUI, Xilem, Masonry, Vello, Iced, Floem, or "Rust-native original."

This is *A Philosophy of Software Design* (Ousterhout), ch. 4 — "**different layer, different abstraction**." Flutter's *behavior* is the abstraction FLUI's users see; Flutter's *Dart class structure* is an implementation detail of a different language. Copying the Dart structure into Rust is the "**pass-through method**" anti-pattern at architecture scale: it adds a layer (the Rust transliteration) that provides no new abstraction over the layer it wraps (the Dart design). Where FLUI copies Dart structure, it should be because the structure is genuinely the best Rust shape — never by default.

### 1.3 Decision rubric

Every structural decision below is graded against five named principles. A decision without a principle behind it is not in this document.

1. **Deep modules** (Ousterhout ch. 4) — the best module has a *small interface* over *substantial functionality*. Prefer the structure that makes the subsystem's interface smaller relative to what it does.
2. **Information hiding / "different layer, different abstraction"** (Ousterhout ch. 4–5) — each layer hides a decision; if two adjacent layers expose the same abstraction, one is redundant.
3. **Compile-time over runtime** (`STRATEGY.md`; Constitution Principle 4; *Programming Rust* ch. 11 on traits & generics) — when a runtime check and a type-system check express the same constraint, the type system wins.
4. **Zero-cost abstraction** (*The Rust Performance Book*; *Rust for Rustaceans* ch. 3) — the chosen shape must not add per-frame cost the hot path cannot afford.
5. **Mental-model legibility** (`STRATEGY.md` key metric: "external PR contributors … mental model понятен снаружи") — FLUI's *product* is developer experience. A structure an outside contributor cannot understand is a defect even if it is fast.

### 1.4 What was verified

Every "verdict vs current FLUI" cites `file:line` read directly from the worktree this session, not inferred from crate names. Reference structure was read from `.flutter/flutter-master/packages/flutter/lib/src/` and `.gpui/` (paths per the brief). The four prior research docs are built **on**, not redone — where this document depends on a sibling's conclusion (architecture-correction, crate-decomposition) it states the assumption explicitly.

### 1.5 Dependency on sibling documents — explicit assumptions

- **Assumption A (crate-decomposition sibling).** The new crates `flui-widgets`, `flui-material`, `flui-cupertino`, `flui-localizations` are created; `flui-physics` is *not* (physics already lives in `crates/flui-types/src/physics/`); `flui-services` is *not* (Flutter `services` dissolved into `flui-platform` + `flui-assets`). This matches the port-phasing doc §5 and the brief. Crate *placement* recommendations below assume this layering.
- **Assumption B (architecture-correction sibling).** The architecture-correction plan owns the *sequencing and execution* of the fixes this document identifies (e.g., wiring the keyed reconciler, closing the `new_minimal` build-context hole). This document states the *target structure*; the sibling states *how to get there*. Where I say "must change direction," I assume the correction plan schedules it.

---

## 2. Master adoption table

| # | Subsystem | Behavior source (`.flutter/`) | Structural pattern source | Verdict vs current FLUI |
|---|---|---|---|---|
| 1 | Three trees & ownership | `widgets/framework.dart`, `rendering/object.dart` | **Flutter** (3-tree) + **Masonry** (library-owns-nodes) | **Matches** — Slab arenas + NonZeroUsize IDs are correct. Hold the line. |
| 2 | Widget→Element reconciliation | `widgets/framework.dart` `updateChild`/`updateChildren` | **Xilem** (`rebuild(prev,el)`) + **Flutter** (linear keyed algo) | **CHANGE** — live path is index-only (`child_storage.rs:494`); keyed reconciler exists but unused. |
| 3 | Layout protocol (constraints/intrinsics/baseline) | `rendering/box.dart`, `rendering/object.dart` | **Flutter** + FLUI **arity** type-state | **Matches structurally**; behavior gaps (empty-body constraint propagation) are bugs, not design errors. |
| 4 | Paint & display-list recording | `rendering/object.dart` `PaintingContext`, `dart:ui Canvas` | **Flutter** (`Canvas`→`DisplayList`) + **Skia/Vello** record-then-replay | **Matches** — `DisplayList` + `DrawCommand` is the right seam. |
| 5 | Layer / compositor tree | `rendering/layer.dart` | **Flutter** layer tree + **GPUI/Flutter** retained-layer lifecycle | **CHANGE** — no layer lifecycle protocol; rebuilds GPU layer every frame (per Cycle 2 audit). |
| 6 | GPU engine (tessellation) | n/a (Flutter uses Skia/Impeller — engine, not framework) | **lyon now** (CPU tess) → **Vello-hybrid later**; structural source = **Vello `SceneRecorder` seam** | **Matches for now**; needs an explicit `RasterBackend` seam so the lyon→Vello swap is non-breaking. |
| 7 | Text layout / shaping / IME | `painting/text_painter.dart`; `services/text_input.dart` | **Rust-native** (cosmic-text stack) + **GPUI** for platform IME | **Matches** on stack choice; IME platform bridge is a genuine gap in `flui-platform`. |
| 8 | Scheduler & frame loop | `scheduler/binding.dart`, `scheduler/ticker.dart` | **Flutter** phases + **GPUI/winit** `ControlFlow::Wait` | **Matches** — `flui-scheduler` is one of the most complete ports. |
| 9 | Gestures / hit-testing / pointer routing | `gestures/*` (arena, recognizers, hit_test) | **Flutter** 1:1 | **Matches** — `flui-interaction` ~95% ported; the model for the rest. |
| 10 | Animation | `animation/*` (controller, curves, tween) | **Flutter** + FLUI `Listenable` | **Matches** (crate disabled — re-enable, not redesign). |
| 11 | Reactivity / state model | `widgets/framework.dart` `State.setState`, `InheritedWidget` | **Flutter `setState`** + **Xilem `memoize`/`can_update`** | **CHANGE (additive)** — keep `setState` canonical, ADD `can_update` short-circuit; signals stay out. |
| 12 | `BuildContext` & inherited data | `widgets/framework.dart` `dependOnInheritedWidgetOfExactType` | **Flutter** semantics + **GPUI lease** for `&mut`-access | **Partial** — callback surface is right; `new_minimal` hole (`behavior.rs:222`) makes `depend_on` dead in real builds. |
| 13 | Heterogeneous children / type-erasure | `widgets/framework.dart` `MultiChildRenderObjectWidget` `List<Widget>` | **Xilem `ViewSequence`** (tuple trait) — *deliberately NOT Flutter structure* | **CHANGE** — `Children`/`BoxedView` (Vec-of-`dyn`) is the wrong primary surface. |
| 14 | Hot-reload | Flutter VM hot reload (not portable) | **Makepad** (designed-in) + **Rust-native** `cdylib` restart | **Matches** at the achievable bar (hot-restart); state-preserving reload is technically out of reach. |
| 15 | Platform abstraction | `services/*` (deliberately dissolved) | **GPUI** platform traits + callback registry | **Matches** — `Platform`/`PlatformWindow` traits + `Box<dyn>` erasure is the GPUI shape, correctly. |
| 16 | Asset pipeline | `painting/image_provider.dart`, `services/asset_bundle.dart` | **Flutter** `ImageProvider`/cache + **Rust-native** async IO | **Partial** — `flui-assets` disabled; structure sound, needs re-enable + `flui-engine` texture wiring. |

**Reading the verdict column:** **Matches** = current FLUI structure is correct, lock it. **Partial** = right direction, a specific hole must close. **CHANGE** = current structure is the wrong Rust shape and must be re-decided before the widget catalog leans on it. Four subsystems are CHANGE: reconciliation (#2), layer lifecycle (#5), reactivity (#11, additive), heterogeneous children (#13). The closing section §20 ranks them.

---

## 3. Subsystem 1 — The three trees & their ownership model

**1. Behavior source.** `widgets/framework.dart` (`Widget`/`Element`/`State` and the build phase) and `rendering/object.dart` (`RenderObject`/`PipelineOwner`). Flutter's three-tree split — immutable `Widget` config → mutable `Element` lifecycle → layout/paint `RenderObject` — is the architectural invariant. `inside-flutter.md` is the canonical description.

**2. Structural pattern source. Flutter for the three-tree split; Masonry (Linebender) for the ownership model.** The three-tree split itself is the whole reason FLUI exists and the ecosystem doc R1 confirms it: Xilem converged on the same thing (Masonry = retained layer ≈ Element+Render, Xilem reactive layer ≈ View). GPUI's single-tree-dropped-per-frame is productive for a code editor but, per `.gpui/src/element.rs` module doc, forces accessibility/IME/focus re-identification every frame — inadequate for a full Flutter-parity toolkit. **Do not adopt GPUI's element model.**

The *ownership* sub-decision is where Rust structure must diverge from Dart. Flutter's `Element` holds `Element? _parent` and `List<Element>` children — pointer-into-tree, which Dart's GC makes free. The Rust-native shape is **library-owns-nodes**: nodes in a `Slab` arena, parents/children are `NonZeroUsize` indices. This is Masonry's exact lesson — Xilem RFC 0001 documents that pre-Masonry `Vec<Pod<Widget>>` (each container owning its children) made it *impossible* to iterate the whole tree for an inspector or route a focus event without walking the ownership chain. Masonry moved widgets into a SlotMap; containers hold keys.

*Principle:* this is **information hiding** (Ousterhout ch. 5) — the arena hides "how a node is stored and addressed" behind an opaque `ElementId`. It is also **zero-cost** (*The Rust Performance Book*, the niche-optimization discussion): `ElementId = NonZeroUsize` means `Option<ElementId>` is 8 bytes, not 16, via Rust's niche optimization — a tree of N nodes saves 8N bytes on every optional parent/child link versus a nullable pointer. The `+1`/`-1` Slab offset (Slab is 0-based, IDs 1-based) is the idiom that buys the niche.

**3. Concrete Rust idioms & crates.**
- `slab::Slab<Node>` for each tree's backing store (already a workspace dep).
- `NonZeroUsize`-wrapped newtype IDs (`ViewId`/`ElementId`/`RenderId`/`LayerId`/`SemanticsId`) — `#[derive(Copy)]`, the `+1`/`-1` offset convention.
- **Typestate** (`Mounted`/`Unmounted`) for lifecycle — *Programming Rust* 2e calls this "states encoded in types"; it makes "operate on an unmounted element" a compile error.
- The unified `flui-tree` trait surface (`TreeRead`/`TreeNav`/`TreeWrite` + `Arity`) is the Rust-native consolidation of Flutter's four bespoke tree implementations — a genuinely *deeper* module than four parallel traversals (one interface, four backings).

**4. Verdict vs current FLUI: MATCHES — hold the line.** `flui-view/src/tree/element_tree.rs` is a `Slab<ElementNode>`; IDs are NonZeroUsize with the offset convention (CLAUDE.md documents it; foundation audit confirms). The element store is library-owns-nodes already. This is the single most important thing the prior research validates (ecosystem doc R1) and the current code is correct. **No change.** The one open item — `flui-view`'s `ElementTree` not implementing the `flui-tree` `TreeRead`/`TreeWrite` traits (Cycle 5 V-7, the unified-tree migration gap) — is a *consolidation* task, not a structural error: the arena shape is right, it is the trait-surface unification that is unfinished.

**5. Risk if adopted wrong.** Getting this wrong = adopting GPUI's drop-tree-per-frame model — which would make a Slab-arena retained tree dead weight and force accessibility/IME re-identification every frame. The current code does *not* make this mistake. The residual risk is the *reverse*: treating `flui-tree`'s unfinished unified surface as a reason to abandon it. Per memory `flui-tree-unified-interface-intent` and `STRATEGY.md`, zero-consumer abstractions in `flui-tree` are a migration gap, not a deletion signal.

---

## 4. Subsystem 2 — Widget→Element reconciliation

**1. Behavior source.** `widgets/framework.dart` — `Element.updateChild` (single child) and `RenderObjectElement.updateChildren` (the linear keyed list algorithm: sync-from-top, sync-from-bottom, keyed-`HashMap` middle, inflate the rest). `Widget.canUpdate` (`runtimeType == runtimeType && key == key`) gates reuse. `inside-flutter.md`: "Contrary to popular belief, Flutter does not employ a tree-diffing algorithm" — it is O(N) linear.

**2. Structural pattern source. Xilem for the per-node primitive shape; Flutter for the list algorithm.** The reconciliation *primitive* should be Xilem's `View::rebuild(prev, element, state)` — it receives the previous description and the retained element, mutates in place, recurses. The ecosystem doc §2 establishes this matches Flutter's `Element.update(newWidget)` 1:1 and is "fundamentally compatible with Rust's borrow checker" because `View` is a value type, not an observer. The *list-level* algorithm (the keyed middle section) is Flutter's linear algorithm, directly portable.

*Principle:* **compile-time over runtime.** The current FLUI update path does `child.update(view.as_ref(), ...)` where `view` is `&dyn View` and the receiving element internally `downcast_ref::<V>()`s — a type match that *can fail at runtime* (`generic.rs` logs `tracing::warn!` on downcast failure, per the architectural-contracts audit Contract 2). A reconciler keyed on Xilem's typed `rebuild` makes the mismatch a compile error. This is exactly the runtime-check-that-should-be-a-type-check the rubric forbids.

**3. Concrete Rust idioms & crates.**
- `View::can_update(&self, prev: &Self) -> bool` — typed, not `&dyn View`. (See §13 reactivity: this is also the `memoize` hook.)
- A general `key: Option<flui_foundation::Key>` field on `ElementNode`, set at insert from `View::key()`. Today only `GlobalKey` is stored (as a hash side-channel, `element_tree.rs:40`); `ValueKey`/`ObjectKey` are *not* stored, so the keyed middle section has nothing to key on.
- `IndexedSlot` (previous-sibling-aware slot, already re-exported from `flui-view/lib.rs`) unified across the single- and variable-child paths so render-tree child moves are correct.
- `std::collections::HashMap<Key, ElementId>` for the keyed middle — Flutter's exact structure.

**4. Verdict vs current FLUI: CHANGE — this is a silent-correctness trap.** Verified this session: `VariableChildStorage::update_with_views` (`crates/flui-view/src/element/child_storage.rs:494-515`) is **pure index-match** — `for (i, view) in views.iter().enumerate()`, update child `i`, push extras, drain the tail. Its own comment at line 495-496 says `// TODO: In a full implementation, this would use keys for reordering`. The keyed `reconcile_children` (`crates/flui-view/src/tree/reconciliation.rs:51`) is a real implementation with the start/end fast paths — but `grep` for callers returns **only** `lib.rs` re-exports and the test file (`reconciliation.rs:244,259,282,312` are all `#[cfg(test)]`). **Zero production callers.** The architectural-contracts audit Contract 5 reaches the same verdict independently.

This is the most dangerous class of defect: static lists reconcile *fine* positionally, so demos pass. State loss surfaces only on reorder — a dismissed list item, a sorted table, a reordered tab bar — in *user* apps, not FLUI's tests. It is a behavior divergence from Flutter, which `docs/PORT.md` "Flutter behaviour primacy" forbids outright.

**The change:** finish-and-wire `reconcile_children` as the sole variable-arity reconciler; add the general `key` field to `ElementNode`; delete the index path. The cost is *finishing*, not *designing* — the algorithm is written. It must precede the catalog: cheaper to validate the reconciler with 0 list widgets than to re-validate every list/grid/table widget after.

**5. Risk if adopted wrong.** If the catalog ships on the index path, every `ListView`/`Table`/`TabBar`/`ReorderableListView` is written and tested against positional reconciliation, *appears* to work, and silently loses scroll/focus/animation state on every reorder. Fixing the reconciler afterward changes observable behavior under every list widget — a catalog-wide re-validation. This is the "looks done, isn't" trap.

---

## 5. Subsystem 3 — Layout protocol (constraints / intrinsics / baseline)

**1. Behavior source.** `rendering/box.dart` (`BoxConstraints`, intrinsic sizing, baseline) and `rendering/object.dart` (the `layout(constraints)` → `performLayout` → size flow). Flutter's protocol: **constraints flow down, sizes flow up, parent sets child offset** — single-pass, with `computeDryLayout` and `computeIntrinsic*` as the auxiliary queries. `inside-flutter.md` "Sublinear layout."

**2. Structural pattern source. Flutter, with FLUI's arity type-state as the Rust-native overlay — NOT Taffy.** The ecosystem doc §6 is explicit and correct: Taffy (flexbox, used by GPUI and Floem) is a tempting alternative but "Flutter's constraint protocol and Taffy give different results for the same widget tree." Adopting Taffy would make FLUI's layout *behavior* diverge from Flutter — a direct violation of the behavior axis. **Stay with Flutter's constraint protocol.**

The Rust-native structural addition is FLUI's **arity system** (`Leaf`/`Single`/`Optional`/`Variable`). Flutter has no compile-time child-count safety — `RenderObjectWithChildMixin` vs `ContainerRenderObjectMixin` is a mixin choice, and a `RenderProxyBox` with a missing child is a runtime null. FLUI's `RenderBox<A: Arity>` makes child count a type parameter: `BoxChild<Single>` vs `BoxChild<Variable>`.

*Principle:* **compile-time over runtime** (`STRATEGY.md` names exactly this: "Arity system … ловит arity-mismatch на этапе компиляции, а не paint"). It is also a **deep module** improvement — the arity marker is a *zero-runtime-size* type parameter (`Leaf` etc. are ZSTs) that moves an entire bug class out of the interface. *Programming Rust* 2e: zero-sized marker types carry information for the compiler at no runtime cost.

**3. Concrete Rust idioms & crates.**
- `Arity` as a **sealed trait** with ZST implementors `Leaf`/`Single`/`Optional`/`Variable` — sealed so the closed set is enforced (`STRATEGY.md` "Sealed traits … exhaustive match").
- `RenderBox<A: Arity>` generic over arity; `BoxConstraints` as a plain `Copy` value type.
- `BoxParentData` indirection retained — `docs/PORT.md` "Mapping rules" explicitly says `RenderObject::parent_data` indirection *stays* even though an arity-keyed enum could eliminate it. This is a deliberate Flutter-primacy carve-out; do not "optimize" it away.
- Intrinsic-size queries as separate trait methods (`compute_intrinsic_size(axis)`), not folded into `layout`.

**4. Verdict vs current FLUI: MATCHES structurally; behavior gaps are bugs.** `crates/flui-rendering/src/traits/render_box.rs` + `protocol/box_protocol.rs` + `constraints/box_constraints.rs` exist; `objects/padding.rs` (read this session) is a clean `RenderBox<Single>`-shaped object using `BoxConstraints`/`BoxParentData`. The arity system is SOLID per Cycle 3. The structure is right.

The *behavior* defects the port-phasing doc §6.1 flags — `propagate_constraints_to_child` and `sync_child_size_to_parent` being **empty-body methods called every frame** in `flui-rendering` — are bugs in the layout *implementation*, not errors in the layout *design*. They belong to the architecture-correction sibling (Assumption B) and are Phase-0 blockers in the port-phasing doc. No structural change needed; the protocol shape is correct.

**5. Risk if adopted wrong.** The risk was Taffy — adopting a CSS-flexbox engine would silently diverge layout behavior from Flutter, and every widget would lay out *almost* like its Flutter twin but not exactly. FLUI did not make this mistake. The residual risk is leaving the empty-body propagation unfixed and building the catalog on it — caught by the port-phasing doc's Phase-0 gate (the 3-level nested-layout exit test).

---

## 6. Subsystem 4 — Paint & display-list recording

**1. Behavior source.** `rendering/object.dart` `PaintingContext` (the per-`RenderObject` `paint(context, offset)` recursion, `pushLayer`, repaint boundaries) and `dart:ui`'s `Canvas`/`Picture` recording API. Flutter records paint ops into a `Picture` per repaint-boundary, not immediate-mode drawing.

**2. Structural pattern source. Flutter's record-then-replay, structurally identical to Skia's `SkPicture` and Vello's scene encoding.** The paint phase produces a *recording* (a serializable list of draw ops), which a separate raster phase consumes. This is the universal shape — Flutter (`Picture`), Skia (`SkPicture`), Vello (scene). It is the right seam because it **decouples paint from raster** (Ousterhout "different layer, different abstraction"): the widget/render layer says *what* to draw; the engine layer decides *how* to rasterize.

*Principle:* **information hiding.** `DisplayList` hides the GPU backend entirely from `flui-rendering`. `flui-rendering` never names wgpu, lyon, or a shader. This is also what makes Subsystem 6 (the lyon→Vello swap) possible without touching render objects — the recording is the firewall.

**3. Concrete Rust idioms & crates.**
- A `DrawCommand` enum — `#[non_exhaustive]` (it is an evolving public-ish contract; *Rust API Guidelines* C-STRUCT-PRIVATE / future-proofing).
- A `Canvas` value type that *appends* to a `DisplayList` (the recorder), single-threaded `Send` — the lock-decision table in `docs/PORT.md` confirms "`DisplayList` recording is single-threaded `Send`."
- The **double dispatch** pattern: `dispatch_command<R: CommandRenderer>(cmd, renderer)` — the recording is generic over a `CommandRenderer` trait, so the same `DisplayList` can drive the wgpu backend, a headless test backend, or an SVG dumper. This is enum-dispatch + a visitor trait, *not* `dyn`.

**4. Verdict vs current FLUI: MATCHES.** Verified this session: `crates/flui-engine/src/commands.rs` has `dispatch_command<R: CommandRenderer + ?Sized>(command: &DrawCommand, renderer: &mut R)` and `dispatch_commands` (batch); `crates/flui-engine/src/traits.rs` defines `CommandRenderer` with the full op set (`render_rect`, `render_rrect`, `render_path`, `render_text`, `render_image`, `render_shadow`, …). The recording-then-replay seam is built, and it is the generic `CommandRenderer` shape — not a `dyn` boundary. The Cycle-5 painting audit confirms `Canvas`/`DisplayList`/`DrawCommand` are "parity-clean and consumed correctly by engine." This is correct; lock it.

The painting-audit note that ~31% of `flui-painting` is zero-consumer (a duplicate `tessellation` module, test-only `TextPainter`) is dead weight to feature-gate/delete — a hygiene task, not a structural error in the paint model.

**5. Risk if adopted wrong.** The anti-pattern would be immediate-mode painting — `RenderObject::paint` issuing wgpu draw calls directly. That couples every render object to the GPU backend, kills the lyon→Vello migration path, and makes headless testing impossible. FLUI did not do this. The one thing to **protect**: per the port-phasing doc R6, freeze the `DrawCommand`/`Scene` contract at end of Phase 0 so the parallel `flui-engine` track cannot drift it.

---

## 7. Subsystem 5 — The layer / compositor tree

**1. Behavior source.** `rendering/layer.dart` — the `Layer`/`ContainerLayer` tree, `OffsetLayer`/`ClipRectLayer`/`OpacityLayer`/`TransformLayer`/`BackdropFilterLayer`/`LeaderLayer`/`FollowerLayer`/`TextureLayer`/`PlatformViewLayer`, and crucially the **retained-layer lifecycle**: `Layer.engineLayer` caches the compositor's handle, `addToScene` is skipped when `!alwaysNeedsAddToScene && !_needsAddToScene`, and `LayerHandle` ref-counts to keep engine layers alive across frames.

**2. Structural pattern source. Flutter's layer tree for the structure; Flutter's own retained-layer lifecycle (mirrored by GPUI's element-handle persistence) for the ownership.** The layer tree shape — a tree of compositing primitives that the paint phase builds and the compositor consumes — is correct and FLUI has it (24 layer files in `flui-layer`, per the gap matrix). The missing piece is the *lifecycle*: a layer must **retain** its GPU-side resource across frames and only re-upload when dirty. This is exactly the `needsPaint`/retained pattern that distinguishes a retained-mode framework from an immediate one (ecosystem doc §8: immediate-mode pays full re-raster every frame; retained-mode with dirty bits does not).

*Principle:* **zero-cost abstraction / the hot path.** Rebuilding the GPU layer tree from scratch every frame is the largest avoidable frame-budget tax — it is the retained-vs-immediate distinction. *The Rust Performance Book*'s central theme is "do not redo work"; a layer whose inputs did not change must not re-encode. It is also **information hiding**: a `LayerHandle` (RAII guard) hides "this layer owns a GPU resource that must be released on drop" — the absence of one means the ownership is *not modeled*, which is the Cycle-2 finding.

**3. Concrete Rust idioms & crates.**
- A `needs_add_to_scene: AtomicBool` dirty bit per layer — lock-free, matching the `AtomicRenderFlags` precedent in `flui-rendering` (`docs/PORT.md` Trigger 4 cites this as the in-crate standard).
- `LayerHandle<L>` as an **RAII guard** (*Programming Rust* 2e ch. 'Drop'; *Rust for Rustaceans* on RAII) — owns a ref-count, releases the retained `engine_layer` on `Drop`. This is Flutter's `LayerHandle` ported as a Rust RAII type, which is *more* correct than Dart's manual ref-counting because `Drop` is deterministic.
- The 19-variant `Layer` enum — the Cycle-2 audit flags it as 360+ bytes per node; **box the large variants** (`Box<BackdropFilterLayer>` etc.) so the enum is pointer-sized. *The Rust Performance Book* "large enum variants" is the exact named pattern; `clippy::large_enum_variant` is the lint.
- Retained `engine_layer: Option<EngineLayerId>` field, populated by the compositor, cleared by the dirty bit.

**4. Verdict vs current FLUI: CHANGE — no layer lifecycle protocol.** The Cycle-2 `flui-layer`/`flui-semantics` audit verdict (quoted in the port-phasing doc §6) is "**fundamentally absent Layer lifecycle protocol**" — no `Drop`, no ref-counted `LayerHandle`, no `needs_add_to_scene` dirty bit, no `engine_layer` retention. Verified this session: `grep` for `needs_add_to_scene`/`engine_layer`/`LayerHandle` across `crates/flui-layer/src/` matches only `scene.rs`, `tree/layer_tree.rs`, and two unrelated layer files — the lifecycle primitives are not there as a coherent protocol. Consequence: **every frame rebuilds the GPU layer from scratch**, and retained rendering — the entire point of a layer tree — is lost.

This is a structural gap, not a bug: the *type system does not model layer ownership*. The fix is to add the lifecycle protocol (dirty bit + `LayerHandle` RAII + `engine_layer` retention) and box the enum. The repair plan `2026-05-22-004` (cited in the port-phasing doc) already scopes Waves 1-4 for exactly this. It is *not* a widget-correctness blocker (widgets render correctly, just slowly) — but it must land before a real app, and it is genuinely a "change direction" item because the current code has the wrong shape.

**5. Risk if adopted wrong.** Leaving it = FLUI is a retained-mode framework on paper and an immediate-mode one in practice — paying full GPU-layer re-encoding every frame, so the three-tree's whole performance argument collapses. The reverse risk (over-engineering: a too-clever incremental-compositing scheme) is real but smaller — Flutter's `LayerHandle` + dirty bit is a *simple* protocol; port that, do not invent.

---

## 8. Subsystem 6 — The GPU engine: tessellation (lyon CPU vs Vello compute)

**1. Behavior source.** Not applicable in the usual sense — Flutter's rasterizer is the C++ **engine** (Skia, now Impeller), *not* the framework being ported. There is no `.flutter/` framework algorithm here. The "behavior" FLUI must preserve is the *visual output* — anti-aliased path fills/strokes, correct blend modes — not a Dart algorithm.

**2. Structural pattern source. lyon (CPU tessellation) for the current phase; the Vello `SceneRecorder`/`RasterBackend` seam as the structural target for the eventual swap.** This is the decision the brief calls out for "deep attention," and `STRATEGY.md` says "stay pragmatic." The ecosystem doc §10 has the full tradeoff table; the verdict synthesizes to:

- **lyon is correct *now*.** It is mature, stable, works on *every* wgpu target including integrated GPUs and WASM/WebGL2 (only needs vertex/fragment shaders), and is a drop-in. Vello requires **compute-shader support** (WebGPU-level) — which excludes WebGL2 without the `vello_hybrid` fallback (merged March 2025, still maturing). Switching renderers during the `flui-platform` MVP, per `STRATEGY.md`'s "Current Priority: Complete flui-platform MVP," would be premature — it would destabilize the one thing currently being stabilized.
- **Vello-style GPU compute is the long-term direction.** Vello's prefix-sum compute rasterization (177fps on `paris-30k`) eliminates CPU tessellation entirely and does masking/blending in a single compute pass — strictly better for dynamic scenes once the hardware floor is acceptable. The `vello_hybrid` (CPU path processing + GPU compositing) is the risk-mitigated migration path because it does not demand full compute shaders. Vello's "sparse strip" research (issue #670) is the thing to watch.

*Principle:* this is **information hiding** applied as *future-proofing*. The decision "which rasterizer" must be hidden behind an interface so it can change. *A Philosophy of Software Design* ch. 5: "the most important technique for hiding information is … to design modules so the information needed by one is not needed by others." `flui-rendering` and `flui-painting` must not need to know lyon exists. The `DisplayList`/`DrawCommand` recording (Subsystem 4) is already 90% of that firewall — the remaining seam is *inside* `flui-engine`: a `RasterBackend` trait that lyon implements now and Vello implements later.

**3. Concrete Rust idioms & crates.**
- A `RasterBackend` (or `PathRasterBackend`) **trait** in `flui-engine` with the path-rasterization interface: `fill_path(&Path, &Paint, &Matrix4)`, `stroke_path(...)`. lyon is the current `impl`; Vello a future `impl`. This is *not* on the per-frame hot path as a `dyn` call — it is selected once at engine init, so static dispatch (a generic `Engine<R: RasterBackend>`) or a single `enum RasterBackend { Lyon(..), Vello(..) }` both work; the enum is simpler and the ecosystem doc R4 endorses the seam.
- `lyon = "1.0"` (current dep, verified in `crates/flui-engine/Cargo.toml:57`). `wgpu` 25.x (CLAUDE.md: stay on 25.x, 26.0+ broken).
- Keep `flui-painting`'s duplicate `tessellation` module **deleted** (Cycle-5 hygiene finding) — there must be exactly one tessellation home, and it is `flui-engine`.

**4. Verdict vs current FLUI: MATCHES for the current phase; needs the explicit seam.** Verified this session: `crates/flui-engine/src/wgpu/tessellator.rs` uses lyon (`FillTessellator`/`StrokeTessellator`) — CPU tessellation, the correct current choice. The gap is that there is **no `RasterBackend` trait** abstracting it — the `WgpuPainter` and tessellator are coupled to lyon directly. That is acceptable *today* (one backend) but means the eventual Vello swap is a `flui-engine` refactor rather than an `impl` addition.

**Verdict in one line: keep lyon; do not switch now; but introduce the `RasterBackend` seam during Phase-0 engine hardening so the swap is later non-breaking.** Re-evaluate Vello at FLUI 0.3 when (a) the render pipeline is stable and (b) `vello_hybrid` has closed the WebGL2 coverage gap. The ecosystem doc R4 reaches the identical conclusion.

**5. Risk if adopted wrong.** Two symmetric risks. **Switching to Vello now**: destabilizes `flui-platform` MVP, drops WebGL2/integrated-GPU coverage, bets on an API the ecosystem doc calls "unstable." **Never planning the seam**: lyon's tessellation specifics leak into `flui-engine` internals, and when Vello matures the migration is a multi-month rewrite instead of an `impl RasterBackend for VelloBackend`. The seam is cheap insurance; buy it in Phase 0.

---

## 9. Subsystem 7 — Text layout / shaping / IME

**1. Behavior source.** `painting/text_painter.dart` (`TextPainter` — line breaking, caret metrics, `TextSpan` tree layout) for *layout*; `services/text_input.dart` (`TextInputConnection`/`TextInputClient`, the IME/composing-region protocol) for *input*. Flutter's text behavior — bidi, grapheme-correct caret movement, `StrutStyle`, placeholder spans — is the parity target.

**2. Structural pattern source. Rust-native (the cosmic-text stack) — a deliberate `docs/PORT.md` binding-deletion carve-out. GPUI for the per-platform IME bridge.** Flutter delegates shaping to the C++ engine (HarfBuzz). FLUI cannot — there is no FLUI C++ engine. The Rust-native equivalent is the **cosmic-text stack** (`cosmic-text` = `fontdb` + `rustybuzz` shaping + `swash` rasterization), which the ecosystem doc §11 confirms is the validated choice (used by Iced, Floem, COSMIC desktop). `docs/PORT.md` records this as the canonical carve-out precedent: `PlatformTextSystem` was *deleted*, not ported, because "cosmic-text + glyphon + flui-assets covers the text-shaping responsibility."

For **IME** specifically, the structural source is **GPUI** — it has working IME on all three desktop platforms, and the ecosystem doc R7 names the exact files to consult (`.gpui/src/platform/mac/text_system.rs`, `.gpui/src/platform/windows/direct_write.rs`, `.gpui/src/platform/linux/text_system.rs`). IME *requires* stable widget identity across frames (the input method tracks a cursor) — which the persistent element tree (Subsystem 1) already provides.

*Principle:* **deep module / "different layer, different abstraction."** Text shaping is the canonical deep module — an enormous amount of functionality (bidi, ligatures, font fallback, hinting) behind a small interface (`layout(text, style, constraints) -> laid-out glyphs`). Reimplementing it would be the opposite of deep; *adopting* cosmic-text is correct precisely because it is someone else's deep module. The carve-out is also **information hiding**: deleting `PlatformTextSystem` removes a *shallow* abstraction (a pass-through binding that added nothing over cosmic-text).

**3. Concrete Rust idioms & crates.**
- `cosmic-text` for shaping + layout; `glyphon` for the wgpu glyph-atlas bridge (both already engine deps per CLAUDE.md). `swash` for rasterization.
- `RenderParagraph` (a `RenderBox<Leaf>`-ish render object) as the *render-tree* wrapper over the cosmic-text layout — this is the genuine missing piece (gap matrix §8: `RenderParagraph` absent, blocks the `Text` widget).
- New `flui-platform` capability traits for IME: `PlatformTextInput` (the port-phasing doc §5 names exactly this) — Win32 `WM_IME_*`, macOS `NSTextInputClient`, Wayland `text-input-v3`. Erased as `Arc<dyn PlatformTextInput>` at the platform boundary (consistent with the existing `Platform` trait shape, Subsystem 15).
- `TextInputFormatter`/editing-delta/text-boundary logic is pure-logic and *must* be ported (gap matrix §12) — it cannot be carved out.

**4. Verdict vs current FLUI: MATCHES on stack; IME bridge is a genuine gap.** The cosmic-text stack is in place (`flui-painting`'s `text_layout`/`text_painter` back onto it; `flui-engine` has `glyphon`). The structural choice is correct and the carve-out is documented. The gaps are *missing implementations*, not wrong structure: (a) `RenderParagraph` does not exist; (b) the IME platform bridge in `flui-platform` does not exist (the gap matrix §12 rates `text_input.dart` as XL, "prerequisite for any text field"). Both belong to later roadmap phases (the port-phasing doc puts IME in Phase 5).

**5. Risk if adopted wrong.** The ecosystem doc and the 2025 Rust GUI survey both cite **IME as a user-facing showstopper** (egui's IME bugs drove users away). The risk is shipping a `TextField` widget *before* the IME bridge exists — a text field that cannot do CJK composition is not a text field. Mitigation (ecosystem doc R7, port-phasing doc Phase 5): the `flui-platform` IME hooks must land *before* the `TextField` widget. The reverse risk — trying to write a Rust HarfBuzz — is a multi-year detour; the carve-out correctly forbids it.

---

## 10. Subsystem 8 — The scheduler & frame loop

**1. Behavior source.** `scheduler/binding.dart` (`SchedulerBinding` — the frame phases: transient callbacks → `handleBeginFrame` → `handleDrawFrame` → persistent callbacks → post-frame callbacks; the `Priority` task queue) and `scheduler/ticker.dart` (`Ticker`/`TickerProvider` — vsync-driven animation ticks). Flutter's frame pipeline ordering is the behavior.

**2. Structural pattern source. Flutter for the frame-phase structure; GPUI/winit for the on-demand `ControlFlow::Wait` event-loop integration.** Flutter's phase split is correct and portable directly. The Rust-native structural addition is **on-demand rendering**: the event loop sits in `ControlFlow::Wait` and only schedules a frame when something is dirty — Constitution Principle 7 ("On-demand Rendering … render only when dirty, 60fps target") and `STRATEGY.md` mandate it. GPUI does exactly this (`.gpui/src/platform.rs` — the platform drives frames; the app does not spin). This is the opposite of egui/immediate-mode's render-every-frame loop.

*Principle:* **zero-cost / do-not-redo-work**, applied at the coarsest grain — an idle UI should burn zero CPU. It is also a clean **layer separation**: the *scheduler* owns "when is the next frame," the *platform* owns "wake me on vsync or input," and neither leaks into the other.

**3. Concrete Rust idioms & crates.**
- `winit::ControlFlow::Wait` (winit 0.30.x, the workspace platform dep) as the idle state; `ControlFlow::Poll` only while an animation is running.
- A frame-callback queue keyed by Flutter's `Priority` numeric values (the Cycle-1 audit confirms `flui-scheduler` realigned `Priority` to Flutter's numbers).
- `Ticker`/`TickerProvider` as a trait pair — `TickerProvider::create_ticker` is the seam `flui-animation` plugs into. The Cycle-1 audit notes `Ticker` adopted the `ChangeNotifier::dispose` RAII-dispose template.
- Frame timing as `web_time::Instant` (already used — `web_time` is the WASM-safe `std::time` shim, verified in `scheduler.rs` doc-comment).

**4. Verdict vs current FLUI: MATCHES.** `flui-scheduler` is, per the gap matrix (§5, ~95% coverage) and the port-phasing doc (SOLID, post-Cycle-1), one of the most complete ports in the workspace. Verified this session: `crates/flui-scheduler/src/` has `frame.rs`, `scheduler.rs`, `vsync.rs`; the module doc describes "FrameScheduler (vsync coordination)" and `schedule_frame`/`schedule_frame_callback`. The structure is the Flutter phase model. No change.

**5. Risk if adopted wrong.** The anti-pattern is an immediate-mode spin loop (render unconditionally at 60fps) — wastes battery, contradicts Constitution Principle 7. FLUI did not do this. A subtler risk is *async creeping into the frame path* — `STRATEGY.md` "sync hot path" and `docs/PORT.md` Trigger 3 forbid `async fn` on `build`/`layout`/`paint`; the scheduler is exactly the layer where async is *allowed* (it is an edge), and the discipline is to keep async at the scheduler boundary and never inside a frame.

---

## 11. Subsystem 9 — Gestures / hit-testing / pointer routing

**1. Behavior source.** The whole `gestures/` package — `events.dart` (the `PointerEvent` hierarchy), `hit_test.dart` (`HitTestResult`, `HitTestable`), `arena.dart` (`GestureArenaManager` — the disambiguation arena), `recognizer.dart` + the recognizer catalog (`tap`, `drag`, `long_press`, `scale`, `force_press`, multi-tap), `pointer_router.dart`, `velocity_tracker.dart`. Flutter's gesture-arena disambiguation is intricate and must be ported 1:1.

**2. Structural pattern source. Flutter 1:1.** This is one of the few subsystems where copying Flutter's *structure* as well as its behavior is correct — because the gesture arena is a self-contained algorithmic component with no Dart-specific shape. There is no nullable-pointer-tree or GC-dependent idiom to translate; the arena is plain state machines and a manager. The W3C-event detail is a Rust-native *improvement*: FLUI uses the `ui-events` and `keyboard-types` crates for the pointer/key event types (gap matrix §2) — standardized, cross-platform event vocabulary, better than transliterating Flutter's `PointerData`.

*Principle:* **mental-model legibility** — a Flutter developer's knowledge of the gesture arena transfers *exactly*. And **deep module**: a `GestureRecognizer` is a deep module (complex disambiguation state machine behind a small `addPointer`/arena-member interface). FLUI should keep that depth.

**3. Concrete Rust idioms & crates.**
- Recognizers as state machines — `enum`-based FSM per recognizer (the Cycle-1 audit mentions a recognizer-FSM consolidation; an `enum State` + `match` transition is the idiom).
- `HitTestResult` as a `Vec<HitTestEntry>` accumulator passed `&mut` down the tree — Flutter's exact shape; verified `crates/flui-interaction/src/routing/hit_test.rs:196` `pub struct HitTestResult` and `:487` `pub trait HitTestable` (sealed — `:487` shows `crate::sealed::hit_testable::Sealed` supertrait).
- `ui-events` + `keyboard-types` crates for event vocabulary (already platform deps).
- Events route **up** through the hit-test path, callbacks fire **down** — the standard tree event flow; the ecosystem doc §Cross-cutting-2 endorses the ID-path model (events carry a path, no closure captures mutable state).

**4. Verdict vs current FLUI: MATCHES — the model for the rest of the port.** The gap matrix §2 rates `flui-interaction` at ~95% — "one of the most complete ports in the workspace … the model for what the rest should reach." Verified this session: `hit_test.rs` has `HitTestResult`, a sealed `HitTestable` trait, `hit_test_behavior`. The structure is Flutter's, correctly. The only open work (Cycle-1) is recognizer-FSM *consolidation* — internal hygiene, not a structural redirection. No change.

**5. Risk if adopted wrong.** The gesture arena is subtle — a *behavior* divergence (e.g., resolving the arena one tick early) produces gestures that feel almost-right and frustrate users. The mitigation is the port mandate itself: behavior 1:1 from `.flutter/gestures/`, with Flutter's gesture test suite as the parity oracle. Structurally there is little risk here — the subsystem is already done right.

---

## 12. Subsystem 10 — Animation

**1. Behavior source.** `animation/` — `animation_controller.dart` (`AnimationController` — vsync-driven, the `forward`/`reverse`/`repeat` state), `curves.dart` (the `Curve` catalog), `tween.dart` (`Tween<T>`/`TweenSequence`), `animation.dart` (`Animation<T>` base + `AnimationStatus`). Flutter's animation model: an `Animation<T>` is a `Listenable` producing values in `[0,1]` mapped through a `Curve` and a `Tween`.

**2. Structural pattern source. Flutter, on FLUI's `Listenable` foundation.** The model is portable directly — `Animation<T>` as a `Listenable` is already how Flutter shapes it, and `Listenable`/`ChangeNotifier` is a value-semantics observer that ports cleanly. The Cycle-3 foundation audit confirms `flui-animation` is "the canonical `Listenable` consumer." No Dart-specific structure to redesign.

*Principle:* **zero-cost.** An `AnimationController` ticking drives `mark_dirty` on its element — the `create_mark_dirty_callback()` pattern (`generic.rs:532`, verified this session) hands out an `Arc`-captured closure so a ticker can mark an element dirty *without* `&mut`. That is a lock-free dirty-mark — the right hot-path shape. It is also **deep**: `AnimationController` hides the entire vsync/curve/status machinery behind `.value` + `.addListener`.

**3. Concrete Rust idioms & crates.**
- `flui-animation` re-enabled (it is a 7,475-LOC port, *larger* than its 5,283-LOC Dart source — gap matrix §3 — so it is a thorough port that just needs re-enabling and `Ticker`-integration repair, per the port-phasing doc Phase 2).
- `Curve` as a trait with a `transform(t: f64) -> f64` method + a catalog of ZST/value implementors; cubic-bezier curves as `Cubic { a,b,c,d }` value types.
- `Tween<T>` generic with a `lerp` bound; spring curves draw on the existing `crates/flui-types/src/physics/` (`SpringSimulation` — verified present, gap matrix §4 confirms physics is 100% ported and folded into `flui-types`).
- The `ListenerCallback` / `create_mark_dirty_callback` lock-free dirty-mark seam.

**4. Verdict vs current FLUI: MATCHES (crate disabled — re-enable, not redesign).** `flui-animation` is disabled in the workspace but the gap matrix (§3, ~85% coverage) and port-phasing doc (Phase 2) both classify the work as *re-enable + integration repair*, not greenfield or redesign. The structure — Flutter's `AnimationController`/`Curve`/`Tween` on `Listenable` — is correct. No structural change; the action is workspace re-entry once `flui-scheduler`'s `Ticker` API is frozen (it is, post-Cycle-1).

**5. Risk if adopted wrong.** Low. The risk is *integration drift* — `flui-animation` was disabled while `flui-scheduler` hardened, so re-enabling must re-verify the `Ticker`/`TickerProvider` seam. The port-phasing doc Phase 2 exit test (an `AnimationController` driven by a real `Ticker` producing a 0→1 ramp) catches drift. No structural trap.

---

## 13. Subsystem 11 — Reactivity / state model (DEEP ATTENTION)

This is the brief's first deep-attention item. The ecosystem doc, the architectural-contracts audit (Contract 1), and `STRATEGY.md` all converge — this section makes the convergence a concrete adoption decision.

**1. Behavior source.** `widgets/framework.dart` — `State.setState` (mark the element dirty), `BuildOwner.buildScope` draining the dirty-element list **shallow-first**, and `InheritedWidget` for cross-tree scoped dependency (the `dependOnInheritedWidgetOfExactType` + dependent-set + `updateShouldNotify` machinery). Flutter has **exactly one** state model. There are no signals.

**2. Structural pattern source. Flutter's `setState`/`InheritedWidget` as canonical — with Xilem's `memoize` adopted as the subtree-skip primitive.** This is the synthesis the brief asks for. The reasoning chain:

- **Flutter's `setState` is *behavior*, not implementation.** `STRATEGY.md` "behavior loyal" therefore *requires* it. `STRATEGY.md` is even more explicit — "Not working on: реинвент Flutter widget tree mental model … любая попытка 'сделать лучше через React signals' откатывается к Flutter-семантике." Signals are named and forbidden.
- **The ecosystem has converged on this for Flutter-class frameworks.** The ecosystem doc R2's reasoning: Xilem — the closest architectural analog to FLUI — uses `rebuild()` + `memoize`, *not* signals, and this is Raph Levien's considered position *after* building Druid (observer pattern), Crochet (immediate mode), and Xilem. Floem/Dioxus signals are a different architecture (signals fire bottom-up to specific nodes; Flutter's dirty list is batched and depth-ordered top-down). Mixing them = two invalidation mechanisms, the ecosystem doc's named "maintenance and correctness hazard."
- **But pure `setState` has a real cliff, and Xilem named the fix.** Without subtree-skipping, eagerly building the whole view subtree under a dirty `State` on every rebuild is a performance cliff (ecosystem doc §2, Xilem's `memoize` lesson). Flutter mitigates this internally — `Element.updateChild` does an early `Widget.canUpdate` / identity check and a `const`-constructor short-circuit. FLUI must expose the same: a way for the framework to *skip calling `build()`* when a view's inputs are unchanged.

The decision: **adopt Option A from the ecosystem doc / the architectural-contracts audit — `setState` is the sole canonical model — and adopt Xilem's `memoize` as a concrete primitive, surfaced as `View::can_update`.**

*Principle:* **mental-model legibility is decisive here.** `STRATEGY.md`'s product is "Rust developer who loves Flutter widget style"; the key metric is external contributors finding the model legible. A Flutter developer knows `setState`. They do not know signal subscription graphs, and the Floem debugging model — "something doesn't update? check if it's a closure" (ecosystem doc §6) — is an alien diagnostic. Two state models (Contract 1's Option 2 hybrid) doubles the cognitive surface and contradicts `STRATEGY.md` directly. Also **deep module**: `setState` is a *one-method* interface over the entire rebuild machinery — maximally deep. Signals expose effects, memos, scopes, cleanup — a far larger interface for the same job.

**3. Concrete Rust idioms & crates.**
- **`setState`** = `Element::set_state(|state| ...)` → `core.mark_dirty()`. Verified this session: `crates/flui-view/src/element/unified.rs:407` `pub fn set_state<F>` does exactly `self.core.mark_dirty()` at `:412`. The dirty bit is `Arc<AtomicBool>` (`ElementCore::dirty`) — lock-free, the `docs/PORT.md` Trigger-4 standard.
- **Dirty propagation** = `BuildOwner` with a `BinaryHeap<Reverse<DirtyElement>>` ordered shallow-first — Flutter's depth-sorted dirty list, ported. (Architectural-contracts audit confirms this is already the code.)
- **`memoize` / subtree-skip** = `View::can_update(&self, prev: &Self) -> bool`. Crucially this should be **typed** (`&Self`, not `&dyn View`) — which interlocks with Subsystem 2 (a typed reconciler) and Subsystem 13 (a tuple `ViewSeq` keeps concrete types). `#[derive(PartialEq)]` on a view + a default `can_update` that returns `self == prev` gives the Xilem `memoize` behavior for free. For expensive subtrees, a `Memo<V>` wrapper view that holds a `PartialEq` key and skips re-`build()` when the key is unchanged.
- **`InheritedView`** = the `BuildOwner.inherited_elements: HashMap<TypeId, ElementId>` O(1) registry — verified this session at `build_owner.rs:104` (field), `:443` (insert), `:448` (remove), `:453` (get). This is the `TypeId`-keyed scoped-dependency mechanism — the *one* sanctioned runtime-reflection window (`STRATEGY.md`). It covers ~90% of real signal use cases (theme, locale, media query, auth) — ecosystem doc R2.
- **`flui-reactivity`** = stays **disabled and out of `flui-widgets`' dependency graph**. The architectural-contracts audit Contract 1 Option 1 and the ecosystem doc R2 are unanimous. If signals ever ship, they ship as an *optional application-author* crate that internally drives `Element::mark_dirty` — never a primitive the catalog depends on — and gated by a new refusal trigger (precedent: Trigger 3 for async).

**4. Verdict vs current FLUI: CHANGE — but additive and small.** The `setState`/`InheritedView`/`BuildOwner` machinery is **already built and correct** (verified: `set_state` at `unified.rs:407`, `inherited_elements` registry at `build_owner.rs:104`). What is *missing* is the `memoize` primitive: `View::can_update` today (`view.rs:82`) only does `view_type_id() == old.view_type_id()` — a type check, not a value-equality short-circuit. There is no `Memo` wrapper, and `can_update` takes `&dyn View` (untyped). The change is: (a) make `can_update` able to compare *values* (typed, `&Self`), (b) add a `Memo<V>` combinator, (c) ensure the framework's `updateChild` equivalent actually *calls* `can_update` to skip `build()`. This is additive — no existing widget breaks — and small. It is "CHANGE" only in that the current `can_update` is insufficient for the performance contract.

**The two-sentence reactivity adoption decision (for the orchestrator):** FLUI adopts Flutter's `setState` + `InheritedWidget` as the sole canonical state model and keeps signals (`flui-reactivity`) permanently out of the widget catalog's dependency graph — this is mandated by `STRATEGY.md` ("реинвент … откатывается к Flutter-семантике") and unanimously endorsed by the ecosystem and architectural-contracts research. The one addition is Xilem's `memoize`, surfaced as a *typed* `View::can_update(&self, prev: &Self) -> bool` plus a `Memo<V>` combinator, so the framework can skip `build()` on unchanged subtrees — Flutter's own internal `canUpdate`/`const` short-circuit, which the current `can_update` (a bare type-id check) does not yet provide.

**5. Risk if adopted wrong.** Two named traps. **Adding signals to the catalog** (Contract 1 Option 2/3): two invalidation systems, two mental models, a violation of `STRATEGY.md`, and `flui-reactivity` is a significant runtime surface (effects/memos/scopes) — premature complexity the disabled-crate status correctly defers. **Shipping without `memoize`**: Xilem's documented cliff — a `State` high in the tree whose `build()` returns a large subtree re-runs the *whole* subtree on every `setState`; the catalog (which is benchmarked against Flutter, and Flutter *has* the `const`/`canUpdate` short-circuit) would be slower than its parity target. The deepest trap, ecosystem doc's "most dangerous": adding a `Clone + PartialEq`-style **bound on application state** (the Druid `Data` mistake). FLUI's app state must stay unconstrained `'static`; `setState` is the mutation primitive; hold that line.

---

## 14. Subsystem 12 — `BuildContext` & inherited-data propagation

**1. Behavior source.** `widgets/framework.dart` — `Element implements BuildContext`; `dependOnInheritedWidgetOfExactType<T>()` (register the element as a dependent of the nearest ancestor `InheritedWidget` of type `T`, return its widget); `findAncestorWidgetOfExactType`/`findAncestorStateOfType`; `BuildContext` as the handle threaded into `build()`.

**2. Structural pattern source. Flutter's *semantics*, with a Rust-native callback adaptation, and GPUI's lease pattern for the `&mut`-access case.** Flutter's `dependOnInheritedWidgetOfExactType<T>() -> T?` returns the widget directly — free in Dart (GC, no borrow checker). In Rust, returning `&T` from a context method either needs a lifetime tying the borrow into the rest of `build()` (fights the borrow checker on every call) or a clone (wasteful). FLUI's **callback form** — `depend_on_inherited(&mut dyn FnMut(&dyn Any))` with a typed `BuildContextExt::depend_on::<T,R>(|t| ...) -> Option<R>` blanket sugar — is the correct Rust adaptation: it threads the borrow safely and the architectural-contracts audit Contract 4 explicitly calls this "a good port decision." Keep it.

The GPUI **lease pattern** (`.gpui/src/app/context.rs`) is the structural source for the harder case: when a component genuinely needs `&mut state` *and* `&mut Cx` simultaneously. GPUI temporarily moves the entity state out of `App` onto the stack, runs the callback with both, returns the lease. The ecosystem doc R3 says this is "directly applicable" to `BuildContext`.

*Principle:* **information hiding** — the callback form hides the borrow lifetime *inside* the framework instead of leaking it into every `build()` signature. And **zero-cost**, eventually: the current `ElementBuildContext` holds `Arc<RwLock<ElementTree>>` + `Arc<RwLock<BuildOwner>>` (verified `docs/PORT.md` lock table; flagged "latent friction"), paying a lock acquisition per tree access during build. The endgame (ecosystem doc R3, Refusal Trigger 1's "sync hot path") is `BuildPhase` holding `&mut ElementTree` exclusively, `BuildContext` a view into that exclusive borrow — no runtime lock. That is the lease pattern's payoff.

**3. Concrete Rust idioms & crates.**
- `BuildContext` stays an **object-safe trait** (`&dyn BuildContext` into `build()`) — *no* lifetime parameter on the public surface, so widget code stays clean and matches Flutter's "context is just a handle" feel. The architectural-contracts audit Contract 4 endorses this.
- The callback-form lookup methods + `BuildContextExt` typed blanket sugar (`depend_on`/`get`/`find_ancestor`). `&mut dyn FnMut(&dyn Any)` is the erasure at the callback boundary — acceptable because it does not store `dyn`, it *passes through* it.
- **Drop `Send + Sync`** from `BuildContext` — build is single-threaded (`STRATEGY.md` "sync hot path"); the bound is unnecessary and forces every captured widget field to be `Send + Sync`. Free bound relaxation (Contract 4).
- Internally: the lease pattern — `BuildPhase` owns `&mut ElementTree`; the `Arc<RwLock<...>>` becomes a `flui-view`-internal detail to be removed, *behind* the locked trait surface.

**4. Verdict vs current FLUI: PARTIAL — the trait surface is right, but a correctness hole makes it non-functional.** The callback-form trait + `BuildContextExt` sugar exist and are well-reasoned (architectural-contracts audit Contract 4). The `inherited_elements` O(1) registry exists (verified `build_owner.rs:104`). **But** — verified this session — `StatelessBehavior::perform_build` and `StatefulBehavior::perform_build` construct the context via `ElementBuildContext::new_minimal(core.depth())` (`crates/flui-view/src/element/behavior.rs:222` and `:325`; the constructor at `element_build_context.rs:211`). A *minimal* context is **not wired to the tree**. So during a real `build()`, `ctx.depend_on::<Theme>()` cannot reach the `inherited_elements` registry — it has no tree reference. **The propagation machine is built; the consumption path from inside `build()` is disconnected.**

This is a hard blocker, not a perf nuance: `Theme` is an `InheritedWidget`, and a `Theme`-consuming widget is approximately Material widget #1. The architectural-contracts audit Contract 4(d) calls `new_minimal` "a correctness hole" that "must be closed before any `InheritedWidget`-consuming widget is written." The change: delete the `new_minimal` build path; the fully-wired context must be what reaches `build()`. This belongs to the architecture-correction sibling (Assumption B).

**5. Risk if adopted wrong.** Leaving `new_minimal`: every themed widget silently cannot read its theme — and theming gates the entire Material layer. The structural risk on the *trait surface* is lower (it is already right) but real: if widgets are written against `&dyn BuildContext` and FLUI later adds a `BuildContext<'tree>` lifetime, every `build()` signature changes — so lock the no-lifetime decision now. The reverse trap — exposing inherited lookup as a method returning `&T` — drags a lifetime into `build()` and is why the callback form exists; do not "simplify" to a returning method.

---

## 15. Subsystem 13 — Heterogeneous children / type-erasure boundary (DEEP ATTENTION)

This is the brief's third deep-attention item, the architectural-contracts audit's "the crux" (Contract 3), and the single sharpest Dart↔Rust impedance mismatch in the whole port.

**1. Behavior source.** `widgets/framework.dart` — `MultiChildRenderObjectWidget` with `final List<Widget> children`. The *behavior* is "a multi-child widget holds an ordered, heterogeneous list of child widgets." In Dart `[Text(...), Button(...), Image(...)]` is a `List<Widget>` for free, because every widget *is* a `Widget` and Dart list literals are heterogeneous-by-default (no monomorphization).

**2. Structural pattern source. Xilem's `ViewSequence` (tuple trait) — and this is the ONE subsystem where FLUI must DELIBERATELY NOT copy Flutter's structure.** `Vec<Widget>` is homogeneous-trivial in Dart and *impossible* in Rust — `Vec<T>` requires one `T`. The architectural-contracts audit Contract 3 is unambiguous and I concur fully: the Rust-native answer is a **tuple-based heterogeneous-children trait**, exactly Xilem's `ViewSequence` (also seen in `bevy_ui`, iced's `row!`). A multi-child widget is generic — `struct Column<C: ViewSeq> { children: C }` — and `(A, B, C)` implements `ViewSeq` via a macro for arities `0..=16`. Each child keeps its **concrete type** all the way to the `Slab` boundary; erasure happens **once**, at element creation. A `column![a, b, c]` macro gives the literal call site.

`STRATEGY.md` wrote the "structure Rust-native" clause for *exactly this case*. The mandate is: port the **feel** (`column![Text(...), Button(...), Image(...)]` reads like Flutter's `Column(children: [...])`) on a **Rust-native tuple spine** — not the Dart `List<Widget>` structure.

*Principles, three of them, all pointing the same way:*
- **Compile-time over runtime.** The current `Children` (a `Vec<BoxedView>` where `BoxedView` wraps `Box<dyn View>`) crosses the `dyn` boundary *and `dyn_clone`s every child every frame* (architectural-contracts audit Contract 3(a)). A tuple keeps concrete types — the keyed reconciler (Subsystem 2) and `can_update` (Subsystem 11) can be *monomorphic* per child position. The `dyn` tax is paid once, not per-frame.
- **Mental-model legibility — the decisive one.** `STRATEGY.md`'s success metric is *literally* "external PR contributors" and "sample apps build pass-rate." `children` is the spine of `Column`/`Row`/`Stack`/`Wrap`/`Flex`/`ListView`/`Table` — every real UI. If the catalog ships on builder-only `.child(x).child(y)`, *every* public example reads worse than the Flutter it is copied from. This is the contract the architectural-contracts audit calls the one that decides "whether users praise FLUI."
- **Deep module.** `ViewSeq` is a small interface (one trait, macro-generated) over substantial functionality (heterogeneous typed iteration, monomorphic reconciliation). The `Vec<BoxedView>` approach has a comparably small interface but *shallower* — it throws away the type information the compiler could have used.

**3. Concrete Rust idioms & crates.**
- A **`ViewSeq` trait** (Xilem's `ViewSequence`), macro-`impl`'d for tuples `()` through `(A, …, P)` — arity-16 cap is standard (Rust's own stdlib trait impls cap at 12; 16 is generous and fine).
- A `column! { a, b, c }` / `row! { … }` **declarative macro** expanding to the tuple — the form examples and docs use.
- A blanket `impl<V: View> ViewSeq for Vec<V>` **and** an `impl ViewSeq for Vec<BoxedView>` — the genuinely-dynamic fallback (a `for` loop building rows, dynamic child count). The user opts into boxing *only* where child count is dynamic — precisely where Flutter would also lose its homogeneity benefit. This is the **type-erasure boundary**, stated exactly: erasure is *opt-in*, at the dynamic-list fallback, not the default.
- `Children`/`BoxedView`/`Child` (current — `crates/flui-view/src/child/children.rs`, `view/into_view.rs`) are **demoted** to the dynamic-fallback implementation detail, not the primary surface.

**The type-erasure boundary, stated as a rule (this is the cross-cutting answer the brief asks for):** Concrete types are preserved from `build()` return value down to the `Slab` node. Erasure to a trait object happens at exactly **two** sanctioned points: (1) **element storage** — the `Slab<ElementNode>` holds either `Box<dyn ElementBase>` or, better (architectural-contracts audit Contract 2 Option 2), a closed `enum ElementNode` over the finite element-behavior set; (2) the **dynamic-children fallback** — `Vec<BoxedView>`, opt-in when child count is not statically known. Everywhere else — `View::build`'s return type, `can_update`, tuple `ViewSeq` children, the reconciler's common contiguous path — is concrete and monomorphic. `View::build()` returns `impl IntoView` (the trait *exists* — verified `crates/flui-view/src/view/into_view.rs`, with the blanket `impl<V: View> IntoView for V`), never `Box<dyn View>`.

**4. Verdict vs current FLUI: CHANGE — the current primary surface is the wrong Rust shape.** Verified this session: `VariableChildStorage` (`crates/flui-view/src/element/child_storage.rs:457`) is `Vec<Box<dyn ElementBase>>`; the architectural-contracts audit Contract 3 confirms the *view-side* `Children` is `Vec<BoxedView>` and the only literal path is homogeneous (`vec![Text, Button]` does not compile for mixed types). And — verified — both `StatefulView`/`StatelessView` `build()` still return `Box<dyn View>` (`crates/flui-view/src/view/stateful.rs:116` `fn build(...) -> Box<dyn View>`), *despite* the view.rs and into_view.rs doc-comments having already been updated to *show* `-> impl IntoView`. The docs were updated; the trait surface was not. This is a half-applied change — the catalog must not be built on the `Box<dyn View>`-returning, `Vec<BoxedView>`-storing surface.

The change: introduce `ViewSeq` + `column!`/`row!` macros; change `build()` to return `impl IntoView`; keep `Vec<BoxedView>` only as the dynamic fallback. The architectural-contracts audit Contract 3(f) marks this "MUST LOCK before construction — top priority" and recommends a dedicated `/speckit.plan` — I concur; it interlocks with Subsystem 2 (a tuple spine makes the reconciler partly monomorphic) and Subsystem 11 (typed `can_update`).

**5. Risk if adopted wrong.** If the catalog ships on `Children`/builder-only: every multi-child Material and Cupertino widget bakes in a call site worse than its Flutter twin; every doc example reads worse; `STRATEGY.md`'s two headline adoption metrics (external contributors, sample-app legibility) are suppressed at the source. Changing it after the catalog exists is not a refactor — it is re-typing the `children` field of every multi-child widget and rewriting every example. The opposite risk — over-reaching, e.g. trying to make *dynamic* lists also monomorphic — is a dead end; the `Vec<BoxedView>` fallback is correct and necessary, and the discipline is simply that it is the *fallback*, not the default.

---

## 16. Subsystem 14 — Hot-reload

**1. Behavior source.** Flutter's hot reload — the Dart VM swaps method bodies in place while *preserving* `State` objects and the element tree, then re-runs `build()`. **This specific mechanism is not portable** — a recompiled Rust `cdylib` has all-new type layouts; a `State` struct from the old `.so` cannot be reinterpreted as the new one. So the *behavior* FLUI can match is the weaker hot-*restart* (code updated, state lost), not hot-*reload*.

**2. Structural pattern source. Makepad (hot-reload designed-in from the start) for the architectural discipline; Rust-native `cdylib`-swap for the mechanism.** The ecosystem doc §5 (Makepad) and R6 give the lesson: hot-reload *retrofitted* requires compromises; *designed-in* works. The architectural constraint is that **widget state must not live in the widget struct** if the struct type changes across reload — it must live in the `Element` (keyed by element type + tree position) or an external store. Flutter satisfies this because `State` is owned by the `Element`, not the `Widget` — and FLUI's `State`-owned-by-`Element` design (the `StatefulBehavior<V>` holding state inside the unified `Element`) is the **same correct foundation**.

*Principle:* **information hiding** — the hot-reload boundary is a decision (what is reloadable vs what needs a cold restart) that must be *hidden behind a declared contract*. The ecosystem doc R6 and the architectural-contracts audit Contract 9 both say: write the spec *before* the catalog. The contract: hot-reload-safe = `View::build()` return values, style values, view-tree shape; cold-restart = `State` struct layout changes, crate API changes.

**3. Concrete Rust idioms & crates.**
- `cdylib` + `libloading`/`dlopen`-style symbol loading — verified this session: `crates/flui-hot-reload/src/plugin.rs` has `scene_plugin!` and `app_plugin!` macros using `extern "C"` symbols, and the doc-comment explicitly says "hot restart semantics (code updated, state lost)."
- `State`-owned-by-`Element` — already the design (the unified `Element<V, A, B>` with `StatefulBehavior<V>`). This is the load-bearing correctness property and it is already correct.
- A declared `HotReloadBoundary` doc/contract (not a heavy type) — what reloads, what restarts.

**4. Verdict vs current FLUI: MATCHES at the achievable bar.** `flui-hot-reload` is scene/restart-level (verified — `extern "C"` plugin macros, "hot restart" semantics). The architectural-contracts audit Contract 9 verdict is "CAN EVOLVE — the only contract that genuinely does not need an early lock," and importantly that the widget contract should *not* carry a hot-reload-driven bound now. I concur: do **not** add a `Serialize` bound to `ViewState` to chase state-preserving reload — that taxes the entire catalog (most-implemented trait) for a feature the `cdylib`-swap model makes technically unreachable anyway. The `State`-owned-by-`Element` foundation is correct and forecloses nothing.

**5. Risk if adopted wrong.** The trap (architectural-contracts audit Contract 9(c) Option 2): imposing a `Serialize`/snapshot bound on every `ViewState` to chase partial state-preserving reload — heavy, catalog-wide, and cross-`.so` type identity is still fragile (a field reorder breaks it). The mitigation is to keep hot-restart, keep the `State`-in-`Element` design, and if state-preserving reload is ever prioritized, add a *defaulted, opt-in* `ViewState::snapshot` hook *then*. The only active discipline: do not make `ViewState` structurally un-snapshottable — and it currently is not.

---

## 17. Subsystem 15 — The platform abstraction

**1. Behavior source.** Flutter's `services/` (platform channels, window, input, clipboard, system chrome) — but per `docs/PORT.md`'s binding-deletion carve-out, `services/` is **deliberately dissolved**, not ported. The "behavior" to preserve is the *capabilities* (open a window, receive input, read the clipboard, drive IME), not Flutter's `MethodChannel` structure.

**2. Structural pattern source. GPUI — explicitly and almost entirely.** This is stated outright in CLAUDE.md ("Check GPUI for platform abstraction and Rust-specific patterns") and it is correct. GPUI's platform layer (`.gpui/src/platform.rs`, `.gpui/src/platform/`) is the most battle-tested Rust desktop-platform abstraction: a `Platform` trait, per-OS implementors (Windows/macOS/Linux), `Box<dyn PlatformWindow>` type erasure, a **callback registry** (`on_quit`, `on_window_event`), interior mutability via `Arc<Mutex<T>>` for the inherently-shared platform state. CLAUDE.md's `Platform` trait sketch (`run`/`open_window`/`text_system`/`background_executor`/`clipboard`) is the GPUI shape directly.

*Principle:* **information hiding** — the `Platform` trait is the *one* place OS differences are allowed to exist; everything above (`flui-app` and up) is OS-agnostic. *A Philosophy of Software Design* ch. 5: a module exists to hide a hard, changeable decision — "which OS" is exactly that. **Different layer, different abstraction:** the platform layer's abstraction is "a window and an event stream"; it must not leak Win32 `HWND` or `NSWindow` upward. And `dyn` is *correct here* — Constitution Principle 4 prefers enum dispatch *by default*, but the platform backend is selected once at startup, is genuinely open (a new OS is a new `impl`), and is off the per-frame hot path. `Box<dyn PlatformWindow>` is the right call; this is the sanctioned exception, and `docs/PORT.md`'s multi-source-reference rule explicitly permits drawing structure from GPUI.

**3. Concrete Rust idioms & crates.**
- `Platform` / `PlatformWindow` / `PlatformExecutor` / `PlatformClipboard` traits; `current_platform()` returning the OS-appropriate `impl` (the GPUI/CLAUDE.md shape).
- `Box<dyn PlatformWindow>` / `Arc<dyn PlatformTextSystem>` — `dyn` erasure at the platform boundary, justified above.
- A **callback registry** — `on_window_event(Box<dyn FnMut(...)>)` etc. — GPUI's pattern.
- `winit` 0.30.x as the cross-platform fallback backend; `windows` 0.5x (Win32), `cocoa`/AppKit, Wayland for native backends. `raw-window-handle` 0.6 for the wgpu surface bridge. `ui-events`/`keyboard-types` for the event vocabulary. (All per CLAUDE.md / the gap matrix.)
- New capability traits absorbing the dissolved `services/`: `PlatformTextInput` (IME — see Subsystem 7), `PlatformSystemChrome`, `PlatformHaptics` — the port-phasing doc §5 names these exactly, as `flui-platform` additions, *not* a `flui-services` crate.

**4. Verdict vs current FLUI: MATCHES.** CLAUDE.md documents the `Platform` trait in the GPUI shape; `flui-platform` is in active MVP development with Windows/macOS/Headless backends + a Winit fallback (gap matrix, port-phasing doc). The structure — GPUI-pattern traits, `dyn` erasure, callback registry — is correct. The `services` carve-out (dissolve into `flui-platform` + `flui-assets`, no `flui-services` crate) is the right call and matches Assumption A. The work remaining is *breadth* (mobile backends, IME) — capability gaps, not structural redirection.

**5. Risk if adopted wrong.** The carve-out itself carries a risk the `docs/PORT.md` rule bounds: dissolving `services/` is correct *only* where a Rust crate stack genuinely owns the responsibility end-to-end (the `PlatformTextSystem`/cosmic-text precedent). The capabilities that have *no* Rust-native owner — IME above all — must not be dissolved into nothing; they become explicit `flui-platform` traits. The risk is forgetting one (IME) and discovering at `TextField`-time that it was carved out into a void. The port-phasing doc Phase 5 and Subsystem 7 above guard against this.

---

## 18. Subsystem 16 — The asset pipeline

**1. Behavior source.** `painting/image_provider.dart` (`ImageProvider`, `ImageStream`, `ImageCache` — the resolve→decode→cache chain, keyed by an `ImageConfiguration`) and `services/asset_bundle.dart` (`AssetBundle` — bundled asset loading, `AssetManifest`). Flutter's image-resolution behavior (provider resolves a key, the cache dedups, the stream delivers frames) is the parity target.

**2. Structural pattern source. Flutter's `ImageProvider`/`ImageCache` model for the structure; Rust-native async IO for the mechanism.** The `ImageProvider` chain — a provider abstraction that resolves to a cache-keyed stream — is a sound design and portable directly. The Rust-native part is the *IO mechanism*: Flutter's image loading is `Future`-based on the Dart event loop; FLUI's is `tokio` async IO in `flui-assets` (the gap matrix lists `flui-assets` deps as `tokio fs`, `reqwest`, `moka`, `image`).

*Principle:* **"Sync hot path, async at edges"** (`STRATEGY.md`; `docs/PORT.md` Trigger 3). Asset IO is the textbook *edge* — `docs/PORT.md` explicitly permits `async fn` in `flui-assets`. The structural rule: async lives entirely in `flui-assets`; the *render* path consumes only already-resolved, already-uploaded textures. The asset pipeline hands the engine a decoded buffer; it never makes `paint` await. **Information hiding:** `ImageProvider` hides "network vs asset vs file vs memory" behind one resolve interface.

**3. Concrete Rust idioms & crates.**
- `flui-assets` re-enabled (currently disabled; the gap matrix and port-phasing doc Phase 3 classify it as re-enable + wiring, not greenfield — it is a 4,607-LOC crate already).
- `image` crate for decode; `tokio` for async file/network IO; `moka` for the bounded async cache (`ImageCache` equivalent); `reqwest` for network images.
- An `ImageProvider` **trait** with concrete implementors (`AssetImage`, `NetworkImage`, `FileImage`, `MemoryImage`) — Flutter's exact taxonomy.
- The engine seam: `flui-assets` produces a decoded `image` buffer; `flui-engine`'s texture pool (`crates/flui-engine/src/wgpu/texture_pool.rs` — verified present) uploads it. `flui-assets` supplies font *bytes* only — shaping stays with cosmic-text (the Subsystem 7 carve-out).

**4. Verdict vs current FLUI: PARTIAL — structure sound, disabled, needs wiring.** `flui-assets` is disabled; `flui-engine` declares an *optional* edge to it (port-phasing doc §2.2 — the edge exists in `cargo metadata`, the crate is commented out of `[workspace.members]`). The `ImageProvider`/cache structure is the right Flutter port. The gap is integration: re-enable `flui-assets`, wire decoded buffers into `flui-engine`'s texture pool, settle the `AssetBundle` abstraction. The port-phasing doc Phase 3 ties this to the `Image` widget (an `Image` widget needs `flui-assets` + a `RenderImage` render object). No structural change — re-enable and connect.

**5. Risk if adopted wrong.** The trap is letting async leak inward — an `ImageProvider` that an `Image` *widget* awaits during `build()`, or worse during `paint`, violates `docs/PORT.md` Trigger 3 and the sync-hot-path clause. The correct shape: the `Image` widget holds an `ImageProvider`, kicks off resolution asynchronously, and rebuilds (via `setState`/a listener) when the frame arrives — exactly Flutter's `ImageStream` + `setState` pattern. The async stays in `flui-assets`; the widget sees only "no image yet" → "image ready" as ordinary state transitions.

---

## 19. Cross-cutting idiom decisions

The brief asks for explicit cross-cutting calls. These apply across all subsystems.

### 19.1 The type-erasure boundary (the single most important cross-cutting rule)

Stated once, applies everywhere. **Concrete types are preserved from `View::build()`'s return value down to the `Slab` node. `dyn` erasure happens at exactly two sanctioned points, and nowhere else:**

1. **Element storage** — the `Slab<ElementNode>`. Today `Box<dyn ElementBase>`; the architectural-contracts audit Contract 2 Option 2 (which I endorse) is to make this a **closed `enum ElementNode`** over the finite element-behavior set (Stateless/Stateful/Proxy/Inherited/Render/Animation — a known, `#[non_exhaustive]`-managed set). An enum lets reconciliation `match` instead of vtable-dispatch, and *eliminates* the runtime `downcast_ref::<V>()` in the update path (`generic.rs` — the failing-downcast that should be impossible). This is element-storage *representation* — internal, can evolve behind the locked `View` trait, but should be done before the catalog (trivial at 0 widgets).
2. **The dynamic-children fallback** — `Vec<BoxedView>`, opt-in, used only when child count is not statically known (Subsystem 13).

Everywhere else is concrete and monomorphic: `build()` returns `impl IntoView`; `can_update` is typed `&Self`; tuple `ViewSeq` children keep concrete types; the keyed reconciler's common contiguous path is monomorphic. The `View` *trait* stays object-safe (it must — `create_element` and the children machinery need it) but `Downcast`/`DynClone` should leave the *public bound surface* where possible. *Rationale:* Constitution Principle 4 ("generics and enum dispatch over `dyn` by default"), `STRATEGY.md` "compile-time over runtime." `dyn` is permitted only where the set is genuinely open *and* off the hot path — the platform backend (Subsystem 15) is the clean example; the reconciliation path is not.

### 19.2 Error model

**`Result<T, E>` + `thiserror` for library crates; `anyhow` only at application/binary edges; `build()` stays infallible.** `STRATEGY.md` "`Result<T, E>` + `thiserror` вместо exceptions"; CLAUDE.md Principle 6 (no `unwrap()`/`panic!` in library code). Each crate defines its own `#[non_exhaustive]` error enum via `thiserror` (`RenderError`, `TessellationError` — verified `#[non_exhaustive]` at `tessellator.rs`). The one nuance, from the architectural-contracts audit Contract 8 which I concur with: **`View::build()` is infallible** — it returns `impl IntoView`, never `Result`. Forcing `Result` on the most-written method in the framework taxes every widget for a rare case and breaks Flutter-parity feel (Flutter's `build()` cannot return an error either). The Flutter *behavior* — a failed widget is replaced by an error widget, the tree survives — is achieved with an internal `std::panic::catch_unwind` boundary around `perform_build` substituting an `ErrorView` (the `ERROR_VIEW_BUILDER` infrastructure already exists, `error.rs`). A deliberate, documented framework-level panic boundary is *not* the sloppy-`unwrap()` Principle 6 forbids — it is the standard Rust pattern for exactly this, used by every Rust UI framework to contain widget panics. Lock `build()` infallible now; the `catch_unwind` placement is internal and can land later.

### 19.3 Async edges

**Async is confined to three named edges; the render hot path is strictly synchronous.** `STRATEGY.md` "Sync hot path, async на краях"; `docs/PORT.md` Trigger 3 (`async fn` forbidden on `build`/`layout`/`paint`/`perform_layout`/`composite`). The permitted edges: **IO** (`flui-assets` — image/font loading, Subsystem 18), **the scheduler** (`flui-scheduler` — frame coordination, Subsystem 8), **the build pipeline** (`flui-build` — the offline tool). One whitelisted exception per `docs/PORT.md`: route-notification handlers in `flui-view/src/binding.rs` (they sit on the binding layer, mirror Flutter's `SystemChannels` async callbacks, not the render path). Rule of thumb: async may *deliver work to* a frame (an asset finishes loading → mark dirty → next frame uses it) but may never run *inside* a frame.

### 19.4 The `memoize` primitive — concrete shape

The reactivity decision (Subsystem 11) names `memoize`; here is its concrete cross-cutting form. **`View::can_update(&self, prev: &Self) -> bool`, typed, defaulting to `PartialEq` value-equality; plus a `Memo<V>` combinator view.** This is Flutter's internal `Widget.canUpdate` + `const`-constructor short-circuit, surfaced. Three concrete pieces:

- `can_update` becomes **typed** — `&Self` not `&dyn View` — so it interlocks with the typed reconciler (Subsystem 2) and tuple children (Subsystem 13). The current `view.rs:82` `can_update(&self, old: &dyn View)` doing only `view_type_id()` equality is insufficient: it gates *element reuse* but cannot gate *`build()` skipping*.
- A widget that `#[derive(PartialEq)]`s gets the Xilem `memoize` behavior from a default `can_update` returning `self == prev` — zero boilerplate for the common case.
- A `Memo<V: View + PartialEq>` wrapper view for expensive subtrees: holds the view + a cached element; when re-`build()` produces a `Memo` whose inner view `== prev`, the framework skips descending. This is Xilem's `memoize` as a combinator.

*Why it is cross-cutting and not just Subsystem 11:* without it, *every* subsystem that calls `build()` on a subtree (reconciliation, animation-driven rebuilds, inherited-data notification) pays the full subtree cost. `memoize` is the one primitive that bounds rebuild cost framework-wide. It must exist from the start (ecosystem doc R2: "must be implemented from the start") — retrofitting it means re-auditing every widget's rebuild behavior.

---

## 20. Where current FLUI must change direction — ranked

Ranked by blast radius — how much of the catalog inherits the defect if the change is not made first. Items 1-4 are the "CHANGE" verdicts from the master table; 5-6 are "Partial" verdicts with a hard blocker.

**1. Heterogeneous children — replace `Children`/`BoxedView` with a `ViewSeq` tuple trait (Subsystem 13).** *Blast radius: maximum.* `children` is in `Column`/`Row`/`Stack`/`Wrap`/`Flex`/`ListView`/`Table` — every multi-child widget in `flui-widgets`, Material, and Cupertino. Ship the catalog on builder-only `Children` and every multi-child widget bakes in a call site worse than its Flutter twin, suppressing `STRATEGY.md`'s two headline adoption metrics at the source. This is also the change that most needs its own `/speckit.plan` (interlocks with #2 and the `memoize` primitive). *Also fix here:* `build()` must return `impl IntoView`, not `Box<dyn View>` — the trait was documented for this (`view.rs`/`into_view.rs` doc-comments) but not yet changed (`stateful.rs:116` still returns `Box<dyn View>`).

**2. Widget→Element reconciliation — wire the keyed reconciler, delete the index path (Subsystem 2).** *Blast radius: high, and it is a silent-correctness trap.* The live `update_with_views` (`child_storage.rs:494`) is index-only; the keyed `reconcile_children` (`reconciliation.rs:51`) has zero production callers. Static lists reconcile fine positionally, so demos pass — state loss surfaces only on reorder, in user apps, under every list/grid/table widget. The fix is *finishing* (the algorithm is written) + adding a general `key` field to `ElementNode`. Must precede the catalog; cheaper to validate with 0 list widgets.

**3. Reactivity — add the `memoize`/`can_update` short-circuit; keep signals out (Subsystem 11, §19.4).** *Blast radius: high (performance), additive.* `setState`/`InheritedView`/`BuildOwner` are built and correct — the change is purely *adding* the typed `View::can_update` value-equality short-circuit and a `Memo<V>` combinator, because the current `can_update` (a bare type-id check) cannot skip `build()`. Without it, every `setState` high in the tree re-runs its whole subtree — the Xilem-documented cliff, and the catalog is benchmarked against a Flutter that *has* the `const`/`canUpdate` short-circuit. Additive, no widget breaks — but must exist before the catalog or every widget's rebuild behavior needs re-auditing.

**4. Layer/compositor lifecycle — add the retained-layer protocol (Subsystem 5).** *Blast radius: high (performance), every app.* No `LayerHandle` RAII, no `needs_add_to_scene` dirty bit, no `engine_layer` retention — so every frame rebuilds the GPU layer tree from scratch and retained rendering is lost. Not a widget-*correctness* blocker (widgets render, slowly) but it makes FLUI immediate-mode-in-practice, collapsing the three-tree's performance argument. Repair plan `2026-05-22-004` already scopes it; also box the 360-byte `Layer` enum.

**5. `BuildContext` — close the `new_minimal` correctness hole (Subsystem 12).** *Blast radius: gates Material widget #1.* During a real `build()`, the context is constructed via `ElementBuildContext::new_minimal` (`behavior.rs:222`, `:325`) — *not* tree-wired — so `ctx.depend_on::<Theme>()` cannot reach the `inherited_elements` registry. Theming is non-functional until this is fixed, and `Theme` is an `InheritedWidget` needed by approximately the first Material widget. The trait surface is right; this is a wiring fix (delete the `new_minimal` build path).

**6. Engine — introduce the `RasterBackend` seam (Subsystem 6).** *Blast radius: low now, high later.* lyon is the correct current choice — do not switch. But there is no `RasterBackend` trait abstracting it, so the eventual Vello migration is a `flui-engine` rewrite instead of an `impl` addition. Introduce the seam during Phase-0 engine hardening; cheap insurance, makes the lyon→Vello swap non-breaking. Lowest urgency on this list — it is forward-insurance, not a current defect.

**Not on this list — explicitly validated as correct, do not touch:** the three-tree Slab-arena ownership (Subsystem 1), the Flutter constraint protocol + arity system (Subsystem 3), the `DisplayList`/`DrawCommand` record-replay seam (Subsystem 4), the cosmic-text stack choice (Subsystem 7), the scheduler (Subsystem 8), `flui-interaction`'s gesture port (Subsystem 9), the animation model (Subsystem 10), the GPUI-shaped platform abstraction (Subsystem 15). These match; the discipline is to *hold the line* against well-meaning redesigns (the recurring temptations: GPUI's drop-tree-per-frame, Taffy for layout, signals for state — each rejected with a named source above).

---

## 21. Sources

| Source | Type | Used for |
|---|---|---|
| `.flutter/flutter-master/packages/flutter/lib/src/` | Reference code | Behavior source for every subsystem (named per-section) |
| `.gpui/src/platform.rs`, `.gpui/src/platform/`, `.gpui/src/app/context.rs`, `.gpui/src/element.rs` | Reference code | Structural source: platform abstraction, lease pattern, element-model anti-lesson |
| `docs/research/2026-05-22-rust-ui-ecosystem-lessons.md` | Prior research | Xilem/GPUI/Floem/Druid/Vello structural lessons; the reactivity decision; the lyon/Vello tradeoff |
| `docs/research/2026-05-22-architectural-contracts.md` | Prior research | Contracts 1-9 — the public-surface decisions; verdicts cross-checked |
| `docs/research/2026-05-22-port-phasing-dependency-order.md` | Prior research | Crate DAG, critical path, disabled-crate re-entry, new-crate placement |
| `docs/research/2026-05-22-flutter-flui-gap-matrix.md` | Prior research | Subsystem-level coverage %, what exists vs missing |
| `STRATEGY.md` | Governance | "Behavior loyal, structure Rust-native"; "compile-time over runtime"; "sync hot path"; the product = DX, metric = external-contributor legibility |
| `docs/PORT.md` | Governance | Refusal triggers; lock-decision table; binding-deletion carve-out; Flutter-behaviour-primacy |
| Ousterhout, *A Philosophy of Software Design* | Book | Deep vs shallow modules; information hiding; "different layer, different abstraction"; pass-through anti-pattern |
| Blandy/Orendorff/Tisdale, *Programming Rust* 2e | Book | Traits vs generics; typestate ("states in types"); zero-sized marker types |
| Gjengset, *Rust for Rustaceans* | Book | Sealed traits; RAII guards; `?Sized` discipline |
| *The Rust Performance Book* | Book | Niche optimization; "do not redo work"; large-enum-variant boxing |
| Rust API Guidelines | Reference | `#[non_exhaustive]`, `#[must_use]`, object-safety discipline |
| **Verified this session** (`file:line` cited inline) | Current code | `view.rs`, `stateful.rs`, `into_view.rs`, `child_storage.rs`, `reconciliation.rs`, `behavior.rs`, `element_build_context.rs`, `build_owner.rs`, `unified.rs`/`generic.rs`, `commands.rs`/`traits.rs`, `tessellator.rs`, `flui-engine`/`flui-layer`/`flui-rendering` directory structure |
