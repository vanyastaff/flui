[← Flutter pipeline study](2026-06-10-flutter-rendering-pipeline-study.md) · [Beat-Flutter plan](2026-06-08-beat-flutter-plan.md) · [ADR-0002](../adr/ADR-0002-engine-wide-threading-architecture.md)

# Rendering Design Amendments — modern/engineering pass

> **Provenance.** The Flutter-pipeline study fixed the *semantics* we must
> honor. This document amends its *mechanisms* after (1) a state-of-the-art
> sweep of 2024-2026 renderer architectures (Vello, WebRender, Impeller,
> GPUI, Slint, Makepad), (2) a Rust engine-patterns sweep (Taffy, salsa,
> sans-IO, typestate limits, slotmap, GhostCell, profiling), and (3) an
> adversarial critique of the study's Cycle A/B/C design that found two of
> its decisions wrong against our own code. Items below supersede the study's
> §1.4/§5 where they conflict.

---

## D1 — Paint model: sans-IO DisplayList builder, NOT a PaintingContext port

**The study's "paint_raw mirroring the U19 layout bridge" is withdrawn.**
Two independent grounds:

1. *Critique (verified in code):* the U19 saga existed because layout takes
   `&mut self` and needs `&mut` children — hence SubtreeBorrows/NodePtr/MIRI.
   **Paint takes `&self`** (traits/render_box.rs:319) and the existing
   `PaintChildCallback` walk (context/paint.rs:54, owner.rs:2204) is safe
   shared-borrow recursion with no raw pointers. Porting the Proxy/erased-GAT
   shape onto paint imports complexity its borrow structure does not need.
2. *Convergence:* Vello (encoded streams, `Scene::append`), WebRender
   (serializable display list, scene/frame split across threads), Flutter
   itself (immutable DisplayList replacing SkPicture, R-Tree culling at
   record time), Makepad (draw lists) — every modern engine treats **paint
   output as an immutable value, separated from rasterization**. The sans-IO
   pattern (canonical in Rust networking) is the same principle; wgpu's own
   trace-replay and Vello's headless snapshot tests are its rendering
   incarnation.

**Decision.** Paint is a pure encoder pass:

```rust
fn paint(&self, ctx: &mut PaintCx<'_, Self::Arity>);
// PaintCx = display-list builder + child composition. ZERO GPU access,
// zero recording state machine. ctx.paint_child(i) appends the child's
// fragment under an offset transform (offset read from the child's
// RenderState — see D5). Leaf arity has no paint_child AT ALL.
```

What this buys, concretely:

- deletes the CanvasContext recording state machine (~690 lines:
  `is_recording`/`layer_stack`/`stop_recording_if_needed`) and its doc-only
  "canvas may dangle after children" footgun — the borrow checker now
  enforces what Flutter enforces by prose;
- **paint becomes the Send job ADR-0002 wants**: control plane snapshots the
  immutable tree, data plane produces `DisplayList` fragments in parallel per
  repaint-boundary subtree (Vello `Scene::append` model), value moves back —
  parallel paint falls out of the design instead of being a Phase-2 project;
- snapshot/golden tests without a GPU device (sans-IO);
- repaint-boundary caching = memoizing a function output in a node-owned
  handle (RAII) — no external keyed map (see D2);
