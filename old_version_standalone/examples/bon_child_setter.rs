//! Example demonstrating bon builder with smart .child() setter
//!
//! This shows how .child() can be used directly in the bon builder chain
//! without needing to call .build() first.

use eframe::egui;
use nebula_ui::widgets::primitives::Container;
use nebula_ui::types::core::Color;
use nebula_ui::types::layout::EdgeInsets;
use nebula_ui::prelude::*;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([600.0, 400.0])
            .with_title("bon Builder .child() Setter Demo"),
        ..Default::default()
    };

    eframe::run_simple_native("bon_child_setter", options, move |ctx, _frame| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("bon Builder with Smart .child() Setter");
            ui.add_space(20.0);

            ui.label("Before (old way - needed .build().child()):");
            ui.code(".build().child(|ui| { ... }).ui(ui)");
            ui.add_space(10.0);

            ui.label("After (new way - .child() in builder chain!):");
            ui.code(".child(|ui| { ... }).ui(ui)");
            ui.add_space(20.0);

            ui.separator();
            ui.add_space(20.0);

            // Example 1: Simple colored box with child in builder chain
            ui.label("Example 1: Colored box");
            Container::builder()
                .width(250.0)
                .height(100.0)
                .padding(EdgeInsets::all(15.0))
                .color(Color::from_rgb(100, 150, 255))
                .child(|ui| {  // ← .child() works directly in builder chain!
                    ui.vertical(|ui| {
                        ui.heading("Success!");
                        ui.label("The .child() method works in bon builder!");
                    }).response
                })
                .ui(ui);  // ← Renders directly!

            ui.add_space(15.0);

            // Example 2: Rounded container with multiple properties
            ui.label("Example 2: Rounded container");
            Container::builder()
                .width(250.0)
                .height(120.0)
                .padding(EdgeInsets::all(20.0))
                .margin(EdgeInsets::all(5.0))
                .child(|ui| {  // ← Still works!
                    ui.vertical(|ui| {
                        ui.colored_label(Color::from_rgb(0, 100, 0), "🎉 Bon Builder Integration");
                        ui.label("Clean Flutter-like syntax");
                        ui.label("With type-safe builder");
                    }).response
                })
                .ui(ui);  // ← Custom finishing function!

            ui.add_space(15.0);

            // Example 3: Using factory method with struct fields
            ui.label("Example 3: Factory method with struct fields");
            let mut container = Container::rounded(Color::from_rgb(255, 200, 100), 12.0);
            container.width = Some(250.0);
            container.padding = EdgeInsets::all(15.0);
            container.child = Some(Box::new(|ui| {
                ui.vertical(|ui| {
                    ui.label("Factory method + struct fields");
                    ui.label("Clean and explicit!");
                }).response
            }));
            container.ui(ui);

            ui.add_space(20.0);
            ui.separator();
            ui.add_space(10.0);

            ui.label("✅ Container creation patterns:");
            ui.label("  1. Struct literal: Container { width: Some(100.0), ... }");
            ui.label("  2. bon builder: Container::builder().width(100.0).ui(ui)");
            ui.label("  3. Factory + fields: Container::colored(Color::RED) with field assignment");
        });
    })
}
