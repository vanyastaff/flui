//! Example demonstrating custom finishing functions for bon builder
//!
//! Shows three convenient APIs:
//! 1. .ui(ui) - Build and render in one call
//! 2. .build() - Build with validation
//! 3. .ui_checked(ui) - Build, validate, and render

use eframe::egui;
use nebula_ui::widgets::primitives::Container;
use nebula_ui::types::core::Color;
use nebula_ui::types::layout::EdgeInsets;
use nebula_ui::prelude::*;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([700.0, 500.0])
            .with_title("Custom Finishing Functions Demo"),
        ..Default::default()
    };

    eframe::run_simple_native("finishing_functions", options, move |ctx, _frame| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Custom Finishing Functions for bon Builder");
            ui.add_space(15.0);

            ui.label("Three convenient APIs for ergonomic usage:");
            ui.add_space(10.0);

            // Example 1: Direct .ui() call (no .build() needed!)
            ui.label("1. Direct .ui() - Most convenient (no .build() needed!):");
            ui.code("Container::builder().width(300).child(...).ui(ui)");
            ui.add_space(5.0);

            Container::builder()
                .width(300.0)
                .height(80.0)
                .padding(EdgeInsets::all(15.0))
                .color(Color::from_rgb(100, 150, 255))
                .child(|ui| {
                    ui.vertical(|ui| {
                        ui.heading("✨ Direct .ui() Call");
                        ui.label("No .build() needed - renders immediately!");
                    }).response
                })
                .ui(ui);  // ← No .build() needed!

            ui.add_space(20.0);

            // Example 2: .build(ui) with validation and render
            ui.label("2. .build(ui)? - Validate + render in one call:");
            ui.code("Container::builder().build(ui)?");
            ui.add_space(5.0);

            match Container::builder()
                .width(300.0)
                .height(80.0)
                .padding(EdgeInsets::all(15.0))
                .color(Color::from_rgb(150, 200, 100))
                .child(|ui| {
                    ui.vertical(|ui| {
                        ui.heading("✓ Validated + Rendered");
                        ui.label("One-call validation and rendering!");
                    }).response
                })
                .build(ui)  // ← New unified API!
            {
                Ok(_response) => {}
                Err(e) => {
                    ui.colored_label(Color::from_rgb(255, 100, 100), format!("Error: {}", e));
                }
            }

            ui.add_space(20.0);

            // Example 3: .try_build() - validate only, return Container
            ui.label("3. .try_build()? - Validate only, returns Container:");
            ui.code("let container = Container::builder().try_build()?;");
            ui.add_space(5.0);

            match Container::builder()
                .width(300.0)
                .height(80.0)
                .padding(EdgeInsets::all(15.0))
                .color(Color::from_rgb(255, 200, 100))
                .child(|ui| {
                    ui.vertical(|ui| {
                        ui.heading("✓ Validated Container");
                        ui.label("Container validated and returned!");
                    }).response
                })
                .try_build()  // ← Returns Container
            {
                Ok(container) => {
                    container.ui(ui);
                }
                Err(e) => {
                    ui.colored_label(Color::from_rgb(255, 100, 100), format!("Validation error: {}", e));
                }
            }

            ui.add_space(20.0);
            ui.separator();
            ui.add_space(10.0);

            // Example 4: Validation catches errors
            ui.label("4. Validation catches configuration errors:");
            ui.code("Container::builder().width(300).min_width(200) // ← Conflict!");
            ui.add_space(5.0);

            match Container::builder()
                .width(300.0)
                .min_width(200.0)  // ← This conflicts with width!
                .height(60.0)
                .padding(EdgeInsets::all(15.0))
                .color(Color::from_rgb(255, 150, 150))
                .try_build()  // ← Validates
            {
                Ok(container) => {
                    container.ui(ui);
                }
                Err(e) => {
                    ui.colored_label(Color::from_rgb(200, 50, 50), format!("❌ Validation error: {}", e));
                }
            }

            ui.add_space(15.0);

            // Example 5: Invalid values caught
            match Container::builder()
                .width(-100.0)  // ← Invalid negative width!
                .height(60.0)
                .try_build()  // ← Validates
            {
                Ok(_container) => {
                    ui.label("Should not reach here");
                }
                Err(e) => {
                    ui.colored_label(Color::from_rgb(200, 50, 50), format!("❌ Invalid width: {}", e));
                }
            }

            ui.add_space(20.0);
            ui.separator();
            ui.add_space(10.0);

            ui.label("API Comparison:");
            ui.label("  • .ui(ui) - Fast, no validation (99% use case)");
            ui.label("  • .build(ui)? - Validate + render, returns Result");
            ui.label("  • .try_build()? - Validate only, returns Container");
        });
    })
}
