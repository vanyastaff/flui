//! Test egui layer transforms
//!
//! This example explores egui's layer transform API to understand
//! how to transform entire UI subtrees.

use eframe::egui;
use egui::{LayerId, Order, Id};

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([600.0, 400.0])
            .with_title("Layer Transform Test"),
        ..Default::default()
    };

    eframe::run_simple_native("Layer Transform Test", options, move |ctx, _frame| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Testing egui Layer Transforms");
            ui.add_space(20.0);

            // Normal rendering (no transform)
            ui.label("Normal (no transform):");
            ui.horizontal(|ui| {
                ui.button("Button 1");
                ui.label("Some text");
            });

            ui.add_space(20.0);
            ui.separator();
            ui.add_space(20.0);

            // Try to use layer transform
            ui.label("With layer transform attempt:");

            // Create a unique layer ID
            let layer_id = LayerId::new(Order::Middle, Id::new("transformed_layer"));

            // Get the painter for this layer
            let painter = ctx.layer_painter(layer_id);

            // Draw something on this layer
            let rect = egui::Rect::from_min_size(
                egui::pos2(50.0, 200.0),
                egui::vec2(100.0, 50.0),
            );
            painter.rect_filled(rect, 0.0, egui::Color32::BLUE);
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Transformed!",
                egui::FontId::default(),
                egui::Color32::WHITE,
            );

            // Try to apply transform to the layer
            // Let's see what methods are available on Context
            ui.label(format!("Checking available transform methods..."));

            // Let's check if we can access the shapes and transform them manually
            // This is a learning example to understand egui's layer system

            ui.add_space(20.0);
            ui.label("Note: This is a test to understand egui's layer transform API");
        });
    })
}
