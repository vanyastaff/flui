//! Render Objects Demo
//!
//! Demonstrates layout with RenderBox objects and rendering to screen.

use flui_engine::Paint;
use flui_engine::{CanvasLayer, Layer, Scene, SceneRenderer};
use flui_painting::Canvas;
use flui_rendering::{
    constraints::BoxConstraints,
    objects::{
        CrossAxisAlignment, MainAxisAlignment, RenderCenter, RenderColoredBox, RenderFlex,
        RenderPadding, RenderSizedBox,
    },
    traits::{RenderBox, RenderObject},
    wrapper::BoxWrapper,
};
use flui_types::{
    geometry::{Offset, Point, Rect, Size},
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

const WIDTH: f32 = 800.0;
const HEIGHT: f32 = 600.0;

/// A simple layout tree for demonstration
struct LayoutDemo {
    /// Root flex container (column)
    root: BoxWrapper<RenderFlex>,
    /// Child boxes with their colors and computed positions
    children: Vec<(BoxWrapper<RenderColoredBox>, Offset)>,
}

impl LayoutDemo {
    fn new() -> Self {
        // Create a vertical column with spacing
        let root = RenderFlex::column()
            .with_main_axis_alignment(MainAxisAlignment::Center)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(20.0);

        Self {
            root: BoxWrapper::new(root),
            children: Vec::new(),
        }
    }

    /// Perform layout and collect child positions
    fn layout(&mut self, size: Size) {
        // For now, we manually layout individual boxes and track positions
        // In a full implementation, the tree would handle this automatically

        let constraints = BoxConstraints::tight(size);

        // Layout root to get available space
        self.root.layout(constraints.clone(), true);

        // Create and layout child boxes manually
        // This simulates what a real render tree would do

        let boxes = vec![
            (
                RenderColoredBox::new([0.9, 0.2, 0.2, 1.0], Size::new(200.0, 80.0)),
                "Red",
            ),
            (
                RenderColoredBox::new([0.2, 0.8, 0.2, 1.0], Size::new(150.0, 60.0)),
                "Green",
            ),
            (
                RenderColoredBox::new([0.2, 0.4, 0.9, 1.0], Size::new(180.0, 70.0)),
                "Blue",
            ),
            (
                RenderColoredBox::new([0.9, 0.7, 0.1, 1.0], Size::new(120.0, 50.0)),
                "Yellow",
            ),
        ];

        // Calculate total height for centering
        let spacing = 20.0;
        let total_height: f32 = boxes
            .iter()
            .map(|(b, _)| b.preferred_size().height)
            .sum::<f32>()
            + spacing * (boxes.len() - 1) as f32;

        // Start position (centered vertically)
        let mut y = (size.height - total_height) / 2.0;

        self.children.clear();
        for (box_obj, _name) in boxes {
            let mut wrapper = BoxWrapper::new(box_obj);

            // Layout with loose constraints
            let child_constraints = BoxConstraints::loose(size);
            wrapper.layout(child_constraints, true);

            let child_size = wrapper.inner().size();

            // Center horizontally
            let x = (size.width - child_size.width) / 2.0;

            self.children.push((wrapper, Offset::new(x, y)));

            y += child_size.height + spacing;
        }
    }

    /// Paint the layout to canvas
    fn paint(&self, canvas: &mut Canvas) {
        // Draw background
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, WIDTH, HEIGHT),
            &Paint::fill(Color::rgb(40, 44, 52)),
        );

        // Draw title area
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, WIDTH, 50.0),
            &Paint::fill(Color::rgb(30, 34, 42)),
        );

        // Draw each child box at its computed position
        for (wrapper, offset) in &self.children {
            let size = wrapper.inner().size();
            let color = wrapper.inner().color();

            let rect = Rect::from_xywh(offset.dx, offset.dy, size.width, size.height);

            // Draw shadow
            canvas.draw_rect(
                Rect::from_xywh(offset.dx + 4.0, offset.dy + 4.0, size.width, size.height),
                &Paint::fill(Color::rgba(0, 0, 0, 80)),
            );

            // Draw box
            canvas.draw_rect(
                rect,
                &Paint::fill(Color::rgba(
                    (color[0] * 255.0) as u8,
                    (color[1] * 255.0) as u8,
                    (color[2] * 255.0) as u8,
                    (color[3] * 255.0) as u8,
                )),
            );

            // Draw border
            canvas.draw_rect(rect, &Paint::stroke(Color::rgb(255, 255, 255), 2.0));
        }

        // Draw info text area
        canvas.draw_rect(
            Rect::from_xywh(10.0, HEIGHT - 40.0, 300.0, 30.0),
            &Paint::fill(Color::rgba(0, 0, 0, 128)),
        );
    }
}

