<!-- Rust Code Studio — feature spec. Acceptance criteria are what /spec-verify checks. -->

# Spec: GPU filters consumer chain (make engine filters reachable end-to-end)

- **Status:** Draft   ·   **Slug:** `gpu-filters-consumer-chain`   ·   **Date:** `2026-06-22`   ·   **Owner:** `chief-architect`
- **Predecessor:** `gpu-image-filters` (DONE — the engine producer; PRs #267-276). Governing decision: vault `adr-filter-consumer-chain`.

## Problem

The `gpu-image-filters` spec shipped a complete GPU **producer**: `ImageFilter::{Blur,Dilate,Erode,Compose}`
on the bounds-growing `DrawItem::Filter` seam, `ColorFilter::{Mode,Gamma,Matrix}` on the bounds-preserving
`LayerFilter` chain, the `ImageFilterLayer`/`ColorFilterLayer`/`BackdropFilterLayer` layer types, and the
engine entry points (`push_image_filter`, `save_layer_with_image_filter`). **But nothing above the engine
produces them** — they are reachable only by hand-building a `LayerTree` (as `examples/filter_demo.rs` does).
The consumer chain is broken at every level above flui-layer:

| Layer | Break (verified) |
|---|---|
| flui-view / widgets | No `ImageFiltered` / `ColorFiltered` / `BackdropFilter` widget exists. |
| flui-rendering | No `RenderImageFiltered` / `RenderColorFiltered` / `RenderBackdropFilter`; `PaintEffectsCapability` has no filter hook; `paint_subtree` wraps only Opacity/Transform. |
| flui-painting | `Paint` has no `image_filter`/`color_filter`; `DrawCommand::SaveLayer{bounds,paint,transform}` carries no filter. |
| flui-layer | `ImageFilterLayer`/`ColorFilterLayer` exist + render, but have **zero producers**; `ColorFilterLayer` wraps a bare `ColorMatrix`, not the full `ColorFilter`. |
| flui-engine | `push_color_filter(&ColorMatrix)` can't express `ColorFilter::Mode`/`Gamma` (which the engine implements); `LayerFilter::Mode`/`Gamma` are `cfg(test)`-only. |

**Goal:** make the engine's filters reachable from the public widget API, improving the cross-crate
architecture. **Breaking changes are allowed** (active dev, no external consumers) — used where the break is
clean, not gratuitously.

## Goals / Non-goals

**Goals**
- A user can apply a Gaussian blur / morphology / compose chain / color filter (Mode/Gamma/Matrix) to a
  widget subtree via dedicated widgets, and it reaches GPU pixels end-to-end.
- Complete the engine producer surface: full `ColorFilter` (not just `ColorMatrix`) through `push_color_filter`.
- Each filter widget carries render-harness + readback/scene-snapshot tests (anti-MVP).

**Non-goals**
- ❌ `Paint.image_filter` / `Paint.color_filter` low-level route (Approach A) — rejected: bloats the
  serialized `DrawCommand`/`Paint` trust boundary, breaks deterministic-replay goldens, forces per-draw
  engine unpacking. Deferred to a future ADR only if a low-level (custom-painter `save_layer`) consumer appears.
- ❌ New engine filter *kinds* — the producer is done; this spec only wires consumers.
- ❌ gpui-style opacity-baking optimization — orthogonal (gpui has no general filters; ruled out as a model).

## Approach (chief-architect ACCEPTABLE; Flutter-idiomatic route, verified vs `.flutter/`)

**Dedicated widgets → proxy render-objects → existing layer-tree filter layers** — the exact structure of
Flutter's `ImageFiltered`/`ColorFiltered`/`BackdropFilter` (`image_filter.dart`/`color_filter.dart`/
`proxy_box.dart`), which flui already half-built (the layer types). `Paint`/`DrawCommand`/serialized wire
format are **untouched**.

