use std::sync::Arc;

use flui_foundation::geometry::Rect;
use flui_painting::BlendMode;

use super::WgpuPainter;

/// Headless GPU device + queue for painter tests.
fn test_device_and_queue() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
    crate::test_support::test_device_and_queue("Painter Test Device")
}

// ===== Color-readback helpers (BUG 1/2/3 regression tests) =====

/// Format used for all color-readback tests: plain UNorm so the stored bytes
/// equal the sRGB-encoded bytes the shader emits 1:1 (no OETF on store),
/// matching the production surface format chosen by `select_surface_format`
/// and Flutter/Impeller's onscreen convention.
const READBACK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Render `draw` into a `size`×`size` UNorm offscreen target cleared to
/// `clear`, then read back the center texel as `[r, g, b, a]` bytes.
///
/// Mirrors the production frame: the painter records draw commands and
/// `render()` flushes them, including offscreen opacity/ColorFilter layers,
/// onto the offscreen target. The center pixel is well inside any full-size
/// fill so we avoid AA-edge ambiguity.
fn render_and_read_center(
    device: &Arc<wgpu::Device>,
    queue: &Arc<wgpu::Queue>,
    size: u32,
    clear: wgpu::Color,
    draw: impl FnOnce(&mut WgpuPainter),
) -> [u8; 4] {
    let rgba = render_to_rgba(device, queue, size, clear, draw);
    pixel_at(&rgba, size, size / 2, size / 2)
}

/// Render `draw` into a `size`×`size` UNorm target cleared to `clear`, then
/// return the tightly-packed RGBA bytes (`size*size*4`, row stride
/// `size*4`). Use [`pixel_at`] to sample an individual texel. Unlike
/// [`render_and_read_center`] this exposes every pixel so edge/column
/// sampling (e.g. atlas-bleed checks) is possible.
fn render_to_rgba(
    device: &Arc<wgpu::Device>,
    queue: &Arc<wgpu::Queue>,
    size: u32,
    clear: wgpu::Color,
    draw: impl FnOnce(&mut WgpuPainter),
) -> Vec<u8> {
    let (target, target_view) = crate::test_support::create_target(
        device,
        "readback target",
        size,
        size,
        READBACK_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );

    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(device),
        Arc::clone(queue),
        READBACK_FORMAT,
        (size, size),
    );

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("readback clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }

    draw(&mut painter);
    painter
        .render_to_view(&target_view, &mut encoder)
        .expect("painter.render must succeed for readback");
    queue.submit(std::iter::once(encoder.finish()));

    crate::test_support::readback_bytes(device, queue, &target, size, size)
}

/// Sample one RGBA texel from a tightly-packed buffer produced by
/// [`render_to_rgba`].
fn pixel_at(rgba: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
    let off = (y as usize * size as usize + x as usize) * 4;
    [rgba[off], rgba[off + 1], rgba[off + 2], rgba[off + 3]]
}

/// BUG 1 (sRGB double-encode): a mid-tone `Color::rgb(128,128,128)` filled
/// over an opaque target must read back ~128 per channel on the UNorm
/// surface format, NOT ~188.
///
/// On an sRGB target the GPU treats the shader's already-sRGB 0.502 as
/// *linear* and applies the linear->sRGB OETF on store, brightening 0x80 to
/// ~0xBC (188). Primaries (0/255) are OETF fixed points, so geometry tests
/// never caught this — only a mid-tone readback does. This fails on the old
/// sRGB-preferring format and passes on UNorm (Impeller parity).
#[test]
fn midtone_fill_is_not_srgb_double_encoded() {
    use flui_painting::Paint;

    let (device, queue) = test_device_and_queue();
    let px = render_and_read_center(&device, &queue, 64, wgpu::Color::BLACK, |painter| {
        painter.draw_rect(
            Rect::from_xywh(0.0, 0.0, 64.0, 64.0),
            &Paint::fill(flui_painting::styling::Color::rgb(128, 128, 128)),
        );
    });

    for (i, label) in ["R", "G", "B"].iter().enumerate() {
        let v = i32::from(px[i]);
        assert!(
            (v - 128).abs() <= 3,
            "channel {label} = {v}, expected ~128 (UNorm 1:1 store). \
                 ~188 indicates an sRGB target double-encoding the color. \
                 full pixel = {px:?}"
        );
    }
}

/// BUG 2 (opacity-layer premultiplied double-multiply): a translucent rect
/// `rgba(255,0,0,128)` drawn inside a `save_layer` of opacity 0.5 over an
/// opaque WHITE background must composite as premultiplied source-over.
///
/// The offscreen texel is premultiplied (`rgb = 255*0.502 = 128`, `a=128`).
/// Pre-scaled by the group tint `(0.5,0.5,0.5,0.5)` it is `(0.251,0,0,0.251)`;
/// premultiplied-OVER white yields R ≈ 255, G ≈ B ≈ 191. The OLD straight-
/// alpha composite re-multiplies rgb by alpha, dropping R to ~223. So R is
/// the discriminating channel: this fails (~223) before the fix and passes
/// (~255) after.
#[test]
fn opacity_layer_composites_premultiplied() {
    use flui_painting::Paint;

    let (device, queue) = test_device_and_queue();
    let px = render_and_read_center(&device, &queue, 64, wgpu::Color::WHITE, |painter| {
        painter.save_layer(
            None,
            &Paint::fill(flui_painting::styling::Color::WHITE).with_alpha(128),
        );
        painter.draw_rect(
            Rect::from_xywh(0.0, 0.0, 64.0, 64.0),
            &Paint::fill(flui_painting::styling::Color::rgba(255, 0, 0, 128)),
        );
        painter.restore_layer();
    });

    let (r, g, b) = (i32::from(px[0]), i32::from(px[1]), i32::from(px[2]));
    // Premultiplied-OVER white: R ≈ 255 (fixed). The straight-alpha bug
    // gives R ≈ 223. Use a tolerance that excludes the buggy value.
    assert!(
        (r - 255).abs() <= 4,
        "R = {r}, expected ~255 (premultiplied composite). \
             R ≈ 223 indicates the straight-alpha double-multiply bug. pixel = {px:?}"
    );
    assert!(
        (g - 191).abs() <= 6 && (b - 191).abs() <= 6,
        "G,B = {g},{b}, expected ~191. pixel = {px:?}"
    );
}

