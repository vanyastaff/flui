# 536 slice 2 — produce damage from the comparison

Slice 1 (#967) gave boundary layers a `RenderId` that survives the frame
boundary. Nothing consumes it yet. This is the plan for what does; scoping
only, verified against the tree 2026-09-07.

## What already exists, end to end

The consumer side is **complete and unused**:

- `DamageTracker` (`crates/flui-layer/src/damage.rs`) has `mark_dirty(rect)`,
  `damage_rects()`, `damage_rect()`, `has_damage()`, Slint-style multi-rect
  merging.
- `WgpuRenderer` holds one and already **skips the whole frame** when it
  reports nothing: `if !has_damage() && !needs_full_repaint() { return
  Ok(false) }` — no present, no vsync block.
- The scissor path and its self-heal exist: a frame that detects an advanced
  shape straddling the damage edge sets `force_full_repaint_next_frame`.
- ADR-0061's measurement, at 1920×1080 with a 128×128 damage rect: 0.18× the
  cost at 4 layers, 0.06× at 16, 0.02× at 64.

The producer side is one line: `crates/flui-engine/src/raster_owner.rs`'s frame path calls
`self.backend.mark_full_repaint()` unconditionally, and its own comment says
to revisit that "once a `Partial` variant lands".

## Sub-slices, in dependency order

### 2a — WITHDRAWN: the frames it would catch do not exist

**Do not build this.** The premise was tested before implementing and does not
hold.

`PipelineOwner::run_paint` returns early when nothing is dirty
(`if !self.scheduler.has_paint_work() { return Ok(()) }`), so a clean frame
produces **no new `LayerTree`** — and there is nothing retained to compare it
against either: the owner's `last_layer_tree` field holds a tree only between
paint and the `take_layer_tree()` that `run_frame` performs to hand it to the
compositor, so by the time a frame ends it is `None`. Above it,
`flui-app` calls that outcome `FramePaintOutcome::Idle`, "nothing was dirty
this frame; no new content to composite", and its own doc records that such a
frame "never reaches `render_scene`" — established there by a probe rather
than assumed. The skip this sub-slice proposed to add already exists, upstream
and cheaper than a tree walk.

The remaining shape — a frame where something WAS dirty but the tree came out
identical — is not reachable either: the dirty node re-records, which yields a
fresh `DisplayList` allocation, so a comparison keyed on picture identity
correctly reports "changed". Conservative and right, and worth nothing.

This was found by writing the baseline test first — two frames over an
untouched tree, asserting the second renders the same content as the first.
It failed before reaching any comparison: the second frame's `run_frame`
returned `Ok(None)`, which is the whole answer. That test was reverted with
the rest of the 2a implementation, so it is described here rather than cited;
`FramePaintOutcome`'s own doc in `crates/flui-app/src/app/ui_realm.rs` is the
committed record of the same fact.

**A note 2b needs.** Because `run_frame` *takes* the tree, 2b has to retain
the previous frame's tree itself — the pipeline does not keep one to diff
against. Whoever holds it (the raster lane is the natural place: it is
per-presentation and outlives the frame) also pays for keeping every
`DisplayList` in it alive, which is an `Arc` clone per picture rather than a
copy.

**What this does not invalidate.** A per-boundary comparison is still needed —
by 2b, to decide WHICH boundaries changed. Its shape differs from what 2a
would have built: per boundary, not whole-tree. The
[opacity trap](#the-trap-that-would-freeze-an-opacity-animation--measured-not-predicted)
recorded below applies to it unchanged, and is the reason it cannot key on
picture identity alone.

**So the first sub-slice is 2b.** Damage's value is entirely in narrowing the
scissor for a frame that DID change, which is where ADR-0061's 0.18× / 0.06× /
0.02× measurements come from.

### 2a (withdrawn, kept for the reasoning) — "nothing changed"

Retain the previous frame's `LayerTree` per presentation. Pair the two by
`render_id`; if every boundary pairs AND every `PictureLayer`'s `DisplayList`
is `Arc::ptr_eq`-identical, report no damage and let `raster_owner` skip the
`mark_full_repaint`, so the renderer's existing skip fires.

Worth doing first for three reasons: it needs no bounds arithmetic; its
failure mode is a **frozen screen**, which is visible and catchable by
`just live-smoke`, not silent corruption; and a genuinely static UI stops
rasterising entirely, which is the largest single win available.

`DamageRegion` gains no variant here — `SceneSnapshot.damage` becomes an
`Option`, or `Full` stays and a separate "unchanged" bit rides alongside.
Decide before writing.

#### The trap that would freeze an opacity animation — measured, not predicted

The obvious "nothing changed" test is *every `PictureLayer`'s `Arc` is
pointer-identical*. **It is wrong, and it fails on this issue's own acceptance
criterion #1** ("animated opacity ticks update alpha without repainting the
child subtree").

Take `RenderOpacity` with a repaint-boundary child. Changing the alpha calls
`mark_needs_paint` on the opacity node, so that node repaints — but the child
is its own boundary, so it is not repainted at all: `graft` replays its
captured layers, and the `DisplayList` inside the replayed `PictureLayer` is
the same allocation, so `Arc::ptr_eq` holds. The only field that differs
anywhere in the two trees is the `OpacityLayer`'s alpha. Probed on a real
two-frame run:

```text
picture Arc  frame 1 = 137511296843184
picture Arc  frame 2 = 137511296843184     identical
OpacityLayer frame 1 = alpha 0.5019608
OpacityLayer frame 2 = alpha 0.2509804     changed
```

A picture-only comparison reports "unchanged" and the screen freezes at the
old alpha — silently, on the single most important case the feature exists
for.

So the comparison must include **each layer's own payload**, not just picture
identity: alpha, transform, clip geometry, offset, filter parameters. The
cheap shape is to compare `Layer` values structurally with `PictureLayer`
compared by `Arc::ptr_eq` (its `DisplayList` is the only field a walk would be
expensive on). Confirm every `Layer` variant is actually covered before
relying on it — a variant whose payload is skipped is a frozen frame for
whatever animates through it, and that failure is invisible to any test that
does not animate that specific property.

This also settles a question 2b would otherwise have to revisit: the same
per-layer payload comparison is what distinguishes a boundary that has moved
from one that has not, since an `OffsetLayer`'s offset is exactly the field
that differs there.

### 2b — bounds for a changed boundary

A boundary's root is an `OffsetLayer`, which deliberately has **no** intrinsic
bounds (`crates/flui-layer/src/layer/bounds.rs`: container layers' extent depends on their
children). So a changed boundary's damage rect is the union of its
descendants' `LayerBounds`, accumulated through the offsets and transforms
between them and the root.

Two correctness traps to design for, not discover:
- a boundary that **moved** damages both its old and its new rect;
- a boundary that was **removed** damages its old rect with nothing to walk,
  so the previous frame's bounds have to be retained, not recomputed.

### 2c — `DamageRegion::Partial` and the raster path

Add the variant, make `raster_owner` inspect it, and route the rects into
`RasterBackend::mark_dirty`. Only now does the scissor narrow.

### 2d — the evidence #536's acceptance list asks for

GPU readback proving pixel equivalence between a damaged and a full frame, and
a benchmark showing the reduction. `flui-engine`'s `damage_scissor` bench is
the template; `gpu-test` on WARP is where readback runs.

## The fixture rule this series has earned

Five fixtures in this session passed for the wrong reason. For damage the
dimensions to vary deliberately are: **nesting** (a boundary inside a reused
boundary — the one that already bit), **count** (one boundary pairs under any
rule), **movement** (a boundary that moved, not just changed), and
**removal** (a boundary that is gone in frame two). A fixture missing any of
those cannot distinguish a correct implementation from several wrong ones.
