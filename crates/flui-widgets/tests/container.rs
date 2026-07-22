//! Composition parity tests for [`Container`] — assert that its
//! `StatelessView::build` composes the right widget stack so padding,
//! sizing, and alignment combine exactly as Flutter's `Container` does.

mod common;

use common::{lay_out, loose, offset, size};
use flui_geometry::EdgeInsets;
use flui_types::Alignment;
use flui_widgets::{Container, SizedBox};

#[test]
fn container_padding_shrink_wraps_child() {
    // Padding(10) around a 50×50 child, no forced size → 70×70.
    let laid = lay_out(
        Container::new()
            .padding(EdgeInsets::all(flui_geometry::px(10.0)))
            .child(SizedBox::square(50.0)),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(70.0, 70.0));
}

#[test]
fn container_width_height_force_size_regardless_of_child() {
    let laid = lay_out(
        Container::new()
            .width(200.0)
            .height(120.0)
            .child(SizedBox::square(10.0)),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(200.0, 120.0));
}

#[test]
fn container_aligns_child_within_forced_size() {
    // ConstrainedBox(tight 100) → Align(center) → SizedBox(20): child centered.
    let laid = lay_out(
        Container::new()
            .width(100.0)
            .height(100.0)
            .alignment(Alignment::CENTER)
            .child(SizedBox::square(20.0)),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(100.0, 100.0));

    let align = laid.only_child(laid.root());
    let inner = laid.only_child(align);
    assert_eq!(laid.size(inner), size(20.0, 20.0));
    // Centered in 100×100: (100-20)/2 = 40 on each axis.
    assert_eq!(laid.offset(inner), offset(40.0, 40.0));
}

#[test]
fn container_childless_with_size_fills_to_size() {
    // No child + forced size: the childless placeholder is pinned by the
    // ConstrainedBox layer to the requested size.
    let laid = lay_out(Container::new().width(80.0).height(40.0), loose(1000.0));
    assert_eq!(laid.size(laid.root()), size(80.0, 40.0));
}
