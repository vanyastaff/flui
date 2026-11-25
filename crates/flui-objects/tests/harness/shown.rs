//! Rows of the shown answer (`crates/flui-objects/src/shown.rs`): what a layout
//! decided to show of its children is one record, and paint, hit testing,
//! semantics and the input regions read it.
//!
//! The overflow boxes (`RenderStack`, `RenderIndexedStack`, `RenderWrap`,
//! `RenderFittedBox`, `RenderAnimatedSize`, `RenderConstraintsTransformBox`)
//! are each built by an [`OverflowBox`] over the same content: a 200×200
//! unclipped stack holding `target`, which lies partly outside the 100×100
//! box, and `wholly`, which lies wholly outside it. Each child is a labelled
//! semantics container over a system-gesture-exclusion input region over a
//! colored leaf, so one child is seen by paint, hits, semantics and regions.
//! The hosts put the box at the root's origin, alone or as the first sliver
//! of an unscrolled viewport, so the box's coordinates are the root's.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;
use std::time::Duration;

use flui_animation::{MotionSpec, SharedCurve, curves};
use flui_foundation::geometry::{Matrix4, Point, Rect, Size};
use flui_foundation::inspect::{Fields, Inspect};
use flui_foundation::{Leaf, RenderId};
use flui_objects::{
    RenderAlign, RenderAnimatedSize, RenderClipRect, RenderColoredBox, RenderConstrainedBox,
    RenderConstraintsTransformBox, RenderFittedBox, RenderIndexedStack, RenderInputRegion,
    RenderSemanticsAnnotations, RenderSliverToBoxAdapter, RenderStack, RenderViewport, RenderWrap,
    SizeMotion,
};
use flui_painting::paint::Clip;
use flui_painting::{Alignment, BoxFit};
use flui_rendering::constraints::{AxisDirection, BoxConstraints};
use flui_rendering::context::BoxLayoutContext;
use flui_rendering::input_regions::{InputRegion, InputRegionKind};
use flui_rendering::parent_data::{BoxParentData, StackParentData};
use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::testing::{
    DrawKind, FrameRun, Probe, RenderTester, TreeNode, box_node, inspect, sliver_node,
};
use flui_rendering::traits::{RenderBox, TextBaseline};
use flui_rendering::{RenderError, RenderResult};

use super::{code_of, labelled, names_whole_identifier, records_a_clip};

// ============================================================================
// The content every overflow box lays out
// ============================================================================

/// The box every [`OverflowBox`] lays out to, at the root's origin.
const BOX: Rect = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
/// Where `target` lies in the box's coordinates: partly outside, below.
const TARGET: Rect = Rect::from_ltrb(30.0, 80.0, 70.0, 120.0);
/// The part of `target` inside the box.
const TARGET_INSIDE: Rect = Rect::from_ltrb(30.0, 80.0, 70.0, 100.0);
/// Where `wholly` lies in the box's coordinates: wholly outside, below.
const WHOLLY: Rect = Rect::from_ltrb(30.0, 150.0, 70.0, 190.0);
/// The region every content child reports.
const EXCLUSION: InputRegionKind = InputRegionKind::SystemGestureExclusion;

/// A content child: a semantics container labelled `label` over an input
/// region over `leaf`, the leaf labelled `label` for hit testing.
fn content_child(label: &'static str, leaf: TreeNode, at: Rect) -> TreeNode {
    box_node(RenderSemanticsAnnotations::from_configuration(labelled(label)).with_container(true))
        .child(box_node(RenderInputRegion::new(EXCLUSION)).child(leaf.label(label)))
        .with_stack_parent_data(
            StackParentData::new()
                .with_left(at.left())
                .with_top(at.top()),
        )
}

/// A colored leaf of `rect`'s size.
fn leaf_of(rect: Rect) -> TreeNode {
    box_node(RenderColoredBox::red(rect.width(), rect.height()))
}

/// A 200×200 unclipped stack holding `target` over `target_leaf` and
/// `wholly`; a box's child constraints may narrow it (a horizontal wrap gives
/// it the box's width), which keeps both children where they are.
fn content(target_leaf: TreeNode) -> TreeNode {
    box_node(RenderConstrainedBox::new(BoxConstraints::tight(Size::new(
        200.0, 200.0,
    ))))
    .label("content")
    .child(
        box_node(RenderStack::new().with_clip_behavior(Clip::None))
            .child(content_child("target", target_leaf, TARGET))
            .child(content_child("wholly", leaf_of(WHOLLY), WHOLLY)),
    )
}

/// A child that lies inside every box: a 50×50 leaf labelled `leaf` at the
/// box's top left.
fn inside_child(leaf: TreeNode) -> TreeNode {
    leaf.label("leaf")
}

