# `RenderLeaderLayer` / `RenderFollowerLayer` — plan (oracle-verified, re-scoped against current source)

Core.2 catalog item, backing `CompositedTransformTarget`/`CompositedTransformFollower`. Oracle: `.flutter/flutter-master/packages/flutter/lib/src/rendering/proxy_box.dart`'s `RenderLeaderLayer` (`:4475-4535`) and `RenderFollowerLayer` (`:4550-4753`), read in full, plus `rendering/layer.dart`'s `LayerLink` (`:2416-2479`), `LeaderLayer` (`:2486-2591`), `FollowerLayer` (`:2603-2904`), and `widgets/basic.dart`'s `CompositedTransformTarget`/`CompositedTransformFollower` (`:1941-2060`).

## Headline verdict, up front

1. **The render-object-level slice is real, well-scoped, and buildable now, exactly like the ShaderMask/BackdropFilter precedent this plan builds directly on.** The prior scoping pass's "nothing consumes this infra" finding is **reconfirmed against current source** (post-`FragmentScope` rename): `crates/flui-rendering/src/context/paint_cx.rs`'s `FragmentScope` enum (renamed from `FragmentClip` exactly as that plan recommended — confirmed live) has `Rect`/`RRect`/`Path`/`ShaderMask`/`BackdropFilter` variants but **no `Leader`/`Follower` variant**; `pipeline/owner/paint.rs`'s `scope_layer` (`:467-503`) has no matching arms; nothing in `flui-objects` constructs either struct (`crates/flui-objects/src/proxy/` has `shader_mask.rs`/`backdrop_filter.rs` — landed since the prior pass — but no `leader.rs`/`follower.rs`). Both new structs fit the **identical** closure-scoped `with_*` mechanism `with_shader_mask`/`with_backdrop_filter` already prove out — no new `FragmentOp` shape needed. §2–§3.
2. **A genuinely deeper architectural question than the prior pass surfaced, precisely isolated: FLUI's paint pipeline is a SINGLE recursive pass that builds the `LayerTree`, unlike Flutter's two-phase model** (`flushPaint` fully populates a *retained* Layer tree with parent pointers — `object.dart:983-1000` — as one pipeline phase; `RenderView.compositeFrame` → `layer.buildScene()`, a **separate, later** step, `view.dart:347-362`, is where `FollowerLayer._establishTransform()` actually resolves a follower's transform, `layer.dart:2797-2841`). This is why Flutter's own debug-only order assertion (`_debugCheckLeaderBeforeFollower`, `layer.dart:2767-2795`) is *not* a correctness requirement for the transform math itself — it's a hygiene check on top of a mechanism that is **inherently same-frame and paint-order-independent** because compositing runs strictly after the whole paint phase completes. §4 pins down exactly what FLUI needs to replicate this guarantee and where the current architecture falls short.
3. **The render-object slice (Tier 1) does not require resolving that question.** `Layer::Leader`'s **own** rendering is already fully self-contained and correct once `RenderLeaderLayer` pushes it correctly (`LeaderLayer::render` — `crates/flui-engine/src/wgpu/layer_render.rs:407-421` — reads only its own `offset`/fields, no registry). `Layer::Follower` is structurally real, field-complete, and harness-verifiable the moment `RenderFollowerLayer` pushes it (`run.structure()`/`LayerTree::iter()` + `as_leader()`/`as_follower()` downcasts already exist, `layer/mod.rs:459-460`). §6 gives the harness test plan; zero new plumbing needed, exactly mirroring the ShaderMask/BackdropFilter precedent.
4. **Actually resolving a follower's on-screen position for GPU rendering is a real, scoped, tractable `flui-layer`+`flui-engine` follow-up — not blocking, not ADR-level.** `LayerTree` already has parent pointers (`layer_tree.rs:150,702`); FLUI's composer only ever changes coordinate space at a repaint-boundary crossing via a single `Layer::Offset` push (`paint.rs:307-318`) — no other layer bends the accumulated `origin`. Resolving leader→follower reduces to summing `Layer::Offset` deltas along the two ancestor chains to their common ancestor (a translation-only analogue of Flutter's `_pathsToCommonAncestor`/`_collectTransformForLayerChain`), then feeding the result to `renderer.push_offset`/`pop_transform` in a new `Layer::Follower` special-case in `render_layer_recursive` (mirroring the **already-existing** `Layer::BackdropFilter` special-case, `renderer.rs:1539-1552`). Sketched, not built, in §5/§7.
5. **Hit-testing correctness for `RenderFollowerLayer` is the genuinely open, ADR-level gap** — reclassified explicitly, per this session's own established practice (Semantics family; pre-ADR-0013 `RenderAnimatedSize`). `RenderObject::hit_test_transform(&self, size) -> Option<Matrix4>` (`traits/render_object.rs:487-490`) is the FLUI hook a Follower would need to override to match oracle's `applyPaintTransform`/`hitTestChildren` — but it takes **zero external context**: no `LinkRegistry`, no `LayerTree`, no `RenderId↔LayerId` bridge (confirmed absent by grep — none exists anywhere today). `PipelineOwner::hit_test` (`accessors.rs:299-311`) is a wholly separate walk over the *render* tree, with no coupling to the paint-produced `LayerTree` at all. Making a Follower's hit-test target the same position its content paints at requires inventing a genuinely new cross-phase channel — a decision with real trade-offs (Flutter's own answer is "one-frame-stale is acceptable," reading `getLastTransform()` cached from the *last completed* composite, `layer.dart:2668-2680,2707-2714`) that deserves a chief-architect ADR conversation, not a call buried in this plan. §4.4, §8.
6. **Struct shape: two independent, non-generic structs** (§5) — an even clearer call than the immediately-preceding ShaderMask/BackdropFilter pair: Leader has no hit-test override at all in oracle (relies on `RenderProxyBoxMixin` defaults); Follower has a materially different, non-trivial custom `hitTest`/`hitTestChildren` override plus three extra fields with no Leader analogue.

## 1. Oracle — `RenderLeaderLayer` / `RenderFollowerLayer` (`proxy_box.dart`, read in full)

### 1.1 `RenderLeaderLayer` (`:4475-4535`)

- Constructor (`:4477`): `required LayerLink link`, `RenderBox? child` (nullable — a `RenderProxyBox`, so **0 or 1 children**, not mandatory).
- `link` setter (`:4486-4496`): on change, clears `_link.leaderSize = null` on the OLD link, swaps to the new link, and — if a previous layout already ran (`_previousLayoutSize != null`) — immediately writes `leaderSize` onto the **new** link. `markNeedsPaint()` only (no `markNeedsLayout`).
- `alwaysNeedsCompositing => true` (`:4498-4499`) — **unconditional**, unlike the immediately-preceding `RenderShaderMask`/`RenderBackdropFilter` pair's `child != null`-gated version. A Leader with no child still needs its own compositor layer (it's a coordinate anchor, not a visual effect).
- `performLayout` (`:4506-4511`): `super.performLayout()` (plain proxy sizing), then caches `_previousLayoutSize = size` and writes `link.leaderSize = size` — **size is published to the link at LAYOUT time**, a separate channel from the offset (published at paint time). This split exists because Flutter's `LayerLink` is itself a mutable, shared object (`_leader`/`leaderSize` fields directly on it); FLUI's `LayerLink` is a bare `Copy` `u64` id (`crates/flui-layer/src/layer/leader.rs:12-14`) with no embedded mutable state at all — the equivalent data lives in the external `LinkRegistry` side-table instead (§3).
- `paint` (`:4513-4528`): **unconditionally** creates-or-updates a `LeaderLayer(link: link, offset: offset)` and does `context.pushLayer(layer!, super.paint, Offset.zero)` — regardless of whether `child` is null. `super.paint` is `RenderProxyBoxMixin.paint` (`:138-144`: `if (child == null) return; context.paintChild(child, offset)`) — so the *layer* always gets pushed, but paints nothing under it when childless.
- `debugFillProperties` (`:4531-4534`): surfaces `link` only.
- **No `hitTest`/`hitTestChildren`/`applyPaintTransform` override at all** — Leader is hit-test-transparent, relying entirely on inherited `RenderProxyBoxMixin` defaults (`hitTestChildren`: `child?.hitTest(...) ?? false`, `:129-132`; `applyPaintTransform`: no-op, `:134-135`).

