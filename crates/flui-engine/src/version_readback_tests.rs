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
    assert_eq!(partial.region_passes(), 2, "one pass per rect");

    let mut full = renderer.retained_capture((SIDE, SIDE)).expect("capture");
    apply(
        &mut full,
        DamageRegion::Full(flui_layer::FullReason::FirstFrame),
    );
    full.render_scene(&Scene::of_version(corners.version()))
        .expect("the full frame renders");
    assert_eq!(full.region_passes(), 0, "a full frame has no region pass");
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
            assert_eq!(partial.region_passes(), 2, "{case}: two passes");
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

/// The clips that `clipped_reader` puts around its layer's content.
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
/// pixel the clip's edge covers in part, which shows it.
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
        // White bars across, one over row 16, so a column also blurs along
        // its length.
        for y in [7.0, 15.0, 23.0, 31.0] {
            canvas.draw_rect(
                Rect::from_xywh(60.0, y, f64::from(SIDE) - 60.0, 2.0),
                &Paint::fill(Color::WHITE),
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
