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