/// One overflow box under test.
#[derive(Clone, Copy)]
struct OverflowBox {
    name: &'static str,
    /// The box over `child`, clipping by `clip`, labelled `box` and wrapped
    /// in the constraints that size it to [`BOX`]: `child` at the box's top
    /// left, laid out at its own size where the box lets it.
    over: fn(Clip, TreeNode) -> TreeNode,
    /// The box's `has_visual_overflow()`.
    overflows: fn(&PipelineOwner, RenderId) -> bool,
    /// The box's `describe_approximate_paint_clip` for its first child.
    clip_of: fn(&PipelineOwner, RenderId, Size) -> Option<Rect>,
    /// Takes the box's early exit from a mounted `over` (no child, a hidden
    /// index).
    empty: fn(&mut FrameRun),
    /// Changes the clip behaviour of the box labelled `box` through its
    /// setter, applying the impact the setter reports.
    set_clip: fn(&mut FrameRun, Clip),
}

/// The [`OverflowBox`] of the render type `$ty`, which `$over` builds.
macro_rules! overflow_box {
    ($name:literal, $ty:ty, $over:expr, $empty:expr) => {
        OverflowBox {
            name: $name,
            over: $over,
            overflows: |owner: &PipelineOwner, id: RenderId| {
                owner
                    .render_object::<$ty>(id)
                    .expect("the box is the type under test")
                    .has_visual_overflow()
            },
            clip_of: |owner: &PipelineOwner, id: RenderId, size: Size| {
                RenderBox::describe_approximate_paint_clip(
                    owner
                        .render_object::<$ty>(id)
                        .expect("the box is the type under test"),
                    0,
                    size,
                )
            },
            empty: $empty,
            set_clip: |run: &mut FrameRun, clip: Clip| {
                let id = run.id("box");
                flui_rendering::testing::update_render_object::<$ty>(
                    run.owner_mut(),
                    id,
                    |object| object.set_clip_behavior(clip),
                );
            },
        }
    };
}

/// The constraints that size every box but the animated one to [`BOX`].
fn tight_box(object: TreeNode) -> TreeNode {
    box_node(RenderConstrainedBox::new(BoxConstraints::tight(BOX.size())))
        .child(object.label("box"))
}

/// Takes away the box's only child.
fn remove_the_child(run: &mut FrameRun) {
    let child = run
        .try_id("content")
        .or_else(|| run.try_id("leaf"))
        .expect("the box has a child to take away");
    assert!(run.owner_mut().remove_render_object(child) > 0);
}

fn stack(clip: Clip, child: TreeNode) -> TreeNode {
    tight_box(
        box_node(RenderStack::new().with_clip_behavior(clip)).child(
            child.with_stack_parent_data(StackParentData::new().with_left(0.0).with_top(0.0)),
        ),
    )
}

fn indexed_stack(clip: Clip, child: TreeNode) -> TreeNode {
    tight_box(
        box_node(
            RenderIndexedStack::new()
                .with_index(Some(0))
                .with_clip_behavior(clip),
        )
        .child(child.with_stack_parent_data(StackParentData::new().with_left(0.0).with_top(0.0))),
    )
}

fn hide_the_index(run: &mut FrameRun) {
    let id = run.id("box");
    flui_rendering::testing::update_render_object::<RenderIndexedStack>(
        run.owner_mut(),
        id,
        |stack| stack.set_index(None),
    );
}

fn wrap(clip: Clip, child: TreeNode) -> TreeNode {
    tight_box(box_node(RenderWrap::new().with_clip_behavior(clip)).child(child))
}

fn fitted_box(clip: Clip, child: TreeNode) -> TreeNode {
    tight_box(
        box_node(RenderFittedBox::new(
            BoxFit::None,
            Alignment::TOP_LEFT,
            clip,
        ))
        .child(child),
    )
}

/// A box whose shown size stays at zero, its owner gone, so it keeps the
/// least size its constraints allow (100×100) while its child is larger.
fn animated_size(clip: Clip, child: TreeNode) -> TreeNode {
    let released = SizeMotion::new(
        MotionSpec::Curve {
            duration: Duration::from_millis(100),
            curve: SharedCurve::new(curves::LINEAR),
        },
        None,
    )
    .handle();
    box_node(RenderConstrainedBox::new(BoxConstraints::new(
        100.0, 200.0, 100.0, 200.0,
    )))
    .child(
        box_node(RenderAnimatedSize::new(released, Alignment::TOP_LEFT, clip))
            .label("box")
            .child(child),
    )
}

fn constraints_transform_box(clip: Clip, child: TreeNode) -> TreeNode {
    tight_box(
        box_node(RenderConstraintsTransformBox::new(
            Alignment::TOP_LEFT,
            None,
            clip,
        ))
        .child(child),
    )
}

