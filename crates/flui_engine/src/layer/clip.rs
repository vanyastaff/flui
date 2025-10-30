//! Clip layers - apply clipping to child layers
//!
//! This module provides layers that clip their children to specific regions,
//! following Flutter's ClipRectLayer and ClipRRectLayer architecture.

use crate::layer::{base_multi_child::MultiChildLayerBase, BoxedLayer, Layer};
use crate::painter::{Painter, RRect};
use flui_types::events::{Event, HitTestResult};
use flui_types::{Offset, Rect};

// ============================================================================
// ClipRectLayer
// ============================================================================

/// A composited layer that clips its children to a rectangle
///
/// This is the proper Flutter-style ClipRectLayer with full lifecycle management.
///
/// # Example
///
/// ```rust,ignore
/// use flui_engine::layer::{ClipRectLayer, LayerHandle};
///
/// struct ClippingRenderObject {
///     clip_layer: LayerHandle<ClipRectLayer>,
/// }
///
/// impl ClippingRenderObject {
///     fn paint(&mut self, context: &mut PaintingContext, offset: Offset) {
///         // Create or reuse layer
///         let old_layer = self.clip_layer.take();
///         let layer = ClipRectLayer::new(
///             Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
///             old_layer,
///         );
///
///         // Paint children into this layer
///         context.push_layer(layer.clone());
///         self.paint_children(context, offset);
///         context.pop_layer();
///
///         self.clip_layer.set(Some(layer));
///     }
///
///     fn dispose(&mut self) {
///         self.clip_layer.clear(); // Release resources
///     }
/// }
/// ```
pub struct ClipRectLayer {
    /// Base multi-child layer functionality
    base: MultiChildLayerBase,

    /// The clip rectangle in layer coordinates
    clip_rect: Rect,
}

impl ClipRectLayer {
    /// Create a new clip rect layer
    ///
    /// # Arguments
    ///
    /// * `clip_rect` - The rectangle to clip to
    pub fn new(clip_rect: Rect) -> Self {
        Self {
            base: MultiChildLayerBase::new(),
            clip_rect,
        }
    }

    /// Set the clip rectangle
    pub fn set_clip_rect(&mut self, rect: Rect) {
        if self.clip_rect != rect {
            self.clip_rect = rect;
            self.base.invalidate_cache();
            self.mark_needs_paint();
        }
    }

    /// Get the clip rectangle
    pub fn clip_rect(&self) -> Rect {
        self.clip_rect
    }

    /// Add a child layer
    pub fn add_child(&mut self, child: BoxedLayer) {
        self.base.add_child(child);
    }

    /// Remove all children
    pub fn clear_children(&mut self) {
        self.base.clear_children();
    }

    /// Get children
    pub fn children(&self) -> &[BoxedLayer] {
        self.base.children()
    }
}

impl Layer for ClipRectLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        painter.save();
        painter.clip_rect(self.clip_rect);
        self.base.paint_children(painter);
        painter.restore();
    }

    fn bounds(&self) -> Rect {
        if let Some(bounds) = self.base.cached_bounds() {
            return bounds;
        }

        if self.base.is_empty() {
            return self.clip_rect;
        }

        // Union of all children bounds, clipped to clip_rect
        let children_bounds = self.base.children_bounds_union();
        children_bounds
            .intersection(&self.clip_rect)
            .unwrap_or(Rect::ZERO)
    }

    fn is_visible(&self) -> bool {
        !self.base.is_disposed()
            && self.clip_rect.width() > 0.0
            && self.clip_rect.height() > 0.0
            && self.base.is_any_child_visible()
    }

    fn mark_needs_paint(&mut self) {
        self.base.invalidate_cache();
        for child in self.base.children_mut() {
            child.mark_needs_paint();
        }
    }

    fn dispose(&mut self) {
        self.base.dispose_children();
    }

    fn is_disposed(&self) -> bool {
        self.base.is_disposed()
    }

    fn debug_description(&self) -> String {
        format!(
            "ClipRectLayer(clip_rect: {:?}, children: {})",
            self.clip_rect,
            self.base.len()
        )
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // First check if position is within clip rect
        if !self.clip_rect.contains(position) {
            return false; // Outside clip region, no hit
        }

        self.base.hit_test_children_reverse(position, result)
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        self.base.handle_event_children_reverse(event)
    }
}

