//! Example demonstrating the new simplified View API
//!
//! This example shows the new simplified View API with no GATs,
//! automatic tree management, and hooks for state.

use flui_core::{AnyView, BuildContext, IntoElement, LeafSingleView};

// ============================================================================
// Example 1: Simple Text Component
// ============================================================================

/// Simple text view using the new API
#[derive(Clone)]
struct SimpleText {
    text: String,
}

impl View for SimpleText {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // In real code, this would use RenderParagraph from flui_rendering
        // For now, we just demonstrate the API pattern
        Leaf(MockTextRender { text: self.text }, ())
    }
}

// ============================================================================
// Example 2: Padding Component (Single Child)
// ============================================================================

#[derive(Clone)]
struct SimplePadding {
    padding: f32,
    child: Option<Box<dyn AnyView>>,
}

impl View for SimplePadding {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Using tuple syntax: (Render, Option<child>)
        (MockPaddingRender {
            padding: self.padding,
        }, self.child)
    }
}

// ============================================================================
// Example 3: Button Component (Composition)
// ============================================================================

#[derive(Clone)]
struct SimpleButton {
    label: String,
}

impl View for SimpleButton {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Compose other views
        SimplePadding {
            padding: 16.0,
            child: Some(Box::new(SimpleText { text: self.label })),
        }
    }
}

// ============================================================================
// Mock Render Objects (for demonstration)
// ============================================================================

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};
use flui_engine::BoxedLayer;
use flui_types::{BoxConstraints, Offset, Size};

#[derive(Debug)]
struct MockTextRender {
    text: String,
}

impl Render for MockTextRender {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let constraints = ctx.constraints;
        constraints.constrain(Size::new(100.0, 20.0))
    }

    fn paint(&self, _ctx: &PaintContext) -> BoxedLayer {
        Box::new(flui_engine::ContainerLayer::new())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Exact(0)
    }
}

#[derive(Debug)]
struct MockPaddingRender {
    padding: f32,
}

impl Render for MockPaddingRender {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;
        let child_size = tree.layout_child(child_id, constraints);
        Size::new(
            child_size.width + self.padding * 2.0,
            child_size.height + self.padding * 2.0,
        )
    }

    fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let offset = ctx.offset;
        tree.paint_child(
            child_id,
            Offset::new(offset.dx + self.padding, offset.dy + self.padding),
        )
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Exact(1)
    }
}

// ============================================================================
// Main (just for compilation test)
// ============================================================================

fn main() {
    println!("=== Simplified View API Example ===");
    println!();
    println!("This example demonstrates the new simplified View API.");
    println!();
    println!("Key improvements:");
    println!("  ✓ No GAT State/Element types");
    println!("  ✓ No manual tree management");
    println!("  ✓ No explicit rebuild() method");
    println!("  ✓ Chainable builder API");
    println!("  ✓ Tuple syntax for single-child renders");
    println!("  ✓ Hooks for state management");
    println!();
    println!("OLD (View trait with GATs):");
    println!("  impl View for Padding {{");
    println!("      type Element = Element;");
    println!("      type State = Option<Box<dyn Any>>;");
    println!("      fn build(self, ctx: &mut BuildContext) -> (Element, State) {{");
    println!("          // 20+ lines of boilerplate...");
    println!("      }}");
    println!("      fn rebuild(...) -> ChangeFlags {{ ... }}");
    println!("  }}");
    println!();
    println!("NEW (Simplified View trait):");
    println!("  impl View for Padding {{");
    println!("      fn build(self, ctx: &BuildContext) -> impl IntoElement {{");
    println!("          Single(RenderPadding::new(self.padding, ()))");
    println!("              .child(self.child)");
    println!("      }}");
    println!("  }}");
    println!();
    println!("✨ 20 lines → 5 lines! ✨");
    println!();
    println!("HOOKS работают:");
    println!("  impl View for Counter {{");
    println!("      fn build(self, ctx: &BuildContext) -> impl IntoElement {{");
    println!("          let count = use_signal(ctx, 0);  // ← Hooks!");
    println!("          Column::new()");
    println!("              .child(Text::new(format!(\"Count: {{}}\", count.get())))");
    println!("      }}");
    println!("  }}");
}
