//! A retained [`SceneVersion`] drawn by the engine's own walk (ADR-0304 §9),
//! read back through the retained target.
//!
//! Each test drives [`RetainedCapture`](crate::headless::RetainedCapture),
//! the windowed renderer's `FrameProtocol` and `record_frame_content`, with
//! the damage the scene owner published for the patch. The oracle is the
//! same frame drawn from the version's flattened tree, which is how the
//! engine drew a version before it walked one.

#![expect(
    deprecated,
    reason = "ADR-0305: the old raster lane, until the window runners move to `RasterExecutor`"
)]

use std::sync::Arc;

use flui_foundation::RenderId;
use flui_foundation::geometry::{Offset, Rect};
use flui_layer::{
    BoundaryContent, Chunk, DamageRegion, Placement, Scene, SceneNodeId, SceneOp, SceneOwner,
    SceneVersion, SceneWriter, SurfaceKey,
};
use flui_painting::styling::Color;
use flui_painting::{Canvas, Paint};

use crate::damage::FramePlan;
use crate::headless::RetainedCapture;
use crate::raster::RasterBackend;

const SIDE: u32 = 128;
const SURFACE: SurfaceKey = SurfaceKey {
    size: (SIDE, SIDE),
    generation: flui_foundation::SurfaceGeneration::ZERO,
};

const WHITE: [u8; 4] = [255, 255, 255, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];

/// Four boundaries in a row, each a 16×16 square at `x = 8 + 28·i`,
/// `y = 8`, painted by its own chunk.
struct Row {
    owner: SceneOwner,
    boxes: [SceneNodeId; 4],
}

impl Row {
    fn new(colors: [Color; 4]) -> Self {
        let mut owner = SceneOwner::new();
        let mut writer = owner.writer();
        let root = writer.current().root();
        let boxes: [SceneNodeId; 4] = std::array::from_fn(|_| writer.mint_node());
        let mut content = BoundaryContent::builder();
        for &node in &boxes {
            content.slot(node);
        }
        for (index, color) in colors.into_iter().enumerate() {
            publish(&mut writer, boxes[index], index, color);
        }
        for (index, &node) in boxes.iter().enumerate() {
            writer.push(SceneOp::Place {
                node,
                placement: Placement::try_new(Offset::new(8.0 + 28.0 * index as f64, 8.0))
                    .expect("a finite offset"),
            });
        }
        writer.push(SceneOp::Publish {
            node: root,
            content: Arc::new(content.finish()),
        });
        owner.apply(SURFACE).expect("the row applies");
        Self { owner, boxes }
    }

    /// Recolours box `index` and answers the damage of the patch.
    fn recolor(&mut self, index: usize, color: Color) -> DamageRegion {
        let mut writer = self.owner.writer();
        publish(&mut writer, self.boxes[index], index, color);
        self.owner.apply(SURFACE).expect("a recolour applies");
        self.owner.patch_damage()
    }

    /// Moves box `index` to `x` and answers the damage of the patch.
    fn place(&mut self, index: usize, x: f64) -> DamageRegion {
        let mut writer = self.owner.writer();
        writer.push(SceneOp::Place {
            node: self.boxes[index],
            placement: Placement::try_new(Offset::new(x, 8.0)).expect("a finite offset"),
        });
        self.owner.apply(SURFACE).expect("a move applies");
        self.owner.patch_damage()
    }

    fn version(&self) -> SceneVersion {
        self.owner.current().clone()
    }
}

/// Queues `node`'s content, box `index`'s square of `color`.
fn publish(writer: &mut SceneWriter<'_>, node: SceneNodeId, index: usize, color: Color) {
    let version = writer.next_content_version().expect("a content version");
    let mut canvas = Canvas::new();
    canvas.draw_rect(Rect::from_xywh(0.0, 0.0, 16.0, 16.0), &Paint::fill(color));
    let mut content = BoundaryContent::builder();
    content.chunk(Arc::new(Chunk::new(
        RenderId::new(index + 2),
        0,
        version,
        canvas.finish(),
    )));
    writer.push(SceneOp::Publish {
        node,
        content: Arc::new(content.finish()),
    });
}

