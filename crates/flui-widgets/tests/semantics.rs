//! Widget-level coverage for the accessibility semantics wrappers.

use crate::common::{lay_out, loose, size};
use flui_rendering::semantics::{SemanticsAction, SemanticsActionHandler, semantics_action_for};
use flui_widgets::{ExcludeSemantics, MergeSemantics, Semantics, SizedBox, Text};

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

// ===========================================================================
// RenderParagraph — plain text publishes its own label
// ===========================================================================

/// `RenderParagraph::describe_semantics_configuration` (`crates/flui-objects/
/// src/text/paragraph.rs`) must publish a labelled semantics node for
/// non-empty text — without it, no screen reader ever announces a `Text`
/// widget, and `A11yTree::find_by_label` cannot locate one. Red before that
/// implementation existed: `describe_semantics_configuration` was the
/// `RenderBox` trait default (a no-op), so `find_by_label("hello")` failed
/// with `A11yQueryError::NotFound`.
#[test]
fn text_with_content_publishes_a_node_labelled_with_its_own_text() {
    let mut laid = lay_out(Text::new("hello"), loose(200.0));
    laid.enable_semantics();
    laid.pump();

    let tree = laid
        .a11y_tree()
        .expect("semantics enabled before the frame");
    tree.find_by_label("hello")
        .unwrap_or_else(|error| panic!("expected one node labelled \"hello\": {error}"));
}

