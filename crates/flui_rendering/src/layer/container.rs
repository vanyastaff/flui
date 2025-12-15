//! Container layers that can hold child layers.

use std::any::Any;

use flui_types::{Offset, Rect};

use super::base::{EngineLayer, Layer, LayerId, SceneBuilder};

// ============================================================================
// ContainerLayer
// ============================================================================

/// A layer that can contain child layers.
///
/// This is the base type for layers that have children. Child layers
/// are painted in order from first to last.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `ContainerLayer` class.
#[derive(Debug)]
pub struct ContainerLayer {
    /// Unique identifier.
    id: LayerId,

    /// Engine layer handle.
    engine_layer: Option<EngineLayer>,

    /// Child layers.
    children: Vec<Box<dyn Layer>>,

    /// Whether this layer needs to be added to the scene.
    needs_add_to_scene: bool,

    /// Cached bounds.
    cached_bounds: Option<Rect>,
}

impl Default for ContainerLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl ContainerLayer {
    /// Creates a new container layer.
    pub fn new() -> Self {
        Self {
            id: LayerId::new(),
            engine_layer: None,
            children: Vec::new(),
            needs_add_to_scene: true,
            cached_bounds: None,
        }
    }

    /// Returns the first child layer.
    pub fn first_child(&self) -> Option<&dyn Layer> {
        self.children.first().map(|c| c.as_ref())
    }

    /// Returns the last child layer.
    pub fn last_child(&self) -> Option<&dyn Layer> {
        self.children.last().map(|c| c.as_ref())
    }

    /// Returns an iterator over child layers.
    pub fn children(&self) -> impl Iterator<Item = &dyn Layer> {
        self.children.iter().map(|c| c.as_ref())
    }

    /// Appends a child layer.
    pub fn append(&mut self, child: Box<dyn Layer>) {
        self.children.push(child);
        self.cached_bounds = None;
        self.needs_add_to_scene = true;
    }

    /// Removes all child layers.
    pub fn remove_all_children(&mut self) {
        self.children.clear();
        self.cached_bounds = None;
        self.needs_add_to_scene = true;
    }

    /// Calculates the bounds of all children.
    fn calculate_bounds(&self) -> Rect {
        if self.children.is_empty() {
            return Rect::ZERO;
        }

        let mut bounds = Rect::ZERO;
        for child in &self.children {
            let child_bounds = child.bounds();
            if bounds == Rect::ZERO {
                bounds = child_bounds;
            } else {
                bounds = bounds.expand_to_include(&child_bounds);
            }
        }
        bounds
    }

    /// Adds children to the scene.
    pub(crate) fn add_children_to_scene(
        &mut self,
        builder: &mut SceneBuilder,
        child_offset: Offset,
    ) {
        for child in &mut self.children {
            child.add_to_scene(builder, child_offset);
        }
    }
}

impl Layer for ContainerLayer {
    fn id(&self) -> LayerId {
        self.id
    }

    fn engine_layer(&self) -> Option<&EngineLayer> {
        self.engine_layer.as_ref()
    }

    fn set_engine_layer(&mut self, layer: Option<EngineLayer>) {
        self.engine_layer = layer;
    }

    fn parent(&self) -> Option<&dyn Layer> {
        None // ContainerLayer doesn't track parent
    }

    fn remove(&mut self) {
        // Container layers don't have parent tracking in this implementation
    }

    fn needs_add_to_scene(&self) -> bool {
        self.needs_add_to_scene
    }

    fn mark_needs_add_to_scene(&mut self) {
        self.needs_add_to_scene = true;
    }

    fn update_subtree_needs_add_to_scene(&mut self) {
        self.needs_add_to_scene = self.children.iter().any(|c| c.needs_add_to_scene());
    }

    fn add_to_scene(&mut self, builder: &mut SceneBuilder, layer_offset: Offset) {
        self.add_children_to_scene(builder, layer_offset);
        self.needs_add_to_scene = false;
    }

    fn find(&self, offset: Offset) -> Option<&dyn Layer> {
        // Search children in reverse order (top to bottom)
        for child in self.children.iter().rev() {
            if let Some(found) = child.find(offset) {
                return Some(found);
            }
        }
        None
    }

