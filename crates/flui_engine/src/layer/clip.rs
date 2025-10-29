//! Clip layers - apply clipping to child layers
//!
//! This module provides layers that clip their children to specific regions,
//! following Flutter's ClipRectLayer and ClipRRectLayer architecture.

use crate::layer::{BoxedLayer, Layer};
use crate::painter::{Painter, RRect};
use flui_types::{Offset, Rect};
use flui_types::events::{Event, HitTestResult};

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
    /// The clip rectangle in layer coordinates
    clip_rect: Rect,

    /// Child layers
    children: Vec<BoxedLayer>,

    /// Whether this layer has been disposed
    disposed: bool,

    /// Cached bounds
    cached_bounds: Option<Rect>,
}

impl ClipRectLayer {
    /// Create a new clip rect layer
    ///
    /// # Arguments
    ///
    /// * `clip_rect` - The rectangle to clip to
    pub fn new(clip_rect: Rect) -> Self {
        Self {
            clip_rect,
            children: Vec::new(),
            disposed: false,
            cached_bounds: None,
        }
    }

    /// Set the clip rectangle
    pub fn set_clip_rect(&mut self, rect: Rect) {
        if self.clip_rect != rect {
            self.clip_rect = rect;
            self.cached_bounds = None;
            self.mark_needs_paint();
        }
    }

    /// Get the clip rectangle
    pub fn clip_rect(&self) -> Rect {
        self.clip_rect
    }

    /// Add a child layer
    pub fn add_child(&mut self, child: BoxedLayer) {
        self.children.push(child);
        self.cached_bounds = None;
    }

    /// Remove all children
    pub fn clear_children(&mut self) {
        self.children.clear();
        self.cached_bounds = None;
    }
}

impl Layer for ClipRectLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        if self.disposed {
            panic!("Cannot use disposed ClipRectLayer");
        }

        painter.save();

        // Apply clip rect
        painter.clip_rect(self.clip_rect);

        // Paint all children
        for child in &self.children {
            if child.is_visible() {
                child.paint(painter);
            }
        }

        painter.restore();
    }

    fn bounds(&self) -> Rect {
        if let Some(bounds) = self.cached_bounds {
            return bounds;
        }

        if self.children.is_empty() {
            return self.clip_rect;
        }

        // Union of all children bounds, clipped to clip_rect
        let mut bounds = Rect::ZERO;
        for (i, child) in self.children.iter().enumerate() {
            let child_bounds = child.bounds();
            if i == 0 {
                bounds = child_bounds;
            } else {
                bounds = bounds.union(&child_bounds);
            }
        }

        // Clip to clip_rect
        bounds.intersection(&self.clip_rect).unwrap_or(Rect::ZERO)
    }

    fn is_visible(&self) -> bool {
        !self.disposed
            && self.clip_rect.width() > 0.0
            && self.clip_rect.height() > 0.0
            && self.children.iter().any(|c| c.is_visible())
    }

    fn mark_needs_paint(&mut self) {
        self.cached_bounds = None;
    }

    fn dispose(&mut self) {
        self.disposed = true;
        self.children.clear();
        self.cached_bounds = None;
    }

    fn is_disposed(&self) -> bool {
        self.disposed
    }

    fn debug_description(&self) -> String {
        format!(
            "ClipRectLayer(clip_rect: {:?}, children: {})",
            self.clip_rect,
            self.children.len()
        )
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // First check if position is within clip rect
        if !self.clip_rect.contains(position) {
            return false; // Outside clip region, no hit
        }

        // Test children in reverse order (front to back)
        let mut hit = false;
        for child in self.children.iter().rev() {
            if child.is_visible() && child.hit_test(position, result) {
                hit = true;
            }
        }

        hit
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        // Dispatch to children in reverse order (front to back)
        for child in self.children.iter_mut().rev() {
            if child.handle_event(event) {
                return true; // Event handled
            }
        }
        false
    }
}

// ============================================================================
// ClipRRectLayer
// ============================================================================

/// A composited layer that clips its children to a rounded rectangle
///
/// Similar to ClipRectLayer but with rounded corners.
pub struct ClipRRectLayer {
    /// The clip rounded rectangle
    clip_rrect: RRect,

    /// Child layers
    children: Vec<BoxedLayer>,

    /// Whether this layer has been disposed
    disposed: bool,

    /// Cached bounds
    cached_bounds: Option<Rect>,
}

impl ClipRRectLayer {
    /// Create a new clip rrect layer
    pub fn new(clip_rrect: RRect) -> Self {
        Self {
            clip_rrect,
            children: Vec::new(),
            disposed: false,
            cached_bounds: None,
        }
    }

