# ADR-0047: Partial repaint needs cross-frame layer identity, not paint-phase bookkeeping

*Damage cannot be derived from which render objects repainted, because the ones
that always repaint cover the screen. It has to come from comparing consecutive
layer trees, which needs layers to be identifiable across frames — they are not.*

---

- **Status:** Accepted
- **Date:** 2026-08-16
- **Deciders:** @vanyastaff
- **Scope:** `flui-layer`'s `DamageTracker` and `DamageRegion`; the scissor path
  in `flui-engine`'s `Renderer::render_scene`; `PipelineOwner::run_paint`'s
  retention bookkeeping; `LayerNode::element_id`.

---

## Context

`DamageTracker` has existed for some time, complete with Slint-style multi-rect
merging, and `render_scene` already consumes it: `damage_rect()` becomes a
scissor, with a self-heal pass that promotes the next frame to a full repaint
when an advanced shape straddles the damage edge.

Nothing produces damage. `Renderer::mark_dirty` has no production caller, so
every path calls `mark_full_repaint()` and the scissor never narrows. The code
says so itself, anticipating that *"when flui-view is wired up, widgets will
call `mark_dirty(bounds)` on state change"*.

### What it is worth

Measured in `flui-engine`'s `damage_scissor` benchmark (PR #756), at
1920×1080 with the damage rect fixed at 128×128 — 0.8% of the surface:

| layers | full | damaged | ratio |
|---|---|---|---|
| 4 | 241 µs | 43 µs | 0.18 |
| 16 | 810 µs | 46 µs | 0.06 |
| 64 | 2901 µs | 56 µs | 0.02 |

The scissored cost is nearly flat while the full cost grows with fragment work.
A partial repaint makes a frame cost what changed rather than what exists. For
scale, `run_paint` on a 1000-boundary tree costs 193 µs, so the GPU side at 16
layers is already several times the CPU paint.

The layers there are translucent full-surface rects — the shape that generates
fragment work and the shape a scissor can cull — so those are an upper bound
per layer, not a forecast. The direction is not in doubt.

## Decision

**Damage is not derived from the paint phase.** It will come from comparing the
layer tree a frame produces against the previous frame's, which requires giving
layers an identity that survives a frame boundary. Until that lands, every frame
stays a full repaint and `DamageRegion` keeps its single `Full` variant.

## Why the obvious approach does not work

The anticipated design — the paint phase reports the bounds of what it
repainted — collapses.

`run_paint` descends from the root and repaints everything that is not a
retained repaint boundary (ADR context: PR #755 added that retention). Inline
content, meaning everything not under a `RepaintBoundary`, is repainted on every
frame unconditionally. In any real application that includes the background and
the app bar, so the union of repainted bounds is the whole surface, and the
scissor would narrow to nothing.

Per-item repaint boundaries in lists, grids, and page views (PR #757) make the
*items* retainable, which is what retention needed. They do not make the chrome
around them retainable, and the chrome is what covers the screen.

Extending retention to any clean subtree rather than explicitly marked
boundaries would fix that, and is what would have to change for a paint-derived
damage to work. It is a larger change than damage itself: every retained subtree
needs its own layer, which is precisely the cost `RepaintBoundary` exists to
make explicit.

## What the alternative needs

Comparing consecutive layer trees needs two things. One exists, one does not.

**Cheap comparison — exists.** `PictureLayer` holds `Arc<DisplayList>` since
PR #755, and a grafted boundary re-inserts clones of the same layers, so the
`Arc` is pointer-identical across frames. `Arc::ptr_eq` therefore answers "is
this content unchanged" in constant time, with no command-list walk.

**Pairing layers across frames — does not exist.** Each frame builds a fresh
`LayerTree` whose slab indices differ, so `LayerId` cannot pair anything.
`LayerNode` carries an `element_id: Option<ElementId>` field that looks made for
this, but `LayerTree::insert_with_element` is never called from the paint path —
the field is always `None` in production. Populating it, or an equivalent
`RenderId`, on the layers a boundary produces is the missing piece.

## Consequences

- `DamageRegion` stays `#[non_exhaustive]` with only `Full`, and
  `raster_owner`'s comment about revisiting when a `Partial` variant lands stays
  accurate.
- `Renderer::mark_dirty` stays without a production caller. It is not dead code
  to be removed: the consuming half of damage is written, tested, and correct,
  and this ADR records what the producing half requires.
- The `damage_scissor` benchmark stays as the baseline any producer must beat.
- Whoever builds the producer should start by populating layer identity for
  retained boundary subtrees, since that is the only part with no existing
  mechanism.
