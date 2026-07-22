# `Layer::ShaderMask` engine visual-rendering fix — plan (scoping-verified)

Follow-up to `docs/research/2026-07-01-render-backdrop-filter-shader-mask-plan.md` §2.5/§6, which found and documented that `LayerRender<ShaderMaskLayer>` (`crates/flui-engine/src/wgpu/layer_render.rs:334-354`) pushes an inert `save_layer`/`push_clip_rect` pair that never reads the layer's `shader()`/`blend_mode()` fields — so a `Layer::ShaderMask` node never visually masks anything on screen, even though the `flui-rendering`/`flui-objects` wiring that produces the node is correct and harness-verified.

## Headline verdict

**Classification: Medium — needs a new but narrow reusable primitive, not an architectural change. No ADR needed.** Every GPU-level primitive required already exists and is already proven in production by the sibling `Backend::render_shader_mask` path (the `Canvas::draw_shader_mask`/`DrawCommand::ShaderMask` route, `crates/flui-engine/src/wgpu/backend.rs:773-963`). `Backend`/`WgpuPainter`'s render target has been a late-bound parameter all along (`RenderTarget`, `crates/flui-engine/src/wgpu/render_target.rs`, whose own doc already anticipates "offscreen child paints" as a target kind) — no restructuring of the core render-target model is needed. What's missing is one new, narrow function (`Renderer::handle_shader_mask`, ~100-150 lines) composing existing primitives, mirroring the already-shipped `handle_backdrop_filter` special-case in `render_layer_recursive` — plus one visibility bump and one correctly-derived transform composition.

## 1. Why this is NOT the same technique as `handle_backdrop_filter`

`handle_backdrop_filter` (`renderer.rs:1628-1693`) blurs whatever is **already painted on the surface behind** the layer (mid-frame flush + copy-from-surface + Dual Kawase blur + composite-back via `Backend::apply_backdrop_blur`), then renders children **normally on top, unmodified**. `ShaderMask` needs the opposite data flow: render the layer's **children's own content** to a separate offscreen texture first, then apply the shader as a mask on **that captured content**, then composite the masked result onto the surface. Confirmed by reading `handle_backdrop_filter`/`apply_backdrop_blur` in full — this is a genuinely different technique (capture-then-mask vs. blur-then-overlay), not a copy-paste of the BackdropFilter special case.

## 2. The existing machinery this technique reuses (all confirmed present, not speculative)

`Backend::render_shader_mask` (`backend.rs:773-963`), the sibling path used by `Canvas::draw_shader_mask`/`DrawCommand::ShaderMask`, already does the full six-step "capture subtree → mask → composite" sequence — just for a flat `DisplayList` input instead of a `LayerTree` subtree:

1. Reads the live ambient CTM/DPR off the real painter (`self.painter.current_transform_matrix()`, `current_max_scale()`, `backend.rs:808-809`) and computes device-resolution dimensions for the offscreen capture (`:812-820`).
2. Acquires a device-sized pooled texture for the child capture (`offscreen.texture_pool().acquire(dev_width, dev_height, format)`, `:831-833`).
3. Gets-or-creates a **second, independent `WgpuPainter`** sized to that texture (`Backend::get_or_create_offscreen_painter`, `:230-258`, calling `WgpuPainter::with_shared_device`, `painter/mod.rs:122-169`).
4. Wraps that second painter in a **brand-new `Backend::new(offscreen_painter)`** (`:871`) and dispatches content into it — today via `for command in child.commands() { dispatch_command(command, &mut temp_backend); }` (`:872-874`). **This is the only DisplayList-specific line in the whole pipeline** — the exact substitution point for the layer-tree case.
5. Flushes that temp painter's batches into the pooled texture via `offscreen_painter.render(RenderTarget::sampleable(child_tex.view(), child_tex.texture()), &mut encoder)` (`:908-918`).
6. Applies the shader as a GPU mask against that texture (`OffscreenRenderer::render_masked`, `crates/flui-engine/src/wgpu/offscreen/mask.rs:156-277`), then queues the masked result onto the real painter's draw order at the correct device-space rect via `WgpuPainter::queue_offscreen_result` (`painter/mod.rs:492-504`, `backend.rs:943-944`).

`RenderTarget`'s own doc: *"The `texture` field is `None` for write-only targets (readback helpers, **offscreen child paints**)"* (`render_target.rs:15-17,31-32,52-53`) — confirming this offscreen-composability was designed in from the start. Nothing in `Backend`/`WgpuPainter`'s persistent state (clip stack, transform stack, blend state, draw batches) references "the surface" — `Backend.surface_view`/`surface_texture` are just two `Option` fields used solely by the backdrop-filter mid-frame-flush path (`backend.rs:88-97`), and are `None` in exactly this offscreen-painter construction path today.

