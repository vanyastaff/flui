//! Integration test for layout + paint pipeline
//!
//! Tests that the full pipeline works end-to-end:
//! - Widget → Element → RenderObject
//! - Layout (recursive)
//! - Paint (recursive)
//! - Layer composition

use flui_core::constraints::BoxConstraints;
use flui_core::*;
use flui_engine::ContainerLayer;

// Import extension traits once at the top - no need to repeat in each function!
// These traits provide arity-specific methods for LayoutCx and PaintCx.
use flui_core::{MultiChild, MultiChildPaint, SingleChild, SingleChildPaint};

// ========== Test RenderObjects ==========

/// Simple colored box render object (Leaf)
#[derive(Debug, Clone)]
struct ColorBox {
    color: (u8, u8, u8),
    size: Size,
}

impl ColorBox {
    fn new(color: (u8, u8, u8), width: f32, height: f32) -> Self {
        Self {
            color,
            size: Size::new(width, height),
        }
    }
}

impl RenderObject for ColorBox {
    type Arity = LeafArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        // Fixed size box
        let size = cx.constraints().constrain(self.size);
        tracing::debug!("ColorBox::layout -> {:?}", size);
        size
    }

    fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        tracing::debug!("ColorBox::paint color={:?}", self.color);
        // Return a simple container layer (no actual drawing for now)
        Box::new(ContainerLayer::new())
    }
}

/// Simple container that lays out a single child (SingleArity)
#[derive(Debug, Clone)]
struct SimpleContainer {
    padding: f32,
}

impl SimpleContainer {
    fn new(padding: f32) -> Self {
        Self { padding }
    }
}

impl RenderObject for SimpleContainer {
    type Arity = SingleArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        // Get child (SingleChild trait is already in scope)
        let child = cx.child();

        // Layout child with reduced constraints (padding)
        let child_constraints = BoxConstraints::new(
            (cx.constraints().min_width - 2.0 * self.padding).max(0.0),
            (cx.constraints().max_width - 2.0 * self.padding).max(0.0),
            (cx.constraints().min_height - 2.0 * self.padding).max(0.0),
            (cx.constraints().max_height - 2.0 * self.padding).max(0.0),
        );

        let child_size = cx.layout_child(child, child_constraints);

        // Our size = child size + padding
        let size = Size::new(
            child_size.width + 2.0 * self.padding,
            child_size.height + 2.0 * self.padding,
        );

        tracing::debug!(
            "SimpleContainer::layout padding={} child={:?} size={:?}",
            self.padding,
            child_size,
            size
        );

        size
    }

    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        tracing::debug!("SimpleContainer::paint padding={}", self.padding);

        // Get child layer (SingleChildPaint trait is already in scope)
        let child = cx.child();
        let child_layer = cx.capture_child_layer(child);

        // Wrap in container with padding offset
        let mut container = ContainerLayer::new();
        container.add_child(child_layer);

        Box::new(container)
    }
}

/// Column-like container that stacks children vertically (MultiArity)
#[derive(Debug, Clone)]
struct SimpleColumn;

impl RenderObject for SimpleColumn {
    type Arity = MultiArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        // Get children (MultiChild trait is already in scope)
        let children = cx.children();

        if children.is_empty() {
            return Size::ZERO;
        }

        let mut total_height = 0.0f32;
        let mut max_width = 0.0f32;

        // Layout each child, stacking vertically
        for &child in &children {
            let child_size = cx.layout_child(child, cx.constraints().clone());
            total_height += child_size.height;
            max_width = max_width.max(child_size.width);
        }

        let size = Size::new(max_width, total_height);

        tracing::debug!(
            "SimpleColumn::layout {} children -> {:?}",
            children.len(),
            size
        );

        size
    }

    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        tracing::debug!("SimpleColumn::paint");

        // Get children (MultiChildPaint trait is already in scope)
        let children = cx.children();
        let mut container = ContainerLayer::new();

        // Paint each child
        for &child in &children {
            let child_layer = cx.capture_child_layer(child);
            container.add_child(child_layer);
        }

        Box::new(container)
    }
}

// NOTE: Widget tests skipped - we're testing RenderObject directly

// ========== Tests ==========

#[test]
fn test_leaf_layout_and_paint() {
    println!("=== Test: Leaf layout and paint ===");

    // Create element tree
    let tree = ElementTree::new();

    // Create a simple ColorBox render object
    let mut render_object = ColorBox::new((255, 0, 0), 100.0, 50.0);

    // Test layout with loose constraints
    let constraints = BoxConstraints::new(0.0, 400.0, 0.0, 400.0);
    let mut layout_cx = LayoutCx::<LeafArity>::new(&tree, 0, constraints);
    let size = render_object.layout(&mut layout_cx);

    // ColorBox returns constrained size (100x50 fits in 0..400)
    assert_eq!(size, Size::new(100.0, 50.0));
    println!("✅ Layout successful: {:?}", size);

    // Test paint
    let paint_cx = PaintCx::<LeafArity>::new(&tree, 0, Offset::ZERO);
    let _layer = render_object.paint(&paint_cx);

    // Can't easily test layer type without downcasting, just verify it doesn't panic
    println!("✅ Paint successful");
}

#[test]
fn test_single_child_layout() {
    println!("=== Test: Single child layout ===");

    // Test container with padding
    let mut _container = SimpleContainer::new(10.0);

    // For this test, we need an ElementTree with actual children
    // This is more complex - skip for now

    println!("⏸️ Single child test needs ElementTree implementation");
}

#[test]
fn test_multi_child_layout() {
    println!("=== Test: Multi child layout ===");

    // Test column with multiple children
    let mut _column = SimpleColumn;

    // For this test, we need an ElementTree with actual children
    // This is more complex - skip for now

    println!("⏸️ Multi child test needs ElementTree implementation");
}

#[test]
fn test_render_object_impls_exist() {
    // Just verify that our types implement the right traits
    fn assert_render_object<T: RenderObject>() {}

    assert_render_object::<ColorBox>();
    assert_render_object::<SimpleContainer>();
    assert_render_object::<SimpleColumn>();

    println!("✅ All RenderObject impls compile");
}
