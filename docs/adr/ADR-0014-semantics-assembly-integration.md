# ADR-0014: The Semantics render-object family is unblocked by porting the CLASSIC full-rebuild assembly into the EXISTING post-paint `run_semantics` phase — the `SemanticsOwner` gets a home on `PipelineOwner`, geometry reuses the paint walk's `offset()`/`paint_transform()` inputs, and the modern `_RenderObjectSemantics` incremental compiler plus the OS accessibility bridge are explicitly deferred

*The three target render objects (`RenderSemanticsAnnotations`/`RenderMergeSemantics`/`RenderExcludeSemantics`) are, in Flutter, thin `describeSemanticsConfiguration` describers — the hard part is the **tree-assembly machinery** they need underneath. That machinery is **90% already present in FLUI, split across `flui-semantics` and `flui-rendering`**: the `SemanticsConfiguration::absorb` merge primitive, `SemanticsNode`/`SemanticsTree`/`SemanticsOwner`, the post-paint `PipelineOwner<Semantics>::run_semantics` phase, the `DirtyKind::Semantics` dirty-tracking, and the `describe_semantics_configuration` hook all exist and are tested. What is missing is exactly **one seam**: `run_semantics()` is a `tracing::warn!` no-op stub — nothing walks the render tree, calls the hook, merges configs into `SemanticsNode`s, and populates a `SemanticsOwner`. This ADR decides that seam: **port Flutter's CLASSIC full-rebuild assembly** (behaviorally loyal to the observable SemanticsNode tree), **NOT** the modern `_RenderObjectSemantics` incremental compiler; give the `SemanticsOwner` a home as an `Option<SemanticsOwner>` field on `PipelineOwner` (Flutter-faithful, and the `on_semantics_owner_created`/`disposed` notifier hooks are already scaffolded for it); reuse the paint walk's geometry inputs (`RenderNode::offset()` + `paint_transform()`) rather than inventing a transform-accumulation mechanism; and close two small additive gaps (one config flag, one exclude hook). No new crate dependency, no phase restructuring.*

---

