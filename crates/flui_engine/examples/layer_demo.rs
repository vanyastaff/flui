//! Demonstration of the Layer system
//!
//! This example shows how layers compose to build a scene graph.

use flui_engine::{
    PictureLayer, OpacityLayer, TransformLayer, ClipLayer, ContainerLayer,
    Paint, Painter, Layer,
};
use flui_types::{Rect, Point, Offset};

fn main() {
    println!("=== FLUI Engine Layer System Demo ===\n");

    // Create a picture layer with drawing commands
    let mut picture1 = PictureLayer::new();
    picture1.draw_rect(
        Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
        Paint {
            color: [1.0, 0.0, 0.0, 1.0],  // Red
            stroke_width: 0.0,
            anti_alias: true,
        }
    );
    picture1.draw_circle(
        Point::new(50.0, 50.0),
        20.0,
        Paint {
            color: [1.0, 1.0, 1.0, 1.0],  // White circle in center
            stroke_width: 0.0,
            anti_alias: true,
        }
    );
    println!("1. Created PictureLayer with red rect and white circle");
    println!("   Bounds: {:?}\n", picture1.bounds());

    // Wrap in opacity layer
    let opacity = OpacityLayer::new(Box::new(picture1), 0.7);
    println!("2. Wrapped in OpacityLayer (opacity: 0.7)");
    println!("   Bounds: {:?}\n", opacity.bounds());

    // Wrap in transform layer
    let transform = TransformLayer::translate(
        Box::new(opacity),
        Offset::new(50.0, 30.0)
    );
    println!("3. Wrapped in TransformLayer (translate +50, +30)");
    println!("   Bounds: {:?}\n", transform.bounds());

    // Create another picture layer
    let mut picture2 = PictureLayer::new();
    picture2.draw_rect(
        Rect::from_xywh(150.0, 0.0, 80.0, 120.0),
        Paint {
            color: [0.0, 0.0, 1.0, 1.0],  // Blue
            stroke_width: 2.0,
            anti_alias: true,
        }
    );
    println!("4. Created second PictureLayer with blue stroked rect");
    println!("   Bounds: {:?}\n", picture2.bounds());

    // Clip the blue rect
    let clip = ClipLayer::rect(
        Box::new(picture2),
        Rect::from_xywh(150.0, 0.0, 80.0, 60.0)  // Clip to half height
    );
    println!("5. Wrapped in ClipLayer (clip to half height)");
    println!("   Bounds: {:?}\n", clip.bounds());

    // Combine in container
    let mut container = ContainerLayer::new();
    container.add_child(Box::new(transform));
    container.add_child(Box::new(clip));
    println!("6. Combined in ContainerLayer");
    println!("   Bounds: {:?}\n", container.bounds());

    // Demonstrate layer tree
    println!("=== Layer Tree Structure ===");
    println!("ContainerLayer");
    println!("  ├─ TransformLayer (translate +50, +30)");
    println!("  │   └─ OpacityLayer (0.7)");
    println!("  │       └─ PictureLayer (red rect + white circle)");
    println!("  └─ ClipLayer (clip to half height)");
    println!("      └─ PictureLayer (blue stroked rect)");

    println!("\n=== Scene Composition Complete ===");
    println!("To render, call: container.paint(&mut painter)");
    println!("The painter backend (egui/wgpu/skia) will execute drawing commands.");

    // Create a mock painter to demonstrate the API
    let mut mock_painter = MockPainter::new();
    println!("\n=== Painting with Mock Backend ===");
    container.paint(&mut mock_painter);
    println!("Drawing operations executed: {}", mock_painter.operation_count);
}

/// Mock painter for demonstration
struct MockPainter {
    operation_count: usize,
    transform_depth: usize,
}

impl MockPainter {
    fn new() -> Self {
        Self {
            operation_count: 0,
            transform_depth: 0,
        }
    }
}

impl Painter for MockPainter {
    fn rect(&mut self, rect: Rect, paint: &Paint) {
        self.operation_count += 1;
        println!("  [{}] draw_rect({:?}, color: {:?})",
            self.operation_count, rect, paint.color);
    }

    fn rrect(&mut self, rrect: flui_engine::RRect, paint: &Paint) {
        self.operation_count += 1;
        println!("  [{}] draw_rrect({:?}, color: {:?})",
            self.operation_count, rrect.rect, paint.color);
    }

    fn circle(&mut self, center: Point, radius: f32, paint: &Paint) {
        self.operation_count += 1;
        println!("  [{}] draw_circle({:?}, radius: {}, color: {:?})",
            self.operation_count, center, radius, paint.color);
    }

    fn line(&mut self, p1: Point, p2: Point, paint: &Paint) {
        self.operation_count += 1;
        println!("  [{}] draw_line({:?} -> {:?}, color: {:?})",
            self.operation_count, p1, p2, paint.color);
    }

    fn save(&mut self) {
        self.transform_depth += 1;
        println!("  [save] Transform depth: {}", self.transform_depth);
    }

    fn restore(&mut self) {
        self.transform_depth -= 1;
        println!("  [restore] Transform depth: {}", self.transform_depth);
    }

    fn translate(&mut self, offset: Offset) {
        println!("  [translate] offset: {:?}", offset);
    }

    fn rotate(&mut self, angle: f32) {
        println!("  [rotate] angle: {}", angle);
    }

    fn scale(&mut self, sx: f32, sy: f32) {
        println!("  [scale] sx: {}, sy: {}", sx, sy);
    }

    fn clip_rect(&mut self, rect: Rect) {
        println!("  [clip_rect] rect: {:?}", rect);
    }

    fn clip_rrect(&mut self, rrect: flui_engine::RRect) {
        println!("  [clip_rrect] rect: {:?}, radius: {}", rrect.rect, rrect.corner_radius);
    }

    fn set_opacity(&mut self, opacity: f32) {
        println!("  [set_opacity] opacity: {}", opacity);
    }
}
