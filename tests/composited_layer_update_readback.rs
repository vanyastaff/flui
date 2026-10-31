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

impl flui_foundation::inspect::Inspect for RunLocalClipParent {
    fn kind(&self) -> &'static str {
        "RunLocalClipParent"
    }

    fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
}

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

/// A running widget drives both identity-to-layer and retained-layer updates.
/// Interior pixel samples distinguish motion from a stale transform; no frame
/// in the measurement dirties the mounted element tree itself.
fn a_slide_tick_moves_pixels_without_rebuilding() {
    use std::rc::Rc;
    use std::time::Duration;

    use flui_animation::{Animatable, Animation, AnimationController, Tween};
    use flui_objects::TranslationFraction;
    use flui_painting::{Alignment, styling::Color};
    use flui_testing::{HeadlessBinding, MountOptions, MountOwners};
    use flui_widgets::{
        Align, ColoredBox, FocusRoot, GestureArenaScope, SizedBox, SlideTransition,
    };

    let mut binding = HeadlessBinding::new();
    let owner =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(binding.vsync()));
    let scalar: Rc<dyn Animation<f64>> = Rc::new(owner.controller().clone());
    let position = Rc::new(
        Tween::new(
            TranslationFraction::ZERO,
            TranslationFraction::new(2.0, 0.0),
        )
        .animate(scalar),
    );
    let root = GestureArenaScope::new(
        binding.arena().clone(),
        FocusRoot::new(Align::new(Alignment::TOP_LEFT).child(SlideTransition::new(
            position,
            SizedBox::new(40.0, 40.0).child(ColoredBox::new(Color::rgb(255, 0, 0))),
        ))),
    );
    let _mounted = binding.mount_root(
        &root,
        MountOwners::fresh(),
        MountOptions::tight(200.0, 200.0),
    );
    binding.pump_frame(Duration::ZERO);
    let renderer = pollster::block_on(HeadlessRenderer::new()).expect("GPU adapter for readback");
    let rasterize = |binding: &HeadlessBinding| {
        renderer
            .render_layer_tree(binding.layer_tree().expect("mounted slide scene"), SURFACE)
            .expect("rasterizing the moving slide")
    };
    let pixel = |pixels: &[u8], x: usize| {
        let index = (20 * 200 + x) * 4;
        [
            pixels[index],
            pixels[index + 1],
            pixels[index + 2],
            pixels[index + 3],
        ]
    };
    let initial = rasterize(&binding);
    assert_eq!(pixel(&initial, 20), [255, 0, 0, 255]);
    assert_eq!(pixel(&initial, 60), [255, 255, 255, 255]);

    owner.controller().forward().expect("fresh slide run");
    binding.pump_frame(Duration::ZERO);
    for (elapsed, old_pixel, new_pixel) in [
        (0.25, [255, 0, 0, 255], [255, 255, 255, 255]),
        (0.5, [255, 255, 255, 255], [255, 0, 0, 255]),
    ] {
        binding.pump_frame(Duration::from_millis(250));
        assert!((owner.controller().value() - elapsed).abs() < 1e-9);
        let pixels = rasterize(&binding);
        assert_eq!(
            pixel(&pixels, 10),
            [255, 255, 255, 255],
            "the origin clears at {elapsed}"
        );
        assert_eq!(
            pixel(&pixels, 50),
            [255, 0, 0, 255],
            "the shifted box appears at {elapsed}"
        );
        assert_eq!(
            pixel(&pixels, 30),
            old_pixel,
            "outgoing interior at {elapsed}"
        );
        assert_eq!(
            pixel(&pixels, 70),
            new_pixel,
            "incoming interior at {elapsed}"
        );
        assert_eq!(binding.last_frame_report().build.elements_built, 0);
    }
}