/// Depth-2 nested opacity: `flush_opacity_layer` recurses correctly.
///
/// Two nested `save_layer` calls each carry opacity 0.5 over an opaque BLACK
/// background. The innermost content is a full-coverage opaque RED rect.
///
/// ## Compositing derivation (premultiplied SrcOver throughout)
///
/// **Inner layer (opacity 0.5):**
/// - Offscreen cleared to TRANSPARENT; RED fill is premultiplied `(1,0,0,1)`.
/// - Group-opacity tint `(0.5,0.5,0.5,0.5)` applied at composite time:
///   effective premultiplied source = `(0.5, 0, 0, 0.5)`.
/// - Composited onto TRANSPARENT outer offscreen (SrcOver premul):
///   outer offscreen = `(0.5, 0, 0, 0.5)` pmul ≡ straight `(1,0,0,0.5)`.
///
/// **Outer layer (opacity 0.5):**
/// - Outer offscreen contains pmul `(0.5, 0, 0, 0.5)`.
/// - Group-opacity tint `(0.5,0.5,0.5,0.5)` → scaled pmul `(0.25, 0, 0, 0.25)`.
/// - SrcOver onto opaque BLACK `(0,0,0,1)`:
///   `dst = src + dst*(1−src.a)` = `(0.25,0,0,0.25) + (0,0,0,1)*0.75`
///   = `(0.25, 0, 0, 1.0)`.
/// - In `[0,255]`: **R ≈ 64, G = 0, B = 0**.
///
/// ## Discriminating power
///
/// | Failure mode                                 | Expected R |
/// |----------------------------------------------|------------|
/// | Recursion dropped — only outer 0.5 applied  | ~128       |
/// | Inner texture leaked / pool not cleared      | ~255       |
/// | Correct depth-2 (this test)                  | ~64        |
///
/// The assertion band `[40, 90]` excludes both failure modes.
#[test]
fn nested_opacity_layers_compose_at_depth_2() {
    use flui_painting::Paint;

    let (device, queue) = test_device_and_queue();
    let center_pixel = render_and_read_center(&device, &queue, 64, wgpu::Color::BLACK, |painter| {
        // Outer group opacity 0.5 — opaque-RGB paint; alpha drives layer opacity.
        painter.save_layer(
            None,
            &Paint::fill(flui_painting::styling::Color::WHITE).with_alpha(128),
        );
        // Inner group opacity 0.5 nested inside the outer.
        painter.save_layer(
            None,
            &Paint::fill(flui_painting::styling::Color::WHITE).with_alpha(128),
        );
        // Opaque RED fills the full canvas (center pixel fully covered).
        painter.draw_rect(
            Rect::from_xywh(0.0, 0.0, 64.0, 64.0),
            &Paint::fill(flui_painting::styling::Color::rgba(255, 0, 0, 255)),
        );
        painter.restore_layer(); // inner → composites at depth-1 offscreen
        painter.restore_layer(); // outer → composites to main surface
    });

    let (r, g, b) = (
        i32::from(center_pixel[0]),
        i32::from(center_pixel[1]),
        i32::from(center_pixel[2]),
    );
    // Depth-2 composite: 0.5 × 0.5 = 0.25 effective opacity → R ≈ 64.
    // Depth-1 only (missed recursion) gives R ≈ 128.
    // Leaked inner texture gives R ≈ 255.
    assert!(
        (40..=90).contains(&r),
        "R = {r}, expected ~64 (doubly-attenuated RED at 0.5×0.5 over BLACK). \
             R ≈ 128 means the inner `flush_opacity_layer` recursion was skipped; \
             R ≈ 255 means the inner offscreen leaked to the outer composite. \
             pixel = {center_pixel:?}"
    );
    assert!(
        g <= 20 && b <= 20,
        "G = {g}, B = {b}, expected ~0 (no green/blue in doubly-attenuated RED). \
             pixel = {center_pixel:?}"
    );
}

/// P1 regression: an alpha-only saveLayer paint with non-white RGB must NOT
/// tint the layer.
///
/// The public canvas opacity helpers (`Canvas::save_layer_alpha` /
/// `save_layer_opacity`, flui-painting `canvas/state.rs`) build their layer
/// paint as `Paint::fill(Color::TRANSPARENT).with_opacity(O)` — RGB
/// `[0,0,0]`, alpha `O`. If `save_layer` treated paint RGB as a composite
/// tint, those layers would composite with `(0,0,0,O)` and render the
/// contents BLACK instead of applying group opacity. `save_layer` must
/// normalize to a white (no-op) chroma; only `save_layer_with_tint` carries
/// chroma.
///
/// Opaque WHITE content in a 0.5 layer (black-RGB paint) over BLACK must
/// composite to mid-gray ≈128, not 0.
#[test]
fn alpha_only_layer_paint_does_not_tint_black() {
    use flui_painting::Paint;

    let (device, queue) = test_device_and_queue();
    let px = render_and_read_center(&device, &queue, 64, wgpu::Color::BLACK, |painter| {
        // Mirror the canvas opacity helper: TRANSPARENT (RGB 0,0,0) + alpha.
        painter.save_layer(
            None,
            &Paint::fill(flui_painting::styling::Color::rgba(0, 0, 0, 128)),
        );
        painter.draw_rect(
            Rect::from_xywh(0.0, 0.0, 64.0, 64.0),
            &Paint::fill(flui_painting::styling::Color::WHITE),
        );
        painter.restore_layer();
    });

    let (r, g, b) = (i32::from(px[0]), i32::from(px[1]), i32::from(px[2]));
    // White at group opacity 0.5 over black ≈ (128,128,128). The pre-fix
    // RGB-as-tint bug gives (0,0,0) — assert clearly above black.
    assert!(
        r > 100 && g > 100 && b > 100,
        "expected mid-gray ~128 (group opacity, white chroma); \
             a near-black result means the alpha-only paint's RGB was wrongly \
             used as a tint. pixel = {px:?}"
    );
    assert!(
        (r - 128).abs() <= 12 && (g - 128).abs() <= 12 && (b - 128).abs() <= 12,
        "R,G,B = {r},{g},{b}, expected ~128. pixel = {px:?}"
    );
}

/// BUG 3 (atlas packed with zero gutter): two images packed adjacently in
/// the shared atlas must not bleed into each other under the Linear sampler.
///
/// RED (A) is allocated first so it occupies atlas column range `[0, 64)`;
/// BLUE (B) is allocated next, immediately to A's right. A is then drawn
/// magnified AND extended past the right of the frame (`dst.x ∈ [-64, 128]`,
/// a 3x stretch) so that its `max_u` maps near screen column 128. Column
/// x=127 checks for BLUE bleed at the atlas seam.
///
/// With the fix: `upload_image` clears a 1px transparent gutter on the right
/// side of A. The bilinear kernel blends the last RED texel with alpha-zero
/// (not B's solid BLUE), leaving B~0. R may be attenuated but is never BLUE.
///
/// Before the fix: no gutter clear — A's `max_u` coincided with B's first
/// texel and bilinear sampling raised B well above 40.
#[test]
fn atlas_neighbors_do_not_bleed_under_linear_sampling() {
    use flui_painting::paint::Image;

    const SIZE: u32 = 128;
    let (device, queue) = test_device_and_queue();

    let red = Image::solid_color(64, 64, flui_painting::styling::Color::rgb(255, 0, 0));
    let blue = Image::solid_color(64, 64, flui_painting::styling::Color::rgb(0, 0, 255));

    let rgba = render_to_rgba(&device, &queue, SIZE, wgpu::Color::BLACK, |painter| {
        // RED packs first → atlas columns [0, 64). It is stretched over
        // screen x ∈ [-64, 128] (width 192, 3x): the dst maps source u=0..1
        // across that span, so screen x=127 → u≈0.995 (near max_u) and is a
        // fully-RED interior pixel because the geometric right edge sits at
        // x=128, off the sampled column.
        painter.draw_image(
            &red,
            Rect::from_xywh(-64.0, 0.0, 192.0, 128.0),
            flui_painting::BlendMode::SrcOver,
        );
        // BLUE packs next → atlas columns immediately right of RED's gutter.
        // Its slot is what an un-guttered bilinear sample of RED's right edge
        // would bleed into. Draw it off-screen; bleed is a texture-space
        // phenomenon, not screen-space.
        painter.draw_image(
            &blue,
            Rect::from_xywh(120.0, 120.0, 8.0, 8.0),
            flui_painting::BlendMode::SrcOver,
        );
    });

    // Sample the near-max_u column (x=127) at mid-height.
    //
    // With a transparent gutter the bilinear kernel at max_u blends the
    // last RED texel with an alpha-zero gutter pixel, which dims the RED
    // channel but contributes *zero* BLUE. So the correct assertion for the
    // "no bleed" property is `b < 40` (BLUE does not reach the sample site)
    // and `r > b + 80` (RED dominates BLUE even when partially attenuated).
    //
    // Without the gutter clear, BLUE from the neighboring atlas entry bleeds
    // in and raises B above 100 — clearly distinguishable from the ~0 B of
    // the transparent-gutter case.
    let edge = pixel_at(&rgba, SIZE, 127, 64);
    let (r, g, b) = (i32::from(edge[0]), i32::from(edge[1]), i32::from(edge[2]));
    assert!(
        b < 40 && r > b + 80,
        "RED's near-max_u column = (R={r}, G={g}, B={b}). \
             Expected B~0 (no BLUE bleed) and R dominant. \
             A B≥40 value means the Linear sampler bled BLUE from the neighboring \
             atlas entry — the transparent gutter strip in upload_image is missing."
    );
}

