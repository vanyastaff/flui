//! `Positioned` parent-data parity — a `Positioned` child contributes a
//! `StackParentData` (edge insets + explicit size) that `RenderStack` reads to
//! place and stretch the child. Proves the ParentDataView seam for the Stack
//! family end-to-end (offsets and resolved sizes).

use crate::common::{lay_out, offset, size, tight};
use flui_widgets::row;
use flui_widgets::{Positioned, SizedBox, Stack};

pub(crate) fn positioned_places_child_at_explicit_edges() {
    // A 200×200 stack with a 50×50 child pinned 10 from the left, 20 from top.
    let laid = lay_out(
        Stack::new(row![
            Positioned::new(SizedBox::new(50.0, 50.0))
                .left(10.0)
                .top(20.0),
        ]),
        tight(200.0, 200.0),
    );

    let root = laid.root();
    assert_eq!(laid.size(root), size(200.0, 200.0));

    let child = laid.only_child(root);
    assert_eq!(laid.size(child), size(50.0, 50.0));
    assert_eq!(laid.offset(child), offset(10.0, 20.0));
}

// ============================================================================
// Stack itself: non-positioned children -- sizing to the largest, and
// alignment. Every test above uses `Positioned` children exclusively;
// `Stack`'s own default (`Alignment::TOP_LEFT`, `StackFit::Loose`) and
// explicit-alignment/fit behavior with plain non-positioned children were
// never directly exercised.
// ============================================================================

/// An indexed stack is heard only through the child it shows, and switching the index
/// switches what is heard.
pub(crate) fn an_indexed_stack_is_heard_only_through_its_shown_child() {
    use flui_view::ViewExt;
    use flui_widgets::{IndexedStack, Semantics};
    let stack = |index| {
        IndexedStack::new(vec![
            Semantics::new()
                .label("first")
                .child(SizedBox::new(20.0, 20.0))
                .boxed(),
            Semantics::new()
                .label("second")
                .child(SizedBox::new(20.0, 20.0))
                .boxed(),
        ])
        .index(Some(index))
    };
    let mut laid = lay_out(stack(0), tight(100.0, 100.0));
    laid.enable_semantics();
    laid.tick();
    let tree = laid.a11y_tree().expect("semantics enabled");
    assert!(tree.find_by_label("first").is_ok(), "the shown child");
    assert!(
        tree.find_by_label("second").is_err(),
        "the hidden child is not heard"
    );

    laid.pump_widget(stack(1));
    laid.tick();
    let tree = laid.a11y_tree().expect("semantics enabled");
    assert!(
        tree.find_by_label("second").is_ok(),
        "the newly shown child"
    );
    assert!(
        tree.find_by_label("first").is_err(),
        "the newly hidden child"
    );
}
