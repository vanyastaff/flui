//! Demo application showing nebula-ui controllers in action

use eframe::egui;
use nebula_ui::prelude::*;
use std::time::Duration;

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_title("nebula-ui Demo"),
        ..Default::default()
    };

    eframe::run_native(
        "nebula-ui Demo",
        options,
        Box::new(|_cc| Ok(Box::new(DemoApp::default()))),
    )
}

struct DemoApp {
    theme_controller: ThemeController,
    animation: AnimationController,
    focus: FocusController,
    visibility: VisibilityController,
    input: InputController,
    validation: ValidationController,
    change_tracker: ChangeTracker,
    demo_value: String,
}

impl Default for DemoApp {
    fn default() -> Self {
        let mut theme_controller = ThemeController::new()
            .with_transition(ThemeTransition::Fade(Duration::from_millis(300)))
            .with_persistence("nebula_ui_demo_theme");

        // Register a custom theme
        let custom_theme = ThemeBuilder::dark()
            .primary(egui::Color32::from_rgb(139, 92, 246)) // Purple
            .secondary(egui::Color32::from_rgb(236, 72, 153)) // Pink
            .build();
        theme_controller.register_theme("Custom Purple", custom_theme);

        Self {
            theme_controller,
            animation: AnimationController::new(Duration::from_millis(500))
                .with_curve(AnimationCurve::EaseInOut),
            focus: FocusController::default(),
            visibility: VisibilityController::new().with_hide_mode(HideMode::Fade),
            input: InputController::new().with_mode(InputMode::Normal),
            validation: ValidationController::new(),
            change_tracker: ChangeTracker::new(),
            demo_value: String::from("Hello, nebula-ui!"),
        }
    }
}

impl eframe::App for DemoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply theme with transitions
        self.theme_controller.apply(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("nebula-ui Controllers Demo");
            ui.separator();

            // Animation demo
            ui.group(|ui| {
                ui.label("Animation Controller:");

                if ui.button("Toggle Animation").clicked() {
                    self.animation.toggle();
                }

                let progress = self.animation.tick();
                ui.label(format!("Progress: {:.2}", progress));

                let animated_width = 100.0 + progress * 200.0;
                let (_id, rect) = ui.allocate_space(egui::vec2(animated_width, 20.0));
                ui.painter().rect_filled(
                    rect,
                    4.0,
                    self.theme_controller.theme().colors.primary,
                );
            });

            ui.separator();

            // Focus controller demo
            ui.group(|ui| {
                ui.label("Focus Controller:");

                let response = ui.button("Focus Me!");
                self.focus.update(&response);

                ui.label(format!(
                    "Focused: {}, Hovered: {}, Pressed: {}",
                    self.focus.has_focus(),
                    self.focus.is_hovered(),
                    self.focus.is_pressed()
                ));
            });

            ui.separator();

            // Visibility controller demo
            ui.group(|ui| {
                ui.label("Visibility Controller:");

                if ui.button("Toggle Visibility").clicked() {
                    self.visibility.toggle();
                }

                self.visibility.apply(ui, |ui| {
                    ui.label("This content can be hidden with animation!");
                });
            });

            ui.separator();

            // Input controller demo
            ui.group(|ui| {
                ui.label("Input Controller:");

                if !self.input.buffer().is_empty() {
                    ui.horizontal(|ui| {
                        ui.label("Editing:");

                        let response = ui.text_edit_singleline(
                            &mut self.demo_value
                        );

                        if response.changed() {
                            self.input.set_buffer(&self.demo_value);
                            self.validation.set_validating();
                        }
                    });

                    ui.label(format!("Buffer: {}", self.input.buffer()));
                } else {
                    if ui.button("Start Editing").clicked() {
                        self.input.begin_edit(&self.demo_value);
                    }
                }
            });

            ui.separator();

            // Validation controller demo
            ui.group(|ui| {
                ui.label("Validation Controller:");

                // Simulate validation states
                ui.horizontal(|ui| {
                    if ui.button("Set Valid").clicked() {
                        self.validation.set_valid();
                    }
                    if ui.button("Set Invalid").clicked() {
                        self.validation.set_error(nebula_ui::controllers::validation::ValidationError::new(
                            "ERROR_001",
                            "This is an error message"
                        ));
                    }
                    if ui.button("Set Warning").clicked() {
                        self.validation.set_warning(nebula_ui::controllers::validation::ValidationWarning::new(
                            "WARN_001",
                            "This is a warning"
                        ));
                    }
                });

                self.validation.render(ui);
            });

            ui.separator();

            // Change tracker demo
            ui.group(|ui| {
                ui.label("Change Tracker:");

                ui.label(format!("Is Dirty: {}", self.change_tracker.is_dirty()));

                ui.horizontal(|ui| {
                    if ui.button("Save Snapshot").clicked() {
                        if let Ok(value) = serde_json::to_value(&self.demo_value) {
                            self.change_tracker.save_snapshot(value);
                        }
                    }

                    let can_undo = self.change_tracker.can_undo();
                    if ui.add_enabled(can_undo, egui::Button::new("Undo")).clicked() {
                        if let Some(snapshot) = self.change_tracker.undo() {
                            if let Ok(string) = serde_json::from_value::<String>(snapshot.data) {
                                self.demo_value = string;
                            }
                        }
                    }

                    let can_redo = self.change_tracker.can_redo();
                    if ui.add_enabled(can_redo, egui::Button::new("Redo")).clicked() {
                        if let Some(snapshot) = self.change_tracker.redo() {
                            if let Ok(string) = serde_json::from_value::<String>(snapshot.data) {
                                self.demo_value = string;
                            }
                        }
                    }
                });
            });

            ui.separator();

            // Theme controller demo
            ui.group(|ui| {
                ui.label("Theme Controller:");

                ui.horizontal(|ui| {
                    if ui.button("Toggle Theme").clicked() {
                        self.theme_controller.toggle();
                    }

                    if ui.button("Dark").clicked() {
                        self.theme_controller.set_mode(ThemeMode::Dark);
                    }
                    if ui.button("Light").clicked() {
                        self.theme_controller.set_mode(ThemeMode::Light);
                    }
                    if ui.button("System").clicked() {
                        self.theme_controller.set_mode(ThemeMode::System);
                    }
                    if ui.button("Custom Purple").clicked() {
                        self.theme_controller.set_custom_theme("Custom Purple");
                    }
                });

                ui.label(format!(
                    "Current mode: {:?}, Is dark: {}",
                    self.theme_controller.mode(),
                    self.theme_controller.is_dark()
                ));
            });
        });

        // Request repaint for animations
        ctx.request_repaint();
    }
}