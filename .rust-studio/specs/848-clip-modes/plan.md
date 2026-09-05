# #848 — honour the clip layer's `Clip` mode in the wgpu backend

## What is actually wrong (verified by reading, not from the issue text)

The issue says the mode is discarded. It is worse than that: **both clip kinds
ignore the mode, and each is wrong in a different direction.**

| call | today's path | today's edge | correct for |
|---|---|---|---|
| `push_clip_rect` | `state_stack::clip_rect` → hardware scissor (integer pixels) | always **hard** | `HardEdge` only |
| `push_clip_rrect` | coarse scissor + `current_rrect_clip` → SDF | always **anti-aliased** | `AntiAlias` only |
| `push_clip_path` | `painter.clip_path` | (unexamined) | — |

The reason is `clipAlpha` in `shaders/common/clip.wgsl`: it ends in
`sdfToAlpha(dist)`, which is `1 - smoothstep(-edge, edge, dist)` with the edge
width taken from the screen-space gradient. There is **no hard-edge branch in
the shader at all**. So a rrect clip cannot currently be hard, and a rect clip
cannot currently be soft.

`Clip::None` is genuinely unaffected — a render object choosing it pushes no
clip layer, so nothing reaches these functions.

## Shape of the fix

1. **Thread the mode.** `push_clip_rect` / `push_clip_rrect` / `push_clip_path`
   (`wgpu/backend.rs:1465-1481`) currently bind it as `_clip_behavior`. Pass it
   through `painter/transform_clip.rs` into `state_stack.rs`.
2. **`rect` + `AntiAlias`** routes to the SDF path instead of the scissor — a
   rrect with zero radii. The scissor is integer-aligned and cannot feather.
   Keep the coarse scissor as an early-reject, as `clip_rrect` already does.
3. **`rrect` / superellipse + `HardEdge`** needs a hard threshold in the shader:
   `step(0.0, -dist)` in place of `sdfToAlpha(dist)`. Cleanest as an orthogonal
   `clip_hard` flag rather than new `clip_kind` values, because `clip_kind`
   already selects the *distance function* (0 none, 2 superellipse, else rrect)
   and overloading it would multiply the cases.
4. **`AntiAliasWithSaveLayer`** — **implementable, not a stub.** Checked: the
   backend already has a real offscreen path (`painter/layer.rs::save_layer_impl`,
   with a compositor, group opacity, blend-mode propagation and a
   `LayerFilterChain`). So the mode means: open a save-layer, render the subtree
   into it, composite it back through the anti-aliased clip — which is exactly
   what the machinery does for opacity and colour filters already. The issue's
   "documented interim" escape is therefore NOT the right answer here; it was
   written before this was checked. Ship it, or say precisely which part of the
   existing offscreen path does not compose with a clip.
5. **Per-mode readback oracles**, one per mode, in
   `wgpu/clip_layer_readback_tests.rs` — which already owns
   `a_clip_rect_layer_clips_its_content_and_its_absence_does_not` and
   `an_aliased_paint_hardens_the_edge_the_default_smooths`, so the harness and
   sampling discipline are there to copy.

## What makes this verifiable here

Readback tests **run locally on this machine** with a real GPU — confirmed:
`cargo nextest run -p flui-engine readback` passes 3/3, including two real
clip-pixel tests. So this does not depend on CI's WARP `gpu-test` job to
develop, only to confirm.

## Traps this repo has already recorded for GPU oracles

From `gpu-oracle-sample-points-must-discriminate`: three ways a readback test
goes green against both the fixed and the broken code — rotation about the
centre, the SSAA area gate, and framebuffer rebases. A per-mode oracle must
sample a point whose coverage **differs** between hard and soft, i.e. a pixel
straddling the boundary, and must assert on partial coverage rather than
"clipped vs not".

Related: `stroked-shapes-bypass-the-instanced-sdf-path` (only Fill+SrcOver
reaches the SDF) and `sdf-clip-is-axis-aligned-only` (rotation falls back to the
scissor — which means a rotated `AntiAlias` clip may not be honourable without
further work; establish that before promising it).

## Order

1. Read `SaveLayer` support to settle item 4 — it decides how much of this ships.
2. Shader flag + `clipAlpha` branch, with an SDF unit test.
3. Thread the mode through painter → state_stack.
4. Route rect+AntiAlias to the SDF.
5. Readback oracles per mode, each verified red against the current behaviour.
6. Update ADR-0054's "Status of the decisions" entry, which records this as a
   named gap.
