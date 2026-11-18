//! A retained [`SceneVersion`] drawn by the engine's own walk (ADR-0304 §9),
//! read back through the retained target.
//!
//! Each test drives [`RetainedCapture`](crate::headless::RetainedCapture),
//! the windowed renderer's `FrameProtocol` and `record_frame_content`, with
//! the damage the scene owner published for the patch. The oracle is the
//! same frame drawn from the version's flattened tree, which is how the
//! engine drew a version before it walked one.

use std::sync::Arc;

use flui_foundation::RenderId;
use flui_foundation::geometry::{Offset, Rect};
use flui_layer::{
    BoundaryContent, Chunk, DamageRegion, Placement, Scene, SceneNodeId, SceneOp, SceneOwner,
    SceneVersion, SceneWriter, SurfaceKey,
};
use flui_painting::styling::Color;
use flui_painting::{Canvas, Paint};

use crate::damage::{FramePlan, RegionPlan};
use crate::headless::RetainedCapture;

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
    capture.apply_damage(region);
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

/// Two boxes in opposite corners and one between them, under a translucent
/// chunk of the root that covers the whole surface.
struct Corners {
    owner: SceneOwner,
    /// The top-left box, the box between, and the bottom-right box.
    boxes: [SceneNodeId; 3],
}

impl Corners {
    /// Where each box sits: two corners, and between them a box neither
    /// corner's damage reaches.
    const ORIGINS: [(f64, f64); 3] = [(4.0, 4.0), (56.0, 56.0), (108.0, 108.0)];

    fn new() -> Self {
        let mut owner = SceneOwner::new();
        let mut writer = owner.writer();
        let root = writer.current().root();
        let boxes: [SceneNodeId; 3] = std::array::from_fn(|_| writer.mint_node());
        let mut content = BoundaryContent::builder();
        for &node in &boxes {
            content.slot(node);
        }
        // Over the boxes in paint order, so a pass that draws it twice, or
        // a clear it is not drawn over, shows in every pixel it covers.
        let version = writer.next_content_version().expect("a content version");
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, f64::from(SIDE), f64::from(SIDE)),
            &Paint::fill(Color::rgba(0, 160, 0, 96)),
        );
        content.chunk(Arc::new(Chunk::new(
            RenderId::new(10),
            0,
            version,
            canvas.finish(),
        )));
        for (index, color) in [Color::RED, Color::BLUE, Color::RED]
            .into_iter()
            .enumerate()
        {
            publish(&mut writer, boxes[index], index, color);
        }
        for (&node, (x, y)) in boxes.iter().zip(Self::ORIGINS) {
            writer.push(SceneOp::Place {
                node,
                placement: Placement::try_new(Offset::new(x, y)).expect("a finite offset"),
            });
        }
        writer.push(SceneOp::Publish {
            node: root,
            content: Arc::new(content.finish()),
        });
        owner.apply(SURFACE).expect("the corners apply");
        Self { owner, boxes }
    }

    /// Recolours both corner boxes in one patch and answers its damage.
    fn recolor_corners(&mut self, color: Color) -> DamageRegion {
        let mut writer = self.owner.writer();
        publish(&mut writer, self.boxes[0], 0, color);
        publish(&mut writer, self.boxes[2], 2, color);
        self.owner.apply(SURFACE).expect("a recolour applies");
        self.owner.patch_damage()
    }

    fn version(&self) -> SceneVersion {
        self.owner.current().clone()
    }
}

/// A region of two rects in opposite corners redraws rect by rect, each pass
/// under its own scissor and culled by its own rect, and the frame equals a
/// full frame of the same version pixel for pixel. The translucent chunk
/// between the rects, which meets both, blends there once, and no pixel
/// outside the rects is written: one scissor around both rects with the
/// walk culled per rect would clear the box between them without redrawing
/// it and blend the translucent chunk twice.
#[test]
fn region_frames_match_a_full_frame() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let mut partial = renderer.retained_capture((SIDE, SIDE)).expect("capture");
    // Production plans the bounding box (`RegionPlan::of`); this pins the
    // rect-by-rect plan it switches to once a frame records once.
    partial.plan_rect_by_rect();
    let mut corners = Corners::new();
    apply(
        &mut partial,
        DamageRegion::Full(flui_layer::FullReason::FirstFrame),
    );
    partial
        .render_scene(&Scene::of_version(corners.version()))
        .expect("the first frame renders");
    // A one-pixel damage moves the capture onto the retained target.
    partial.mark_dirty(Rect::from_ltrb(0.0, 0.0, 1.0, 1.0));
    partial
        .render_scene(&Scene::of_version(corners.version()))
        .expect("the warm-up renders");
    assert_eq!(partial.last_plan(), Some(FramePlan::RetainedFull));
    let before = partial.read_rgba().expect("readback");

    let damage = corners.recolor_corners(Color::BLUE);
    let DamageRegion::Region(region) = damage else {
        panic!("a recolour damages a region: {damage:?}");
    };
    assert_eq!(region.rects().len(), 2, "one rect per corner: {region:?}");
    apply(&mut partial, damage);
    partial
        .render_scene(&Scene::of_version(corners.version()))
        .expect("the region frame renders");
    assert_eq!(
        partial.last_plan(),
        Some(FramePlan::RetainedPartial(RegionPlan::Rects(region))),
        "two distant rects redraw rect by rect"
    );
    assert_eq!(partial.counters().region_passes, 2, "one pass per rect");

    let mut full = renderer.retained_capture((SIDE, SIDE)).expect("capture");
    apply(
        &mut full,
        DamageRegion::Full(flui_layer::FullReason::FirstFrame),
    );
    full.render_scene(&Scene::of_version(corners.version()))
        .expect("the full frame renders");
    assert_eq!(
        full.counters().region_passes,
        0,
        "a full frame has no region pass"
    );
    assert_eq!(
        differing(&mut partial, &mut full),
        0,
        "pixels the region frame drew unlike the full frame"
    );

    let after = partial.read_rgba().expect("readback");
    let inside = |x: u32, y: u32| {
        region
            .rects()
            .iter()
            .any(|r| (r.left()..r.right()).contains(&x) && (r.top()..r.bottom()).contains(&y))
    };
    let written_outside = (0..SIDE)
        .flat_map(|y| (0..SIDE).map(move |x| (x, y)))
        .filter(|&(x, y)| !inside(x, y) && px(&before, x, y) != px(&after, x, y))
        .count();
    assert_eq!(written_outside, 0, "pixels outside the region changed");
    // The samples the assertions above stand on: the box between the rects
    // and a corner box both read through the translucent chunk.
    let (between, corner) = (px(&after, 64, 64), px(&after, 12, 12));
    assert!(
        between[2] > between[1] && between[1] > 0,
        "the box between, blue under green: {between:?}"
    );
    assert!(
        corner[2] > 0 && corner[0] == 0,
        "the recoloured corner, blue under green: {corner:?}"
    );
}

/// Two tall boxes of `color` over stripes and, when `root_too`, the root:
/// the stripes, the boxes' slots and between them a backdrop filter that
/// reaches below them, which neither box's damage meets.
fn gap_reader(writer: &mut SceneWriter<'_>, boxes: [SceneNodeId; 2], color: Color, root_too: bool) {
    use flui_layer::{BackdropFilterLayer, Layer};
    use flui_painting::paint::{BlendMode, ImageFilter};

    for (index, (&node, x)) in boxes.iter().zip([0.0, 70.0]).enumerate() {
        let version = writer.next_content_version().expect("a content version");
        let mut canvas = Canvas::new();
        canvas.draw_rect(Rect::from_xywh(0.0, 0.0, 30.0, 100.0), &Paint::fill(color));
        let mut boxed = BoundaryContent::builder();
        boxed.chunk(Arc::new(Chunk::new(
            RenderId::new(index + 2),
            0,
            version,
            canvas.finish(),
        )));
        writer.push(SceneOp::Publish {
            node,
            content: Arc::new(boxed.finish()),
        });
        if root_too {
            writer.push(SceneOp::Place {
                node,
                placement: Placement::try_new(Offset::new(x, 0.0)).expect("a finite offset"),
            });
        }
    }
    if !root_too {
        return;
    }
    let root = writer.current().root();
    let mut content = BoundaryContent::builder();
    let version = writer.next_content_version().expect("a content version");
    let mut canvas = Canvas::new();
    canvas.draw_rect(
        Rect::from_xywh(0.0, 0.0, f64::from(SIDE), f64::from(SIDE)),
        &Paint::fill(Color::GREEN),
    );
    for x in (0..SIDE).step_by(8) {
        canvas.draw_rect(
            Rect::from_xywh(f64::from(x), 0.0, 4.0, f64::from(SIDE)),
            &Paint::fill(Color::BLUE),
        );
    }
    content.chunk(Arc::new(Chunk::new(
        RenderId::new(20),
        0,
        version,
        canvas.finish(),
    )));
    for &node in &boxes {
        content.slot(node);
    }
    content.push_effect(
        Layer::from(BackdropFilterLayer::new(
            ImageFilter::blur(3.0),
            BlendMode::SrcOver,
            Rect::from_ltrb(45.0, 60.0, 55.0, 120.0),
        )),
        RenderId::new(21),
        0,
    );
    content.pop_effect();
    writer.push(SceneOp::Publish {
        node: root,
        content: Arc::new(content.finish()),
    });
}

