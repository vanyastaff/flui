# 536 slice 3 — measurement before building either arm

The slice-3 plan (`slice-3-plan.md`) did not survive its adversarial review.
Rather than pick between the two surviving designs on argument, this measures
the prize both compete for. Measured 2026-09-07 on this machine
(32 cores; benchmark noise floor in this area is ±5%).

## What was measured

`paint/opacity_tick` (`crates/flui-rendering/benches/paint.rs`) over the tree

```text
RenderFlex row
 ├─ RenderRepaintBoundary ── RenderOpacity(0.5) ── row of `subtree` leaves
 └─ RenderRepaintBoundary ── one leaf                     (the sibling)
```

- **`repaint`** dirties a leaf inside the opacity. `RenderOpacity` is not a
  repaint boundary today, so `mark_needs_paint` walks up to the enclosing
  `RenderRepaintBoundary` and the whole subtree repaints. **This is what an
  alpha tick costs now.**
- **`graft`** dirties the sibling instead, so that boundary stays clean and its
  capture is replayed. **This is the floor any update-only design can reach**,
  since patching one layer on top of a graft is O(1).

The sibling exists so the clean arm still has paint work and cannot take
`run_paint`'s nothing-is-dirty early return — the trap that made sub-slice 2a
unbuildable.

## Result

| subtree nodes | repaint | graft | ratio |
|---|---|---|---|
| 1 | 1.11 µs | 1.24 µs | 1.12 (graft *slower*) |
| 10 | 3.27 µs | 1.25 µs | 0.38 |
| 100 | 23.97 µs | 1.13 µs | 0.047 |
| 1000 | 233.5 µs | 1.18 µs | 0.005 |

**The prize is real and large, and it is not slice 2 again.** Repaint scales
linearly with content under the opacity; graft is flat. At 1000 nodes an
update-only commit is ~198× cheaper. Below ~10 nodes there is nothing to win
(and a hair to lose), which is the expected shape, not a defect.

## Why graft is flat — and why this decides the design

A flat number is also what a graft that silently dropped the subtree would
report, so it was verified rather than assumed:
`a_grafted_opacity_subtree_matches_a_full_repaint_at_any_size`
(`crates/flui-rendering/tests/retained_boundary_layers.rs`) asserts the grafted
layer tree is structurally identical to a full repaint's, is non-empty, and
holds **exactly 6 layers at subtree = 1, 10, and 100**. (Confirmed live: the
assertion was temporarily pointed at a wrong value and reported `[6, 6, 6]`.)

The cause: the leaves are inline (non-boundary), so `run_paint` merges their
draw runs into ONE `PictureLayer` sharing an `Arc<DisplayList>`. Grafting
clones an `Arc` instead of re-recording every command. That is the
structural-sharing substrate ADR-0061 named as retention's prerequisite,
doing exactly the work it was added for.

**This favours patching inside the enclosing capture over promoting opacity to
its own repaint boundary.** The `graft` arm above never made the opacity a
boundary — it grafted the *enclosing* `RenderRepaintBoundary`, whose capture is
already 6 layers and already O(1) to replay. Everything the promotion design
would buy is therefore already on the table; all that is missing is patching
one of those 6 nodes with the current alpha. Promoting each `Opacity` to a
boundary would add an `OffsetLayer` and a whole retained capture per node to
reach a position the enclosing boundary's capture already reaches.

It also sidesteps, rather than solves, the review's ship-stopper: promotion
requires `is_repaint_boundary()` to become dynamic, and that flag is
insert-time configuration (`set_repaint_boundary_flag` has one production
caller, `bootstrap_repaint_boundary_flag`, reached only from the four insert
paths; the paint walk reads the live trait answer while the scheduler, the
compositing walk, and `subtree_arena`'s `laid_out` recording read the flag).
Patching inside the enclosing capture never makes it dynamic, so the
flag-coherence work is not a prerequisite — it is only needed if the promotion
design is ever revived, and no production render object has a non-constant
`is_repaint_boundary()` today, so the split is latent rather than a live bug.

## What the next slice has to solve

Carried forward from the review, still true under the patch-in-capture design:

- **The stored capture must be written back.** `graft` takes
  `&RetainedSubtree` and clones; `retained_captures.push` happens only on the
  repaint branch. Patching only the emitted frame leaves the stored capture at
  the old alpha, and a later frame that grafts it for an unrelated reason
  reverts the opacity permanently. Needs a **three**-frame test — tick, tick,
  dirty-a-sibling — because a two-frame test cannot see it.
- **A node's layers can be flattened into several captures.** Nested
  boundaries are flattened into each enclosing capture, so a patch must reach
  every capture holding that render id, not just the nearest one.
- **The count guard is not Flutter's identity assert.** Comparing how many
  effect layers were rebuilt passes when alpha→`None` and transform→`Some` in
  the same frame. Compare the discriminant per position.
- `RetainedNode` carries `offset` and `render_id` besides `layer`; a patch has
  to say what happens to those.