- New render-objects (`Single` arity, always-composite + repaint-boundary — the flui analogue of Flutter's
  `RenderProxyBox`): `RenderColorFiltered{filter: ColorFilter}` → `Layer::ColorFilter`;
  `RenderImageFiltered{filter: ImageFilter}` → `Layer::ImageFilter`;
  `RenderBackdropFilter{filter, blend_mode}` → `Layer::BackdropFilter`.
- Seam mechanism: extend `PaintEffectsCapability` with filter hooks (default `None` → additive, non-breaking)
  + one `paint_subtree` arm mirroring the Opacity arm. **Footgun (from advanced-blend memory):** the filter
  arm must fire on the filter hook ALONE, independent of `paint_alpha` (the existing arm silently drops
  `paint_layer_blend` unless `paint_alpha` is Some — the filter arm must NOT replicate that asymmetry).
- Producer-completeness (breaking, internal only): `push_color_filter(&ColorMatrix)` → `&ColorFilter`;
  promote `LayerFilter::Mode`/`Gamma` to production; widen `ColorFilterLayer` to carry `ColorFilter`.

### Alternatives
| Option | Why not |
|---|---|
| **A — `Paint.image_filter` + `DrawCommand::SaveLayer` filter field** | Bloats the serialized closed-enum trust boundary; breaks replay goldens; per-draw unpack. Matches only Flutter's low-level dart:ui surface, not the widget route apps use. Rejected. |
| **gpui-style (effect-as-style-property + opacity stack)** | gpui has NO general image/color filters (opacity + box-shadow blur only — narrow code-editor scope). No abstraction to borrow for general filters. Ruled out. |

## Public surface & semver impact
Active-dev breaking allowed. Breaking edits are **internal trait/layer signatures only — zero serialized
format change** (replay goldens stay valid): `push_color_filter(&ColorFilter)` (B1), `ColorFilterLayer`
field widened to `ColorFilter` (B2). Additive: `PaintEffectsCapability` filter hooks (default None), new
render-objects + widgets. `Paint`/`DrawCommand`/DisplayList wire format unchanged.

## Pre-code maintainer verdict: **ACCEPTABLE**
Filter-on-content owned by render-objects/widgets (compositing concept) producing layers (GPU-bridge concept);
the `Paint`/`DrawCommand` trust boundary is not widened (one fact, one place — the typed filter enum flows,
never duplicated into Paint). Maximal sibling reuse (engine seam + 3 layer types + `PaintEffectsCapability`/
`paint_subtree`/`SceneComposer` + the `RenderOpacity` Single-arity template). Strict-maintainer risks pinned
in acceptance below.

## Acceptance criteria
- [ ] **Full `ColorFilter` through the engine** — `push_color_filter` accepts `&ColorFilter` (Mode/Gamma/Matrix); `LayerFilter::Mode`/`Gamma` are production (not `cfg(test)`); `ColorFilterLayer` carries `ColorFilter`. *(GPU readback: Mode + Gamma reachable via the layer path; Matrix unchanged.)*
- [ ] **Filter render seam** — `PaintEffectsCapability` gains filter hooks (default `None`); `paint_subtree` wraps the fragment in the filter layer when a hook returns `Some`, **firing independently of `paint_alpha`** (regression test for the no-silent-drop invariant). *(harness test that fails without the arm.)*
- [ ] **`ColorFiltered` widget + `RenderColorFiltered`** — applies Mode/Matrix/Gamma to a subtree; always-composites; repaint-boundary. *(render-harness + GPU/scene-snapshot test across ALL THREE sub-modes — Matrix-only ≠ done.)*
- [ ] **`ImageFiltered` widget + `RenderImageFiltered`** — applies Blur/Dilate/Erode/Compose to a subtree. *(harness + readback across the variants; off-origin content not clipped — reuse the grown-bounds lessons.)*
- [ ] **`BackdropFilter` widget + `RenderBackdropFilter`** — blurs backdrop content under the existing renderer intercept (`supports_copy_src || intermediate_active` gate). *(harness + readback under the gate.)*
- [ ] **End-to-end parity** — `examples/filter_demo` (and a new `color_filter_demo`) build via the **widget tree** (not a hand-built `LayerTree`), proving the chain closes public-API→pixels; each widget's edge behavior cross-checked vs `.flutter/` (anti-MVP).
- [ ] **Repaint-boundary reuse** — a filter render-object reuses its layer on offset-only moves (Flutter `updateCompositedLayer` parity), not full repaint.
- [ ] **Gates** — fmt; clippy both modes incl. `--features enable-wgpu-tests` `-D warnings`; nextest; doc `-D warnings`; port-check; the gpu-image-filters readback suite + deterministic-replay goldens stay green (serialized format untouched).

