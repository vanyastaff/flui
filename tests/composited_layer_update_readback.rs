//! Pixel equivalence for issue #536's update-only composited-layer commits.
//!
//! The rest of that feature's tests assert on the layer tree: structure,
//! layer kind, and the emitted alpha. None of them looks at a pixel, and a
//! layer tree that is right in every asserted field can still rasterize
//! differently — that is the gap this file closes, and it is the acceptance
//! criterion #536 names as "GPU readback proves pixel equivalence".
//!
//! It lives in the facade rather than in `flui-rendering` because it needs
//! BOTH the render pipeline (to produce the two layer trees) and
//! `flui_engine::wgpu::HeadlessRenderer` (to rasterize them). Those crates are
//! siblings in layer 4, and the facade is the one place that already depends
//! on both — adding a wgpu dev-dependency to `flui-rendering` just to reach
//! the renderer would pull the whole GPU stack into that crate's test build.

use flui_engine::wgpu::HeadlessRenderer;
use flui_objects::{
    RenderClipRRect, RenderColoredBox, RenderFlex, RenderOpacity, RenderPadding,
    RenderRepaintBoundary, RenderTransform,
};
use flui_rendering::{
    constraints::BoxConstraints,
    pipeline::PipelineOwner,
    testing::{box_node, tree},
};
use flui_types::{
    Matrix4, Size,
    geometry::px,
    styling::{BorderRadius, BorderRadiusExt},
};

const SURFACE: (u32, u32) = (200, 200);

/// Root row → [boundary → opacity → coloured leaves, boundary → leaf].
///
/// The opacity deliberately sits INSIDE a repaint boundary rather than being
/// one: that is the shape the update path is built around, and the leaves are
/// coloured so a wrong alpha is a visible difference rather than a structural
/// one.
fn mount(opacity: f32) -> (PipelineOwner, flui_foundation::RenderId) {
    let content = box_node(RenderFlex::row())
        .children((0..4).map(|_| box_node(RenderColoredBox::red(40.0, 40.0))));

    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(
        &mut owner,
        box_node(RenderFlex::row())
            .child(
                box_node(RenderRepaintBoundary::new()).child(
                    box_node(RenderOpacity::new(opacity))
                        .label("opacity")
                        .child(content),
                ),
            )
            .child(
                box_node(RenderRepaintBoundary::new())
                    .child(box_node(RenderColoredBox::red(20.0, 20.0))),
            ),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(
        px(f32::from(
            u16::try_from(SURFACE.0).expect("surface fits u16"),
        )),
        px(f32::from(
            u16::try_from(SURFACE.1).expect("surface fits u16"),
        )),
    ))));
    let opacity_id = registry.get("opacity").expect("opacity is labelled");
    (owner, opacity_id)
}

fn set_opacity(owner: &mut PipelineOwner, id: flui_foundation::RenderId, value: f32) {
    let impact = owner
        .render_tree_mut()
        .get_mut(id)
        .expect("opacity node")
        .as_box_mut()
        .expect("box entry")
        .render_object_mut()
        .as_any_mut()
        .downcast_mut::<RenderOpacity>()
        .expect("RenderOpacity")
        .set_opacity(value);
    owner.apply_render_update_impact(id, impact);
}

/// Rasterize the second frame after changing the opacity to `value`.
///
/// `force_repaint` picks the arm: an explicit paint mark wins over the
/// layer-update mark the setter reports, so the same mutation goes down the
/// old path.
fn frame_after_alpha_change(
    renderer: &HeadlessRenderer,
    value: f32,
    force_repaint: bool,
) -> Vec<u8> {
    let (owner, opacity_id) = mount(0.5);
    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");

    set_opacity(&mut owner, opacity_id, value);
    if force_repaint {
        owner.mark_needs_paint(opacity_id);
    }
    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("second frame")
        .expect("second frame produces a layer tree");
    let pixels = renderer
        .render_layer_tree(&tree, SURFACE)
        .expect("rasterizing the layer tree");
    drop(owner);
    pixels
}

