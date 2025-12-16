//! Clip layers for constraining rendering to geometric shapes.

use std::any::Any;

use flui_types::painting::Path;
use flui_types::{Offset, Point, RRect, Rect};

use super::base::{EngineLayer, Layer, LayerId, SceneBuilder};
use super::container::ContainerLayer;

// ============================================================================
// Clip Behavior
// ============================================================================

/// How to clip content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Clip {
    /// No clipping.
    #[default]
    None,
    /// Clip to bounds with hard edges (no anti-aliasing).
    HardEdge,
    /// Clip to bounds with anti-aliased edges.
    AntiAlias,
    /// Clip to bounds with anti-aliased edges and save the layer.
    AntiAliasWithSaveLayer,
}

impl Clip {
    /// Returns whether clipping is enabled.
    #[inline]
    pub fn is_clipping(&self) -> bool {
        !matches!(self, Clip::None)
    }

    /// Alias for [`is_clipping`](Self::is_clipping).
    #[inline]
    pub fn clips(&self) -> bool {
        self.is_clipping()
    }

    /// Returns whether this clip behavior uses anti-aliasing.
    #[inline]
    pub fn is_anti_alias(&self) -> bool {
        matches!(self, Clip::AntiAlias | Clip::AntiAliasWithSaveLayer)
    }

    /// Returns whether this clip behavior uses a save layer.
    #[inline]
    pub fn uses_save_layer(&self) -> bool {
        matches!(self, Clip::AntiAliasWithSaveLayer)
    }
}

// ============================================================================
// ClipRectLayer
// ============================================================================

/// A layer that clips its children to a rectangle.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `ClipRectLayer` class.
#[derive(Debug)]
pub struct ClipRectLayer {
    /// The container layer data.
    container: ContainerLayer,

    /// The clip rectangle in local coordinates.
    clip_rect: Rect,

    /// The clip behavior.
    clip_behavior: Clip,
}

impl ClipRectLayer {
    /// Creates a new clip rect layer.
    pub fn new(clip_rect: Rect, clip_behavior: Clip) -> Self {
        Self {
            container: ContainerLayer::new(),
            clip_rect,
            clip_behavior,
        }
    }

    /// Returns the clip rectangle.
    pub fn clip_rect(&self) -> Rect {
        self.clip_rect
    }

    /// Sets the clip rectangle.
    pub fn set_clip_rect(&mut self, rect: Rect) {
        if self.clip_rect != rect {
            self.clip_rect = rect;
            self.container.mark_needs_add_to_scene();
        }
    }

