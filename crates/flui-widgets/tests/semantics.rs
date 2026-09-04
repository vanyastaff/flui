//! Widget-level coverage for the accessibility semantics wrappers.

use crate::common::{lay_out, loose, size};
use flui_widgets::{ExcludeSemantics, MergeSemantics, Semantics, SizedBox};

#[test]
fn semantics_widget_mounts_annotations_render_object() {
    let laid = lay_out(
        Semantics::new()
            .container(true)
            .label("Submit")
            .button(true)
            .enabled(true)
            .child(SizedBox::new(40.0, 20.0)),
        loose(200.0),
    );

    let root = laid.find_by_render_type("RenderSemanticsAnnotations");
    assert_eq!(root, laid.root());
    assert_eq!(laid.size(root), size(40.0, 20.0));
    assert_eq!(laid.size(laid.only_child(root)), size(40.0, 20.0));
}

#[test]
fn merge_semantics_widget_mounts_merge_render_object() {
    let laid = lay_out(
        MergeSemantics::new().child(SizedBox::new(30.0, 18.0)),
        loose(200.0),
    );

    let root = laid.find_by_render_type("RenderMergeSemantics");
    assert_eq!(root, laid.root());
    assert_eq!(laid.size(root), size(30.0, 18.0));
}

#[test]
fn exclude_semantics_widget_mounts_exclude_render_object() {
    let laid = lay_out(
        ExcludeSemantics::new().child(SizedBox::new(24.0, 16.0)),
        loose(200.0),
    );

    let root = laid.find_by_render_type("RenderExcludeSemantics");
    assert_eq!(root, laid.root());
    assert_eq!(laid.size(root), size(24.0, 16.0));
}

/// A viewport must not hand a screen reader a rect for content that is not on
/// screen. Rows scrolled just past the edge stay in the tree — a user can ask
/// to scroll to them — but are flagged hidden and narrowed to the part of the
/// cache area they occupy; rows past the cache area are absent entirely.
///
/// Oracle: `RenderViewportBase.describeSemanticsClip` (bounds grown by the
/// cache extent along the axis) and `describeApproximatePaintClip` (the bounds
/// themselves), applied by `_SemanticsGeometry`.
#[test]
fn viewport_clips_the_semantics_rects_of_off_screen_rows() {
    use flui_view::{BoxedView, ViewExt as _};
    use flui_widgets::{CustomScrollView, Semantics, SliverFixedExtentList};

    let rows: Vec<BoxedView> = (0..4)
        .map(|i| {
            Semantics::new()
                .container(true)
                .label(format!("row {i}"))
                .child(SizedBox::new(200.0, 200.0))
                .boxed()
        })
        .collect();

    // A 200 px viewport over 4 × 200 px of rows, default 250 px cache extent:
    // the semantics clip is y ∈ [-250, 450], the paint clip y ∈ [0, 200].
    let mut laid = lay_out(
        CustomScrollView::new((SliverFixedExtentList::new(200.0, rows),)),
        crate::common::tight(200.0, 200.0),
    );
    laid.enable_semantics();
    laid.pump();

    let tree = laid
        .a11y_tree()
        .expect("semantics was enabled before the frame");
    let row = |label: &str| {
        tree.find_by_label(label)
            .unwrap_or_else(|e| panic!("expected one {label}: {e}"))
    };

    let visible = row("row 0");
    let bounds = visible.bounds().expect("a laid-out row carries bounds");
    assert_eq!(
        (bounds.y0, bounds.y1),
        (0.0, 200.0),
        "the on-screen row keeps its own rect"
    );
    assert!(
        !visible.raw().is_hidden(),
        "the on-screen row must not be announced as hidden"
    );

    let just_past = row("row 1");
    let bounds = just_past.bounds().expect("bounds");
    assert_eq!(
        (bounds.y0, bounds.y1),
        (200.0, 400.0),
        "a row inside the cache area keeps its full rect"
    );
    assert!(
        just_past.raw().is_hidden(),
        "a row outside the paint clip is off-screen: hidden, not announced as visible"
    );

    let straddling = row("row 2");
    let bounds = straddling.bounds().expect("bounds");
    assert_eq!(
        (bounds.y0, bounds.y1),
        (400.0, 450.0),
        "a row straddling the cache boundary is narrowed to the part inside it"
    );

    assert!(
        tree.find_all_by_label("row 3").is_empty(),
        "a row past the cache area has no accessibility presence at all"
    );
}