- **Status:** Proposed — chief-architect ARCH-GATE: **ACCEPTABLE** (integration + scope decision only; the three render objects are separate DEV tasks and must be DoD-cross-checked against `.flutter/flutter-master/packages/flutter/lib/src/rendering/proxy_box.dart` + the assembly walk against `object.dart`).
- **Date:** 2026-07-01
- **Deciders:** chief-architect; consult api-design-lead (the new `Option<SemanticsOwner>` accessor surface on `PipelineOwner`, the additive `is_merging_semantics_of_descendants` config field, and the new `excludes_semantics_subtree`/`visit_children_for_semantics` hook shape on `RenderBox`/`RenderSliver`), qa-lead (the harness proof that a real render tree produces the correct `SemanticsNode` tree), and product-steward (confirming the OS accessibility bridge stays out of this milestone's scope).
- **Relates to:** the render-object catalog completion epic (71/~74 → the Semantics family is the last gap). Sibling in spirit to ADR-0013 / ADR-0011 / ADR-0012: **close a gap by wiring existing machinery end-to-end for the first time, not by inventing a parallel subsystem.** Supersedes the deferral markers in `pipeline/owner/semantics.rs:84-90` and `binding/mod.rs:378` / `:461-466`.
- **Gate:** ARCH-GATE (this doc) → then per-slice DEV-GATEs.

---

## Context

### What the three render objects actually need (oracle: `rendering/proxy_box.dart`)

All three are `RenderProxyBox` subclasses that do almost nothing themselves — they configure semantics and delegate the real work to the assembly machinery:

- **`RenderMergeSemantics`** (`proxy_box.dart:4379-4390`) overrides `describeSemanticsConfiguration` to set exactly two bits: `config..isSemanticBoundary = true ..isMergingSemanticsOfDescendants = true`. Its entire behavior is "become a boundary that collapses its whole subtree into a single semantics leaf."
- **`RenderExcludeSemantics`** (`proxy_box.dart:4399-4429`) holds `excluding: bool` and overrides **`visitChildrenForSemantics`** (`:4416-4422`) — when excluding, it visits **no** children, dropping the subtree from the semantics walk. Its setter calls `markNeedsSemanticsUpdate()` (`:4413`).
- **`RenderSemanticsAnnotations`** (`proxy_box.dart:4309-4333`) is the substantial one: it mixes in `SemanticsAnnotationsMixin` and calls `initSemanticsAnnotations(properties, container, explicitChildNodes, excludeSemantics, blockUserActions, localeForSubtree, textDirection)`. The mixin's `describeSemanticsConfiguration` (`object.dart:5378` region) writes a full `SemanticsProperties` payload into the config. This backs the `Semantics(...)` widget.

**The load-bearing conclusion:** none of the three needs bespoke design. Each is a `describe_semantics_configuration` override (plus, for exclude, a children-visitor hook). They are a **fast follow-on** the moment the assembly machinery underneath them exists — which is what this ADR is about.

### The oracle's assembly hooks (object.dart)

Flutter's `RenderObject` exposes exactly the seam FLUI already mirrors:

- `describeSemanticsConfiguration(config)` — the per-object hook, default "nothing to do" (`object.dart:3835-3837`). **FLUI parity: exact** — `RenderObject::describe_semantics_configuration(&self, &mut SemanticsConfiguration)`, default no-op (`traits/render_object.rs:539`), forwarded from `RenderBox` (`traits/render_box.rs:445`) and `RenderSliver` (`traits/render_sliver.rs:447`).
- `assembleSemanticsNode(node, config, children)` — default `node.updateWith(config, childrenInInversePaintOrder)` (`object.dart:3946-3953`). **FLUI parity: `SemanticsNode::absorb` + `set_config` + `add_child` are the primitives; no per-object override hook exists yet (only `RenderCustomPaint`-style objects need one, out of scope).**
- `visitChildrenForSemantics(visitor)` — default walks all children (`object.dart:3928-3930`); `RenderExcludeSemantics` overrides it. **FLUI parity: NONE — `rg visit_children_for_semantics` is empty across the workspace.** This is one of the two additive gaps (see D5).
- `markNeedsSemanticsUpdate()` (`object.dart:3909-3916`) → enqueues onto `owner._nodesNeedingSemanticsUpdate`. **FLUI parity: exact** — `DirtyKind::Semantics`, `RenderState.flags().mark_needs_semantics()` (`storage/flags.rs:790`), routed via the scheduler's semantics dirty queue (`pipeline/scheduler.rs:313`, `add_node_needing_semantics`).

### The oracle's assembly *engine* — and the choice it forces

Behind those hooks, current Flutter master runs the **modern `_RenderObjectSemantics` compiler** (`object.dart:5525+`): `_SemanticsFragment`/`_IncompleteSemanticsFragment` (`:5432`, `:5451`), `mergeUp` / `siblingMergeGroups` lists (`:5573`, `:5580`), `computeAncestorInfo`, `parentDataDirty`/`geometryDirty` incremental tracking, `firstAncestorNodeWithCleanGeometry` (`:1580`), and `_producedSiblingNodesAndOwners` (`:5581`). This is a ~1000-line incremental machine with heavy nullable per-node state — a 2024+ rewrite of Flutter's earlier semantics compiler.

**This is the pivotal scoping fact, and it corrects the framing of the prior scoping pass.** The prior pass pointed at "`RenderObject._updateSemantics`/`SemanticsConfiguration` merge algorithm" as if it were one algorithm to port. In current master it is the *modern incremental compiler*. Porting that 1:1 would be a multi-slice effort in its own right, and would import a nullable-field state machine that is precisely the kind of shape AGENTS.md rule #2 says to reconsider rather than transcribe.

But FLUI's `flui-semantics` data model already implements the **classic** model's merge primitive:

- `SemanticsConfiguration::absorb` (`configuration.rs:886-961`) is a **Flutter-faithful, U16-verified** merge: flag union, blocked/unblocked action-mask filtering (`UNBLOCKED_USER_ACTIONS_MASK`), custom-action concatenation, tag set-merge, **text-direction-aware label/hint concatenation**, first-wins for value/tooltip/sort_key, and role merge. Its doc even cites `.flutter/…/semantics.dart:6790-6862`.
- `SemanticsNode::absorb` (`node.rs:309-320`) does the node-level merge: config `absorb` + rect union.
- `SemanticsNode::is_semantics_boundary()` (`node.rs:260-262`) = `config.is_semantics_boundary() || has_content()`.

That is exactly the classic assembly's building blocks. The classic model — walk from a boundary, call `describe_semantics_configuration` per node, non-boundary content merges *up* into the nearest boundary's node via `absorb`, boundary children spawn their own nodes — produces the **same observable `SemanticsNode` tree** as the modern compiler for the common path, differing only on the modern machine's incremental/optimization/edge-case surface (sibling merge groups, block-previous-sibling, geometry-dirty incrementalism, cross-frame node-id stability).

### The verified state of the pipeline (flui-rendering)

- `flui-rendering` **already depends on `flui-semantics`** (`Cargo.toml:21`) and re-exports it as `pub use flui_semantics as semantics` (`lib.rs:79`). The crate edge exists; no manifest change is needed to reach the data model.
- The **Semantics phase already exists as a genuinely separate, post-paint pass**: the typestate walk is `… → PaintPhase::run_paint → PaintPhase::into_semantics() (paint.rs:30) → Semantics::run_semantics() → Semantics::finish()` (`pipeline/owner/semantics.rs:17,31`). This matches Flutter's `PipelineOwner.flushSemantics` running after `flushPaint`. The phase is even documented to sort **shallow-first** so parents assemble before children fold in (`semantics.rs:59`, `scheduler.rs:562`).
- The dirty-tracking is wired end-to-end: `DirtyKind::Semantics` drains through `drain_pending_dirty` → `add_node_needing_semantics` (`accessors.rs:135`), the queue is `dirty.needs_semantics` (`dirty.rs:200`), and semantics marks fire the visual-update wake exactly once per new entry, Flutter-parity (`scheduler.rs:778-799`).
- **The one missing seam:** `run_semantics()` (`semantics.rs:31-102`) counts pending nodes, emits `tracing::warn!("full SemanticsOwner integration pending; semantics config build … is a no-op until RenderObject → SemanticsConfiguration plumbing lands")` (`:84-90`), and clears the queue. **No `RenderObject` anywhere overrides `describe_semantics_configuration`** (`rg` over `flui-objects/src` is empty), and `PipelineOwner` holds **no `SemanticsOwner`/`SemanticsTree`** — its fields (`mod.rs:104-203`) are `render_tree`, `root_id`, `notifier`, `scheduler`, `semantics_enabled: AtomicBool`, `last_layer_tree`, … but no semantics tree. The assembly has no home and no body.

### What is genuinely missing (the whole decision)

1. **A body for `run_semantics`** — the assembly walk that turns the render tree + `describe_semantics_configuration` into a merged `SemanticsNode` tree.
2. **A home for the `SemanticsOwner`** — `PipelineOwner` has none.
3. **One children-visitor hook** — `visit_children_for_semantics`-equivalent for `RenderExcludeSemantics` (absent).
4. **One config field** — `is_merging_semantics_of_descendants` for `RenderMergeSemantics` (absent from `configuration.rs`; the `SemanticsFlags` bitset at `flags.rs:18-104` has no merge flag either).

Everything else — the merge algebra, the phase, the dirty tracking, the hook, the crate edge, the enable/disable + `SemanticsBinding` platform plumbing (`flui-app/src/bindings/renderer_binding.rs:238-249`, `flui-semantics/src/binding.rs`) — already exists.

---

## Decision

**Wire the existing machinery end-to-end by porting the CLASSIC full-rebuild assembly into the existing `run_semantics` phase.** Give the `SemanticsOwner` a home on `PipelineOwner`, reuse the paint walk's geometry inputs, and close the two small additive gaps. No new crate dependency, no phase restructuring, no async on any hot path.

### D1 — The `SemanticsOwner` lives on `PipelineOwner` as `Option<SemanticsOwner>`, lazily created (Flutter `ensureSemantics` parity)

Add one field to `PipelineOwner` (`pipeline/owner/mod.rs:104`): `semantics_owner: Option<SemanticsOwner>`. It is `None` until semantics is enabled (a `SemanticsHandle` is requested via the already-present `SemanticsBinding::ensure_semantics`, `binding.rs:205`), then created — firing the **already-scaffolded** `notifier.fire_semantics_owner_created()` (`accessors.rs:1162`), and disposed (firing `fire_semantics_owner_disposed()`, `:1164`) when the last handle drops. Those two notifier callbacks (`accessors.rs:45-57`, `notifier.rs:86-114`) exist today with **no field to fire about** — they were built in anticipation of exactly this field.

- This is the Flutter-faithful home: `PipelineOwner._semanticsOwner`, created by `ensureSemantics()`, is where `flushSemantics` builds. `RenderView` already holds a `Weak<RwLock<PipelineOwner>>` (`view/render_view.rs:54`), so the owner is reachable for `perform_semantics_action` dispatch (closing the `binding/mod.rs:378` stub) and for `debug_dump_semantics_tree` (closing `binding/mod.rs:461-466`).
- The field must **not** leak a lock in the public API (SP-6 / port-check): `SemanticsOwner` is stored behind the owner's existing single-writer discipline (`Arc<RwLock<PipelineOwner>>` at the app layer), accessed only through methods, never returned as a guard.

### D2 — Assembly runs in the **existing** `run_semantics` phase; no phase restructuring

The body of `run_semantics` (`semantics.rs:31`) changes from the warn-stub to a real assembly pass; **the phase machinery, typestate walk, dirty queue, and shallow-first sort are untouched.** Because FLUI's pipeline already runs semantics as a separate phase *after* paint (`into_semantics()`, `paint.rs:30`), the answer to "does semantics need a genuinely separate pass?" is **it already is one** — matching `flushSemantics` after `flushPaint`. Nothing about the single-pass-per-frame layout/paint shape needs to change.

For the minimal slice the assembly is a **full rebuild each semantics frame**: when the dirty queue is non-empty, clear the `SemanticsOwner`'s tree and re-walk from the root, then `flush()` (the owner's existing dirty-driven platform push, `owner.rs:335`). The dirty queue thus serves as a coarse "a rebuild is needed this frame" trigger rather than a fine-grained incremental map. This is an honest simplification (see D6/Consequences), correct-but-not-optimal, and it is the smallest thing that produces a correct tree.