### 1.2 `RenderFollowerLayer` (`:4550-4753`)

- Constructor (`:4552-4564`): `required LayerLink link`, `bool showWhenUnlinked = true`, `Offset offset = Offset.zero`, `Alignment leaderAnchor = Alignment.topLeft`, `Alignment followerAnchor = Alignment.topLeft`, `RenderBox? child`.
- Every setter (`link`/`showWhenUnlinked`/`offset`/`leaderAnchor`/`followerAnchor`, `:4568-4647`) is a plain guard-and-set + `markNeedsPaint()`.
- `detach()` override (`:4649-4653`): sets `layer = null` before calling `super.detach()` — this is Flutter's own layer-lifecycle mechanism (a detached-then-reattached `RenderFollowerLayer` gets a fresh `FollowerLayer` next paint rather than reusing a stale one whose leader link may have changed underneath it).
- `alwaysNeedsCompositing => true` (`:4656`) — **unconditional**, same as Leader.
- `layer` getter override (`:4659-4660`) narrows `super.layer` to `FollowerLayer?`.
- `getCurrentTransform()` (`:4668-4670`): `layer?.getLastTransform() ?? Matrix4.identity()` — reads a value **cached on the layer from the last completed compositing pass**, not recomputed live. This is the load-bearing detail for the hit-test question (§1.3).
- `hitTest` override (`:4672-4683`): `if (link.leader == null && !showWhenUnlinked) return false;` then delegates to `hitTestChildren` — **Follower never adds itself as a hit target**, only forwards (unlike `RenderProxyBoxWithHitTestBehavior`).
- `hitTestChildren` override (`:4685-4694`): wraps the default forward in `result.addWithPaintTransform(transform: getCurrentTransform(), ...)` — i.e. hit-testing explicitly re-applies the **same cached transform** paint used, by construction consistent with what's on screen (assuming no intervening frame moved the leader without a follower repaint — see §1.3).
- `paint` (`:4696-4738`): computes `effectiveLinkedOffset` from `leaderAnchor`/`followerAnchor`/`link.leaderSize` (`Alignment.alongSize` is exactly FLUI's `FollowerLayer::calculate_offset` anchor math, already ported verbatim, §3), then **unconditionally** creates-or-updates a `FollowerLayer(link, showWhenUnlinked, linkedOffset: effectiveLinkedOffset, unlinkedOffset: offset)` and pushes it with `childPaintBounds` widened to `(-∞, -∞, +∞, +∞)` (`:4726-4732`, comment: "We don't know where we'll end up, so we have no idea what our cull rect should be" — a correctness note that a Follower's cull/paint-bounds cannot be inferred from its own tree position, since it may render anywhere on screen).
- `applyPaintTransform` (`:4741-4743`): `transform.multiply(getCurrentTransform())` — the SAME cached value as hit-test uses.
- `debugFillProperties` (`:4746-4752`): `link`, `showWhenUnlinked`, `offset`, and a computed `TransformProperty('current transform matrix', getCurrentTransform())`.
- **The single `offset` field does double duty**: it is (a) the pixel gap added into `effectiveLinkedOffset` when a leader is present, AND (b) passed directly as `unlinkedOffset` — the standalone position used when there's no leader. FLUI's `FollowerLayer` (flui-layer) has no matching "unlinked offset" concept — only `target_offset`, consumed exclusively by `calculate_offset` (which *requires* a resolved leader pose). §7 traps this precisely.

### 1.3 The paint-order / compositing-phase mechanism, confirmed precisely (not guessed)

This is the crux the task asked to nail down exactly, not infer.

