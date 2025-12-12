//! Basic usage example for flui_rendering
//!
//! This example demonstrates the core architecture of the rendering system.

use flui_rendering::prelude::*;
use std::any::Any;

// Example render object using BoxProtocol
#[derive(Debug)]
struct ExampleRenderBox {
    proxy: ProxyBox,
}

impl ExampleRenderBox {
    fn new() -> Self {
        Self {
            proxy: ProxyBox::new(),
        }
    }
}

// Implement RenderObject trait
impl RenderObject for ExampleRenderBox {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// Implement RenderBox trait
impl RenderBox for ExampleRenderBox {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        // Simple layout: use smallest size if no child
        if let Some(child) = self.proxy.child_mut() {
            let size = child.perform_layout(constraints);
            self.proxy.set_geometry(size);
            size
        } else {
            let size = constraints.smallest();
            self.proxy.set_geometry(size);
            size
        }
    }

    fn size(&self) -> Size {
        *self.proxy.geometry()
    }

    fn paint(&self, _context: &mut dyn PaintingContext, _offset: Offset) {
        // Paint implementation would go here
        println!("Painting at offset: {:?}", _offset);
    }
}

fn main() {
    println!("FLUI Rendering - Basic Usage Example");
    println!("======================================\n");

    // Create a render object
    let mut render_box = ExampleRenderBox::new();
    println!("✓ Created ExampleRenderBox");

    // Create constraints
    let constraints = BoxConstraints::new(100.0, 200.0, 50.0, 150.0);
    println!("✓ Created constraints: {}", constraints);

    // Perform layout
    let size = render_box.perform_layout(constraints);
    println!("✓ Layout complete. Size: {:?}", size);

    // Check size
    assert_eq!(render_box.size(), size);
    println!("✓ Size matches");

    println!("\n=== Protocol System Demo ===\n");

    // Demonstrate type safety
    println!("BoxProtocol name: {}", BoxProtocol::name());
    println!("SliverProtocol name: {}", SliverProtocol::name());

    // Show that containers are type-safe
    let _box_child: BoxChild = BoxChild::new();
    let _sliver_child: SliverChild = SliverChild::new();
    println!("✓ Type-safe containers created");

    println!("\n=== Success! ===");
    println!("The architecture is working correctly!");
}