fn apply(capture: &mut RetainedCapture, region: DamageRegion) {
    match region {
        DamageRegion::Region(region) => capture.mark_dirty(region.bounds().to_rect()),
        DamageRegion::Unchanged => {}
        DamageRegion::Full(_) => capture.mark_full_repaint(),
    }
}

fn px(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * SIDE + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

/// The centre of box `index`.
fn centre(index: u32) -> (u32, u32) {
    (16 + 28 * index, 16)
}

/// Draws `version` with `damage` into each capture: one walks the version,
/// the other draws its flattened tree. Answers the plan the walk ran.
fn both(
    walked: &mut RetainedCapture,
    flattened: &mut RetainedCapture,
    version: &SceneVersion,
    damage: DamageRegion,
) -> Option<FramePlan> {
    apply(walked, damage);
    walked
        .render_scene(&Scene::of_version(version.clone()))
        .expect("the walked frame renders");
    apply(flattened, damage);
    flattened
        .render_scene(&Scene::new(version.flatten()))
        .expect("the flattened frame renders");
    assert_eq!(walked.last_plan(), flattened.last_plan());
    walked.last_plan()
}

/// A partial frame of a version redraws what the patch damaged and leaves
/// the rest of the retained target alone, pixel for pixel as the flattened
/// tree draws it. A sentinel painted outside the damage stands for the
/// previous frame and survives; the recoloured box reads its new colour.
#[test]
fn a_partial_frame_of_a_version_draws_as_its_flattened_tree() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let mut walked = renderer.retained_capture((SIDE, SIDE)).expect("capture");
    let mut flattened = renderer.retained_capture((SIDE, SIDE)).expect("capture");
    let mut row = Row::new([Color::RED, Color::RED, Color::RED, Color::RED]);
    let version = row.version();
    let full = DamageRegion::Full(flui_layer::FullReason::FirstFrame);
    both(&mut walked, &mut flattened, &version, full);
    // A one-pixel damage moves both onto the retained target.
    let warm = DamageRegion::Region(flui_layer::Region::new(
        flui_layer::DamageRect::try_covering(Rect::from_ltrb(0.0, 0.0, 1.0, 1.0), SURFACE.size)
            .expect("a finite rect")
            .expect("a rect on the surface"),
    ));
    both(&mut walked, &mut flattened, &version, warm);
    let sentinel = (100, 100, 8, 8);
    walked.paint_retained(sentinel, GREEN);
    flattened.paint_retained(sentinel, GREEN);

    let damage = row.recolor(2, Color::BLUE);
    assert!(matches!(damage, DamageRegion::Region(_)), "{damage:?}");
    let plan = both(&mut walked, &mut flattened, &row.version(), damage);
    assert!(
        matches!(plan, Some(FramePlan::RetainedPartial(_))),
        "{plan:?}"
    );

    assert_eq!(
        differing(&mut walked, &mut flattened),
        0,
        "pixels the walk drew unlike the flattened tree"
    );
    let pixels = walked.read_rgba().expect("readback");
    let (x, y) = centre(2);
    assert_eq!(px(&pixels, x, y), BLUE, "the recoloured box");
    for index in [0, 1, 3] {
        let (x, y) = centre(index);
        assert_eq!(px(&pixels, x, y), RED, "box {index} stands still");
    }
    assert_eq!(
        px(&pixels, 104, 104),
        GREEN,
        "outside the damage, untouched"
    );
    assert_eq!(px(&pixels, 2, 60), WHITE, "the background");
}