/// A `ClipRect` does not hand a screen reader rects for content it clips away.
///
/// The clip was honoured by paint and by hit-test and by nothing a screen
/// reader could see: `RenderClip` never overrode
/// `describe_approximate_paint_clip`, so the semantics walk saw the trait's
/// `None` default and published full-size rects for children the clip visibly
/// cut in half.
///
/// Oracle: `_RenderCustomClip.describeApproximatePaintClip`
/// (`rendering/proxy_box.dart`) returns the clip whenever the behaviour is not
/// `none`.
#[test]
fn clip_rect_narrows_the_semantics_rect_of_the_content_it_clips() {
    use flui_types::Alignment;
    use flui_types::geometry::px;
    use flui_types::painting::Clip;
    use flui_widgets::{ClipRect, OverflowBox, Semantics};

    // `OverflowBox` is load-bearing: without it the outer 100x40 constrains
    // the child to 40 and there is no overflow to clip, so both legs would
    // agree and the oracle would prove nothing.
    let clipped = |clip: Clip| {
        SizedBox::new(100.0, 40.0).child(
            ClipRect::new().clip_behavior(clip).child(
                OverflowBox::new()
                    .with_alignment(Alignment::TOP_LEFT)
                    .with_max_height(px(200.0))
                    .child(
                        Semantics::new()
                            .container(true)
                            .label("Half hidden")
                            .child(SizedBox::new(100.0, 200.0)),
                    ),
            ),
        )
    };

    let mut laid = lay_out(clipped(Clip::HardEdge), crate::common::tight(100.0, 40.0));
    laid.enable_semantics();
    laid.pump();
    let tree = laid.a11y_tree().expect("semantics enabled");
    let bounds = tree
        .find_by_label("Half hidden")
        .expect("the clipped child still has a presence")
        .bounds()
        .expect("bounds");
    assert_eq!(
        (bounds.y0, bounds.y1),
        (0.0, 40.0),
        "the 200px child is narrowed to the 40px the clip leaves of it",
    );

    // `Clip::None` clips nothing, so it must impose nothing here either — the
    // same rule the paint side follows, and the leg that fails against an
    // implementation which returns its bounds unconditionally.
    let mut laid = lay_out(clipped(Clip::None), crate::common::tight(100.0, 40.0));
    laid.enable_semantics();
    laid.pump();
    let tree = laid.a11y_tree().expect("semantics enabled");
    let bounds = tree
        .find_by_label("Half hidden")
        .expect("present")
        .bounds()
        .expect("bounds");
    assert_eq!(
        (bounds.y0, bounds.y1),
        (0.0, 200.0),
        "an unclipped ClipRect narrows nothing",
    );
}

/// Scrolling must move the published semantics rects.
///
/// A scroll is a layout event and nothing else — the viewport's offset
/// listener calls `mark_needs_layout` and no element rebuilds (see
/// `crates/flui-objects/src/sliver/viewport.rs`). So unless layout itself
/// marks semantics, `run_semantics` finds nothing pending, publishes no
/// update, and the accessibility tree keeps describing where the content used
/// to be. Flutter pairs `performLayout()` with `markNeedsSemanticsUpdate()` in
/// both layout entry points (`rendering/object.dart`) for exactly this reason.
///
/// The oracle is the *rect*, not the presence of the node: a stale tree still
/// answers `find_by_label`, with the old geometry.
#[test]
fn scrolling_republishes_the_semantics_rects() {
    use flui_view::{BoxedView, ViewExt as _};
    use flui_widgets::{ScrollController, SliverFixedExtentList, Viewport};

    let builds = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let rows: Vec<BoxedView> = (0..2)
        .map(|i| {
            Semantics::new()
                .container(true)
                .label(format!("row {i}"))
                .child(RowBody {
                    builds: std::sync::Arc::clone(&builds),
                })
                .boxed()
        })
        .collect();

    // A `Viewport` in position mode: `set_pixels` writes the shared
    // `ScrollPosition` the render object holds, so the offset listener marks
    // it needing layout and no element rebuilds. `CustomScrollView::offset`
    // would take the other path — a new view configuration, which rebuilds —
    // and could not tell this defect apart.
    // Two 100 px rows in a 100 px viewport with the default 250 px cache
    // extent: both are inside the band from the first frame and stay there, so
    // the scroll below materialises nothing new. That is the whole point — a
    // scroll that builds a row would mark THAT row's semantics through
    // `apply_render_update_impact`, and the graft would refresh its moved
    // siblings for free, hiding the gap.
    let controller = ScrollController::new();
    controller.update_dimensions(100.0, 0.0, 100.0);
    let mut laid = lay_out(
        Viewport::new((SliverFixedExtentList::new(100.0, rows),)).position(controller.position()),
        crate::common::tight(200.0, 100.0),
    );
    laid.enable_semantics();
    laid.pump();

    let top_of = |laid: &crate::common::LaidOut, label: &str| -> f32 {
        laid.a11y_tree()
            .expect("semantics enabled")
            .find_by_label(label)
            .unwrap_or_else(|e| panic!("expected one {label}: {e}"))
            .bounds()
            .expect("a laid-out row carries bounds")
            .y0 as f32
    };

    let before = top_of(&laid, "row 1");
    assert_eq!(before, 100.0, "row 1 starts one row down");

    // Scroll by one row, and prove the frame that follows is layout-only:
    // if any element rebuilt, its `update_render_object` could mark semantics
    // by itself and this test would be measuring that instead.
    let builds_before = builds.load(std::sync::atomic::Ordering::SeqCst);
    controller.set_pixels(50.0);
    laid.tick();
    assert_eq!(
        builds.load(std::sync::atomic::Ordering::SeqCst),
        builds_before,
        "the scroll frame must rebuild no row, or this test is measuring a \
         rebuild's own semantics mark rather than layout's"
    );

    let after = top_of(&laid, "row 1");
    assert_eq!(
        after, 50.0,
        "a 50 px scroll moves row 1 from 100 to 50; a semantics tree that \
         layout never re-marked would still report {before}"
    );
}