// ============================================================================
// ClipRRectLayer
// ============================================================================

/// A composited layer that clips its children to a rounded rectangle
///
/// Similar to ClipRectLayer but with rounded corners.
pub struct ClipRRectLayer {
    /// Base multi-child layer functionality
    base: MultiChildLayerBase,

    /// The clip rounded rectangle
    clip_rrect: RRect,
}

impl ClipRRectLayer {
    /// Create a new clip rrect layer
    pub fn new(clip_rrect: RRect) -> Self {
        Self {
            base: MultiChildLayerBase::new(),
            clip_rrect,
        }
    }

    /// Set the clip rounded rectangle
    pub fn set_clip_rrect(&mut self, rrect: RRect) {
        if self.clip_rrect != rrect {
            self.clip_rrect = rrect;
            self.base.invalidate_cache();
            self.mark_needs_paint();
        }
    }

    /// Get the clip rounded rectangle
    pub fn clip_rrect(&self) -> RRect {
        self.clip_rrect
    }

    /// Add a child layer
    pub fn add_child(&mut self, child: BoxedLayer) {
        self.base.add_child(child);
    }

    /// Remove all children
    pub fn clear_children(&mut self) {
        self.base.clear_children();
    }

    /// Get children
    pub fn children(&self) -> &[BoxedLayer] {
        self.base.children()
    }
}

