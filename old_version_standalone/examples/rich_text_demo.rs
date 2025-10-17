//! Rich text demonstration using TextSpan
//!
//! This example shows how to create text with multiple styles using TextSpan,
//! similar to Flutter's RichText widget.

use eframe::egui;
use nebula_ui::widgets::primitives::Text;
use nebula_ui::types::typography::{TextSpan, TextStyle, TextAlign};
use nebula_ui::types::core::Color;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Rich Text Demo",
        options,
        Box::new(|_cc| Ok(Box::new(RichTextDemoApp::default()))),
    )
}

struct RichTextDemoApp;

impl Default for RichTextDemoApp {
    fn default() -> Self {
        Self
    }
}

impl eframe::App for RichTextDemoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Rich Text Demo - Multiple Styles");
            ui.add_space(20.0);

            // Example 1: Bold + Normal + Italic
            ui.label("1. Bold + Normal + Italic:");
            let span1 = TextSpan::new("Bold ")
                .with_style(TextStyle::body().bold())
                .with_child(TextSpan::new("Normal "))
                .with_child(TextSpan::new("Italic").with_style(TextStyle::body().italic()));

            ui.add(Text::rich(span1));
            ui.add_space(15.0);

            // Example 2: Different colors
            ui.label("2. Different colors:");
            let span2 = TextSpan::new("Red ")
                .with_style(TextStyle::body().with_color(Color::RED))
                .with_child(TextSpan::new("Green ").with_style(TextStyle::body().with_color(Color::GREEN)))
                .with_child(TextSpan::new("Blue").with_style(TextStyle::body().with_color(Color::BLUE)));

            ui.add(Text::rich(span2));
            ui.add_space(15.0);

            // Example 3: Different sizes
            ui.label("3. Different sizes:");
            let span3 = TextSpan::new("Small ")
                .with_style(TextStyle::body_small())
                .with_child(TextSpan::new("Medium ").with_style(TextStyle::body()))
                .with_child(TextSpan::new("Large").with_style(TextStyle::body_large()));

            ui.add(Text::rich(span3));
            ui.add_space(15.0);

            // Example 4: Headlines mixed with body
            ui.label("4. Headline + Body:");
            let span4 = TextSpan::new("Title: ")
                .with_style(TextStyle::headline2())
                .with_child(TextSpan::new("This is the body text in normal style").with_style(TextStyle::body()));

            ui.add(Text::rich(span4));
            ui.add_space(15.0);

            // Example 5: Code inline with text
            ui.label("5. Inline code:");
            let span5 = TextSpan::new("The function ")
                .with_style(TextStyle::body())
                .with_child(TextSpan::new("println!()").with_style(TextStyle::code().with_color(Color::DARK_GREEN)))
                .with_child(TextSpan::new(" prints to stdout").with_style(TextStyle::body()));

            ui.add(Text::rich(span5));
            ui.add_space(15.0);

            // Example 6: Complex styling
            ui.label("6. Complex combination:");
            let span6 = TextSpan::empty()
                .with_child(TextSpan::new("Error: ").with_style(TextStyle::body().bold().with_color(Color::RED)))
                .with_child(TextSpan::new("File "))
                .with_child(TextSpan::new("config.json").with_style(TextStyle::code()))
                .with_child(TextSpan::new(" not found!").with_style(TextStyle::body().italic()));

            ui.add(Text::rich(span6));
            ui.add_space(15.0);

            // Example 7: Centered rich text
            ui.label("7. Centered rich text:");
            let span7 = TextSpan::new("Bold ")
                .with_style(TextStyle::headline3().bold())
                .with_child(TextSpan::new("and "))
                .with_child(TextSpan::new("Colorful").with_style(TextStyle::headline3().with_color(Color::BLUE)));

            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                ui.add(Text::rich(span7));
            });
            ui.add_space(15.0);

            // Example 8: Long text with wrapping
            ui.label("8. Long wrapped text with styles:");
            let span8 = TextSpan::new("This is a ")
                .with_child(TextSpan::new("very long text ").with_style(TextStyle::body().bold()))
                .with_child(TextSpan::new("that should wrap "))
                .with_child(TextSpan::new("across multiple lines ").with_style(TextStyle::body().italic()))
                .with_child(TextSpan::new("demonstrating "))
                .with_child(TextSpan::new("rich text ").with_style(TextStyle::body().with_color(Color::DARK_GREEN)))
                .with_child(TextSpan::new("with different styles "))
                .with_child(TextSpan::new("maintained ").with_style(TextStyle::body().bold()))
                .with_child(TextSpan::new("throughout the wrapped text."));

            ui.add(Text::rich(span8));
        });
    }
}