/// The update path and the repaint path produce the same pixels.
///
/// Both arms apply the SAME alpha change to the SAME tree; the only
/// difference is that one is additionally marked needing paint, which forces
/// the old full-repaint path. Anything the patch gets wrong — a stale alpha, a
/// dropped layer, a subtree replayed at the wrong offset — shows up here as a
/// byte difference, including failures every layer-tree assertion would pass.
#[test]
fn the_update_path_and_a_repaint_produce_the_same_pixels() {
    // No `if let Ok(..) else { return }`: a host without an adapter must fail
    // loudly. A readback test that silently skips is counted as passing and
    // then proves nothing, which is exactly how a GPU oracle goes quietly
    // dead.
    let renderer = HeadlessRenderer::new()
        .expect("a GPU adapter for headless capture (CI runs this on the software rasterizer)");

    let updated = frame_after_alpha_change(&renderer, 0.25, false);
    let repainted = frame_after_alpha_change(&renderer, 0.25, true);

    assert_eq!(
        updated.len(),
        repainted.len(),
        "both arms rasterize the same surface size",
    );

    let differing = updated
        .iter()
        .zip(&repainted)
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        differing,
        0,
        "the update path must be pixel-identical to a full repaint; \
         {differing} of {} bytes differ",
        updated.len(),
    );
}

/// The oracle can tell the two alphas apart.
///
/// Without this, the equivalence test above passes just as well against a
/// renderer that ignores opacity entirely, or a fixture whose content is
/// invisible — the "green that checked nothing" shape. A different alpha must
/// produce different pixels for the comparison to mean anything.
#[test]
fn a_different_alpha_produces_different_pixels() {
    let renderer = HeadlessRenderer::new()
        .expect("a GPU adapter for headless capture (CI runs this on the software rasterizer)");

    let quarter = frame_after_alpha_change(&renderer, 0.25, false);
    let three_quarters = frame_after_alpha_change(&renderer, 0.75, false);

    assert_ne!(
        quarter, three_quarters,
        "alpha 0.25 and 0.75 must rasterize differently, or the equivalence \
         assertion is comparing two images that never depended on alpha",
    );
}

// ---------------------------------------------------------------------------
// Transform: the same pixel-equivalence proof for a matrix update
// ---------------------------------------------------------------------------

/// Root row → [boundary → transform → coloured leaves, boundary → leaf].
///
/// Mirrors `mount` above, substituting `RenderTransform` for `RenderOpacity`.
/// The seeded matrix is a SCALE, never a translation — a translation owns no
/// `TransformLayer` at all (painted as a plain offset), which would route
/// every mutation through `PAINT` instead of the `COMPOSITED_LAYER_UPDATE`
/// path this file exists to prove pixel-identical to a repaint.
fn mount_transform(seed: Matrix4) -> (PipelineOwner, flui_foundation::RenderId) {
    let content = box_node(RenderFlex::row())
        .children((0..4).map(|_| box_node(RenderColoredBox::red(40.0, 40.0))));

    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(
        &mut owner,
        box_node(RenderFlex::row())
            .child(
                box_node(RenderRepaintBoundary::new()).child(
                    box_node(RenderTransform::new(seed))
                        .label("transform")
                        .child(content),
                ),
            )
            .child(
                box_node(RenderRepaintBoundary::new())
                    .child(box_node(RenderColoredBox::red(20.0, 20.0))),
            ),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(
        px(f32::from(
            u16::try_from(SURFACE.0).expect("surface fits u16"),
        )),
        px(f32::from(
            u16::try_from(SURFACE.1).expect("surface fits u16"),
        )),
    ))));
    let transform_id = registry.get("transform").expect("transform is labelled");
    (owner, transform_id)
}

