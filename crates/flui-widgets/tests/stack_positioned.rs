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