impl Layer for ClipRRectLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        painter.save();
        painter.clip_rrect(self.clip_rrect);
        self.base.paint_children(painter);
        painter.restore();
    }

    fn bounds(&self) -> Rect {
        if let Some(bounds) = self.base.cached_bounds() {
            return bounds;
        }

        if self.base.is_empty() {
            return self.clip_rrect.rect;
        }

        let children_bounds = self.base.children_bounds_union();
        children_bounds
            .intersection(&self.clip_rrect.rect)
            .unwrap_or(Rect::ZERO)
    }

    fn is_visible(&self) -> bool {
        !self.base.is_disposed()
            && self.clip_rrect.rect.width() > 0.0
            && self.clip_rrect.rect.height() > 0.0
            && self.base.is_any_child_visible()
    }

    fn mark_needs_paint(&mut self) {
        self.base.invalidate_cache();
        for child in self.base.children_mut() {
            child.mark_needs_paint();
        }
    }

    fn dispose(&mut self) {
        self.base.dispose_children();
    }

    fn is_disposed(&self) -> bool {
        self.base.is_disposed()
    }

    fn debug_description(&self) -> String {
        format!(
            "ClipRRectLayer(clip_rrect: {:?}, children: {})",
            self.clip_rrect,
            self.base.len()
        )
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // First check if position is within clip rrect (proper rounded rect testing)
        if !self.clip_rrect.contains(position.into()) {
            return false; // Outside clip region, no hit
        }

        self.base.hit_test_children_reverse(position, result)
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        self.base.handle_event_children_reverse(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::Point;

    #[test]
    fn test_clip_rect_layer_lifecycle() {
        let mut layer = ClipRectLayer::new(Rect::from_xywh(0.0, 0.0, 100.0, 100.0));

        assert!(!layer.is_disposed());
        assert_eq!(layer.clip_rect(), Rect::from_xywh(0.0, 0.0, 100.0, 100.0));

        layer.dispose();
        assert!(layer.is_disposed());
    }

    #[test]
    fn test_clip_rect_layer_children() {
        let mut layer = ClipRectLayer::new(Rect::from_xywh(0.0, 0.0, 100.0, 100.0));

        // Add child layers with actual content (so they're visible)
        use crate::layer::PictureLayer;
        use crate::painter::Paint;

        let mut picture1 = PictureLayer::new();
        picture1.draw_rect(Rect::from_xywh(10.0, 10.0, 20.0, 20.0), Paint::default());

        let mut picture2 = PictureLayer::new();
        picture2.draw_rect(Rect::from_xywh(50.0, 50.0, 30.0, 30.0), Paint::default());

        layer.add_child(Box::new(picture1));
        layer.add_child(Box::new(picture2));

        // Verify clipping and visibility (now has visible children)
        assert!(layer.is_visible());
        assert_eq!(layer.clip_rect(), Rect::from_xywh(0.0, 0.0, 100.0, 100.0));
    }

    #[test]
    fn test_clip_rect_layer_update() {
        let mut layer = ClipRectLayer::new(Rect::from_xywh(0.0, 0.0, 50.0, 50.0));

        // Update clip rect
        layer.set_clip_rect(Rect::from_xywh(10.0, 10.0, 100.0, 100.0));
        assert_eq!(layer.clip_rect(), Rect::from_xywh(10.0, 10.0, 100.0, 100.0));
    }

    #[test]
    fn test_clip_rrect_hit_test_center() {
        use crate::layer::PictureLayer;
        use crate::painter::Paint;
        use flui_types::geometry::RRect;

        // Create a rounded rect layer (100x100 with 20px corner radius)
        let rrect = RRect::from_rect_circular(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), 20.0);
        let mut layer = ClipRRectLayer::new(rrect);

        // Add a child that covers the entire area
        let mut picture = PictureLayer::new();
        picture.draw_rect(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), Paint::default());
        layer.add_child(Box::new(picture));

        let mut result = HitTestResult::new();

        // Center should always hit
        assert!(layer.hit_test(Offset::new(50.0, 50.0), &mut result));
    }

    #[test]
    fn test_clip_rrect_hit_test_corners() {
        use crate::layer::PictureLayer;
        use crate::painter::Paint;
        use flui_types::geometry::RRect;

        // Create a rounded rect layer (100x100 with 20px corner radius)
        let rrect = RRect::from_rect_circular(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), 20.0);
        let mut layer = ClipRRectLayer::new(rrect);

        // Add a child that covers the entire area
        let mut picture = PictureLayer::new();
        picture.draw_rect(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), Paint::default());
        layer.add_child(Box::new(picture));

        let mut result = HitTestResult::new();

        // Top-left corner (0,0) should NOT hit (inside rounded corner)
        assert!(!layer.hit_test(Offset::new(0.0, 0.0), &mut result));

        // Point inside the corner radius (but outside rounded edge) should NOT hit
        assert!(!layer.hit_test(Offset::new(2.0, 2.0), &mut result));

        // Point well inside corner radius should hit
        assert!(layer.hit_test(Offset::new(15.0, 15.0), &mut result));
    }

    #[test]
    fn test_clip_rrect_hit_test_edges() {
        use crate::layer::PictureLayer;
        use crate::painter::Paint;
        use flui_types::geometry::RRect;

        // Create a rounded rect layer (100x100 with 20px corner radius)
        let rrect = RRect::from_rect_circular(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), 20.0);
        let mut layer = ClipRRectLayer::new(rrect);

        // Add a child that covers the entire area
        let mut picture = PictureLayer::new();
        picture.draw_rect(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), Paint::default());
        layer.add_child(Box::new(picture));

        let mut result = HitTestResult::new();

        // Middle of edges should hit (not affected by corner radius)
        assert!(layer.hit_test(Offset::new(50.0, 0.0), &mut result)); // top edge
        assert!(layer.hit_test(Offset::new(50.0, 100.0), &mut result)); // bottom edge
        assert!(layer.hit_test(Offset::new(0.0, 50.0), &mut result)); // left edge
        assert!(layer.hit_test(Offset::new(100.0, 50.0), &mut result)); // right edge
    }

    #[test]
    fn test_clip_rrect_hit_test_outside() {
        use crate::layer::PictureLayer;
        use crate::painter::Paint;
        use flui_types::geometry::RRect;

        // Create a rounded rect layer (100x100 with 20px corner radius)
        let rrect = RRect::from_rect_circular(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), 20.0);
        let mut layer = ClipRRectLayer::new(rrect);

        // Add a child that covers the entire area
        let mut picture = PictureLayer::new();
        picture.draw_rect(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), Paint::default());
        layer.add_child(Box::new(picture));

        let mut result = HitTestResult::new();

        // Points completely outside should not hit
        assert!(!layer.hit_test(Offset::new(-10.0, 50.0), &mut result));
        assert!(!layer.hit_test(Offset::new(110.0, 50.0), &mut result));
        assert!(!layer.hit_test(Offset::new(50.0, -10.0), &mut result));
        assert!(!layer.hit_test(Offset::new(50.0, 110.0), &mut result));
    }

    #[test]
    fn test_clip_rrect_hit_test_no_radius() {
        use crate::layer::PictureLayer;
        use crate::painter::Paint;
        use flui_types::geometry::RRect;

        // Create a rect with no rounding (should behave like ClipRectLayer)
        let rrect = RRect::from_rect(Rect::from_xywh(0.0, 0.0, 100.0, 100.0));
        let mut layer = ClipRRectLayer::new(rrect);

        // Add a child that covers the entire area
        let mut picture = PictureLayer::new();
        picture.draw_rect(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), Paint::default());
        layer.add_child(Box::new(picture));

        let mut result = HitTestResult::new();

        // All corners should hit (no rounding)
        assert!(layer.hit_test(Offset::new(0.0, 0.0), &mut result));
        assert!(layer.hit_test(Offset::new(100.0, 0.0), &mut result));
        assert!(layer.hit_test(Offset::new(0.0, 100.0), &mut result));
        assert!(layer.hit_test(Offset::new(100.0, 100.0), &mut result));
    }

    #[test]
    fn test_clip_path_layer_circle() {
        use crate::layer::PictureLayer;
        use crate::painter::Paint;
        use flui_types::painting::path::Path;

        // Create a circular path (100x100, radius 50)
        let mut path = Path::new();
        path.add_circle(Point::new(50.0, 50.0), 50.0);

        let mut layer = ClipPathLayer::new(path);

        // Add a child that covers the entire area
        let mut picture = PictureLayer::new();
        picture.draw_rect(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), Paint::default());
        layer.add_child(Box::new(picture));

        let mut result = HitTestResult::new();

        // Center should hit
        assert!(layer.hit_test(Offset::new(50.0, 50.0), &mut result));

        // Points on circle edge should hit
        assert!(layer.hit_test(Offset::new(50.0, 0.0), &mut result)); // top
        assert!(layer.hit_test(Offset::new(50.0, 100.0), &mut result)); // bottom
        assert!(layer.hit_test(Offset::new(0.0, 50.0), &mut result)); // left
        assert!(layer.hit_test(Offset::new(100.0, 50.0), &mut result)); // right

        // Corners should NOT hit (outside circle)
        assert!(!layer.hit_test(Offset::new(0.0, 0.0), &mut result));
        assert!(!layer.hit_test(Offset::new(100.0, 0.0), &mut result));
        assert!(!layer.hit_test(Offset::new(0.0, 100.0), &mut result));
        assert!(!layer.hit_test(Offset::new(100.0, 100.0), &mut result));
    }

    #[test]
    fn test_clip_path_layer_triangle() {
        use crate::layer::PictureLayer;
        use crate::painter::Paint;
        use flui_types::painting::path::Path;

        // Create a triangle path
        let mut path = Path::new();
        path.move_to(Point::new(50.0, 0.0)); // top
        path.line_to(Point::new(100.0, 100.0)); // bottom right
        path.line_to(Point::new(0.0, 100.0)); // bottom left
        path.close();

        let mut layer = ClipPathLayer::new(path);

        // Add a child
        let mut picture = PictureLayer::new();
        picture.draw_rect(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), Paint::default());
        layer.add_child(Box::new(picture));

        let mut result = HitTestResult::new();

        // Center should hit
        assert!(layer.hit_test(Offset::new(50.0, 50.0), &mut result));

        // Point inside triangle should hit
        assert!(layer.hit_test(Offset::new(50.0, 60.0), &mut result)); // middle-bottom

        // Top corners should NOT hit (outside triangle)
        assert!(!layer.hit_test(Offset::new(0.0, 0.0), &mut result));
        assert!(!layer.hit_test(Offset::new(100.0, 0.0), &mut result));
    }

    #[test]
    fn test_clip_path_layer_bounds() {
        use flui_types::painting::path::Path;

        // Create a path with known bounds
        let mut path = Path::new();
        path.add_rect(Rect::from_xywh(10.0, 20.0, 80.0, 60.0));

        let layer = ClipPathLayer::new(path);

        // Layer bounds should match path bounds
        let bounds = layer.bounds();
        assert_eq!(bounds, Rect::from_xywh(10.0, 20.0, 80.0, 60.0));
    }

    #[test]
    fn test_clip_path_layer_lifecycle() {
        use flui_types::painting::path::Path;

        let mut path = Path::new();
        path.add_circle(Point::new(50.0, 50.0), 50.0);

        let mut layer = ClipPathLayer::new(path);

        assert!(!layer.is_disposed());
        assert!(!layer.clip_path().is_empty());

        layer.dispose();
        assert!(layer.is_disposed());
    }
}

