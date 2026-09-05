> **Status: partly shipped.** Read item 2's strikethrough before following
> this plan — the rect half was implemented, found to cost more than it bought,
> and reverted. The rounded half shipped.

# #848 — honour the clip layer's `Clip` mode in the wgpu backend

## What is actually wrong (verified by reading, not from the issue text)

The issue says the mode is discarded. It is worse than that: **both clip kinds
ignore the mode, and each is wrong in a different direction.**

| call | today's path | today's edge | correct for |
|---|---|---|---|
| `push_clip_rect` | `state_stack::clip_rect` → hardware scissor (integer pixels) | always **hard** | `HardEdge` only — and it STAYS this way, see item 2 |
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
2. ~~**`rect` + `AntiAlias`** routes to the SDF path instead of the scissor.~~
   **TRIED AND REJECTED — this is the opposite of what shipped.** The scissor
   is not only an early reject: text is handed to glyphon with it alone and
   never evaluates the SDF, nested clips share one SDF slot so the inner clears
   the outer, and the SDF's coarse scissor is padded so pixels leak. A rect
   clip therefore stays the scissor under BOTH modes, and `Clip::AntiAlias` on
   a rect is a named gap. See `Painter::clip_rect` for the full reasoning and
   ADR-0054's status entry.
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

## Order — DONE, except where marked

1. ~~Read `SaveLayer` support to settle item 4.~~ Done: the offscreen machinery
   exists (`painter::save_layer_impl`), so `AntiAliasWithSaveLayer` is a
   deferral, not a limitation.
2. ~~Shader flag + `clipAlpha` branch.~~ **Shipped.** The mode rides in bit 2 of
   the existing `clip_kind` lane, so no shader grows a varying.
3. ~~Thread the mode through painter → state_stack.~~ **Shipped**, including
   superellipse, which the first attempt missed.
4. ~~Route rect+AntiAlias to the SDF.~~ **REJECTED — do not do this.** It was
   implemented and reverted: the SDF is a per-instance uniform and the scissor
   is not, so the swap stops clipping text (glyphon gets the scissor alone),
   stops intersecting under nesting (one SDF slot, inner clears outer), and
   stops being exact. A rect clip stays the scissor under both modes.
5. ~~Readback oracles per mode.~~ **Shipped**, each verified red against the
   behaviour it pins.
6. ~~Update ADR-0054.~~ **Shipped.**

### What is actually left for #848

- **Route text through the SDF mask.** This is the root of three separate
  findings on the PR: it is why rect clips cannot use the SDF, why the coarse
  scissor cannot be padded, and why any clip whose precision lives in the SDF
  is invisible to a label. Everything else here is downstream of it.
- **A clip STACK in the shader**, so nested SDF clips intersect instead of the
  inner clearing the outer.
- **`AntiAliasWithSaveLayer`'s offscreen composite**, on top of
  `painter::save_layer_impl`.
- **`discard` for destructive blend modes** — tracked separately as #890, since
  it predates this work.