/// A version's region planned as its bounding box has the box closed over
/// the version's backdrop readers: a filter in the gap between the damaged
/// boxes, which neither box's damage meets, is redrawn whole, and the frame
/// draws as a full one. A version has no straddle self-heal, so a filter
/// left cut by the box's scissor would stay wrong in the frames after it.
#[test]
fn a_backdrop_effect_in_the_gap_of_a_bounding_box_is_redrawn_whole() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let mut partial = renderer.retained_capture((SIDE, SIDE)).expect("capture");
    let mut owner = SceneOwner::new();
    let boxes: [SceneNodeId; 2] = {
        let mut writer = owner.writer();
        let boxes = std::array::from_fn(|_| writer.mint_node());
        gap_reader(&mut writer, boxes, Color::RED, true);
        boxes
    };
    owner.apply(SURFACE).expect("the scene applies");
    apply(
        &mut partial,
        DamageRegion::Full(flui_layer::FullReason::FirstFrame),
    );
    partial
        .render_scene(&Scene::of_version(owner.current().clone()))
        .expect("the first frame renders");
    partial.mark_dirty(Rect::from_ltrb(0.0, 0.0, 1.0, 1.0));
    partial
        .render_scene(&Scene::of_version(owner.current().clone()))
        .expect("the warm-up renders");

    gap_reader(&mut owner.writer(), boxes, Color::WHITE, false);
    owner.apply(SURFACE).expect("a recolour applies");
    let damage = owner.patch_damage();
    let DamageRegion::Region(region) = damage else {
        panic!("a recolour damages a region: {damage:?}");
    };
    assert_eq!(region.rects().len(), 2, "one rect per box: {region:?}");
    apply(&mut partial, damage);
    partial
        .render_scene(&Scene::of_version(owner.current().clone()))
        .expect("the region frame renders");
    assert!(
        matches!(
            partial.last_plan(),
            Some(FramePlan::RetainedPartial(RegionPlan::Bounds(_)))
        ),
        "{:?}",
        partial.last_plan()
    );
    let mut full = renderer.retained_capture((SIDE, SIDE)).expect("capture");
    apply(
        &mut full,
        DamageRegion::Full(flui_layer::FullReason::FirstFrame),
    );
    full.render_scene(&Scene::of_version(owner.current().clone()))
        .expect("the full frame renders");
    let (pixels, oracle) = (
        partial.read_rgba().expect("readback"),
        full.read_rgba().expect("readback"),
    );
    let stale = (0..SIDE)
        .flat_map(|y| (0..SIDE).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            px(&pixels, x, y)
                .iter()
                .zip(px(&oracle, x, y))
                .any(|(&a, b)| a.abs_diff(b) > 2)
        })
        .count();
    assert_eq!(
        stale, 0,
        "pixels the region frame drew unlike the full frame"
    );
}

/// Where `grouped_readers` puts the group's first member.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FirstMember {
    /// An effect of the root.
    InRoot,
    /// The effect of its own child node.
    Alone,
    /// Its own child, under a clip that leaves it nothing to write.
    Clipped,
    /// Its own child, and the second member, in a save layer under a clip
    /// that leaves the first nothing to show.
    ClippedThroughLayer,
}

/// A retained root holding a box (`nodes[0]`), then a backdrop group's two
/// members with a white square painted between them. The first member is an
/// effect of the root or, as `placed` says, of its own child `nodes[1]`,
/// there under a clip that hides it, or with the second member in a save
/// layer whose clip hides the first. Without `root_too` only the box's content is published, in `color`.
fn grouped_readers(
    writer: &mut SceneWriter<'_>,
    nodes: [SceneNodeId; 2],
    color: Color,
    group: &flui_layer::BackdropGroupKey,
    placed: FirstMember,
    root_too: bool,
) {
    use flui_layer::{BackdropFilterLayer, Layer};
    use flui_painting::paint::{BlendMode, ImageFilter};

    let picture = |writer: &mut SceneWriter<'_>, id: usize, paint: &dyn Fn(&mut Canvas)| {
        let mut canvas = Canvas::new();
        paint(&mut canvas);
        Arc::new(Chunk::new(
            RenderId::new(id),
            0,
            writer.next_content_version().expect("a content version"),
            canvas.finish(),
        ))
    };
    let mut boxed = BoundaryContent::builder();
    boxed.chunk(picture(writer, 2, &|canvas| {
        canvas.draw_rect(Rect::from_xywh(0.0, 0.0, 24.0, 24.0), &Paint::fill(color));
    }));
    writer.push(SceneOp::Publish {
        node: nodes[0],
        content: Arc::new(boxed.finish()),
    });
    if !root_too {
        return;
    }
    writer.push(SceneOp::Place {
        node: nodes[0],
        placement: Placement::try_new(Offset::new(100.0, 4.0)).expect("a finite offset"),
    });
    let member = |bounds: Rect<f64>| {
        Layer::from(
            BackdropFilterLayer::new(ImageFilter::blur(3.0), BlendMode::SrcOver, bounds)
                .with_group(Some(group.clone())),
        )
    };
    let first = member(Rect::from_ltrb(8.0, 8.0, 24.0, 24.0));
    let mut content = BoundaryContent::builder();
    content.chunk(picture(writer, 20, &|canvas| {
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, f64::from(SIDE), f64::from(SIDE)),
            &Paint::fill(Color::GREEN),
        );
        for x in (0..SIDE).step_by(8) {
            canvas.draw_rect(
                Rect::from_xywh(f64::from(x), 0.0, 4.0, f64::from(SIDE)),
                &Paint::fill(Color::BLUE),
            );
        }
    }));
    content.slot(nodes[0]);
    if placed == FirstMember::ClippedThroughLayer {
        // A save layer under a clip that shows the second member alone: the
        // first draws into the layer, whose composite the clip cuts.
        content.push_effect(
            Layer::from(flui_layer::ClipRectLayer::new(
                Rect::from_ltrb(90.0, 0.0, 128.0, 40.0),
                flui_painting::paint::Clip::AntiAliasWithSaveLayer,
            )),
            RenderId::new(5),
            0,
        );
    }
    if placed == FirstMember::InRoot {
        content.push_effect(first, RenderId::new(21), 0);
        content.pop_effect();
    } else {
        let mut alone = BoundaryContent::builder();
        alone.push_effect(first, RenderId::new(3), 0);
        alone.pop_effect();
        writer.push(SceneOp::Publish {
            node: nodes[1],
            content: Arc::new(alone.finish()),
        });
        writer.push(SceneOp::Place {
            node: nodes[1],
            placement: Placement::try_new(Offset::ZERO).expect("a finite offset"),
        });
        if placed == FirstMember::Clipped {
            // Over none of the member's pixels.
            content.push_effect(
                Layer::from(flui_layer::ClipRectLayer::new(
                    Rect::from_ltrb(60.0, 8.0, 70.0, 24.0),
                    flui_painting::paint::Clip::HardEdge,
                )),
                RenderId::new(4),
                0,
            );
            content.slot(nodes[1]);
            content.pop_effect();
        } else {
            content.slot(nodes[1]);
        }
    }
    // Painted after the group's capture: the second member must not see it.
    content.chunk(picture(writer, 22, &|canvas| {
        canvas.draw_rect(
            Rect::from_xywh(106.0, 10.0, 12.0, 12.0),
            &Paint::fill(Color::WHITE),
        );
    }));
    content.push_effect(
        member(Rect::from_ltrb(104.0, 8.0, 120.0, 24.0)),
        RenderId::new(23),
        0,
    );
    content.pop_effect();
    if placed == FirstMember::ClippedThroughLayer {
        content.pop_effect();
    }
    writer.push(SceneOp::Publish {
        node: writer.current().root(),
        content: Arc::new(content.finish()),
    });
}

/// A version's backdrop group reads the backdrop as it stood at its first
/// member, in a partial frame as in a full one: damage that meets only the
/// second member still replays the first, which captures, so the second
/// does not blur the square painted between them. Each row puts the first
/// member somewhere the damage does not reach; the last plans one pass per
/// rect, with a second rect far from the group.
#[test]
fn a_backdrop_group_outside_the_damage_still_captures_at_its_first_member() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    // (case, where the first member is, one pass per rect)
    for (case, placed, by_rect) in [
        ("first member in the root", FirstMember::InRoot, false),
        ("first member in its own node", FirstMember::Alone, false),
        ("one pass per rect", FirstMember::Alone, true),
        // It writes nothing, in a full frame as in a partial one.
        ("first member hidden by a clip", FirstMember::Clipped, false),
        (
            "first member hidden by a clip over a save layer",
            FirstMember::ClippedThroughLayer,
            false,
        ),
    ] {
        let group = flui_layer::BackdropGroupKey::new();
        let mut partial = renderer.retained_capture((SIDE, SIDE)).expect("capture");
        let mut owner = SceneOwner::new();
        let nodes: [SceneNodeId; 2] = {
            let mut writer = owner.writer();
            let nodes = std::array::from_fn(|_| writer.mint_node());
            grouped_readers(&mut writer, nodes, Color::RED, &group, placed, true);
            nodes
        };
        owner.apply(SURFACE).expect("the scene applies");
        apply(
            &mut partial,
            DamageRegion::Full(flui_layer::FullReason::FirstFrame),
        );
        partial
            .render_scene(&Scene::of_version(owner.current().clone()))
            .expect("the first frame renders");
        partial.mark_dirty(Rect::from_ltrb(0.0, 0.0, 1.0, 1.0));
        partial
            .render_scene(&Scene::of_version(owner.current().clone()))
            .expect("the warm-up renders");

        grouped_readers(
            &mut owner.writer(),
            nodes,
            Color::BLUE,
            &group,
            placed,
            false,
        );
        owner.apply(SURFACE).expect("a recolour applies");
        apply(&mut partial, owner.patch_damage());
        if by_rect {
            // A second rect far from the group.
            partial.plan_rect_by_rect();
            partial.mark_dirty(Rect::from_xywh(50.0, 100.0, 10.0, 10.0));
        }
        partial
            .render_scene(&Scene::of_version(owner.current().clone()))
            .expect("the partial frame renders");
        assert!(
            matches!(partial.last_plan(), Some(FramePlan::RetainedPartial(_))),
            "{case}: the frame is partial: {:?}",
            partial.last_plan()
        );
        if by_rect {
            // The group's members share one rect, the far one has its own.
            assert_eq!(partial.counters().region_passes, 2, "{case}: two passes");
        }
        let mut full = renderer.retained_capture((SIDE, SIDE)).expect("capture");
        apply(
            &mut full,
            DamageRegion::Full(flui_layer::FullReason::FirstFrame),
        );
        full.render_scene(&Scene::of_version(owner.current().clone()))
            .expect("the full frame renders");
        let (pixels, oracle) = (
            partial.read_rgba().expect("readback"),
            full.read_rgba().expect("readback"),
        );
        // Hidden, the first member does not capture: the second reads the square.
        assert_eq!(
            px(&oracle, 112, 16)[0] < 200,
            !matches!(
                placed,
                FirstMember::Clipped | FirstMember::ClippedThroughLayer
            ),
            "{case}: precondition: the second member blurs the backdrop as it stood at the capture"
        );
        let stale = (0..SIDE)
            .flat_map(|y| (0..SIDE).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                px(&pixels, x, y)
                    .iter()
                    .zip(px(&oracle, x, y))
                    .any(|(&a, b)| a.abs_diff(b) > 2)
            })
            .count();
        assert_eq!(
            stale, 0,
            "{case}: pixels the partial frame drew unlike the full frame"
        );
    }
}