- **Flutter's pipeline runs paint and compositing as two structurally separate phases.** `PipelineOwner`'s own doc (`object.dart:983-1000`) enumerates: `flushLayout` → `flushCompositingBits` → `flushPaint` ("visits any render objects that need to paint... record painting commands into `PictureLayer`s **and construct other composited `Layer`s**") → (semantics). Building the actual GPU `ui.Scene` is a **separate, later** call: `RenderView.compositeFrame()` (`view.dart:347-362`) calls `layer!.buildScene(builder)`, which is what triggers every attached layer's `addToScene`, including `FollowerLayer.addToScene` (`layer.dart:2856-2886`).
- **Consequence: by the time `FollowerLayer.addToScene` runs, `flushPaint` has ALREADY completed for the entire tree.** Every `LeaderLayer` anywhere in the tree — regardless of its position relative to the Follower, including a completely disjoint `Overlay` entry — has already called `attach()` (`layer.dart:2535-2538`), which calls `link._registerLeader(this)`, setting `link._leader`. `addToScene` is a **retained-tree walk with real parent pointers** (`Layer.parent`, set when a layer is appended to a `ContainerLayer` during paint): `FollowerLayer._establishTransform` (`:2797-2841`) calls `_pathsToCommonAncestor(leader, this, ...)` (`:2739-2765`) to find the nearest shared ancestor, then `_collectTransformForLayerChain` (`:2722-2731`) to compose every intervening `ContainerLayer.applyTransform` call along both paths — this is a **pure tree-structure computation over the already-fully-built retained layer tree**, independent of which subtree's `paint()` ran first during the preceding `flushPaint` pass.
- **The debug-only `_debugCheckLeaderBeforeFollower` (`:2767-2795`, wrapped in an `assert(...)`, stripped in release/profile builds) is a hygiene contract, not a correctness requirement for the transform math** — the math itself works regardless of paint-order because compositing is a wholly separate, later pass over a tree that's already complete. `widgets/basic.dart`'s `CompositedTransformFollower` doc states the phase split explicitly (`:1966-1980`): *"When this widget is composited during the compositing phase (which comes after the paint phase, as described in `WidgetsBinding.drawFrame`)..."* and *"The `CompositedTransformTarget` must come earlier in the paint order than this `CompositedTransformFollower`"* — the latter is the documented (debug-enforced) contract Flutter still asks authors to honor, even though the actual mechanism doesn't strictly need it for the transform to resolve.
- **Net verdict on "one-frame lag" vs. "same-frame resolution": Flutter achieves genuine same-frame, order-independent resolution** — not a one-frame lag — because compositing runs strictly after the complete paint pass. Any FLUI design that wants oracle parity must replicate that same **two-phase** shape (§4), not just "paint order happens to work out."
- **Hit-testing reads a CACHED value, not a live re-derivation** (§1.2, `getCurrentTransform() = layer?.getLastTransform() ?? Matrix4.identity()`): pointer events are dispatched between frames (after a complete frame's paint+composite already ran), so in steady state this is exactly up to date; the only staleness window is mid-frame, which is unobservable to hit-testing since hit-testing never runs concurrently with a frame in Flutter's single-threaded UI model. This is the specific, confirmed mechanism the prompt asked to pin down — not a hand-wave.

## 2. FLUI building blocks — `flui-layer`, verified against current live source

### 2.1 `LayerLink` / `LeaderLayer` (`crates/flui-layer/src/layer/leader.rs`, read in full)

`LayerLink` is a bare `Copy`, `Eq`+`Hash` `u64` id (`:12-14`, atomic-counter-generated, `:16-23`) — **not** an `Arc`-wrapped shared mutable object like Flutter's. All the mutable state Flutter embeds directly on `LayerLink` (`_leader`, `leaderSize`) lives externally, in `LinkRegistry` (§2.3). `LeaderLayer` (`:80-90`): `link: LayerLink`, `size: Size<Pixels>`, `offset: Offset<Pixels>` (defaults `ZERO`); constructors `new(link, size)` / `with_offset(link, size, offset)`; `bounds()` derives a `Rect` from `offset`+`size`. `#[derive(Debug, Clone, Copy, PartialEq)]` — trivially cheap to construct fresh every paint pass.

### 2.2 `FollowerLayer` (`crates/flui-layer/src/layer/follower.rs`, read in full)

Fields (`:58-73`): `link: LayerLink`, `target_offset: Offset<Pixels>`, `show_when_unlinked: bool` (default `true`), `leader_anchor: Alignment` (default `TOP_LEFT`), `follower_anchor: Alignment` (default `TOP_LEFT`). `Alignment` here is `flui_types::painting::Alignment`, re-exporting the canonical `flui_types::layout::Alignment` (`(-1,-1)`=top-left, `(0,0)`=center — matches oracle's `Alignment` 1:1). Builder methods (`with_target_offset`/`with_show_when_unlinked`/`with_leader_anchor`/`with_follower_anchor`) plus plain setters, plus convenience constructors `below`/`above`/`left_of`/`right_of` (gap-based, tooltip-style).

`calculate_offset(leader_offset, leader_size, follower_size) -> Offset<Pixels>` (`:184-212`, unit-tested `:291-326`) is the **full anchor-math port of oracle's `Alignment.alongSize` arithmetic** — maps `leader_anchor` to a pixel point inside a leader-sized rect, `follower_anchor` to a pixel point inside a follower-sized rect, and returns `leader_offset + leader_anchor_px + target_offset − follower_anchor_px`. **This function has no "unlinked" code path at all** — it unconditionally requires a resolved `leader_offset`/`leader_size`. There is no FLUI analogue of oracle's `unlinkedOffset` (§1.2's "double-duty `offset` field") anywhere in this struct or its methods — §7 traps this.

### 2.3 `LinkRegistry` (`crates/flui-layer/src/link_registry.rs`, read in full)

`leaders: HashMap<LayerLink, LeaderInfo>` / `followers: HashMap<LayerId, LayerLink>` (`:121-127`). `LeaderInfo { layer_id, offset: Offset<Pixels>, size: Size<Pixels>, followers: Vec<LayerId> }` (`:64-77`) — **bundles offset+size together**, both writable via one `register_leader(link, layer_id, offset, size)` call (`:150-164`) or `update_leader(link, offset, size)` (`:167-172`). This is a deliberate FLUI simplification relative to oracle's split registration (size at layout time via `link.leaderSize =`, offset at paint time via the pushed `LeaderLayer.offset`): since FLUI's `LinkRegistry` is an external side-table (not a field embedded in `LayerLink` itself), both pieces of data are naturally available together at the single point `RenderLeaderLayer::paint` runs (`ctx.size()` for size, the accumulated paint `origin` for offset) — there is no correctness reason to split it across layout+paint the way oracle does, since FLUI's paint phase always runs strictly after layout completes for the whole tree anyway. `remove_orphaned_followers()` (`:284-297`) does the GC the module doc promises. `crates/flui-layer/ARCHITECTURE.md:123` already documents the intended usage shape ("Touched during scene build, occasionally during render (follower position lookup)") — corroborating, independently, the design direction §4 arrives at from first principles.

### 2.4 `Scene` (`crates/flui-layer/src/scene.rs`, read in full) — confirmed reproduced, with the escape hatch already present

`Scene::new(size, layer_tree, root, frame_number)` (`:140-154`) **always** constructs `link_registry: LinkRegistry::new()` — a **fresh, empty** registry, every call. This is the exact gap the prior scoping pass found, reconfirmed against current source. **But `Scene::with_links(size, layer_tree, root, link_registry, frame_number)` already exists** (`:157-172`) — a constructor that accepts a pre-populated `LinkRegistry`. `crates/flui-app/src/app/binding.rs:738` calls `Scene::new` (not `with_links`), reconfirming the prior finding precisely: the escape hatch exists at the `Scene` API level, it's simply never invoked by production code because nothing upstream (the pipeline) produces a populated `LinkRegistry` to pass in yet.

## 3. FLUI building blocks — `flui-rendering`, verified against CURRENT source (post-ShaderMask/BackdropFilter landing)

### 3.1 `FragmentScope` / `PaintCx` (`crates/flui-rendering/src/context/paint_cx.rs`, read in full, 490+ lines) — confirmed the right extension point, confirmed already renamed

The prior pass's scoping-note about `FragmentClip` was **acted on**: the enum is now `FragmentScope` (`:114-164`) with `Rect`/`RRect`/`Path`/`ShaderMask`/`BackdropFilter` variants, each following the identical closure-scoped shape: `push_scope(FragmentScope::Variant{..}); f(self); pop_scope()` (e.g. `with_shader_mask`, `:442-455`; `with_backdrop_filter`, `:464-477`, both already landed and already shipping in `crates/flui-objects/src/proxy/shader_mask.rs`/`backdrop_filter.rs`). **`Leader` and `Follower` fit this mechanism exactly as well as `ShaderMask`/`BackdropFilter` did** — both oracle classes' `paint()` bodies are "push one layer that tags/positions a child subtree, then paint the child inside it" (§1.1/1.2), the identical shape `with_shader_mask` already proves. No new `FragmentOp` variant, no leaf-push-without-scope pattern is needed — the task's speculative "does Follower need something different since it has no child subtree in the same sense" is **resolved: no**, oracle's own `RenderFollowerLayer.paint()` (`:4722-4733`) pushes exactly one layer wrapping exactly one `super.paint` call, identically shaped to Leader/ShaderMask/BackdropFilter.

**One real divergence from the ShaderMask/BackdropFilter gating pattern**: those two gate on `ctx.child_count() == 0` (push nothing at all when childless, §1.1/1.2 of that plan). Leader/Follower do **not** gate — oracle pushes unconditionally regardless of child (§1.1/1.2 above, confirmed by the absence of any `if (child != null)` check in either oracle `paint()` body). The new `with_leader`/`with_follower` methods must NOT copy the `if ctx.child_count() == 0 { return; }` guard from their siblings.

Sketch (placed alongside `with_shader_mask`/`with_backdrop_filter`, same file):

```rust
/// Wraps everything painted inside `f` in a `LeaderLayer` tagged with
/// `link`, publishing this node's own paint-time size to the layer
/// (Flutter registers size at layout time via `link.leaderSize =`;
/// FLUI's `LinkRegistry` bundles offset+size together and both are
/// naturally available here — see plan §2.3).  Pushed UNCONDITIONALLY,
/// unlike `with_shader_mask`/`with_backdrop_filter` — oracle's
/// `RenderLeaderLayer.paint` never gates on child presence (`:4513-4528`).
pub fn with_leader(&mut self, link: LayerLink, size: Size<Pixels>, f: impl FnOnce(&mut Self)) {
    self.rec.push_scope(FragmentScope::Leader { link, size });
    f(self);
    self.rec.pop_scope();
}

/// Wraps everything painted inside `f` in a `FollowerLayer` tagged with
/// `link`.  Also pushed UNCONDITIONALLY (oracle `:4708-4721`) — the
/// no-leader/hidden decision is resolved at composite/render time
/// (`FollowerLayer.addToScene`, layer.dart `:2857-2865`), not here.
pub fn with_follower(
    &mut self,
    link: LayerLink,
    target_offset: Offset<Pixels>,
    show_when_unlinked: bool,
    leader_anchor: Alignment,
    follower_anchor: Alignment,
    f: impl FnOnce(&mut Self),
) {
    self.rec.push_scope(FragmentScope::Follower {
        link, target_offset, show_when_unlinked, leader_anchor, follower_anchor,
    });
    f(self);
    self.rec.pop_scope();
}
```

`FragmentScope` gains two matching variants (no local `bounds` needed — Leader/Follower carry no clip/mask geometry, just link identity + anchor-math inputs). Needs `flui_types::painting::Alignment` and `flui_rendering::layer::LayerLink` (already re-exported, `flui-rendering/src/lib.rs:106-109` — `pub mod layer { pub use flui_layer::*; }`) added to `paint_cx.rs`'s imports.

### 3.2 `pipeline/owner/paint.rs`'s `scope_layer` (read in full, `:1-504`) — the mapping, and the crux single-pass finding

`scope_layer(scope: FragmentScope, origin: Offset) -> Layer` (`:467-503`) gains two arms:

```rust
FragmentScope::Leader { link, size } =>
    Layer::Leader(LeaderLayer::with_offset(link, size, origin)),
FragmentScope::Follower { link, target_offset, show_when_unlinked, leader_anchor, follower_anchor } =>
    Layer::Follower(
        FollowerLayer::new(link)
            .with_target_offset(target_offset)
            .with_show_when_unlinked(show_when_unlinked)
            .with_leader_anchor(leader_anchor)
            .with_follower_anchor(follower_anchor),
    ),
```

This mechanically reproduces oracle's `LeaderLayer(link, offset: offset)` (`offset` = this node's accumulated position, exactly what `origin` already is at this call site, `paint.rs:270`). **`Layer::Follower` carries no resolved position at all** — matching oracle, where `FollowerLayer`'s `linkedOffset`/`unlinkedOffset` are *inputs* to a later resolution (`_lastTransform`/`_lastOffset` are the *outputs*, computed by `_establishTransform`, never stored as constructor args). This confirms FLUI's `Layer::Follower` shape is already correctly "resolution is a later, separate step" — it just has no such step wired up yet (§4).

**The crux finding, precisely**: `run_paint` (`:53-133`) calls `self.paint_subtree(&mut composer, root_id, Offset::ZERO, &dirty_ids)` **once**, and `paint_subtree_impl` (`:155-328`) is a single recursive descent that **directly builds the final `LayerTree`** via `FragmentComposer` (push/pop onto a stack, `:342-428`) as it visits nodes in render-tree order — there is **no second, later pass** analogous to Flutter's `compositeFrame`/`buildScene`. `composer.finish()` (`:85`) returns the complete tree the moment the recursion returns. **This is structurally different from Flutter's two-phase (`flushPaint` fully-populates-retained-tree, THEN separately `compositeFrame`-walks-it) model** — FLUI's "paint" and "compositing" are the same pass. Consequently: if a `Layer::Leader` is registered as a side effect of visiting it (e.g. into a registry) only *during* this single descent, a `Layer::Follower` visited **earlier** in tree order (the common real case: a tooltip/dropdown in an `Overlay`-equivalent that paints before or in a disconnected part of the tree from its leader) would see an **empty or stale** registry entry at its own visit point — the same-frame, order-independent guarantee Flutter achieves (§1.3) does **not** fall out of FLUI's current single-pass shape "for free."

### 3.3 `flui-engine`'s render-time consumption — confirmed gap, confirmed shape of the fix

`LayerRender<LeaderLayer>` (`crates/flui-engine/src/wgpu/layer_render.rs:407-421`) already does the **correct, self-contained** thing: `renderer.push_offset(self.get_offset())` / `renderer.pop_transform()` on cleanup, gated on non-zero offset — this needs **no registry at all**, it's just "translate to wherever this node painted," exactly matching oracle's `LeaderLayer.addToScene` (`layer.dart:2556-2569`, a plain `pushTransform(translationValues(offset...))`). Once §3.1/§3.2 land, `Layer::Leader` renders correctly with **zero further engine work**.

`LayerRender<FollowerLayer>` (`:423-431`) is **confirmed still a no-op** on both `render`/`cleanup`, comment: *"Transform is calculated by the compositor"* — and `LinkRegistry` is confirmed to have **zero consumers anywhere in `flui-engine`** (grep across the crate: the only hit is the `pub use` re-export in `lib.rs:126`). Tracing the call chain confirms **no signature currently threads a `LinkRegistry` to where it would be needed**: `Renderer::render_scene` (`renderer.rs:1048`) → `render_scene_content` (`:1339-1500`, has `scene: &flui_layer::Scene` in scope, so `scene.link_registry()` **is** reachable here) → `render_layer_recursive` (`:1517-1572`, receives only `tree: &LayerTree`, **not** the `Scene` or its registry) → `layer.render(backend)` (`LayerRender::render(&self, renderer: &mut R)` — **no external-context parameter at all**, confirmed by the trait definition, `layer_render.rs:40-49`).

**The fix is mechanical and precedented**: `render_layer_recursive` already special-cases one layer kind outside the generic `LayerRender` dispatch — `Layer::BackdropFilter` (`:1539-1552`, `if let flui_layer::Layer::BackdropFilter(bf_layer) = layer { Self::handle_backdrop_filter(...); return; }`). A `Layer::Follower` arm follows the identical shape: thread `link_registry: &LinkRegistry` down from `render_scene_content` (which already has it via `scene.link_registry()`) alongside the existing `tree`/`ctx`/`surface_texture`/`surface_view` parameters, special-case `Layer::Follower` before the generic dispatch, resolve the offset (§4's ancestor-chain-sum algorithm, run against `tree` which is by now fully built), and call `renderer.push_offset(resolved)`/children/`renderer.pop_transform()` — reusing exactly the mechanism `LeaderLayer::render` already uses correctly.

## 4. The paint-order resolution design — a concrete answer, not left ambiguous

The task requires picking one design. Given §3.2's single-pass finding and §3.3's confirmed engine-side gap:

**Decision: resolve Follower positions at render time (inside `flui-engine`'s `render_layer_recursive`), against the already-fully-built `LayerTree` for the current frame — genuine same-frame, paint-order-independent resolution, matching Flutter's actual guarantee (§1.3), not a one-frame lag.** This works because:

1. `run_paint`'s single descent (§3.2) still **fully completes** the `LayerTree` before `render_scene`/`render_layer_recursive` ever runs on it (paint and render remain two genuinely separate `PipelineOwner` phases, even though "paint" itself is one recursive pass rather than two) — so by the time the engine walks the tree, every `Layer::Leader` in it, anywhere, has already been composed with its correct position.
2. `LayerTree` already has parent pointers (`layer_tree.rs:150,702`), and FLUI's composer only ever changes accumulated coordinate space at a repaint-boundary crossing, via a single `Layer::Offset` push (`paint.rs:307-318` — no other pushed layer touches the `origin` accumulator threaded through recursion). Resolving "the leader's position relative to the follower" is therefore: walk `LayerTree::parent()` from the `Layer::Leader` node to the root, walk from the `Layer::Follower` node to the root, find the last common node, and sum the `Layer::Offset.offset()` values encountered along each side back to that common ancestor — a **translation-only** analogue of Flutter's `_pathsToCommonAncestor`/`_collectTransformForLayerChain` (`layer.dart:2722-2765`), tractable specifically *because* FLUI's model only ever composites via plain-offset boundary hops (unlike Flutter's fully general `Matrix4` ancestor chain — a documented, deliberate scope-down, §8).
3. This needs **no** cross-frame persistence, no `PipelineOwner`-owned `LinkRegistry`, and no change to `run_paint`'s recursive shape: a **fresh** `LinkRegistry`, populated once per frame as a byproduct of the SAME single paint descent (every `FragmentScope::Leader`/`FragmentScope::Follower` composed into a `Layer::Leader`/`Layer::Follower` also registers `(link, layer_id, size)`/`(follower_layer_id, link)` into a registry the `FragmentComposer` carries alongside its `tree`/`stack` — a small, mechanical addition to `FragmentComposer`, since `push_layer` already knows the `LayerId` it just inserted), then handed to `Scene::with_links` (`binding.rs:738`, replacing today's `Scene::new` call) instead of being discarded. **No `PipelineOwner`-level persistent-registry redesign is required for the rendering half** — reclassifying the prior pass's "PipelineOwner-level design decision" framing down to "small, mechanical, same-frame plumbing" for this specific half.
4. **Hit-testing is the part that genuinely cannot use this same trick** (§4.4) — `PipelineOwner::hit_test` runs as an independent walk over the *render* tree with no reference to any `LayerTree`/`Scene`/`LinkRegistry` at all, and can run at arbitrary times relative to a paint/render pass (e.g. synthetic/headless hit-testing in tests, or — once threaded through the app layer — real pointer events that may arrive without a fresh paint having run). Making it consult the *same* per-frame registry §4.3 builds for rendering requires deciding **where that registry (or a resolved-transform cache derived from it) lives when hit-testing runs**, which is exactly the kind of decision that changes a public trait signature (`hit_test_transform`) or adds a new `RenderId↔LayerId` correlation that doesn't exist in any form today — genuinely a chief-architect-level call, not a mechanical extension of §4.1-3's design. **This is the piece to route to an ADR before it's implementation-ready** (§8), not the rendering half.

## 5. FLUI struct shape — explicit two-struct decision

**Decision: two independent, non-generic structs.** Even clearer than the immediately-preceding ShaderMask/BackdropFilter call (that plan's own §3, itself contrasted against the `RenderPhysicalModel`/`RenderPhysicalShape` generic precedent): Leader has **zero** hit-test/`applyPaintTransform` override in oracle at all (relies on inherited `RenderProxyBoxMixin` defaults, §1.1); Follower has a **materially different**, non-trivial custom `hitTest`/`hitTestChildren` override (the no-leader/`showWhenUnlinked` gate plus the wrapped-transform forward, §1.2) and three fields with no Leader analogue (`show_when_unlinked`, `offset`/`target_offset`, `leader_anchor`/`follower_anchor`). The only shared shape is "single-optional-child proxy, unconditionally pushes a link-tagged layer wrapping whatever paints inside it, `always_needs_compositing() == true` unconditionally" — about three lines, and even that unconditional-push behavior is itself a point of *contrast* with the sibling ShaderMask/BackdropFilter pair (§3.1), not a shared novel mechanism worth a generic parameter.

```rust
// crates/flui-objects/src/proxy/leader.rs (new)
use flui_rendering::layer::LayerLink;
use flui_types::{Offset, Size};

pub struct RenderLeaderLayer {
    link: LayerLink,
    has_child: bool,
}

impl RenderBox for RenderLeaderLayer {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            self.has_child = true;
            let size = ctx.layout_child(0, constraints);
            ctx.position_child(0, Offset::ZERO);
            size
        } else {
            self.has_child = false;
            constraints.smallest()
        }
    }
    flui_rendering::forward_single_child_box_queries!();

    // Oracle `:4498-4499` — UNCONDITIONAL, unlike ShaderMask/BackdropFilter's `self.has_child`.
    fn always_needs_compositing(&self) -> bool { true }

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        // Oracle pushes LeaderLayer regardless of child (`:4513-4528`) — no
        // `ctx.child_count() == 0` gate, unlike the sibling proxy pair.
        let size = ctx.size();
        ctx.with_leader(self.link, size, |ctx| ctx.paint_children_in_order());
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() { return false; }
        if self.has_child { ctx.hit_test_child_at_offset(0, Offset::ZERO) } else { false }
    }
}
```

```rust
// crates/flui-objects/src/proxy/follower.rs (new)
use flui_rendering::layer::LayerLink;
use flui_types::{Offset, Size, painting::Alignment};

pub struct RenderFollowerLayer {
    link: LayerLink,
    show_when_unlinked: bool,   // default true — oracle `:4554`
    offset: Offset,             // oracle's dual-purpose field, `:4555` — feeds BOTH
                                // the linked-anchor gap AND the unlinked fallback (§1.2, §7)
    leader_anchor: Alignment,   // default TOP_LEFT — oracle `:4556`
    follower_anchor: Alignment, // default TOP_LEFT — oracle `:4557`
    has_child: bool,
}

impl RenderBox for RenderFollowerLayer {
    type Arity = Single;
    type ParentData = BoxParentData;

    // perform_layout / forward_single_child_box_queries!() — identical shape to
    // RenderLeaderLayer above (plain single-optional-child proxy sizing).

    fn always_needs_compositing(&self) -> bool { true } // oracle `:4656` — UNCONDITIONAL

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        // Oracle pushes FollowerLayer regardless of child (`:4708-4721`) — the
        // no-leader/hidden decision resolves at render time (§4), not here.
        ctx.with_follower(
            self.link, self.offset, self.show_when_unlinked,
            self.leader_anchor, self.follower_anchor,
            |ctx| ctx.paint_children_in_order(),
        );
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        // Oracle `:4672-4694`: Follower never adds itself as a hit target, only
        // forwards, gated on `link.leader == null && !show_when_unlinked`, wrapped
        // in the CURRENT resolved transform. This Tier-1 body implements only the
        // structural forward (§4.4/§8 — resolved-transform hit-testing is the
        // explicitly deferred ADR-level half; a self-cached `Cell<Offset>` here
        // would be silently wrong whenever this node's own paint ran before its
        // leader's in the same pass, so it is NOT a shortcut taken in this plan).
        if !self.has_child { return false; }
        ctx.hit_test_child_at_offset(0, Offset::ZERO) // TODO(follow-up ADR): resolved offset
    }
}
```

Setters follow `RenderClip::set_clipper`'s established convention (`clip.rs:430-438`): unconditional overwrite + changed-`bool` return; `LayerLink`/`Alignment` are both `Copy + PartialEq` (unlike `ShaderCallback`'s `Arc<dyn Fn>`), so no identity-only comparison workaround is needed here. Diagnostics: `RenderLeaderLayer` surfaces `link` (matching oracle `:4531-4534`); `RenderFollowerLayer` surfaces `link`/`show_when_unlinked`/`offset`/`leader_anchor`/`follower_anchor` (matching oracle `:4746-4752` minus the derived `current transform matrix`, which has no resolved value to show until §8 lands — document the omission rather than fabricating a value).

## 6. Test plan

Pattern precedent: `crates/flui-objects/tests/render_object_harness.rs`'s `harness_shader_mask_*`/`harness_backdrop_filter_*` blocks (`:1827+`) are the direct structural template, extended exactly the same way that plan extended the clip/opacity precedent before it — `run.structure()` (already returns `"Leader"`/`"Follower"` today, `layer/mod.rs:389-390`, zero new plumbing) and `LayerTree::iter()` + the **already-generated** `is_leader`/`as_leader`/`as_leader_mut`, `is_follower`/`as_follower`/`as_follower_mut` downcasts (`layer/mod.rs:459-460`).

- **Layout pass-through**: Single-arity proxy, `box_geometry(root) == box_geometry(child)` when a child is present; `constraints.smallest()` when absent — mirrors every other proxy harness test in the file.
- **Always-push regression (the key divergence from the sibling pair)**: mount **with zero children**; assert `run.structure()` **does** contain `"Leader"`/`"Follower"` (unlike ShaderMask/BackdropFilter's own no-child test, which asserts the layer is **absent** — §1.1/§1.2/§3.1's confirmed unconditional-push behavior). This is the direct regression test for the trap a naive port would fall into by copying the sibling's gating.
- **Field round-trip — Leader**: construct with a specific `LayerLink`; walk `run.layer_tree().unwrap().iter().find(|(_, n)| n.layer().is_leader())`, assert `.as_leader().unwrap().link() == expected_link` and `.size()` equals the node's committed paint size.
- **Field round-trip — Follower**: construct with non-default `show_when_unlinked`/`leader_anchor`/`follower_anchor`/`offset`; assert the corresponding `as_follower()` fields round-trip (catches a composer wiring bug that drops or defaults a field — the same class of test the ShaderMask/BackdropFilter plan used for `blend_mode`).
- **`always_needs_compositing`**: unit-test (no harness needed) that both return a hardcoded `true` regardless of `has_child` — the regression test contrasting with the sibling pair's `self.has_child`-gated version (§5).
- **Hit-test forwarding (structural half only)**: a child positioned to receive a hit; assert the hit reaches it through `RenderLeaderLayer` unmodified (no shape gate, matching `RenderProxyBoxMixin` defaults) and through `RenderFollowerLayer` **at its own layout-relative position** (documenting, via the test's own name/comment, that this covers only the "has a child, forwards" structural half — resolved-transform-aware hit-testing is explicitly out of this plan's scope, §8, and the test must say so rather than silently implying full parity).
- **Diagnostics**: `assert_descendant_properties` for `link` (Leader) and `link`/`show_when_unlinked`/`offset`/`leader_anchor`/`follower_anchor` (Follower).
- **Catalog guard**: add `"RenderLeaderLayer"`/`"RenderFollowerLayer"` to `RENDER_OBJECT_TYPES` (`render_object_harness.rs:133-161`, alongside the `RenderShaderMask`/`RenderBackdropFilter` rows at `:160-161`), register `mod leader; mod follower;` + `pub use leader::*; pub use follower::*;` in `crates/flui-objects/src/proxy/mod.rs` (alongside the existing `mod shader_mask;`/`mod backdrop_filter;` at `:1,9` and their `pub use` at `:11,19`), and add both names to the flat re-export in `crates/flui-objects/src/lib.rs`.

**Explicitly out of harness scope, needing real multi-frame/engine-level testing** (do not fake-pass these at the render-object harness level):
- **Actual on-screen follower positioning** (§4's ancestor-chain-sum resolution) — needs a `flui-engine`-level test asserting the rendered pixels/transform match `FollowerLayer::calculate_offset`'s output for a real leader+follower pair, including the cross-repaint-boundary case (leader and follower under different `Layer::Offset` ancestors) — this is a `flui-engine` test suite concern, not this pass's `render_object_harness.rs`.
- **Resolved-transform hit-testing** — blocked on §4.4/§8's ADR; no harness test should assert this "works" until the mechanism exists, per AGENTS.md's "no fake-passing" rule (a test that only checks the has-child structural forward, as sketched above, is the honest ceiling for this pass).
- **`showWhenUnlinked`/leader-never-mounted/leader-unmounted-mid-lifetime semantics** — genuinely needs the §4 registry resolution to exist first (the render-object level cannot observe "is my leader currently linked" at all today); flag as deferred rather than asserting a behavior that isn't wired up.

## 7. Traps a naive port would fall into

1. **Copying the sibling ShaderMask/BackdropFilter pair's `if ctx.child_count() == 0 { return; }` gate onto Leader/Follower.** Oracle pushes both layers **unconditionally** regardless of child (§1.1/§1.2/§3.1) — a childless `CompositedTransformTarget`/`Follower` (a common real pattern: an invisible anchor widget) must still push its layer.
2. **Copying `self.has_child` into `always_needs_compositing()`.** Oracle's `alwaysNeedsCompositing => true` is **unconditional** for both (`:4498-4499`, `:4656`) — a real, confirmed contrast with the immediately-preceding pair's data-dependent version (§2.7 of that plan). Gating on `has_child` here silently disables compositing for a childless Leader/Follower that still needs its own layer.
3. **Treating the debug-only `_debugCheckLeaderBeforeFollower` assertion as a hard runtime requirement and building a same-descent, tree-order-dependent resolution scheme around it.** The actual mechanism (§1.3, §4) is order-independent *because* Flutter's compositing is a separate, later pass over an already-complete retained tree — not because paint order happens to line up. A design that tries to resolve a Follower during the SAME single recursive visit that also visits its Leader (rather than after the whole pass completes) will silently break for the common cross-`Overlay` case, exactly the scenario the task called out.
4. **Assuming FLUI's `FollowerLayer::calculate_offset` covers the unlinked case.** It does not (§2.2, §1.2's "double-duty `offset` field") — the unlinked fallback (`show_when_unlinked=true`, no leader currently registered) must be handled as a **separate branch** at resolution time (§4's render-time special-case, mirroring where oracle itself puts this exact branch — inside `FollowerLayer.addToScene`, `layer.dart:2857-2865`, not inside `RenderFollowerLayer.paint`), using the render object's own `offset` field directly as a plain paint-origin-relative position, not routed through `calculate_offset` at all.
5. **`RenderFollowerLayer::hit_test` self-caching a resolved transform via `Cell`/interior mutability as a shortcut.** Paint in FLUI is `&self` (immutable) by design, and even with interior mutability, whatever value would be cached during THIS node's own paint visit is only as fresh as the single-pass ordering allows (§3.2/§6) — a same-descent self-cache is not more correct than the "structural only" Tier-1 hit-test body, just more likely to be mistaken for correct. Don't build it without also building §4/§8's actual resolution channel.
6. **`RenderBox::hit_test()`'s trait default is leaf-shaped** (`ctx.is_within_own_size()` alone, `render_box.rs:171-173`) — both new structs must explicitly override it (Leader: plain forward, no shape gate, matching `RenderProxyBoxMixin` defaults exactly, §1.1; Follower: the has-child forward sketched in §5, with the resolved-transform half honestly deferred).
7. **Reporting "Follower now positions correctly relative to its Leader" once §3–§5 land.** That claim is **false** until §4's render-time resolution (a real, separate `flui-engine`/`flui-layer` follow-up) exists — this pass makes the `LayerTree` node structurally correct and harness-verifiable, exactly like the ShaderMask/BackdropFilter precedent's own "structurally correct, visually not yet" scoping. Say so explicitly; don't imply completeness the render-object level cannot check.

## 8. Deferred, documented — including the piece that needs an ADR, not a plan

- **§4's render-time ancestor-chain-sum resolution (`flui-layer` + `flui-engine`)** — real, scoped, sketched above, **not** ADR-level: parent pointers already exist on `LayerTree`; the only new surface is a small utility (`fn resolve_follower_offset(tree: &LayerTree, leader_id: LayerId, follower_info: &LeaderInfo, follower_size: Size<Pixels>) -> Offset<Pixels>`-shaped, living in `flui-layer` alongside `LinkRegistry`) plus the `render_layer_recursive` special-case mirroring the existing `Layer::BackdropFilter` one. A good, well-bounded next PR once this render-object slice lands.
- **A same-frame `LinkRegistry` populated by the `FragmentComposer` and threaded through `Scene::with_links` instead of today's `Scene::new`** (§4.3) — mechanical, no `PipelineOwner`-level cross-frame persistence needed for the *rendering* half; this reclassifies the prior pass's framing of "does `PipelineOwner` need to own a `LinkRegistry` across frames" down to "no, a fresh per-frame one built alongside the `LayerTree` is sufficient," **for rendering**.
- **Resolved-transform-aware hit-testing for `RenderFollowerLayer` — genuinely ADR-level, reclassified explicitly, not forced into this plan.** `hit_test_transform(&self, size) -> Option<Matrix4>` has no external-context parameter; `PipelineOwner::hit_test` has no coupling to any `LayerTree`/`LinkRegistry`; no `RenderId↔LayerId` correlation exists anywhere in FLUI today (confirmed absent by grep, §4.4). Closing this gap means choosing, at a chief-architect level, among real trade-offs: (a) thread the last-completed frame's resolved-offset data into the hit-test walk (mirrors Flutter's own accepted "one-frame-stale is fine" answer, `getLastTransform()`, §1.3); (b) add a `RenderId→LayerId` correlation and cache resolved transforms on `RenderState` after each render pass, read generically by the existing hit-test walk instead of through `hit_test_transform`; (c) something else entirely. This is exactly the shape of question ADR-0013 existed to answer for `RenderAnimatedSize`'s self-dirty handle, and the Semantics family's own reclassification earlier this session — surfaced here rather than silently resolved with an unjustified pick.
- **`showWhenUnlinked` = false hiding a follower's hit-testing entirely, and "leader never mounted"/"leader unmounted mid-lifetime" semantics** — depend entirely on §4's resolution existing first; not implementable, let alone testable, at the render-object level today.
- **Semantics** (no semantics tree in FLUI yet) — consistent with every other catalog entry.

### Critical Files for Implementation
- `crates/flui-rendering/src/context/paint_cx.rs` (new `with_leader`/`with_follower` methods; `FragmentScope::Leader`/`FragmentScope::Follower` variants)
- `crates/flui-rendering/src/pipeline/owner/paint.rs` (`scope_layer` new match arms producing `Layer::Leader`/`Layer::Follower`; `FragmentComposer` — the natural home for the per-frame `LinkRegistry`-population side-effect sketched in §4.3)
- `crates/flui-objects/src/proxy/leader.rs` (new — `RenderLeaderLayer`)
- `crates/flui-objects/src/proxy/follower.rs` (new — `RenderFollowerLayer`)
- `crates/flui-layer/src/link_registry.rs` / `crates/flui-layer/src/scene.rs` (existing — `LinkRegistry`, `Scene::with_links` escape hatch already present; read, and extended by the §4.3 follow-up, not by this pass)
- `crates/flui-engine/src/wgpu/renderer.rs` (`render_layer_recursive`'s existing `Layer::BackdropFilter` special-case, `:1539-1552` — the precedent shape the deferred `Layer::Follower` resolution follow-up must mirror; read, not modified, by this plan)
- `crates/flui-objects/tests/render_object_harness.rs` (catalog registration; `harness_shader_mask_*`/`harness_backdrop_filter_*` structural template; `run.structure()`/`LayerTree::iter()` facilities already sufficient)