fn set_transform(owner: &mut PipelineOwner, id: flui_foundation::RenderId, matrix: Matrix4) {
    let impact = owner
        .render_tree_mut()
        .get_mut(id)
        .expect("transform node")
        .as_box_mut()
        .expect("box entry")
        .render_object_mut()
        .as_any_mut()
        .downcast_mut::<RenderTransform>()
        .expect("RenderTransform")
        .set_transform(matrix);
    owner.apply_render_update_impact(id, impact);
}

/// Rasterize the second frame after changing the matrix to `matrix`.
///
/// Mirrors `frame_after_alpha_change`: `force_repaint` picks the arm exactly
/// the same way.
fn frame_after_transform_change(
    renderer: &HeadlessRenderer,
    matrix: Matrix4,
    force_repaint: bool,
) -> Vec<u8> {
    let (owner, transform_id) = mount_transform(Matrix4::scaling(2.0, 2.0, 1.0));
    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");

    set_transform(&mut owner, transform_id, matrix);
    if force_repaint {
        owner.mark_needs_paint(transform_id);
    }
    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("second frame")
        .expect("second frame produces a layer tree");
    let pixels = renderer
        .render_layer_tree(&tree, SURFACE)
        .expect("rasterizing the layer tree");
    drop(owner);
    pixels
}

/// The update path and the repaint path produce the same pixels, for a
/// matrix change.
///
/// Same proof as `the_update_path_and_a_repaint_produce_the_same_pixels`,
/// for `RenderTransform` instead of `RenderOpacity`: a stale origin, a
/// dropped layer, or a subtree replayed at the wrong offset shows up here as
/// a byte difference that no layer-tree assertion would catch.
#[test]
fn the_transform_update_path_and_a_repaint_produce_the_same_pixels() {
    let renderer = HeadlessRenderer::new()
        .expect("a GPU adapter for headless capture (CI runs this on the software rasterizer)");

    let updated = frame_after_transform_change(&renderer, Matrix4::scaling(3.0, 3.0, 1.0), false);
    let repainted = frame_after_transform_change(&renderer, Matrix4::scaling(3.0, 3.0, 1.0), true);

    assert_eq!(
        updated.len(),
        repainted.len(),
        "both arms rasterize the same surface size",
    );

    let differing = updated
        .iter()
        .zip(&repainted)
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        differing,
        0,
        "the update path must be pixel-identical to a full repaint; \
         {differing} of {} bytes differ",
        updated.len(),
    );
}

/// The oracle can tell two different matrices apart.
///
/// Without this, the equivalence test above passes just as well against a
/// renderer that ignores the transform entirely. A different scale must
/// produce different pixels for the comparison to mean anything.
#[test]
fn a_different_matrix_produces_different_pixels() {
    let renderer = HeadlessRenderer::new()
        .expect("a GPU adapter for headless capture (CI runs this on the software rasterizer)");

    let smaller = frame_after_transform_change(&renderer, Matrix4::scaling(1.2, 1.2, 1.0), false);
    let larger = frame_after_transform_change(&renderer, Matrix4::scaling(3.0, 3.0, 1.0), false);

    assert_ne!(
        smaller, larger,
        "scale 1.2 and 3.0 must rasterize differently, or the equivalence \
         assertion is comparing two images that never depended on the matrix",
    );
}

// ---------------------------------------------------------------------------
// Clip: the same pixel-equivalence proof for a border-radius update
// ---------------------------------------------------------------------------

