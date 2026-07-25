//! `MouseRegion` widget coverage over `RenderMouseRegion`.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use crate::common::{lay_out, loose, size, tight};
use flui_widgets::{MouseRegion, SizedBox};

#[test]
fn mouse_region_childless_fills_parent_and_mounts_render_object() {
    let laid = lay_out(MouseRegion::new(), tight(80.0, 40.0));

    let root = laid.root();
    assert_eq!(laid.find_by_render_type("RenderMouseRegion"), root);
    assert_eq!(
        laid.size(root),
        size(80.0, 40.0),
        "childless MouseRegion must grow to the incoming biggest constraints",
    );
}

#[test]
fn mouse_region_with_child_sizes_to_child() {
    let laid = lay_out(
        MouseRegion::new().child(SizedBox::new(30.0, 20.0)),
        loose(80.0),
    );

    assert_eq!(laid.size(laid.root()), size(30.0, 20.0));
}

#[test]
fn mouse_region_hover_callback_fires_on_hover_move() {
    let hovers = Arc::new(AtomicUsize::new(0));
    let in_callback = Arc::clone(&hovers);
    let laid = lay_out(
        MouseRegion::new()
            .on_hover(move |_device, _position| {
                in_callback.fetch_add(1, Ordering::SeqCst);
            })
            .child(SizedBox::new(60.0, 30.0)),
        tight(60.0, 30.0),
    );

    laid.dispatch_pointer_hover(10.0, 10.0);
    assert_eq!(
        hovers.load(Ordering::SeqCst),
        1,
        "MouseRegion::on_hover must route through RenderMouseRegion's hit entry",
    );
}