- record-time bounds on every op, propagated downstream without
  recomputation (Impeller's documented hotspot, flutter#142054).

Flutter semantics retained: the three-way `paint_child` branch
(boundary / was-boundary / inline), `WAS_REPAINT_BOUNDARY` written pre-paint,
deepest-first dirty order, paint-must-not-redirty as a typed debug check.

**Pre-decision required before building:** paint-time caches (TextPainter's
lazy reshape) must use interior mutability (`Cell`/`OnceCell`) on the render
object — paint stays `&self`. Decide per-cache, never widen paint to
`&mut self`.

## D2 — Generational `RenderId` is prerequisite #0

`RenderId` is a bare `NonZeroUsize` (flui-foundation id.rs:112) over a slab
that reuses slots (tree.rs:477) — ABA. Any `RenderId`-keyed retained cache or
async wake silently targets the wrong node after scroll-driven removal+insert
(stale pixels under a different widget; wrong-node dirty marks).
`ElementId` already packs a generation (id.rs:10).

**Decision.** Make `RenderId` generational (or move RenderTree to
`slotmap`, which also gives `SecondaryMap` hot/cold attachment for free —
evaluate cost in the PR). Mechanical, small, lands **before** any keyed
structure. The study's "RenderId-keyed retained-layer cache seam in Cycle A's
signature" is **dropped**; per-boundary retention is a node-owned
`LayerHandle`/fragment handle on `RenderState` (RAII — dies with the node),
and cross-frame retention design stays the gated item it already was.

## D3 — Dispose inversion: removal is the dispose site, `Drop` is node-local

The study said "`Drop` subsumes dispose" — backwards. `Drop` has no
`&PipelineOwner`: a dropped node cannot evict its dirty-queue entries or
release owner-side state. Today's paint walk *tolerates* orphan dirty entries
(owner.rs:2160-2167); retained anything turns that tolerance into dangling
references.

**Decision.** `RenderTree::remove_shallow/remove_recursive` become the
dispose protocol: they hold `&mut tree`, take `&mut DirtySets`, evict the
id from all queues, release the node's retained handles, THEN free the slot.
`Drop` handles only node-local resources (decoded images, shaped text,
GPU handles) — true RAII scope. Flutter's attach-re-enqueue rule
(object.dart:2477-2503) still ports as designed.

## D4 — `RepaintHandle` full spec (closes the critique's idle-wake hole)

`PipelineOwnerHandle::request_mark_dirty` (handle.rs:141) enqueues but never
wakes; `drain_pending_dirty` runs only at `run_layout` start. A decode
finishing while the app is idle would never appear — the exact "GIF frozen
until you scroll" bug Cycle C claims to kill.

**Decision.** `RepaintHandle` = { generational `RenderId` (D2), dirty-kind,
sender, **wake capability** }. Send path: enqueue → `wake_frame()`
(flui-app binding chain). Drain path: validate generation; stale → silent
no-op (ADR-0002 §77 fallible-writeback rule). Handle is `Send + Sync`
(data plane completes decodes), `Clone`, and RAII-revoked on node removal
via D3.

## D5 — ParentData: split the problem; offsets are already solved

Critique finding: the child offset is **already authoritative** on
`RenderState` as `AtomicOffset` (storage/state/offset.rs:90-114); the
`Vec<Offset>` copies in flex.rs:88/stack.rs:206 and the `child_offset(i)`
trait method are the duplication.

**Decision.**
1. Offsets: paint/hit-test read the child's `RenderState.offset`;
   delete parents' `Vec<Offset>` duplicates AND `child_offset(i)`.
2. Erased `Box<dyn ParentData>` only where non-offset payload exists
   (Flex factor/fit, Stack positioning, Table) — created by the parent's
   `create_child_parent_data()` at adopt, downcast once in the blanket
   bridge. Not a uniform per-child allocation.
3. **Reparent rule (answered, not deferred):** on adopt, if stored PD
   `TypeId` ≠ new parent's PD type → unconditional replace. Flutter's
   "keep richer subtype" `is!` check is a micro-opt that breeds reparent
   bugs; do not port. Test fixture: Flex→Stack reparent.

## D6 — Adopted from the state-of-the-art sweep

| Item | Source | When |
|---|---|---|
| Record-time bounds on every display-list op, never recomputed downstream | Impeller lesson (flutter#142054) | D1 PR |
| `profiling` crate spans (layout/paint/compositing) + `wgpu-profiler` timestamp queries → Tracy | Bevy precedent; zero cost disabled | now (cheap, retrofit expensive) |
| Hot/cold split: layout geometry as separate arrays (`SecondaryMap` if slotmap) | partial-SoA; full SoA rejected at UI scale (working set fits L2) | with D2, bench-gated |
| Per-type primitive batching kept SoA-form CPU-side before upload | GPUI model; engine already has instanced shaders | engine-side, verify in Cycle A test |
| Buffer-age-aware damage (`dirty_region_history[N]`) | Slint; avoids their bounding-rect bottleneck because our boundaries own bounds | after retained-layer design |
| Taffy-style `TraversePartialTree` marker discipline (layout algorithms type-enforce "immediate children only") | Taffy 0.4 unlock for Bevy/Dioxus/Zed | Cycle B, with the cache work |
| Multi-thread fragment assembly (per-boundary DisplayList encode) | Vello `Scene::append`; ADR-0002 data plane | designed-for in D1, enabled later |

## D7 — Explicitly rejected (with reasons, so we don't relitigate)

- **Salsa/incremental for per-frame layout** — per-query bookkeeping
  dominates at 60 Hz UI node counts; no production UI uses it for layout.
  (Style-cascade incrementality may revisit.)
- **GhostCell/qcell** — closure brand poisons async + vtables (wg-async
  "Barbara" story); Xilem's tree_arena structural exclusivity is the
  pattern we already follow.
- **WebRender full interning + per-tile quadtrees** — browser-scale
  machinery; repaint boundaries capture our change locality.
- **GPUI "no retained layers, redraw world"** — right for bounded editor
  UIs, wrong for arbitrary widget trees with clip/save-layer nesting.
- **Impeller's DL→Aiks→Entity three-hop** — their team is deleting Aiks;
  we go display-list → wgpu directly.
- **Full-typestate dirty×phase×protocol products** — embedded-HAL community
  walked this back; typestate stays linear (pipeline phases only).
- **Cycle A retained-cache seam over non-generational ids** — dropped per D2.

## D8 — Gaps promoted into cycle DoDs (cheap now, structural later)

- **Transform symmetry:** one transform accessor consumed by BOTH paint and
  hit-test (inverse); hit-test-under-transform integration test in Cycle A
  DoD. (Two existing objects already have this bug class: transform.rs,
  fitted_box.rs.)
- **DPR** threaded through paint+layout contexts (text shaping needs it;
  "intrinsics are pure over the shaper" is false if DPR floats).
- **Resize-mid-frame rule:** `set_root_constraints` → relayout → repaint
  atomicity defined in Cycle B lifecycle work.
- **Selection state home** decided before TextPainter split freezes
  (render-object-owned, Flutter-style), even if implementation defers.

## Revised sequencing

```
D2 generational RenderId (prereq #0, mechanical)
  → Cycle A′: PaintCx DisplayList builder (D1) + three-way paint_child
              + needs_compositing wiring + damage/wake fixes (flui-app)
              + transform-symmetry test (D8) + profiling spans (D6)
              Exit: colored box через настоящий View→DisplayList→wgpu,
              снапшот-тест без GPU.
  → Cycle B′: D3 dispose inversion + lifecycle + D5 ParentData split
              + flex fixes (clamped free_space, Stretch, unbounded-main,
              MainAxisSize — NOT the refuted spacing change) + caches.
  → Cycle C′: D4 RepaintHandle + TextPainter shaped/paint split
              + BoxDecoration painter + RenderImage.
```

A-before-B holds: paint reads offsets from `RenderState.offset` (D5.1), so
Cycle A′ does not block on B′'s ParentData work.