/// Root row → [boundary → padding → clip → coloured leaf, boundary → leaf].
///
/// Mirrors `mount`/`mount_transform` above, substituting `RenderClipRRect` for
/// `RenderOpacity`/`RenderTransform`. Unlike those two, this fixture's proof
/// needs a KNOWN, non-zero absolute position — the pixel test below samples
/// right at the clipped corner — so `RenderPadding::all(20.0)` sits between
/// the boundary and the clip. `RenderFlex::row()` defaults both axes to
/// `Start`, so the first row child sits flush at the row's own origin, which
/// is the root's `(0, 0)`; nothing between the row and the padding adds an
/// offset of its own (`RenderRepaintBoundary` is a plain pass-through proxy),
/// so the clip's absolute origin is exactly the padding's inset: `(20, 20)`.
/// `RenderClipRRect` is itself a pass-through proxy (`forward_single_child_box_layout!`),
/// so it adopts the `40x40` coloured box's size unchanged — the box's
/// top-left corner is therefore also at `(20, 20)`. `Clip::HardEdge` (not
/// `AntiAlias`) is deliberate: see `a_different_radius_produces_different_pixels`'s
/// doc for why the sample point never actually needed it, and why hard-edge
/// is still the safer fixture choice.
fn mount_clip_rrect(radius: f32) -> (PipelineOwner, flui_foundation::RenderId) {
    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(
        &mut owner,
        box_node(RenderFlex::row())
            .child(
                box_node(RenderRepaintBoundary::new()).child(
                    box_node(RenderPadding::all(20.0)).child(
                        box_node(
                            RenderClipRRect::hard_edge()
                                .with_border_radius(BorderRadius::circular(px(radius))),
                        )
                        .label("clip")
                        .child(box_node(RenderColoredBox::red(40.0, 40.0))),
                    ),
                ),
            )
            .child(
                box_node(RenderRepaintBoundary::new())
                    .child(box_node(RenderColoredBox::red(20.0, 20.0))),
            ),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(
        px(f32::from(
            u16::try_from(SURFACE.0).expect("surface fits u16"),
        )),
        px(f32::from(
            u16::try_from(SURFACE.1).expect("surface fits u16"),
        )),
    ))));
    let clip_id = registry.get("clip").expect("clip is labelled");
    (owner, clip_id)
}

fn set_border_radius(owner: &mut PipelineOwner, id: flui_foundation::RenderId, radius: f32) {
    let impact = owner
        .render_tree_mut()
        .get_mut(id)
        .expect("clip node")
        .as_box_mut()
        .expect("box entry")
        .render_object_mut()
        .as_any_mut()
        .downcast_mut::<RenderClipRRect>()
        .expect("RenderClipRRect")
        .set_border_radius(Some(BorderRadius::circular(px(radius))));
    owner.apply_render_update_impact(id, impact);
}

/// Rasterize the second frame after changing the border radius to `radius`.
///
/// Mirrors `frame_after_alpha_change`/`frame_after_transform_change`:
/// `force_repaint` picks the arm exactly the same way. The seed radius (5,
/// via `mount_clip_rrect`) is deliberately distinct from every `radius` this
/// file calls with (8 and 2) — same reason `mount`'s seed alpha (0.5) and
/// `mount_transform`'s seed scale (2.0) both sit outside the values their own
/// "different X" tests compare: reusing a compared value as the seed would
/// make that one call a same-value no-op mutation instead of the
/// composited-layer-update this file exists to prove pixel-correct.
fn frame_after_radius_change(
    renderer: &HeadlessRenderer,
    radius: f32,
    force_repaint: bool,
) -> Vec<u8> {
    let (owner, clip_id) = mount_clip_rrect(5.0);
    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");

    set_border_radius(&mut owner, clip_id, radius);
    if force_repaint {
        owner.mark_needs_paint(clip_id);
    }
    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("second frame")
        .expect("second frame produces a layer tree");
    let pixels = renderer
        .render_layer_tree(&tree, SURFACE)
        .expect("rasterizing the layer tree");
    drop(owner);
    pixels
}

/// The pixel at `(x, y)` in a tightly-packed, top-row-first RGBA8 buffer of
/// `width` pixels — the layout `HeadlessRenderer::render_layer_tree` returns.
fn pixel_at(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * width + x) * 4) as usize;
    [
        pixels[idx],
        pixels[idx + 1],
        pixels[idx + 2],
        pixels[idx + 3],
    ]
}