/// BUG 3 sharpness: a 2×1 image drawn exactly 1:1 must sample each texel
/// with its own color, not a blend with its neighbor.
///
/// With the (wrong) half-texel UV inset, `min_u` for a 2-wide image in a
/// 2048-wide atlas shifts right by `0.5/2048 ≈ 0.000244`, and `max_u`
/// shifts left symmetrically.  The left screen pixel's UV maps to roughly
/// `texel 0.25` (mix of 75% RED + 25% GREEN) rather than `texel 0.5` (pure
/// RED); the right pixel maps to roughly `texel 1.75` (25% RED + 75% GREEN)
/// rather than `texel 1.5` (pure GREEN).  The RED and GREEN channels would
/// both read ~191 instead of 255/0.
///
/// With exact texel-boundary UVs and a transparent gutter: left pixel maps to
/// `texel 0.5` → pure RED; right pixel maps to `texel 1.5` → pure GREEN.
#[test]
fn atlas_image_is_sharp_at_one_to_one() {
    use flui_painting::paint::Image;

    // 2-pixel wide, 1-pixel tall render target (drawn 1:1).
    const W: u32 = 2;
    const H: u32 = 1;
    let (device, queue) = test_device_and_queue();

    // Left texel = RED, right texel = GREEN.
    let pixels: Vec<u8> = vec![
        255, 0, 0, 255, // left pixel: RED
        0, 255, 0, 255, // right pixel: GREEN
    ];
    let img = Image::from_rgba8(W, H, pixels);

    let rgba = render_to_rgba(&device, &queue, W, wgpu::Color::BLACK, |painter| {
        painter.draw_image(
            &img,
            Rect::from_xywh(0.0, 0.0, 2.0, 1.0),
            flui_painting::BlendMode::SrcOver,
        );
    });

    let left = pixel_at(&rgba, W, 0, 0);
    let right = pixel_at(&rgba, W, 1, 0);
    let (lr, lg) = (i32::from(left[0]), i32::from(left[1]));
    let (rr, rg) = (i32::from(right[0]), i32::from(right[1]));
    assert!(
        lr > 200 && lg < 55,
        "left pixel = (R={lr}, G={lg}): expected RED (~255,~0). \
             A blended value means uv_coords has a half-texel inset that \
             shifts sampling away from the texel center."
    );
    assert!(
        rg > 200 && rr < 55,
        "right pixel = (R={rr}, G={rg}): expected GREEN (~0,~255). \
             A blended value means uv_coords has a half-texel inset that \
             shifts sampling away from the texel center."
    );
}

// ===== Phase A per-draw blend mode (fixed-function Porter-Duff) =====
//
// These tests exercise the TESSELLATED path: a filled `Path` and stroked
// shapes always tessellate (never the instanced fast path), so they go
// through `pipeline_key_from_paint` → the blend pipeline keyed by
// `PipelineKey::blend_mode`. All values are premultiplied-alpha results on
// the UNorm readback target.

/// Helper: a filled-path rect covering the whole `size`×`size` frame. Forces
/// the tessellated path regardless of the (axis-aligned) transform, so the
/// per-draw blend pipeline is selected.
fn full_frame_fill_path(size: f32) -> flui_painting::paint::path::Path {
    flui_painting::paint::path::Path::rectangle(Rect::from_xywh(
        0.0,
        0.0,
        f64::from(size),
        f64::from(size),
    ))
}

/// SrcOver pixel-identity (regression guard for the premultiply switch):
/// a 50%-alpha RED filled PATH over opaque white must read back the same
/// straight-SrcOver value the old `input.color` + `ALPHA_BLENDING` path
/// produced. Premultiplied SrcOver is `src + dst*(1-a)`; with
/// `src = (0.502,0,0,0.502)` over white this is `(1.0, 0.498, 0.498)` ≈
/// `(255, 127, 127)` — identical to the old output. A divergence here means
/// the premultiply switch changed visible SrcOver output.
#[test]
fn blend_srcover_filled_path_pixel_identity() {
    use flui_painting::Paint;

    let (device, queue) = test_device_and_queue();
    let px_val = render_and_read_center(&device, &queue, 64, wgpu::Color::WHITE, |painter| {
        painter.draw_path(
            &full_frame_fill_path(64.0),
            &Paint::fill(flui_painting::styling::Color::rgba(255, 0, 0, 128)),
        );
    });

    let (r, g, b) = (
        i32::from(px_val[0]),
        i32::from(px_val[1]),
        i32::from(px_val[2]),
    );
    assert!(
        (r - 255).abs() <= 3,
        "R = {r}, expected ~255 (premultiplied SrcOver red over white). pixel = {px_val:?}"
    );
    assert!(
        (g - 127).abs() <= 4 && (b - 127).abs() <= 4,
        "G,B = {g},{b}, expected ~127 (50% red over white). \
             A drift here means premultiplied SrcOver is no longer identical to \
             the old straight-alpha output. pixel = {px_val:?}"
    );
}