const STACK: OverflowBox = overflow_box!("stack", RenderStack, stack, remove_the_child);
const INDEXED_STACK: OverflowBox = overflow_box!(
    "indexed_stack",
    RenderIndexedStack,
    indexed_stack,
    hide_the_index
);
const WRAP: OverflowBox = overflow_box!("wrap", RenderWrap, wrap, remove_the_child);
const FITTED_BOX: OverflowBox =
    overflow_box!("fitted_box", RenderFittedBox, fitted_box, remove_the_child);
const ANIMATED_SIZE: OverflowBox = overflow_box!(
    "animated_size",
    RenderAnimatedSize,
    animated_size,
    remove_the_child
);
const CONSTRAINTS_TRANSFORM_BOX: OverflowBox = overflow_box!(
    "constraints_transform_box",
    RenderConstraintsTransformBox,
    constraints_transform_box,
    remove_the_child
);

/// The six overflow boxes.
const OVERFLOW_BOXES: [OverflowBox; 6] = [
    STACK,
    INDEXED_STACK,
    WRAP,
    FITTED_BOX,
    ANIMATED_SIZE,
    CONSTRAINTS_TRANSFORM_BOX,
];

// ============================================================================
// Hosts and readers
// ============================================================================

/// Where a box is mounted: alone, or under a scrollable.
#[derive(Clone, Copy, Debug)]
enum Host {
    Alone,
    Scrollable,
}

const HOSTS: [Host; 2] = [Host::Alone, Host::Scrollable];

/// Mounts `subject` at the root's origin in a 300×300 window and runs a
/// frame with semantics on.
fn mount(host: Host, subject: TreeNode) -> FrameRun {
    let aligned = box_node(RenderAlign::new(Alignment::TOP_LEFT)).child(subject);
    let root = match host {
        Host::Alone => aligned,
        Host::Scrollable => box_node(RenderViewport::new(AxisDirection::TopToBottom))
            .label("viewport")
            .child(sliver_node(RenderSliverToBoxAdapter::new()).child(aligned)),
    };
    RenderTester::mount(root)
        .with_size(Size::new(300.0, 300.0))
        .with_semantics_enabled()
        .run_frame()
}

fn map_rect(transform: &Matrix4, rect: Rect) -> Rect {
    let (left, top) = transform.transform_point(rect.left(), rect.top());
    let (right, bottom) = transform.transform_point(rect.right(), rect.bottom());
    Rect::from_ltrb(
        left.min(right),
        top.min(bottom),
        left.max(right),
        top.max(bottom),
    )
}

/// One published semantics node: its label, its rectangle in the root's
/// coordinates, and whether it is hidden.
type Published = (String, Rect, bool);

/// The labelled semantics nodes of the last frame, in tree order.
fn published(run: &FrameRun) -> Vec<Published> {
    fn walk(
        owner: &flui_semantics::SemanticsOwner,
        id: flui_foundation::SemanticsId,
        to_root: Matrix4,
        out: &mut Vec<Published>,
    ) {
        let node = owner.get(id).expect("rooted semantics node");
        let to_root = node
            .transform()
            .map_or(to_root, |transform| to_root * *transform);
        if let Some(label) = node.label() {
            out.push((
                label.to_owned(),
                map_rect(&to_root, node.rect()),
                node.config().is_hidden(),
            ));
        }
        for &child in node.children() {
            walk(owner, child, to_root, out);
        }
    }
    let owner = run.owner().semantics_owner().expect("semantics enabled");
    let mut out = Vec::new();
    if let Some(root) = owner.root() {
        walk(owner, root, Matrix4::IDENTITY, &mut out);
    }
    out
}

/// The published node labelled `label`, if any.
fn published_as(run: &FrameRun, label: &str) -> Option<(Rect, bool)> {
    published(run)
        .into_iter()
        .find(|(published, _, _)| published == label)
        .map(|(_, rect, hidden)| (rect, hidden))
}

/// The part of `rect` the last frame painted: `rect` cut by every clip layer.
/// The hosts keep every offset at zero, so a clip layer's rectangle is in the
/// root's coordinates.
fn painted_part(run: &FrameRun, rect: Rect) -> Option<Rect> {
    assert!(
        !run.display_commands()
            .iter()
            .any(|command| matches!(command.kind, DrawKind::Clip)),
        "the boxes clip by a layer, whose rectangle this reads"
    );
    let clips = run.layer_tree().map_or_default(inspect::clip_rects);
    clips
        .into_iter()
        .try_fold(rect, |painted, clip| painted.intersect(&clip))
}

/// Sample points over `rect` grown by 10 on each side, none on an edge of
/// the rectangles these rows use.
fn samples_around(rect: Rect) -> Vec<Point> {
    let mut points = Vec::new();
    let mut y = rect.top() - 7.5;
    while y < rect.bottom() + 10.0 {
        let mut x = rect.left() - 7.5;
        while x < rect.right() + 10.0 {
            points.push(Point::new(x, y));
            x += 5.0;
        }
        y += 5.0;
    }
    points
}

