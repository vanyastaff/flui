//! Input tracker example
//!
//! Comprehensive demonstration of all event types:
//! - Keyboard input (keys, modifiers, text)
//! - Mouse movement and clicks
//! - Scroll wheel
//! - Window events
//!
//! This example shows how to track and display all user input in real-time.

use flui_engine::{App, AppConfig, AppLogic, Backend, PictureLayer, Scene, Paint};
use flui_types::{Event, KeyEvent, Offset, PhysicalKey, PointerEvent, Rect, Size, WindowEvent};
use std::collections::VecDeque;

/// Maximum number of events to display
const MAX_EVENT_HISTORY: usize = 15;

/// Input tracking application
struct InputTrackerApp {
    scene: Scene,
    mouse_position: Offset,
    mouse_buttons: u8,
    keys_pressed: Vec<PhysicalKey>,
    modifiers: String,
    scroll_delta: (f32, f32),
    event_history: VecDeque<String>,
    last_text_input: String,
}

impl InputTrackerApp {
    fn new() -> Self {
        let viewport_size = Size::new(1000.0, 700.0);
        let scene = Scene::new(viewport_size);

        Self {
            scene,
            mouse_position: Offset::ZERO,
            mouse_buttons: 0,
            keys_pressed: Vec::new(),
            modifiers: String::new(),
            scroll_delta: (0.0, 0.0),
            event_history: VecDeque::new(),
            last_text_input: String::new(),
        }
    }

    fn add_event(&mut self, event: String) {
        self.event_history.push_front(event);
        if self.event_history.len() > MAX_EVENT_HISTORY {
            self.event_history.pop_back();
        }
    }

    fn format_key(key: PhysicalKey) -> String {
        format!("{:?}", key).replace("Key", "")
    }