/// A gradient fill carrying `BlendMode::Clear` erases the target.
///
/// This assertion is the inverse of the one it replaces. That one pinned the
/// LIMIT — a gradient's blend mode was accepted, carried on the paint, and
/// dropped, so `Clear` rendered as `SrcOver` and left the background visible —
/// and it fired the moment the gradient pipelines became blend-mode-keyed.
/// Its original claim is preserved verbatim in the failure message below, as
/// the thing that must NOT be observed.
///
/// The arithmetic of every fixed-function mode on every gradient kind, and its
/// behaviour under partial coverage, lives in `gradient_blend_readback_tests`.
/// What this one keeps is the specific shape the limit was recorded against: a
/// full-frame red-to-blue gradient over an opaque white surface.
#[test]
fn a_gradient_fill_carrying_clear_erases_the_target() {
    use flui_painting::Paint;
    use flui_painting::paint::TileMode;

    let (device, queue) = test_device_and_queue();

    let shader = flui_painting::Shader::linear_gradient(
        flui_foundation::geometry::Point::new(0.0, 0.0).into(),
        flui_foundation::geometry::Point::new(64.0, 0.0).into(),
        vec![
            flui_painting::styling::Color::rgb(255, 0, 0),
            flui_painting::styling::Color::rgb(0, 0, 255),
        ],
        None,
        TileMode::Clamp,
    );
    let paint = Paint::fill(flui_painting::styling::Color::WHITE)
        .with_shader(shader)
        .with_blend_mode(BlendMode::Clear);

    let cleared = render_and_read_center(&device, &queue, 64, wgpu::Color::WHITE, |painter| {
        painter.draw_rect(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), &paint);
    });

    assert_eq!(
        cleared,
        [0, 0, 0, 0],
        "a gradient painted with Clear must erase the surface. A non-zero pixel \
         means the paint's blend mode was dropped and the gradient rendered as \
         SrcOver — which is what this test used to assert, back when the \
         gradient pipelines hard-coded ALPHA_BLENDING. got {cleared:?}"
    );
}

/// Draw-order correctness for non-SrcOver blend modes (P1 regression).
///
/// `flush_segment` renders batches in FIXED order (instanced → tessellated),
/// not recording order.  Without the segment-split fix, a non-SrcOver
/// tessellated shape batched into the same segment as LATER instanced draws
/// would execute AFTER those draws, erasing content that was laid down after it.
///
/// Scenario (64×64 frame, cleared to black):
///   1. Draw opaque RED instanced rect over the entire frame (SrcOver) → S0.
///   2. Draw a Clear `draw_path` over the entire frame (non-SrcOver →
///      tessellated → segment-split fix seals S0 and opens S1).
///   3. Draw opaque GREEN instanced rect over the entire frame (SrcOver → S1).
///
/// Correct draw order: RED, then Clear (transparent), then GREEN → center GREEN.
///
/// Without the fix (all three in segment S0):
///   - `flush_segment` runs instanced FIRST (RED + GREEN: last writer wins →
///     GREEN), then tessellated Clear → transparent. Center = transparent.
///   - The GREEN assertion (G > 200) fails.
///
/// With the fix (segment split after Clear):
///   - S0 flush: RED (instanced), Clear (tess) → transparent.
///   - S1 flush: GREEN (instanced) → GREEN on transparent → GREEN visible.
///   - Center reads GREEN ~(0,255,0). Assertion passes.
///
/// RED-BEFORE (no fix): center alpha = 0, G = 0 (cleared by out-of-order Clear).
/// GREEN-AFTER (fix):   center pixel ~(0,255,0,255).
#[test]
fn blend_clear_respects_draw_order() {
    use flui_painting::Paint;

    const SIZE: u32 = 64;
    let (device, queue) = test_device_and_queue();

    let rgba = render_to_rgba(&device, &queue, SIZE, wgpu::Color::BLACK, |painter| {
        let red = flui_painting::styling::Color::rgb(255, 0, 0);
        let green = flui_painting::styling::Color::rgb(0, 255, 0);

        // Step 1: fill frame RED via instanced path (SrcOver → S0 rect_batch).
        painter.draw_rect(
            Rect::from_xywh(0.0, 0.0, f64::from(SIZE as f32), f64::from(SIZE as f32)),
            &Paint::fill(red),
        );

        // Step 2: Clear entire frame via tessellated path (BlendMode::Clear).
        // The segment-split fix appends Clear to S0's tess_batches then seals
        // S0 → draw_order, opening fresh S1.
        // Without fix: Clear goes into S0 alongside the RED and GREEN instanced
        // draws, and flush_segment would run instanced FIRST (RED+GREEN both
        // rendered, last-writer GREEN wins), then tessellated Clear → erases
        // everything; center is transparent.
        painter.draw_path(
            &full_frame_fill_path(SIZE as f32),
            &Paint::fill(red).with_blend_mode(BlendMode::Clear),
        );

        // Step 3: fill frame GREEN via instanced path (SrcOver → S1 rect_batch).
        // With the fix: S1 flushes entirely AFTER S0 (which ended with Clear),
        // so GREEN is drawn on top of transparent → GREEN visible.
        painter.draw_rect(
            Rect::from_xywh(0.0, 0.0, f64::from(SIZE as f32), f64::from(SIZE as f32)),
            &Paint::fill(green),
        );
    });

    // Center must be GREEN: drawn AFTER the Clear.
    // Without fix: center is transparent (0,0,0,0) — Clear ran last, erased GREEN.
    // With fix:    center is ~(0,255,0,255) — Clear sealed S0, GREEN in S1 is intact.
    let center = pixel_at(&rgba, SIZE, SIZE / 2, SIZE / 2);
    assert!(
        center[1] > 200 && center[0] < 10 && center[2] < 10,
        "center pixel = {center:?}, expected GREEN ~(0,255,0). \
             A transparent or red result means the out-of-order Clear erased \
             the GREEN that was drawn AFTER it (segment-split fix missing)."
    );
    assert_eq!(
        center[3], 255,
        "center alpha = {}, expected 255 (GREEN fully covers the cleared frame). \
             alpha=0 means the Clear ran AFTER GREEN and erased it. pixel = {center:?}",
        center[3]
    );
}

// ===== Shape-batcher characterisation readback safety net =====
//
// Locks down the relocated batcher slow path (non-axis-aligned rect with a
// non-SrcOver blend mode).  A regression in the moved branch would pass the
// instanced-path tests but silently break the tessellated-path segment seal.