// ============================================================================
// ClipOvalLayer
// ============================================================================

/// A composited layer that clips its children to an oval
///
/// The oval fills the bounding rectangle provided.
/// If the rect is square, this creates a circle.
pub struct ClipOvalLayer {
    /// Base multi-child layer functionality
    base: MultiChildLayerBase,

    /// The bounding rectangle of the oval
    clip_rect: Rect,
}

impl ClipOvalLayer {
    /// Create a new clip oval layer
    ///
    /// # Arguments
    ///
    /// * `clip_rect` - The bounding rectangle of the oval
    pub fn new(clip_rect: Rect) -> Self {
        Self {
            base: MultiChildLayerBase::new(),
            clip_rect,
        }
    }

    /// Set the clip rectangle
    pub fn set_clip_rect(&mut self, rect: Rect) {
        if self.clip_rect != rect {
            self.clip_rect = rect;
            self.base.invalidate_cache();
            self.mark_needs_paint();
        }
    }

    /// Get the clip rectangle
    pub fn clip_rect(&self) -> Rect {
        self.clip_rect
    }

    /// Add a child layer
    pub fn add_child(&mut self, child: BoxedLayer) {
        self.base.add_child(child);
    }

    /// Remove all children
    pub fn clear_children(&mut self) {
        self.base.clear_children();
    }

