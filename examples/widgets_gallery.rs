//! Widget gallery — composes the `flui-widgets` catalog into one screen through
//! the real pipeline. Its purpose is twofold: a runnable demo, and a proof that
//! the public authoring API reads cleanly (FLUI's C3 adoption metric — the
//! call site below is the thing an app author actually writes).
//!
//! ```text
//! Container → Column → { Text, Row of ColoredBox swatches, a centered card }
//!   → Element tree → render objects → layout → paint → LayerTree → wgpu
//! ```
//!
//! Run with: cargo run --example widgets_gallery

use flui_app::run_app;
use flui_widgets::prelude::*;
// `column!`/`row!` are imported explicitly (not via the prelude glob) to shadow
// std's same-named macros.
use flui_widgets::{column, row};

/// A solid colour square — a small reusable builder returning a concrete
/// `View` so it slots into the `row!` tuple sequence.
fn swatch(color: Color) -> ColoredBox {
    ColoredBox::new(color).child(SizedBox::square(64.0))
}

/// The gallery root: a dark padded surface with a title, a row of swatches, and
/// a centred "card".
#[derive(Clone, StatelessView)]
struct Gallery;

impl StatelessView for Gallery {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Container::new()
            .color(Color::rgb(18, 18, 24))
            .padding(EdgeInsets::all(px(24.0)))
            .alignment(Alignment::TOP_LEFT)
            .child(Column::new(column![
                Text::new("FLUI widget gallery"),
                SizedBox::height(16.0),
                Row::new(row![
                    swatch(Color::rgb(229, 57, 53)),
                    SizedBox::width(12.0),
                    swatch(Color::rgb(30, 136, 229)),
                    SizedBox::width(12.0),
                    swatch(Color::rgb(67, 160, 71)),
                ]),
                SizedBox::height(24.0),
                Container::new()
                    .color(Color::rgb(38, 38, 48))
                    .padding(EdgeInsets::all(px(16.0)))
                    .child(Center::new().child(Text::new("centered in a card"))),
            ]))
    }
}

fn main() {
    run_app(Gallery);
}
