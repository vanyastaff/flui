//! `CustomPaint` widget smoke coverage over `RenderCustomPaint`.

mod common;

use common::{lay_out, loose, size};
use flui_widgets::{CustomPaint, SizedBox};

/// The true default (`CustomPaint::new()`, no `.size()` call and no child)
/// lays out to `Size::ZERO` — `Size`'s `Default` and `ZERO` are the same
/// value, but this proves it through an actual layout pass rather than just
/// reading the constructed field back.
///
/// Flutter parity: `custom_paint_test.dart` "CustomPaint sizing" (3.44.0) —
/// `Center(child: CustomPaint(key: target))` measures to `Size.zero`.
#[test]
fn custom_paint_childless_default_size_is_zero() {
    let laid = lay_out(CustomPaint::new(), loose(200.0));
    assert_eq!(laid.size(laid.root()), size(0.0, 0.0));
}

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