### D3 — Port the CLASSIC merge model, NOT the modern `_RenderObjectSemantics` compiler

The assembly walk is a depth-first traversal from the root boundary, structurally a **sibling of `paint_subtree`** (`paint.rs:144`) — same tree, same child-order, but emitting `SemanticsNode`s instead of layers:

1. At each render node, build a fresh `SemanticsConfiguration` and call `describe_semantics_configuration` (the erased `RenderObject` hook).
2. **Boundary decision:** if the node forms its own semantics node (`config.is_semantics_boundary()`, or the classic "has content" rule refined per below), it spawns a `SemanticsNode`; otherwise its config is **merged up** into the nearest boundary ancestor's node via `SemanticsConfiguration::absorb` / `SemanticsNode::absorb` (`configuration.rs:886`, `node.rs:309`).
3. **Merge-descendants (`RenderMergeSemantics`):** when `config.is_merging_semantics_of_descendants()` is set, the whole subtree collapses into this one node — every descendant config is absorbed and no descendant spawns its own node.
4. **Exclude (`RenderExcludeSemantics`):** the walk consults the children-visitor hook (D5) and skips the excluded subtree entirely.

The walk is loyal to the **observable output** (the assembled `SemanticsNode` tree) for the common path, which is what rule #1 protects. It deliberately does **not** reproduce the modern compiler's internal `_SemanticsFragment`/`mergeUp`/`siblingMergeGroups` state machine — those are an incremental-performance + edge-case layer, deferred in D6.