/// Demo showing individual object layout
fn demo_individual_objects(canvas: &mut Canvas) {
    // Background
    canvas.draw_rect(
        Rect::from_xywh(0.0, 0.0, WIDTH, HEIGHT),
        &Paint::fill(Color::rgb(40, 44, 52)),
    );

    // === Demo 1: RenderColoredBox ===
    let mut colored_box = BoxWrapper::new(RenderColoredBox::red(100.0, 100.0));
    colored_box.layout(BoxConstraints::loose(Size::new(200.0, 200.0)), true);

    let size = colored_box.inner().size();
    canvas.draw_rect(
        Rect::from_xywh(50.0, 50.0, size.width, size.height),
        &Paint::fill(Color::rgb(220, 50, 50)),
    );

    // Label
    canvas.draw_rect(
        Rect::from_xywh(50.0, 160.0, 100.0, 20.0),
        &Paint::fill(Color::rgb(80, 80, 80)),
    );

    // === Demo 2: RenderSizedBox (expand) ===
    let mut sized_box = BoxWrapper::new(RenderSizedBox::fixed(150.0, 80.0));
    sized_box.layout(BoxConstraints::loose(Size::new(300.0, 200.0)), true);

    let size = sized_box.inner().size();
    canvas.draw_rect(
        Rect::from_xywh(200.0, 50.0, size.width, size.height),
        &Paint::fill(Color::rgb(50, 180, 50)),
    );

    // Label
    canvas.draw_rect(
        Rect::from_xywh(200.0, 160.0, 150.0, 20.0),
        &Paint::fill(Color::rgb(80, 80, 80)),
    );

    // === Demo 3: RenderPadding (no child, just padding space) ===
    let mut padding = BoxWrapper::new(RenderPadding::all(20.0));
    padding.layout(BoxConstraints::tight(Size::new(100.0, 100.0)), true);

    let size = padding.inner().size();
    // Draw outer bounds
    canvas.draw_rect(
        Rect::from_xywh(400.0, 50.0, size.width, size.height),
        &Paint::stroke(Color::rgb(100, 150, 255), 2.0),
    );
    // Draw inner area (where child would be)
    canvas.draw_rect(
        Rect::from_xywh(420.0, 70.0, 60.0, 60.0),
        &Paint::fill(Color::rgb(100, 150, 255)),
    );

    // Label
    canvas.draw_rect(
        Rect::from_xywh(400.0, 160.0, 100.0, 20.0),
        &Paint::fill(Color::rgb(80, 80, 80)),
    );

    // === Demo 4: RenderCenter (no child) ===
    let mut center = BoxWrapper::new(RenderCenter::new());
    center.layout(BoxConstraints::tight(Size::new(120.0, 100.0)), true);

    let size = center.inner().size();
    canvas.draw_rect(
        Rect::from_xywh(550.0, 50.0, size.width, size.height),
        &Paint::stroke(Color::rgb(255, 200, 50), 2.0),
    );
    // Center indicator
    canvas.draw_circle(
        Point::new(550.0 + size.width / 2.0, 50.0 + size.height / 2.0),
        10.0,
        &Paint::fill(Color::rgb(255, 200, 50)),
    );

    // Label
    canvas.draw_rect(
        Rect::from_xywh(550.0, 160.0, 120.0, 20.0),
        &Paint::fill(Color::rgb(80, 80, 80)),
    );

    // === Demo 5: RenderFlex (row, no children) ===
    let mut flex_row = BoxWrapper::new(RenderFlex::row().with_spacing(10.0));
    flex_row.layout(BoxConstraints::loose(Size::new(200.0, 80.0)), true);

    // Draw row container outline
    canvas.draw_rect(
        Rect::from_xywh(50.0, 220.0, 200.0, 80.0),
        &Paint::stroke(Color::rgb(255, 100, 200), 2.0),
    );

    // Simulate 3 children in row
    for i in 0..3 {
        canvas.draw_rect(
            Rect::from_xywh(60.0 + i as f32 * 60.0, 240.0, 50.0, 40.0),
            &Paint::fill(Color::rgb(255, 100, 200)),
        );
    }

    // Label
    canvas.draw_rect(
        Rect::from_xywh(50.0, 310.0, 200.0, 20.0),
        &Paint::fill(Color::rgb(80, 80, 80)),
    );

    // === Demo 6: RenderFlex (column) ===
    canvas.draw_rect(
        Rect::from_xywh(300.0, 220.0, 100.0, 150.0),
        &Paint::stroke(Color::rgb(100, 220, 220), 2.0),
    );

    // Simulate 3 children in column
    for i in 0..3 {
        canvas.draw_rect(
            Rect::from_xywh(310.0, 230.0 + i as f32 * 45.0, 80.0, 35.0),
            &Paint::fill(Color::rgb(100, 220, 220)),
        );
    }

    // Label
    canvas.draw_rect(
        Rect::from_xywh(300.0, 380.0, 100.0, 20.0),
        &Paint::fill(Color::rgb(80, 80, 80)),
    );

    // === Info section ===
    canvas.draw_rect(
        Rect::from_xywh(450.0, 220.0, 300.0, 180.0),
        &Paint::fill(Color::rgb(50, 54, 62)),
    );

    // Title indicator
    canvas.draw_rect(
        Rect::from_xywh(460.0, 230.0, 280.0, 30.0),
        &Paint::fill(Color::rgb(70, 74, 82)),
    );

    // List of objects
    let colors = [
        Color::rgb(220, 50, 50),   // Red - ColoredBox
        Color::rgb(50, 180, 50),   // Green - SizedBox
        Color::rgb(100, 150, 255), // Blue - Padding
        Color::rgb(255, 200, 50),  // Yellow - Center
        Color::rgb(255, 100, 200), // Pink - Flex Row
        Color::rgb(100, 220, 220), // Cyan - Flex Column
    ];

    for (i, color) in colors.iter().enumerate() {
        canvas.draw_rect(
            Rect::from_xywh(470.0, 270.0 + i as f32 * 20.0, 12.0, 12.0),
            &Paint::fill(*color),
        );
        canvas.draw_rect(
            Rect::from_xywh(490.0, 270.0 + i as f32 * 20.0, 100.0, 12.0),
            &Paint::fill(Color::rgb(90, 94, 102)),
        );
    }
}

