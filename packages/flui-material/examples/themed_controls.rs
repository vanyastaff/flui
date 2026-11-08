//! Mount Material controls with a typed component theme and print the measured result.
//!
//! Run with `cargo run -p flui-material --example themed_controls`.

use flui_material::{
    ButtonStyle, Checkbox, ElevatedButton, ElevatedButtonThemeData, Theme, ThemeData,
};
use flui_sdk::painting::styling::Color;
use flui_sdk::testing::widgets::{lay_out, loose};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{Column, Text, WidgetStateProperty};

const COMPONENT_SUMMARY: &str = "elevated_button_theme=present";

#[derive(Clone, Debug, StatelessView)]
struct ThemedControls;

impl StatelessView for ThemedControls {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let component = Theme::component_of::<ElevatedButtonThemeData>(ctx);
        let has_background = component
            .and_then(|theme| theme.style)
            .and_then(|style| style.background_color)
            .is_some();
        let summary = if has_background {
            COMPONENT_SUMMARY
        } else {
            "elevated_button_theme=missing"
        };

        Column::new(vec![
            Text::new(summary).boxed(),
            ElevatedButton::new(Text::new("Save"))
                .on_pressed(|_cx| {})
                .boxed(),
            Checkbox::new(true).boxed(),
        ])
    }
}

fn main() {
    let button_theme = ElevatedButtonThemeData {
        style: Some(ButtonStyle {
            background_color: Some(WidgetStateProperty::all(Some(Color::rgb(0x67, 0x50, 0xA4)))),
            ..ButtonStyle::default()
        }),
    };
    let theme = ThemeData::light().with_component(button_theme);
    let laid = lay_out(Theme::new(theme, ThemedControls), loose(320.0));

    let summary = laid
        .find_text(COMPONENT_SUMMARY)
        .expect("BUG: the typed component theme was read during build");
    let button = laid
        .find_text("Save")
        .expect("BUG: the themed ElevatedButton mounted its label");
    let summary_size = laid.size(summary);
    let button_size = laid.size(button);

    println!(
        "Material: {COMPONENT_SUMMARY}; controls=ElevatedButton+Checkbox; render_nodes={}; summary={:.0}x{:.0}; button_label={:.0}x{:.0}",
        laid.render_node_count(),
        summary_size.width,
        summary_size.height,
        button_size.width,
        button_size.height,
    );
}
