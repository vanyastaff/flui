//! Container rotation demo - testing visual transform rendering with epaint::Mesh::rotate

use eframe::egui;
use egui::Widget;  // Use egui's Widget trait
use nebula_ui::widgets::primitives::Container;
use nebula_ui::types::core::{Color, Transform};
use nebula_ui::types::layout::{EdgeInsets, Alignment};
use nebula_ui::types::styling::BoxDecoration;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_title("Container Rotation Demo"),
        ..Default::default()
    };

    eframe::run_simple_native("Container Rotation Demo", options, move |ctx, _frame| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Container Rotation - Transform Rendering Test");
            ui.add_space(20.0);

            ui.horizontal(|ui| {
                ui.label("Testing epaint::Mesh::rotate() for visual rotation:");
            });

            ui.add_space(20.0);

            // 1. No rotation (baseline)
            ui.label("No rotation:");
            Container::new()
                .with_width(100.0)
                .with_height(60.0)
                .with_color(Color::from_rgb(100, 150, 255))
                .child(|ui| {
                    ui.label("Normal")
                })
                .ui(ui);

            ui.add_space(20.0);

            // 2. 45 degree rotation
            ui.label("45° rotation:");
            Container::new()
                .with_width(100.0)
                .with_height(60.0)
                .with_color(Color::from_rgb(255, 100, 100))
                .with_transform(Transform::rotate_degrees(45.0))
                .child(|ui| {
                    ui.label("Rotated!")
                })
                .ui(ui);

            ui.add_space(20.0);

            // 3. 90 degree rotation
            ui.label("90° rotation:");
            Container::new()
                .with_width(100.0)
                .with_height(60.0)
                .with_color(Color::from_rgb(100, 255, 100))
                .with_transform(Transform::rotate_degrees(90.0))
                .child(|ui| {
                    ui.label("Vertical!")
                })
                .ui(ui);

            ui.add_space(20.0);

            // 4. Rotation with transform alignment
            ui.label("45° rotation with TOP_LEFT alignment:");
            Container::new()
                .with_width(100.0)
                .with_height(60.0)
                .with_color(Color::from_rgb(255, 200, 100))
                .with_transform(Transform::rotate_degrees(45.0))
                .with_transform_alignment(Alignment::TOP_LEFT)
                .child(|ui| {
                    ui.label("Corner!")
                })
                .ui(ui);

            ui.add_space(20.0);

            // 5. Rotation with decoration (border)
            ui.label("30° rotation with border:");
            Container::new()
                .with_width(120.0)
                .with_height(80.0)
                .with_decoration(
                    BoxDecoration::new()
                        .with_color(Color::from_rgb(200, 200, 255))
                )
                .with_padding(EdgeInsets::all(10.0))
                .with_transform(Transform::rotate_degrees(30.0))
                .child(|ui| {
                    ui.label("Decorated!")
                })
                .ui(ui);

            ui.add_space(20.0);

            ui.separator();
            ui.label("✅ Background boxes should be visually rotated");
            ui.label("⚠️  Child text will NOT rotate (egui limitation for widgets)");
            ui.label("This demonstrates epaint::Mesh::rotate() working for decoration");
        });
    })
}