/// The layers that `clipped_reader` puts around its content.
#[derive(Clone, Copy, Debug)]
enum LayerClip {
    /// A clip that opens its own layer and cuts that layer's composite.
    SaveLayer,
    /// A hard clip over an opacity layer, which takes the clip to its
    /// composite.
    HardOverOpacity,
    /// A save-layer clip over an image filter's layer: the clip shows what
    /// the filter draws from the input it requires, a little past the clip.
    SaveLayerOverImageFilter,
    /// No clip: a layer that changes nothing, an image filter composing no
    /// filter, which isolates nothing in any frame.
    Unchanging,
}

/// A retained root over stripes, then, under `clip` (x `left`..128), a red
/// box (x 70..`right`) left of the clip (`nodes[0]`), a box at (100, 4) in `color`
/// (`nodes[1]`) and a blur reading the clip's left edge. Without
/// `root_too` only the second box's content is published.
fn clipped_reader(
    writer: &mut SceneWriter<'_>,
    nodes: [SceneNodeId; 2],
    color: Color,
    clip: LayerClip,
    [left, right]: [f64; 2],
    root_too: bool,
) {
    use flui_layer::{BackdropFilterLayer, ClipRectLayer, ImageFilterLayer, Layer, OpacityLayer};
    use flui_painting::paint::{BlendMode, Clip, ImageFilter};

    let picture = |writer: &mut SceneWriter<'_>, id: usize, paint: &dyn Fn(&mut Canvas)| {
        let mut canvas = Canvas::new();
        paint(&mut canvas);
        Arc::new(Chunk::new(
            RenderId::new(id),
            0,
            writer.next_content_version().expect("a content version"),
            canvas.finish(),
        ))
    };
    let mut boxed = BoundaryContent::builder();
    boxed.chunk(picture(writer, 2, &|canvas| {
        canvas.draw_rect(Rect::from_xywh(0.0, 0.0, 24.0, 24.0), &Paint::fill(color));
    }));
    writer.push(SceneOp::Publish {
        node: nodes[1],
        content: Arc::new(boxed.finish()),
    });
    if !root_too {
        return;
    }
    let mut boxed_left = BoundaryContent::builder();
    boxed_left.chunk(picture(writer, 3, &|canvas| {
        canvas.draw_rect(
            Rect::from_ltrb(70.0, 4.0, right, 28.0),
            &Paint::fill(Color::RED),
        );
    }));
    writer.push(SceneOp::Publish {
        node: nodes[0],
        content: Arc::new(boxed_left.finish()),
    });
    for (node, at) in [
        (nodes[0], Offset::ZERO),
        (nodes[1], Offset::new(100.0, 4.0)),
    ] {
        writer.push(SceneOp::Place {
            node,
            placement: Placement::try_new(at).expect("a finite offset"),
        });
    }
    let mut content = BoundaryContent::builder();
    content.chunk(picture(writer, 20, &|canvas| {
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, f64::from(SIDE), f64::from(SIDE)),
            &Paint::fill(Color::GREEN),
        );
        for x in (0..SIDE).step_by(8) {
            canvas.draw_rect(
                Rect::from_xywh(f64::from(x), 0.0, 4.0, f64::from(SIDE)),
                &Paint::fill(Color::BLUE),
            );
        }
    }));
    let window = Rect::from_ltrb(left, 0.0, 128.0, 40.0);
    let layers = match clip {
        LayerClip::SaveLayer => vec![Layer::from(ClipRectLayer::new(
            window,
            Clip::AntiAliasWithSaveLayer,
        ))],
        LayerClip::HardOverOpacity => vec![
            Layer::from(ClipRectLayer::new(window, Clip::HardEdge)),
            Layer::from(OpacityLayer::new(0.5)),
        ],
        LayerClip::Unchanging => vec![Layer::from(ImageFilterLayer::new(ImageFilter::Compose(
            Vec::new(),
        )))],
        LayerClip::SaveLayerOverImageFilter => vec![
            Layer::from(ClipRectLayer::new(window, Clip::AntiAliasWithSaveLayer)),
            Layer::from(ImageFilterLayer::new(ImageFilter::blur(1.0))),
        ],
    };
    let depth = layers.len();
    for (index, layer) in layers.into_iter().enumerate() {
        content.push_effect(layer, RenderId::new(4 + index), 0);
    }
    content.slot(nodes[0]);
    content.slot(nodes[1]);
    content.push_effect(
        Layer::from(BackdropFilterLayer::new(
            ImageFilter::blur(3.0),
            BlendMode::SrcOver,
            Rect::from_ltrb(90.0, 8.0, 110.0, 24.0),
        )),
        RenderId::new(23),
        0,
    );
    content.pop_effect();
    for _ in 0..depth {
        content.pop_effect();
    }
    writer.push(SceneOp::Publish {
        node: writer.current().root(),
        content: Arc::new(content.finish()),
    });
}

/// A backdrop filter inside a layer whose composite a clip cuts reads only
/// what the clip shows. The layer's children draw into it unclipped, but a
/// partial frame skips a node the clip hides, so a filter that read past
/// the clip would blur the hidden box in a full frame and not in a partial
/// one. At a fractional edge the box ends where the clip starts, inside the
/// pixel the clip's edge covers in part, which shows it. A layer that
/// changes nothing isolates no backdrop, in a partial frame as in a full one:
/// its children keep the damage scissor rather than give it to a composite.
#[test]
fn a_backdrop_filter_in_a_clipped_layer_reads_only_inside_the_clip() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    // (clip, [clip's left edge, the hidden box's right edge])
    for (clip, edges) in [
        (LayerClip::SaveLayer, [90.0, 86.0]),
        (LayerClip::HardOverOpacity, [90.0, 86.0]),
        (LayerClip::SaveLayerOverImageFilter, [90.0, 86.0]),
        (LayerClip::SaveLayer, [89.6, 89.6]),
        (LayerClip::HardOverOpacity, [89.4, 89.4]),
        (LayerClip::Unchanging, [90.0, 86.0]),
    ] {
        let mut partial = renderer.retained_capture((SIDE, SIDE)).expect("capture");
        let mut owner = SceneOwner::new();
        let nodes: [SceneNodeId; 2] = {
            let mut writer = owner.writer();
            let nodes = std::array::from_fn(|_| writer.mint_node());
            clipped_reader(&mut writer, nodes, Color::WHITE, clip, edges, true);
            nodes
        };
        owner.apply(SURFACE).expect("the scene applies");
        apply(
            &mut partial,
            DamageRegion::Full(flui_layer::FullReason::FirstFrame),
        );
        partial
            .render_scene(&Scene::of_version(owner.current().clone()))
            .expect("the first frame renders");
        partial.mark_dirty(Rect::from_ltrb(0.0, 0.0, 1.0, 1.0));
        partial
            .render_scene(&Scene::of_version(owner.current().clone()))
            .expect("the warm-up renders");

        clipped_reader(&mut owner.writer(), nodes, Color::BLUE, clip, edges, false);
        owner.apply(SURFACE).expect("a recolour applies");
        apply(&mut partial, owner.patch_damage());
        partial
            .render_scene(&Scene::of_version(owner.current().clone()))
            .expect("the partial frame renders");
        assert!(
            matches!(partial.last_plan(), Some(FramePlan::RetainedPartial(_))),
            "{clip:?} at {edges:?}: the frame is partial: {:?}",
            partial.last_plan()
        );
        let mut full = renderer.retained_capture((SIDE, SIDE)).expect("capture");
        apply(
            &mut full,
            DamageRegion::Full(flui_layer::FullReason::FirstFrame),
        );
        full.render_scene(&Scene::of_version(owner.current().clone()))
            .expect("the full frame renders");
        let (pixels, oracle) = (
            partial.read_rgba().expect("readback"),
            full.read_rgba().expect("readback"),
        );
        let stale = (0..SIDE)
            .flat_map(|y| (0..SIDE).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                px(&pixels, x, y)
                    .iter()
                    .zip(px(&oracle, x, y))
                    .any(|(&a, b)| a.abs_diff(b) > 2)
            })
            .count();
        assert_eq!(
            stale, 0,
            "{clip:?} at {edges:?}: pixels the partial frame drew unlike the full frame"
        );
    }
}

