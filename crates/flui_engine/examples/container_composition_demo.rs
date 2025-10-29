//! Container Composition Demo
//!
//! This demo showcases layer composition that Container uses under the hood:
//! 1. ContainerLayer - holds child layers
//! 2. PictureLayer - contains drawing commands for decoration
//! 3. OffsetLayer - applies padding/margin offsets
//!
//! The demo demonstrates the low-level layer composition that widgets produce:
//! - PictureLayer for backgrounds and decorations
//! - ContainerLayer for grouping
//! - Layer ordering for proper painting
//!
//! This simulates what RenderDecoratedBox, RenderPadding, and RenderAlign do.

use flui_engine::{App, AppConfig, AppLogic, Painter, Paint};
use flui_engine::layer::{ContainerLayer, PictureLayer, OffsetLayer, Layer};
use flui_types::{Event, Rect, Point, Offset};

struct ContainerCompositionDemo {
    frame_count: u32,
}

impl ContainerCompositionDemo {
    fn new() -> Self {
        Self {
            frame_count: 0,
        }
    }

    /// Create a decorated box layer (simulates RenderDecoratedBox)
    ///
    /// This shows how Container's decoration is painted:
    /// 1. Create PictureLayer
    /// 2. Draw background color/gradient
    /// 3. Draw border
    fn create_decorated_box(&self, rect: Rect, bg_color: [f32; 4], border_color: Option<[f32; 4]>) -> Box<dyn Layer> {
        let mut picture = PictureLayer::new();

        // Draw background
        picture.draw_rect(rect, Paint {
            color: bg_color,
            stroke_width: 0.0,
            anti_alias: true,
        });

        // Draw border if specified
        if let Some(border) = border_color {
            picture.draw_rect(rect, Paint {
                color: border,
                stroke_width: 2.0,
                anti_alias: true,
            });
        }

        Box::new(picture)
    }

    /// Create a decorated box with rounded corners
    fn create_rounded_box(&self, rect: Rect, bg_color: [f32; 4], radius: f32) -> Box<dyn Layer> {
        let mut picture = PictureLayer::new();

        let rrect = flui_engine::painter::RRect {
            rect,
            corner_radius: radius,
        };

        picture.draw_rrect(rrect, Paint {
            color: bg_color,
            stroke_width: 0.0,
            anti_alias: true,
        });

        Box::new(picture)
    }

    /// Create a container with padding (simulates Container → DecoratedBox → Padding composition)
    ///
    /// Layer hierarchy:
    /// ```
    /// ContainerLayer
    ///   ├─ PictureLayer (background decoration)
    ///   └─ OffsetLayer (padding offset)
    ///       └─ PictureLayer (child content)
    /// ```
    fn create_padded_container(&self, x: f32, y: f32, width: f32, height: f32, padding: f32) -> Box<dyn Layer> {
        let mut container = ContainerLayer::new();

        // 1. Background decoration (painted first, behind everything)
        let bg_rect = Rect::from_xywh(x, y, width, height);
        let background = self.create_decorated_box(
            bg_rect,
            [0.2, 0.6, 0.8, 1.0], // Blue
            Some([0.1, 0.3, 0.5, 1.0]), // Darker blue border
        );
        container.add_child(background);

        // 2. Child content (inside padding)
        let mut child_picture = PictureLayer::new();
        let child_rect = Rect::from_xywh(0.0, 0.0, width - 2.0 * padding, height - 2.0 * padding);
        child_picture.draw_rect(child_rect, Paint {
            color: [0.9, 0.9, 0.9, 1.0], // Light gray
            stroke_width: 0.0,
            anti_alias: true,
        });

        // Add text label
        child_picture.draw_text(
            "Padded Content",
            Point::new(child_rect.width() / 2.0 - 40.0, child_rect.height() / 2.0 - 8.0),
            flui_types::typography::TextStyle {
                color: Some(flui_types::Color::rgb(50, 50, 50)),
                font_size: Some(14.0),
                ..Default::default()
            },
        );

        // 3. Padding offset (positions child content)
        let offset_layer = OffsetLayer::new(Box::new(child_picture))
            .with_offset(Offset::new(x + padding, y + padding));
        container.add_child(Box::new(offset_layer));

        Box::new(container)
    }