    /// Get children
    pub fn children(&self) -> &[BoxedLayer] {
        self.base.children()
    }
}

impl Layer for ClipOvalLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        painter.save();
        painter.clip_oval(self.clip_rect);
        self.base.paint_children(painter);
        painter.restore();
    }

    fn bounds(&self) -> Rect {
        if let Some(bounds) = self.base.cached_bounds() {
            return bounds;
        }

        if self.base.is_empty() {
            return self.clip_rect;
        }

        let children_bounds = self.base.children_bounds_union();
        children_bounds
            .intersection(&self.clip_rect)
            .unwrap_or(Rect::ZERO)
    }

    fn is_visible(&self) -> bool {
        !self.base.is_disposed()
            && self.clip_rect.width() > 0.0
            && self.clip_rect.height() > 0.0
            && self.base.is_any_child_visible()
    }

    fn mark_needs_paint(&mut self) {
        self.base.invalidate_cache();
        for child in self.base.children_mut() {
            child.mark_needs_paint();
        }
    }

    fn dispose(&mut self) {
        self.base.dispose_children();
    }

    fn is_disposed(&self) -> bool {
        self.base.is_disposed()
    }

    fn debug_description(&self) -> String {
        format!(
            "ClipOvalLayer(clip_rect: {:?}, children: {})",
            self.clip_rect,
            self.base.len()
        )
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // Check if position is within oval
        // Use ellipse equation: ((x-cx)/rx)^2 + ((y-cy)/ry)^2 <= 1
        let center_x = self.clip_rect.left() + self.clip_rect.width() / 2.0;
        let center_y = self.clip_rect.top() + self.clip_rect.height() / 2.0;
        let rx = self.clip_rect.width() / 2.0;
        let ry = self.clip_rect.height() / 2.0;

        if rx <= 0.0 || ry <= 0.0 {
            return false;
        }

        let dx = (position.dx - center_x) / rx;
        let dy = (position.dy - center_y) / ry;
        let in_oval = (dx * dx + dy * dy) <= 1.0;

        if !in_oval {
            return false; // Outside oval, no hit
        }

        self.base.hit_test_children_reverse(position, result)
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        self.base.handle_event_children_reverse(event)
    }
}

