//! The headless harness can report what a frame actually **composited**, not
//! just what it built.
//!
//! Layers are produced by *paint*, so they answer questions the render tree
//! structurally cannot: whether a widget forced a clip, a transform, or an
//! opacity layer into the output, and how many. Upstream's widget tests lean on
//! this constantly (`tester.layers`, and the `getLayers()` container-chain walk
//! `fitted_box_test.dart` defines locally).
//!
//! The pipeline always produced the tree — `HeadlessBinding::pump_frame` simply
//! dropped it on the floor, because headlessly there is no compositor to hand it
//! to. These tests pin that it is kept and reachable.

use crate::common::{lay_out, tight};
use flui_foundation::geometry::Matrix4;
use flui_painting::styling::Color;
use flui_widgets::{ColoredBox, SizedBox, Transform};

/// The structural form: [`LaidOut::layer_tree`] exposes parent/child shape, not
/// just a flat list.
///
/// Upstream's `fitted_box_test.dart` needs exactly this — its local
/// `getLayers()` walks a single-child *container chain* from the root and
/// asserts `firstChild == lastChild` at every step, which a flattened list
/// cannot express. This pins that the shape is reachable, so that helper is
/// portable when the features it exercises land.
pub(crate) fn the_composited_tree_exposes_parent_child_shape_not_just_a_flat_list() {
    let mut laid = lay_out(
        Transform::new(Matrix4::scaling(2.0, 2.0, 1.0))
            .child(SizedBox::new(50.0, 50.0).child(ColoredBox::new(Color::rgb(10, 20, 30)))),
        tight(200.0, 200.0),
    );
    laid.pump();

    let tree = laid.layer_tree().expect("a pumped frame composites a tree");
    let root = tree.root();
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