**One correctness refinement to flag for the DEV task:** the current `SemanticsNode::is_semantics_boundary()` conflates "is a boundary" with "has content" (`node.rs:260-262`). The classic model distinguishes *forms a node* (boundary) from *has content to merge up* (non-boundary). For the trivial single-leaf case they coincide; for a `MergeSemantics` subtree they do not. The assembly walk must implement the correct boundary-vs-merge decision and may need to refine that predicate — this is behavior to DoD-cross-check against `object.dart`, not a data-model change.

### D4 — Geometry reuses the paint walk's inputs (`offset()` + `paint_transform()`); NO new transform mechanism

The semantics geometry (`SemanticsNode::set_rect` / `set_transform`, `node.rs:209/221`) is computed from the **same inputs the paint walk already threads**:

- **Position:** `RenderNode::offset()` (`storage/node.rs:711`) — the authoritative child position committed by layout, exactly what `paint_subtree` reads (`paint.rs:305`). The semantics walk accumulates it as an `Offset` `origin` parameter, identically to paint.
- **Size:** the node's laid-out geometry from `RenderState` (the sole geometry owner), resolved the same way the paint driver resolves the `size` it hands to `paint_raw`.
- **Transform (rare):** when a node reports a `paint_transform(size) -> Option<Matrix4>` (`render_object.rs:477`), compose it into the accumulated transform (the same matrix the paint walk feeds to `TransformLayer` via `conjugate`, `paint.rs:253-266`).

