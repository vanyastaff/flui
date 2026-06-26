//! Scroll-path parity test — the first coverage of the View→Element→mount seam
//! for a Box-viewport parent with a Sliver child (cross-protocol). Proves a
//! [`SingleChildScrollView`] lays its child out UNBOUNDED on the scroll axis
//! (so it can overflow) while the viewport itself sizes to its constraints.

mod common;

use common::{lay_out, size, tight};
use flui_types::Color;
use flui_view::ViewExt;
use flui_widgets::{ColoredBox, ListView, SingleChildScrollView, SizedBox};

#[test]
fn single_child_scroll_view_lays_child_out_unbounded_on_scroll_axis() {
    // Viewport bounded to 200×300; a 200×600 child is taller than the viewport.
    let laid = lay_out(
        SingleChildScrollView::new().child(SizedBox::new(200.0, 600.0)),
        tight(200.0, 300.0),
    );

    // The viewport (root box) sizes to its constraints — it does NOT grow to
    // the child; the overflow is what gets scrolled/clipped.
    let viewport = laid.root();
    assert_eq!(laid.size(viewport), size(200.0, 300.0));

    // Viewport → SliverToBoxAdapter (sliver) → the box child. The child keeps
    // its full 600 height: it was laid out with an unbounded main axis, the
    // essence of scrollability.
    let adapter = laid.only_child(viewport);
    let child = laid.only_child(adapter);
    assert_eq!(laid.size(child), size(200.0, 600.0));
}

#[test]
fn single_child_scroll_view_horizontal_lays_child_unbounded_on_width() {
    use flui_widgets::prelude::Axis;
    let laid = lay_out(
        SingleChildScrollView::new()
            .scroll_direction(Axis::Horizontal)
            .child(SizedBox::new(800.0, 100.0)),
        tight(300.0, 100.0),
    );
    let viewport = laid.root();
    assert_eq!(laid.size(viewport), size(300.0, 100.0));
    let child = laid.only_child(laid.only_child(viewport));
    assert_eq!(laid.size(child), size(800.0, 100.0));
}

#[test]
fn list_view_gives_each_row_the_fixed_item_extent() {
    // 4 rows at item_extent 50 → 200 total scroll extent in a 120-tall viewport.
    // Each childless ColoredBox fills its slot: viewport-wide × item_extent.
    let rows: Vec<_> = [
        Color::rgb(229, 57, 53),
        Color::rgb(30, 136, 229),
        Color::rgb(67, 160, 71),
        Color::rgb(255, 193, 7),
    ]
    .into_iter()
    .map(|c| ColoredBox::new(c).boxed())
    .collect();

    let laid = lay_out(ListView::new(50.0, rows), tight(200.0, 120.0));
    let viewport = laid.root();
    assert_eq!(laid.size(viewport), size(200.0, 120.0));

    // Viewport → SliverFixedExtentList → first row: forced to item_extent (50)
    // on the main axis, viewport-wide on the cross axis.
    let list = laid.only_child(viewport);
    let first_row = laid.child(list, 0);
    assert_eq!(laid.size(first_row), size(200.0, 50.0));
}

#[test]
fn sliver_padding_insets_its_sliver_child() {
    use flui_widgets::{SliverPadding, SliverToBoxAdapter, Viewport};
    // Viewport → SliverPadding(10) → SliverToBoxAdapter → box: the padding's
    // 10-per-side cross inset shrinks the box's cross axis to 200-20 = 180.
    let laid = lay_out(
        Viewport::new((SliverPadding::all(10.0)
            .child(SliverToBoxAdapter::new().child(SizedBox::new(180.0, 100.0))),)),
        tight(200.0, 300.0),
    );
    let viewport = laid.root();
    assert_eq!(laid.size(viewport), size(200.0, 300.0));

    let padding = laid.only_child(viewport);
    let adapter = laid.only_child(padding);
    let box_child = laid.only_child(adapter);
    assert_eq!(laid.size(box_child), size(180.0, 100.0));
}