    /// Create aligned container (simulates Container → Align composition)
    ///
    /// Layer hierarchy:
    /// ```
    /// ContainerLayer
    ///   ├─ PictureLayer (container background)
    ///   └─ OffsetLayer (alignment offset)
    ///       └─ PictureLayer (aligned child)
    /// ```
    fn create_aligned_container(&self, x: f32, y: f32, width: f32, height: f32) -> Box<dyn Layer> {
        let mut container = ContainerLayer::new();

        // 1. Container background
        let bg = self.create_decorated_box(
            Rect::from_xywh(x, y, width, height),
            [0.95, 0.95, 0.95, 1.0], // Light gray
            Some([0.7, 0.7, 0.7, 1.0]), // Gray border
        );
        container.add_child(bg);

        // 2. Aligned child (top-right alignment)
        let child_width = 80.0;
        let child_height = 60.0;
        let align_x = width - child_width; // Right
        let align_y = 0.0; // Top

        let mut child_picture = PictureLayer::new();
        child_picture.draw_rect(
            Rect::from_xywh(0.0, 0.0, child_width, child_height),
            Paint {
                color: [0.9, 0.3, 0.2, 1.0], // Red
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        child_picture.draw_text(
            "Aligned",
            Point::new(15.0, 25.0),
            flui_types::typography::TextStyle {
                color: Some(flui_types::Color::WHITE),
                font_size: Some(12.0),
                ..Default::default()
            },
        );

        let offset_layer = OffsetLayer::new(Box::new(child_picture))
            .with_offset(Offset::new(x + align_x, y + align_y));
        container.add_child(Box::new(offset_layer));

        Box::new(container)
    }

    /// Create complex nested container (simulates full Container composition)
    ///
    /// Layer hierarchy (what Container::build() produces):
    /// ```
    /// ContainerLayer (outermost)
    ///   ├─ PictureLayer (decoration - gradient/color)
    ///   └─ OffsetLayer (padding)
    ///       └─ ContainerLayer (child container)
    ///           ├─ PictureLayer (child decoration)
    ///           └─ OffsetLayer (child padding)
    ///               └─ PictureLayer (child content)
    /// ```
    fn create_complex_container(&self, x: f32, y: f32) -> Box<dyn Layer> {
        let mut outer_container = ContainerLayer::new();

        // Outer decoration (rounded box with gradient-like effect)
        let outer_rect = Rect::from_xywh(x, y, 300.0, 200.0);
        let outer_bg = self.create_rounded_box(
            outer_rect,
            [0.3, 0.5, 0.8, 1.0], // Blue
            16.0, // Corner radius
        );
        outer_container.add_child(outer_bg);

        // Inner container content
        let mut content_picture = PictureLayer::new();

        // Title
        content_picture.draw_text(
            "Complex Container",
            Point::new(0.0, 0.0),
            flui_types::typography::TextStyle {
                color: Some(flui_types::Color::rgb(50, 50, 50)),
                font_size: Some(18.0),
                ..Default::default()
            },
        );

        // Description
        content_picture.draw_text(
            "• Outer decoration layer",
            Point::new(0.0, 35.0),
            flui_types::typography::TextStyle {
                color: Some(flui_types::Color::rgb(80, 80, 80)),
                font_size: Some(12.0),
                ..Default::default()
            },
        );

        content_picture.draw_text(
            "• Outer padding offset",
            Point::new(0.0, 55.0),
            flui_types::typography::TextStyle {
                color: Some(flui_types::Color::rgb(80, 80, 80)),
                font_size: Some(12.0),
                ..Default::default()
            },
        );

        content_picture.draw_text(
            "• Inner decoration layer",
            Point::new(0.0, 75.0),
            flui_types::typography::TextStyle {
                color: Some(flui_types::Color::rgb(80, 80, 80)),
                font_size: Some(12.0),
                ..Default::default()
            },
        );

        content_picture.draw_text(
            "• Inner padding offset",
            Point::new(0.0, 95.0),
            flui_types::typography::TextStyle {
                color: Some(flui_types::Color::rgb(80, 80, 80)),
                font_size: Some(12.0),
                ..Default::default()
            },
        );

        // Inner padding offset
        let inner_offset = OffsetLayer::new(Box::new(content_picture))
            .with_offset(Offset::new(16.0, 16.0));

        // Inner container
        let mut inner_container = ContainerLayer::new();
        let inner_rect = Rect::from_xywh(0.0, 0.0, 260.0, 160.0);
        let inner_bg = self.create_rounded_box(
            inner_rect,
            [0.9, 0.9, 0.9, 1.0], // Light gray
            12.0,
        );
        inner_container.add_child(inner_bg);
        inner_container.add_child(Box::new(inner_offset));

        // Outer padding offset
        let padding = 20.0;
        let outer_offset = OffsetLayer::new(Box::new(inner_container))
            .with_offset(Offset::new(x + padding, y + padding));
        outer_container.add_child(Box::new(outer_offset));

        Box::new(outer_container)
    }
}

impl AppLogic for ContainerCompositionDemo {
    fn on_event(&mut self, event: &Event) -> bool {
        match event {
            Event::Window(window_event) => {
                if let flui_types::WindowEvent::CloseRequested = window_event {
                    return false; // Exit
                }
            }
            _ => {}
        }
        true
    }

    fn update(&mut self, _delta_time: f32) {
        self.frame_count += 1;
    }

    fn render(&mut self, painter: &mut dyn Painter) {
        // Background
        painter.rect(
            Rect::from_xywh(0.0, 0.0, 1400.0, 900.0),
            &Paint {
                color: [0.96, 0.96, 0.96, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // Title
        painter.text(
            "Container Layer Composition Demo",
            Point::new(400.0, 30.0),
            32.0,
            &Paint { color: [0.2, 0.3, 0.4, 1.0], ..Default::default() }
        );

        painter.text(
            "Demonstrating Widget → RenderObject → Layer chain",
            Point::new(420.0, 70.0),
            16.0,
            &Paint { color: [0.5, 0.5, 0.5, 1.0], ..Default::default() }
        );

        // Section 1: Basic Decoration
        painter.text(
            "1. Basic Decoration (RenderDecoratedBox)",
            Point::new(50.0, 120.0),
            18.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() }
        );

        let basic_box = self.create_decorated_box(
            Rect::from_xywh(50.0, 150.0, 200.0, 150.0),
            [0.3, 0.7, 0.3, 1.0], // Green
            Some([0.2, 0.5, 0.2, 1.0]), // Dark green border
        );
        basic_box.paint(painter);

        // Section 2: Rounded Decoration
        painter.text(
            "2. Rounded Box (Border Radius)",
            Point::new(300.0, 120.0),
            18.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() }
        );

        let rounded = self.create_rounded_box(
            Rect::from_xywh(300.0, 150.0, 200.0, 150.0),
            [0.8, 0.4, 0.2, 1.0], // Orange
            20.0,
        );
        rounded.paint(painter);

        // Section 3: Padded Container
        painter.text(
            "3. Padded Container (Decoration + Padding)",
            Point::new(550.0, 120.0),
            18.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() }
        );

        let padded = self.create_padded_container(550.0, 150.0, 200.0, 150.0, 20.0);
        padded.paint(painter);

        // Section 4: Aligned Container
        painter.text(
            "4. Aligned Child (Decoration + Alignment)",
            Point::new(800.0, 120.0),
            18.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() }
        );

        let aligned = self.create_aligned_container(800.0, 150.0, 200.0, 150.0);
        aligned.paint(painter);

        // Section 5: Complex Nested
        painter.text(
            "5. Complex Nested Container (Full Composition)",
            Point::new(50.0, 350.0),
            18.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() }
        );

        let complex = self.create_complex_container(50.0, 380.0);
        complex.paint(painter);

        // Layer hierarchy explanation
        painter.text(
            "Layer Composition Hierarchy:",
            Point::new(400.0, 380.0),
            18.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() }
        );