// ===== Application =====

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<SceneRenderer>,
    layout: Option<LayoutDemo>,
    use_layout_demo: bool,
}

impl App {
    fn new() -> Self {
        Self {
            window: None,
            renderer: None,
            layout: None,
            use_layout_demo: false,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let attrs = Window::default_attributes()
                .with_title("FLUI Render Objects Demo - Press SPACE to toggle view")
                .with_inner_size(winit::dpi::LogicalSize::new(WIDTH, HEIGHT));

            let window = Arc::new(event_loop.create_window(attrs).unwrap());
            let size = window.inner_size();

            let renderer = pollster::block_on(SceneRenderer::with_window(
                window.clone(),
                size.width,
                size.height,
            ))
            .expect("Failed to create renderer");

            // Initialize layout demo
            let mut layout = LayoutDemo::new();
            layout.layout(Size::new(size.width as f32, size.height as f32));

            self.window = Some(window);
            self.renderer = Some(renderer);
            self.layout = Some(layout);
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
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Space),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                self.use_layout_demo = !self.use_layout_demo;
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
                if let Some(layout) = &mut self.layout {
                    layout.layout(Size::new(size.width as f32, size.height as f32));
                }
            }
            WindowEvent::RedrawRequested => {
                if let (Some(window), Some(renderer)) = (&self.window, &mut self.renderer) {
                    let size = window.inner_size();

                    let mut canvas = Canvas::new();

                    if self.use_layout_demo {
                        if let Some(layout) = &self.layout {
                            layout.paint(&mut canvas);
                        }
                    } else {
                        demo_individual_objects(&mut canvas);
                    }

                    let layer = CanvasLayer::from_canvas(canvas);
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
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("wgpu=warn".parse().unwrap())
                .add_directive("naga=warn".parse().unwrap()),
        )
        .init();

    println!("=== FLUI Render Objects Demo ===");
    println!();
    println!("Demonstrating layout objects:");
    println!("  - RenderColoredBox (Leaf) - colored rectangle");
    println!("  - RenderSizedBox (Leaf) - fixed size container");
    println!("  - RenderPadding (Single) - adds padding");
    println!("  - RenderCenter (Single) - centers child");
    println!("  - RenderFlex (Variable) - row/column layout");
    println!();
    println!("Controls:");
    println!("  SPACE - Toggle between object demo and layout demo");
    println!("  ESC   - Exit");
    println!();

    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = App::new();
    event_loop.run_app(&mut app).unwrap();
}
