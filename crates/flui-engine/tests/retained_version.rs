//! A retained scene version, rasterized through the public headless
//! renderer, draws as the tree it flattens to (ADR-0304 §9).

use std::sync::Arc;

use flui_engine::HeadlessRenderer;
use flui_foundation::RenderId;
use flui_foundation::geometry::{Matrix4, Offset, Rect};
use flui_layer::{
    BoundaryContent, Chunk, ClipRectLayer, Layer, OpacityLayer, Placement, Scene, SceneOp,
    SceneOwner, SurfaceKey, TransformLayer,
};
use flui_painting::styling::Color;
use flui_painting::{Canvas, Paint};

const SIDE: u32 = 96;

/// The headless renderer, or `None` on a host without a usable GPU; a run
/// that sets `FLUI_REQUIRE_GPU` fails instead, as every readback suite of
/// the crate does.
fn renderer_or_skip() -> Option<HeadlessRenderer> {
    match pollster::block_on(HeadlessRenderer::new()) {
        Ok(renderer) => Some(renderer),
        Err(error) => {
            assert!(
                std::env::var_os("FLUI_REQUIRE_GPU").is_none(),
                "FLUI_REQUIRE_GPU is set and no GPU is usable: {error}"
            );
            eprintln!("skipping: no usable GPU ({error})");
            None
        }
    }
}

fn square(owner: usize, writer: &mut flui_layer::SceneWriter<'_>, color: Color) -> Arc<Chunk> {
    let mut canvas = Canvas::new();
    canvas.draw_rect(Rect::from_xywh(0.0, 0.0, 24.0, 24.0), &Paint::fill(color));
    Arc::new(Chunk::new(
        RenderId::new(owner),
        0,
        writer.next_content_version().expect("a content version"),
        canvas.finish(),
    ))
}

/// A version whose root holds three boundaries: one plain, one under a
/// rotation and a translucency, one under a clip that cuts it in half.
fn version() -> flui_layer::SceneVersion {
    let mut owner = SceneOwner::new();
    let mut writer = owner.writer();
    let root = writer.current().root();
    let boxes: [_; 3] = std::array::from_fn(|_| writer.mint_node());
    for (index, (&node, color)) in boxes
        .iter()
        .zip([Color::RED, Color::BLUE, Color::GREEN])
        .enumerate()
    {
        let mut content = BoundaryContent::builder();
        content.chunk(square(index + 10, &mut writer, color));
        writer.push(SceneOp::Publish {
            node,
            content: Arc::new(content.finish()),
        });
        writer.push(SceneOp::Place {
            node,
            placement: Placement::try_new(Offset::new(8.0 + 30.0 * index as f64, 8.0))
                .expect("a finite offset"),
        });
    }
    let mut content = BoundaryContent::builder();
    content.slot(boxes[0]);
    content.push_effect(
        Layer::from(TransformLayer::new(Matrix4::rotation_z(0.2))),
        RenderId::new(2),
        0,
    );
    content.push_effect(Layer::from(OpacityLayer::new(0.5)), RenderId::new(2), 1);
    content.slot(boxes[1]);
    content.pop_effect();
    content.pop_effect();
    content.push_effect(
        Layer::from(ClipRectLayer::hard_edge(Rect::from_xywh(
            68.0, 8.0, 12.0, 24.0,
        ))),
        RenderId::new(3),
        0,
    );
    content.slot(boxes[2]);
    content.pop_effect();
    writer.push(SceneOp::Publish {
        node: root,
        content: Arc::new(content.finish()),
    });
    owner
        .apply(SurfaceKey {
            size: (SIDE, SIDE),
            generation: flui_foundation::SurfaceGeneration::ZERO,
        })
        .expect("the scene applies");
    owner.current().clone()
}

fn px(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * SIDE + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

/// The engine walks a version in place and draws it pixel for pixel as the
/// tree it flattens to: each boundary at its placement, under the effects
/// above its slot.
#[test]
fn a_retained_version_renders_as_its_flattened_tree() {
    let Some(renderer) = renderer_or_skip() else {
        return;
    };
    let version = version();
    let walked = renderer
        .render_scene(&Scene::of_version(version.clone()), (SIDE, SIDE))
        .expect("the version renders");
    let flattened = renderer
        .render_layer_tree(&version.flatten(), (SIDE, SIDE))
        .expect("the flattened tree renders");
    assert_eq!(px(&walked, 20, 20), [255, 0, 0, 255], "the plain box");
    assert_eq!(
        px(&walked, 72, 20),
        [0, 255, 0, 255],
        "the clipped box, inside"
    );
    assert_eq!(
        px(&walked, 86, 20),
        [255, 255, 255, 255],
        "the clipped box, cut away"
    );
    let differing = walked
        .as_chunks::<4>()
        .0
        .iter()
        .zip(flattened.as_chunks::<4>().0)
        .filter(|(walked, flat)| walked != flat)
        .count();
    assert_eq!(
        differing, 0,
        "pixels the walk drew unlike the flattened tree"
    );
}
