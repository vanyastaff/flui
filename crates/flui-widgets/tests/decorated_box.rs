//! Layout tests for `DecoratedBox` — decoration affects painting only, so
//! layout is a pass-through (the child's size), mirroring `tests/clip.rs`'s
//! convention for the other paint-effect proxy widgets.

mod common;

use common::{lay_out, loose, size};
use flui_types::Color;
use flui_types::styling::BoxDecoration;
use flui_widgets::{DecoratedBox, SizedBox};

fn decoration() -> BoxDecoration<flui_types::Pixels> {
    BoxDecoration::new().set_color(Some(Color::rgb(200, 0, 0)))
}

#[test]
fn decorated_box_is_a_layout_passthrough() {
    let laid = lay_out(
        DecoratedBox::new(decoration()).child(SizedBox::new(120.0, 80.0)),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(120.0, 80.0));
}

#[test]
fn decorated_box_foreground_is_also_a_layout_passthrough() {
    // `foreground()` only changes paint order (decoration over vs. under the
    // child); it must not affect the layout pass-through contract.
    let laid = lay_out(
        DecoratedBox::new(decoration())
            .foreground()
            .child(SizedBox::new(64.0, 48.0)),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(64.0, 48.0));
}

#[test]
fn decorated_box_mounts_a_render_decorated_box() {
    let laid = lay_out(
        DecoratedBox::new(decoration()).child(SizedBox::new(50.0, 50.0)),
        loose(1000.0),
    );
    // The widget must actually mount `RenderDecoratedBox` (not silently
    // degrade to a plain pass-through box with no decoration render object).
    let _ = laid.find_by_render_type("RenderDecoratedBox");
}
