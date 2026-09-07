# ADR-0062: The paint queue is the cross-pass record; node flags are intra-pass

*A frame that fails partway keeps its queue and loses its flags. So anything a
later pass must know lives in the queue entry — including WHY a boundary is
queued.*

---

- **Status:** Accepted
- **Date:** 2026-09-07
- **Deciders:** @vanyastaff
- **Scope:** `flui-rendering`'s `PipelineOwner::run_paint`, `DirtyTracker`, and
  `pipeline::dirty`'s `PaintQueue` / `PaintEntry` / `PaintKind`.

---

## Context

`run_paint`'s error arm returns **before** `clear_paint_queue()` and before
every commit (retained captures, layer patches, flag clears). That is
deliberate: the root descent is all-or-nothing, so a poisoned `paint_raw`
abandons the whole frame, and the queue has to survive for the retry.

But `paint_subtree_impl` clears node flags **as it walks** — `NEEDS_PAINT` is
cleared before the node paints, which is what makes `mark_needs_paint`'s
idempotence check work and what gives the paint-must-not-redirty assertion its
oracle.

So after a pass that fails partway the two disagree by construction: the queue
names the work, the flags say clean. Any decision that outlives a pass and is
taken from a flag is wrong on the retry.

Issue #536 added a second disposition to the paint phase — a boundary can be
REUSED with one node's effect layers rebuilt, rather than repainted — and that
made the disagreement reachable. Ten review rounds found fourteen defects; the
majority were one shape: a cross-pass decision read from an intra-pass record.
Each was fixed individually, and the next round found the next ordering.

## Decision

**The paint queue is the authoritative cross-pass record, and it carries its
own reason.** `DirtySets::needs_paint` is a `PaintQueue` of

```rust
PaintEntry { id, depth, kind: PaintKind::Repaint | PaintKind::LayerUpdate(targets) }
```

Node flags remain, and remain cleared mid-walk. They answer "did the current
walk visit this node". They do not answer "what does the next commit owe".

Two rules make ordering a non-issue rather than a family of guards:

- `PaintQueue::enqueue` **upgrades** `LayerUpdate → Repaint` and never
  downgrades. A repaint subsumes an update.
- `enqueue` is reached through one write site, `DirtyTracker::enqueue_paint`,
  which owns mid-phase routing *and* the join — the mid-phase queue and the main
  queue can each hold an entry for the same boundary, so an upgrade has to find
  the id in either before pushing a new one.

Consequently update-then-paint, paint-then-update, and a mark arriving after a
failed pass all reduce to the same entry.

## Why the obvious alternatives were rejected

**A side map keyed by boundary** (`layer_update_boundaries`, the first
implementation). Correct only while an invariant — "in the map ⇒ update-only,
queued and not in the map ⇒ repaint" — is preserved at four write sites. Four
of the fourteen defects were a write site that did not. The invariant was also
unfalsifiable at runtime: a `debug_assert` written against the flags could never
fire on the case its own message named, because in that case the flag is false.

**Deferring all flag mutation to commit.** Rejected for three reasons.
`mark_needs_paint`'s `if node.needs_paint() { return; }` is what bounds
invalidation to O(path); with the flag still set mid-walk a mid-paint re-mark
becomes a *silent* no-op instead of being routed to `mid_layout_marks` and
retried. The paint-must-not-redirty assertion loses its oracle entirely. And
`set_was_repaint_boundary` is written for the *next* compositing walk, so it
must survive the current one regardless.

**Putting `kind` on `DirtyNode`.** That type serves the layout, compositing and
semantics queues too, where `PaintKind` is meaningless — it would trade one
illegal state for three — and it is held to two words by
`dirty_node_is_two_usize`, which a `SmallVec` payload breaks.

## Divergence from Flutter, and why it does not port

Flutter's `PipelineOwner.flushPaint`
(`.flutter/packages/flutter/lib/src/rendering/object.dart`, tag `3.44.0`)
**consumes** the queue before walking:

```dart
final List<RenderObject> dirtyNodes = _nodesNeedingPaint;
_nodesNeedingPaint = <RenderObject>[];
```

It then classifies each entry by reading `node._needsPaint` /
`node._needsCompositedLayerUpdate`, and clears both during the walk. Flags are
authoritative there, and soundly so: there is no surviving second record for
them to disagree with, and Flutter iterates boundaries independently
(`repaintCompositedChild` per entry), so one throwing node does not abandon the
rest.

FLUI inverts the authority because it made the opposite choice about failure:
the queue survives so the frame can be retried whole. That predates #536 — #536
is simply the first feature to collide with it.

## Consequences

- A new paint disposition is a `PaintKind` variant, and its precedence is a
  join in `enqueue` rather than a guard at each mark site.
- The classification cannot be observed to disagree with the queue, because
  there is no second record. What remains checkable — and is checked with a
  `debug_assert` at the dereference — is that the position index tracks the
  entries.
- `PaintQueue::append` upgrades on collision, unlike `DirtySet::append`, which
  skips duplicates. Skipping would lose a repaint queued mid-paint.
- The residue scan is driven by a walk-recorded `visited` set rather than by
  `needs_paint()`, for the same reason this ADR exists: keyed on the flag it
  went silent over exactly the entries it is meant to catch, and it is the only
  diagnostic on that path.
- Not addressed here: `is_repaint_boundary()` has two currencies of its own —
  the live trait answer that paint reads and the insert-time storage flag the
  scheduler and compositing walk read. They cannot disagree today because no
  production render object varies its boundary status. Tracked in issue #995.
