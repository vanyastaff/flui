//! Demonstration of three different syntax styles for creating Container widgets
//!
//! This example shows:
//! 1. Struct Literal (Flutter-like) - most concise
//! 2. Builder Pattern (Rust idiomatic) - chainable
//! 3. bon Builder (Type-safe) - auto-generated

use eframe::egui;
use nebula_ui::prelude::*;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 700.0])
            .with_title("Three Syntax Styles for Container"),
        ..Default::default()
    };

    eframe::run_simple_native("Three Syntax Styles", options, move |ctx, _frame| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("🎨 Three Syntax Styles for Container");
            ui.add_space(10.0);
            ui.label("nebula-ui supports three different ways to create widgets:");
            ui.add_space(20.0);

            // Display all three styles side by side
            ui.horizontal(|ui| {
                // Style 1: Struct Literal
                ui.vertical(|ui| {
                    ui.strong("1️⃣  Struct Literal (Flutter-like)");
                    ui.add_space(5.0);

                    Container {
                        width: Some(250.0),
                        height: Some(180.0),
                        padding: EdgeInsets::all(15.0),
                        decoration: Some(BoxDecoration::new()
                            .with_color(Color::from_rgb(100, 150, 255))
                            .with_border_radius(BorderRadius::circular(12.0))
                        ),
                        alignment: Some(Alignment::CENTER),
                        child: Some(Box::new(|ui| {
                            ui.vertical_centered(|ui| {
                                ui.heading("Struct Literal");
                                ui.add_space(5.0);
                                ui.label("✅ Most concise");
                                ui.label("✅ Named fields");
                                ui.label("✅ Flutter-like");
                                ui.add_space(5.0);
                                ui.label("❌ Needs Some(...)");
                                ui.label("❌ Child needs Box");
                            });
                            ui.allocate_response(ui.available_size(), egui::Sense::hover())
                        })),
                        ..Default::default()
                    }
                    .ui(ui);
                });

                ui.add_space(10.0);

                // Style 2: Builder Pattern
                ui.vertical(|ui| {
                    ui.strong("2️⃣  Builder Pattern (Current)");
                    ui.add_space(5.0);

                    Container::new()
                        .with_width(250.0)
                        .with_height(180.0)
                        .with_padding(EdgeInsets::all(15.0))
                        .with_decoration(BoxDecoration::new()
                            .with_color(Color::from_rgb(255, 150, 100))
                            .with_border_radius(BorderRadius::circular(12.0))
                        )
                        .with_alignment(Alignment::CENTER)
                        .child(|ui| {
                            ui.vertical_centered(|ui| {
                                ui.heading("Builder Pattern");
                                ui.add_space(5.0);
                                ui.label("✅ Chainable");
                                ui.label("✅ No Some(...)");
                                ui.label("✅ Rust idiomatic");
                                ui.add_space(5.0);
                                ui.label("❌ .with_* prefix");
                            });
                            ui.allocate_response(ui.available_size(), egui::Sense::hover())
                        })
                        .ui(ui);
                });

                ui.add_space(10.0);

                // Style 3: bon Builder
                ui.vertical(|ui| {
                    ui.strong("3️⃣  bon Builder (Type-safe)");
                    ui.add_space(5.0);

                    Container::builder()
                        .width(250.0)
                        .height(180.0)
                        .padding(EdgeInsets::all(15.0))
                        .decoration(BoxDecoration::new()
                            .with_color(Color::from_rgb(150, 200, 100))
                            .with_border_radius(BorderRadius::circular(12.0))
                        )
                        .alignment(Alignment::CENTER)
                        .build()
                        .child(|ui| {
                            ui.vertical_centered(|ui| {
                                ui.heading("bon Builder");
                                ui.add_space(5.0);
                                ui.label("✅ Type-safe");
                                ui.label("✅ No Some(...)");
                                ui.label("✅ Clean names");
                                ui.add_space(5.0);
                                ui.label("❌ Needs .build()");
                            });
                            ui.allocate_response(ui.available_size(), egui::Sense::hover())
                        })
                        .ui(ui);
                });
            });

            ui.add_space(30.0);
            ui.separator();
            ui.add_space(15.0);

            // Code comparison
            ui.collapsing("📝 Code Comparison", |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.strong("Struct Literal:");
                        ui.monospace("Container {");
                        ui.monospace("  width: Some(250.0),");
                        ui.monospace("  padding: EdgeInsets::all(15.0),");
                        ui.monospace("  child: Some(Box::new(|ui| {");
                        ui.monospace("    ui.label(\"Hello\")");
                        ui.monospace("  })),");
                        ui.monospace("  ..Default::default()");
                        ui.monospace("}.ui(ui);");
                    });

                    ui.add_space(15.0);

                    ui.vertical(|ui| {
                        ui.strong("Builder Pattern:");
                        ui.monospace("Container::new()");
                        ui.monospace("  .with_width(250.0)");
                        ui.monospace("  .with_padding(EdgeInsets::all(15.0))");
                        ui.monospace("  .child(|ui| {");
                        ui.monospace("    ui.label(\"Hello\")");
                        ui.monospace("  })");
                        ui.monospace("  .ui(ui);");
                    });

                    ui.add_space(15.0);

                    ui.vertical(|ui| {
                        ui.strong("bon Builder:");
                        ui.monospace("Container::builder()");
                        ui.monospace("  .width(250.0)");
                        ui.monospace("  .padding(EdgeInsets::all(15.0))");
                        ui.monospace("  .build()");
                        ui.monospace("  .child(|ui| {");
                        ui.monospace("    ui.label(\"Hello\")");
                        ui.monospace("  })");
                        ui.monospace("  .ui(ui);");
                    });
                });
            });

            ui.add_space(15.0);

            ui.collapsing("📊 Comparison Table", |ui| {
                use egui_extras::{TableBuilder, Column};

                TableBuilder::new(ui)
                    .column(Column::auto())
                    .column(Column::auto())
                    .column(Column::auto())
                    .column(Column::auto())
                    .header(20.0, |mut header| {
                        header.col(|ui| { ui.strong("Feature"); });
                        header.col(|ui| { ui.strong("Struct Literal"); });
                        header.col(|ui| { ui.strong("Builder"); });
                        header.col(|ui| { ui.strong("bon"); });
                    })
                    .body(|mut body| {
                        body.row(18.0, |mut row| {
                            row.col(|ui| { ui.label("Conciseness"); });
                            row.col(|ui| { ui.label("⭐⭐⭐⭐⭐"); });
                            row.col(|ui| { ui.label("⭐⭐⭐"); });
                            row.col(|ui| { ui.label("⭐⭐⭐⭐"); });
                        });
                        body.row(18.0, |mut row| {
                            row.col(|ui| { ui.label("Flutter-like"); });
                            row.col(|ui| { ui.label("⭐⭐⭐⭐⭐"); });
                            row.col(|ui| { ui.label("⭐⭐⭐"); });
                            row.col(|ui| { ui.label("⭐⭐⭐⭐"); });
                        });
                        body.row(18.0, |mut row| {
                            row.col(|ui| { ui.label("Type safety"); });
                            row.col(|ui| { ui.label("⭐⭐⭐"); });
                            row.col(|ui| { ui.label("⭐⭐⭐⭐"); });
                            row.col(|ui| { ui.label("⭐⭐⭐⭐⭐"); });
                        });
                        body.row(18.0, |mut row| {
                            row.col(|ui| { ui.label("Ease of use"); });
                            row.col(|ui| { ui.label("⭐⭐⭐"); });
                            row.col(|ui| { ui.label("⭐⭐⭐⭐⭐"); });
                            row.col(|ui| { ui.label("⭐⭐⭐⭐"); });
                        });
                        body.row(18.0, |mut row| {
                            row.col(|ui| { ui.label("IDE support"); });
                            row.col(|ui| { ui.label("⭐⭐⭐⭐"); });
                            row.col(|ui| { ui.label("⭐⭐⭐⭐⭐"); });
                            row.col(|ui| { ui.label("⭐⭐⭐⭐"); });
                        });
                    });
            });

            ui.add_space(15.0);

            ui.collapsing("💡 When to Use Each Style", |ui| {
                ui.group(|ui| {
                    ui.label("🔹 Use Struct Literal when:");
                    ui.label("  • Creating simple containers");
                    ui.label("  • You want Flutter-like syntax");
                    ui.label("  • Code brevity is important");
                    ui.label("  • You're okay with Some(...) wrappers");
                });

                ui.add_space(10.0);

                ui.group(|ui| {
                    ui.label("🔹 Use Builder Pattern when:");
                    ui.label("  • You need .child() with closures");
                    ui.label("  • You prefer Rust idioms");
                    ui.label("  • You want chainable API");
                    ui.label("  • Existing codebase uses this style");
                });

                ui.add_space(10.0);

                ui.group(|ui| {
                    ui.label("🔹 Use bon Builder when:");
                    ui.label("  • You want compile-time type safety");
                    ui.label("  • You want Flutter-like field names");
                    ui.label("  • You prefer no Some(...) wrappers");
                    ui.label("  • You can add .child() after .build()");
                });
            });

            ui.add_space(15.0);

            ui.collapsing("🎯 Recommended Approach", |ui| {
                ui.label("✨ Mix and match based on your needs!");
                ui.add_space(5.0);
                ui.label("All three styles work perfectly and can be used together:");
                ui.add_space(5.0);
                ui.monospace("// Use bon builder for base");
                ui.monospace("let container = Container::builder()");
                ui.monospace("    .width(300.0)");
                ui.monospace("    .padding(EdgeInsets::all(20.0))");
                ui.monospace("    .build();");
                ui.add_space(3.0);
                ui.monospace("// Extend with manual builder");
                ui.monospace("container");
                ui.monospace("    .with_decoration(...)");
                ui.monospace("    .child(|ui| { ... })");
                ui.monospace("    .ui(ui);");
            });
        });
    })
}