/// A frame that panics once recorded commits nothing, so the next frame
/// redraws the whole version whatever its own damage: the box the panicked
/// frame recoloured reads its colour although the next patch damages only
/// another box.
#[test]
fn a_raster_panic_renders_the_next_frame_in_full() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let mut capture = renderer.retained_capture((SIDE, SIDE)).expect("capture");
    let mut row = Row::new([Color::RED, Color::RED, Color::RED, Color::RED]);
    apply(
        &mut capture,
        DamageRegion::Full(flui_layer::FullReason::FirstFrame),
    );
    capture
        .render_scene(&Scene::of_version(row.version()))
        .expect("the first frame renders");
    capture.mark_dirty(Rect::from_ltrb(0.0, 0.0, 1.0, 1.0));
    capture
        .render_scene(&Scene::of_version(row.version()))
        .expect("the warm-up renders");
    assert_eq!(capture.last_plan(), Some(FramePlan::RetainedFull));

    let damage = row.recolor(1, Color::BLUE);
    apply(&mut capture, damage);
    capture.panic_after_next_record();
    let scene = Scene::of_version(row.version());
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: the test holds the raster frame's own panic"
    )]
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        capture.render_scene(&scene)
    }));
    assert!(unwound.is_err(), "the injected panic unwinds the frame");

    // Box 3 alone is damaged now; box 1's colour was never shown.
    let damage = row.recolor(3, Color::BLUE);
    apply(&mut capture, damage);
    capture
        .render_scene(&Scene::of_version(row.version()))
        .expect("the frame after the panic renders");
    let plan = capture.last_plan();
    assert!(
        matches!(plan, Some(FramePlan::Direct | FramePlan::RetainedFull)),
        "a full frame, whatever its damage: {plan:?}"
    );
    let pixels = capture.read_rgba().expect("readback");
    for (index, colour) in [(0, RED), (1, BLUE), (2, RED), (3, BLUE)] {
        let (x, y) = centre(index);
        assert_eq!(px(&pixels, x, y), colour, "box {index}");
    }
}

/// How many pixels of the two captures differ.
fn differing(walked: &mut RetainedCapture, flattened: &mut RetainedCapture) -> usize {
    let pixels = walked.read_rgba().expect("readback");
    let oracle = flattened.read_rgba().expect("readback");
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(oracle.as_chunks::<4>().0)
        .filter(|(walked, flat)| walked != flat)
        .count()
}

/// A damage that ends inside a pixel redraws that whole pixel, so a
/// boundary that reaches only the part of it the damage leaves out still
/// draws its share there, as the flattened tree does.
#[test]
fn a_boundary_in_the_last_partly_damaged_pixel_draws_its_share() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let mut walked = renderer.retained_capture((SIDE, SIDE)).expect("capture");
    let mut flattened = renderer.retained_capture((SIDE, SIDE)).expect("capture");
    let mut row = Row::new([Color::RED, Color::RED, Color::RED, Color::RED]);
    // Box 1 starts 0.7 into pixel 35.
    let _ = row.place(1, 35.7);
    let version = row.version();
    let full = DamageRegion::Full(flui_layer::FullReason::FirstFrame);
    both(&mut walked, &mut flattened, &version, full);
    for capture in [&mut walked, &mut flattened] {
        capture.mark_dirty(Rect::from_ltrb(0.0, 0.0, 1.0, 1.0));
        capture
            .render_scene(&Scene::new(version.flatten()))
            .expect("the warm-up renders");
    }

    // The damage ends at 35.5, short of box 1, and its scissor takes pixel
    // 35 whole.
    let damage = Rect::from_ltrb(20.0, 10.0, 35.5, 20.0);
    walked.mark_dirty(damage);
    walked
        .render_scene(&Scene::of_version(version.clone()))
        .expect("the walked frame renders");
    flattened.mark_dirty(damage);
    flattened
        .render_scene(&Scene::new(version.flatten()))
        .expect("the flattened frame renders");
    assert!(
        matches!(walked.last_plan(), Some(FramePlan::RetainedPartial(_))),
        "{:?}",
        walked.last_plan()
    );
    let pixels = walked.read_rgba().expect("readback");
    assert_ne!(px(&pixels, 35, 16), WHITE, "box 1's share of pixel 35");
    assert_eq!(
        differing(&mut walked, &mut flattened),
        0,
        "pixels the walk drew unlike the flattened tree"
    );
}