/// Fractional edge coverage must apply before the final framebuffer quantization.
fn a_color_tween_preserves_sub_byte_alpha_until_rasterization() {
    use std::rc::Rc;
    use std::time::Duration;

    use flui::painting::{Canvas, CustomPainter, Paint};
    use flui_animation::{Animatable, Animation, AnimationController, Tween};
    use flui_foundation::{Listenable, geometry::Rect};
    use flui_painting::styling::Color;
    use flui_testing::{HeadlessBinding, MountOptions, MountOwners};
    use flui_widgets::{CustomPaint, FocusRoot, GestureArenaScope};

    #[derive(Debug)]
    struct FadePainter(Rc<dyn Animation<Color>>, Rc<std::cell::Cell<Option<Color>>>);

    impl CustomPainter for FadePainter {
        fn paint(&self, canvas: &mut Canvas, size: Size) {
            self.1.set(Some(self.0.value()));
            canvas.draw_rect(
                Rect::from_xywh(0.0, 0.0, size.width, size.height),
                &Paint::fill(Color::BLACK),
            );
            canvas.draw_rect(
                Rect::from_xywh(1.5, 1.0, 13.5, 14.0),
                &Paint::fill(self.0.value()),
            );
        }

        fn should_repaint(&self, _old: &dyn CustomPainter) -> bool {
            false
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn repaint(&self) -> Option<Rc<dyn Listenable>> {
            Some(self.0.clone())
        }
    }

    let mut binding = HeadlessBinding::new();
    let owner =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(binding.vsync()));
    let scalar: Rc<dyn Animation<f64>> = Rc::new(owner.controller().clone());
    let color = Rc::new(Tween::new(Color::rgba(255, 255, 255, 0), Color::WHITE).animate(scalar));
    let painted = Rc::new(std::cell::Cell::new(None));
    let root = GestureArenaScope::new(
        binding.arena().clone(),
        FocusRoot::new(
            CustomPaint::new()
                .size(Size::new(16.0, 16.0))
                .painter(FadePainter(color.clone(), painted.clone())),
        ),
    );
    let _mounted = binding.mount_root(&root, MountOwners::fresh(), MountOptions::tight(16.0, 16.0));
    binding.pump_frame(Duration::ZERO);
    let renderer = pollster::block_on(HeadlessRenderer::new()).expect("GPU adapter for readback");
    owner.controller().forward().expect("fresh color run");
    binding.pump_frame(Duration::ZERO);

    let mut edges = Vec::new();
    let mut interiors = Vec::new();
    let mut samples = Vec::new();
    for (delta, elapsed) in [
        (Duration::from_micros(2500), 0.0025),
        (Duration::from_micros(2500), 0.005),
    ] {
        binding.pump_frame(delta);
        assert!((owner.controller().value() - elapsed).abs() < 1e-9);
        let [r, g, b, alpha] = color.value().to_rgba_f32_array();
        assert!(
            r > 0.999 && g > 0.999 && b > 0.999,
            "white sample: {:?}",
            color.value()
        );
        samples.push((alpha, elapsed));
        assert_eq!(
            painted.get(),
            Some(color.value()),
            "paint accepted latest color"
        );
        let pixels = renderer
            .render_layer_tree(binding.layer_tree().expect("mounted color scene"), (16, 16))
            .expect("rasterizing the color tween");
        let pixel = |x: usize| {
            let index = (8 * 16 + x) * 4;
            <[u8; 4]>::try_from(&pixels[index..index + 4]).expect("RGBA pixel")
        };
        assert_eq!(pixel(0), [0, 0, 0, 255], "black backdrop at {elapsed}");
        interiors.push(pixel(8));
        assert_eq!(binding.last_frame_report().build.elements_built, 0);
        edges.push(pixel(1));
    }
    // The pixel center lies exactly on the straight edge: coverage is 1/2.
    // Continuous alpha produces 0.31875 and 0.6375 encoded byte values.
    // Byte quantization before coverage maps both alphas to the same 1/255,
    // so the two edge pixels cannot differ. Samples avoid the near-half-byte
    // values whose result also depends on the device's blending precision.
    assert_eq!(
        (interiors, edges),
        (
            vec![[1, 1, 1, 255], [1, 1, 1, 255]],
            vec![[0, 0, 0, 255], [1, 1, 1, 255]]
        )
    );
    for (alpha, elapsed) in samples {
        assert!((f64::from(alpha) - elapsed).abs() < 1e-9, "alpha {alpha}");
    }
}