// ============================================================================
// ClipPathLayer
// ============================================================================

use flui_types::painting::path::Path;

/// A composited layer that clips its children to an arbitrary path
///
/// This layer clips children to any arbitrary path shape.
pub struct ClipPathLayer {
    /// Base multi-child layer functionality
    base: MultiChildLayerBase,

    /// The clip path
    clip_path: Path,

    /// Pre-computed bounds of the path
    path_bounds: Rect,
}

impl ClipPathLayer {
    /// Create a new clip path layer
    ///
    /// # Arguments
    ///
    /// * `clip_path` - The path to clip to
    pub fn new(mut clip_path: Path) -> Self {
        let path_bounds = clip_path.bounds();
        Self {
            base: MultiChildLayerBase::new(),
            clip_path,
            path_bounds,
        }
    }

    /// Set the clip path
    pub fn set_clip_path(&mut self, mut path: Path) {
        self.path_bounds = path.bounds();
        self.clip_path = path;
        self.base.invalidate_cache();
        self.mark_needs_paint();
    }

    /// Get reference to the clip path
    pub fn clip_path(&self) -> &Path {
        &self.clip_path
    }

    /// Get the pre-computed bounds
    pub fn path_bounds(&self) -> Rect {
        self.path_bounds
    }

    /// Add a child layer
    pub fn add_child(&mut self, child: BoxedLayer) {
        self.base.add_child(child);
    }

    /// Remove all children
    pub fn clear_children(&mut self) {
        self.base.clear_children();
    }

    /// Get children
    pub fn children(&self) -> &[BoxedLayer] {
        self.base.children()
    }
}

impl Layer for ClipPathLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        painter.save();
        painter.clip_path(&self.clip_path, self.path_bounds);
        self.base.paint_children(painter);
        painter.restore();
    }

    fn bounds(&self) -> Rect {
        if let Some(bounds) = self.base.cached_bounds() {
            return bounds;
        }

        if self.base.is_empty() {
            return self.path_bounds;
        }

        let children_bounds = self.base.children_bounds_union();
        children_bounds
            .intersection(&self.path_bounds)
            .unwrap_or(Rect::ZERO)
    }

    fn is_visible(&self) -> bool {
        !self.base.is_disposed() && !self.clip_path.is_empty() && self.base.is_any_child_visible()
    }

    fn mark_needs_paint(&mut self) {
        self.base.invalidate_cache();
        for child in self.base.children_mut() {
            child.mark_needs_paint();
        }
    }

    fn dispose(&mut self) {
        self.base.dispose_children();
    }

    fn is_disposed(&self) -> bool {
        self.base.is_disposed()
    }

    fn debug_description(&self) -> String {
        format!("ClipPathLayer(children: {})", self.base.len())
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // Use proper path containment check
        if !self.clip_path.contains(position.into()) {
            return false; // Outside clip path, no hit
        }

        self.base.hit_test_children_reverse(position, result)
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        self.base.handle_event_children_reverse(event)
    }
}
