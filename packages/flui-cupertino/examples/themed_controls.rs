//! Mount Cupertino controls under an explicit theme and print the measured result.
//!
//! Run with `cargo run -p flui-cupertino --example themed_controls`.

use flui_cupertino::{
    CupertinoActivityIndicator, CupertinoButton, CupertinoColor, CupertinoTheme, CupertinoThemeData,
};
use flui_sdk::painting::styling::Color;
use flui_sdk::platform::Brightness;
use flui_sdk::testing::widgets::{lay_out, loose};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{Column, Text};

const THEME_SUMMARY: &str = "theme=Dark; primary_resolved=true";

#[derive(Clone, Debug, StatelessView)]
struct ThemedControls;

impl StatelessView for ThemedControls {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = CupertinoTheme::of(ctx);
        let brightness = CupertinoTheme::brightness_of(ctx);
        let primary_resolved = matches!(theme.primary_color(), CupertinoColor::Static(_));
        let summary = format!("theme={brightness:?}; primary_resolved={primary_resolved}");

        Column::new(vec![
            Text::new(summary).boxed(),
            CupertinoButton::filled(Text::new("Continue"))
                .on_pressed(|_cx| {})
                .pressed_opacity(None)
                .boxed(),
            CupertinoActivityIndicator::new().boxed(),
        ])
    }
}

fn main() {
    let theme = CupertinoThemeData::new()
        .with_brightness(Brightness::Dark)
        .with_primary_color(Color::rgb(0x0A, 0x84, 0xFF));
    let laid = lay_out(CupertinoTheme::new(theme, ThemedControls), loose(320.0));

    let summary = laid
        .find_text(THEME_SUMMARY)
        .expect("BUG: CupertinoTheme resolved its primary color during build");
    let button = laid
        .find_text("Continue")
        .expect("BUG: the themed CupertinoButton mounted its label");
    let summary_size = laid.size(summary);
    let button_size = laid.size(button);

    println!(
        "Cupertino: {THEME_SUMMARY}; controls=CupertinoButton+CupertinoActivityIndicator; render_nodes={}; summary={:.0}x{:.0}; button_label={:.0}x{:.0}",
        laid.render_node_count(),
        summary_size.width,
        summary_size.height,
        button_size.width,
        button_size.height,
    );
}