    /// Returns the clip behavior.
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    /// Sets the clip behavior.
    pub fn set_clip_behavior(&mut self, behavior: Clip) {
        if self.clip_behavior != behavior {
            self.clip_behavior = behavior;
            self.container.mark_needs_add_to_scene();
        }
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

impl Layer for ClipRectLayer {
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
        if !self.clip_behavior.is_clipping() {
            // No clipping, just add children
            self.container.add_to_scene(builder, layer_offset);
            return;
        }

        // Push clip rect
        let offset_rect = self.clip_rect.translate_offset(layer_offset);
        builder.push_clip_rect(offset_rect);

        // Add children
        self.container.add_to_scene(builder, layer_offset);

        // Pop
        builder.pop();
    }

    fn find(&self, offset: Offset) -> Option<&dyn Layer> {
        // Only find if within clip rect
        let point = Point::new(offset.dx, offset.dy);
        if self.clip_behavior.is_clipping() && !self.clip_rect.contains(point) {
            return None;
        }
        self.container.find(offset)
    }

    fn bounds(&self) -> Rect {
        if self.clip_behavior.is_clipping() {
            // Bounds are constrained to clip rect
            let child_bounds = self.container.bounds();
            child_bounds.intersect(self.clip_rect).unwrap_or(Rect::ZERO)
        } else {
            self.container.bounds()
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// ClipRRectLayer
// ============================================================================

/// A layer that clips its children to a rounded rectangle.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `ClipRRectLayer` class.
#[derive(Debug)]
pub struct ClipRRectLayer {
    /// The container layer data.
    container: ContainerLayer,

    /// The clip rounded rectangle in local coordinates.
    clip_rrect: RRect,

    /// The clip behavior.
    clip_behavior: Clip,
}

impl ClipRRectLayer {
    /// Creates a new clip rounded rect layer.
    pub fn new(clip_rrect: RRect, clip_behavior: Clip) -> Self {
        Self {
            container: ContainerLayer::new(),
            clip_rrect,
            clip_behavior,
        }
    }

    /// Returns the clip rounded rectangle.
    pub fn clip_rrect(&self) -> &RRect {
        &self.clip_rrect
    }

    /// Sets the clip rounded rectangle.
    pub fn set_clip_rrect(&mut self, rrect: RRect) {
        if self.clip_rrect != rrect {
            self.clip_rrect = rrect;
            self.container.mark_needs_add_to_scene();
        }
    }

    /// Returns the clip behavior.
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    /// Sets the clip behavior.
    pub fn set_clip_behavior(&mut self, behavior: Clip) {
        if self.clip_behavior != behavior {
            self.clip_behavior = behavior;
            self.container.mark_needs_add_to_scene();
        }
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

impl Layer for ClipRRectLayer {
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
        if !self.clip_behavior.is_clipping() {
            self.container.add_to_scene(builder, layer_offset);
            return;
        }

        // Push clip rrect
        let offset_rrect = self.clip_rrect.translate_offset(layer_offset);
        builder.push_clip_rrect(offset_rrect);

        // Add children
        self.container.add_to_scene(builder, layer_offset);

        // Pop
        builder.pop();
    }

    fn find(&self, offset: Offset) -> Option<&dyn Layer> {
        // Only find if within clip rrect
        let point = Point::new(offset.dx, offset.dy);
        if self.clip_behavior.is_clipping() && !self.clip_rrect.contains(point) {
            return None;
        }
        self.container.find(offset)
    }

    fn bounds(&self) -> Rect {
        if self.clip_behavior.is_clipping() {
            let child_bounds = self.container.bounds();
            child_bounds
                .intersect(self.clip_rrect.rect)
                .unwrap_or(Rect::ZERO)
        } else {
            self.container.bounds()
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// ClipPathLayer
// ============================================================================

/// A layer that clips its children to an arbitrary path.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `ClipPathLayer` class.
#[derive(Debug)]
pub struct ClipPathLayer {
    /// The container layer data.
    container: ContainerLayer,

    /// The clip path in local coordinates.
    clip_path: Path,

    /// The clip behavior.
    clip_behavior: Clip,
}

impl ClipPathLayer {
    /// Creates a new clip path layer.
    pub fn new(clip_path: Path, clip_behavior: Clip) -> Self {
        Self {
            container: ContainerLayer::new(),
            clip_path,
            clip_behavior,
        }
    }

    /// Returns the clip path.
    pub fn clip_path(&self) -> &Path {
        &self.clip_path
    }

    /// Sets the clip path.
    pub fn set_clip_path(&mut self, path: Path) {
        self.clip_path = path;
        self.container.mark_needs_add_to_scene();
    }

    /// Returns the clip behavior.
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    /// Sets the clip behavior.
    pub fn set_clip_behavior(&mut self, behavior: Clip) {
        if self.clip_behavior != behavior {
            self.clip_behavior = behavior;
            self.container.mark_needs_add_to_scene();
        }
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

impl Layer for ClipPathLayer {
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
        if !self.clip_behavior.is_clipping() {
            self.container.add_to_scene(builder, layer_offset);
            return;
        }

        // TODO: Add PushClipPath operation when path clipping is supported
        // For now, use bounding rect
        let mut path_copy = self.clip_path.clone();
        let path_bounds = path_copy.bounds();
        let offset_rect = path_bounds.translate_offset(layer_offset);
        builder.push_clip_rect(offset_rect);

        // Add children
        self.container.add_to_scene(builder, layer_offset);

        // Pop
        builder.pop();
    }

    fn find(&self, offset: Offset) -> Option<&dyn Layer> {
        // Only find if within clip path
        let point = Point::new(offset.dx, offset.dy);
        if self.clip_behavior.is_clipping() && !self.clip_path.contains(point) {
            return None;
        }
        self.container.find(offset)
    }

    fn bounds(&self) -> Rect {
        if self.clip_behavior.is_clipping() {
            let child_bounds = self.container.bounds();
            let mut path_copy = self.clip_path.clone();
            child_bounds
                .intersect(path_copy.bounds())
                .unwrap_or(Rect::ZERO)
        } else {
            self.container.bounds()
        }
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
    fn test_clip_behavior() {
        assert!(!Clip::None.is_clipping());
        assert!(Clip::HardEdge.is_clipping());
        assert!(Clip::AntiAlias.is_clipping());
        assert!(Clip::AntiAliasWithSaveLayer.is_clipping());
    }

    #[test]
    fn test_clip_rect_layer() {
        let rect = Rect::from_ltwh(0.0, 0.0, 100.0, 100.0);
        let layer = ClipRectLayer::new(rect, Clip::HardEdge);
        assert_eq!(layer.clip_rect(), rect);
        assert_eq!(layer.clip_behavior(), Clip::HardEdge);
    }

    #[test]
    fn test_clip_rrect_layer() {
        let rrect = RRect::from_rect_xy(Rect::from_ltwh(0.0, 0.0, 100.0, 100.0), 10.0, 10.0);
        let layer = ClipRRectLayer::new(rrect, Clip::AntiAlias);
        assert_eq!(layer.clip_rrect(), &rrect);
        assert_eq!(layer.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_clip_path_layer() {
        let path = Path::new();
        let layer = ClipPathLayer::new(path.clone(), Clip::AntiAliasWithSaveLayer);
        assert_eq!(layer.clip_behavior(), Clip::AntiAliasWithSaveLayer);
    }

    #[test]
    fn test_clip_rect_set_values() {
        let rect1 = Rect::from_ltwh(0.0, 0.0, 100.0, 100.0);
        let rect2 = Rect::from_ltwh(10.0, 10.0, 80.0, 80.0);
        let mut layer = ClipRectLayer::new(rect1, Clip::HardEdge);

        layer.set_clip_rect(rect2);
        assert_eq!(layer.clip_rect(), rect2);

        layer.set_clip_behavior(Clip::AntiAlias);
        assert_eq!(layer.clip_behavior(), Clip::AntiAlias);
    }
}