fn hits(run: &FrameRun, label: &str, point: Point) -> bool {
    run.try_id(label)
        .is_some_and(|id| run.hit(point.x, point.y).contains(&id))
}

// ============================================================================
// The shared row
// ============================================================================

/// The shared row: a child is hit, published and reports its input region
/// exactly where it is painted, for `shows`, a layout that clips `target` at
/// the box's edge, and for the early exit `T`'s [`OverflowBox::empty`] takes
/// from it, alone and under a scrollable. The early exit is reached from
/// `shows`, so its check is also the check that nothing of the first layout
/// is left: no clip reported or pushed, nothing of `target` hit, published or
/// reported. `T` is the type the box is, which the keeper guard reads from
/// the call.
fn hits_and_publishes_a_child_only_where_it_paints<T: RenderBox + 'static>(
    object: OverflowBox,
    shows: fn() -> TreeNode,
) {
    let name = object.name;
    for host in HOSTS {
        let mut run = mount(host, shows());
        let id = run.id("box");
        assert!(
            run.owner().render_object::<T>(id).is_some(),
            "{name}: the box is a {}",
            std::any::type_name::<T>()
        );
        let size = run.box_geometry(id);
        assert_eq!(size, BOX.size(), "{name} {host:?}: the box's size");

        let painted = painted_part(&run, TARGET).expect("target is partly painted");
        assert_eq!(
            painted, TARGET_INSIDE,
            "{name} {host:?}: paint cuts target at the box's edge"
        );
        assert_eq!(
            (object.clip_of)(run.owner(), id, size),
            Some(BOX),
            "{name} {host:?}: the box reports the clip it paints"
        );
        for point in samples_around(TARGET) {
            assert_eq!(
                hits(&run, "target", point),
                painted.contains(point),
                "{name} {host:?}: target is hit at {point:?} only where it is painted"
            );
        }
        let (rect, hidden) = published_as(&run, "target").expect("target is published");
        assert_eq!(
            (rect, hidden),
            (painted, false),
            "{name} {host:?}: target is published where it is painted"
        );
        assert_eq!(
            run.owner().input_regions(),
            vec![InputRegion::new(EXCLUSION, painted)],
            "{name} {host:?}: only the painted part of target reports its region"
        );

        (object.empty)(&mut run);
        run.pump();
        let size = run.box_geometry(id);
        assert_eq!(
            (object.clip_of)(run.owner(), id, size),
            None,
            "{name} {host:?}: the early exit reports no clip"
        );
        assert!(
            !records_a_clip(&run),
            "{name} {host:?}: the early exit pushes no clip"
        );
        for point in samples_around(TARGET) {
            assert!(
                !hits(&run, "target", point),
                "{name} {host:?}: nothing hits target at {point:?} after the early exit"
            );
        }
        assert_eq!(
            published(&run),
            Vec::new(),
            "{name} {host:?}: nothing is published after the early exit"
        );
        assert_eq!(
            run.owner().input_regions(),
            Vec::new(),
            "{name} {host:?}: no region is reported after the early exit"
        );
    }
}

pub(super) fn harness_stack_hits_and_publishes_a_child_only_where_it_paints() {
    hits_and_publishes_a_child_only_where_it_paints::<RenderStack>(STACK, || {
        stack(Clip::HardEdge, content(leaf_of(TARGET)))
    });
}

pub(super) fn harness_indexed_stack_hits_and_publishes_a_child_only_where_it_paints() {
    hits_and_publishes_a_child_only_where_it_paints::<RenderIndexedStack>(INDEXED_STACK, || {
        indexed_stack(Clip::HardEdge, content(leaf_of(TARGET)))
    });
}

pub(super) fn harness_wrap_hits_and_publishes_a_child_only_where_it_paints() {
    hits_and_publishes_a_child_only_where_it_paints::<RenderWrap>(WRAP, || {
        wrap(Clip::HardEdge, content(leaf_of(TARGET)))
    });
}

pub(super) fn harness_fitted_box_hits_and_publishes_a_child_only_where_it_paints() {
    hits_and_publishes_a_child_only_where_it_paints::<RenderFittedBox>(FITTED_BOX, || {
        fitted_box(Clip::HardEdge, content(leaf_of(TARGET)))
    });
}

pub(super) fn harness_animated_size_hits_and_publishes_a_child_only_where_it_paints() {
    hits_and_publishes_a_child_only_where_it_paints::<RenderAnimatedSize>(ANIMATED_SIZE, || {
        animated_size(Clip::HardEdge, content(leaf_of(TARGET)))
    });
}

pub(super) fn harness_constraints_transform_box_hits_and_publishes_a_child_only_where_it_paints() {
    hits_and_publishes_a_child_only_where_it_paints::<RenderConstraintsTransformBox>(
        CONSTRAINTS_TRANSFORM_BOX,
        || constraints_transform_box(Clip::HardEdge, content(leaf_of(TARGET))),
    );
}

