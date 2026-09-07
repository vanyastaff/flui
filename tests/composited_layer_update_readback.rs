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
use flui_objects::{RenderColoredBox, RenderFlex, RenderOpacity, RenderRepaintBoundary};
use flui_rendering::{
    constraints::BoxConstraints,
    pipeline::PipelineOwner,
    testing::{box_node, tree},
};
use flui_types::{Size, geometry::px};

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
