//! Shadow effects demonstration.
//!
//! This example showcases the shadow painting capabilities of nebula-ui,
//! including GPU-accelerated gaussian blur shadows.

use eframe::egui;
use nebula_ui::types::core::{Color, Offset};
use nebula_ui::types::layout::EdgeInsets;
use nebula_ui::types::styling::{BoxDecoration, BoxShadow, BorderRadius, BlurStyle};
use nebula_ui::widgets::primitives::Container;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_title("Nebula UI - Shadow Effects Demo"),
        ..Default::default()
    };

    eframe::run_native(
        "Shadow Demo",
        options,
        Box::new(|_cc| Ok(Box::new(ShadowDemoApp::default()))),
    )
}

struct ShadowDemoApp {
    blur_radius: f32,
    spread_radius: f32,
    offset_x: f32,
    offset_y: f32,
    shadow_opacity: f32,
    elevation: f32,
}

impl Default for ShadowDemoApp {
    fn default() -> Self {
        Self {
            blur_radius: 10.0,
            spread_radius: 0.0,
            offset_x: 4.0,
            offset_y: 4.0,
            shadow_opacity: 0.5,  // Важно! Без этого тень невидима
            elevation: 4.0,
        }
    }
}

impl eframe::App for ShadowDemoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("🎨 Shadow Effects Demonstration");
            ui.add_space(10.0);

            ui.label("Powered by GPU-accelerated gaussian blur (GlowShadowPainter)");
            ui.add_space(20.0);

            ui.columns(2, |columns| {
                // Left panel - controls
                columns[0].vertical(|ui| {
                    ui.set_min_width(300.0);

                    ui.group(|ui| {
                        ui.label("Shadow Parameters:");
                        ui.add_space(10.0);

                        ui.horizontal(|ui| {
                            ui.label("Blur Radius:");
                            ui.add(egui::Slider::new(&mut self.blur_radius, 0.0..=50.0));
                        });

                        ui.horizontal(|ui| {
                            ui.label("Spread Radius:");
                            ui.add(egui::Slider::new(&mut self.spread_radius, -10.0..=20.0));
                        });

                        ui.horizontal(|ui| {
                            ui.label("Offset X:");
                            ui.add(egui::Slider::new(&mut self.offset_x, -30.0..=30.0));
                        });

                        ui.horizontal(|ui| {
                            ui.label("Offset Y:");
                            ui.add(egui::Slider::new(&mut self.offset_y, -30.0..=30.0));
                        });

                        ui.horizontal(|ui| {
                            ui.label("Shadow Opacity:");
                            ui.add(egui::Slider::new(&mut self.shadow_opacity, 0.0..=1.0));
                        });

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(10.0);

                        ui.horizontal(|ui| {
                            ui.label("Material Elevation:");
                            ui.add(egui::Slider::new(&mut self.elevation, 0.0..=24.0));
                        });
                    });

                    ui.add_space(20.0);

                    // Presets
                    ui.group(|ui| {
                        ui.label("Presets:");
                        ui.add_space(5.0);

                        if ui.button("Soft Shadow").clicked() {
                            self.blur_radius = 20.0;
                            self.spread_radius = 0.0;
                            self.offset_x = 0.0;
                            self.offset_y = 10.0;
                            self.shadow_opacity = 0.3;
                        }

                        if ui.button("Hard Shadow").clicked() {
                            self.blur_radius = 0.0;
                            self.spread_radius = 0.0;
                            self.offset_x = 5.0;
                            self.offset_y = 5.0;
                            self.shadow_opacity = 0.5;
                        }

                        if ui.button("Glow Effect").clicked() {
                            self.blur_radius = 25.0;
                            self.spread_radius = 5.0;
                            self.offset_x = 0.0;
                            self.offset_y = 0.0;
                            self.shadow_opacity = 0.8;
                        }

                        if ui.button("Drop Shadow").clicked() {
                            self.blur_radius = 10.0;
                            self.spread_radius = 2.0;
                            self.offset_x = 4.0;
                            self.offset_y = 4.0;
                            self.shadow_opacity = 0.4;
                        }
                    });
                });

                // Right panel - preview
                columns[1].vertical(|ui| {
                    ui.set_min_width(600.0); // Set minimum width for preview panel
                    ui.heading("Live Preview");
                    ui.add_space(30.0);

                    // Custom shadow box
                    ui.group(|ui| {
                        ui.label("Custom Shadow:");
                        ui.add_space(20.0);
                        let shadow_color = Color::from_rgba(
                            0,
                            0,
                            0,
                            (self.shadow_opacity * 255.0) as u8,
                        );

                        let shadow = BoxShadow::simple(
                            shadow_color,
                            Offset::new(self.offset_x, self.offset_y),
                            self.blur_radius,
                        ).with_spread_radius(self.spread_radius);

                        let decoration = BoxDecoration::new()
                            .with_color(Color::WHITE)
                            .with_border_radius(BorderRadius::circular(12.0))
                            .with_shadow(shadow);

                        // Center the container horizontally
                        ui.vertical_centered(|ui| {
                            // Use egui Widget trait directly
                            use egui::Widget;
                            Container::new()
                                .with_decoration(decoration)
                                .with_padding(EdgeInsets::all(32.0))
                                .with_width(200.0)
                                .with_height(120.0)
                                .child(|ui| {
                                    ui.centered_and_justified(|ui| {
                                        ui.label("Custom Shadow (Container)");
                                    });
                                    ui.allocate_response(ui.available_size(), egui::Sense::hover())
                                })
                                .ui(ui);
                        });
                    });

                    ui.add_space(30.0);

                    // Material elevation examples
                    ui.group(|ui| {
                        ui.label(format!("Material Design Elevation ({}dp):", self.elevation as i32));
                        ui.add_space(20.0);

                        let (key_shadow, ambient_shadow) = BoxShadow::elevation_shadows(self.elevation);

                        let decoration = BoxDecoration::new()
                            .with_color(Color::WHITE)
                            .with_border_radius(BorderRadius::circular(8.0))
                            .with_shadows(vec![ambient_shadow, key_shadow]);

                        ui.vertical_centered(|ui| {
                            ui.add_space(20.0);

                            use egui::Widget;
                            Container::new()
                                .with_decoration(decoration)
                                .with_padding(EdgeInsets::all(24.0))
                                .with_width(200.0)
                                .with_height(100.0)
                                .child(|ui| {
                                    ui.centered_and_justified(|ui| {
                                        ui.label("Elevated Card");
                                    });
                                    ui.allocate_response(ui.available_size(), egui::Sense::hover())
                                })
                                .ui(ui);

                            ui.add_space(20.0);
                        });
                    });

                    ui.add_space(30.0);

                    // Multiple shadows example
                    ui.group(|ui| {
                        ui.label("Layered Shadows:");
                        ui.add_space(20.0);

                        let red_shadow = BoxShadow::simple(
                            Color::from_rgba(255, 0, 0, 100),
                            Offset::new(-6.0, -6.0),
                            12.0,
                        ).with_spread_radius(2.0);

                        let blue_shadow = BoxShadow::simple(
                            Color::from_rgba(0, 0, 255, 100),
                            Offset::new(6.0, 6.0),
                            12.0,
                        ).with_spread_radius(2.0);

                        let decoration = BoxDecoration::new()
                            .with_color(Color::from_rgb(100, 150, 255))
                            .with_border_radius(BorderRadius::circular(16.0))
                            .with_shadows(vec![red_shadow, blue_shadow]);

                        ui.vertical_centered(|ui| {
                            ui.add_space(20.0);

                            use egui::Widget;
                            Container::new()
                                .with_decoration(decoration)
                                .with_padding(EdgeInsets::all(24.0))
                                .with_width(200.0)
                                .with_height(100.0)
                                .child(|ui| {
                                    ui.centered_and_justified(|ui| {
                                        ui.label("Double Shadow");
                                    });
                                    ui.allocate_response(ui.available_size(), egui::Sense::hover())
                                })
                                .ui(ui);

                            ui.add_space(20.0);
                        });
                    });
                });
            });

            ui.add_space(20.0);
            ui.separator();
            ui.label("💡 Tip: All shadows use GPU-accelerated gaussian blur for smooth, realistic effects!");
        });
    }
}