        let explanation = [
            "ContainerLayer",
            "  ├─ PictureLayer (decoration)",
            "  └─ OffsetLayer (padding/margin)",
            "      └─ ContainerLayer (child)",
            "          ├─ PictureLayer (child decoration)",
            "          └─ OffsetLayer (child padding)",
            "              └─ PictureLayer (content)",
        ];

        for (i, line) in explanation.iter().enumerate() {
            painter.text(
                line,
                Point::new(400.0, 420.0 + i as f32 * 25.0),
                14.0,
                &Paint { color: [0.3, 0.3, 0.3, 1.0], ..Default::default() }
            );
        }

        // Frame counter
        painter.text(
            &format!("Frame: {}", self.frame_count),
            Point::new(1250.0, 30.0),
            14.0,
            &Paint { color: [0.5, 0.5, 0.5, 1.0], ..Default::default() }
        );
    }
}

#[cfg(feature = "egui")]
fn main() {
    println!("=== Container Layer Composition Demo ===");
    println!();
    println!("This demo shows the layer composition that Container produces:");
    println!();
    println!("Container Widget Composition:");
    println!("  Container::build() creates:");
    println!("    → SizedBox (width/height constraints)");
    println!("    → Padding (margin)");
    println!("    → DecoratedBox (decoration/color)");
    println!("    → Align (alignment)");
    println!("    → Padding (inner padding)");
    println!("    → child");
    println!();
    println!("RenderObject Layer Generation:");
    println!("  RenderDecoratedBox::paint() creates:");
    println!("    → ContainerLayer");
    println!("      ├─ PictureLayer (background)");
    println!("      └─ child layer");
    println!();
    println!("  RenderPadding::paint() creates:");
    println!("    → OffsetLayer (offset by padding)");
    println!("      └─ child layer");
    println!();

    let app = App::with_config(AppConfig::new().backend(flui_engine::Backend::Egui))
        .title("Container Layer Composition Demo")
        .size(1400, 900);

    app.run(ContainerCompositionDemo::new()).expect("Failed to run app");
}

#[cfg(not(feature = "egui"))]
fn main() {
    panic!("This example requires the 'egui' feature");
}
