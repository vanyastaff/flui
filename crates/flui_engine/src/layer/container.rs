//! Container layer - holds multiple child layers

use crate::layer::{BoxedLayer, Layer};
use crate::painter::Painter;
use flui_types::{Offset, Rect};
use flui_types::events::{Event, HitTestResult};

/// Container layer that holds multiple child layers
///
/// This is the fundamental building block for compositing. It doesn't
/// apply any effects itself - it just holds and paints its children.
///
/// # Example Layer Tree
///
/// ```text
/// ContainerLayer (root)
///   ├─ TransformLayer
///   │   └─ PictureLayer
///   └─ OpacityLayer
///       └─ PictureLayer
/// ```
#[derive(Default)]
pub struct ContainerLayer {
    children: Vec<BoxedLayer>,
}

impl ContainerLayer {
    /// Create a new empty container layer
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
        }
    }

    /// Add a child layer
    pub fn add_child(&mut self, layer: BoxedLayer) {
        self.children.push(layer);
    }

    /// Get all child layers
    pub fn children(&self) -> &[BoxedLayer] {
        &self.children
    }

    /// Get mutable access to child layers
    pub fn children_mut(&mut self) -> &mut Vec<BoxedLayer> {
        &mut self.children
    }
}

impl Layer for ContainerLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        // Paint all children in order
        for child in &self.children {
            if child.is_visible() {
                child.paint(painter);
            }
        }
    }

    fn bounds(&self) -> Rect {
        if self.children.is_empty() {
            return Rect::ZERO;
        }

        // Union of all children bounds
        let mut bounds = self.children[0].bounds();
        for child in &self.children[1..] {
            bounds = bounds.union(&child.bounds());
        }
        bounds
    }

    fn is_visible(&self) -> bool {
        // Container is visible if any child is visible
        self.children.iter().any(|child| child.is_visible())
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // Test children in reverse order (front to back)
        // This ensures that topmost layers are hit first
        let mut hit = false;

        for child in self.children.iter().rev() {
            if child.is_visible() && child.hit_test(position, result) {
                hit = true;
                // Continue testing other children (don't break)
                // This allows multiple overlapping layers to receive events
            }
        }

        hit
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        // Dispatch to children in reverse order (front to back)
        // Stop if any child handles the event
        for child in self.children.iter_mut().rev() {
            if child.handle_event(event) {
                return true; // Event handled, stop propagation
            }
        }

        false // No child handled the event
    }
}