    fn rebuild_scene(&mut self) {
        self.scene.clear();

        // Background
        let mut background = PictureLayer::new();
        background.draw_rect(
            Rect::from_xywh(0.0, 0.0, 1000.0, 700.0),
            Paint {
                color: [0.05, 0.05, 0.08, 1.0],
                ..Default::default()
            },
        );
        self.scene.add_layer(Box::new(background));

        // Mouse tracking area (left side)
        let mut mouse_area = PictureLayer::new();
        mouse_area.draw_rect(
            Rect::from_xywh(20.0, 20.0, 460.0, 320.0),
            Paint {
                color: [0.1, 0.1, 0.15, 1.0],
                ..Default::default()
            },
        );
        mouse_area.draw_rect(
            Rect::from_xywh(20.0, 20.0, 460.0, 320.0),
            Paint {
                color: [0.3, 0.5, 0.8, 1.0],
                stroke_width: 2.0,
                ..Default::default()
            },
        );
        self.scene.add_layer(Box::new(mouse_area));

        // Mouse cursor indicator
        let mut cursor = PictureLayer::new();
        cursor.draw_circle(
            self.mouse_position.into(),
            8.0,
            Paint {
                color: [1.0, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );
        cursor.draw_circle(
            self.mouse_position.into(),
            8.0,
            Paint {
                color: [1.0, 1.0, 1.0, 1.0],
                stroke_width: 2.0,
                ..Default::default()
            },
        );
        self.scene.add_layer(Box::new(cursor));

        // Keyboard tracking area (right side)
        let mut keyboard_area = PictureLayer::new();
        keyboard_area.draw_rect(
            Rect::from_xywh(520.0, 20.0, 460.0, 320.0),
            Paint {
                color: [0.1, 0.1, 0.15, 1.0],
                ..Default::default()
            },
        );
        keyboard_area.draw_rect(
            Rect::from_xywh(520.0, 20.0, 460.0, 320.0),
            Paint {
                color: [0.5, 0.8, 0.3, 1.0],
                stroke_width: 2.0,
                ..Default::default()
            },
        );
        self.scene.add_layer(Box::new(keyboard_area));

        // Visualize pressed keys
        let mut y_offset = 60.0;
        for (i, key) in self.keys_pressed.iter().take(8).enumerate() {
            let mut key_vis = PictureLayer::new();
            let x = 540.0 + (i % 4) as f32 * 110.0;
            let y = y_offset + (i / 4) as f32 * 60.0;

            key_vis.draw_rect(
                Rect::from_xywh(x, y, 100.0, 50.0),
                Paint {
                    color: [0.3, 0.6, 0.3, 1.0],
                    ..Default::default()
                },
            );
            key_vis.draw_rect(
                Rect::from_xywh(x, y, 100.0, 50.0),
                Paint {
                    color: [0.5, 1.0, 0.5, 1.0],
                    stroke_width: 2.0,
                    ..Default::default()
                },
            );

            self.scene.add_layer(Box::new(key_vis));
        }

        // Event history area (bottom)
        let mut history_area = PictureLayer::new();
        history_area.draw_rect(
            Rect::from_xywh(20.0, 360.0, 960.0, 320.0),
            Paint {
                color: [0.08, 0.08, 0.12, 1.0],
                ..Default::default()
            },
        );
        history_area.draw_rect(
            Rect::from_xywh(20.0, 360.0, 960.0, 320.0),
            Paint {
                color: [0.8, 0.5, 0.3, 1.0],
                stroke_width: 2.0,
                ..Default::default()
            },
        );
        self.scene.add_layer(Box::new(history_area));

        // Draw event history lines
        for (i, event_str) in self.event_history.iter().enumerate() {
            let alpha = 1.0 - (i as f32 * 0.05);
            let mut event_line = PictureLayer::new();

            // Draw a small indicator rect for each event
            event_line.draw_rect(
                Rect::from_xywh(30.0, 380.0 + i as f32 * 20.0, 10.0, 10.0),
                Paint {
                    color: [0.8, 0.8, 0.3, alpha],
                    ..Default::default()
                },
            );

            self.scene.add_layer(Box::new(event_line));
        }

        // Info panels with colored backgrounds
        self.draw_info_panel(30.0, 30.0, "MOUSE", [0.3, 0.5, 0.8, 1.0]);
        self.draw_info_panel(530.0, 30.0, "KEYBOARD", [0.5, 0.8, 0.3, 1.0]);
        self.draw_info_panel(30.0, 370.0, "EVENT HISTORY", [0.8, 0.5, 0.3, 1.0]);
    }

    fn draw_info_panel(&mut self, x: f32, y: f32, title: &str, color: [f32; 4]) {
        let mut panel = PictureLayer::new();
        panel.draw_rect(
            Rect::from_xywh(x, y, 150.0, 25.0),
            Paint {
                color,
                ..Default::default()
            },
        );
        self.scene.add_layer(Box::new(panel));
    }
}

impl AppLogic for InputTrackerApp {
    fn setup(&mut self) {
        println!("=== Input Tracker Demo ===");
        println!("Move your mouse, click buttons, type on keyboard, and scroll!");
        println!("All events are tracked and displayed in real-time.");
        println!("Press ESC to exit.");
        self.rebuild_scene();
    }

    fn on_event(&mut self, event: &Event) -> bool {
        match event {
            Event::Pointer(pointer_event) => {
                match pointer_event {
                    PointerEvent::Move(data) => {
                        self.mouse_position = data.position;
                        self.add_event(format!(
                            "Mouse Move: ({:.0}, {:.0})",
                            data.position.dx, data.position.dy
                        ));
                    }
                    PointerEvent::Down(data) => {
                        self.mouse_buttons |= 1;
                        self.add_event(format!(
                            "Mouse Down: {:?} at ({:.0}, {:.0})",
                            data.button, data.position.dx, data.position.dy
                        ));
                        println!("🖱️  Mouse button pressed: {:?}", data.button);
                    }
                    PointerEvent::Up(data) => {
                        self.mouse_buttons &= !1;
                        self.add_event(format!(
                            "Mouse Up: {:?}",
                            data.button
                        ));
                        println!("🖱️  Mouse button released: {:?}", data.button);
                    }
                    _ => {}
                }
                self.rebuild_scene();
                true
            }

            Event::Key(key_event) => {
                match key_event {
                    KeyEvent::Down(data) => {
                        if !data.repeat && !self.keys_pressed.contains(&data.physical_key) {
                            self.keys_pressed.push(data.physical_key);
                        }

                        self.modifiers = format!(
                            "{}{}{}{}",
                            if data.modifiers.shift { "Shift+" } else { "" },
                            if data.modifiers.control { "Ctrl+" } else { "" },
                            if data.modifiers.alt { "Alt+" } else { "" },
                            if data.modifiers.meta { "Meta+" } else { "" },
                        );

                        let key_name = Self::format_key(data.physical_key);
                        let event_str = if let Some(text) = &data.text {
                            self.last_text_input = text.clone();
                            format!("Key Down: {} -> '{}'", key_name, text)
                        } else {
                            format!("Key Down: {}", key_name)
                        };

                        self.add_event(event_str.clone());
                        println!("⌨️  {}", event_str);
                    }
                    KeyEvent::Up(data) => {
                        self.keys_pressed.retain(|k| k != &data.physical_key);

                        let key_name = Self::format_key(data.physical_key);
                        self.add_event(format!("Key Up: {}", key_name));
                    }
                }
                self.rebuild_scene();
                true
            }

            Event::Scroll(scroll_data) => {
                match scroll_data.delta {
                    flui_types::ScrollDelta::Lines { x, y } => {
                        self.scroll_delta = (x, y);
                        self.add_event(format!("Scroll: lines ({:.1}, {:.1})", x, y));
                        println!("🔄 Scroll (lines): x={:.1}, y={:.1}", x, y);
                    }
                    flui_types::ScrollDelta::Pixels { x, y } => {
                        self.scroll_delta = (x, y);
                        self.add_event(format!("Scroll: pixels ({:.0}, {:.0})", x, y));
                        println!("🔄 Scroll (pixels): x={:.0}, y={:.0}", x, y);
                    }
                }
                self.rebuild_scene();
                true
            }

            Event::Window(window_event) => {
                let event_str = match window_event {
                    WindowEvent::Resized { width, height } => {
                        format!("Window Resized: {}x{}", width, height)
                    }
                    WindowEvent::Focused => "Window Focused".to_string(),
                    WindowEvent::Unfocused => "Window Unfocused".to_string(),
                    WindowEvent::CloseRequested => "Window Close Requested".to_string(),
                    WindowEvent::ScaleFactorChanged { scale_factor } => {
                        format!("DPI Changed: {:.2}", scale_factor)
                    }
                };

                self.add_event(event_str.clone());
                println!("🪟  {}", event_str);
                self.rebuild_scene();
                true
            }
        }
    }

    fn render(&mut self, painter: &mut dyn flui_engine::Painter) {
        self.scene.paint(painter);
    }
}

#[cfg(feature = "wgpu")]
fn main() {
    let config = AppConfig::new().backend(Backend::Wgpu);

    let app = App::with_config(config)
        .title("Input Tracker Demo - Move mouse, type, scroll!")
        .size(1000, 700)
        .vsync(true);

    app.run(InputTrackerApp::new()).expect("Failed to run app");
}

#[cfg(not(feature = "wgpu"))]
fn main() {
    println!("This example requires the 'wgpu' feature to be enabled.");
    println!("Run with: cargo run --example input_tracker --features wgpu");
}