/// The root's content: a hard clip of `clip` over one chunk that multiplies
/// `color` over a square larger than the clip.
fn clipped_multiply(writer: &mut SceneWriter<'_>, clip: Rect<f64>, color: Color) {
    let version = writer.next_content_version().expect("a content version");
    let mut canvas = Canvas::new();
    canvas.draw_rect(
        clip.expand(10.0),
        &Paint::fill(color).with_blend_mode(flui_painting::paint::BlendMode::Multiply),
    );
    let mut content = BoundaryContent::builder();
    content.push_effect(
        flui_layer::Layer::from(flui_layer::ClipRectLayer::hard_edge(clip)),
        RenderId::new(2),
        0,
    );
    content.chunk(Arc::new(Chunk::new(
        RenderId::new(3),
        0,
        version,
        canvas.finish(),
    )));
    content.pop_effect();
    let root = writer.current().root();
    writer.push(SceneOp::Publish {
        node: root,
        content: Arc::new(content.finish()),
    });
}

/// A destination-reading chunk larger than the clip over it damages the
/// clip, and the frame that redraws it draws as the flattened tree does:
/// what the chunk reads and writes outside the clip is cut away, so it does
/// not straddle the damage, and the next frame stays partial.
#[test]
fn a_clipped_destination_reading_chunk_redraws_inside_its_clip() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let mut walked = renderer.retained_capture((SIDE, SIDE)).expect("capture");
    let mut flattened = renderer.retained_capture((SIDE, SIDE)).expect("capture");
    let clip = Rect::from_xywh(40.0, 40.0, 24.0, 24.0);
    let mut owner = SceneOwner::new();
    clipped_multiply(&mut owner.writer(), clip, Color::rgba(0, 128, 255, 255));
    owner.apply(SURFACE).expect("the scene applies");
    let version = owner.current().clone();
    both(
        &mut walked,
        &mut flattened,
        &version,
        DamageRegion::Full(flui_layer::FullReason::FirstFrame),
    );
    let warm = Rect::from_ltrb(0.0, 0.0, 1.0, 1.0);
    for capture in [&mut walked, &mut flattened] {
        capture.mark_dirty(warm);
        capture
            .render_scene(&Scene::new(version.flatten()))
            .expect("the warm-up renders");
    }

    clipped_multiply(&mut owner.writer(), clip, Color::rgba(255, 0, 128, 255));
    owner.apply(SURFACE).expect("a recolour applies");
    let damage = owner.patch_damage();
    let DamageRegion::Region(region) = damage else {
        panic!("a recolour damages a region: {damage:?}");
    };
    let damaged = region.bounds().to_rect();
    assert!(
        damaged.contains_rect(&clip) && !damaged.contains_rect(&clip.expand(10.0)),
        "the damage is the clip, short of the chunk: {damaged:?}"
    );
    let plan = both(&mut walked, &mut flattened, owner.current(), damage);
    assert!(
        matches!(plan, Some(FramePlan::RetainedPartial(_))),
        "{plan:?}"
    );
    assert_eq!(
        differing(&mut walked, &mut flattened),
        0,
        "pixels the walk drew unlike the flattened tree"
    );
    // The frame was correct, so nothing forces the next one to repaint in
    // full.
    walked.mark_dirty(warm);
    walked
        .render_scene(&Scene::of_version(owner.current().clone()))
        .expect("the next frame renders");
    assert!(
        matches!(walked.last_plan(), Some(FramePlan::RetainedPartial(_))),
        "a correct frame forced a full repaint: {:?}",
        walked.last_plan()
    );
}
