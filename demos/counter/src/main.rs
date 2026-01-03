//! Minimal Path Debug Demo
//!
//! Tests only path/line rendering to debug tessellation pipeline.

use flui_engine::Paint;
use flui_engine::{CanvasLayer, Layer, Scene, SceneRenderer};
use flui_painting::Canvas;
use flui_types::{
    geometry::{Offset, Point, Rect, Size},
    painting::Path,
    styling::Color,
};
use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

const WIDTH: f32 = 900.0;
const HEIGHT: f32 = 700.0;

/// Draw a simple test scene with basic shapes
fn draw_test_scene(canvas: &mut Canvas) {
    // Background
    canvas.draw_rect(
        Rect::from_xywh(0.0, 0.0, WIDTH, HEIGHT),
        &Paint::fill(Color::rgb(30, 30, 40)),
    );

    // Test 1: Simple filled rectangle (uses instanced rendering - should work)
    canvas.draw_rect(
        Rect::from_xywh(50.0, 50.0, 100.0, 100.0),
        &Paint::fill(Color::rgb(255, 0, 0)), // RED
    );

    // Test 2: Simple line (uses tessellation pipeline)
    canvas.draw_line(
        Point::new(200.0, 50.0),
        Point::new(400.0, 150.0),
        &Paint::stroke(Color::rgb(0, 255, 0), 5.0), // GREEN, 5px thick
    );

    // Test 3: Simple triangle path (uses tessellation pipeline)
    let mut triangle = Path::new();
    triangle.move_to(Point::new(450.0, 150.0));
    triangle.line_to(Point::new(550.0, 150.0));
    triangle.line_to(Point::new(500.0, 50.0));
    triangle.close();
    canvas.draw_path(&triangle, &Paint::fill(Color::rgb(0, 0, 255))); // BLUE

    // Test 4: Another line
    canvas.draw_line(
        Point::new(600.0, 50.0),
        Point::new(850.0, 150.0),
        &Paint::stroke(Color::rgb(255, 255, 0), 3.0), // YELLOW
    );

    // Test 5: Circle (uses instanced rendering - should work)
    canvas.draw_circle(
        Point::new(100.0, 250.0),
        50.0,
        &Paint::fill(Color::rgb(255, 0, 255)), // MAGENTA
    );

    // Test 6: Square path (uses tessellation)
    let mut square = Path::new();
    square.move_to(Point::new(200.0, 200.0));
    square.line_to(Point::new(300.0, 200.0));
    square.line_to(Point::new(300.0, 300.0));
    square.line_to(Point::new(200.0, 300.0));
    square.close();
    canvas.draw_path(&square, &Paint::fill(Color::rgb(0, 255, 255))); // CYAN

    // Labels (as colored rectangles)
    // "Rect (inst)" label
    canvas.draw_rect(
        Rect::from_xywh(50.0, 160.0, 100.0, 20.0),
        &Paint::fill(Color::rgb(100, 100, 100)),
    );

    // "Line (tess)" label
    canvas.draw_rect(
        Rect::from_xywh(270.0, 160.0, 80.0, 20.0),
        &Paint::fill(Color::rgb(100, 100, 100)),
    );

    // "Triangle (tess)" label
    canvas.draw_rect(
        Rect::from_xywh(460.0, 160.0, 80.0, 20.0),
        &Paint::fill(Color::rgb(100, 100, 100)),
    );

    // "Circle (inst)" label
    canvas.draw_rect(
        Rect::from_xywh(50.0, 310.0, 100.0, 20.0),
        &Paint::fill(Color::rgb(100, 100, 100)),
    );

    // "Square (tess)" label
    canvas.draw_rect(
        Rect::from_xywh(210.0, 310.0, 80.0, 20.0),
        &Paint::fill(Color::rgb(100, 100, 100)),
    );
}

// ===== Application =====

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<SceneRenderer>,
    start_time: std::time::Instant,
}

impl App {
    fn new() -> Self {
        Self {
            window: None,
            renderer: None,
            start_time: std::time::Instant::now(),
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let attrs = Window::default_attributes()
                .with_title("Path Debug Demo - RED=rect GREEN=line BLUE=triangle")
                .with_inner_size(winit::dpi::LogicalSize::new(WIDTH, HEIGHT));

            let window = Arc::new(event_loop.create_window(attrs).unwrap());
            let size = window.inner_size();

            // Create renderer with window (handles surface creation internally)
            let renderer = pollster::block_on(SceneRenderer::with_window(
                window.clone(),
                size.width,
                size.height,
            ))
            .expect("Failed to create renderer");

            self.window = Some(window);
            self.renderer = Some(renderer);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Escape),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                if let (Some(window), Some(renderer)) = (&self.window, &mut self.renderer) {
                    let size = window.inner_size();

                    // Create canvas and draw
                    let mut canvas = Canvas::new();
                    draw_test_scene(&mut canvas);

                    // Create canvas layer from canvas
                    let layer = CanvasLayer::from_canvas(canvas);

                    // Create scene and render
                    let scene = Scene::from_layer(
                        Size::new(size.width as f32, size.height as f32),
                        Layer::Canvas(layer),
                        0,
                    );

                    if let Err(e) = renderer.render_scene(&scene) {
                        eprintln!("Render error: {}", e);
                    }

                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("wgpu=warn".parse().unwrap())
                .add_directive("naga=warn".parse().unwrap()),
        )
        .init();

    println!("=== Path Debug Demo ===");
    println!(
        "Expected: RED rect, GREEN line, BLUE triangle, YELLOW line, MAGENTA circle, CYAN square"
    );
    println!("Instanced (should work): RED rect, MAGENTA circle");
    println!("Tessellated (debugging): GREEN line, BLUE triangle, YELLOW line, CYAN square");
    println!();

    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = App::new();
    event_loop.run_app(&mut app).unwrap();
}
