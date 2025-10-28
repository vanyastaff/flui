//! Interactive button example
//!
//! Demonstrates event handling with clickable buttons that change color on hover and click.
//! Shows how to use the event system with Scene and custom layer event handling.

use flui_engine::{
    App, AppConfig, AppLogic, Backend, EventRouter, Layer, PictureLayer, Scene, SceneBuilder,
    Paint,
};
use flui_types::{Event, Offset, PointerEvent, Rect, Size};

/// Simple interactive button
struct Button {
    /// Button bounds
    bounds: Rect,
    /// Button label
    label: String,
    /// Current state
    state: ButtonState,
    /// Click count
    clicks: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ButtonState {
    Normal,
    Hovered,
    Pressed,
}

impl Button {
    fn new(x: f32, y: f32, width: f32, height: f32, label: &str) -> Self {
        Self {
            bounds: Rect::from_xywh(x, y, width, height),
            label: label.to_string(),
            state: ButtonState::Normal,
            clicks: 0,
        }
    }

    fn contains(&self, position: Offset) -> bool {
        self.bounds.contains(position)
    }

    fn handle_pointer_event(&mut self, event: &PointerEvent) -> bool {
        let position = event.position();

        match event {
            PointerEvent::Move(_) => {
                if self.contains(position) {
                    if self.state != ButtonState::Pressed {
                        self.state = ButtonState::Hovered;
                    }
                } else {
                    if self.state != ButtonState::Pressed {
                        self.state = ButtonState::Normal;
                    }
                }
                false // Don't stop propagation for move
            }
            PointerEvent::Down(_) => {
                if self.contains(position) {
                    self.state = ButtonState::Pressed;
                    true // Event handled
                } else {
                    false
                }
            }
            PointerEvent::Up(_) => {
                if self.contains(position) && self.state == ButtonState::Pressed {
                    self.clicks += 1;
                    self.state = ButtonState::Hovered;
                    println!("Button '{}' clicked! Total clicks: {}", self.label, self.clicks);
                    true // Event handled
                } else {
                    self.state = ButtonState::Normal;
                    false
                }
            }
            _ => false,
        }
    }

    fn get_color(&self) -> [f32; 4] {
        match self.state {
            ButtonState::Normal => [0.3, 0.5, 0.8, 1.0],    // Blue
            ButtonState::Hovered => [0.4, 0.6, 0.9, 1.0],   // Light blue
            ButtonState::Pressed => [0.2, 0.4, 0.7, 1.0],   // Dark blue
        }
    }
}

/// Application with interactive buttons
struct ButtonApp {
    scene: Scene,
    buttons: Vec<Button>,
    mouse_position: Offset,
}

impl ButtonApp {
    fn new() -> Self {
        let viewport_size = Size::new(800.0, 600.0);
        let scene = Scene::new(viewport_size);

        // Create some buttons
        let buttons = vec![
            Button::new(100.0, 100.0, 200.0, 60.0, "Button 1"),
            Button::new(100.0, 200.0, 200.0, 60.0, "Button 2"),
            Button::new(100.0, 300.0, 200.0, 60.0, "Button 3"),
            Button::new(400.0, 150.0, 200.0, 60.0, "Reset All"),
        ];

        Self {
            scene,
            buttons,
            mouse_position: Offset::ZERO,
        }
    }

    fn rebuild_scene(&mut self) {
        self.scene.clear();

        // Background
        let mut background = PictureLayer::new();
        background.draw_rect(
            Rect::from_xywh(0.0, 0.0, 800.0, 600.0),
            Paint {
                color: [0.1, 0.1, 0.15, 1.0],
                ..Default::default()
            },
        );
        self.scene.add_layer(Box::new(background));

        // Draw buttons
        for button in &self.buttons {
            let mut layer = PictureLayer::new();

            // Button background
            layer.draw_rect(
                button.bounds,
                Paint {
                    color: button.get_color(),
                    ..Default::default()
                },
            );

            // Button border
            layer.draw_rect(
                button.bounds,
                Paint {
                    color: [1.0, 1.0, 1.0, 0.5],
                    stroke_width: 2.0,
                    ..Default::default()
                },
            );

            self.scene.add_layer(Box::new(layer));
        }

        // Instructions text area
        let mut text_bg = PictureLayer::new();
        text_bg.draw_rect(
            Rect::from_xywh(50.0, 450.0, 700.0, 120.0),
            Paint {
                color: [0.15, 0.15, 0.2, 1.0],
                ..Default::default()
            },
        );
        self.scene.add_layer(Box::new(text_bg));

        // Mouse position indicator
        let mut cursor = PictureLayer::new();
        cursor.draw_circle(
            self.mouse_position.into(),
            5.0,
            Paint {
                color: [1.0, 0.3, 0.3, 0.8],
                ..Default::default()
            },
        );
        self.scene.add_layer(Box::new(cursor));
    }
}

impl AppLogic for ButtonApp {
    fn setup(&mut self) {
        println!("=== Interactive Button Demo ===");
        println!("Click the buttons to see event handling in action!");
        println!("Watch the console for click events.");
        self.rebuild_scene();
    }

    fn on_event(&mut self, event: &Event) -> bool {
        // Track mouse position
        if let Event::Pointer(PointerEvent::Move(data)) = event {
            self.mouse_position = data.position;
            self.rebuild_scene();
        }

        // Handle button events
        if let Event::Pointer(pointer_event) = event {
            let mut handled = false;

            // Check Reset All button first
            if self.buttons.len() > 3 {
                if self.buttons[3].handle_pointer_event(pointer_event) {
                    // Reset all other buttons
                    for i in 0..3 {
                        self.buttons[i].clicks = 0;
                    }
                    println!("All buttons reset!");
                    handled = true;
                }
            }

            // Check other buttons if reset wasn't clicked
            if !handled {
                for button in self.buttons.iter_mut().take(3) {
                    if button.handle_pointer_event(pointer_event) {
                        handled = true;
                        break;
                    }
                }
            }

            if handled || matches!(pointer_event, PointerEvent::Move(_)) {
                self.rebuild_scene();
            }

            return handled;
        }

        false
    }

    fn render(&mut self, painter: &mut dyn flui_engine::Painter) {
        self.scene.paint(painter);
    }
}

#[cfg(feature = "wgpu")]
fn main() {
    let config = AppConfig::new().backend(Backend::Wgpu);

    let app = App::with_config(config)
        .title("Interactive Button Demo")
        .size(800, 600)
        .vsync(true);

    app.run(ButtonApp::new()).expect("Failed to run app");
}

#[cfg(not(feature = "wgpu"))]
fn main() {
    println!("This example requires the 'wgpu' feature to be enabled.");
    println!("Run with: cargo run --example interactive_button --features wgpu");
}