fn group_opacity_retains_fractional_alpha_until_composition() {
    let renderer = pollster::block_on(HeadlessRenderer::new()).expect("GPU adapter for readback");
    for alpha in [0.0025, 0.005] {
        let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
        let (root, _) = tree::mount(
            &mut owner,
            box_node(RenderFlex::row()).child(box_node(RenderOpacity::new(alpha)).child(box_node(
                RenderColoredBox::new([0.0, 0.0, 0.0, 1.0], Size::new(16.0, 16.0)),
            ))),
        );
        owner.set_root_id(Some(root));
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(16.0, 16.0))));
        let (_owner, result) = owner.run_frame();
        let layers = result.expect("opacity frame").expect("painted layer tree");
        assert_eq!(
            flui_rendering::testing::inspect::first_opacity_alpha(&layers),
            Some(alpha),
            "group opacity must reach the layer without byte quantization"
        );
        let pixels = renderer
            .render_layer_tree(&layers, (16, 16))
            .expect("opacity readback");
        let center = (8 * 16 + 8) * 4;
        assert_eq!(
            &pixels[center..center + 4],
            &[254, 254, 254, 255],
            "group alpha {alpha} must survive until composition"
        );
    }
}

fn a_decoration_image_filter_preserves_fractional_alpha_before_modulation() {
    run_cases(
        "decoration image source filters",
        &[
            ("mode", decoration_mode_filter_precision),
            ("matrix", decoration_matrix_filter_precision),
            (
                "tap order and batch identity",
                decoration_filters_preserve_tap_order_and_batch_identity,
            ),
        ],
    );
}

fn decoration_mode_filter_precision() {
    decoration_filter_precision(|alpha| flui_painting::paint::ColorFilter::Mode {
        color: flui_painting::styling::Color::WHITE.with_opacity(f64::from(alpha)),
        blend_mode: flui_painting::paint::BlendMode::SrcIn,
    });
}

fn decoration_filters_preserve_tap_order_and_batch_identity() {
    use flui::painting::CustomPainter;
    use flui_foundation::geometry::Rect;
    use flui_objects::RenderCustomPaint;
    use flui_painting::paint::{BlendMode, BoxFit, ColorFilter, Image};
    use flui_painting::styling::{BoxDecoration, Color, DecorationImage};
    use flui_painting::{Canvas, DecorationPaintOptions, Paint, paint_box_decoration};
    use std::rc::Rc;

    #[derive(Debug)]
    struct FilterPainter(Image);
    impl CustomPainter for FilterPainter {
        fn paint(&self, canvas: &mut Canvas, _size: Size) {
            canvas.draw_rect(
                Rect::from_xywh(0.0, 0.0, 16.0, 16.0),
                &Paint::fill(Color::BLACK),
            );
            // Each middle pixel mixes opaque black and white equally. Applying
            // gamma or ColorDodge after interpolation gives a different result.
            for (x, filter) in [
                (1.0, ColorFilter::SrgbToLinearGamma),
                (5.0, ColorFilter::LinearToSrgbGamma),
                (
                    9.0,
                    ColorFilter::Mode {
                        color: Color::rgb(128, 128, 128),
                        blend_mode: BlendMode::ColorDodge,
                    },
                ),
                (
                    13.0,
                    ColorFilter::Mode {
                        color: Color::RED,
                        blend_mode: BlendMode::Src,
                    },
                ),
            ] {
                let image = DecorationImage::new(self.0.clone_handle())
                    .with_fit(BoxFit::Fill)
                    .with_color_filter(filter);
                paint_box_decoration(
                    canvas,
                    Rect::from_xywh(x, 1.0, 3.0, 14.0),
                    &BoxDecoration::with_image(image),
                    DecorationPaintOptions::default(),
                );
            }
        }
        fn should_repaint(&self, _old: &dyn CustomPainter) -> bool {
            false
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let image = Image::from_rgba8(2, 1, vec![0, 0, 0, 255, 255, 255, 255, 255]);
    let (root, _) = tree::mount(
        &mut owner,
        box_node(RenderCustomPaint::new(
            Some(Rc::new(FilterPainter(image))),
            None,
            Size::new(16.0, 16.0),
        )),
    );
    owner.set_root_id(Some(root));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(16.0, 16.0))));
    let (_, frame) = owner.run_frame();
    let layers = frame
        .expect("filtered image frame")
        .expect("painted layers");
    let renderer = pollster::block_on(HeadlessRenderer::new()).expect("GPU adapter for readback");
    let pixels = renderer
        .render_layer_tree(&layers, (16, 16))
        .expect("filtered source readback");
    for x in [2_usize, 6, 10] {
        let index = (8 * 16 + x) * 4;
        let pixel = &pixels[index..index + 4];
        assert!(
            pixel[..3].iter().all(|channel| channel.abs_diff(128) <= 1),
            "per-tap filtering at {x}: {pixel:?}"
        );
        assert_eq!(pixel[3], 255);
    }
    assert_eq!(
        &pixels[(8 * 16 + 14) * 4..(8 * 16 + 15) * 4],
        &[255, 0, 0, 255],
        "the same source with a different filter must start another batch"
    );
}