/// Publishes `holder`, whose own content clips a box ending at `right` with
/// an anti-aliased clip from 89.6, filled with `color`, and slots it in the
/// root at the origin when `root_too`.
fn content_clipped_box(
    writer: &mut SceneWriter<'_>,
    holder: SceneNodeId,
    color: Color,
    right: f64,
    root_too: bool,
) {
    use flui_layer::{ClipRectLayer, Layer};
    use flui_painting::paint::Clip;

    let mut canvas = Canvas::new();
    canvas.draw_rect(Rect::from_ltrb(70.0, 4.0, right, 28.0), &Paint::fill(color));
    let mut content = BoundaryContent::builder();
    content.push_effect(
        Layer::from(ClipRectLayer::new(
            Rect::from_ltrb(89.6, 0.0, 128.0, 40.0),
            Clip::AntiAlias,
        )),
        RenderId::new(1),
        0,
    );
    content.chunk(Arc::new(Chunk::new(
        RenderId::new(2),
        0,
        writer.next_content_version().expect("a content version"),
        canvas.finish(),
    )));
    content.pop_effect();
    writer.push(SceneOp::Publish {
        node: holder,
        content: Arc::new(content.finish()),
    });
    if !root_too {
        return;
    }
    writer.push(SceneOp::Place {
        node: holder,
        placement: Placement::try_new(Offset::ZERO).expect("a finite offset"),
    });
    let mut top = BoundaryContent::builder();
    top.slot(holder);
    let root = writer.current().root();
    writer.push(SceneOp::Publish {
        node: root,
        content: Arc::new(top.finish()),
    });
}

/// A box that a clip in its node's own content cuts away but for the pixel
/// the clip's anti-aliased edge covers in part: a partial frame repaints
/// that pixel as a full frame does.
#[test]
fn a_box_a_content_clip_shows_in_one_pixel_repaints_in_a_partial_frame() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    for right in [89.3, 89.6] {
        let mut partial = renderer.retained_capture((SIDE, SIDE)).expect("capture");
        let mut owner = SceneOwner::new();
        let holder = {
            let mut writer = owner.writer();
            let holder = writer.mint_node();
            content_clipped_box(&mut writer, holder, Color::RED, right, true);
            holder
        };
        owner.apply(SURFACE).expect("the scene applies");
        apply(
            &mut partial,
            DamageRegion::Full(flui_layer::FullReason::FirstFrame),
        );
        partial
            .render_scene(&Scene::of_version(owner.current().clone()))
            .expect("the first frame renders");
        partial.mark_dirty(Rect::from_ltrb(0.0, 0.0, 1.0, 1.0));
        partial
            .render_scene(&Scene::of_version(owner.current().clone()))
            .expect("the warm-up renders");

        content_clipped_box(&mut owner.writer(), holder, Color::BLUE, right, false);
        owner.apply(SURFACE).expect("a recolour applies");
        apply(&mut partial, owner.patch_damage());
        partial
            .render_scene(&Scene::of_version(owner.current().clone()))
            .expect("the partial frame renders");
        assert!(
            matches!(partial.last_plan(), Some(FramePlan::RetainedPartial(_))),
            "box to {right}: the frame is partial: {:?}",
            partial.last_plan()
        );
        let mut full = renderer.retained_capture((SIDE, SIDE)).expect("capture");
        apply(
            &mut full,
            DamageRegion::Full(flui_layer::FullReason::FirstFrame),
        );
        full.render_scene(&Scene::of_version(owner.current().clone()))
            .expect("the full frame renders");
        let (pixels, oracle) = (
            partial.read_rgba().expect("readback"),
            full.read_rgba().expect("readback"),
        );
        assert_ne!(
            px(&oracle, 89, 16),
            px(&oracle, 120, 16),
            "box to {right}: the clip's edge pixel shows the box"
        );
        let stale = (0..SIDE)
            .flat_map(|y| (0..SIDE).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                px(&pixels, x, y)
                    .iter()
                    .zip(px(&oracle, x, y))
                    .any(|(&a, b)| a.abs_diff(b) > 2)
            })
            .count();
        assert_eq!(
            stale, 0,
            "box to {right}: pixels the partial frame drew unlike the full frame"
        );
    }
}

/// One full frame of stripes and bars in save layers nested under `clips` (each an
/// x range), under an image filter's blur of `spread` when there is one,
/// with a blur backdrop over `backdrop` after them.
fn clipped_stripes(
    renderer: &crate::headless::HeadlessRenderer,
    clips: &[(f64, f64)],
    spread: Option<f64>,
    backdrop: Option<Rect<f64>>,
) -> Vec<u8> {
    use flui_layer::{BackdropFilterLayer, ClipRectLayer, ImageFilterLayer, Layer};
    use flui_painting::paint::{BlendMode, Clip, ImageFilter};

    let mut owner = SceneOwner::new();
    {
        let mut writer = owner.writer();
        let mut canvas = Canvas::new();
        for x in (60..SIDE).step_by(4) {
            let color = if x % 8 == 0 {
                Color::BLUE
            } else {
                Color::GREEN
            };
            canvas.draw_rect(
                Rect::from_xywh(f64::from(x), 0.0, 4.0, 40.0),
                &Paint::fill(color),
            );
        }
        // Black bars across, one over row 16, so a column also blurs along
        // its length.
        for y in [7.0, 15.0, 23.0, 31.0] {
            canvas.draw_rect(
                Rect::from_xywh(60.0, y, f64::from(SIDE) - 60.0, 2.0),
                &Paint::fill(Color::BLACK),
            );
        }
        let stripes = Arc::new(Chunk::new(
            RenderId::new(2),
            0,
            writer.next_content_version().expect("a content version"),
            canvas.finish(),
        ));
        let mut content = BoundaryContent::builder();
        for (index, &(left, right)) in clips.iter().enumerate() {
            content.push_effect(
                Layer::from(ClipRectLayer::new(
                    Rect::from_ltrb(left, 0.0, right, 40.0),
                    Clip::AntiAliasWithSaveLayer,
                )),
                RenderId::new(10 + index),
                0,
            );
        }
        if let Some(sigma) = spread {
            content.push_effect(
                Layer::from(ImageFilterLayer::new(ImageFilter::blur(sigma))),
                RenderId::new(4),
                0,
            );
        }
        content.chunk(stripes);
        if let Some(bounds) = backdrop {
            content.push_effect(
                Layer::from(BackdropFilterLayer::new(
                    ImageFilter::blur(3.0),
                    BlendMode::SrcOver,
                    bounds,
                )),
                RenderId::new(5),
                0,
            );
            content.pop_effect();
        }
        if spread.is_some() {
            content.pop_effect();
        }
        for _ in clips {
            content.pop_effect();
        }
        writer.push(SceneOp::Publish {
            node: writer.current().root(),
            content: Arc::new(content.finish()),
        });
    }
    owner.apply(SURFACE).expect("the scene applies");
    let mut full = renderer.retained_capture((SIDE, SIDE)).expect("capture");
    apply(
        &mut full,
        DamageRegion::Full(flui_layer::FullReason::FirstFrame),
    );
    full.render_scene(&Scene::of_version(owner.current().clone()))
        .expect("the frame renders");
    full.read_rgba().expect("readback")
}

/// Whether any pixel of row 16 between `columns` differs in the two frames.
fn row_differs(a: &[u8], b: &[u8], columns: std::ops::Range<u32>) -> bool {
    columns.into_iter().any(|x| {
        px(a, x, 16)
            .iter()
            .zip(px(b, x, 16))
            .any(|(&a, b)| a.abs_diff(b) > 2)
    })
}

/// A backdrop filter under an image filter's layer draws what the image
/// filter spreads into a clip over that layer, though the filter's own
/// output lies past the clip: the clip bounds the filter by the input the
/// image filter requires to draw what the clip shows, not by the clip.
#[test]
fn a_backdrop_filter_under_an_image_filter_draws_what_the_filter_spreads_into_the_clip() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    // Wholly left of the clip, inside what the blur draws it from.
    let backdrop = Rect::from_ltrb(84.0, 8.0, 90.0, 24.0);
    let with = clipped_stripes(&renderer, &[(90.0, 128.0)], Some(2.0), Some(backdrop));
    let without = clipped_stripes(&renderer, &[(90.0, 128.0)], Some(2.0), None);
    assert!(
        row_differs(&with, &without, 90..96),
        "the image filter spreads the backdrop filter's output into the clip"
    );
}

/// A backdrop filter in a clipped layer filters the pixel a clip's
/// fractional edge shows in part, as it does the pixels the clip covers,
/// and the pixel two nested clips' edges share though their exact
/// intersection is empty.
#[test]
fn a_backdrop_filter_filters_the_pixel_a_clip_edge_shows_in_part() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let backdrop = Some(Rect::from_ltrb(80.0, 8.0, 110.0, 24.0));
    // (case, the nested clips, the pixel their edges show in part)
    for (case, clips, edge) in [
        ("one clip", &[(90.6, 128.0)][..], 90),
        (
            "two clips sharing a pixel",
            &[(89.6, 128.0), (0.0, 89.4)][..],
            89,
        ),
    ] {
        let with = clipped_stripes(&renderer, clips, None, backdrop);
        let without = clipped_stripes(&renderer, clips, None, None);
        assert_ne!(
            px(&without, edge, 16),
            px(&without, 0, 16),
            "{case}: precondition: the clips show the pixel their edges cover in part"
        );
        if clips.len() == 1 {
            assert!(
                row_differs(&with, &without, edge + 1..edge + 2),
                "{case}: precondition: the filter blurs the stripes the clip covers"
            );
        }
        assert!(
            row_differs(&with, &without, edge..edge + 1),
            "{case}: the filter blurs the pixel the clip's edge covers in part: {:?} {:?}",
            px(&with, edge, 16),
            px(&without, edge, 16)
        );
    }
}