    fn bounds(&self) -> Rect {
        self.cached_bounds
            .unwrap_or_else(|| self.calculate_bounds())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// OffsetLayer
// ============================================================================

/// A container layer with a translation offset.
///
/// This is the most common layer type for render objects that need
/// their own compositing layer with an offset.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `OffsetLayer` class.
#[derive(Debug)]
pub struct OffsetLayer {
    /// The container layer data.
    container: ContainerLayer,

    /// The offset to apply to children.
    offset: Offset,
}

impl Default for OffsetLayer {
    fn default() -> Self {
        Self::new(Offset::ZERO)
    }
}

impl OffsetLayer {
    /// Creates a new offset layer with the given offset.
    pub fn new(offset: Offset) -> Self {
        Self {
            container: ContainerLayer::new(),
            offset,
        }
    }

    /// Returns the offset.
    pub fn offset(&self) -> Offset {
        self.offset
    }

    /// Sets the offset.
    pub fn set_offset(&mut self, offset: Offset) {
        if self.offset != offset {
            self.offset = offset;
            self.container.mark_needs_add_to_scene();
        }
    }

    /// Returns the first child layer.
    pub fn first_child(&self) -> Option<&dyn Layer> {
        self.container.first_child()
    }

    /// Returns the last child layer.
    pub fn last_child(&self) -> Option<&dyn Layer> {
        self.container.last_child()
    }

    /// Appends a child layer.
    pub fn append(&mut self, child: Box<dyn Layer>) {
        self.container.append(child);
    }

    /// Removes all child layers.
    pub fn remove_all_children(&mut self) {
        self.container.remove_all_children();
    }
}

impl Layer for OffsetLayer {
    fn id(&self) -> LayerId {
        self.container.id()
    }

    fn engine_layer(&self) -> Option<&EngineLayer> {
        self.container.engine_layer()
    }

    fn set_engine_layer(&mut self, layer: Option<EngineLayer>) {
        self.container.set_engine_layer(layer);
    }

    fn parent(&self) -> Option<&dyn Layer> {
        self.container.parent()
    }

    fn remove(&mut self) {
        self.container.remove();
    }

    fn needs_add_to_scene(&self) -> bool {
        self.container.needs_add_to_scene()
    }

    fn mark_needs_add_to_scene(&mut self) {
        self.container.mark_needs_add_to_scene();
    }

    fn update_subtree_needs_add_to_scene(&mut self) {
        self.container.update_subtree_needs_add_to_scene();
    }

    fn add_to_scene(&mut self, builder: &mut SceneBuilder, layer_offset: Offset) {
        // Push offset
        let effective_offset = layer_offset + self.offset;
        builder.push_offset(effective_offset.dx, effective_offset.dy);

        // Add children
        self.container.add_children_to_scene(builder, Offset::ZERO);

        // Pop
        builder.pop();

        self.container.needs_add_to_scene = false;
    }

    fn find(&self, offset: Offset) -> Option<&dyn Layer> {
        // Transform the offset and search children
        let local_offset = offset - self.offset;
        self.container.find(local_offset)
    }

    fn bounds(&self) -> Rect {
        let child_bounds = self.container.bounds();
        child_bounds.translate_offset(self.offset)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_layer_new() {
        let layer = ContainerLayer::new();
        assert!(layer.first_child().is_none());
        assert!(layer.needs_add_to_scene());
    }

    #[test]
    fn test_offset_layer_new() {
        let layer = OffsetLayer::new(Offset::new(10.0, 20.0));
        assert_eq!(layer.offset(), Offset::new(10.0, 20.0));
    }

    #[test]
    fn test_offset_layer_set_offset() {
        let mut layer = OffsetLayer::new(Offset::ZERO);
        layer.set_offset(Offset::new(5.0, 10.0));
        assert_eq!(layer.offset(), Offset::new(5.0, 10.0));
    }

    #[test]
    fn test_container_bounds_empty() {
        let layer = ContainerLayer::new();
        assert_eq!(layer.bounds(), Rect::ZERO);
    }
}
