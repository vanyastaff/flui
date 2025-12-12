//! Example demonstrating ambassador delegation in render objects
//!
//! This example shows how to use ambassador to automatically implement traits
//! with minimal boilerplate. We'll create RenderOpacity and RenderPadding.

use flui_rendering::prelude::*;
use ambassador::Delegate;
use std::any::Any;

// ============================================================================
// Example 1: RenderOpacity - ProxyBox with delegation
// ============================================================================

/// Render object that applies opacity to its child
///
/// This demonstrates the ProxyBox pattern where parent size == child size.
#[derive(Debug, Delegate)]
#[delegate(SingleChildRenderBox, target = "proxy")]
#[delegate(RenderObject, target = "proxy")]
struct RenderOpacity {
    proxy: ProxyBox,
    opacity: f32,
}

impl RenderOpacity {
    fn new(opacity: f32) -> Self {
        Self {
            proxy: ProxyBox::new(),
            opacity: opacity.clamp(0.0, 1.0),
        }
    }

    fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.0, 1.0);
    }
}

// Marker trait implementation - this is all you need!
impl RenderProxyBox for RenderOpacity {}

// Implement RenderBox for the specific paint behavior
impl RenderBox for RenderOpacity {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        // Delegate to proxy's default implementation
        RenderProxyBox::perform_layout(self, constraints)
    }

    fn size(&self) -> Size {
        *self.proxy.geometry()
    }

    fn paint(&self, context: &mut dyn PaintingContext, offset: Offset) {
        // Custom paint with opacity
        if self.opacity == 0.0 {
            return; // Invisible
        }

        if let Some(_child) = self.proxy.child() {
            if self.opacity == 1.0 {
                // Full opacity - just paint normally
                RenderProxyBox::paint(self, context, offset);
            } else {
                // Apply opacity layer
                println!("Painting with opacity {} at {:?}", self.opacity, offset);
                // In real implementation: context.push_opacity(...)
                RenderProxyBox::paint(self, context, offset);
                // In real implementation: context.pop()
            }
        }
    }
}

// ============================================================================
// Example 2: RenderPadding - ShiftedBox with delegation
// ============================================================================

/// Simple edge insets struct
#[derive(Debug, Clone, Copy, PartialEq)]
struct EdgeInsets {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl EdgeInsets {
    fn all(value: f32) -> Self {
        Self {
            left: value,
            top: value,
            right: value,
            bottom: value,
        }
    }

    fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}

/// Render object that adds padding around its child
///
/// This demonstrates the ShiftedBox pattern with custom positioning.
#[derive(Debug, Delegate)]
#[delegate(SingleChildRenderBox, target = "shifted")]
#[delegate(RenderObject, target = "shifted")]
struct RenderPadding {
    shifted: ShiftedBox,
    padding: EdgeInsets,
}

impl RenderPadding {
    fn new(padding: EdgeInsets) -> Self {
        Self {
            shifted: ShiftedBox::new(),
            padding,
        }
    }
}

// Implement RenderShiftedBox
impl RenderShiftedBox for RenderPadding {
    fn child_offset(&self) -> Offset {
        *self.shifted.offset()
    }
}

// Implement RenderBox for layout
impl RenderBox for RenderPadding {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        // Deflate constraints by padding
        let inner = BoxConstraints::new(
            (constraints.min_width - self.padding.horizontal()).max(0.0),
            (constraints.max_width - self.padding.horizontal()).max(0.0),
            (constraints.min_height - self.padding.vertical()).max(0.0),
            (constraints.max_height - self.padding.vertical()).max(0.0),
        );

        // Layout child
        let child_size = if let Some(child) = self.shifted.child_mut() {
            child.perform_layout(inner)
        } else {
            Size::ZERO
        };

        // Compute final size
        let size = Size::new(
            child_size.width + self.padding.horizontal(),
            child_size.height + self.padding.vertical(),
        );

        // Set child offset
        self.shifted.set_offset(Offset::new(self.padding.left, self.padding.top));
        self.shifted.set_geometry(size);

        size
    }

    fn size(&self) -> Size {
        *self.shifted.geometry()
    }

    fn paint(&self, context: &mut dyn PaintingContext, offset: Offset) {
        // Delegate to shifted box (uses child_offset automatically)
        RenderShiftedBox::paint(self, context, offset);
    }
}

// ============================================================================
// Main demonstration
// ============================================================================

fn main() {
    println!("FLUI Rendering - Ambassador Delegation Example");
    println!("===============================================\n");

    // Example 1: RenderOpacity
    {
        println!("=== Example 1: RenderOpacity (ProxyBox) ===\n");

        let mut opacity = RenderOpacity::new(0.5);
        println!("✓ Created RenderOpacity with opacity: {}", opacity.opacity);

        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let size = opacity.perform_layout(constraints);
        println!("✓ Layout complete. Size: {:?}", size);

        println!("✓ Opacity uses ProxyBox pattern:");
        println!("  - Parent size == child size");
        println!("  - Minimal trait implementation needed");
        println!("  - Ambassador handles delegation\n");
    }

    // Example 2: RenderPadding
    {
        println!("=== Example 2: RenderPadding (ShiftedBox) ===\n");

        let mut padding = RenderPadding::new(EdgeInsets::all(10.0));
        println!("✓ Created RenderPadding with padding: {:?}", padding.padding);

        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let size = padding.perform_layout(constraints);
        println!("✓ Layout complete. Size: {:?}", size);
        println!("✓ Child offset: {:?}", padding.child_offset());

        println!("✓ Padding uses ShiftedBox pattern:");
        println!("  - Custom child positioning");
        println!("  - Parent size != child size");
        println!("  - Offset computed during layout\n");
    }

    // Demonstrate trait hierarchy
    {
        println!("=== Trait Hierarchy Demonstration ===\n");

        let opacity = RenderOpacity::new(0.7);

        println!("RenderOpacity implements:");
        println!("  ✓ RenderProxyBox (marker trait)");
        println!("  ✓ SingleChildRenderBox (via blanket impl)");
        println!("  ✓ RenderBox (explicit impl)");
        println!("  ✓ RenderObject (via delegation)");

        println!("\nWith ambassador delegation:");
        println!("  - child() and child_mut() delegated to proxy");
        println!("  - RenderObject methods delegated to proxy");
        println!("  - Only override what you need to customize!");
    }

    println!("\n=== Success! ===");
    println!("Ambassador delegation working correctly!");
    println!("Minimal boilerplate for complex render objects!");
}