    /// Set the clip rounded rectangle
    pub fn set_clip_rrect(&mut self, rrect: RRect) {
        if self.clip_rrect != rrect {
            self.clip_rrect = rrect;
            self.cached_bounds = None;
            self.mark_needs_paint();
        }
    }

    /// Get the clip rounded rectangle
    pub fn clip_rrect(&self) -> RRect {
        self.clip_rrect
    }

    /// Add a child layer
    pub fn add_child(&mut self, child: BoxedLayer) {
        self.children.push(child);
        self.cached_bounds = None;
    }

    /// Remove all children
    pub fn clear_children(&mut self) {
        self.children.clear();
        self.cached_bounds = None;
    }
}

impl Layer for ClipRRectLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        if self.disposed {
            panic!("Cannot use disposed ClipRRectLayer");
        }

        painter.save();

        // Apply clip rrect
        painter.clip_rrect(self.clip_rrect);

        // Paint all children
        for child in &self.children {
            if child.is_visible() {
                child.paint(painter);
            }
        }

        painter.restore();
    }

    fn bounds(&self) -> Rect {
        if let Some(bounds) = self.cached_bounds {
            return bounds;
        }

        if self.children.is_empty() {
            return self.clip_rrect.rect;
        }

        let mut bounds = Rect::ZERO;
        for (i, child) in self.children.iter().enumerate() {
            let child_bounds = child.bounds();
            if i == 0 {
                bounds = child_bounds;
            } else {
                bounds = bounds.union(&child_bounds);
            }
        }

        bounds
            .intersection(&self.clip_rrect.rect)
            .unwrap_or(Rect::ZERO)
    }

    fn is_visible(&self) -> bool {
        !self.disposed
            && self.clip_rrect.rect.width() > 0.0
            && self.clip_rrect.rect.height() > 0.0
            && self.children.iter().any(|c| c.is_visible())
    }

    fn mark_needs_paint(&mut self) {
        self.cached_bounds = None;
    }

    fn dispose(&mut self) {
        self.disposed = true;
        self.children.clear();
        self.cached_bounds = None;
    }

    fn is_disposed(&self) -> bool {
        self.disposed
    }

    fn debug_description(&self) -> String {
        format!(
            "ClipRRectLayer(clip_rrect: {:?}, children: {})",
            self.clip_rrect,
            self.children.len()
        )
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // First check if position is within clip rrect
        // For now, use rectangular hit test (TODO: proper rounded rect hit testing)
        if !self.clip_rrect.rect.contains(position) {
            return false; // Outside clip region, no hit
        }

        // Test children in reverse order (front to back)
        let mut hit = false;
        for child in self.children.iter().rev() {
            if child.is_visible() && child.hit_test(position, result) {
                hit = true;
            }
        }

        hit
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        // Dispatch to children in reverse order (front to back)
        for child in self.children.iter_mut().rev() {
            if child.handle_event(event) {
                return true; // Event handled
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

// ============================================================================
// ClipOvalLayer
// ============================================================================

/// A composited layer that clips its children to an oval
///
/// The oval fills the bounding rectangle provided.
/// If the rect is square, this creates a circle.
pub struct ClipOvalLayer {
    /// The bounding rectangle of the oval
    clip_rect: Rect,

    /// Child layers
    children: Vec<BoxedLayer>,

    /// Whether this layer has been disposed
    disposed: bool,

    /// Cached bounds
    cached_bounds: Option<Rect>,
}

impl ClipOvalLayer {
    /// Create a new clip oval layer
    ///
    /// # Arguments
    ///
    /// * `clip_rect` - The bounding rectangle of the oval
    pub fn new(clip_rect: Rect) -> Self {
        Self {
            clip_rect,
            children: Vec::new(),
            disposed: false,
            cached_bounds: None,
        }
    }

    /// Set the clip rectangle
    pub fn set_clip_rect(&mut self, rect: Rect) {
        if self.clip_rect != rect {
            self.clip_rect = rect;
            self.cached_bounds = None;
            self.mark_needs_paint();
        }
    }

    /// Get the clip rectangle
    pub fn clip_rect(&self) -> Rect {
        self.clip_rect
    }

    /// Add a child layer
    pub fn add_child(&mut self, child: BoxedLayer) {
        self.children.push(child);
        self.cached_bounds = None;
    }

    /// Remove all children
    pub fn clear_children(&mut self) {
        self.children.clear();
        self.cached_bounds = None;
    }
}

impl Layer for ClipOvalLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        if self.disposed {
            panic!("Cannot use disposed ClipOvalLayer");
        }

        painter.save();

        // Apply oval clip
        painter.clip_oval(self.clip_rect);

        // Paint all children
        for child in &self.children {
            if child.is_visible() {
                child.paint(painter);
            }
        }

        painter.restore();
    }

    fn bounds(&self) -> Rect {
        if let Some(bounds) = self.cached_bounds {
            return bounds;
        }

        if self.children.is_empty() {
            return self.clip_rect;
        }

        // Union of all children bounds, clipped to clip_rect
        let mut bounds = Rect::ZERO;
        for (i, child) in self.children.iter().enumerate() {
            let child_bounds = child.bounds();
            if i == 0 {
                bounds = child_bounds;
            } else {
                bounds = bounds.union(&child_bounds);
            }
        }

        // Clip to oval bounds (use rect as approximation)
        bounds.intersection(&self.clip_rect).unwrap_or(Rect::ZERO)
    }

    fn is_visible(&self) -> bool {
        !self.disposed
            && self.clip_rect.width() > 0.0
            && self.clip_rect.height() > 0.0
            && self.children.iter().any(|c| c.is_visible())
    }

    fn mark_needs_paint(&mut self) {
        self.cached_bounds = None;
    }

    fn dispose(&mut self) {
        self.disposed = true;
        self.children.clear();
        self.cached_bounds = None;
    }

    fn is_disposed(&self) -> bool {
        self.disposed
    }

    fn debug_description(&self) -> String {
        format!(
            "ClipOvalLayer(clip_rect: {:?}, children: {})",
            self.clip_rect,
            self.children.len()
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

        // Test children in reverse order (front to back)
        let mut hit = false;
        for child in self.children.iter().rev() {
            if child.is_visible() && child.hit_test(position, result) {
                hit = true;
            }
        }

        hit
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        // Dispatch to children in reverse order (front to back)
        for child in self.children.iter_mut().rev() {
            if child.handle_event(event) {
                return true; // Event handled
            }
        }
        false
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
    /// The clip path
    clip_path: Path,

    /// Pre-computed bounds of the path
    path_bounds: Rect,

    /// Child layers
    children: Vec<BoxedLayer>,

    /// Whether this layer has been disposed
    disposed: bool,

    /// Cached bounds
    cached_bounds: Option<Rect>,
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
            clip_path,
            path_bounds,
            children: Vec::new(),
            disposed: false,
            cached_bounds: None,
        }
    }

    /// Set the clip path
    pub fn set_clip_path(&mut self, mut path: Path) {
        self.path_bounds = path.bounds();
        self.clip_path = path;
        self.cached_bounds = None;
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
        self.children.push(child);
        self.cached_bounds = None;
    }

    /// Remove all children
    pub fn clear_children(&mut self) {
        self.children.clear();
        self.cached_bounds = None;
    }
}

impl Layer for ClipPathLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        if self.disposed {
            panic!("Cannot use disposed ClipPathLayer");
        }

        painter.save();

        // Apply path clip with pre-computed bounds
        painter.clip_path(&self.clip_path, self.path_bounds);

        // Paint all children
        for child in &self.children {
            if child.is_visible() {
                child.paint(painter);
            }
        }

        painter.restore();
    }

    fn bounds(&self) -> Rect {
        if let Some(bounds) = self.cached_bounds {
            return bounds;
        }

        if self.children.is_empty() {
            return self.path_bounds;
        }

        // Union of all children bounds, clipped to path bounds
        let mut bounds = Rect::ZERO;
        for (i, child) in self.children.iter().enumerate() {
            let child_bounds = child.bounds();
            if i == 0 {
                bounds = child_bounds;
            } else {
                bounds = bounds.union(&child_bounds);
            }
        }

        // Clip to path bounds (already computed)
        bounds.intersection(&self.path_bounds).unwrap_or(Rect::ZERO)
    }

    fn is_visible(&self) -> bool {
        !self.disposed && !self.clip_path.is_empty() && self.children.iter().any(|c| c.is_visible())
    }

    fn mark_needs_paint(&mut self) {
        self.cached_bounds = None;
    }

    fn dispose(&mut self) {
        self.disposed = true;
        self.children.clear();
        self.cached_bounds = None;
    }

    fn is_disposed(&self) -> bool {
        self.disposed
    }

    fn debug_description(&self) -> String {
        format!("ClipPathLayer(children: {})", self.children.len())
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // For path clipping, use bounding box as conservative hit test
        // TODO: Implement proper path containment check
        if !self.path_bounds.contains(position) {
            return false;
        }

        // Test children in reverse order (front to back)
        let mut hit = false;
        for child in self.children.iter().rev() {
            if child.is_visible() && child.hit_test(position, result) {
                hit = true;
            }
        }

        hit
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        // Dispatch to children in reverse order (front to back)
        for child in self.children.iter_mut().rev() {
            if child.handle_event(event) {
                return true; // Event handled
            }
        }
        false
    }
}