/// A row body that counts its builds, so a scroll test can prove its frame
/// rebuilt nothing.
#[derive(Clone)]
struct RowBody {
    builds: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl flui_view::view::StatelessView for RowBody {
    fn build(&self, _ctx: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
        self.builds
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        SizedBox::new(200.0, 100.0)
    }
}

impl flui_view::View for RowBody {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

/// An `IndexedSemantics` publishes its child's set position, and an enclosing
/// lazy sliver publishes the set size.
///
/// Together these are the "12" and the "100" a screen reader reads out. They
/// come from different places on purpose: only the item knows which one it is,
/// and only the sliver knows how many there are — a virtualised list cannot
/// count its own materialised children and get the total.
///
/// The oracle is `position_in_set`/`size_of_set` on the published AccessKit
/// nodes rather than the framework-side configuration: the whole chain
/// (config → node → update → translation) existed in pieces before this and
/// connected to nothing, so asserting the near end proves nothing about what a
/// screen reader receives.
#[test]
fn an_indexed_item_publishes_its_position_and_the_sliver_its_set_size() {
    use flui_view::ViewExt as _;
    use flui_widgets::{IndexedSemantics, ScrollController, SliverList, Viewport};

    const ROWS: usize = 3;

    let controller = ScrollController::new();
    let mut laid = lay_out(
        Viewport::new((SliverList::new(
            ROWS,
            100.0,
            std::rc::Rc::new(|i: usize| {
                (i < ROWS).then(|| {
                    // The index sits INSIDE the row's semantics container. A
                    // non-boundary config is absorbed by its nearest ANCESTOR
                    // boundary here, so an `IndexedSemantics` above the
                    // container would index whatever node forms above the row
                    // — the sliver — instead of the row itself. Flutter's
                    // delegates wrap outside because its merge runs the other
                    // way; this is the FLUI placement.
                    Semantics::new()
                        .container(true)
                        .label(format!("row {i}"))
                        .child(IndexedSemantics::new(i as i32).child(SizedBox::new(200.0, 100.0)))
                        .boxed()
                })
            }),
        ),))
        .position(controller.position()),
        crate::common::tight(200.0, 300.0),
    );
    laid.enable_semantics();
    laid.pump();

    let tree = laid.a11y_tree().expect("semantics enabled");
    for (announced, label) in [(1usize, "row 0"), (2, "row 1"), (3, "row 2")] {
        let node = tree
            .find_by_label(label)
            .unwrap_or_else(|e| panic!("expected one {label}: {e}"));
        assert_eq!(
            node.position_in_set(),
            Some(announced),
            "{label} must publish a one-based set position, converted from the \
             zero-based index the widget carries",
        );
    }

    // The other half: only the sliver knows how many rows there are, so the
    // total has to come from it. Asserted rather than merely described — a
    // published position with no size is "item 12 of ?", which is what this
    // shipped as before the sliver described its own configuration.
    let sizes: Vec<usize> = tree.nodes().filter_map(|node| node.size_of_set()).collect();
    assert_eq!(
        sizes,
        vec![ROWS],
        "exactly one node — the sliver — must publish the set size",
    );
}
