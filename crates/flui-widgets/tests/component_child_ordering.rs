//! Regression: a multi-child render parent (`Row`) keeps its render children in
//! element-slot order even when a child is a COMPONENT element (a
//! `StatelessView`) whose render descendant is built in a *later* `build_scope`
//! iteration than a render sibling that already attached.
//!
//! This generalizes the `Expanded`/`Positioned` parent-data coverage: the
//! slot-correct ordering fix (`reorder_render_children_after_build`) is keyed on
//! "component child of a multi-child render", not on parent-data specifically.
//! Without the fix the directly-rendered `SizedBox` (slot 1) would attach first
//! and land at render-index 0, inverting the layout.

mod common;

use common::{lay_out, offset, size, tight};
use flui_widgets::prelude::{BuildContext, IntoView, StatelessView};
use flui_widgets::row;
use flui_widgets::{Row, SizedBox};

/// A minimal `StatelessView` (component element — no render object of its own)
/// that builds to a fixed-size box one level down.
#[derive(Clone, Debug, StatelessView)]
struct BoxBuilder {
    width: f32,
    height: f32,
}

impl StatelessView for BoxBuilder {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(self.width, self.height)
    }
}

#[test]
fn component_child_keeps_slot_order_before_a_render_sibling() {
    // Row 300 wide: a component child FIRST (builds its box in a later
    // iteration), a directly-rendered box SECOND. Slot order must win.
    let laid = lay_out(
        Row::new(row![
            BoxBuilder {
                width: 50.0,
                height: 30.0,
            },
            SizedBox::new(70.0, 30.0),
        ]),
        tight(300.0, 30.0),
    );

    let root = laid.root();

    let first = laid.child(root, 0);
    let second = laid.child(root, 1);

    // child(root, 0) must be the COMPONENT's box (50 wide) at the left edge —
    // not the directly-rendered SizedBox that attached first.
    assert_eq!(laid.size(first), size(50.0, 30.0));
    assert_eq!(laid.offset(first), offset(0.0, 0.0));

    // child(root, 1) is the directly-rendered SizedBox, following at x = 50.
    assert_eq!(laid.size(second), size(70.0, 30.0));
    assert_eq!(laid.offset(second), offset(50.0, 0.0));
}
