//! Layout tests for `DecoratedBox` — decoration affects painting only, so
//! layout is a pass-through (the child's size), mirroring `tests/clip.rs`'s
//! convention for the other paint-effect proxy widgets.

use crate::common::{lay_out, loose, size};
use flui_painting::styling::BoxDecoration;
use flui_painting::styling::Color;
use flui_widgets::{DecoratedBox, SizedBox};

fn decoration() -> BoxDecoration<f64> {
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
