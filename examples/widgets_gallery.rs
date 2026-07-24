//! Widget gallery — composes the `flui-widgets` catalog into one screen through
//! the real pipeline. Its purpose is twofold: a runnable demo, and a proof that
//! the public authoring API reads cleanly (FLUI's C3 adoption metric — the
//! call site below is the thing an app author actually writes).
//!
//! ```text
//! Container → Column → { Text, a Row of circular avatars, a faded Row,
//!   a centered card } → Element tree → render objects → layout → paint
//!   → LayerTree → wgpu
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

/// A circular colour avatar: a coloured box clipped to an inscribed oval.
fn avatar(color: Color) -> ClipOval {
    ClipOval::new().child(swatch(color))
}

/// A half-faded circular avatar, showing the `Opacity` paint effect.
fn faded_avatar(color: Color) -> Opacity {
    Opacity::new(0.4).child(avatar(color))
}

/// The gallery root: a dark padded surface with a title, a row of circular
/// avatars, a faded row, and a centred "card".
#[derive(Clone, Debug, StatelessView)]
pub struct Gallery;

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
                    avatar(Color::rgb(229, 57, 53)),
                    SizedBox::width(12.0),
                    avatar(Color::rgb(30, 136, 229)),
                    SizedBox::width(12.0),
                    avatar(Color::rgb(67, 160, 71)),
                ]),
                SizedBox::height(12.0),
                Row::new(row![
                    faded_avatar(Color::rgb(229, 57, 53)),
                    SizedBox::width(12.0),
                    faded_avatar(Color::rgb(30, 136, 229)),
                    SizedBox::width(12.0),
                    faded_avatar(Color::rgb(67, 160, 71)),
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