// ============================================================================
// The overflow record
// ============================================================================

/// A child partly outside the box publishes a semantics rectangle and an
/// input region cut to the box, and one wholly outside publishes neither,
/// since nothing above the box can reveal it; without a clip both publish
/// their whole rectangles.
fn cuts_semantics_and_regions_at_the_overflow_clip(object: OverflowBox) {
    let name = object.name;
    let cut = |run: &FrameRun, when: &str| {
        assert_eq!(
            published(run),
            vec![("target".to_owned(), TARGET_INSIDE, false)],
            "{name} {when}: target is cut to the box and wholly is dropped"
        );
        assert_eq!(
            run.owner().input_regions(),
            vec![InputRegion::new(EXCLUSION, TARGET_INSIDE)],
            "{name} {when}: target's region is cut to the box and wholly reports none"
        );
    };
    let mut run = mount(
        Host::Alone,
        (object.over)(Clip::HardEdge, content(leaf_of(TARGET))),
    );
    cut(&run, "mounted clipping");

    // A clip change alone reaches semantics: the setter's impact carries it.
    (object.set_clip)(&mut run, Clip::None);
    run.pump();
    assert_eq!(
        published(&run),
        vec![
            ("target".to_owned(), TARGET, false),
            ("wholly".to_owned(), WHOLLY, false),
        ],
        "{name}: turning the clip off publishes the children whole"
    );
    (object.set_clip)(&mut run, Clip::HardEdge);
    run.pump();
    cut(&run, "after the clip is turned back on");

    let run = mount(
        Host::Alone,
        (object.over)(Clip::None, content(leaf_of(TARGET))),
    );
    assert_eq!(
        published(&run),
        vec![
            ("target".to_owned(), TARGET, false),
            ("wholly".to_owned(), WHOLLY, false),
        ],
        "{name}: an unclipped box publishes its children whole"
    );
    assert_eq!(
        run.owner().input_regions(),
        vec![
            InputRegion::new(EXCLUSION, TARGET),
            InputRegion::new(EXCLUSION, WHOLLY),
        ],
        "{name}: an unclipped box reports its children's whole regions"
    );
}

pub(super) fn harness_stack_cuts_semantics_and_regions_at_its_overflow_clip() {
    cuts_semantics_and_regions_at_the_overflow_clip(STACK);
}

pub(super) fn harness_indexed_stack_cuts_semantics_and_regions_at_its_overflow_clip() {
    cuts_semantics_and_regions_at_the_overflow_clip(INDEXED_STACK);
}

pub(super) fn harness_wrap_cuts_semantics_and_regions_at_its_overflow_clip() {
    cuts_semantics_and_regions_at_the_overflow_clip(WRAP);
}

pub(super) fn harness_fitted_box_cuts_semantics_and_regions_at_its_overflow_clip() {
    cuts_semantics_and_regions_at_the_overflow_clip(FITTED_BOX);
}

pub(super) fn harness_animated_size_cuts_semantics_and_regions_at_its_overflow_clip() {
    cuts_semantics_and_regions_at_the_overflow_clip(ANIMATED_SIZE);
}

pub(super) fn harness_constraints_transform_box_cuts_semantics_and_regions_at_its_overflow_clip() {
    cuts_semantics_and_regions_at_the_overflow_clip(CONSTRAINTS_TRANSFORM_BOX);
}

/// An overflow clip is an authored clip like any other: each box clipping by
/// its overflow publishes, alone and under a scrollable, the semantics and
/// regions the same box publishes unclipped under a `RenderClipRect` of its
/// size, with one child partly outside and one wholly outside.
pub(super) fn harness_overflow_clip_publishes_what_a_clip_rect_publishes() {
    for object in OVERFLOW_BOXES {
        let name = object.name;
        for host in HOSTS {
            let clipped = mount(
                host,
                (object.over)(Clip::HardEdge, content(leaf_of(TARGET))),
            );
            let reference = mount(
                host,
                box_node(RenderClipRect::new(Clip::HardEdge))
                    .child((object.over)(Clip::None, content(leaf_of(TARGET)))),
            );
            assert_eq!(
                published_as(&reference, "target"),
                Some((TARGET_INSIDE, false)),
                "{name} {host:?}: the clip rect cuts target"
            );
            assert_eq!(
                published(&clipped),
                published(&reference),
                "{name} {host:?}: the overflow clip publishes what a clip rect publishes"
            );
            assert_eq!(
                clipped.owner().input_regions(),
                reference.owner().input_regions(),
                "{name} {host:?}: the overflow clip cuts the regions a clip rect cuts"
            );
        }
    }
}

/// How a [`FailsWhenArmed`] fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    /// Its layout returns an error its parent hands up.
    Refuses,
    /// Its baseline, which the box reads during its own layout, panics.
    Panics,
}