So the answer to "does semantics need its own transform-accumulation mechanism, or can it reuse the paint side?" is: **reuse the inputs, not the machinery.** The `FragmentComposer`/layer tree is for pixels and is *not* reused; but its two geometry inputs (`offset()`, `paint_transform()`) are. **Offset-first is the minimal correct scope** — the overwhelming majority of render objects only translate, so an `Offset` accumulator with a matrix path only when `paint_transform` is `Some` is both correct and simple. Full `Matrix4` accumulation for every node is deferrable precisely because the common case is a translation.

### D5 — The three render objects: config-describers + one exclude hook + one merge field

- **`RenderMergeSemantics`** and the `Semantics`-widget side of **`RenderSemanticsAnnotations`** are pure `describe_semantics_configuration` overrides using the existing config setters (`set_semantics_boundary`, `set_label`, `set_button`, `add_action`, …). `RenderMergeSemantics` additionally needs the new **`is_merging_semantics_of_descendants`** config field — a small additive field on `SemanticsConfiguration` (and its getter/setter), mirroring `isMergingSemanticsOfDescendants`. This is the only data-model addition.
- **`RenderExcludeSemantics`** (and the `excludeSemantics` param of `RenderSemanticsAnnotations`) needs the missing children-visitor seam. **Recommendation:** add a least-privilege boolean hook `excludes_semantics_subtree(&self) -> bool` (default `false`) on `RenderObject`/`RenderBox`/`RenderSliver`, honored by the assembly walk before it recurses — this exactly covers "exclude my whole subtree," which is what both objects need, and is smaller than a full visitor. **Strategic fork noted for api-design-lead:** the more general Flutter shape is `visit_children_for_semantics(visitor)`, which also enables per-child reordering/filtering (e.g. future `RenderIndexedSemantics`, sliver child ordering). Since no current object needs the general form, the boolean is the right minimal call; the general visitor is a clean additive follow-up if a consumer appears. **Recommendation: ship the boolean now, defer the visitor.**

### D6 — Explicitly OUT of scope (state honestly; do not imply "next")

- **The OS accessibility bridge (AT-SPI / UIAccessibility / MSAA / accesskit).** `flui-platform` has **zero** a11y bridge (`rg` for atspi/accesskit/UIAccessibility/MSAA is empty) and there is **no downstream consumer** of the assembled tree. The `SemanticsOwner`'s platform `callback` (`owner.rs:20,122`) currently has nowhere to go. Standing up an OS bridge is its own **multi-session effort** regardless of this ADR; the assembled tree is verified via the render harness and `debug_dump_semantics_tree`, not a live screen reader, until then.
- **The modern `_RenderObjectSemantics` incremental compiler** — `_SemanticsFragment`/`mergeUp`/`siblingMergeGroups`/`computeAncestorInfo`/geometry-dirty incrementalism (`object.dart:5525+`). Deferred; the classic full-rebuild is the loyal-to-observable-output slice.
- **Sibling merge groups** and **`RenderBlockSemantics`/`isBlockingSemanticsOfPreviouslyPaintedNodes`** (`proxy_box.dart:4340-4370`) — not among the three; needs its own config flag + walk support. Deferred.
- **Cross-frame stable `SemanticsId` identity.** The full-rebuild allocates a fresh tree each frame. Stable AT identity needs a `RenderId → SemanticsId` mapping cached on `RenderState` (no such field today) — deferred, and functionally irrelevant until an OS bridge exists.
- **Sliver semantics geometry** (viewport-relative rects, scroll semantics) — the box walk lands first; sliver geometry is a follow-up.
- **Incremental / partial-subtree rebuild** — the coarse full-rebuild is the slice; fine-grained dirty-driven rebuild is the optimization layer.

