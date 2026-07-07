//! `CustomPaint` widget smoke coverage over `RenderCustomPaint`.

use crate::common::{lay_out, loose, size};
use flui_widgets::{CustomPaint, SizedBox};

#[test]
fn custom_paint_childless_uses_preferred_size() {
    let laid = lay_out(CustomPaint::new().size(size(30.0, 20.0)), loose(200.0));

    let root = laid.root();
    assert_eq!(laid.find_by_render_type("RenderCustomPaint"), root);
    assert_eq!(laid.size(root), size(30.0, 20.0));
}

#[test]
fn custom_paint_with_child_sizes_to_child() {
    let laid = lay_out(
        CustomPaint::new()
            .size(size(90.0, 80.0))
            .child(SizedBox::new(40.0, 10.0)),
        loose(200.0),
    );

    let root = laid.root();
    assert_eq!(laid.size(root), size(40.0, 10.0));
    assert_eq!(laid.size(laid.only_child(root)), size(40.0, 10.0));
}