fn decoration_matrix_filter_precision() {
    decoration_filter_precision(|alpha| {
        let mut matrix = flui_painting::paint::ColorMatrix::identity();
        matrix.values[18] = 0.0;
        matrix.values[19] = alpha;
        flui_painting::paint::ColorFilter::Matrix(matrix)
    });
}

fn decoration_filter_precision(filter: fn(f32) -> flui_painting::paint::ColorFilter) {
    use std::rc::Rc;

    use flui::painting::CustomPainter;
    use flui_foundation::geometry::Rect;
    use flui_objects::RenderCustomPaint;
    use flui_painting::paint::Image;
    use flui_painting::styling::{BoxDecoration, Color, DecorationImage};
    use flui_painting::{Canvas, DecorationPaintOptions, Paint, paint_box_decoration};

    #[derive(Debug)]
    struct ImagePainter(BoxDecoration<f64>);

    impl CustomPainter for ImagePainter {
        fn paint(&self, canvas: &mut Canvas, size: Size) {
            let bounds = Rect::from_xywh(0.0, 0.0, size.width, size.height);
            canvas.draw_rect(bounds, &Paint::fill(Color::BLACK));
            paint_box_decoration(
                canvas,
                Rect::from_xywh(1.0, 1.0, 14.0, 14.0),
                &self.0,
                DecorationPaintOptions::default(),
            );
        }

        fn should_repaint(&self, _old: &dyn CustomPainter) -> bool {
            false
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    let renderer = pollster::block_on(HeadlessRenderer::new()).expect("GPU adapter for readback");
    for filtered in [false, true] {
        let mut interiors = Vec::new();
        for alpha in [0.0025_f32, 0.005_f32] {
            let image = DecorationImage::new(Image::solid_color(1, 1, Color::WHITE))
                .with_fit(flui_painting::paint::BoxFit::Fill);
            let image = if filtered {
                image.with_opacity(0.5).with_color_filter(filter(alpha))
            } else {
                image.with_opacity(f64::from(alpha) * 0.5)
            };
            let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
            let (root, _) = tree::mount(
                &mut owner,
                box_node(RenderCustomPaint::new(
                    Some(Rc::new(ImagePainter(BoxDecoration::with_image(image)))),
                    None,
                    Size::new(16.0, 16.0),
                )),
            );
            owner.set_root_id(Some(root));
            owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(16.0, 16.0))));
            let (_owner, result) = owner.run_frame();
            let layers = result
                .expect("decoration frame")
                .expect("painted layer tree");
            let pixels = renderer
                .render_layer_tree(&layers, (16, 16))
                .expect("image readback");
            let pixel = |x: usize| {
                <[u8; 4]>::try_from(&pixels[(8 * 16 + x) * 4..(8 * 16 + x) * 4 + 4])
                    .expect("RGBA pixel")
            };
            assert_eq!(
                pixel(0),
                [0, 0, 0, 255],
                "black backdrop, filtered={filtered}"
            );
            interiors.push(pixel(8));
        }
        assert_eq!(
            interiors,
            [[0, 0, 0, 255], [1, 1, 1, 255]],
            "interior, filtered={filtered}"
        );
    }
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
/// updates rasterize exactly as a repaint does, a canvas clip stays in its run when
/// `paint_child` splits the picture, and a running slide moves pixels without
/// rebuilding. The rows run in turn on the available GPU adapter.
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
            (
                "a_slide_tick_moves_pixels_without_rebuilding",
                a_slide_tick_moves_pixels_without_rebuilding,
            ),
            (
                "a_color_tween_preserves_sub_byte_alpha_until_rasterization",
                a_color_tween_preserves_sub_byte_alpha_until_rasterization,
            ),
            (
                "group_opacity_retains_fractional_alpha_until_composition",
                group_opacity_retains_fractional_alpha_until_composition,
            ),
            (
                "a_decoration_image_filter_preserves_fractional_alpha_before_modulation",
                a_decoration_image_filter_preserves_fractional_alpha_before_modulation,
            ),
        ],
    );
}
