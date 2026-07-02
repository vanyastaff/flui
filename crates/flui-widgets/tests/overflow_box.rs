//! `OverflowBox`/`SizedOverflowBox` -- alignment must survive a widget
//! rebuild, not just the initial mount.
//!
//! Regression test for a real bug: `RenderConstrainedOverflowBox`/
//! `RenderSizedOverflowBox` had no `set_alignment`, so
//! `update_render_object` could only thread the width/height overrides
//! (and requested size) through on rebuild -- a widget instance built with
//! a NEW `alignment` value silently kept its FIRST alignment forever.

mod common;

use common::{lay_out, loose, offset, size};
use flui_types::Alignment;
use flui_widgets::{OverflowBox, SizedBox, SizedOverflowBox};

#[test]
fn overflow_box_alignment_change_repositions_the_child_on_rebuild() {
    // Loose (not tight) so the unconstrained child can report its own
    // 50x50 size instead of being clamped to fill the parent.
    let mut laid = lay_out(
        OverflowBox::new().child(SizedBox::new(50.0, 50.0)),
        loose(200.0),
    );

    let root = laid.root();
    let child = laid.only_child(root);
    // Default Alignment::CENTER: (200 - 50) / 2 = 75 on both axes.
    assert_eq!(laid.offset(child), offset(75.0, 75.0));

    laid.pump_widget(
        OverflowBox::new()
            .with_alignment(Alignment::TOP_LEFT)
            .child(SizedBox::new(50.0, 50.0)),
    );

    assert_eq!(
        laid.offset(child),
        offset(0.0, 0.0),
        "a rebuilt OverflowBox with a new alignment must reposition its child, \
         not silently keep the alignment from the first build",
    );
}

#[test]
fn sized_overflow_box_alignment_change_repositions_the_child_on_rebuild() {
    // Loose so requested_size (100x100) is reported unclamped, and the
    // unconstrained child can report its own 50x50 size.
    let mut laid = lay_out(
        SizedOverflowBox::new(size(100.0, 100.0)).child(SizedBox::new(50.0, 50.0)),
        loose(200.0),
    );

    let root = laid.root();
    assert_eq!(laid.size(root), size(100.0, 100.0));
    let child = laid.only_child(root);
    // Default Alignment::CENTER within the claimed 100x100 slot: (100-50)/2 = 25.
    assert_eq!(laid.offset(child), offset(25.0, 25.0));

    laid.pump_widget(
        SizedOverflowBox::new(size(100.0, 100.0))
            .with_alignment(Alignment::BOTTOM_RIGHT)
            .child(SizedBox::new(50.0, 50.0)),
    );

    assert_eq!(
        laid.offset(child),
        offset(50.0, 50.0),
        "a rebuilt SizedOverflowBox with a new alignment must reposition its \
         child, not silently keep the alignment from the first build",
    );
}