/// A rotated rect with `BlendMode::Clear` seals its segment so a later
/// `SrcOver` instanced rect composites correctly.
///
/// # What this covers
///
/// After extracting `DrawBatcher::rect`, the *slow path* inside that
/// method — reached when the transform is NOT axis-aligned OR the blend mode
/// is not `SrcOver` — calls `add_tessellated_with_key`, which in turn calls
/// `finish_current_segment` for any non-`SrcOver` blend.  This is the moved
/// code that had no GPU-readback coverage before this test.
///
/// # Draw sequence
///
/// 1. Fill frame RED via the fast instanced path (SrcOver, axis-aligned) → S0.
/// 2. `save` + `rotate(45°)` so `is_axis_aligned()` returns `false`.
///    Draw an overlapping rect with `BlendMode::Clear` via the batcher slow
///    path.  `add_tessellated_with_key` appends it to S0's tess_batches then
///    seals S0 (non-SrcOver contract), opening S1.  `restore` returns to
///    identity.
/// 3. Fill frame GREEN via the fast instanced path (SrcOver, axis-aligned) → S1.
///
/// # Correct outcome
///
/// - S0 flushes: RED instanced, then Clear tessellated → frame transparent at
///   the rotated quad region.
/// - S1 flushes: GREEN instanced on top of (possibly transparent) background →
///   center pixel is GREEN.
///
/// # Failure modes caught
///
/// - **Slow-path seal missing** (seal removed from `add_tessellated_with_key`):
///   Clear and GREEN end up in the same segment; `flush_segment` runs instanced
///   first (RED+GREEN → GREEN wins), then Clear erases everything → center
///   transparent.  `center[1] > 200` fails.
/// - **Slow path not reached** (rotation guard removed, fast path taken):
///   Clear is submitted as an instanced rect, bypasses `add_tessellated_with_key`,
///   no segment seal → same failure mode as above.
#[test]
fn batcher_rotated_clear_rect_seals_segment_before_srcover() {
    use flui_painting::Paint;
    use std::f32::consts::FRAC_PI_4;

    const SIZE: u32 = 64;
    let (device, queue) = test_device_and_queue();

    let rgba = render_to_rgba(&device, &queue, SIZE, wgpu::Color::BLACK, |painter| {
        let red = flui_painting::styling::Color::rgb(255, 0, 0);
        let green = flui_painting::styling::Color::rgb(0, 255, 0);

        // Step 1: fill the frame RED via the fast instanced path.
        // axis-aligned + SrcOver → S0 rect_batch.
        painter.draw_rect(
            Rect::from_xywh(0.0, 0.0, f64::from(SIZE as f32), f64::from(SIZE as f32)),
            &Paint::fill(red),
        );

        // Step 2: draw a large rotated rect with BlendMode::Clear.
        // The 45° rotation makes is_axis_aligned() = false AND blend_mode !=
        // SrcOver, so DrawBatcher::rect takes the tessellated slow path.
        // add_tessellated_with_key appends the tess batch then calls
        // finish_current_segment (non-SrcOver contract), sealing S0 and
        // opening S1.
        painter.save();
        // Rotate around the frame centre so the rotated quad covers centre.
        let half = SIZE as f32 / 2.0;
        painter.translate(flui_foundation::geometry::Offset::new(
            f64::from(half),
            f64::from(half),
        ));
        painter.rotate(FRAC_PI_4);
        painter.translate(flui_foundation::geometry::Offset::new(
            f64::from(-half),
            f64::from(-half),
        ));
        painter.draw_rect(
            Rect::from_xywh(0.0, 0.0, f64::from(SIZE as f32), f64::from(SIZE as f32)),
            &Paint::fill(red).with_blend_mode(BlendMode::Clear),
        );
        painter.restore();

        // Step 3: fill the frame GREEN via the fast instanced path (SrcOver).
        // After step 2 sealed S0, this goes into S1.
        painter.draw_rect(
            Rect::from_xywh(0.0, 0.0, f64::from(SIZE as f32), f64::from(SIZE as f32)),
            &Paint::fill(green),
        );
    });

    // The center pixel must be GREEN: S1 (GREEN fill) flushed after S0 (which
    // ended with Clear), so GREEN is drawn on top of whatever Clear left.
    // Failure = center is transparent or red (Clear ran after GREEN in the same
    // segment, erasing it), indicating the moved slow-path seal was broken.
    let center = pixel_at(&rgba, SIZE, SIZE / 2, SIZE / 2);
    assert!(
        center[1] > 200 && center[0] < 10 && center[2] < 10,
        "center pixel = {center:?}, expected GREEN ~(0,255,0,255). \
             A transparent or red result means the rotated-Clear rect did not seal \
             its segment before the subsequent SrcOver rect (slow-path seal broken)."
    );
    assert_eq!(
        center[3], 255,
        "center alpha = {}, expected 255 (GREEN fully covers the frame). \
             alpha=0 means Clear executed after GREEN (segment seal missing). \
             pixel = {center:?}",
        center[3]
    );
}

// ===== T6 Characterisation readback safety-net =====
//
// These tests lock down the SDF-clip baking (clip_rrect / clip_rsuperellipse
// corner cutouts) and the nested save+clip+restore scissor restoration that
// had ZERO pixel coverage before T6. They are characterisation tests: they
// pass on the current (correct) code and will FAIL if the GpuStateStack
// extraction) breaks clip/scissor behaviour.
//
// Each test discriminates: the assertion would fail if the clip were a plain
// axis-aligned square (no SDF applied) or if save/restore leaked the scissor.

/// T6-1: `clip_rrect` SDF baking removes corner pixels.
///
/// A 100×100 target is cleared to BLACK. An 80×80 RRect with 20px uniform
/// corner radius is set as the active clip. The entire 80×80 bounding box is
/// then filled RED.
///
/// The pixel at the TOP-LEFT CORNER of the bounding box (x=0, y=0 relative to
/// the rrect) sits inside the axis-aligned bounding box but outside the
/// rounded corner arc. The SDF shader discards it; a plain scissor-only clip
/// would paint it RED.
///
/// Interior pixel (50, 50) is well inside every corner arc — must be RED.
/// Corner pixel (10, 10) is inside the bbox but outside the arc — must be BLACK.
///
/// Without SDF: corner pixel = RED (clip is merely a square scissor).
/// With SDF:    corner pixel = BLACK (fragment discarded by rrect SDF).
#[test]
fn clip_rrect_sdf_removes_corner_pixels() {
    use flui_painting::Paint;

    const SIZE: u32 = 100;
    // The clip rect: 10..90 in both axes (80×80), 20px uniform corner radius.
    // The corner at (10,10) to (30,30) is a quadrant governed by the arc.
    // The exact centre of the corner quarter-circle is at (30, 30) screen-space
    // (i.e. rrect.left + radius, rrect.top + radius).  The pixel at (11, 11) is
    // 1 pixel past the corner — outside the arc, inside the bbox.
    const RRECT_LEFT: f32 = 10.0;
    const RRECT_TOP: f32 = 10.0;
    const RRECT_RIGHT: f32 = 90.0;
    const RRECT_BOTTOM: f32 = 90.0;
    const RADIUS: f32 = 20.0;

    let (device, queue) = test_device_and_queue();

    let rgba = render_to_rgba(&device, &queue, SIZE, wgpu::Color::BLACK, |painter| {
        let rrect = flui_foundation::geometry::RRect::from_rect_circular(
            Rect::from_xywh(
                f64::from(RRECT_LEFT),
                f64::from(RRECT_TOP),
                f64::from(RRECT_RIGHT - RRECT_LEFT),
                f64::from(RRECT_BOTTOM - RRECT_TOP),
            ),
            f64::from(RADIUS),
        );
        painter.clip_rrect(rrect, flui_painting::paint::Clip::AntiAlias);

        // Fill the entire canvas RED. Only pixels passing the rrect SDF will
        // actually be painted; the rest remain BLACK (clear colour).
        painter.draw_rect(
            Rect::from_xywh(0.0, 0.0, f64::from(SIZE as f32), f64::from(SIZE as f32)),
            &Paint::fill(flui_painting::styling::Color::rgb(255, 0, 0)),
        );
    });

    // Interior pixel at (50, 50) — deep inside the rrect, far from all corners.
    // Must be RED; if the clip discards everything this test would also fail.
    let interior = pixel_at(&rgba, SIZE, 50, 50);
    assert!(
        interior[0] > 200,
        "interior pixel (50,50) R={}, expected ~255 (RED fill inside rrect). \
             clip_rrect is discarding too much — possible SDF radius overclaim. \
             pixel={interior:?}",
        interior[0]
    );

    // Corner pixel: (11, 11) is 1px inside the axis-aligned bbox but inside
    // the corner arc's quadrant. At radius=20 the SDF for a point at distance
    // (~13 px) from the corner centre (30,30) is positive (outside the arc).
    //
    // Discriminator: a plain scissor-only implementation would paint this RED
    // (it is inside the 10..90 scissor).  The SDF shader must discard the
    // draw, leaving the opaque BLACK clear colour.
    //
    // The clear colour is wgpu::Color::BLACK = (0.0, 0.0, 0.0, 1.0), so the
    // pixel is [0, 0, 0, 255]. We check R < 30 (not alpha) to discriminate:
    //   - SDF applied correctly → R ≈ 0 (BLACK, not painted) ✓
    //   - SDF missing (scissor-only) → R ≈ 255 (RED fill bleeds into corner)
    let corner = pixel_at(&rgba, SIZE, 11, 11);
    assert!(
        corner[0] < 30,
        "corner pixel (11,11) R={}, expected ~0 (BLACK — outside rounded corner arc). \
             A non-zero R means clip_rrect is acting as a plain scissor (no SDF applied). \
             pixel={corner:?}",
        corner[0]
    );

    // Sanity: a pixel strictly outside the bounding box must be BLACK.
    let outside_bbox = pixel_at(&rgba, SIZE, 5, 5);
    assert!(
        outside_bbox[0] < 10,
        "pixel (5,5) is outside the rrect bbox, R={}, expected 0. \
             pixel={outside_bbox:?}",
        outside_bbox[0]
    );
}