`OffscreenRenderer` (`crates/flui-engine/src/wgpu/offscreen/mod.rs`) is a general offscreen-effects toolbox, not blur-only: `mask.rs` (shader-mask pass), `blur.rs` (Dual Kawase blur), `blit.rs` (blit-to-surface), sharing one `TexturePool` and one `ShaderCache` already used by both `apply_backdrop_blur` and `render_shader_mask` — confirming it's already a shared, multi-purpose facility.

## 3. Concrete implementation sketch (primitive-by-primitive, every reference already exists)

**Gate** (add next to the existing `BackdropFilter` gate, `renderer.rs:1544-1558`):
```rust
if let flui_layer::Layer::ShaderMask(sm_layer) = layer && backend.offscreen_mut().is_some() {
    Self::handle_shader_mask(sm_layer, node, tree, link_registry, backend, ctx, surface_texture, surface_view);
    return;
}
```
No offscreen renderer available → fall through to the existing "Normal path" (today's harmless inert clip) — an identical no-GPU degrade to `BackdropFilter`'s own non-`Blur` fallback (`renderer.rs:1644-1662`).

**New `Renderer::handle_shader_mask`**, composed entirely from existing primitives:
1. `bounds = sm_layer.bounds()`, `shader = sm_layer.shader()`, `blend_mode = sm_layer.blend_mode()` (`crates/flui-layer/src/layer/shader_mask.rs:78,83,88`).
2. `ambient_ctm = backend.painter().current_transform_matrix()`, `dpr = backend.painter().current_max_scale()` (both `pub(crate)`, `painter/mod.rs:710,727`, already read this way in `handle_backdrop_filter`, `renderer.rs:1674-1677`). Compute `device_bounds = ambient_ctm.transform_rect(&bounds)` and device pixel dimensions exactly as `backend.rs:812-820` already does.
3. Acquire `child_tex` from `offscreen.texture_pool().acquire(dev_width, dev_height, format)` (`backend.rs:831-833`).
4. `backend.get_or_create_offscreen_painter(&device, &queue, format, (dev_width, dev_height))` (`backend.rs:230-258`) — **needs one visibility change: bump `fn get_or_create_offscreen_painter` from private to `pub(crate)`** so `renderer.rs` can call it. This is the only non-mechanical wiring change needed anywhere.
5. `offscreen_painter.reset_frame_state()` (`painter/mod.rs:203-220`), then `let mut temp_backend = Backend::new(offscreen_painter);`.
6. **The one real design decision (the trap, §4 below): seed `temp_backend` with `push_transform(&(Matrix4::translate(-device_bounds.left, -device_bounds.top) * ambient_ctm))`** using the already-existing `LayerStateStack::push_transform(&Matrix4)` (`traits.rs:424`, impl at `backend.rs:1474-1491`, already exercised for real `Layer::Transform` nodes) — **not** the DPR-only reset `render_shader_mask` uses for its `DisplayList` case (§4 explains why).
7. Recurse: `for &child_id in node.children() { Self::render_layer_recursive(tree, link_registry, child_id, &mut temp_backend, ctx, child_tex.texture(), child_tex.view()); }` — pure parameter substitution (`render_layer_recursive` takes `surface_texture`/`surface_view` as plain parameters; `LayerRender` impls never reference any surface directly, confirmed by grep).
8. `temp_backend.pop_transform()`, drop it (Drop flushes any lazy transform, `backend.rs:507-518`).
9. Flush the offscreen painter to `child_tex` exactly as `backend.rs:880-918` already does (clear pass + `offscreen_painter.render(RenderTarget::sampleable(...), &mut encoder)`).
10. `offscreen.render_masked(bounds, result_size, shader, blend_mode, child_tex.texture())` (`offscreen/mask.rs:156-277`) → masked texture.
11. `backend.painter_mut().queue_offscreen_result(masked_texture, device_bounds)` (`painter/mod.rs:492-504`, already `pub fn`; `Backend::painter_mut` already `pub fn` at `backend.rs:199`).

## 4. The one non-mechanical trap — coordinate frame, not a copy-paste of `render_shader_mask`'s reset

`render_shader_mask`'s `DisplayList` path resets its offscreen painter to identity-plus-DPR-only (`backend.rs:863-870`) because `Canvas::draw_shader_mask` records children into a **fresh** `Canvas::new()` (`crates/flui-painting/src/canvas/drawing.rs:451-452`) — children are recorded in a local, self-relative frame there. `Layer::ShaderMask`'s children in the composed `LayerTree` are **not** local — they are expressed in the same ambient-CTM-relative frame as `bounds()` itself, provable by structural parity with `BackdropFilterLayer` (identical construction path per the prior plan's §2.4/§2.1): `handle_backdrop_filter` reads `bf_layer.bounds()` against the *unmodified* ambient CTM and renders children through that *same, unmodified* backend/CTM with no extra offset math — regression-tested by the CTM-translation-honoring test at `renderer.rs:1948+`.

**A builder who copies `render_shader_mask`'s "reset + DPR-scale-only" setup verbatim for the layer-tree case would silently mis-position (or clip away) any `ShaderMask` that isn't sitting at the coordinate-frame origin** — an easy, shallow-test-passing regression (a test mounting `ShaderMask` at the tree root would pass; one nested under any offset/padding would silently break). The fix (step 6 above) is still 100% pre-existing primitives (`push_transform`, `Matrix4::translate`/`Mul`, both confirmed present in `crates/flui-geometry/src/matrix4.rs:408,623`), just used correctly: seed the offscreen painter with `translate(-device_bounds.origin) * ambient_ctm` so children render into the offscreen texture at the SAME effective screen position they'd occupy on the real surface, just clipped to the texture's bounds.

## 5. Explicit first-cut scope boundary (precedented)

Build the offscreen `temp_backend` via `Backend::new(offscreen_painter)` (no nested `OffscreenRenderer`) for the first cut — a `ShaderMask`/`BackdropFilter` *nested inside* this `ShaderMask`'s children gracefully degrades (unmasked/unblurred), identical to the `DisplayList` path's own already-shipped limitation (`backend.rs:958-962`, "no OffscreenRenderer, rendering child without mask"). A fuller version reborrowing `backend.offscreen_mut()` into `Backend::with_offscreen` for the temp backend is a small, precedented follow-up (the exact sequential-reborrow pattern is already used in `apply_backdrop_blur`, `backend.rs:395-405,459-464,490-494`) — not required to close the base gap.

**Minor non-blocking caveat**: a nested `BackdropFilter`-inside-`ShaderMask`'s mid-frame-flush gate (`ctx.supports_copy_src || ctx.intermediate_active`) is keyed to the real swapchain's capability, not the offscreen child texture's (which always has `COPY_SRC`, `texture_pool.rs:326-329`) — on a COPY_SRC-less adapter with no intermediate active, this would over-conservatively skip blur in that double-nested case. Same class of documented, non-blocking caveat the prior plan already accepted elsewhere. Document, do not fix in this pass.

## 6. Test plan

- A GPU pixel-readback test (mirroring the Follower Tier-2 tests' style, `crates/flui-engine/src/wgpu/renderer.rs` test module, `--features enable-wgpu-tests`) mounting a `Layer::ShaderMask` at the tree root wrapping a solid-colored child, asserting the rendered pixels reflect the shader mask's effect (e.g. a solid-color shader with `BlendMode::SrcIn` should tint/replace the child's color in the masked region).
- **The trap's own regression test**: the same scenario but with the `ShaderMask` nested under a `Layer::Offset`/`Layer::Transform` ancestor (non-zero ambient CTM) — asserting the masked content still appears at the correct on-screen position, not shifted to the origin. This is the test that would catch the coordinate-frame trap in §4.
- A no-offscreen-renderer fallback test (mirroring the `BackdropFilter` non-`Blur` degrade test) confirming the layer falls through to the inert clip path without panicking when `backend.offscreen_mut()` is `None`.
- Confirm existing `harness_shader_mask_*` tests in `crates/flui-objects/tests/render_object_harness.rs` (structural, `LayerTree`-level) still pass unchanged — this fix is engine-only, no `flui-rendering`/`flui-objects` changes expected.

### Critical Files for Implementation
- `crates/flui-engine/src/wgpu/renderer.rs` (add the `Layer::ShaderMask` gate beside `:1544-1558`; new `Renderer::handle_shader_mask` mirroring `handle_backdrop_filter` at `:1628-1693`)
- `crates/flui-engine/src/wgpu/backend.rs` (reuse `render_shader_mask`'s offscreen-capture steps, `:773-963`; bump `get_or_create_offscreen_painter`, `:230-258`, from private to `pub(crate)`)
- `crates/flui-engine/src/wgpu/offscreen/mask.rs` (`render_masked`, `:156-277` — unchanged, reused as-is)
- `crates/flui-engine/src/wgpu/render_target.rs` (unchanged — already the generic offscreen-target abstraction to reuse)
- `crates/flui-engine/src/wgpu/layer_render.rs` (the inert `LayerRender<ShaderMaskLayer>` impl at `:334-354` stays as the harmless no-offscreen-renderer fallback; no longer the primary path once the `renderer.rs` special case lands)
