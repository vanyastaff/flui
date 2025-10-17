//! Demo of convenient imports with nebula_ui::prelude
//!
//! Shows the difference between verbose and convenient imports.

use eframe::egui;

// Option 1: Use prelude for everything (includes Widget trait!)
use nebula_ui::prelude::*;

// Now we can use all types directly!
// No need for:
// - nebula_ui::types::core::Color
// - nebula_ui::types::layout::EdgeInsets
// - nebula_ui::types::styling::BoxDecoration
// etc.

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([600.0, 400.0])
            .with_title("Prelude Demo - Convenient Imports"),
        ..Default::default()
    };

    eframe::run_simple_native("Prelude Demo", options, move |ctx, _frame| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("nebula_ui::prelude Demo");
            ui.label("All types imported with single line:");
            ui.code("use nebula_ui::prelude::*;");

            ui.add_space(20.0);

            // Use Container with all types directly available
            Container::new()
                .with_width(300.0)
                .with_height(200.0)
                .with_decoration(
                    BoxDecoration::new()
                        .with_color(Color::from_rgb(100, 150, 255))
                        .with_border_radius(BorderRadius::circular(12.0))
                )
                .with_padding(EdgeInsets::all(20.0))
                .with_transform(
                    Transform::rotate_degrees(5.0)
                        .then_scale_uniform(1.05)
                )
                .with_transform_alignment(Alignment::CENTER)
                .child(|ui| {
                    let response = ui.vertical_centered(|ui| {
                        ui.heading("Easy Imports! 🎉");
                        ui.add_space(10.0);
                        ui.label("No long paths needed:");
                        ui.code("Color::from_rgb(...)");
                        ui.code("EdgeInsets::all(...)");
                        ui.code("Transform::rotate_degrees(...)");
                        ui.add_space(10.0);
                        ui.label("Everything available via prelude!");
                    });
                    response.response
                })
                .ui(ui);

            ui.add_space(20.0);

            ui.separator();

            ui.collapsing("Available Types", |ui| {
                ui.label("Core: Color, Offset, Point, Rect, Size, Scale, Transform, Matrix4");
                ui.label("Layout: Alignment, EdgeInsets, BoxConstraints");
                ui.label("Styling: BoxDecoration, Border, BorderRadius, BoxShadow, Clip, Gradient");
                ui.label("Widgets: Container");
                ui.label("Painters: DecorationPainter, TransformPainter");
                ui.label("Controllers: AnimationController, ThemeController, FocusController, ...");
            });
        });
    })
}
