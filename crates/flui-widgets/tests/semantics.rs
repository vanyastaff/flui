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