/// A 50×50 leaf that fails on demand: its layout returns an error that
/// propagates up through its parent, or its baseline query panics inside the
/// parent's layout.
#[derive(Debug)]
struct FailsWhenArmed {
    failure: Rc<Cell<Option<Failure>>>,
}

impl Inspect for FailsWhenArmed {
    fn kind(&self) -> &'static str {
        "FailsWhenArmed"
    }

    fn inspect(&self, _out: &mut Fields<'_>) {}
}

impl RenderBox for FailsWhenArmed {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
    ) -> RenderResult<Size> {
        if self.failure.get() == Some(Failure::Refuses) {
            return Err(RenderError::TextContextBusy);
        }
        Ok(ctx.constraints().constrain(Size::new(50.0, 50.0)))
    }

    fn compute_distance_to_actual_baseline(
        &self,
        _baseline: TextBaseline,
    ) -> RenderResult<Option<f64>> {
        // Disarmed before it panics, so the test can tell that the query ran:
        // a pipeline that contains the box's panic still finishes the frame.
        if self.failure.get() == Some(Failure::Panics) {
            self.failure.set(None);
            panic!("deliberate test failure in a baseline query");
        }
        Ok(Some(40.0))
    }
}

/// After a layout that fails, by a returned error or a panic, a box clips to
/// the size it keeps, though its last good layout had nothing outside it: the
/// failed pass moved children, so no earlier answer is true of them.
/// `RenderFittedBox` reads no baseline in its layout, so only the returned
/// error reaches it.
pub(super) fn harness_an_overflow_box_clips_after_a_layout_that_fails() {
    for object in OVERFLOW_BOXES {
        let name = object.name;
        let failures: &[Failure] = if object.name == FITTED_BOX.name {
            &[Failure::Refuses]
        } else {
            &[Failure::Refuses, Failure::Panics]
        };
        for &failure in failures {
            let armed = Rc::new(Cell::new(None));
            let leaf = box_node(FailsWhenArmed {
                failure: Rc::clone(&armed),
            });
            let mut run = mount(
                Host::Alone,
                (object.over)(Clip::HardEdge, inside_child(leaf)),
            );
            let id = run.id("box");
            let kept = run.box_geometry(id);
            assert!(
                !(object.overflows)(run.owner(), id),
                "{name}: precondition: the healthy layout has nothing outside the box"
            );
            assert_eq!((object.clip_of)(run.owner(), id, kept), None, "{name}");

            armed.set(Some(failure));
            let leaf = run.id("leaf");
            run.owner_mut().mark_needs_layout(leaf);
            run.owner_mut().mark_needs_layout(id);
            let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run.try_pump()));
            match failure {
                Failure::Refuses => assert!(
                    matches!(failed, Ok(Err(_))),
                    "{name}: the refusal reaches the frame: {failed:?}"
                ),
                Failure::Panics => assert_eq!(
                    armed.get(),
                    None,
                    "{name}: the box's layout reached the panicking query"
                ),
            }
            assert!(
                (object.overflows)(run.owner(), id),
                "{name} {failure:?}: a failed layout leaves the box clipping"
            );
            assert_eq!(
                (object.clip_of)(run.owner(), id, run.box_geometry(id)),
                Some(Rect::from_origin_size(Point::ZERO, run.box_geometry(id))),
                "{name} {failure:?}: the box clips to the size it keeps"
            );
            assert_eq!(run.box_geometry(id), kept, "{name} {failure:?}");

            armed.set(None);
            run.owner_mut().mark_needs_layout(leaf);
            run.owner_mut().mark_needs_layout(id);
            run.pump();
            assert!(
                !(object.overflows)(run.owner(), id),
                "{name} {failure:?}: the next good layout answers again"
            );
        }
    }
}

/// A layout that returns early answers like one that runs to the end: an
/// overflowing box that loses its child, and an indexed stack whose index
/// becomes none, no longer clip.
pub(super) fn harness_an_overflow_answer_is_cleared_by_a_layout_that_returns_early() {
    for object in OVERFLOW_BOXES {
        let name = object.name;
        let mut run = mount(
            Host::Alone,
            (object.over)(Clip::HardEdge, content(leaf_of(TARGET))),
        );
        let id = run.id("box");
        assert!(
            (object.overflows)(run.owner(), id),
            "{name}: precondition: the content overflows"
        );
        (object.empty)(&mut run);
        run.pump();
        assert!(
            !(object.overflows)(run.owner(), id),
            "{name}: the early exit clears the answer"
        );
        assert!(!records_a_clip(&run), "{name}: and pushes no clip");
    }
}