/// T6-3: nested `save + clip_rect + paint + restore` correctly removes the scissor.
///
/// This test proves the save/restore scissor asymmetry is DESIGN (correct), not
/// a bug. See the `save()` comment block for the invariant proof.
///
/// Layout (100×100 target, cleared to BLACK):
///   - Paint the full canvas GREEN.
///   - save() → clip_rect to LEFT half (x 0..50) → paint RIGHT half RED.
///   - restore() → the scissor must be gone.
///   - Paint a narrow BLUE column at x=60..62 (right of the clip boundary).
///
/// Assertions:
///   A. LEFT interior (x=25, y=50):  GREEN (painted before clip, not touched after).
///   B. RIGHT interior before BLUE (x=55, y=50): GREEN (RED was clipped away).
///   C. BLUE column (x=61, y=50): BLUE — proves restore removed the scissor so
///      the post-restore paint reaches the right half.
///
/// Without correct restore: BLUE column = BLACK (scissor still active after restore).
/// With correct restore:    BLUE column = BLUE.
#[test]
fn nested_save_clip_restore_removes_scissor() {
    use flui_painting::Paint;

    const SIZE: u32 = 100;
    let (device, queue) = test_device_and_queue();

    let rgba = render_to_rgba(&device, &queue, SIZE, wgpu::Color::BLACK, |painter| {
        let green = flui_painting::styling::Color::rgb(0, 255, 0);
        let red = flui_painting::styling::Color::rgb(255, 0, 0);
        let blue = flui_painting::styling::Color::rgb(0, 0, 255);

        // Step 1: paint the full canvas GREEN (baseline for both halves).
        painter.draw_rect(
            Rect::from_xywh(0.0, 0.0, f64::from(SIZE as f32), f64::from(SIZE as f32)),
            &Paint::fill(green),
        );

        // Step 2: save, clip to left half, try to paint the right half RED.
        // The RED paint must be clipped (scissor blocks x≥50).
        painter.save();
        painter.clip_rect(
            Rect::from_xywh(0.0, 0.0, 50.0, f64::from(SIZE as f32)),
            flui_painting::paint::Clip::HardEdge,
        );
        painter.draw_rect(
            Rect::from_xywh(50.0, 0.0, 50.0, f64::from(SIZE as f32)),
            &Paint::fill(red),
        );
        painter.restore();

        // Step 3: after restore the scissor must be cleared. Paint a BLUE column
        // at x=60..62 which is in the right half (would be clipped if scissor leaked).
        painter.draw_rect(
            Rect::from_xywh(60.0, 0.0, 2.0, f64::from(SIZE as f32)),
            &Paint::fill(blue),
        );
    });

    // A. Left half (x=25, y=50): must be GREEN (painted in step 1, unaffected).
    let left = pixel_at(&rgba, SIZE, 25, 50);
    assert!(
        left[1] > 200 && left[0] < 10 && left[2] < 10,
        "left interior (25,50) expected GREEN, got {left:?}. \
             Left half should be the original GREEN fill."
    );

    // B. Right half between clip boundary and blue column (x=55, y=50):
    // must be GREEN. RED was clipped by the scissor, so GREEN underneath survives.
    let right_no_blue = pixel_at(&rgba, SIZE, 55, 50);
    assert!(
        right_no_blue[1] > 200 && right_no_blue[0] < 30 && right_no_blue[2] < 30,
        "right interior (55,50) expected GREEN (RED clipped away), got {right_no_blue:?}. \
             If RED appears the scissor did not clip during save+clip+restore."
    );

    // C. BLUE column (x=61, y=50): must be BLUE.
    // Discriminator: if restore() leaked the scissor, x=61 is still clipped and
    // stays GREEN; only a correct restore allows the post-restore blue paint through.
    let blue_col = pixel_at(&rgba, SIZE, 61, 50);
    assert!(
        blue_col[2] > 200 && blue_col[0] < 10 && blue_col[1] < 30,
        "blue column (61,50) expected BLUE, got {blue_col:?}. \
             A non-blue result means restore() left the scissor active (leaked scissor), \
             blocking the post-restore BLUE paint from reaching the right half."
    );
}