/// A frame counts what its walk and recording did (ADR-0304 §9): a full
/// frame enters every boundary and records every chunk without asking the
/// cull, a partial frame over one box enters the root and that box, asks the
/// cull about the root's four slots and the box's chunk, and records that
/// chunk alone in one pass, and a frame with nothing to draw counts nothing.
#[test]
fn a_frame_counts_the_chunks_it_records() {
    use crate::frame_protocol::RasterCounters;
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
    let counters = capture.counters();
    assert!(
        counters.recording_cache_bytes > 0,
        "the row's recordings are kept"
    );
    assert_eq!(
        counters,
        RasterCounters {
            nodes_entered: 5,
            cull_tests: 0,
            chunks_recorded: 4,
            region_passes: 0,
            recording_cache_hits: 0,
            recording_cache_evictions: 0,
            recording_cache_bytes: counters.recording_cache_bytes,
        },
        "a full frame walks the whole row"
    );
    capture.mark_dirty(Rect::from_ltrb(0.0, 0.0, 1.0, 1.0));
    capture
        .render_scene(&Scene::of_version(row.version()))
        .expect("the warm-up renders");
    assert_eq!(capture.last_plan(), Some(FramePlan::RetainedFull));

    let damage = row.recolor(1, Color::BLUE);
    apply(&mut capture, damage);
    capture
        .render_scene(&Scene::of_version(row.version()))
        .expect("the partial frame renders");
    assert!(
        matches!(capture.last_plan(), Some(FramePlan::RetainedPartial(_))),
        "{:?}",
        capture.last_plan()
    );
    let counters = capture.counters();
    assert_eq!(
        counters,
        RasterCounters {
            nodes_entered: 2,
            cull_tests: 5,
            chunks_recorded: 1,
            region_passes: 1,
            recording_cache_hits: 0,
            recording_cache_evictions: 0,
            recording_cache_bytes: counters.recording_cache_bytes,
        },
        "a partial frame over one box"
    );

    capture
        .render_scene(&Scene::of_version(row.version()))
        .expect("the frame with no damage renders");
    assert_eq!(capture.last_plan(), Some(FramePlan::Skip));
    assert_eq!(
        capture.counters(),
        RasterCounters::default(),
        "a skipped frame keeps no count of the frame before it"
    );
}

/// One chunk owned by `owner`, at `writer`'s next content version, painted by
/// `paint`.
fn chunk_of(writer: &mut SceneWriter<'_>, owner: usize, paint: fn(&mut Canvas)) -> Arc<Chunk> {
    let mut canvas = Canvas::new();
    paint(&mut canvas);
    Arc::new(Chunk::new(
        RenderId::new(owner),
        0,
        writer.next_content_version().expect("a content version"),
        canvas.finish(),
    ))
}

/// Publishes a root of `chunks`, the ones from `clipped` on under a rounded
/// clip of `radius` when there is one, and answers the version.
fn publish_root(
    owner: &mut SceneOwner,
    chunks: &[Arc<Chunk>],
    clipped: Option<(usize, f64)>,
) -> SceneVersion {
    use flui_foundation::geometry::{RRect, Radius};
    use flui_layer::{ClipRRectLayer, Layer};
    use flui_painting::paint::Clip;

    let mut writer = owner.writer();
    let mut content = BoundaryContent::builder();
    for (index, chunk) in chunks.iter().enumerate() {
        if let Some((_, radius)) = clipped.filter(|&(from, _)| from == index) {
            content.push_effect(
                Layer::from(ClipRRectLayer::new(
                    RRect::from_rect_and_radius(
                        Rect::from_xywh(2.0, 2.0, 120.0, 120.0),
                        Radius::circular(radius),
                    ),
                    Clip::AntiAlias,
                )),
                RenderId::new(40),
                0,
            );
        }
        content.chunk(Arc::clone(chunk));
    }
    if clipped.is_some() {
        content.pop_effect();
    }
    writer.push(SceneOp::Publish {
        node: writer.current().root(),
        content: Arc::new(content.finish()),
    });
    owner.apply(SURFACE).expect("the root applies");
    owner.current().clone()
}

fn paint_text(canvas: &mut Canvas, word: &str, at: Offset<f64>) {
    use flui_painting::TextPainter;
    use flui_painting::typography::{TextDirection, TextSpan, TextStyle};

    let mut painter = TextPainter::new()
        .with_text(
            TextSpan::new(word.to_owned()).with_style(
                TextStyle::new()
                    .with_font_size(14.0)
                    .with_color(Color::BLACK),
            ),
        )
        .with_text_direction(TextDirection::Ltr);
    painter
        .layout(
            &mut flui_painting::TextContext::new(&flui_painting::FontCollection::new()),
            0.0,
            100.0,
        )
        .expect("the fixture's text lays out");
    painter.paint(canvas, at);
}

fn paint_gradient(canvas: &mut Canvas, rect: Rect<f64>, [from, to]: [Color; 2]) {
    use flui_painting::paint::{Shader, TileMode};

    canvas.draw_rect(
        rect,
        &Paint::fill(Color::WHITE).with_shader(Shader::linear_gradient(
            Offset::new(rect.left(), rect.top()),
            Offset::new(rect.right(), rect.top()),
            vec![from, to],
            None,
            TileMode::Clamp,
        )),
    );
}

/// A small triangle, tessellated rather than drawn supersampled.
fn paint_triangle(canvas: &mut Canvas, at: Offset<f64>) {
    use flui_foundation::geometry::Point;
    use flui_painting::paint::Path;

    let mut path = Path::new();
    path.move_to(Point::new(at.dx, at.dy));
    path.line_to(Point::new(at.dx + 14.0, at.dy));
    path.line_to(Point::new(at.dx + 7.0, at.dy + 12.0));
    path.close();
    canvas.draw_path(&path, &Paint::fill(Color::GREEN));
}

/// A 4×4 image of two colours, small enough for the shared atlas.
fn paint_image(canvas: &mut Canvas, rect: Rect<f64>) {
    let texels = [RED, BLUE].concat().repeat(8);
    canvas.draw_image(
        flui_painting::paint::Image::from_rgba8(4, 4, texels),
        rect,
        None,
    );
}

/// The chunks of [`mixed_scene`].
const MIXED_CHUNKS: u64 = 8;

/// A root of text, rects, a gradient, a path and an image, then text, a
/// gradient and a path again, one chunk each: the later ones replay onto
/// arenas the earlier ones filled.
fn mixed_scene() -> SceneVersion {
    let mut owner = SceneOwner::new();
    let chunks = {
        let mut writer = owner.writer();
        [
            chunk_of(&mut writer, 2, |canvas| {
                paint_text(canvas, "Cached", Offset::new(4.0, 4.0));
            }),
            chunk_of(&mut writer, 3, |canvas| {
                canvas.draw_rect(
                    Rect::from_xywh(4.0, 30.0, 20.0, 10.0),
                    &Paint::fill(Color::RED),
                );
                canvas.draw_rect(
                    Rect::from_xywh(30.0, 30.0, 20.0, 10.0),
                    &Paint::fill(Color::BLUE),
                );
            }),
            chunk_of(&mut writer, 4, |canvas| {
                paint_gradient(
                    canvas,
                    Rect::from_xywh(4.0, 50.0, 60.0, 12.0),
                    [Color::RED, Color::BLUE],
                );
            }),
            chunk_of(&mut writer, 5, |canvas| {
                paint_triangle(canvas, Offset::new(70.0, 50.0));
            }),
            chunk_of(&mut writer, 6, |canvas| {
                paint_image(canvas, Rect::from_xywh(4.0, 70.0, 16.0, 16.0));
            }),
            chunk_of(&mut writer, 7, |canvas| {
                paint_text(canvas, "again", Offset::new(30.0, 70.0));
            }),
            chunk_of(&mut writer, 8, |canvas| {
                paint_gradient(
                    canvas,
                    Rect::from_xywh(4.0, 96.0, 60.0, 12.0),
                    [Color::GREEN, Color::BLACK],
                );
            }),
            chunk_of(&mut writer, 9, |canvas| {
                paint_triangle(canvas, Offset::new(70.0, 96.0));
            }),
        ]
    };
    publish_root(&mut owner, &chunks, None)
}

/// What one frame did, recorded and drew.
struct Drawn {
    counters: crate::frame_protocol::RasterCounters,
    ir: Vec<u8>,
    pixels: Vec<u8>,
}

/// Draws `version` in full, with the cache unread when `reason` is
/// `DamageOff`.
fn full_frame(
    capture: &mut RetainedCapture,
    version: &SceneVersion,
    reason: flui_layer::FullReason,
) -> Drawn {
    apply(capture, DamageRegion::Full(reason));
    capture
        .render_scene(&Scene::of_version(version.clone()))
        .expect("a full frame renders");
    assert!(
        matches!(
            capture.last_plan(),
            Some(FramePlan::Direct | FramePlan::RetainedFull)
        ),
        "{:?}",
        capture.last_plan()
    );
    drawn(capture)
}

fn drawn(capture: &RetainedCapture) -> Drawn {
    Drawn {
        counters: capture.counters(),
        ir: capture
            .last_ir()
            .expect("the capture keeps its IR")
            .to_vec(),
        pixels: capture.read_rgba().expect("readback"),
    }
}

/// How many of the pixels of `a` and `b` differ.
fn pixels_differing(a: &[u8], b: &[u8]) -> usize {
    let (a, _) = a.as_chunks::<4>();
    let (b, _) = b.as_chunks::<4>();
    a.iter().zip(b).filter(|(a, b)| a != b).count()
}