/// When nothing overflows, a box reports no clip and pushes none. Under
/// `Clip::None` a box that overflows reports no clip either, and a fitted box
/// over an empty child, which paints nothing, does not overflow.
pub(super) fn harness_no_overflow_reports_no_clip() {
    for object in OVERFLOW_BOXES {
        let name = object.name;
        let run = mount(
            Host::Alone,
            (object.over)(
                Clip::HardEdge,
                inside_child(leaf_of(Rect::from_ltrb(0.0, 0.0, 50.0, 50.0))),
            ),
        );
        let id = run.id("box");
        assert!(!(object.overflows)(run.owner(), id), "{name}");
        assert_eq!(
            (object.clip_of)(run.owner(), id, run.box_geometry(id)),
            None,
            "{name}: nothing overflows, so no clip is reported"
        );
        assert!(!records_a_clip(&run), "{name}: and none is pushed");

        let run = mount(
            Host::Alone,
            (object.over)(Clip::None, content(leaf_of(TARGET))),
        );
        let id = run.id("box");
        assert!((object.overflows)(run.owner(), id), "{name}");
        assert_eq!(
            (object.clip_of)(run.owner(), id, run.box_geometry(id)),
            None,
            "{name}: Clip::None reports no clip though the content overflows"
        );
        assert!(!records_a_clip(&run), "{name}: and pushes none");
    }

    let run = mount(
        Host::Alone,
        fitted_box(
            Clip::HardEdge,
            inside_child(box_node(RenderColoredBox::red(0.0, 0.0))),
        ),
    );
    assert!(
        !(FITTED_BOX.overflows)(run.owner(), run.id("box")),
        "a fitted box over an empty child does not overflow"
    );

    // The crop is decided in layout alone, so a fit change relayouts: a fit
    // that scales the content into the box clears the clip.
    let mut run = mount(
        Host::Alone,
        fitted_box(Clip::HardEdge, content(leaf_of(TARGET))),
    );
    let id = run.id("box");
    assert!(
        (FITTED_BOX.overflows)(run.owner(), id),
        "precondition: an unscaled fit crops the content"
    );
    flui_rendering::testing::update_render_object::<RenderFittedBox>(
        run.owner_mut(),
        id,
        |fitted| fitted.set_fit(BoxFit::Contain),
    );
    run.pump();
    assert!(
        !(FITTED_BOX.overflows)(run.owner(), id),
        "a fit that scales the content into the box crops nothing"
    );
    assert!(!records_a_clip(&run), "and pushes no clip");
}

/// An unclipped overflow is painted, published and reports its region in
/// full, but a pointer reaches it only inside the box: a hit test is bounded
/// by each ancestor's box, clipped or not. A parent sized to hold what must
/// be hit is the remedy.
pub(super) fn harness_an_unclipped_overflow_is_painted_and_published_but_hit_only_inside_the_box() {
    for object in OVERFLOW_BOXES {
        let name = object.name;
        let run = mount(
            Host::Alone,
            (object.over)(Clip::None, content(leaf_of(TARGET))),
        );
        assert_eq!(
            painted_part(&run, TARGET),
            Some(TARGET),
            "{name}: target is painted in full"
        );
        assert_eq!(
            published_as(&run, "target"),
            Some((TARGET, false)),
            "{name}: target is published in full"
        );
        assert_eq!(
            run.owner().input_regions().first(),
            Some(&InputRegion::new(EXCLUSION, TARGET)),
            "{name}: target reports its whole region"
        );
        for point in samples_around(TARGET) {
            assert_eq!(
                hits(&run, "target", point),
                TARGET.contains(point) && BOX.contains(point),
                "{name}: target is hit at {point:?} only inside the box"
            );
        }
    }
}

// ============================================================================
// The keeper guard
// ============================================================================

/// The record types whose keepers must run the shared row.
const RECORD_TYPES: [&str; 2] = ["OverflowClip", "ShownChildren"];

/// The shared row's generic function; a call names the keeper it runs as its
/// type argument.
const SHARED_ROW: &str = "hits_and_publishes_a_child_only_where_it_paints";

/// Each struct `source` declares, with the code of its field types: the text
/// after the first `:` of each line of a braced body, or the parenthesised
/// list of a tuple struct. A struct starts at a line whose code
/// (`code_of`) opens `struct ` under any visibility, and its braced body ends
/// at the first line below it that is `}` at the struct line's indentation.
fn struct_fields(source: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = source.lines().collect();
    let mut structs = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        let code = code_of(line);
        let declaration = code
            .strip_prefix("pub ")
            .or_else(|| {
                code.strip_prefix("pub(")
                    .and_then(|rest| rest.split_once(") ").map(|(_, rest)| rest))
            })
            .unwrap_or(&code);
        let Some(rest) = declaration.strip_prefix("struct ") else {
            continue;
        };
        let name: String = rest
            .chars()
            .take_while(|&c| c.is_alphanumeric() || c == '_')
            .collect();
        let after_name = &rest[name.len()..];
        let fields = if let Some((_, tuple)) = after_name.split_once('(') {
            tuple.to_owned()
        } else if after_name.ends_with('{') {
            let indent = &line[..line.len() - line.trim_start().len()];
            let closing = format!("{indent}}}");
            lines[at + 1..]
                .iter()
                .take_while(|body| body.trim_end() != closing)
                .filter_map(|body| {
                    code_of(body)
                        .split_once(':')
                        .map(|(_, field_type)| field_type.to_owned())
                })
                .collect::<Vec<String>>()
                .join(" ")
        } else {
            String::new()
        };
        structs.push((name, fields));
    }
    structs
}