/// Path-cache characterisation: `draw_path` cache-hit branch uses the *current*
/// `paint.color`, not the color from the first (cache-miss) tessellation.
///
/// # Discriminating strategy
///
/// Both draws happen in the **same painter frame** so the second call hits
/// the per-frame `path_cache` entry written by the first call:
///
///   1. Draw a filled triangle that covers the top-left quadrant in RED.
///   2. Draw the **identical path** in BLUE — the cache returns the
///      untransformed positions; the cache-hit branch must reconstruct
///      `Vertex`s with the *current* blue paint color before submitting.
///
/// Sampling the center of the second triangle must yield BLUE (not RED).
/// If the cache-hit branch silently reuses the first tessellation's
/// `Vertex::color` bytes, the pixel stays red and the assertion fails.
///
/// The triangle is translated for the second draw so it does not overlap
/// with the first, making the test pixel unambiguous.
#[cfg(feature = "testing")]
#[test]
fn draw_path_cache_hit_uses_current_paint_color() {
    use flui_painting::paint::path::Path;

    const SIZE: u32 = 64;
    let (device, queue) = test_device_and_queue();

    // A filled right-triangle occupying the top-left 32×32 area.
    let triangle_path = {
        let mut p = Path::new();
        p.move_to(flui_foundation::geometry::Point::new(0.0, 0.0));
        p.line_to(flui_foundation::geometry::Point::new(32.0, 0.0));
        p.line_to(flui_foundation::geometry::Point::new(0.0, 32.0));
        p.close();
        p
    };

    let red_paint = flui_painting::Paint::fill(flui_painting::styling::Color::rgb(255, 0, 0));
    let blue_paint = flui_painting::Paint::fill(flui_painting::styling::Color::rgb(0, 0, 255));

    let rgba = render_to_rgba(&device, &queue, SIZE, wgpu::Color::TRANSPARENT, |painter| {
        // First draw: cache MISS — tessellates and caches; renders at origin.
        painter.draw_path(&triangle_path, &red_paint);

        // Translate right so the second triangle doesn't overlap the first.
        painter.translate(flui_foundation::geometry::Offset::new(32.0, 0.0));

        // Second draw: cache HIT — must use blue_paint.color, not cached red.
        painter.draw_path(&triangle_path, &blue_paint);
    });

    // Sample a pixel well inside the second (blue) triangle's area.
    // After the translate(32, 0), the second triangle spans x=[32..64], y=[0..32].
    // x=40, y=8 is safely inside the filled region.
    let second_triangle_pixel = pixel_at(&rgba, SIZE, 40, 8);

    assert_eq!(
        second_triangle_pixel[3], 255,
        "second triangle pixel alpha={}, expected 255 (path rendered opaque). \
             Alpha=0 means draw_path did not submit geometry. pixel={second_triangle_pixel:?}",
        second_triangle_pixel[3]
    );
    assert!(
        second_triangle_pixel[2] > 200,
        "second triangle pixel B={}: expected B > 200 (blue fill). \
             Low blue means the cache-hit branch reused the first draw's red color. \
             pixel={second_triangle_pixel:?}",
        second_triangle_pixel[2]
    );
    assert!(
        second_triangle_pixel[0] < 10,
        "second triangle pixel R={}: expected R < 10 (no red leakage from cache). \
             High red means the cache-hit branch did not apply current paint.color. \
             pixel={second_triangle_pixel:?}",
        second_triangle_pixel[0]
    );
}

/// `ColorFilter::Matrix` recolors the image on the CPU then routes through
/// `draw_image`. A red↔blue channel-swap matrix turns an opaque RED image
/// blue — covering the CPU-recolor delegation path the `Mode` branch now
/// shares.
#[test]
fn draw_image_filtered_matrix_swaps_channels() {
    use flui_painting::paint::Image;
    use flui_painting::paint::image::ColorFilter;

    const SIZE: u32 = 16;
    let (device, queue) = test_device_and_queue();

    // Opaque RED source image.
    let red_pixels: Vec<u8> = (0..SIZE * SIZE).flat_map(|_| [255u8, 0, 0, 255]).collect();
    let red_image = Image::from_rgba8(SIZE, SIZE, red_pixels);

    // 5×4 row-major matrix swapping R and B (R'=B, G'=G, B'=R, A'=A).
    let swap_rb = ColorFilter::matrix([
        0.0, 0.0, 1.0, 0.0, 0.0, // R' = B
        0.0, 1.0, 0.0, 0.0, 0.0, // G' = G
        1.0, 0.0, 0.0, 0.0, 0.0, // B' = R
        0.0, 0.0, 0.0, 1.0, 0.0, // A' = A
    ]);

    let px_val = render_and_read_center(&device, &queue, SIZE, wgpu::Color::BLACK, |painter| {
        painter.draw_image_filtered(
            &red_image,
            Rect::from_xywh(0.0, 0.0, f64::from(SIZE as f32), f64::from(SIZE as f32)),
            swap_rb,
            flui_painting::BlendMode::SrcOver,
        );
    });

    let (r, g, b) = (
        i32::from(px_val[0]),
        i32::from(px_val[1]),
        i32::from(px_val[2]),
    );

    assert!(
        b >= 200,
        "B={b}: expected B ≈ 255 (R swapped into B). pixel={px_val:?}"
    );
    assert!(
        r <= 40,
        "R={r}: expected R ≈ 0 (B=0 swapped into R). pixel={px_val:?}"
    );
    assert!(
        g <= 40,
        "G={g}: expected G ≈ 0 (unchanged). pixel={px_val:?}"
    );
}

/// Two filtered draws of the **same source image** with **different** color
/// filters in one frame must not alias in the texture cache.
///
/// Each filter produces a short-lived temporary `Image`; filtered draws key
/// the cache on a hash of the produced bytes, not the temporary's pointer.
/// If they keyed on the pointer (as a plain `draw_image` does), the second
/// temporary — frequently reallocated at the just-freed address of the first
/// — would collide on key and the cache would return the first filter's
/// texture for the second draw (it hits on key alone, never re-comparing
/// bytes). Here a white source is modulated RED in the top half and BLUE in
/// the bottom half; a collision would paint the bottom half red.
#[test]
fn draw_image_filtered_distinct_filters_do_not_alias() {
    use flui_painting::paint::image::ColorFilter;
    use flui_painting::{paint::Image, styling::Color};

    const SIZE: u32 = 16;
    let (device, queue) = test_device_and_queue();

    let white_pixels: Vec<u8> = (0..SIZE * SIZE)
        .flat_map(|_| [255u8, 255, 255, 255])
        .collect();
    let white_image = Image::from_rgba8(SIZE, SIZE, white_pixels);

    let modulate_red = ColorFilter::mode(
        Color::rgba(255, 0, 0, 255),
        flui_painting::BlendMode::Modulate,
    );
    let modulate_blue = ColorFilter::mode(
        Color::rgba(0, 0, 255, 255),
        flui_painting::BlendMode::Modulate,
    );

    let half = SIZE as f32 / 2.0;
    let rgba = render_to_rgba(&device, &queue, SIZE, wgpu::Color::BLACK, |painter| {
        // Two separate filtered draws → two short-lived temporaries, the
        // second likely reusing the first's freed allocation address.
        painter.draw_image_filtered(
            &white_image,
            Rect::from_xywh(0.0, 0.0, f64::from(SIZE as f32), f64::from(half)),
            modulate_red,
            flui_painting::BlendMode::SrcOver,
        );
        painter.draw_image_filtered(
            &white_image,
            Rect::from_xywh(
                0.0,
                f64::from(half),
                f64::from(SIZE as f32),
                f64::from(half),
            ),
            modulate_blue,
            flui_painting::BlendMode::SrcOver,
        );
    });

    let top = pixel_at(&rgba, SIZE, SIZE / 2, SIZE / 4);
    let bottom = pixel_at(&rgba, SIZE, SIZE / 2, SIZE * 3 / 4);

    // Top half: modulate RED → red.
    assert!(
        top[0] >= 200 && top[2] <= 40,
        "top={top:?}: expected red (R≈255, B≈0) from modulate-RED"
    );
    // Bottom half: modulate BLUE → blue. A cache collision with the first
    // (red) temporary would paint this red instead.
    assert!(
        bottom[2] >= 200 && bottom[0] <= 40,
        "bottom={bottom:?}: expected blue (B≈255, R≈0) from modulate-BLUE. \
             Red here means the second filtered draw aliased the first in the \
             texture cache (pointer-identity key on a freed temporary)."
    );
}