/// A chunk whose content, transform and surroundings are unchanged replays
/// what it recorded (ADR-0304 §9, ADR-0006 §6): a second full frame of a
/// version of text, rects, a gradient, a path and an image records no chunk,
/// splices every one from the cache, and builds the IR the first frame
/// recorded, byte for byte, which draws the same pixels.
#[test]
fn a_cached_chunk_replays_the_command_stream_it_recorded() {
    use flui_layer::FullReason;
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let version = mixed_scene();
    let mut capture = renderer.retained_capture((SIDE, SIDE)).expect("capture");
    capture.keep_recorded_ir();
    let first = full_frame(&mut capture, &version, FullReason::FirstFrame);
    assert_eq!(
        (
            first.counters.chunks_recorded,
            first.counters.recording_cache_hits
        ),
        (MIXED_CHUNKS, 0),
        "the first frame records every chunk"
    );
    assert!(
        first
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|&pixel| pixel != WHITE),
        "precondition: the version draws"
    );

    let second = full_frame(&mut capture, &version, FullReason::FirstFrame);
    assert_eq!(
        (
            second.counters.chunks_recorded,
            second.counters.recording_cache_hits
        ),
        (0, MIXED_CHUNKS),
        "the second frame replays every chunk"
    );
    assert!(
        second.ir == first.ir,
        "the replayed IR is the recorded one ({} and {} bytes)",
        second.ir.len(),
        first.ir.len()
    );
    assert_eq!(pixels_differing(&first.pixels, &second.pixels), 0);
}

/// An empty root, drawn in full between two frames of another version.
fn empty_frame(capture: &mut RetainedCapture) {
    let mut owner = SceneOwner::new();
    let version = publish_root(&mut owner, &[], None);
    full_frame(capture, &version, flui_layer::FullReason::FirstFrame);
}

/// A kept recording names glyph and image texels, so each event that moves
/// them misses every one (ADR-0304 §9, `ResourceEpoch`): after a critical
/// reclaim, a switch of font source or an image-atlas reset, the next full
/// frame records every chunk anew and draws what a fresh renderer draws. The
/// first row, a frame between that moves nothing, replays every chunk.
#[test]
fn an_atlas_reset_misses_every_cached_recording() {
    use flui_foundation::memory::MemoryPressureLevel;
    use flui_layer::FullReason;
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    // (row, what happens between the frames, whether the second frame and
    // the fresh one draw through the plugin font source, whether it hits)
    type Between = fn(&mut RetainedCapture);
    let rows: [(&str, Between, bool, bool); 4] = [
        ("a frame between", empty_frame, false, true),
        (
            "a critical reclaim",
            |capture| capture.reclaim(MemoryPressureLevel::Critical),
            false,
            false,
        ),
        ("a font-source switch", |_| {}, true, false),
        (
            "an image-atlas reset",
            |capture| {
                capture.mark_image_atlas_full();
                empty_frame(capture);
            },
            false,
            false,
        ),
    ];
    let version = mixed_scene();
    let second_frame = |capture: &mut RetainedCapture, plugin: bool| {
        apply(capture, DamageRegion::Full(FullReason::FirstFrame));
        let scene = Scene::of_version(version.clone());
        if plugin {
            capture.render_plugin_frame(&scene, false)
        } else {
            capture.render_scene(&scene)
        }
        .expect("the second frame renders");
        drawn(capture)
    };
    for (row, between, plugin, hits) in rows {
        let mut capture = renderer.retained_capture((SIDE, SIDE)).expect("capture");
        capture.keep_recorded_ir();
        let first = full_frame(&mut capture, &version, FullReason::FirstFrame);
        assert_eq!(
            first.counters.chunks_recorded, MIXED_CHUNKS,
            "{row}: the first frame records every chunk"
        );
        between(&mut capture);
        let second = second_frame(&mut capture, plugin);
        let expected = if hits {
            (0, MIXED_CHUNKS)
        } else {
            (MIXED_CHUNKS, 0)
        };
        assert_eq!(
            (
                second.counters.chunks_recorded,
                second.counters.recording_cache_hits
            ),
            expected,
            "{row}: chunks recorded and replayed"
        );
        let mut fresh = renderer.retained_capture((SIDE, SIDE)).expect("capture");
        fresh.keep_recorded_ir();
        let oracle = second_frame(&mut fresh, plugin);
        assert_eq!(
            pixels_differing(&second.pixels, &oracle.pixels),
            0,
            "{row}: the frame draws what a fresh renderer draws"
        );
    }
}

/// A frame that drew nothing on screen is retried as a full frame of the
/// version it held. After a frame that failed once begun the retry records
/// no chunk: each comes from the cache, and the pixels are a fresh
/// renderer's. After a frame that panicked once recorded it records every
/// chunk again, since a raster panic drops the recordings (ADR-0304 §10),
/// and its counters report the recordings the panic dropped with those a
/// reclaim dropped before the panicked frame. The engine has no path that
/// re-renders a held version of its own, so each retry is the forced full
/// frame the frame protocol owes after the failure.
#[test]
fn a_frame_not_shown_is_retried_without_recording() {
    use flui_foundation::memory::MemoryPressureLevel;
    use flui_layer::FullReason;
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let version = mixed_scene();
    let scene = Scene::of_version(version.clone());
    let mut fresh = renderer.retained_capture((SIDE, SIDE)).expect("capture");
    fresh.keep_recorded_ir();
    let oracle = full_frame(&mut fresh, &version, FullReason::FirstFrame);

    // A frame shown, then one that fails after it began.
    let mut capture = renderer.retained_capture((SIDE, SIDE)).expect("capture");
    capture.keep_recorded_ir();
    full_frame(&mut capture, &version, FullReason::FirstFrame);
    apply(&mut capture, DamageRegion::Full(FullReason::FirstFrame));
    capture.fail_next_frame_after_begin();
    assert!(
        capture.render_scene(&scene).is_err(),
        "the injected failure fails the frame"
    );
    capture.render_scene(&scene).expect("the retry renders");
    let retry = drawn(&capture);
    assert_eq!(
        (
            retry.counters.chunks_recorded,
            retry.counters.recording_cache_hits
        ),
        (0, MIXED_CHUNKS),
        "after a failed frame: the retry replays every chunk"
    );
    assert_eq!(
        pixels_differing(&retry.pixels, &oracle.pixels),
        0,
        "after a failed frame: the retry draws what a fresh renderer draws"
    );

    // A frame shown, a reclaim that drops its recordings, then a frame that
    // records them again and panics before it is submitted.
    let mut capture = renderer.retained_capture((SIDE, SIDE)).expect("capture");
    capture.keep_recorded_ir();
    full_frame(&mut capture, &version, FullReason::FirstFrame);
    capture.reclaim(MemoryPressureLevel::Critical);
    apply(&mut capture, DamageRegion::Full(FullReason::FirstFrame));
    capture.panic_after_next_record();
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: the test holds the raster frame's own panic"
    )]
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        capture.render_scene(&scene)
    }));
    assert!(unwound.is_err(), "the injected panic unwinds the frame");
    capture.render_scene(&scene).expect("the retry renders");
    let retry = drawn(&capture);
    assert_eq!(
        (
            retry.counters.chunks_recorded,
            retry.counters.recording_cache_hits,
            retry.counters.recording_cache_evictions
        ),
        (MIXED_CHUNKS, 0, 2 * MIXED_CHUNKS),
        "after a panicked frame: the retry records every chunk and reports \
         what the reclaim and the panic dropped"
    );
    assert_eq!(
        pixels_differing(&retry.pixels, &oracle.pixels),
        0,
        "after a panicked frame: the retry draws what a fresh renderer draws"
    );
}

