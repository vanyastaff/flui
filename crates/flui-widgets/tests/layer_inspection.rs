//! The headless harness can report what a frame actually **composited**, not
//! just what it built.
//!
//! Layers are produced by *paint*, so they answer questions the render tree
//! structurally cannot: whether a widget forced a clip, a transform, or an
//! opacity layer into the output, and how many. Upstream's widget tests lean on
//! this constantly (`tester.layers`, and the `getLayers()` container-chain walk
//! `fitted_box_test.dart` defines locally); five files in
//! `tests/parity/` currently list cases as out of scope naming the absence of
//! exactly this capability.
//!
//! The pipeline always produced the tree — `HeadlessBinding::pump_frame` simply
//! dropped it on the floor, because headlessly there is no compositor to hand it
//! to. These tests pin that it is kept and reachable.

use std::time::Duration;

use crate::common::{lay_out, tight};
use flui_geometry::Matrix4;
use flui_types::Color;
use flui_widgets::{ColoredBox, Opacity, SizedBox, Transform};

/// The baseline: a pumped frame reports the layers it composited.
///
/// Asserts the *content*, not merely that the vec is non-empty — a stub
/// returning `vec!["Picture"]` would pass an is-empty check. A painted tree
/// bottoms out in a `Picture` leaf beneath an `Offset` root, and both must be
/// named.
///
/// Red-check: revert `pump_frame` to discarding `run_pipeline`'s return value.
/// `layer_kinds` then reports an empty vec and both assertions fail.
#[test]
fn a_pumped_frame_reports_the_layers_it_composited() {
    let mut laid = lay_out(
        SizedBox::new(50.0, 50.0).child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(200.0, 200.0),
    );
    laid.pump();

    let kinds = laid.layer_kinds();
    assert!(
        kinds.contains(&"Picture"),
        "a painted tree must composite a Picture leaf; got {kinds:?}"
    );
    assert!(
        kinds.contains(&"Offset"),
        "the composited root must be an Offset layer; got {kinds:?}"
    );
}

/// Layers reflect **paint**, so a widget that forces one into the output is
/// visible here and nowhere else in the harness.
///
/// `Opacity` is the clearest case: it adds no render-tree geometry a
/// `find_all_by_render_type` sweep could distinguish from its child alone, but
/// it must composite an `Opacity` layer. This is the capability's whole reason
/// for existing — asserting on composited output rather than built structure.
#[test]
fn a_widget_that_forces_a_compositing_layer_shows_up_in_the_composited_output() {
    let mut without = lay_out(
        SizedBox::new(50.0, 50.0).child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(200.0, 200.0),
    );
    without.pump();
    assert!(
        !without.layer_kinds().contains(&"Opacity"),
        "the control tree must not composite an Opacity layer; got {:?}",
        without.layer_kinds()
    );

    let mut with = lay_out(
        Opacity::new(0.5)
            .child(SizedBox::new(50.0, 50.0).child(ColoredBox::new(Color::rgb(10, 20, 30)))),
        tight(200.0, 200.0),
    );
    with.pump();
    assert!(
        with.layer_kinds().contains(&"Opacity"),
        "a half-opaque subtree must composite an Opacity layer; got {:?}",
        with.layer_kinds()
    );
}

/// The structural form: [`LaidOut::layer_tree`] exposes parent/child shape, not
/// just a flat list.
///
/// Upstream's `fitted_box_test.dart` needs exactly this — its local
/// `getLayers()` walks a single-child *container chain* from the root and
/// asserts `firstChild == lastChild` at every step, which a flattened list
/// cannot express. This pins that the shape is reachable, so that helper is
/// portable when the features it exercises land.
#[test]
fn the_composited_tree_exposes_parent_child_shape_not_just_a_flat_list() {
    let mut laid = lay_out(
        Transform::new(Matrix4::scaling(2.0, 2.0, 1.0))
            .child(SizedBox::new(50.0, 50.0).child(ColoredBox::new(Color::rgb(10, 20, 30)))),
        tight(200.0, 200.0),
    );
    laid.pump();

    let tree = laid.layer_tree().expect("a pumped frame composites a tree");
    let root = tree.root().expect("the composited tree has a root");
    assert!(
        tree.len() > 1,
        "this tree composites more than the root alone; got {} layer(s)",
        tree.len()
    );
    assert_eq!(
        tree.parent(root),
        None,
        "the root layer must have no parent"
    );

    let children = tree
        .children(root)
        .expect("the root of a multi-layer tree has children");
    assert!(
        !children.is_empty(),
        "the root must actually parent the layers below it — a flat list of the \
         right length would not prove the tree is linked"
    );
    for &child in children {
        assert_eq!(
            tree.parent(child),
            Some(root),
            "every child's parent link must point back at the root"
        );
    }
}

/// Composited output is the *last frame's* output, not a cached
/// last-known-good tree.
///
/// A frame over a tree with nothing dirty repaints nothing and therefore
/// composites nothing. Reporting a stale tree there would be the more
/// convenient lie — and would silently answer "yes, this widget composited a
/// clip" about a frame that never ran. Pinning the honest behavior keeps a
/// future caller from building on the convenient one.
#[test]
fn a_frame_that_repaints_nothing_composites_nothing() {
    let mut laid = lay_out(
        SizedBox::new(50.0, 50.0).child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(200.0, 200.0),
    );
    laid.pump();
    assert!(
        !laid.layer_kinds().is_empty(),
        "the dirty frame must composite something to make this test meaningful"
    );

    // `pump_for` advances virtual time without dirtying the tree.
    laid.pump_for(Duration::from_millis(16));
    assert!(
        laid.layer_kinds().is_empty(),
        "a frame that repainted nothing must report no composited layers rather \
         than the previous frame's; got {:?}",
        laid.layer_kinds()
    );
}
