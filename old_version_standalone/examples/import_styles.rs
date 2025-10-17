//! Demo showing different import style options for nebula-ui
//!
//! Demonstrates 4 different ways to import types:
//! 1. Crate-level prelude (most convenient)
//! 2. Module-level preludes (for selective imports)
//! 3. Direct root imports (explicit)
//! 4. Full paths (most verbose)

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_title("Import Styles Demo"),
        ..Default::default()
    };

    eframe::run_simple_native("Import Styles Demo", options, move |ctx, _frame| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("nebula-ui Import Styles");
            ui.add_space(10.0);

            ui.collapsing("1. Crate-level prelude (Recommended)", |ui| {
                ui.code("use nebula_ui::prelude::*;");
                ui.label("Imports all commonly used types:");
                ui.label("  • Core: Color, Transform, Matrix4, Offset, Point, Rect, Size, ...");
                ui.label("  • Layout: Alignment, EdgeInsets, BoxConstraints, ...");
                ui.label("  • Styling: BoxDecoration, Border, BorderRadius, BoxShadow, ...");
                ui.label("  • Widgets: Container");
                ui.label("  • Painters: DecorationPainter, TransformPainter");
                ui.label("  • Controllers: AnimationController, ThemeController, ...");
                ui.label("  • egui::Widget trait (no need for separate import!)");
            });

            ui.add_space(10.0);

            ui.collapsing("2. Module-level preludes (NEW!)", |ui| {
                ui.code("use nebula_ui::types::prelude::*;");
                ui.label("Imports ALL type preludes (core + layout + styling)");
                ui.label("Good for when you need types but not widgets/controllers");
                ui.add_space(10.0);

                ui.label("Or import specific category preludes:");
                ui.add_space(5.0);

                ui.code("use nebula_ui::types::core::prelude::*;");
                ui.label("Imports only core types:");
                ui.label("  • Color, Transform, Matrix4, Offset, Point, Rect, Size, Scale");
                ui.label("  • Duration, Opacity, Rotation, Vector2, Vector3");
                ui.label("  • Circle, Arc, Bounds, Path, Range1D, Range2D");
                ui.add_space(5.0);

                ui.code("use nebula_ui::types::layout::prelude::*;");
                ui.label("Imports only layout types:");
                ui.label("  • Alignment, EdgeInsets, BoxConstraints");
                ui.label("  • Padding, Margin, Spacing");
                ui.label("  • CrossAxisAlignment, MainAxisAlignment, Axis, FlexDirection, ...");
                ui.add_space(5.0);

                ui.code("use nebula_ui::types::styling::prelude::*;");
                ui.label("Imports only styling types:");
                ui.label("  • BoxDecoration, Border, BorderRadius, BorderSide, Radius");
                ui.label("  • BoxShadow, Shadow, BlurStyle, Clip");
                ui.label("  • Gradient, LinearGradient, RadialGradient");
                ui.label("  • BlendMode, StrokeCap, StrokeJoin, StrokeStyle");
            });

            ui.add_space(10.0);

            ui.collapsing("3. Direct root imports", |ui| {
                ui.code("use nebula_ui::{Container, Color, Transform, EdgeInsets};");
                ui.label("Import specific types from crate root");
                ui.label("More explicit than prelude, less verbose than full paths");
            });

            ui.add_space(10.0);

            ui.collapsing("4. Full paths (Most verbose)", |ui| {
                ui.code("use nebula_ui::types::core::Color;");
                ui.code("use nebula_ui::types::core::Transform;");
                ui.code("use nebula_ui::types::layout::EdgeInsets;");
                ui.code("use nebula_ui::widgets::primitives::Container;");
                ui.label("Most explicit, but requires more typing");
            });

            ui.add_space(20.0);
            ui.separator();
            ui.add_space(10.0);

            // Example usage with crate-level prelude
            example_with_crate_prelude(ui);
        });
    })
}

// Example using crate-level prelude
fn example_with_crate_prelude(ui: &mut egui::Ui) {
    use nebula_ui::prelude::*;

    ui.label("Example Container using prelude imports:");

    Container::new()
        .with_width(300.0)
        .with_height(100.0)
        .with_decoration(
            BoxDecoration::new()
                .with_color(Color::from_rgb(100, 200, 150))
                .with_border_radius(BorderRadius::circular(10.0))
        )
        .with_padding(EdgeInsets::all(15.0))
        .with_transform(Transform::rotate_degrees(3.0))
        .with_transform_alignment(Alignment::CENTER)
        .child(|ui| {
            let response = ui.vertical_centered(|ui| {
                ui.heading("Prelude Example");
                ui.label("All types available without long paths!");
            });
            response.response
        })
        .ui(ui);
}
