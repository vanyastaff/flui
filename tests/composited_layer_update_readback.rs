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
//! `flui_engine::HeadlessRenderer` (to rasterize them). Those crates are
//! siblings in layer 4, and the facade is the one place that already depends
//! on both — adding a wgpu dev-dependency to `flui-rendering` just to reach
//! the renderer would pull the whole GPU stack into that crate's test build.

use flui_engine::HeadlessRenderer;
use flui_foundation::geometry::{Matrix4, Size};
use flui_objects::{
    RenderClipRRect, RenderColoredBox, RenderFlex, RenderOpacity, RenderPadding,
    RenderRepaintBoundary, RenderTransform,
};
use flui_painting::styling::{BorderRadius, BorderRadiusExt};
use flui_rendering::{
    constraints::BoxConstraints,
    pipeline::PipelineOwner,
    testing::{box_node, tree},
};

const SURFACE: (u32, u32) = (200, 200);

/// Root row → [boundary → opacity → coloured leaves, boundary → leaf].
///
/// The opacity deliberately sits INSIDE a repaint boundary rather than being
/// one: that is the shape the update path is built around, and the leaves are
/// coloured so a wrong alpha is a visible difference rather than a structural
/// one.
fn mount(opacity: f64) -> (PipelineOwner, flui_foundation::RenderId) {
    let content = box_node(RenderFlex::row())
        .children((0..4).map(|_| box_node(RenderColoredBox::red(40.0, 40.0))));

    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
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
        f64::from(u16::try_from(SURFACE.0).expect("surface fits u16")),
        f64::from(u16::try_from(SURFACE.1).expect("surface fits u16")),
    ))));
    let opacity_id = registry.get("opacity").expect("opacity is labelled");
    (owner, opacity_id)
}

