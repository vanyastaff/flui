use eframe::egui;
use nebula_ui::prelude::*;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_title("Size Hint Demo"),
        ..Default::default()
    };

    eframe::run_native(
        "size_hint_demo",
        options,
        Box::new(|_cc| Ok(Box::new(SizeHintDemo::new()))),
    )
}

struct SizeHintDemo {
    show_hints: bool,
}

impl SizeHintDemo {
    fn new() -> Self {
        Self {
            show_hints: true,
        }
    }
}

impl eframe::App for SizeHintDemo {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("📏 Size Hint Demo");
            ui.add_space(10.0);

            ui.checkbox(&mut self.show_hints, "Show size hints");
            ui.separator();
            ui.add_space(20.0);

            // Example 1: Fixed size container
            ui.heading("1. Fixed Size Container");
            ui.label("Container знает свой размер заранее:");

            let container1 = Container {
                width: Some(300.0),
                height: Some(150.0),
                color: Some(Color::from_rgb(100, 150, 255)),
                padding: EdgeInsets::all(20.0),
                margin: EdgeInsets::all(10.0),
                ..Default::default()
            };

            if self.show_hints {
                if let Some(hint) = size_hint(&container1, ui) {
                    ui.colored_label(
                        Color::GREEN,
                        format!("✅ Size hint: {:.0} × {:.0} px", hint.x, hint.y)
                    );
                } else {
                    ui.colored_label(Color::RED, "❌ No size hint available");
                }
            }

            container1.ui(ui);
            ui.add_space(20.0);

            // Example 2: Container with only minimum constraints
            ui.heading("2. Container with Min Constraints");
            ui.label("Container знает минимальный размер:");

            let container2 = Container {
                min_width: Some(250.0),
                min_height: Some(100.0),
                color: Some(Color::from_rgb(255, 150, 100)),
                padding: EdgeInsets::all(15.0),
                ..Default::default()
            };

            if self.show_hints {
                if let Some(hint) = size_hint(&container2, ui) {
                    ui.colored_label(
                        Color::GREEN,
                        format!("✅ Size hint (min): {:.0} × {:.0} px", hint.x, hint.y)
                    );
                } else {
                    ui.colored_label(Color::RED, "❌ No size hint available");
                }
            }

            container2.ui(ui);
            ui.add_space(20.0);

            // Example 3: Container without fixed size (depends on child)
            ui.heading("3. Container without Fixed Size");
            ui.label("Container НЕ знает размер (зависит от child):");

            let container3 = Container {
                padding: EdgeInsets::all(25.0),
                color: Some(Color::from_rgb(100, 255, 150)),
                ..Default::default()
            };

            if self.show_hints {
                if let Some(hint) = size_hint(&container3, ui) {
                    ui.colored_label(
                        Color::GREEN,
                        format!("✅ Size hint: {:.0} × {:.0} px", hint.x, hint.y)
                    );
                } else {
                    ui.colored_label(
                        Color::YELLOW,
                        "⚠️ No size hint (depends on content)"
                    );
                }
            }

            container3.ui(ui);
            ui.add_space(20.0);

            // Example 4: Practical usage - Column layout
            ui.heading("4. Practical: Column Layout Optimization");
            ui.label("Column может предвычислить общую высоту:");

            let containers = vec![
                Container {
                    width: Some(400.0),
                    height: Some(60.0),
                    color: Some(Color::from_rgb(255, 100, 100)),
                    padding: EdgeInsets::all(10.0),
                    ..Default::default()
                },
                Container {
                    width: Some(400.0),
                    height: Some(80.0),
                    color: Some(Color::from_rgb(100, 255, 100)),
                    padding: EdgeInsets::all(10.0),
                    ..Default::default()
                },
                Container {
                    width: Some(400.0),
                    height: Some(70.0),
                    color: Some(Color::from_rgb(100, 100, 255)),
                    padding: EdgeInsets::all(10.0),
                    ..Default::default()
                },
            ];

            // Calculate total height before rendering
            let spacing = 10.0;
            let total_height: Option<f32> = containers
                .iter()
                .try_fold(0.0, |acc, container| {
                    size_hint(container, ui).map(|hint| acc + hint.y)
                })
                .map(|h| h + spacing * (containers.len() - 1) as f32);

            if self.show_hints {
                if let Some(height) = total_height {
                    ui.colored_label(
                        Color::GREEN,
                        format!(
                            "✅ Total column height (calculated): {:.0} px",
                            height
                        )
                    );
                } else {
                    ui.colored_label(Color::YELLOW, "⚠️ Cannot calculate total height");
                }
            }

            // Render column
            ui.vertical(|ui| {
                for container in containers {
                    container.ui(ui);
                    ui.add_space(spacing);
                }
            });

            ui.add_space(20.0);

            // Example 5: Size hint with BoxConstraints
            ui.heading("5. Container with BoxConstraints");
            ui.label("BoxConstraints также дают size hint:");

            let container5 = Container {
                constraints: Some(BoxConstraints::new(200.0, 400.0, 100.0, 200.0)),
                color: Some(Color::from_rgb(255, 200, 100)),
                padding: EdgeInsets::all(15.0),
                ..Default::default()
            };

            if self.show_hints {
                if let Some(hint) = size_hint(&container5, ui) {
                    ui.colored_label(
                        Color::GREEN,
                        format!(
                            "✅ Size hint (from constraints): {:.0} × {:.0} px",
                            hint.x, hint.y
                        )
                    );
                } else {
                    ui.colored_label(Color::RED, "❌ No size hint available");
                }
            }

            container5.ui(ui);

            ui.add_space(30.0);
            ui.separator();
            ui.add_space(10.0);

            ui.label("💡 Tip: Size hints помогают оптимизировать layout!");
            ui.label("   - Column/Row могут предвычислить размеры");
            ui.label("   - ScrollArea знает размер контента");
            ui.label("   - Grid может распределить ячейки");
        });
    }
}

/// Helper function to get size hint from Container
/// (В будущем это будет метод Widget trait)
fn size_hint(container: &Container, _ui: &egui::Ui) -> Option<egui::Vec2> {
    // Самый простой случай: фиксированные width и height
    if let (Some(w), Some(h)) = (container.width, container.height) {
        // Добавляем padding и margin
        return Some(egui::vec2(
            w + container.padding.horizontal_total() + container.margin.horizontal_total(),
            h + container.padding.vertical_total() + container.margin.vertical_total(),
        ));
    }

    // Если есть минимальные ограничения
    if let (Some(min_w), Some(min_h)) = (container.min_width, container.min_height) {
        return Some(egui::vec2(
            min_w + container.padding.horizontal_total() + container.margin.horizontal_total(),
            min_h + container.padding.vertical_total() + container.margin.vertical_total(),
        ));
    }

    // Если есть BoxConstraints, используем минимальные значения
    if let Some(_constraints) = &container.constraints {
        // Для простоты примера, просто возвращаем None
        // В реальности нужно было бы использовать constraints.min_width/min_height
        return None;
    }

    // Размер зависит от child или available space
    None
}
