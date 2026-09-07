# 536 slice 3 — measurement, and what it changed

The first slice-3 plan (`slice-3-plan.md`) did not survive its adversarial
review, so the prize both surviving designs competed for was measured before
either was built. The measurement then picked the design, and PR review
corrected the measurement twice. All numbers below are from this machine
(32 cores); the noise floor in this area is ±5%.

## The benchmark

`paint/opacity_alpha_change` (`crates/flui-rendering/benches/paint.rs`) over

```text
RenderFlex row
 ├─ RenderRepaintBoundary ── RenderOpacity(0.5) ── row of `subtree` leaves
 └─ RenderRepaintBoundary ── one leaf
```

Both arms mutate the SAME tree by the SAME property through the seam a widget
rebuild uses (`set_opacity` → `RenderUpdateImpact` → `apply_render_update_impact`).
The only difference is that `repaint` additionally marks the opacity node
needing paint, which forces the old path.

`RenderOpacity` is deliberately **not** a repaint boundary — its own effect
layer is addressable inside the ENCLOSING boundary's capture, which is what
lets an alpha change be served without promoting anything.

## Result

| shape | subtree | update | repaint | ratio |
|---|---|---|---|---|
| inline | 1 | 1.18 µs | 1.83 µs | 0.65 |
| inline | 10 | 1.40 µs | 7.67 µs | 0.18 |
| inline | 100 | 1.61 µs | 29.25 µs | 0.055 |
| inline | 1000 | 1.43 µs | 233.7 µs | **0.006** |
| layered | 1 | 0.82 µs | 1.06 µs | 0.77 |
| layered | 10 | 1.91 µs | 5.10 µs | 0.37 |
| layered | 100 | 14.2 µs | 32.2 µs | 0.44 |
| layered | 1000 | 253.6 µs | 346.9 µs | **0.73** |

**The two shapes must be read together.** `inline` leaves merge into ONE
`PictureLayer` sharing an `Arc<DisplayList>`, so the retained capture is a
handful of nodes at any size and the update arm is flat — up to 163× cheaper.
`layered` gives every leaf its own repaint boundary, so the capture's layer
count grows with `subtree` and the update arm grows with it: a graft clones
every captured node. The win there narrows to ~1.4×.

So the win is governed by **how many retained layers the boundary holds**, not
by how many render nodes sit under it. Quoting only the inline number would
overstate the general case.

## Two corrections the PR review forced

Both came from Codex on PR #994 and both changed conclusions, not just wording.

1. **The first benchmark was asymmetric.** Its cheap arm dirtied a *sibling*
   boundary to force a paint pass, so that arm's timing included repainting and
   recapturing a whole extra branch while the expensive arm grafted it. That
   asymmetry produced a reported "1.12× slower at subtree = 1" and the
   conclusion that nothing under ~10 nodes was worth serving. With the
   symmetric fixture the update arm is **faster even at subtree = 1** (0.65×),
   and that conclusion is withdrawn.
2. **`graft` is not O(1) in general.** The original fixture varied only inline
   leaves, which merge into one picture, so `subtree` never increased the graft
   workload and the flat curve was an artifact of the shape rather than a
   property of the mechanism. The `layered` series above exists to characterise
   the real asymptotics.

A third review finding tightened the control test rather than the numbers: its
structural `fingerprint` compared only per-node child counts, so a graft that
preserved topology while emitting the wrong layer variant would have passed.
It now compares layer kind as well. Layer *properties* are still asserted
directly by the tests that care, and pixel equivalence remains the GPU
readback suite's job.

## Why this picked the design

The cheap arm never makes the opacity a repaint boundary — it grafts the
**enclosing** one and rebuilds just the opacity's own layer inside it. So the
alternative design (promote every `Opacity` to a repaint boundary, as Flutter
does) would add an `OffsetLayer` and a whole retained capture per node to reach
a position the enclosing capture already reaches — and, per the `layered`
column, promoting nodes is precisely what makes a graft more expensive.

It also sidesteps rather than solves the review's ship-stopper: promotion needs
`is_repaint_boundary()` to become dynamic, and that flag is insert-time
configuration (`set_repaint_boundary_flag` has one production caller,
`bootstrap_repaint_boundary_flag`, reached only from the four insert paths,
while the paint walk reads the live trait answer). No production render object
has a non-constant `is_repaint_boundary()` today, so the split is latent rather
than a live bug — tracked separately.