fn set_opacity(owner: &mut PipelineOwner, id: flui_foundation::RenderId, value: f64) {
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
    value: f64,
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
fn the_update_path_and_a_repaint_produce_the_same_pixels() {
    // No `if let Ok(..) else { return }`: a host without an adapter must fail
    // loudly. A readback test that silently skips is counted as passing and
    // then proves nothing, which is exactly how a GPU oracle goes quietly
    // dead.
    let renderer = pollster::block_on(HeadlessRenderer::new())
        .expect("a GPU adapter for local headless capture");

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

    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
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
        f64::from(u16::try_from(SURFACE.0).expect("surface fits u16")),
        f64::from(u16::try_from(SURFACE.1).expect("surface fits u16")),
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
fn the_transform_update_path_and_a_repaint_produce_the_same_pixels() {
    let renderer = pollster::block_on(HeadlessRenderer::new())
        .expect("a GPU adapter for local headless capture");

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

// ---------------------------------------------------------------------------
// Clip: the same pixel-equivalence proof for a border-radius update
// ---------------------------------------------------------------------------

/// Root row → [boundary → padding → clip → coloured leaf, boundary → leaf].
///
/// Mirrors `mount`/`mount_transform` above, substituting `RenderClipRRect` for
/// `RenderOpacity`/`RenderTransform`. Unlike those two, this fixture puts the
/// clip at a KNOWN, non-zero absolute position: `RenderPadding::all(20.0)`
/// sits between the boundary and the clip. `RenderFlex::row()` defaults both axes to
/// `Start`, so the first row child sits flush at the row's own origin, which
/// is the root's `(0, 0)`; nothing between the row and the padding adds an
/// offset of its own (`RenderRepaintBoundary` is a plain pass-through proxy),
/// so the clip's absolute origin is exactly the padding's inset: `(20, 20)`.
/// `RenderClipRRect` is itself a pass-through proxy (`forward_single_child_box_layout!`),
/// so it adopts the `40x40` coloured box's size unchanged — the box's
/// top-left corner is therefore also at `(20, 20)`. `Clip::HardEdge` (not
/// `AntiAlias`) is deliberate: a hard edge leaves no half-covered pixel whose
/// coverage could differ between the two arms for reasons other than the
/// radius.
fn mount_clip_rrect(radius: f64) -> (PipelineOwner, flui_foundation::RenderId) {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let (root_id, registry) = tree::mount(
        &mut owner,
        box_node(RenderFlex::row())
            .child(
                box_node(RenderRepaintBoundary::new()).child(
                    box_node(RenderPadding::all(20.0)).child(
                        box_node(
                            RenderClipRRect::hard_edge()
                                .with_border_radius(BorderRadius::circular(radius)),
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
        f64::from(u16::try_from(SURFACE.0).expect("surface fits u16")),
        f64::from(u16::try_from(SURFACE.1).expect("surface fits u16")),
    ))));
    let clip_id = registry.get("clip").expect("clip is labelled");
    (owner, clip_id)
}

fn set_border_radius(owner: &mut PipelineOwner, id: flui_foundation::RenderId, radius: f64) {
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
        .set_border_radius(Some(BorderRadius::circular(radius)));
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
    radius: f64,
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

/// The update path and the repaint path produce the same pixels, for a
/// border-radius change (radius 5 seeded, mutated to 2 in both arms).
///
/// Same proof as `the_update_path_and_a_repaint_produce_the_same_pixels` /
/// `the_transform_update_path_and_a_repaint_produce_the_same_pixels`: both
/// arms apply the SAME radius change to the SAME tree, one additionally
/// forced through the full-repaint path. A stale radius, a dropped clip
/// layer, or a subtree replayed at the wrong offset shows up here as a byte
/// difference no layer-tree assertion would catch.
fn the_clip_update_path_and_a_repaint_produce_the_same_pixels() {
    let renderer = pollster::block_on(HeadlessRenderer::new())
        .expect("a GPU adapter for local headless capture");

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

#[derive(Debug)]
struct RunLocalClipParent;

impl flui_foundation::Diagnosticable for RunLocalClipParent {}

impl flui_rendering::traits::RenderBox for RunLocalClipParent {
    type Arity = flui_rendering::prelude::Single;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        ctx.layout_child(0, BoxConstraints::tight(Size::new(20.0, 20.0)))?;
        ctx.position_child(0, flui_foundation::geometry::Offset::new(40.0, 0.0));
        Ok(ctx.constraints().biggest())
    }

    fn paint(&self, ctx: &mut flui_rendering::context::PaintCx<'_, Self::Arity>) {
        use flui_foundation::geometry::Rect;
        use flui_painting::Paint;
        use flui_painting::styling::Color;

        let canvas = ctx.canvas();
        assert_eq!(canvas.save_count(), 1);
        canvas.restore();
        canvas.clip_rect(Rect::from_xywh(0.0, 0.0, 10.0, 20.0));
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, 20.0, 20.0),
            &Paint::fill(Color::RED),
        );
        ctx.paint_child();
        ctx.canvas().draw_rect(
            Rect::from_xywh(60.0, 0.0, 20.0, 20.0),
            &Paint::fill(Color::GREEN),
        );
    }

    fn hit_test(
        &self,
        _ctx: &mut flui_rendering::context::BoxHitTestContext<'_, Self::Arity, Self::ParentData>,
    ) -> bool {
        false
    }
}

fn canvas_clip_stays_in_its_run_when_paint_child_splits_the_picture() {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let (root_id, _) = tree::mount(
        &mut owner,
        box_node(RunLocalClipParent).child(box_node(RenderColoredBox::blue(20.0, 20.0))),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(80.0, 40.0))));
    let (_owner, frame) = owner.run_frame();
    let layer_tree = frame.expect("paint frame").expect("layer tree");
    let renderer = pollster::block_on(HeadlessRenderer::new()).expect("GPU adapter for readback");
    let pixels = renderer
        .render_layer_tree(&layer_tree, (80, 40))
        .expect("readback");
    let pixel = |x: usize| &pixels[(5 * 80 + x) * 4..(5 * 80 + x + 1) * 4];
    assert_eq!(
        pixel(5),
        &[255, 0, 0, 255],
        "the parent clip permits its interior"
    );
    assert_eq!(
        pixel(15),
        &[255, 255, 255, 255],
        "the parent clip still clips its own run"
    );
    assert_eq!(
        pixel(45),
        &[0, 0, 255, 255],
        "the child escapes the parent's run-local clip"
    );
    assert_eq!(
        pixel(65),
        &[0, 255, 0, 255],
        "the resumed parent run starts unclipped"
    );
}

/// Runs every row, then panics once naming each row that failed.
fn run_cases(family: &str, cases: &[(&str, fn())]) {
    let mut failures = Vec::new();
    for (name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(*case) {
            let message = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("<non-string panic payload>");
            failures.push(format!("  {name}: {message}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{family}: {} of {} rows failed:
{}",
        failures.len(),
        cases.len(),
        failures.join(
            "
"
        )
    );
}

/// GPU readbacks for the composited-layer update path: opacity, transform and clip
/// updates rasterize exactly as a repaint does, and a canvas clip stays in its run when
/// `paint_child` splits the picture. One software rasterizer, so the rows run in turn.
#[test]
fn composited_layer_update_readbacks() {
    run_cases(
        "composited_layer_update_readbacks",
        &[
            (
                "the_update_path_and_a_repaint_produce_the_same_pixels",
                the_update_path_and_a_repaint_produce_the_same_pixels,
            ),
            (
                "the_transform_update_path_and_a_repaint_produce_the_same_pixels",
                the_transform_update_path_and_a_repaint_produce_the_same_pixels,
            ),
            (
                "the_clip_update_path_and_a_repaint_produce_the_same_pixels",
                the_clip_update_path_and_a_repaint_produce_the_same_pixels,
            ),
            (
                "canvas_clip_stays_in_its_run_when_paint_child_splits_the_picture",
                canvas_clip_stays_in_its_run_when_paint_child_splits_the_picture,
            ),
        ],
    );
}