/// The update path and the repaint path produce the same pixels, for a
/// border-radius change (radius 5 seeded, mutated to 2 in both arms).
///
/// Same proof as `the_update_path_and_a_repaint_produce_the_same_pixels` /
/// `the_transform_update_path_and_a_repaint_produce_the_same_pixels`: both
/// arms apply the SAME radius change to the SAME tree, one additionally
/// forced through the full-repaint path. A stale radius, a dropped clip
/// layer, or a subtree replayed at the wrong offset shows up here as a byte
/// difference no layer-tree assertion would catch.
#[test]
fn the_clip_update_path_and_a_repaint_produce_the_same_pixels() {
    let renderer = HeadlessRenderer::new()
        .expect("a GPU adapter for headless capture (CI runs this on the software rasterizer)");

    let updated = frame_after_radius_change(&renderer, 2.0, false);
    let repainted = frame_after_radius_change(&renderer, 2.0, true);

    assert_eq!(
        updated.len(),
        repainted.len(),
        "both arms rasterize the same surface size",
    );

    let differing = updated
        .iter()
        .zip(&repainted)
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        differing,
        0,
        "the update path must be pixel-identical to a full repaint; \
         {differing} of {} bytes differ",
        updated.len(),
    );
}

/// The oracle can tell two different radii apart — sampled right at the
/// clipped corner instead of over the whole buffer, since a corner is the one
/// place in this fixture where the radius alone decides what shows through.
///
/// The clip's absolute origin is `(20, 20)` and its `40x40` child box shares
/// that same top-left corner (see `mount_clip_rrect`'s doc). `(21, 21)` — one
/// pixel diagonally in from that corner — discriminates the two radii this
/// file compares, both well clear of their own rounding edge (never a
/// half-covered, anti-aliasable pixel):
///
/// - radius 8: distance from the rounding center `(8, 8)` (corner-local) to
///   `(1, 1)` is `(8 - 1) * sqrt(2) ≈ 9.90` px, OUTSIDE the radius-8 circle —
///   the corner is clipped away there, so the frame shows the white
///   background.
/// - radius 2: the same point is `(2 - 1) * sqrt(2) ≈ 1.41` px from the
///   rounding center `(2, 2)`, INSIDE the radius-2 circle — the point is
///   still inside the clip, so the frame shows the red box.
///
/// Without this, `the_clip_update_path_and_a_repaint_produce_the_same_pixels`
/// passes just as well against a renderer that ignores the border radius
/// entirely, or a fixture whose clip never reaches the sampled pixel.
#[test]
fn a_different_radius_produces_different_pixels() {
    let renderer = HeadlessRenderer::new()
        .expect("a GPU adapter for headless capture (CI runs this on the software rasterizer)");

    const SAMPLE: (u32, u32) = (21, 21);
    const WHITE: [u8; 4] = [255, 255, 255, 255];
    const RED: [u8; 4] = [255, 0, 0, 255];

    let wide = frame_after_radius_change(&renderer, 8.0, false);
    let narrow = frame_after_radius_change(&renderer, 2.0, false);

    let wide_pixel = pixel_at(&wide, SURFACE.0, SAMPLE.0, SAMPLE.1);
    let narrow_pixel = pixel_at(&narrow, SURFACE.0, SAMPLE.0, SAMPLE.1);

    assert_eq!(
        wide_pixel, WHITE,
        "radius 8 must clip the sample pixel away, leaving the white background",
    );
    assert_eq!(
        narrow_pixel, RED,
        "radius 2 must leave the sample pixel inside the clip, showing the red box",
    );
    assert_ne!(
        wide_pixel, narrow_pixel,
        "radius 8 and 2 must rasterize differently at the clipped corner, or the \
         equivalence assertion above is comparing two images that never depended \
         on the radius",
    );
}
