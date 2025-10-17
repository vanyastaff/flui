//! Text widget demonstration
//!
//! This example shows various usage patterns of the Text widget,
//! similar to Flutter's Text widget functionality.

use eframe::egui;
use nebula_ui::widgets::primitives::Text;
use nebula_ui::types::typography::{TextStyle, TextAlign, TextOverflow, TextScaler};
use nebula_ui::types::core::Color;
use nebula_ui::prelude::*;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Text Widget Demo",
        options,
        Box::new(|_cc| Ok(Box::new(TextDemoApp::default()))),
    )
}

struct TextDemoApp {
    scale_factor: f32,
}

impl Default for TextDemoApp {
    fn default() -> Self {
        Self {
            scale_factor: 1.0,
        }
    }
}

impl eframe::App for TextDemoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Text Widget Demo - Flutter Style");

            ui.add_space(10.0);

            // Simple text
            ui.label("1. Simple Text");
            Text::new("Hello World").ui(ui);

            ui.add_space(10.0);

            // Styled text with headline1
            ui.label("2. Headline 1 Style");
            Text::builder()
                .data("Large Headline Text")
                .style(TextStyle::headline1())
                .ui(ui);

            ui.add_space(10.0);

            // Colored text
            ui.label("3. Colored Text");
            Text::builder()
                .data("This text is blue!")
                .style(TextStyle::body().with_color(Color::BLUE))
                .ui(ui);

            ui.add_space(10.0);

            // Centered text
            ui.label("4. Centered Text");
            Text::builder()
                .data("I am centered!")
                .text_align(TextAlign::Center)
                .style(TextStyle::headline2())
                .ui(ui);

            ui.add_space(10.0);

            // Text with max lines and ellipsis
            ui.label("5. Text with Max Lines (2) and Ellipsis");
            Text::builder()
                .data("This is a very long text that should wrap to multiple lines and then be truncated with an ellipsis because we set max_lines to 2. You shouldn't see this part of the text.")
                .max_lines(2)
                .overflow(TextOverflow::Ellipsis)
                .ui(ui);

            ui.add_space(10.0);

            // Non-wrapping text
            ui.label("6. Non-Wrapping Text (single line)");
            Text::builder()
                .data("This text will not wrap no matter how long it is - it stays on one line")
                .soft_wrap(false)
                .overflow(TextOverflow::Ellipsis)
                .ui(ui);

            ui.add_space(10.0);

            // Different text styles
            ui.label("7. Different Text Styles");
            ui.horizontal(|ui| {
                Text::builder()
                    .data("Body ")
                    .style(TextStyle::body())
                    .ui(ui);
                Text::builder()
                    .data("Button ")
                    .style(TextStyle::button())
                    .ui(ui);
                Text::builder()
                    .data("Caption ")
                    .style(TextStyle::caption())
                    .ui(ui);
                Text::builder()
                    .data("Code")
                    .style(TextStyle::code())
                    .ui(ui);
            });

            ui.add_space(10.0);

            // Text scaler
            ui.label("8. Text Scaler");
            ui.horizontal(|ui| {
                ui.label("Scale factor:");
                ui.add(egui::Slider::new(&mut self.scale_factor, 0.5..=3.0).text(""));
            });

            Text::builder()
                .data("This text scales with the slider above")
                .text_scaler(TextScaler::new(self.scale_factor))
                .style(TextStyle::headline3())
                .ui(ui);

            ui.add_space(10.0);

            // Bold and italic (using TextStyle builders)
            ui.label("9. Bold and Italic Text");
            ui.horizontal(|ui| {
                Text::builder()
                    .data("Bold")
                    .style(TextStyle::body().bold())
                    .ui(ui);
                ui.label(" ");
                Text::builder()
                    .data("Italic")
                    .style(TextStyle::body().italic())
                    .ui(ui);
                ui.label(" ");
                Text::builder()
                    .data("Bold Italic")
                    .style(TextStyle::body().bold().italic())
                    .ui(ui);
            });

            ui.add_space(10.0);

            // Different alignments
            ui.label("10. Text Alignments");
            ui.group(|ui| {
                ui.set_width(ui.available_width());

                Text::builder()
                    .data("Left Aligned")
                    .text_align(TextAlign::Left)
                    .ui(ui);

                Text::builder()
                    .data("Center Aligned")
                    .text_align(TextAlign::Center)
                    .ui(ui);

                Text::builder()
                    .data("Right Aligned")
                    .text_align(TextAlign::Right)
                    .ui(ui);
            });

            ui.add_space(10.0);

            // Multiple colors
            ui.label("11. Rainbow Text");
            ui.horizontal(|ui| {
                for (text, color) in [
                    ("Red", Color::RED),
                    ("Orange", Color::from_rgb(255, 165, 0)),
                    ("Yellow", Color::YELLOW),
                    ("Green", Color::GREEN),
                    ("Blue", Color::BLUE),
                ] {
                    Text::builder()
                        .data(text)
                        .style(TextStyle::body().with_color(color))
                        .ui(ui);
                    ui.label(" ");
                }
            });

            ui.add_space(20.0);

            ui.separator();
            ui.label("Flutter-style Text widget for egui - nebula-ui");
        });
    }
}
