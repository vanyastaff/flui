//! Full Render Pipeline Example
//!
//! This example demonstrates the complete rendering pipeline:
//! Widget → RenderObject → Layer → Scene → Compositor → Screen
//!
//! This showcases the integration between:
//! - flui_core (RenderPipeline, Widgets, RenderObjects)
//! - flui_engine (Layers, Scene, Compositor, Painter)
//! - eframe (Window, Platform)

use flui_core::{Widget, RenderObjectWidget, RenderObject};
use flui_core::{LeafArity, LayoutCx, PaintCx};
use flui_core::render::RenderPipeline;
use flui_engine::{Scene, Compositor, Paint};
use flui_types::{Size, Rect};
use flui_types::constraints::BoxConstraints;

/// Simple colored box widget
#[derive(Debug, Clone)]
struct ColoredBox {
    width: f32,
    height: f32,
    color: [f32; 4],
}

impl ColoredBox {
    fn new(width: f32, height: f32, color: [f32; 4]) -> Self {
        Self { width, height, color }
    }
}

impl Widget for ColoredBox {}

impl RenderObjectWidget for ColoredBox {
    type RenderObject = ColoredBoxRender;
    type Arity = LeafArity;

    fn create_render_object(&self) -> Self::RenderObject {
        ColoredBoxRender {
            size: Size::new(self.width, self.height),
            color: self.color,
        }
    }

    fn update_render_object(&self, render: &mut Self::RenderObject) {
        render.size = Size::new(self.width, self.height);
        render.color = self.color;
    }
}

/// Render object for ColoredBox
#[derive(Debug)]
struct ColoredBoxRender {
    size: Size,
    color: [f32; 4],
}

impl RenderObject for ColoredBoxRender {
    type Arity = LeafArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        // Return our preferred size, constrained by parent
        cx.constraints().constrain(self.size)
    }

    fn paint(&self, _cx: &PaintCx<Self::Arity>) -> flui_engine::BoxedLayer {
        // Create a PictureLayer with our colored rectangle
        let mut picture = flui_engine::PictureLayer::new();

        // Use our own size (from layout phase)
        let rect = Rect::from_xywh(0.0, 0.0, self.size.width, self.size.height);
        let paint = Paint {
            color: self.color,
            ..Default::default()
        };

        picture.draw_rect(rect, paint);

        Box::new(picture)
    }
}

fn main() {
    println!("=== Full Render Pipeline Demo ===\n");

    // Step 1: Create RenderPipeline
    println!("Step 1: Creating RenderPipeline...");
    let mut pipeline = RenderPipeline::new();

    // Step 2: Insert root widget
    println!("Step 2: Creating Widget Tree...");
    let red_box = ColoredBox::new(200.0, 150.0, [1.0, 0.0, 0.0, 1.0]); // Red
    let _root_id = pipeline.insert_root(red_box);
    println!("  ✓ Root widget inserted");

    // Step 3: Layout Phase
    println!("\nStep 3: Layout Phase...");
    let viewport_size = Size::new(800.0, 600.0);
    let constraints = BoxConstraints::new(0.0, viewport_size.width, 0.0, viewport_size.height);

    let size = pipeline.flush_layout(constraints);
    println!("  ✓ Layout complete");
    println!("  └─ Root size: {:?}", size);

    // Step 4: Paint Phase (RenderObject → Layer)
    println!("\nStep 4: Paint Phase (RenderObject → Layer)...");
    let layer = pipeline.flush_paint();
    println!("  ✓ Layer tree created");
    println!("  └─ Root layer bounds: {:?}", layer.bounds());

    // Step 5: Scene Building (Layer → Scene)
    println!("\nStep 5: Scene Building (Layer → Scene)...");
    let scene = Scene::from_layer(layer, size.unwrap_or(viewport_size));
    println!("  ✓ Scene created");
    println!("  └─ Layer count: {}", scene.layer_count());
    println!("  └─ Scene bounds: {:?}", scene.content_bounds());
    println!("  └─ Viewport: {:?}", scene.viewport_size());

    // Step 6: Compositor (Scene → Screen)
    println!("\nStep 6: Compositor Setup...");
    let compositor = Compositor::new();
    println!("  ✓ Compositor created");
    println!("  └─ Culling enabled: {}", compositor.options().enable_culling);

    // In a real application, we would now:
    // 1. Create a Surface (eframe, winit + wgpu, etc.)
    // 2. Begin frame and get a Painter
    // 3. Call compositor.composite(&scene, painter)
    // 4. Present the frame

    println!("\n=== Pipeline Summary ===");
    println!("Widget (ColoredBox)");
    println!("  ↓ create_render_object()");
    println!("RenderObject (ColoredBoxRender)");
    println!("  ↓ layout() → Size");
    println!("  ↓ paint() → Layer");
    println!("Layer (PictureLayer)");
    println!("  ↓ Scene::from_layer()");
    println!("Scene (with EventRouter)");
    println!("  ↓ compositor.composite()");
    println!("Painter (EguiPainter / WgpuPainter)");
    println!("  ↓ present()");
    println!("Screen 🖥️");

    println!("\n✓ All pipeline stages demonstrated successfully!");
    println!("\nTo see this running in a window, check out:");
    println!("  - examples/window_demo.rs (eframe backend)");
    println!("  - crates/flui_engine/examples/interactive_button.rs (wgpu backend)");
}