## Risks & open questions
- **Capability-hook alpha-asymmetry footgun** (`paint_subtree` drops `paint_layer_blend` without `paint_alpha`) — the filter arm must not replicate it; encode + regression-test. (vault `verify-guard-premise-before-debug-assert` / advanced-blend memory.)
- **Repaint-boundary / always-composite** — if the filter RO doesn't report repaint-boundary, offset moves repaint instead of reusing the layer.
- **Anti-MVP** — `ColorFiltered`'s three sub-modes each need coverage; shipping Matrix-only = MVP-as-parity.
- **Backdrop ordering gate** — `RenderBackdropFilter` correct only under `supports_copy_src || intermediate_active` (already true for the existing path).
- **Catalog CI guard** — every new `RenderBox` must appear in `RENDER_OBJECT_TYPES` with a `harness_*` test.

## Slicing (dependency-ordered; each an independent `/dev-task` → review → local GPU-readback)
- **T1 — Producer-completeness: full `ColorFilter` through the engine.** `[BREAK B1/B2]` `[SIGN-OFF: api-design-lead]` Widen `push_color_filter`→`&ColorFilter`; promote `LayerFilter::Mode/Gamma`; widen `ColorFilterLayer`→`ColorFilter`; update dispatch + `LayerRender for ColorFilterLayer`. GPU-readback Mode+Gamma via the layer path. *Engine+layer only — critical-path root, self-contained, immediate value.*
- **T2 — Render seam: `PaintEffectsCapability` filter hooks + `paint_subtree` arm.** `[SIGN-OFF: chief-architect]` (hook shape + alpha-asymmetry footgun). Depends on T1.
- **T3a — `RenderColorFiltered` + `ColorFiltered` widget** (Mode/Matrix/Gamma). Depends on T2.
- **T3b — `RenderImageFiltered` + `ImageFiltered` widget** (Blur/Dilate/Erode/Compose). Depends on T2.
- **T3c — `RenderBackdropFilter` + `BackdropFilter` widget** (under the backdrop gate). Depends on T2.
  *(T3a/b/c parallel — disjoint render-objects/widgets sharing the T2 seam.)*
- **T4 — End-to-end demos + `.flutter/` parity verification** (widget-tree-built `filter_demo`/`color_filter_demo`). Depends on T3a/b/c.

## Links
- Predecessor: `gpu-image-filters` spec + `verify-report.md`. Engine seam: `DrawItem::Filter`/`LayerFilter`, `layer_render.rs:447-477`, `traits.rs:441`, `command_ir.rs`.
- Reference (read this design): `.flutter/` `lib/src/widgets/image_filter.dart`, `color_filter.dart`, `rendering/proxy_box.dart` (`RenderProxyBox`, `RenderBackdropFilter`). `.gpui/` ruled out (no general filters).
- Vault: `adr-filter-consumer-chain`, `bounded-filter-intermediate-integer-grid-composite`, `occlusion-cull-was-unsound-in-back-to-front-order`, `flui-future-proof-over-yagni`.
