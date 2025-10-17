//! Full TRS Transform Demo - Translation, Rotation, Scale
//!
//! Demonstrates Container widget with complete Matrix4-like transformations:
//! - Translation (offset)
//! - Rotation (angle)
//! - Scale (uniform and non-uniform)
//! - Combined TRS transformations

use eframe::egui;
use egui::Widget;
use nebula_ui::widgets::primitives::Container;
use nebula_ui::types::core::{Color, Transform, Scale, Offset};
use nebula_ui::types::layout::Alignment;
use nebula_ui::types::styling::BoxDecoration;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 700.0])
            .with_title("Full Transform Demo - TRS (Translate, Rotate, Scale)"),
        ..Default::default()
    };

    eframe::run_simple_native("Full Transform Demo", options, move |ctx, _frame| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Complete TRS Transform System");
            ui.label("Like Flutter's Matrix4 - Translation, Rotation, Scale");
            ui.add_space(10.0);

            egui::ScrollArea::vertical().show(ui, |ui| {
                // Section 1: Translation
                ui.separator();
                ui.heading("1. Translation (Offset)");
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.label("Baseline:");
                    Container::new()
                        .with_width(60.0)
                        .with_height(40.0)
                        .with_color(Color::from_rgb(200, 200, 200))
                        .ui(ui);

                    ui.add_space(20.0);

                    ui.label("Translate(30, 0):");
                    Container::new()
                        .with_width(60.0)
                        .with_height(40.0)
                        .with_color(Color::from_rgb(100, 150, 255))
                        .with_transform(Transform::translate(30.0, 0.0))
                        .ui(ui);

                    ui.add_space(20.0);

                    ui.label("Translate(0, 20):");
                    Container::new()
                        .with_width(60.0)
                        .with_height(40.0)
                        .with_color(Color::from_rgb(255, 150, 100))
                        .with_transform(Transform::translate(0.0, 20.0))
                        .ui(ui);
                });

                ui.add_space(20.0);

                // Section 2: Rotation
                ui.separator();
                ui.heading("2. Rotation");
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.label("0°:");
                    Container::new()
                        .with_width(60.0)
                        .with_height(40.0)
                        .with_color(Color::from_rgb(200, 200, 200))
                        .ui(ui);

                    ui.add_space(20.0);

                    ui.label("45°:");
                    Container::new()
                        .with_width(60.0)
                        .with_height(40.0)
                        .with_color(Color::from_rgb(255, 100, 100))
                        .with_transform(Transform::rotate_degrees(45.0))
                        .ui(ui);

                    ui.add_space(20.0);

                    ui.label("90°:");
                    Container::new()
                        .with_width(60.0)
                        .with_height(40.0)
                        .with_color(Color::from_rgb(100, 255, 100))
                        .with_transform(Transform::rotate_degrees(90.0))
                        .ui(ui);

                    ui.add_space(20.0);

                    ui.label("180°:");
                    Container::new()
                        .with_width(60.0)
                        .with_height(40.0)
                        .with_color(Color::from_rgb(255, 255, 100))
                        .with_transform(Transform::rotate_degrees(180.0))
                        .ui(ui);
                });

                ui.add_space(20.0);

                // Section 3: Scale
                ui.separator();
                ui.heading("3. Scale");
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.label("1.0 (normal):");
                    Container::new()
                        .with_width(60.0)
                        .with_height(40.0)
                        .with_color(Color::from_rgb(200, 200, 200))
                        .ui(ui);

                    ui.add_space(20.0);

                    ui.label("Scale 1.5:");
                    Container::new()
                        .with_width(60.0)
                        .with_height(40.0)
                        .with_color(Color::from_rgb(255, 150, 255))
                        .with_transform(Transform::scale_uniform(1.5))
                        .ui(ui);

                    ui.add_space(20.0);

                    ui.label("Scale 0.7:");
                    Container::new()
                        .with_width(60.0)
                        .with_height(40.0)
                        .with_color(Color::from_rgb(150, 255, 255))
                        .with_transform(Transform::scale_uniform(0.7))
                        .ui(ui);
                });

                ui.add_space(20.0);

                // Section 4: Non-uniform Scale
                ui.separator();
                ui.heading("4. Non-Uniform Scale");
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.label("Normal:");
                    Container::new()
                        .with_width(60.0)
                        .with_height(40.0)
                        .with_color(Color::from_rgb(200, 200, 200))
                        .ui(ui);

                    ui.add_space(20.0);

                    ui.label("Scale(2.0, 1.0):");
                    Container::new()
                        .with_width(60.0)
                        .with_height(40.0)
                        .with_color(Color::from_rgb(200, 150, 255))
                        .with_transform(Transform::scale(2.0, 1.0))
                        .ui(ui);

                    ui.add_space(20.0);

                    ui.label("Scale(1.0, 2.0):");
                    Container::new()
                        .with_width(60.0)
                        .with_height(40.0)
                        .with_color(Color::from_rgb(255, 200, 150))
                        .with_transform(Transform::scale(1.0, 2.0))
                        .ui(ui);
                });

                ui.add_space(20.0);

                // Section 5: Combined Transforms
                ui.separator();
                ui.heading("5. Combined Transformations (TRS)");
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.label("Rotate + Scale:");
                    Container::new()
                        .with_width(60.0)
                        .with_height(40.0)
                        .with_color(Color::from_rgb(255, 150, 100))
                        .with_transform(
                            Transform::rotate_degrees(30.0)
                                .then_scale_uniform(1.3)
                        )
                        .ui(ui);

                    ui.add_space(20.0);

                    ui.label("Scale + Rotate:");
                    Container::new()
                        .with_width(60.0)
                        .with_height(40.0)
                        .with_color(Color::from_rgb(100, 255, 150))
                        .with_transform(
                            Transform::scale_uniform(1.3)
                                .then_rotate_degrees(30.0)
                        )
                        .ui(ui);

                    ui.add_space(20.0);

                    ui.label("All three (TRS):");
                    Container::new()
                        .with_width(60.0)
                        .with_height(40.0)
                        .with_color(Color::from_rgb(255, 100, 255))
                        .with_transform(
                            Transform::translate(20.0, -10.0)
                                .then_rotate_degrees(25.0)
                                .then_scale(1.2, 0.8)
                        )
                        .ui(ui);
                });

                ui.add_space(20.0);

                // Section 6: Transform Alignment
                ui.separator();
                ui.heading("6. Transform Alignment (Pivot Point)");
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.label("Center (default):");
                    Container::new()
                        .with_width(80.0)
                        .with_height(60.0)
                        .with_color(Color::from_rgb(150, 200, 255))
                        .with_transform(Transform::rotate_degrees(30.0))
                        .with_transform_alignment(Alignment::CENTER)
                        .ui(ui);

                    ui.add_space(20.0);

                    ui.label("Top-Left:");
                    Container::new()
                        .with_width(80.0)
                        .with_height(60.0)
                        .with_color(Color::from_rgb(255, 200, 150))
                        .with_transform(Transform::rotate_degrees(30.0))
                        .with_transform_alignment(Alignment::TOP_LEFT)
                        .ui(ui);

                    ui.add_space(20.0);

                    ui.label("Bottom-Right:");
                    Container::new()
                        .with_width(80.0)
                        .with_height(60.0)
                        .with_color(Color::from_rgb(200, 255, 150))
                        .with_transform(Transform::rotate_degrees(30.0))
                        .with_transform_alignment(Alignment::BOTTOM_RIGHT)
                        .ui(ui);
                });

                ui.add_space(20.0);

                // Section 7: Advanced Examples
                ui.separator();
                ui.heading("7. Advanced Examples");
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.label("Spinning + Scaling:");
                    Container::new()
                        .with_width(70.0)
                        .with_height(70.0)
                        .with_decoration(
                            BoxDecoration::new()
                                .with_color(Color::from_rgb(255, 100, 200))
                        )
                        .with_transform(
                            Transform::rotate_degrees(45.0)
                                .then_scale(1.4, 0.7)
                        )
                        .ui(ui);

                    ui.add_space(20.0);

                    ui.label("Full TRS:");
                    Container::new()
                        .with_width(70.0)
                        .with_height(70.0)
                        .with_decoration(
                            BoxDecoration::new()
                                .with_color(Color::from_rgb(100, 200, 255))
                        )
                        .with_transform(
                            Transform::new(
                                Offset::new(15.0, -5.0),  // Translation
                                60.0_f32.to_radians(),    // Rotation (60°)
                                Scale::new(1.2, 1.5),     // Scale
                            )
                        )
                        .with_transform_alignment(Alignment::TOP_LEFT)
                        .ui(ui);
                });

                ui.add_space(30.0);
                ui.separator();

                ui.label("✅ All transformations use TRS (Translate-Rotate-Scale) order");
                ui.label("✅ Like Flutter's Matrix4 transformation system");
                ui.label("⚠️  Child content doesn't transform (egui limitation)");
            });
        });
    })
}
