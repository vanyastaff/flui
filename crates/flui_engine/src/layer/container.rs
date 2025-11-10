//! Container layer - holds multiple child layers

use crate::layer::{base_multi_child::MultiChildLayerBase, BoxedLayer, Layer};
use crate::painter::Painter;
use flui_types::events::{Event, HitTestResult};
use flui_types::{Offset, Rect};

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
    /// Base multi-child layer functionality
    base: MultiChildLayerBase,
}

impl ContainerLayer {
    /// Create a new empty container layer
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a child layer
    pub fn add_child(&mut self, layer: BoxedLayer) {
        self.base.add_child(layer);
    }

    /// Get all child layers
    pub fn children(&self) -> &[BoxedLayer] {
        self.base.children()
    }

    /// Get mutable access to child layers
    pub fn children_mut(&mut self) -> &mut Vec<BoxedLayer> {
        self.base.children_mut()
    }
}


impl Layer for ContainerLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        self.base.paint_children(painter);
    }

    fn bounds(&self) -> Rect {
        self.base.children_bounds_union()
    }

    fn is_visible(&self) -> bool {
        self.base.is_any_child_visible()
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        self.base.hit_test_children_reverse(position, result)
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        self.base.handle_event_children_reverse(event)
    }

    fn dispose(&mut self) {
        self.base.dispose_children();
    }

    fn is_disposed(&self) -> bool {
        self.base.is_disposed()
    }

    fn mark_needs_paint(&mut self) {
        self.base.invalidate_cache();
        for child in self.base.children_mut() {
            child.mark_needs_paint();
        }
    }
}