/// A kept recording spliced after another prefix builds the IR that
/// recording it there builds: its first element merges into what the prefix
/// left or opens its own, as recorded in place, every offset rebased, and its
/// draws take the clip they are spliced under. Each row draws a root of two
/// chunks twice, the first chunk changed between the frames and the second
/// kept: one capture reads the cache, the other draws with damage off and so
/// records both chunks each frame.
#[test]
fn a_recording_spliced_after_another_prefix_equals_one_recorded_there() {
    use flui_layer::FullReason;
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    type Painting = fn(&mut Canvas);
    let rect: Painting = |canvas| {
        canvas.draw_rect(
            Rect::from_xywh(10.0, 10.0, 20.0, 10.0),
            &Paint::fill(Color::RED),
        );
    };
    let other_rect: Painting = |canvas| {
        canvas.draw_rect(
            Rect::from_xywh(40.0, 10.0, 20.0, 10.0),
            &Paint::fill(Color::BLUE),
        );
    };
    let path: Painting = |canvas| paint_triangle(canvas, Offset::new(10.0, 40.0));
    let gradient: Painting = |canvas| {
        paint_gradient(
            canvas,
            Rect::from_xywh(10.0, 60.0, 40.0, 8.0),
            [Color::RED, Color::BLUE],
        );
    };
    let text: Painting = |canvas| paint_text(canvas, "ab", Offset::new(10.0, 80.0));
    // (row, first chunk in frame one, in frame two, the kept chunk, its
    // rounded clip's radius in each frame)
    type Radii = Option<(f64, f64)>;
    let rows: [(&str, Painting, Painting, Painting, Radii); 9] = [
        (
            "a rect after a path, then after a rect",
            path,
            rect,
            other_rect,
            None,
        ),
        (
            "a rect after a rect, then after a path",
            rect,
            path,
            other_rect,
            None,
        ),
        (
            "a path, a rect and a path after a rect, then after a path",
            rect,
            path,
            |canvas| {
                paint_triangle(canvas, Offset::new(40.0, 40.0));
                canvas.draw_rect(
                    Rect::from_xywh(70.0, 40.0, 10.0, 10.0),
                    &Paint::fill(Color::RED),
                );
                paint_triangle(canvas, Offset::new(90.0, 40.0));
            },
            None,
        ),
        (
            "a path after a path, then after a rect",
            path,
            rect,
            |canvas| paint_triangle(canvas, Offset::new(40.0, 40.0)),
            None,
        ),
        (
            "a path, a rect and a path after a path, then after a rect",
            path,
            rect,
            |canvas| {
                paint_triangle(canvas, Offset::new(40.0, 40.0));
                canvas.draw_rect(
                    Rect::from_xywh(70.0, 40.0, 10.0, 10.0),
                    &Paint::fill(Color::RED),
                );
                paint_triangle(canvas, Offset::new(90.0, 40.0));
            },
            None,
        ),
        (
            "a gradient after a rect, then after a gradient",
            rect,
            gradient,
            |canvas| {
                paint_gradient(
                    canvas,
                    Rect::from_xywh(60.0, 60.0, 40.0, 8.0),
                    [Color::GREEN, Color::BLACK],
                );
                canvas.draw_rect(
                    Rect::from_xywh(70.0, 70.0, 10.0, 10.0),
                    &Paint::fill(Color::GREEN),
                );
            },
            None,
        ),
        (
            "text after a rect, then after text",
            rect,
            text,
            |canvas| {
                paint_text(canvas, "cd", Offset::new(60.0, 80.0));
                canvas.draw_rect(
                    Rect::from_xywh(70.0, 100.0, 10.0, 10.0),
                    &Paint::fill(Color::GREEN),
                );
            },
            None,
        ),
        (
            "a rect after a gradient, then after text",
            gradient,
            text,
            other_rect,
            None,
        ),
        (
            "a rect under a rounded clip whose radius changes",
            rect,
            other_rect,
            |canvas| {
                canvas.draw_rect(
                    Rect::from_xywh(0.0, 0.0, 30.0, 30.0),
                    &Paint::fill(Color::GREEN),
                );
                paint_triangle(canvas, Offset::new(2.0, 2.0));
            },
            Some((4.0, 12.0)),
        ),
    ];
    for (row, before, after, kept, radius) in rows {
        let mut owner = SceneOwner::new();
        let (first, shared) = {
            let mut writer = owner.writer();
            (
                chunk_of(&mut writer, 2, before),
                chunk_of(&mut writer, 3, kept),
            )
        };
        let clip = |radius: Option<f64>| radius.map(|radius| (1, radius));
        let one = publish_root(
            &mut owner,
            &[first, Arc::clone(&shared)],
            clip(radius.map(|(one, _)| one)),
        );
        let second = chunk_of(&mut owner.writer(), 2, after);
        let two = publish_root(
            &mut owner,
            &[second, shared],
            clip(radius.map(|(_, two)| two)),
        );

        let mut cached = renderer.retained_capture((SIDE, SIDE)).expect("capture");
        cached.keep_recorded_ir();
        let mut recorded = renderer.retained_capture((SIDE, SIDE)).expect("capture");
        recorded.keep_recorded_ir();
        let cached_one = full_frame(&mut cached, &one, FullReason::FirstFrame);
        let recorded_one = full_frame(&mut recorded, &one, FullReason::DamageOff);
        assert!(
            cached_one.ir == recorded_one.ir,
            "{row}: precondition: both captures record the first frame alike"
        );
        let cached_two = full_frame(&mut cached, &two, FullReason::FirstFrame);
        let recorded_two = full_frame(&mut recorded, &two, FullReason::DamageOff);
        assert_eq!(
            (
                cached_two.counters.chunks_recorded,
                cached_two.counters.recording_cache_hits
            ),
            (1, 1),
            "{row}: the kept chunk is spliced"
        );
        assert_eq!(
            (
                recorded_two.counters.chunks_recorded,
                recorded_two.counters.recording_cache_hits
            ),
            (2, 0),
            "{row}: with damage off the cache is not read"
        );
        assert!(
            cached_two.ir == recorded_two.ir,
            "{row}: the spliced IR is the one recorded in place ({} and {} bytes)",
            cached_two.ir.len(),
            recorded_two.ir.len()
        );
        assert_eq!(
            pixels_differing(&cached_two.pixels, &recorded_two.pixels),
            0,
            "{row}: the spliced frame draws as the recorded one"
        );
    }
}