/// The structs of `sources` that keep a shown record: a field whose type
/// names a record type, or a struct that keeps one, as a whole identifier.
fn keepers<'a>(sources: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
    let structs: Vec<(String, String)> = sources.into_iter().flat_map(struct_fields).collect();
    let mut kept: BTreeSet<String> = RECORD_TYPES.iter().map(|&name| name.to_owned()).collect();
    let mut keepers = BTreeSet::new();
    loop {
        let found: Vec<String> = structs
            .iter()
            .filter(|(name, fields)| {
                !keepers.contains(name)
                    && kept
                        .iter()
                        .any(|record| names_whole_identifier(fields, record))
            })
            .map(|(name, _)| name.clone())
            .collect();
        if found.is_empty() {
            return keepers;
        }
        kept.extend(found.iter().cloned());
        keepers.extend(found);
    }
}

/// The type arguments of the calls of the shared row in `source`'s code:
/// `hits_and_publishes_a_child_only_where_it_paints::<RenderStack>` names
/// `RenderStack`. A call in a comment names nothing.
fn shared_row_calls(source: &str) -> BTreeSet<String> {
    let call = format!("{SHARED_ROW}::<");
    source
        .lines()
        .flat_map(|line| {
            let code = code_of(line);
            code.match_indices(&call)
                .map(|(start, _)| {
                    code[start + call.len()..]
                        .chars()
                        .take_while(|&c| c.is_alphanumeric() || c == '_')
                        .collect::<String>()
                })
                .collect::<Vec<String>>()
        })
        .collect()
}

/// The `.rs` files under the crate's `src/`, read from the run's
/// `CARGO_MANIFEST_DIR` first, as `family_directory` does.
fn crate_sources() -> Vec<(std::path::PathBuf, String)> {
    fn read(directory: &std::path::Path, out: &mut Vec<(std::path::PathBuf, String)>) {
        let entries = std::fs::read_dir(directory)
            .unwrap_or_else(|error| panic!("{}: {error}", directory.display()));
        for entry in entries {
            let path = entry
                .expect("the source directory lists its entries")
                .path();
            if path.is_dir() {
                read(&path, out);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let source = std::fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
                out.push((path, source));
            }
        }
    }
    let root = std::env::var_os("CARGO_MANIFEST_DIR")
        .map_or_else(
            || std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
            std::path::PathBuf::from,
        )
        .join("src");
    let mut out = Vec::new();
    read(&root, &mut out);
    out
}

#[test]
fn every_keeper_of_a_shown_record_runs_the_shared_row() {
    let sources = crate_sources();
    let keepers = keepers(sources.iter().map(|(_, source)| source.as_str()));
    assert!(
        keepers.contains("RenderStack") && keepers.contains("RenderIndexedStack"),
        "precondition: the guard finds the keepers it knows, directly and through a field: \
         {keepers:?}"
    );
    let called = shared_row_calls(include_str!("shown.rs"));
    let missing: Vec<&String> = keepers.difference(&called).collect();
    assert!(
        missing.is_empty(),
        "these types keep an {RECORD_TYPES:?} field but no row calls \
         `{SHARED_ROW}::<Type>` for them: {missing:?}"
    );
}

#[test]
fn the_keeper_guard_finds_a_record_field_in_literal_source() {
    let source = "pub struct RenderA {\n    overflow: OverflowClip,\n}\n\npub(crate) struct RenderB {\n    // shown: ShownChildren,\n    inner: RenderA,\n}\n\nstruct RenderC(ShownChildren);\n\nstruct RenderD {\n    clip: OverflowClipLike,\n}\n\nmod tests {\n    struct Nested {\n        shown: ShownChildren,\n    }\n}\n\nstruct RenderE {\n    count: usize,\n}\n";
    assert_eq!(
        keepers([source]),
        ["Nested", "RenderA", "RenderB", "RenderC"]
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<String>>(),
        "a record field, a keeper field and a tuple field count; a commented field, a longer \
         name and a plain field do not"
    );
    let calls = format!(
        "    {SHARED_ROW}::<RenderA>(A, || a());\n    // {SHARED_ROW}::<RenderB>(B, || b());\n    {SHARED_ROW}::<RenderC>(\n"
    );
    assert_eq!(
        shared_row_calls(&calls),
        ["RenderA", "RenderC"]
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<String>>(),
        "a call names its type argument; a call in a comment names nothing"
    );
}