### Leapfrog note (AGENTS.md rule #2)

Keep the classic assembly for the tree-assembly *behavior* (rule #1 protects the SemanticsNode-tree mental model). Where a genuinely better Rust-native shape belongs is the **incremental layer**: when incremental semantics is eventually needed, design a Rust-native incremental compiler rather than porting `_RenderObjectSemantics`'s nullable-per-node-field machine 1:1. The observable `SemanticsNode` tree is the contract; the compiler's internal shape is not. This is explicitly *not* touched now — it is a directional note for the deferred work.

---

## Consequences

**Positive**

- Closes the last render-object-catalog gap by **wiring existing machinery end-to-end for the first time**: the merge algebra (`absorb`), the phase (`run_semantics`), the dirty tracking (`DirtyKind::Semantics`), the hook (`describe_semantics_configuration`), and the platform enable/disable plumbing (`SemanticsBinding`) are all *reused*, not rebuilt. The net new surface is one `PipelineOwner` field, one config field, one boolean hook, and the assembly walk body.
- No new crate dependency (`flui-rendering → flui-semantics` already exists), no phase restructuring (the post-paint semantics phase already matches `flushSemantics`), no lock in public API, no async on layout/paint/hit-test hot paths (assembly runs in its own post-paint phase).
- The three render objects become a genuine **fast follow-on**: each is a `describe_semantics_configuration` override (plus the exclude boolean), DoD-checkable against `proxy_box.dart` one-for-one.
- Layering stays clean and one-directional: the data model stays in `flui-semantics`, the assembly stays in `flui-rendering`'s pipeline, and the OS bridge (when it lands) stays in `flui-platform` behind the owner's existing `callback` seam.
- The `on_semantics_owner_created/disposed` notifier hooks stop being dead scaffolding — they finally fire about a real field.

**Negative / Trade-offs**

- **Full-rebuild-every-semantics-frame** is O(tree) per frame while semantics is enabled, and allocates a fresh `SemanticsNode` tree each time (no cross-frame node reuse). Accepted: it is correct, it only runs when an assistive tech has enabled semantics (rare relative to paint), and the `SemanticsOwner`'s dirty-flush already amortizes the platform-push allocation (`owner.rs:317-327`). Incrementalism is a named follow-up, not a correctness prerequisite.
- **The classic model diverges from current-master Flutter on edges** — sibling merge groups, block-previous-sibling, geometry-dirty incrementalism, stable ids. This must be reported honestly (DoD anti-cheating): the slice is "classic assembly correct for the common path + the three objects," **not** "semantics parity." Untested edges (a `MergeSemantics` nested inside a sibling-merge scenario, a `BlockSemantics` mask) are explicitly deferred, not silently diverged.
- **A refinement to `is_semantics_boundary()`** (D3) touches a predicate other code may read; the DEV task must confirm no regression to the existing `flui-semantics` unit tests.

**Follow-ups (named, sequenced, not in this ADR)**

- DEV: `SemanticsOwner` field on `PipelineOwner` + lazy create/dispose firing the notifier hooks.
- DEV: assembly walk body in `run_semantics` (the classic rebuild) + the harness proof.
- DEV: the three render objects in `flui-objects` (config describers + `is_merging_semantics_of_descendants` + `excludes_semantics_subtree`).
- Deferred epics: incremental semantics compiler (Rust-native, per leapfrog note); `RenderBlockSemantics` + sibling groups; sliver semantics geometry; **OS accessibility bridge (its own multi-session effort)**.

---

## Alternatives Considered

| Option | Why rejected |
|---|---|
| **Port the modern `_RenderObjectSemantics` incremental compiler 1:1** (`object.dart:5525+`). | Multi-slice effort importing a ~1000-line nullable-per-node state machine (`_SemanticsFragment`/`mergeUp`/`siblingMergeGroups`/geometry-dirty). It is an incremental-performance + edge-case layer whose *observable output* the classic full-rebuild already reproduces for the common path. Porting it now front-loads the hardest, least-observable code before there is even an OS consumer to justify it — and transcribes exactly the nullable-field shape AGENTS.md rule #2 says to reconsider. Do the classic slice; leapfrog the incremental layer later. |
| **Put the `SemanticsOwner` on `RenderView`, or in the `flui-app` binding layer, instead of `PipelineOwner`.** | Breaks Flutter parity (`PipelineOwner._semanticsOwner`) and orphans the `on_semantics_owner_created/disposed` notifier hooks that already live on `PipelineOwner` waiting for this field. `flushSemantics` is a pipeline-owner phase; its output tree belongs to the same owner that runs the phase. `RenderView` already reaches the owner via its `Weak` handle for action dispatch, so nothing is gained by relocating the tree. |
| **Give semantics its own new pipeline phase / restructure the phase walk.** | Unnecessary: the phase already exists as a separate post-paint pass (`into_semantics()` → `run_semantics()` → `finish()`), already sorted shallow-first, already draining a dedicated dirty queue. Only the *body* is a stub. Adding a phase would duplicate machinery that is already correctly shaped. |
| **Build a dedicated transform-accumulation mechanism for the semantics walk.** | The paint walk already computes the two inputs semantics needs — `RenderNode::offset()` (committed position) and `paint_transform()` (the effect matrix). Reusing them (Offset-first, matrix only when present) is one fact in one place; a parallel accumulator would be a second source of truth for node geometry, risking desync with paint. The layer/`FragmentComposer` machinery is *not* reused (it is for pixels); only its geometry inputs are. |
| **Model `RenderExcludeSemantics` with a full `visit_children_for_semantics(visitor)` port now.** | More surface than any current object needs. All three objects only ever need "exclude my whole subtree," which a `excludes_semantics_subtree() -> bool` hook covers at least privilege. The general visitor is a clean additive follow-up the moment a consumer (per-child reorder/filter, sliver ordering) actually appears — deferring it avoids designing a hook shape with no consumer to validate it. |
| **Skip the `SemanticsOwner`/assembly and just make the three objects set config on a per-node cache** (fake the harness green). | DoD anti-cheating violation: it would report "Semantics family done" while `run_semantics` still assembles nothing and the tree is never built. The gap is the assembly, not the config setters; papering over it with a stub that passes a narrow test is exactly the "MVP reported as parity" failure mode AGENTS.md forbids. |

---

## Ordered implementation plan

Changes are confined to `flui-rendering` (owner field + assembly walk + the exclude hook on the traits) and `flui-semantics` (one additive config field), plus `flui-objects` for the three consumers. No workspace-manifest edits.

**Slice A — `SemanticsOwner` home + lifecycle (independent, landable first):**

1. Add `semantics_owner: Option<SemanticsOwner>` to `PipelineOwner` (`pipeline/owner/mod.rs:104`) + thread it through `rebind_phase` (`mod.rs:234`). Lazy create on enable / dispose on last-handle-drop, firing the existing `fire_semantics_owner_created/disposed` (`accessors.rs:1162-1164`). Unit test: enabling semantics creates the owner and fires the created notifier; disabling disposes and fires disposed.

**Slice B — the additive data-model + trait gaps (small, independent):**

2. Add `is_merging_semantics_of_descendants` to `SemanticsConfiguration` (`flui-semantics/src/configuration.rs`) with getter/setter + a unit test, mirroring the existing boolean-config pattern.
3. Add `excludes_semantics_subtree(&self) -> bool` (default `false`) to `RenderObject` (`traits/render_object.rs`, alongside `describe_semantics_configuration`) and forward it from `RenderBox`/`RenderSliver`, mirroring the `describe_semantics_configuration` forwarding at `render_box.rs:722`.

**Slice C — the assembly walk (depends on A + B):**

4. Replace the `run_semantics` warn-stub body (`pipeline/owner/semantics.rs:83-93`) with the classic full-rebuild walk: DFS from `root_id` threading an `Offset` origin (matrix path when `paint_transform` is `Some`), calling `describe_semantics_configuration`, applying the boundary-vs-merge decision (`absorb` up / spawn node), honoring `excludes_semantics_subtree` and `is_merging_semantics_of_descendants`, populating the `SemanticsOwner`'s tree, and calling `flush()`.
5. **★ MILESTONE — harness proof.** In `render_object_harness.rs`, build a small real render tree (a labeled/button leaf, a `MergeSemantics` subtree, an `ExcludeSemantics` subtree) with semantics enabled, run a frame, and assert the assembled `SemanticsNode` tree: the button leaf is one node with the right label/flags/rect; the merge subtree collapses to a single leaf; the excluded subtree contributes nothing. This test is **red** before step 4 and is the DoD evidence that the behavior — not just the gate — is real. Cross-check the assembled shape against `.flutter/` semantics behavior for the same tree.

**Slice D — first real consumers (separate DEV task, out of this ADR's scope but named for sequencing):** `RenderMergeSemantics`, `RenderExcludeSemantics`, `RenderSemanticsAnnotations` in `flui-objects`, each DoD-cross-checked against `proxy_box.dart:4309-4429`, wired into the catalog CI guard (`RENDER_OBJECT_TYPES` + `harness_*` tests), backing the `Semantics`/`MergeSemantics`/`ExcludeSemantics` widgets.

---

## Maintainer-grade pre-code gate

**Verdict: ACCEPTABLE.** The design reuses the capabilities that already exist across `flui-semantics` (the Flutter-faithful `SemanticsConfiguration::absorb` merge, `SemanticsNode`/`SemanticsTree`/`SemanticsOwner`) and `flui-rendering` (the post-paint `run_semantics` phase, the `DirtyKind::Semantics` dirty channel, the `describe_semantics_configuration` hook, the `on_semantics_owner_created/disposed` scaffolding) rather than inventing a parallel semantics compiler. It adds **no** new crate dependency (`flui-rendering → flui-semantics` is pre-existing), **no** new pipeline phase (the separate post-paint phase already matches `flushSemantics`), **no** lock in public API (the `SemanticsOwner` stays behind the owner's methods), and **no** async on the layout/paint/hit-test hot path (assembly is its own post-paint pass). Boundaries stay acyclic and one-directional: data model in `flui-semantics`, assembly in `flui-rendering`, OS bridge (when it exists) in `flui-platform` behind the owner's existing `callback`. One fact, one place: geometry is read from the same `offset()`/`paint_transform()` inputs the paint walk uses (no second geometry source of truth), and the merge algebra is the single `absorb` primitive (no duplicate merge logic). Boundary-type check: the additions are one struct field, one additive config field, and one defaulted forwarded trait method (`excludes_semantics_subtree`, matching the established `describe_semantics_configuration`/`reassemble` no-op-default pattern, ISP-preserving) — deliberately smaller than a full `visit_children_for_semantics` visitor, which would over-encode a contract no current consumer needs. Forward view (2 years / 3 extensions): the same assembly seam serves the box family now, then sliver geometry, then the incremental compiler (designed Rust-native per the leapfrog note), then the OS bridge — each an additive layer over a correct classic core, none requiring the core to be reshaped. The `.flutter/flutter-master/` oracle (`proxy_box.dart` for the three objects, `object.dart` for the assembly hooks) is available for the Slice-C/D DoD cross-check; this ADR's own milestone is the harness proof in Slice C. The one honesty flag the DEV task must carry forward: this is **classic-assembly correctness for the common path and the three objects, not full semantics parity** — the modern compiler's edges and the OS bridge are deferred, and must be reported as deferred, never as done.