/// External-texture resolution happens at replay time, not record time.
///
/// Register a solid-RED texture under ID 77, record `draw_texture`, then
/// call `update()` on the same ID replacing it with a solid-GREEN texture —
/// all BEFORE `render()`.  Assert the readback shows GREEN, not RED.
///
/// This test fails with record-time resolution (the stale view survives the
/// update) and PASSES after (replay-time resolution → GREEN wins).
///
/// Flutter reference: `Texture` widget and the engine's `ExternalTextureRegistry`
/// feed the platform's most-recently-uploaded frame to the rasterizer at
/// present time, not at the `drawImage` command time — any frame produced
/// between record and rasterize is presented (latest-frame semantics).
/// This test encodes that contract for the FLUI IR.
#[test]
fn external_texture_resolves_at_replay_not_record_time() {
    const SIZE: u32 = 16;
    let (device, queue) = test_device_and_queue();

    // Helper: create a solid-color 1-channel RGBA Unorm texture.
    let make_solid_texture =
        |device: &wgpu::Device, queue: &wgpu::Queue, r: u8, g: u8, b: u8| -> wgpu::Texture {
            let data: Vec<u8> = (0..SIZE * SIZE).flat_map(|_| [r, g, b, 0xFFu8]).collect();
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("solid color test texture"),
                size: wgpu::Extent3d {
                    width: SIZE,
                    height: SIZE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * SIZE),
                    rows_per_image: Some(SIZE),
                },
                wgpu::Extent3d {
                    width: SIZE,
                    height: SIZE,
                    depth_or_array_layers: 1,
                },
            );
            tex
        };

    let tex_id = flui_painting::paint::TextureId::new(77);

    // Build a painter with a full-size UNorm render target.
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("replay-timing readback target"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: READBACK_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());

    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        READBACK_FORMAT,
        (SIZE, SIZE),
    );

    // Step 1: register a solid-RED texture.
    painter.external_texture_registry_mut().register(
        tex_id,
        make_solid_texture(&device, &queue, 0xFF, 0x00, 0x00),
        SIZE,
        SIZE,
        true,  // dynamic
        false, // nearest sampler
    );

    // Step 2: record draw_texture.  Under record-time resolution this would
    // capture RED's TextureView into the IR.  Under replay-time resolution
    // only the TextureId is stored.
    painter.draw_texture(
        tex_id,
        Rect::from_xywh(0.0, 0.0, f64::from(SIZE as f32), f64::from(SIZE as f32)),
        None,
        flui_painting::paint::FilterQuality::None,
        1.0,
    );

    // Step 3: update the same ID to a solid-GREEN texture BEFORE render().
    // Under record-time resolution this update would be invisible (the old
    // RED view was already cloned into the IR).  Under replay-time resolution
    // the registry lookup at flush time picks up GREEN.
    let updated = painter.external_texture_registry_mut().update(
        tex_id,
        make_solid_texture(&device, &queue, 0x00, 0xFF, 0x00),
    );
    assert!(updated, "update must return true when the ID is registered");

    // Step 4: render.
    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("replay-timing clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
    painter
        .render_to_view(&target_view, &mut encoder)
        .expect("painter.render must succeed");

    // Readback.
    let bytes_per_pixel = 4u32;
    let unpadded = SIZE * bytes_per_pixel;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = unpadded.div_ceil(align) * align;
    let readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("replay-timing readback buffer"),
        size: u64::from(padded_bytes_per_row) * u64::from(SIZE),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback_buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(SIZE),
            },
        },
        wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(std::iter::once(encoder.finish()));

    let slice = readback_buf.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| {
        r.expect("buffer mapping must succeed");
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .expect("device poll must complete the readback copy");

    let data = slice
        .get_mapped_range()
        .expect("staging buffer must be mapped: the poll above waited for the map to complete");
    // Sample center pixel.
    let center = (SIZE / 2) as usize;
    let stride = padded_bytes_per_row as usize;
    let off = center * stride + center * 4;
    let pixel = [data[off], data[off + 1], data[off + 2], data[off + 3]];
    drop(data);
    readback_buf.unmap();

    let (r, g, b) = (
        i32::from(pixel[0]),
        i32::from(pixel[1]),
        i32::from(pixel[2]),
    );
    // Must be GREEN (updated texture), NOT RED (originally recorded texture).
    // Failure here means resolution happened at record time.
    assert!(
        g > 200 && r < 20,
        "Center pixel = {pixel:?}: expected GREEN (G>200, R<20) to prove \
             replay-time resolution. \
             RED (R>200, G<20) means the TextureView was captured at record time \
             and the update() was invisible — regression in the record/replay seam."
    );
    assert!(
        b < 20,
        "B = {b}, expected near-zero (solid GREEN texture, no blue). pixel = {pixel:?}"
    );
}

/// An offscreen result composited with a blend mode other than `SrcOver`
/// must actually use that mode.
///
/// `ShaderMaskLayer` carries its own `blend_mode()`; before this test the
/// layer path accepted it, threaded it into `render_masked`, and that
/// function dropped it on the floor — every masked layer composited
/// `SrcOver` regardless of what the caller asked for. The mode now rides on
/// `DrawItem::OffscreenTexture` and selects the composite pipeline, so a
/// `Clear` result erases what is under it rather than drawing over it.
///
/// The scene: an opaque red frame, then a full-surface offscreen result
/// composited with `Clear`. `Clear` ignores the source entirely and writes
/// zero, so the centre must come back transparent. `SrcOver` (the pre-fix
/// behaviour) would leave red.
#[test]
fn an_offscreen_result_composites_with_its_own_blend_mode() {
    use flui_painting::Paint;

    const SIZE: u32 = 64;
    let (device, queue) = test_device_and_queue();

    let rgba = render_to_rgba(&device, &queue, SIZE, wgpu::Color::BLACK, |painter| {
        // Step 1: opaque red, so the frame has something to erase.
        painter.draw_rect(
            Rect::from_xywh(0.0, 0.0, f64::from(SIZE as f32), f64::from(SIZE as f32)),
            &Paint::fill(flui_painting::styling::Color::rgb(255, 0, 0)),
        );

        // Step 2: an all-zero offscreen texture, composited with Clear. A
        // `Clear` composite ignores the source colour, which makes this the
        // discriminating case: SrcOver leaves the red, Clear erases it.
        let mut pool = crate::texture_pool::TexturePool::new(Arc::clone(&device));
        let texture = pool.acquire(SIZE, SIZE, READBACK_FORMAT);
        {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("clear-composite source"),
            });
            {
                let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("clear-composite source fill"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: texture.view(),
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::GREEN),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
            }
            queue.submit(std::iter::once(encoder.finish()));
        }

        painter.queue_offscreen_result(
            texture,
            Rect::from_xywh(0.0, 0.0, f64::from(SIZE as f32), f64::from(SIZE as f32)),
            BlendMode::Clear,
        );
    });

    let center = pixel_at(&rgba, SIZE, SIZE / 2, SIZE / 2);
    assert_eq!(
        center,
        [0, 0, 0, 0],
        "centre pixel = {center:?}; a Clear composite of a GREEN offscreen \
         must erase the red frame to transparent."
    );
}