/// The companion negative case: empty text contributes no label anywhere in
/// the tree (see `crates/flui-objects/ARCHITECTURE.md`'s "`RenderParagraph`
/// publishes no semantics node for empty text" mapping decision) — an empty
/// `RenderParagraph` must not merge a spurious empty label into whatever
/// boundary it sits under.
#[test]
fn empty_text_publishes_no_label_anywhere() {
    let mut laid = lay_out(Text::new(""), loose(200.0));
    laid.enable_semantics();
    laid.pump();

    let tree = laid
        .a11y_tree()
        .expect("semantics enabled before the frame");
    assert!(
        tree.nodes().all(|node| node.label().is_none()),
        "an empty Text must not publish a label on any node. Tree was:\n{}",
        tree.describe()
    );
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

/// A row whose first render object is semantics-transparent still announces.
///
/// The stamp lands on the item's FIRST render descendant, which is whatever the
/// builder returns — very often a `Padding` or an `Align` with no semantics of
/// its own. Such a node does not contribute a semantics node, so a position
/// applied only where a node forms never reaches the row that carries the
/// label.
///
/// The sibling tests all put `Semantics` at the root, which is exactly the
/// shape that works either way; this is the one that fails when the stamp is
/// only consulted for contributing nodes.
#[test]
fn a_row_behind_a_transparent_root_still_announces_its_position() {
    use flui_view::ViewExt as _;
    use flui_widgets::{Padding, ScrollController, SliverList, Viewport};

    const ROWS: usize = 3;

    let controller = ScrollController::new();
    let mut laid = lay_out(
        Viewport::new((SliverList::new(
            ROWS,
            100.0,
            std::rc::Rc::new(|i: usize| {
                (i < ROWS).then(|| {
                    // Padding is the item's root: it carries the stamp and has
                    // no semantics of its own.
                    Padding::all(4.0)
                        .child(
                            Semantics::new()
                                .container(true)
                                .label(format!("padded {i}"))
                                .child(SizedBox::new(180.0, 92.0)),
                        )
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
    for (announced, label) in [(1usize, "padded 0"), (2, "padded 1"), (3, "padded 2")] {
        let node = tree
            .find_by_label(label)
            .unwrap_or_else(|e| panic!("expected one {label}: {e}"));
        assert_eq!(
            node.position_in_set(),
            Some(announced),
            "{label} must announce its position even though the stamped node \
             is a transparent Padding rather than the row itself",
        );
    }
}

/// An explicit `IndexedSemantics` overrides the stamped index.
///
/// Both paths reach the same property, so precedence has to be decided rather
/// than left to whichever writes last. The explicit widget wins: hand-indexed
/// content inside a lazy list — a grid of cards that indexes by row, a list
/// whose items are logically grouped — is the reason `IndexedSemantics` stays
/// public, and a stamp that overwrote it would make that impossible.
///
/// The declared indices are deliberately the REVERSE of the stamped ones, so
/// the assertion fails whichever way the precedence is wrong. With them equal
/// the test would pass against both orders, which is what the sibling test
/// above cannot rule out on its own.
#[test]
fn an_explicit_index_overrides_the_one_the_sliver_stamped() {
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
                    // Reversed: row 0 declares index 2, row 2 declares 0.
                    let declared = (ROWS - 1 - i) as i32;
                    Semantics::new()
                        .container(true)
                        .label(format!("override {i}"))
                        .child(IndexedSemantics::new(declared).child(SizedBox::new(200.0, 100.0)))
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
    for (announced, label) in [(3usize, "override 0"), (2, "override 1"), (1, "override 2")] {
        let node = tree
            .find_by_label(label)
            .unwrap_or_else(|e| panic!("expected one {label}: {e}"));
        assert_eq!(
            node.position_in_set(),
            Some(announced),
            "{label} must announce the index its own IndexedSemantics declared, \
             not the one the sliver stamped",
        );
    }
}

/// Two slivers in one viewport number independently — the reference's reason
/// for `semanticIndexOffset`.
///
/// Flutter: "If multiple delegates are used in a single scroll view, then the
/// indexes will not be correct by default. The `semanticIndexOffset` can be
/// used to offset the semantic indexes of each delegate so that the indexes are
/// monotonically increasing."
///
/// This pins the DEFAULT, which is the same as the reference's: each delegate
/// numbers from zero, and a caller composing several says so explicitly. It
/// exists so the composed case is a checked state rather than an assumption,
/// and so the offset landing has something to change.
#[test]
fn two_slivers_in_one_viewport_each_number_from_zero_by_default() {
    use flui_view::ViewExt as _;
    use flui_widgets::{ScrollController, SliverList, Viewport};

    let list = |tag: &'static str, count: usize| {
        SliverList::new(
            count,
            60.0,
            std::rc::Rc::new(move |i: usize| {
                (i < count).then(|| {
                    Semantics::new()
                        .container(true)
                        .label(format!("{tag} {i}"))
                        .child(SizedBox::new(200.0, 60.0))
                        .boxed()
                })
            }),
        )
    };

    let controller = ScrollController::new();
    let mut laid = lay_out(
        Viewport::new((list("first", 2), list("second", 2))).position(controller.position()),
        crate::common::tight(200.0, 400.0),
    );
    laid.enable_semantics();
    laid.pump();

    let tree = laid.a11y_tree().expect("semantics enabled");
    let position = |label: &str| {
        tree.find_by_label(label)
            .unwrap_or_else(|e| panic!("expected one {label}: {e}"))
            .position_in_set()
    };

    assert_eq!(
        (position("first 0"), position("first 1")),
        (Some(1), Some(2)),
        "the first delegate numbers its own children"
    );
    assert_eq!(
        (position("second 0"), position("second 1")),
        (Some(1), Some(2)),
        "and so does the second — independently, which is why the reference \
         has `semanticIndexOffset` at all"
    );
}

/// Composed slivers read monotonically once the second declares its offset.
///
/// The sibling test above pins the default: each delegate numbers from zero,
/// as the reference does. This is the escape the reference provides for it —
/// `semanticIndexOffset`, whose docs give this exact scenario: "if a scroll
/// view contains two delegates where the first has 10 children contributing
/// semantics, then the second delegate should offset its children by 10."
///
/// The offset and the composed SIZE are declared together, and the assertions
/// check both. An offset alone would announce "item 3 of 2" for the second
/// delegate's first row — a position from the composed set paired with a size
/// from its own.
#[test]
fn a_composed_offset_makes_two_slivers_read_as_one_set() {
    use flui_view::ViewExt as _;
    use flui_widgets::{ScrollController, SliverList, Viewport};

    const FIRST: usize = 2;
    const SECOND: usize = 2;
    const TOTAL: i32 = (FIRST + SECOND) as i32;

    let list = |tag: &'static str, count: usize| {
        SliverList::new(
            count,
            60.0,
            std::rc::Rc::new(move |i: usize| {
                (i < count).then(|| {
                    Semantics::new()
                        .container(true)
                        .label(format!("{tag} {i}"))
                        .child(SizedBox::new(200.0, 60.0))
                        .boxed()
                })
            }),
        )
    };

    let controller = ScrollController::new();
    let mut laid = lay_out(
        Viewport::new((
            list("a", FIRST).semantics(
                flui_view::element::SemanticSetMapping::one_to_one(FIRST.into())
                    .composed_at(0, Some(TOTAL)),
            ),
            list("b", SECOND).semantics(
                flui_view::element::SemanticSetMapping::one_to_one(SECOND.into())
                    .composed_at(FIRST as i32, Some(TOTAL)),
            ),
        ))
        .position(controller.position()),
        crate::common::tight(200.0, 400.0),
    );
    laid.enable_semantics();
    laid.pump();

    let tree = laid.a11y_tree().expect("semantics enabled");
    let announced = |label: &str| {
        let node = tree
            .find_by_label(label)
            .unwrap_or_else(|e| panic!("expected one {label}: {e}"));
        (node.position_in_set(), node.size_of_set())
    };

    assert_eq!(
        [
            announced("a 0"),
            announced("a 1"),
            announced("b 0"),
            announced("b 1"),
        ],
        [
            (Some(1), Some(4)),
            (Some(2), Some(4)),
            (Some(3), Some(4)),
            (Some(4), Some(4)),
        ],
        "the two delegates must read as one set of four, numbered 1..=4 — \
         `(Some(1), _)` for `b 0` means the offset was ignored, and \
         `(_, Some(2))` means the size was left as that delegate's own",
    );
}

/// A shrinking list stops announcing the old total.
///
/// Residents that keep their index are not relocated, so nothing moves them —
/// and a stamp written only on mount or on a move leaves them announcing the
/// set they were mounted into. A list going from six items to three would keep
/// saying "of 6" for every row that stayed put, which is worse than saying
/// nothing: the reader is told a definite, wrong total.
#[test]
fn shrinking_a_list_stops_announcing_the_old_total() {
    use flui_view::ViewExt as _;
    use flui_widgets::{ScrollController, SliverList, Viewport};

    let rows = |count: usize| {
        Viewport::new((SliverList::new(
            count,
            60.0,
            std::rc::Rc::new(move |i: usize| {
                (i < count).then(|| {
                    Semantics::new()
                        .container(true)
                        .label(format!("row {i}"))
                        .child(SizedBox::new(200.0, 60.0))
                        .boxed()
                })
            }),
        ),))
    };

    let controller = ScrollController::new();
    let mut laid = lay_out(
        rows(6).position(controller.position()),
        crate::common::tight(200.0, 400.0),
    );
    laid.enable_semantics();
    laid.pump();

    let before = laid.a11y_tree().expect("semantics enabled");
    assert_eq!(
        before
            .find_by_label("row 0")
            .expect("row 0 present")
            .size_of_set(),
        Some(6),
        "precondition: the six-item list announces six",
    );

    // Same rows 0..3, still at the same indices — nothing relocates them.
    laid.pump_widget(rows(3).position(controller.position()));
    laid.pump();

    let after = laid.a11y_tree().expect("semantics enabled");
    assert_eq!(
        after
            .find_by_label("row 0")
            .expect("row 0 survives the shrink")
            .size_of_set(),
        Some(3),
        "a resident that never moved must still pick up the new total; \
         `Some(6)` here is the set it was mounted into, which no longer exists",
    );
}

/// A separated list counts its ITEMS, not its interleaved children.
///
/// `SliverList::separated` builds `2n - 1` children: items at even logical
/// indices, separators at odd ones. A position derived from the logical index
/// therefore announces separators as set members and gives the real items
/// positions 1, 3, 5 — and a size taken from the child count says "of 5" on a
/// three-item list. Both are exactly what
/// `crates/flui-rendering/ARCHITECTURE.md` warned a naive derivation would do.
#[test]
fn a_separated_lists_positions_count_items_not_separators() {
    use flui_view::ViewExt as _;
    use flui_widgets::{ScrollController, SliverList, Viewport};

    const ITEMS: usize = 3;

    let controller = ScrollController::new();
    let mut laid = lay_out(
        Viewport::new((SliverList::separated(
            ITEMS,
            50.0,
            std::rc::Rc::new(|i: usize| {
                (i < ITEMS).then(|| {
                    Semantics::new()
                        .container(true)
                        .label(format!("item {i}"))
                        .child(SizedBox::new(200.0, 50.0))
                        .boxed()
                })
            }),
            std::rc::Rc::new(|i: usize| {
                Some(
                    Semantics::new()
                        .container(true)
                        .label(format!("sep {i}"))
                        .child(SizedBox::new(200.0, 50.0))
                        .boxed(),
                )
            }),
        ),))
        .position(controller.position()),
        crate::common::tight(200.0, 400.0),
    );
    laid.enable_semantics();
    laid.pump();

    let tree = laid.a11y_tree().expect("semantics enabled");

    for (announced, label) in [(1usize, "item 0"), (2, "item 1"), (3, "item 2")] {
        let node = tree
            .find_by_label(label)
            .unwrap_or_else(|e| panic!("expected one {label}: {e}"));
        assert_eq!(
            node.position_in_set(),
            Some(announced),
            "{label} must be counted among the ITEMS; positions 1, 3, 5 mean \
             the separators were counted too",
        );
        assert_eq!(
            node.size_of_set(),
            Some(ITEMS),
            "{label} must announce the item count, not the interleaved child \
             count of {}",
            ITEMS * 2 - 1,
        );
    }

    for label in ["sep 0", "sep 1"] {
        let node = tree
            .find_by_label(label)
            .unwrap_or_else(|e| panic!("expected one {label}: {e}"));
        assert_eq!(
            node.position_in_set(),
            None,
            "{label} is not a member of the set and must announce no position",
        );
    }
}

/// A lazy list item publishes its position with NO wrapper widget.
///
/// Flutter's delegates supply this by wrapping every materialised item in an
/// `IndexedSemantics` (`addSemanticIndexes`, on by default) — one render node
/// per item, carrying an index captured when the item was built. FLUI reads the
/// slot the sliver already stamps into the child's parent data, which costs no
/// node and tracks the row as the band moves.
///
/// The rows here carry no `IndexedSemantics` at all. That is the whole point:
/// the sibling test above uses one and proves the explicit path still works,
/// while this one proves a plain lazy child is announced without it.
#[test]
fn a_lazy_child_publishes_its_position_without_an_indexed_semantics_wrapper() {
    use flui_view::ViewExt as _;
    use flui_widgets::{ScrollController, SliverList, Viewport};

    const ROWS: usize = 3;

    let controller = ScrollController::new();
    let mut laid = lay_out(
        Viewport::new((SliverList::new(
            ROWS,
            100.0,
            std::rc::Rc::new(|i: usize| {
                (i < ROWS).then(|| {
                    Semantics::new()
                        .container(true)
                        .label(format!("bare {i}"))
                        .child(SizedBox::new(200.0, 100.0))
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
    for (announced, label) in [(1usize, "bare 0"), (2, "bare 1"), (3, "bare 2")] {
        let node = tree
            .find_by_label(label)
            .unwrap_or_else(|e| panic!("expected one {label}: {e}"));
        assert_eq!(
            node.position_in_set(),
            Some(announced),
            "{label} must publish a one-based set position derived from the \
             index the sliver stamped, with nothing in the tree wrapping it",
        );
        assert_eq!(
            node.size_of_set(),
            Some(ROWS),
            "...and the set's size beside it, since the delegate's own count \
             does describe the set these rows belong to",
        );
    }
}

/// An `IndexedSemantics` publishes its child's position within the set.
///
/// That is the "12" a screen reader reads out in "item 12 of 100". The "100"
/// is not published at all yet — see the assertion at the end for why the
/// obvious place for it is the wrong one.
///
/// The oracle is `position_in_set`/`size_of_set` on the published AccessKit
/// nodes rather than the framework-side configuration: the whole chain
/// (config → node → update → translation) existed in pieces before this and
/// connected to nothing, so asserting the near end proves nothing about what a
/// screen reader receives.
#[test]
fn an_indexed_item_publishes_its_position_in_the_set() {
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

    // No size, and that is the CONTRACT here rather than a gap. Explicit
    // numbering exists to describe a set the delegate does not materialise —
    // six cards numbered as three rows, or an offset numbering — so pairing
    // these positions with the delegate's own count would announce "item 2 of
    // 6" for a row the caller numbered within a set of three, and an offset
    // could put the position past the size entirely.
    //
    // A caller who wants both supplies both. "item 2 of ?" is the honest
    // degradation, the same trade made for an unresolved `ItemCount::Unknown`.
    // The derived path publishes both — see the sibling test below.
    let sizes: Vec<usize> = tree.nodes().filter_map(|node| node.size_of_set()).collect();
    assert!(
        sizes.is_empty(),
        "an explicitly numbered row must not be paired with the delegate's \
         count, which describes a different set; found {sizes:?}",
    );
}

/// A mapping change alone reaches the residents, without waiting for an
/// unrelated layout to carry it.
///
/// The adaptor refreshes its resident children inside the service pass that
/// follows a layout, and a rebuild that changes only the *numbering* leaves
/// the delegate — the builder and key callback — identical. Flagging the
/// residents for refresh without also scheduling that layout is inert: the
/// sliver stays clean, the pass never runs, and every resident goes on
/// announcing the position it was stamped with.
///
/// The delegate is deliberately one shared `StaticChildren`, cloned into both
/// roots. Building a fresh one per root would hand the adaptor a different
/// builder, take the changed-delegate path, and never exercise the case at
/// all.
#[test]
fn a_mapping_change_alone_restamps_the_resident_children() {
    use flui_view::ViewExt as _;
    use flui_view::element::{SemanticSetMapping, StaticChildren};
    use flui_widgets::{ScrollController, SliverList, Viewport};

    const ROWS: usize = 2;
    const SET: usize = 10;

    let row = |i: usize| {
        Semantics::new()
            .container(true)
            .label(format!("row {i}"))
            .child(SizedBox::new(200.0, 60.0))
            .boxed()
    };
    let children = StaticChildren::new((0..ROWS).map(row).collect::<Vec<_>>());
    let controller = ScrollController::new();

    let root = |offset: i32| {
        Viewport::new((SliverList::over(60.0, &children).semantics(
            SemanticSetMapping::one_to_one(ROWS.into())
                .composed_at(offset, i32::try_from(SET).ok()),
        ),))
        .position(controller.position())
    };

    let mut laid = lay_out(root(0), crate::common::tight(200.0, 400.0));
    laid.enable_semantics();
    laid.pump();

    let announced = |laid: &mut flui_widgets::testing::LaidOut, label: &str| {
        let tree = laid.a11y_tree().expect("semantics enabled");
        let node = tree
            .find_by_label(label)
            .unwrap_or_else(|e| panic!("expected one {label}: {e}"));
        (node.position_in_set(), node.size_of_set())
    };

    assert_eq!(
        [announced(&mut laid, "row 0"), announced(&mut laid, "row 1")],
        [(Some(1), Some(SET)), (Some(2), Some(SET))],
        "the un-offset mapping numbers the two rows 1 and 2 of the composed set"
    );

    // Same children, same delegate, same count — only the offset moves.
    laid.pump_widget(root(5));

    assert_eq!(
        [announced(&mut laid, "row 0"), announced(&mut laid, "row 1")],
        [(Some(6), Some(SET)), (Some(7), Some(SET))],
        "the new offset must reach the residents on this frame — still \
         announcing 1 and 2 means the refresh was flagged but no layout was \
         scheduled to carry it"
    );
}

/// The composed offset reaches the grid entry point too, not just the list.
///
/// `SliverList` and `SliverGrid` are both re-exports of the same
/// `SliverMultiBoxAdaptor` alias, so the mapping is declared identically on
/// each. This pins that shared route from the grid side: a regression that
/// wired the offset into the list's own layout rather than the adaptor's
/// stamping would keep the list test green and fail here.
#[test]
fn a_composed_offset_reaches_the_grid_entry_point() {
    use flui_view::ViewExt as _;
    use flui_view::element::{SemanticSetMapping, StaticChildren};
    use flui_widgets::{
        ScrollController, SliverGrid, SliverGridDelegateWithFixedCrossAxisCount, Viewport,
    };
    use std::sync::Arc;

    const CELLS: usize = 2;
    const SET: usize = 6;
    const OFFSET: i32 = 4;

    let cell = |i: usize| {
        Semantics::new()
            .container(true)
            .label(format!("cell {i}"))
            .child(SizedBox::new(200.0, 60.0))
            .boxed()
    };
    let children = StaticChildren::new((0..CELLS).map(cell).collect::<Vec<_>>());
    let controller = ScrollController::new();

    // One cell per row, so a grid position is a list position and the
    // assertion below reads the same either way.
    let delegate = Arc::new(SliverGridDelegateWithFixedCrossAxisCount::new(1));

    let mut laid = lay_out(
        Viewport::new((SliverGrid::over(delegate, &children).semantics(
            SemanticSetMapping::one_to_one(CELLS.into())
                .composed_at(OFFSET, i32::try_from(SET).ok()),
        ),))
        .position(controller.position()),
        crate::common::tight(200.0, 400.0),
    );
    laid.enable_semantics();
    laid.pump();

    let tree = laid.a11y_tree().expect("semantics enabled");
    let announced = |label: &str| {
        let node = tree
            .find_by_label(label)
            .unwrap_or_else(|e| panic!("expected one {label}: {e}"));
        (node.position_in_set(), node.size_of_set())
    };

    assert_eq!(
        [announced("cell 0"), announced("cell 1")],
        [(Some(5), Some(SET)), (Some(6), Some(SET))],
        "the grid's cells must carry the composed offset — `(Some(1), _)` \
         means the mapping never reached this entry point"
    );
}

/// The composed offset reaches the fixed-extent entry point, through both of
/// its delegate kinds.
///
/// This entry point is a widget-level struct that shadows the view-level
/// adaptor alias of the same name, rather than a re-export of it like
/// `SliverList` and `SliverGrid`. It therefore has to forward the mapping
/// explicitly, and the two delegate arms of its `build` are two places that
/// can forget to — hence one assertion per arm rather than one for the type.
#[test]
fn a_composed_offset_reaches_both_fixed_extent_delegate_kinds() {
    use flui_view::ViewExt as _;
    use flui_view::element::SemanticSetMapping;
    use flui_widgets::{ScrollController, SliverFixedExtentList, Viewport};

    const ROWS: usize = 2;
    const SET: usize = 8;
    const OFFSET: i32 = 3;

    let row = |i: usize| {
        Semantics::new()
            .container(true)
            .label(format!("row {i}"))
            .child(SizedBox::new(200.0, 60.0))
            .boxed()
    };
    let mapping =
        || SemanticSetMapping::one_to_one(ROWS.into()).composed_at(OFFSET, i32::try_from(SET).ok());

    // Positions are 1-based, so the first row reads OFFSET + 1.
    let expected = [
        (Some(OFFSET as usize + 1), Some(SET)),
        (Some(OFFSET as usize + 2), Some(SET)),
    ];

    let announce = |laid: &mut flui_widgets::testing::LaidOut| {
        laid.enable_semantics();
        laid.pump();
        let tree = laid.a11y_tree().expect("semantics enabled");
        let one = |label: &str| {
            let node = tree
                .find_by_label(label)
                .unwrap_or_else(|e| panic!("expected one {label}: {e}"));
            (node.position_in_set(), node.size_of_set())
        };
        [one("row 0"), one("row 1")]
    };

    // Static delegate.
    let mut statics = lay_out(
        Viewport::new((SliverFixedExtentList::new(60.0, (row(0), row(1))).semantics(mapping()),))
            .position(ScrollController::new().position()),
        crate::common::tight(200.0, 400.0),
    );
    assert_eq!(
        announce(&mut statics),
        expected,
        "the static delegate must carry the composed offset — `(Some(1), _)` \
         means the mapping never reached this entry point"
    );

    // Builder delegate: the same numbering, from the other arm of `build`.
    let mut built = lay_out(
        Viewport::new((SliverFixedExtentList::builder(60.0, ROWS, move |i| {
            (i < ROWS).then(|| row(i))
        })
        .semantics(mapping()),))
        .position(ScrollController::new().position()),
        crate::common::tight(200.0, 400.0),
    );
    assert_eq!(
        announce(&mut built),
        expected,
        "the builder delegate must number its children identically — the \
         mapping is not a property of which delegate serves them"
    );
}

/// A lazy sliver that declares no mapping still announces its real size.
///
/// The default mapping is derived from `item_count`, and every
/// static-children constructor sets that count *after* calling the adaptor's
/// `new`. A default materialised at construction is therefore frozen at the
/// count known then — zero — and every row announces "item N of 0", a pair
/// whose position exceeds its size and which AccessKit cannot represent.
///
/// Every other oracle in this file declares a mapping explicitly, which
/// overrides the default and hides this. This one must not: the assertion is
/// about the path a caller gets without asking for anything.
#[test]
fn a_lazy_sliver_with_no_declared_mapping_announces_its_real_set_size() {
    use flui_view::ViewExt as _;
    use flui_view::element::StaticChildren;
    use flui_widgets::{ListView, ScrollController, SliverList, Viewport};

    let row = |i: usize| {
        Semantics::new()
            .container(true)
            .label(format!("row {i}"))
            .child(SizedBox::new(200.0, 60.0))
            .boxed()
    };
    let announced = |laid: &mut flui_widgets::testing::LaidOut| {
        laid.enable_semantics();
        laid.pump();
        let tree = laid.a11y_tree().expect("semantics enabled");
        let one = |label: &str| {
            let node = tree
                .find_by_label(label)
                .unwrap_or_else(|e| panic!("expected one {label}: {e}"));
            (node.position_in_set(), node.size_of_set())
        };
        [one("row 0"), one("row 1")]
    };

    let mut list_view = lay_out(
        ListView::new(60.0, (row(0), row(1))).position(ScrollController::new().position()),
        crate::common::tight(200.0, 400.0),
    );
    assert_eq!(
        announced(&mut list_view),
        [(Some(1), Some(2)), (Some(2), Some(2))],
        "a two-row ListView announces a set of two — `Some(0)` is the count \
         the adaptor was constructed with, not the one its delegate holds"
    );

    let children = StaticChildren::new(vec![row(0), row(1)]);
    let mut sliver = lay_out(
        Viewport::new((SliverList::over(60.0, &children),))
            .position(ScrollController::new().position()),
        crate::common::tight(200.0, 400.0),
    );
    assert_eq!(
        announced(&mut sliver),
        [(Some(1), Some(2)), (Some(2), Some(2))],
        "and so does the sliver the list view is built on"
    );
}

/// Merging collapses its descendants in the **accessibility tree**, not just in
/// the render tree.
///
/// The sibling tests above assert that `RenderMergeSemantics` mounts. That
/// proves the render object exists; it says nothing about what a screen reader
/// is handed, and the two are separated by the whole assemble-and-translate
/// path. This asserts the end product.
///
/// The premise runs first and is load-bearing: without the wrapper both labels
/// must be separately findable. A test that only checked "the children are not
/// findable" would pass just as well against a tree where they never
/// contributed semantics at all — which is the ordinary state of affairs for a
/// bare `SizedBox`, and why the children here carry real `Semantics`.
#[test]
fn merge_semantics_collapses_its_descendants_in_the_a11y_tree() {
    use flui_view::ViewExt as _;
    use flui_widgets::Column;

    let labelled = |text: &'static str| {
        Semantics::new()
            .container(true)
            .label(text)
            .child(SizedBox::new(40.0, 20.0))
    };

    // Premise: unmerged, each child is its own node.
    let mut apart = lay_out(
        Column::new((labelled("first").boxed(), labelled("second").boxed())),
        loose(200.0),
    );
    apart.enable_semantics();
    apart.pump();
    let apart_tree = apart.a11y_tree().expect("semantics enabled");
    assert!(
        apart_tree.find_by_label("first").is_ok() && apart_tree.find_by_label("second").is_ok(),
        "premise: without merging, both labels are separately findable — \
         otherwise the assertion below proves nothing. Tree was:\n{}",
        apart_tree.describe()
    );

    let mut merged = lay_out(
        MergeSemantics::new().child(Column::new((
            labelled("first").boxed(),
            labelled("second").boxed(),
        ))),
        loose(200.0),
    );
    merged.enable_semantics();
    merged.pump();
    let merged_tree = merged.a11y_tree().expect("semantics enabled");

    // The combined label is the assertion that distinguishes merging from
    // DESTROYING. "neither label is findable" plus "fewer nodes" would both
    // hold if the merge path dropped its descendants outright, or kept only
    // one of them — which is exclusion's contract, not merging's.
    assert!(
        merged_tree.find_by_label("first second").is_ok(),
        "merging must carry its descendants' labels into one node, combined \
         and in order. Tree was:\n{}",
        merged_tree.describe()
    );
    assert!(
        merged_tree.find_by_label("first").is_err() && merged_tree.find_by_label("second").is_err(),
        "and neither descendant may remain addressable on its own; a reader \
         should meet one node, not three. Tree was:\n{}",
        merged_tree.describe()
    );
    assert!(
        merged_tree.len() < apart_tree.len(),
        "the merged tree must have strictly fewer nodes than the unmerged one \
         ({} vs {})",
        merged_tree.len(),
        apart_tree.len()
    );
}

/// Excluding removes its subtree from the accessibility tree.
///
/// Same shape as the merge test, and the premise matters for the same reason:
/// "not findable" is the default state of most of the tree, so the label must
/// be shown findable without the wrapper before its absence means anything.
#[test]
fn exclude_semantics_removes_its_subtree_from_the_a11y_tree() {
    let labelled = || {
        Semantics::new()
            .container(true)
            .label("hidden from readers")
            .child(SizedBox::new(40.0, 20.0))
    };

    let mut included = lay_out(labelled(), loose(200.0));
    included.enable_semantics();
    included.pump();
    let included_tree = included.a11y_tree().expect("semantics enabled");
    assert!(
        included_tree.find_by_label("hidden from readers").is_ok(),
        "premise: the label is findable when nothing excludes it. Tree was:\n{}",
        included_tree.describe()
    );

    let mut excluded = lay_out(ExcludeSemantics::new().child(labelled()), loose(200.0));
    excluded.enable_semantics();
    excluded.pump();
    let excluded_tree = excluded.a11y_tree().expect("semantics enabled");

    // Absence only means exclusion if the tree was published at all. A
    // translation that emitted nothing would satisfy the assertion below for
    // entirely the wrong reason, so the surviving root is checked first.
    assert_eq!(
        excluded_tree.len(),
        1,
        "the root must still be reachable — an empty tree would make the \
         absence below meaningless. Tree was:\n{}",
        excluded_tree.describe()
    );
    assert!(
        excluded_tree.find_by_label("hidden from readers").is_err(),
        "an excluded subtree must contribute nothing a reader can reach. \
         Tree was:\n{}",
        excluded_tree.describe()
    );
}

// ===========================================================================
// Actions: a platform request, routed back to the widget's own callback
// ===========================================================================

use std::assert_matches;
use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use flui_testing::a11y::A11yPoint;
use flui_testing::{Action, ActionData, ActionRequest, NodeId, TreeId, invoke_semantics_action};

/// Mounts `semantics` with semantics enabled and returns the single node
/// carrying `label`, together with a live view of the a11y tree.
///
/// The tree is returned alongside its own description so a failing assertion
/// can show what was actually there, matching this file's other a11y tests.
fn pump_labelled(
    semantics: Semantics,
) -> (
    flui_widgets::testing::LaidOut,
    flui_testing::A11yTree,
    NodeId,
) {
    let mut laid = lay_out(semantics, loose(200.0));
    laid.enable_semantics();
    laid.pump();
    let tree = laid
        .a11y_tree()
        .expect("semantics enabled before the frame");
    let node_id = tree
        .find_by_label(LABEL)
        .unwrap_or_else(|error| panic!("expected one node labelled {LABEL:?}: {error}"))
        .id();
    (laid, tree, node_id)
}

/// The label every action test mounts under; unique within its own tree.
const LABEL: &str = "Action Host";

/// Builds an `ActionRequest` addressed at `node_id`.
fn request(action: Action, node_id: NodeId, data: Option<ActionData>) -> ActionRequest {
    ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: node_id,
        data,
    }
}

/// A tap handler reaches its callback through a platform click request.
///
/// Two halves, and both are necessary: the node must *tell* the platform the
/// action exists, and pressing it must *do* something. A node that passes only
/// the first is the dead control this whole surface exists to rule out — an
/// action advertised outbound that nothing routes inbound.
#[test]
fn a_tap_handler_round_trips_from_a_platform_click_to_the_callback() {
    let activations = Arc::new(AtomicU32::new(0));
    let counted = Arc::clone(&activations);

    let (laid, tree, node_id) = pump_labelled(
        Semantics::new()
            .container(true)
            .label(LABEL)
            .on_tap(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            })
            .child(SizedBox::new(40.0, 20.0)),
    );

    assert!(
        tree.find_by_label(LABEL)
            .expect("node was located a moment ago")
            .supports_action(Action::Click),
        "a node with an on_tap handler must advertise a click, or no assistive \
         technology can reach it. Tree was:\n{}",
        tree.describe()
    );

    invoke_semantics_action(
        &laid.pipeline_owner(),
        request(Action::Click, node_id, None),
    )
    .expect("a click on a node advertising one must resolve");

    assert_eq!(
        activations.load(Ordering::SeqCst),
        1,
        "the handler must have run exactly once",
    );
}

/// A `GestureDetector`'s `on_tap` is reachable from assistive technology
/// without any pointer event: the detector advertises a click on the
/// nearest node, and a platform click request runs the `Rc` callback after
/// the frame the request schedules (see `GestureDetector`'s
/// "Assistive-technology activation").
///
/// Red-check: before the detector published the action, `supports_action`
/// was false on this node and the count stayed 0 — the live `AXPress` half
/// of `cargo xtask device macos-a11y` showed exactly that on the generated counter.
#[test]
fn a_gesture_detector_tap_is_reachable_through_a_platform_click() {
    let activations = Rc::new(Cell::new(0));
    let counted = Rc::clone(&activations);

    let (mut laid, tree, node_id) = pump_labelled(
        Semantics::new()
            .container(true)
            .button(true)
            .label(LABEL)
            .child(
                flui_widgets::GestureDetector::new()
                    .on_tap(move || counted.set(counted.get() + 1))
                    .child(SizedBox::new(40.0, 20.0)),
            ),
    );

    assert!(
        tree.find_by_label(LABEL)
            .expect("node was located a moment ago")
            .supports_action(Action::Click),
        "a GestureDetector with on_tap must advertise a click on its nearest node. \
         Tree was:\n{}",
        tree.describe()
    );

    invoke_semantics_action(
        &laid.pipeline_owner(),
        request(Action::Click, node_id, None),
    )
    .expect("a click on a node advertising one must resolve");
    assert_eq!(
        activations.get(),
        0,
        "the request is recorded, not performed inline — the callback runs after \
         the frame the request schedules"
    );

    // The scheduled rebuild drains the request; the callback runs after
    // that frame.
    laid.pump();
    assert_eq!(
        activations.get(),
        1,
        "the on_tap callback must have run exactly once"
    );

    // A control: the request was consumed, not left armed.
    laid.pump();
    assert_eq!(activations.get(), 1);
}

/// Without a handler the platform is told nothing.
///
/// The control for the test above: it shows the advertised click comes from the
/// handler and not from the annotation existing at all.
#[test]
fn a_semantics_node_with_no_tap_handler_advertises_no_click() {
    let (_laid, tree, _node_id) = pump_labelled(
        Semantics::new()
            .container(true)
            .label(LABEL)
            .child(SizedBox::new(40.0, 20.0)),
    );

    assert!(
        !tree
            .find_by_label(LABEL)
            .expect("one node labelled")
            .supports_action(Action::Click),
        "a node with no tap handler must not advertise a click. Tree was:\n{}",
        tree.describe()
    );
}

/// `block_user_actions` refuses a click even though a handler is registered.
///
/// The handler stays in the configuration — blocking is not unregistering — so
/// this is the case where a dispatch path that consulted the raw action set
/// instead of the effective one would run the callback anyway.
///
/// Both halves are measured, and both are asserted: the request errors *and*
/// the handler did not run, which is the refusal half; and the node does not
/// advertise the click, which is the advertise half. The second is not
/// incidental — `blocks_user_actions` narrows the effective action set that
/// snapshot export and input dispatch both consult, so a blocked node is
/// invisible to assistive technology rather than a control it can see and press
/// to no effect.
#[test]
fn block_user_actions_refuses_a_click_the_node_still_holds_a_handler_for() {
    let activations = Arc::new(AtomicU32::new(0));
    let counted = Arc::clone(&activations);

    let (laid, tree, node_id) = pump_labelled(
        Semantics::new()
            .container(true)
            .label(LABEL)
            .block_user_actions(true)
            .on_tap(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            })
            .child(SizedBox::new(40.0, 20.0)),
    );

    assert!(
        !tree
            .find_by_label(LABEL)
            .expect("one node labelled")
            .supports_action(Action::Click),
        "a blocked node must not advertise the click either — measured, it does \
         not, so the refusal is visible to the platform rather than only to a \
         dispatch that reaches the node. Tree was:\n{}",
        tree.describe()
    );

    let outcome = invoke_semantics_action(
        &laid.pipeline_owner(),
        request(Action::Click, node_id, None),
    );

    assert_matches!(
        outcome,
        Err(flui_testing::InvokeActionError::Resolution(_)),
        "a blocked node must refuse the request: the node still holds the \
         handler, so the refusal is resolution's — an unroutable action or a \
         malformed target would mean this test never reached the node at all",
    );
    assert_eq!(
        activations.load(Ordering::SeqCst),
        0,
        "and the handler must not have run",
    );
}

/// An argument-bearing action carries its payload through to the handler.
///
/// This is the half a no-argument round trip cannot see: the action can route
/// correctly and the payload still be dropped on the floor, which for a text
/// edit means the screen reader's input silently vanishes.
#[test]
fn a_set_text_request_carries_its_payload_into_the_handler() {
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);

    let (laid, tree, node_id) = pump_labelled(
        Semantics::new()
            .container(true)
            .label(LABEL)
            .on_set_text(move |text| {
                sink.lock()
                    .expect("this test never poisons its own lock")
                    .push(text.to_owned());
            })
            .child(SizedBox::new(40.0, 20.0)),
    );

    assert!(
        tree.find_by_label(LABEL)
            .expect("one node labelled")
            .supports_action(Action::SetValue),
        "an on_set_text handler must be advertised as a value change. Tree was:\n{}",
        tree.describe()
    );

    invoke_semantics_action(
        &laid.pipeline_owner(),
        request(
            Action::SetValue,
            node_id,
            Some(ActionData::Value("hello".into())),
        ),
    )
    .expect("a set-text request on a node advertising one must resolve");

    assert_eq!(
        seen.lock()
            .expect("this test never poisons its own lock")
            .as_slice(),
        ["hello"],
        "the handler must receive the payload the platform sent, not an empty string",
    );
}

/// A set-text request that arrives without its payload is dropped, not emptied.
///
/// `""` is a thing a platform can legitimately mean — clear the field — so
/// synthesizing one for a request that lost its payload would turn a failure to
/// route into a silent erasure of whatever the field held. The request itself
/// resolves: this is the payload being dropped, not the action being refused,
/// which is why the outcome is asserted positive as well.
#[test]
fn a_set_text_request_without_a_payload_is_dropped_rather_than_emptied() {
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);

    let (laid, _tree, node_id) = pump_labelled(
        Semantics::new()
            .container(true)
            .label(LABEL)
            .on_set_text(move |text| {
                sink.lock()
                    .expect("this test never poisons its own lock")
                    .push(text.to_owned());
            })
            .child(SizedBox::new(40.0, 20.0)),
    );

    // `SetValue` with no `data`: the action routes, the payload does not.
    invoke_semantics_action(
        &laid.pipeline_owner(),
        request(Action::SetValue, node_id, None),
    )
    .expect(
        "the request must resolve — the node advertises the action, so a \
         rejection here would mean this test never reached the payload",
    );

    assert!(
        seen.lock()
            .expect("this test never poisons its own lock")
            .is_empty(),
        "the handler must not run: an empty string is an edit the platform \
         never asked for",
    );
}

/// A scroll-to-offset request carries its offset through to the handler.
#[test]
fn a_scroll_to_offset_request_carries_its_offset_into_the_handler() {
    let seen: Arc<Mutex<Vec<(f64, f64)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);

    let (laid, tree, node_id) = pump_labelled(
        Semantics::new()
            .container(true)
            .label(LABEL)
            .on_scroll_to_offset(move |x, y| {
                sink.lock()
                    .expect("this test never poisons its own lock")
                    .push((x, y));
            })
            .child(SizedBox::new(40.0, 20.0)),
    );

    assert!(
        tree.find_by_label(LABEL)
            .expect("one node labelled")
            .supports_action(Action::SetScrollOffset),
        "an on_scroll_to_offset handler must be advertised as a scroll-offset \
         action. Tree was:\n{}",
        tree.describe()
    );

    invoke_semantics_action(
        &laid.pipeline_owner(),
        request(
            Action::SetScrollOffset,
            node_id,
            Some(ActionData::SetScrollOffset(A11yPoint { x: 3.0, y: -4.5 })),
        ),
    )
    .expect("a scroll request on a node advertising one must resolve");

    assert_eq!(
        seen.lock()
            .expect("this test never poisons its own lock")
            .as_slice(),
        [(3.0, -4.5)],
        "the handler must receive the offset the platform sent, including its \
         sign — a coordinate clamped or reordered in translation would show here",
    );
}

/// A scroll-to-offset request without its payload is dropped, not sent to the
/// origin.
///
/// The sibling of the set-text drop above, for the payload that is a
/// coordinate: `(0.0, 0.0)` is a position a platform can mean, so defaulting to
/// it would scroll the view somewhere the request never asked for.
#[test]
fn a_scroll_to_offset_request_without_a_payload_is_dropped_rather_than_origin() {
    let seen: Arc<Mutex<Vec<(f64, f64)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);

    let (laid, _tree, node_id) = pump_labelled(
        Semantics::new()
            .container(true)
            .label(LABEL)
            .on_scroll_to_offset(move |x, y| {
                sink.lock()
                    .expect("this test never poisons its own lock")
                    .push((x, y));
            })
            .child(SizedBox::new(40.0, 20.0)),
    );

    invoke_semantics_action(
        &laid.pipeline_owner(),
        request(Action::SetScrollOffset, node_id, None),
    )
    .expect(
        "the request must resolve — the node advertises the action, so a \
         rejection here would mean this test never reached the payload",
    );

    assert!(
        seen.lock()
            .expect("this test never poisons its own lock")
            .is_empty(),
        "the handler must not run: the origin is a scroll position the request \
         never asked for",
    );
}

/// The `on_action` escape hatch routes as well as the typed builders do.
///
/// Registration shape must not decide reachability. A caller who registers an
/// action the typed builders do not cover gets a control a screen reader can
/// press, and the handler sees the action it was registered under — the
/// argument a typed builder already knows and drops, and the caller here does
/// not.
#[test]
fn an_on_action_handler_round_trips_a_platform_click() {
    let seen: Arc<Mutex<Vec<(SemanticsAction, bool)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let handler: SemanticsActionHandler = Arc::new(move |action, arguments| {
        sink.lock()
            .expect("this test never poisons its own lock")
            .push((action, arguments.is_some()));
    });

    let (laid, tree, node_id) = pump_labelled(
        Semantics::new()
            .container(true)
            .label(LABEL)
            .on_action(SemanticsAction::Tap, Arc::clone(&handler))
            .child(SizedBox::new(40.0, 20.0)),
    );

    assert!(
        tree.find_by_label(LABEL)
            .expect("one node labelled")
            .supports_action(Action::Click),
        "a handler registered through on_action must be advertised like any \
         other, or the escape hatch reaches no assistive technology. Tree \
         was:\n{}",
        tree.describe()
    );

    invoke_semantics_action(
        &laid.pipeline_owner(),
        request(Action::Click, node_id, None),
    )
    .expect("a click on a node advertising one must resolve");

    assert_eq!(
        seen.lock()
            .expect("this test never poisons its own lock")
            .as_slice(),
        [(SemanticsAction::Tap, false)],
        "the handler must run once, with the action it was registered under and \
         no arguments, since a click carries none",
    );
}

/// Every FLUI `SemanticsAction` variant, listed once.
///
/// Two different checks make this list mean what it claims, and neither of them
/// is the array's length — that is a literal, and a literal accepts a duplicate
/// that quietly drops a variant.
///
/// Exhaustiveness is checked where the enum lives, in `flui-semantics`: a
/// variant added there must be named in a wildcard-free `match`, and the set
/// comparison that runs over `SemanticsAction::values()` fails for a variant
/// that reaches `values()` without reaching the sibling list there — so a new
/// action cannot be published by `values()` without being classified.
/// What *this* file owes is agreement with that published set, asserted by
/// [`the_actions_classified_here_are_exactly_the_ones_the_enum_publishes`].
const FLUI_ACTIONS: [flui_rendering::semantics::SemanticsAction; 24] = [
    SemanticsAction::Tap,
    SemanticsAction::LongPress,
    SemanticsAction::ScrollLeft,
    SemanticsAction::ScrollRight,
    SemanticsAction::ScrollUp,
    SemanticsAction::ScrollDown,
    SemanticsAction::Increase,
    SemanticsAction::Decrease,
    SemanticsAction::ShowOnScreen,
    SemanticsAction::MoveCursorForwardByCharacter,
    SemanticsAction::MoveCursorBackwardByCharacter,
    SemanticsAction::SetSelection,
    SemanticsAction::Copy,
    SemanticsAction::Cut,
    SemanticsAction::Paste,
    SemanticsAction::DidGainAccessibilityFocus,
    SemanticsAction::DidLoseAccessibilityFocus,
    SemanticsAction::CustomAction,
    SemanticsAction::Dismiss,
    SemanticsAction::MoveCursorForwardByWord,
    SemanticsAction::MoveCursorBackwardByWord,
    SemanticsAction::SetText,
    SemanticsAction::Focus,
    SemanticsAction::ScrollToOffset,
];

/// `FLUI_ACTIONS` classifies exactly the actions the enum publishes.
///
/// The drop-set assertion below can only ever see the actions listed here, so a
/// variant that joined the enum and `SemanticsAction::values()` without joining
/// this list would go unclassified while every other test in this file stayed
/// green — the shipped framework would route 25 actions and this file would
/// reason about 24. Compared as bitmasks rather than as slices: the order an
/// action is published in is not part of the contract, the set of actions is.
#[test]
fn the_actions_classified_here_are_exactly_the_ones_the_enum_publishes() {
    let classified: BTreeSet<u64> = FLUI_ACTIONS.iter().map(|action| action.value()).collect();
    let published: BTreeSet<u64> = SemanticsAction::values()
        .iter()
        .map(|action| action.value())
        .collect();

    assert_eq!(
        classified.len(),
        FLUI_ACTIONS.len(),
        "FLUI_ACTIONS lists one action twice under a pinned length, so the \
         action it displaced is classified nowhere",
    );
    assert_eq!(
        classified, published,
        "an action the enum publishes is missing from FLUI_ACTIONS, or one it \
         does not publish is listed: either way the drop-set assertion below is \
         reasoning about a set that is not the framework's",
    );
}

/// Every `accesskit::Action` variant, with FLUI's routing answer, written once.
///
/// The macro below emits **both** the enumerated `PLATFORM_ACTIONS` and the
/// wildcard-free `flui_routes` match from this single invocation, so a variant
/// cannot be classified without also joining the list. Two separate hand-written
/// artifacts could: the agreement and drop-set tests above reason over
/// `PLATFORM_ACTIONS`, while the compile-time gate is the match — so an arm
/// added for a new upstream variant classified `true` would join neither the
/// list nor either test's reasoning, and both would keep reasoning about the
/// old count while the shipped framework routed one more action.
///
/// The match is deliberately wildcard-free over a foreign enum.
/// `accesskit::Action` is not `#[non_exhaustive]`, so an upstream release that
/// adds a variant stops this matching function from compiling; the arm the
/// compiler then demands is written inside this same invocation, which is what
/// puts it in the list. That gate is the only one that exists — the translation
/// table's own `_ => None` arm would absorb a new platform action silently, and
/// a maintainer would never be prompted to decide whether FLUI should route it.
///
/// A variant listed twice is caught at compile time, not at run time: the
/// duplicate pattern makes the generated match emit `unreachable_patterns`,
/// which the workspace lints deny.
macro_rules! platform_actions {
    ($($variant:ident => $routes:literal),+ $(,)?) => {
        /// Every `accesskit::Action` variant, so the tests here have a complete
        /// denominator to reason over.
        const PLATFORM_ACTIONS: &[Action] = &[$(Action::$variant),+];

        /// Whether FLUI routes `action` back from the platform.
        fn flui_routes(action: Action) -> bool {
            match action {
                $(Action::$variant => $routes,)+
            }
        }
    };
}

platform_actions! {
    Click => true,
    ShowContextMenu => true,
    ScrollLeft => true,
    ScrollRight => true,
    ScrollUp => true,
    ScrollDown => true,
    Increment => true,
    Decrement => true,
    ScrollIntoView => true,
    SetTextSelection => true,
    SetValue => true,
    SetScrollOffset => true,
    Focus => true,
    Blur => true,
    CustomAction => true,
    Collapse => false,
    Expand => false,
    HideTooltip => false,
    ShowTooltip => false,
    ReplaceSelectedText => false,
    ScrollToPoint => false,
    SetSequentialFocusNavigationStartingPoint => false,
}

/// The set of FLUI actions the platform cannot reach is exactly the documented
/// one — and the one hole in it is asserted, not left to prose.
///
/// This is the "shipped seams never wired" guard. A builder that lets an author
/// register a handler nothing can ever invoke is a lying API, and the only way
/// to keep that from happening by accident is to name the unreachable set and
/// fail when it changes.
#[test]
fn the_actions_the_platform_cannot_reach_are_exactly_the_documented_drop_set() {
    let reachable: Vec<SemanticsAction> = FLUI_ACTIONS
        .iter()
        .copied()
        .filter(|action| {
            // Reachable if any platform action translates back to it, which is
            // the live table's answer rather than a restatement of it.
            PLATFORM_ACTIONS
                .iter()
                .copied()
                .any(|platform| semantics_action_for(platform) == Some(*action))
        })
        .collect();

    let unreachable: Vec<SemanticsAction> = FLUI_ACTIONS
        .iter()
        .copied()
        .filter(|action| !reachable.contains(action))
        .collect();

    // Eight are dropped on purpose — the four cursor moves and copy/cut/paste,
    // none of which the platform vocabulary FLUI targets exposes, plus Dismiss.
    // The ninth is the live defect: `DidGainAccessibilityFocus` is advertised
    // outbound (folded into the platform's Focus action) but nothing inbound
    // ever produces it, so a node registering only it advertises a control that
    // does nothing. Asserted here so the hole stays visible until it is closed.
    let expected = [
        SemanticsAction::MoveCursorForwardByCharacter,
        SemanticsAction::MoveCursorBackwardByCharacter,
        SemanticsAction::MoveCursorForwardByWord,
        SemanticsAction::MoveCursorBackwardByWord,
        SemanticsAction::Copy,
        SemanticsAction::Cut,
        SemanticsAction::Paste,
        SemanticsAction::Dismiss,
        SemanticsAction::DidGainAccessibilityFocus,
    ];

    assert_eq!(
        unreachable.len(),
        expected.len(),
        "the unreachable set changed size: {unreachable:?}"
    );
    for action in expected {
        assert!(
            unreachable.contains(&action),
            "{action:?} is no longer unreachable — if the translation table gained \
             a route for it, remove it from this list deliberately. Unreachable: {unreachable:?}"
        );
    }
}

/// The test's own `flui_routes` must agree with the production table.
///
/// Without this, `flui_routes` is a second, drifting copy of the translation
/// table — a test that reimplements the predicate is not a pin. The list it runs
/// over and the match answering it are expanded from one invocation above, so
/// this asserts a property of the production table rather than of two
/// hand-maintained copies of it.
#[test]
fn the_exhaustive_routing_list_agrees_with_the_translation_table() {
    for &action in PLATFORM_ACTIONS {
        assert_eq!(
            flui_routes(action),
            semantics_action_for(action).is_some(),
            "the exhaustive list and the production table disagree about {action:?}",
        );
    }
}