/// A chunk moved by whole device pixels replays its recording with every
/// device position shifted, which is the IR recording it at the new position
/// builds (ADR-0304 §9). Each row draws one boundary of one chunk at one
/// offset and then at another, under a hard-edged clip or a device-pixel
/// ratio when it has one: one capture reads the cache, the other draws with
/// damage off and so records the chunk each frame. A scissor that cuts the
/// chunk is in its key relative to the chunk's pixel origin, so a chunk cut by
/// a clip that moves with it replays and one moved under a fixed one records
/// again, as does a chunk moved by a fraction of a pixel. Text placed across
/// 0, at a tie between subpixel bins and far from 0 replays as it records.
#[test]
fn a_chunk_moved_by_whole_pixels_hits_the_recording_cache() {
    use flui_foundation::geometry::{Matrix4, Point, RRect, Radius};
    use flui_layer::{ClipRectLayer, FullReason, Layer, TransformLayer};
    use flui_painting::paint::{Clip, Path, Shader, TileMode};
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    type Painting = fn(&mut Canvas);
    // A hard-edged clip of the chunk: in the root, so the chunk moves under
    // it, or in the boundary, so it moves with the chunk.
    #[derive(Clone, Copy)]
    enum Cut {
        None,
        Fixed(Rect<f64>),
        Moving(Rect<f64>),
    }
    type Move = (f64, f64);
    struct MoveRow {
        row: &'static str,
        painting: Painting,
        cut: Cut,
        /// The boundary's offset in frame one and in frame two.
        before: Move,
        after: Move,
        /// Whether frame two replays the chunk.
        hits: bool,
        /// The device-pixel ratio the root scales by.
        ratio: f64,
        target: (u32, u32),
    }
    let row = |row, painting, before, after| MoveRow {
        row,
        painting,
        cut: Cut::None,
        before,
        after,
        hits: true,
        ratio: 1.0,
        target: (SIDE, SIDE),
    };
    let text: Painting = |canvas| paint_text(canvas, "Moved", Offset::new(2.0, 2.0));
    let shapes: Painting = |canvas| {
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, 20.0, 10.0),
            &Paint::fill(Color::RED),
        );
        canvas.draw_rrect(
            RRect::from_rect_and_radius(
                Rect::from_xywh(0.0, 14.0, 20.0, 10.0),
                Radius::circular(4.0),
            ),
            &Paint::fill(Color::BLUE),
        );
        canvas.draw_circle(Point::new(30.0, 10.0), 6.0, &Paint::fill(Color::GREEN));
    };
    let rows = [
        row("text", text, (10.0, 20.0), (13.0, 17.0)),
        row(
            "a rect, a rounded rect and a circle",
            shapes,
            (10.0, 20.0),
            (14.0, 25.0),
        ),
        row(
            "a rotated rect and circle",
            |canvas| {
                canvas.save();
                canvas.translate(8.0, 4.0);
                canvas.rotate(0.5);
                canvas.draw_rect(
                    Rect::from_xywh(0.0, 0.0, 20.0, 10.0),
                    &Paint::fill(Color::RED),
                );
                canvas.draw_circle(Point::new(0.0, 0.0), 4.0, &Paint::fill(Color::BLUE));
                canvas.restore();
            },
            (20.0, 20.0),
            (17.0, 26.0),
        ),
        row(
            "rotated text",
            |canvas| {
                canvas.save();
                canvas.translate(8.0, 4.0);
                canvas.rotate(0.5);
                paint_text(canvas, "Moved", Offset::ZERO);
                canvas.restore();
            },
            (20.0, 20.0),
            (17.0, 26.0),
        ),
        row(
            "a gradient",
            |canvas| {
                paint_gradient(
                    canvas,
                    Rect::from_xywh(0.0, 0.0, 40.0, 8.0),
                    [Color::RED, Color::BLUE],
                );
            },
            (10.0, 20.0),
            (12.0, 29.0),
        ),
        row(
            "a gradient on a rounded rect and on a path",
            |canvas| {
                let rect = Rect::from_xywh(0.0, 0.0, 40.0, 12.0);
                let shader = Shader::linear_gradient(
                    Offset::new(0.0, 0.0),
                    Offset::new(40.0, 0.0),
                    vec![Color::RED, Color::BLUE],
                    None,
                    TileMode::Clamp,
                );
                canvas.draw_rrect(
                    RRect::from_rect_and_radius(rect, Radius::circular(5.0)),
                    &Paint::fill(Color::WHITE).with_shader(shader.clone()),
                );
                let mut path = Path::new();
                path.move_to(Point::new(0.0, 16.0));
                path.line_to(Point::new(30.0, 16.0));
                path.line_to(Point::new(15.0, 34.0));
                path.close();
                canvas.draw_path(&path, &Paint::fill(Color::WHITE).with_shader(shader));
            },
            (10.0, 20.0),
            (13.0, 26.0),
        ),
        row(
            "a radial and a sweep gradient",
            |canvas| {
                canvas.draw_rect(
                    Rect::from_xywh(0.0, 0.0, 24.0, 24.0),
                    &Paint::fill(Color::WHITE).with_shader(Shader::radial_gradient(
                        Offset::new(12.0, 12.0),
                        12.0,
                        vec![Color::RED, Color::BLUE],
                        None,
                        TileMode::Clamp,
                        None,
                        None,
                    )),
                );
                canvas.draw_rect(
                    Rect::from_xywh(30.0, 0.0, 24.0, 24.0),
                    &Paint::fill(Color::WHITE).with_shader(Shader::sweep_gradient(
                        Offset::new(42.0, 12.0),
                        vec![Color::RED, Color::GREEN, Color::BLUE],
                        None,
                        TileMode::Clamp,
                        0.0,
                        std::f64::consts::TAU,
                    )),
                );
            },
            (10.0, 20.0),
            (15.0, 17.0),
        ),
        row(
            "two shadows and an arc",
            |canvas| {
                let mut path = Path::new();
                path.add_rect(Rect::from_xywh(4.0, 4.0, 20.0, 12.0));
                canvas.draw_shadow(&path, Color::BLACK, 4.0);
                // A rounded rect takes the analytic shadow instance.
                let rounded = Path::from_rrect(RRect::from_rect_and_radius(
                    Rect::from_xywh(4.0, 24.0, 20.0, 12.0),
                    Radius::circular(3.0),
                ));
                canvas.draw_shadow(&rounded, Color::BLACK, 4.0);
                canvas.draw_arc(
                    Rect::from_xywh(30.0, 0.0, 20.0, 20.0),
                    0.0,
                    2.0,
                    true,
                    &Paint::fill(Color::GREEN),
                );
            },
            (10.0, 20.0),
            (14.0, 27.0),
        ),
        row(
            "a path",
            |canvas| paint_triangle(canvas, Offset::new(2.0, 2.0)),
            (10.0, 20.0),
            (7.0, 21.0),
        ),
        row(
            "an image",
            |canvas| paint_image(canvas, Rect::from_xywh(0.0, 0.0, 8.0, 8.0)),
            (10.0, 20.0),
            (15.0, 18.0),
        ),
        row(
            "text at a fractional offset",
            text,
            (10.25, 20.5),
            (13.25, 17.5),
        ),
        row(
            "shapes at a fractional offset",
            shapes,
            (10.375, 20.75),
            (6.375, 23.75),
        ),
        row(
            "shapes moved up and left",
            shapes,
            (30.0, 30.0),
            (25.0, 24.0),
        ),
        row("text up across 0", text, (10.0, -1.5), (10.0, -3.5)),
        row("text down across 0", text, (10.0, -3.5), (10.0, -1.5)),
        row("text left across 0", text, (-1.125, 20.0), (-2.125, 20.0)),
        row(
            "text left across 0 at a tie",
            text,
            (-1.375, 20.0),
            (-2.375, 20.0),
        ),
        MoveRow {
            target: (4224, 32),
            ..row(
                "text moved far right",
                text,
                (6.374_877_929_687_5, 4.0),
                (4_102.374_877_929_687_5, 4.0),
            )
        },
        MoveRow {
            ratio: 2.0,
            ..row("text at a ratio of 2", text, (10.0, 20.0), (13.0, 17.0))
        },
        MoveRow {
            ratio: 1.25,
            ..row(
                "shapes at a ratio of 1.25",
                shapes,
                (8.0, 16.0),
                (12.0, 12.0),
            )
        },
        MoveRow {
            ratio: 1.25,
            ..row("text at a ratio of 1.25", text, (8.0, 16.0), (12.0, 12.0))
        },
        MoveRow {
            cut: Cut::Fixed(Rect::from_xywh(4.0, 4.0, 120.0, 120.0)),
            ..row(
                "text under a clip that holds it",
                text,
                (24.0, 20.0),
                (27.0, 17.0),
            )
        },
        MoveRow {
            cut: Cut::Fixed(Rect::from_xywh(0.0, 0.0, 40.0, 128.0)),
            hits: false,
            ..row("text a fixed clip cuts", text, (24.0, 20.0), (27.0, 17.0))
        },
        MoveRow {
            cut: Cut::Moving(Rect::from_xywh(0.0, 0.0, 30.0, 40.0)),
            ..row(
                "text a clip moving with it cuts",
                text,
                (24.0, 20.0),
                (27.0, 17.0),
            )
        },
        MoveRow {
            hits: false,
            ..row(
                "text moved by half a pixel",
                text,
                (10.0, 20.0),
                (10.5, 20.0),
            )
        },
    ];
    for MoveRow {
        row,
        painting,
        cut,
        before,
        after,
        hits,
        ratio,
        target,
    } in rows
    {
        let surface = SurfaceKey {
            size: target,
            generation: flui_foundation::SurfaceGeneration::ZERO,
        };
        let place = |writer: &mut SceneWriter<'_>, node, (x, y): Move| {
            writer.push(SceneOp::Place {
                node,
                placement: Placement::try_new(Offset::new(x, y)).expect("a finite offset"),
            });
        };
        let clipped = |content: &mut flui_layer::ContentBuilder, rect| {
            content.push_effect(
                Layer::from(ClipRectLayer::new(rect, Clip::HardEdge)),
                RenderId::new(1),
                0,
            );
        };
        let mut owner = SceneOwner::new();
        let mut writer = owner.writer();
        let root = writer.current().root();
        let node = writer.mint_node();
        let mut content = BoundaryContent::builder();
        if let Cut::Moving(rect) = cut {
            clipped(&mut content, rect);
        }
        content.chunk(chunk_of(&mut writer, 2, painting));
        if let Cut::Moving(_) = cut {
            content.pop_effect();
        }
        writer.push(SceneOp::Publish {
            node,
            content: Arc::new(content.finish()),
        });
        let mut top = BoundaryContent::builder();
        let scaled = ratio != 1.0;
        if scaled {
            top.push_effect(
                Layer::from(TransformLayer::new(Matrix4::scaling(ratio, ratio, 1.0))),
                RenderId::new(3),
                0,
            );
        }
        if let Cut::Fixed(rect) = cut {
            clipped(&mut top, rect);
        }
        top.slot(node);
        if let Cut::Fixed(_) = cut {
            top.pop_effect();
        }
        if scaled {
            top.pop_effect();
        }
        writer.push(SceneOp::Publish {
            node: root,
            content: Arc::new(top.finish()),
        });
        place(&mut writer, node, before);
        owner.apply(surface).expect("the boundary applies");
        let one = owner.current().clone();
        place(&mut owner.writer(), node, after);
        owner.apply(surface).expect("the move applies");
        let two = owner.current().clone();

        let mut cached = renderer.retained_capture(target).expect("capture");
        cached.keep_recorded_ir();
        let mut recorded = renderer.retained_capture(target).expect("capture");
        recorded.keep_recorded_ir();
        let cached_one = full_frame(&mut cached, &one, FullReason::FirstFrame);
        let recorded_one = full_frame(&mut recorded, &one, FullReason::DamageOff);
        assert!(
            cached_one.ir == recorded_one.ir,
            "{row}: precondition: both captures record the first frame alike"
        );
        let cached_two = full_frame(&mut cached, &two, FullReason::FirstFrame);
        let recorded_two = full_frame(&mut recorded, &two, FullReason::DamageOff);
        eprintln!(
            "ROW {row}: hits={} ir={} px={}",
            cached_two.counters.recording_cache_hits,
            cached_two.ir == recorded_two.ir,
            pixels_differing(&cached_two.pixels, &recorded_two.pixels)
        );
        if std::env::var_os("ROWS").is_some() {
            continue;
        }
        assert_eq!(
            (
                cached_two.counters.chunks_recorded,
                cached_two.counters.recording_cache_hits
            ),
            if hits { (0, 1) } else { (1, 0) },
            "{row}: the moved chunk replays only when its key holds"
        );
        assert_eq!(
            recorded_two.counters.recording_cache_hits, 0,
            "{row}: with damage off the cache is not read"
        );
        assert!(
            cached_two.ir == recorded_two.ir,
            "{row}: the replayed IR is the one recorded at the new position ({} and {} bytes)",
            cached_two.ir.len(),
            recorded_two.ir.len()
        );
        assert_eq!(
            pixels_differing(&cached_two.pixels, &recorded_two.pixels),
            0,
            "{row}: the replayed frame draws as the recorded one"
        );
        assert_ne!(
            pixels_differing(&cached_one.pixels, &cached_two.pixels),
            0,
            "{row}: precondition: the move shows"
        );
    }
}

/// A fill of the whole target records the target's size, so a chunk drawn
/// at one size and then at another records again at the second and fills
/// it, as a fresh renderer does.
#[test]
fn a_target_fill_drawn_at_a_new_size_fills_the_new_size() {
    use flui_painting::paint::BlendMode;
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let mut owner = SceneOwner::new();
    let fill = chunk_of(&mut owner.writer(), 2, |canvas| {
        canvas.draw_color(Color::RED, BlendMode::SrcOver);
    });
    let scene = Scene::of_version(publish_root(&mut owner, &[fill], None));
    renderer
        .render_scene(&scene, (32, 32))
        .expect("the small frame renders");
    let large = renderer
        .render_scene(&scene, (64, 64))
        .expect("the large frame renders");
    let fresh = crate::test_support::renderer_or_skip()
        .expect("a second renderer on the same device")
        .render_scene(&scene, (64, 64))
        .expect("the fresh frame renders");
    let corner = ((63 * 64 + 63) * 4) as usize;
    assert_eq!(&large[corner..corner + 4], &RED, "the far corner is filled");
    assert_eq!(pixels_differing(&large, &fresh), 0);
}

/// A chunk drawn while the glyph atlas was too full to place a glyph it
/// asked for is not kept: the next frame records it again, with its glyphs,
/// replays the other chunks, and draws what a fresh renderer draws.
#[test]
fn a_chunk_drawn_without_a_glyph_is_recorded_again() {
    use flui_layer::FullReason;
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    // The two chunks of the mixed scene that draw text.
    const TEXT_CHUNKS: u64 = 2;
    let version = mixed_scene();
    let mut capture = renderer.retained_capture((SIDE, SIDE)).expect("capture");
    capture.keep_recorded_ir();
    capture.hold_glyph_atlas_full(true);
    let first = full_frame(&mut capture, &version, FullReason::FirstFrame);
    assert_eq!(first.counters.chunks_recorded, MIXED_CHUNKS);
    capture.hold_glyph_atlas_full(false);
    let second = full_frame(&mut capture, &version, FullReason::FirstFrame);
    assert_eq!(
        (
            second.counters.chunks_recorded,
            second.counters.recording_cache_hits
        ),
        (TEXT_CHUNKS, MIXED_CHUNKS - TEXT_CHUNKS),
        "the text chunks record again, the others replay"
    );
    let mut fresh = renderer.retained_capture((SIDE, SIDE)).expect("capture");
    fresh.keep_recorded_ir();
    let oracle = full_frame(&mut fresh, &version, FullReason::FirstFrame);
    assert_ne!(
        pixels_differing(&first.pixels, &oracle.pixels),
        0,
        "precondition: the first frame drew without its glyphs"
    );
    assert_eq!(pixels_differing(&second.pixels, &oracle.pixels), 0);
}
