//! Clip layers - apply clipping to child layers
//!
//! This module provides layers that clip their children to specific regions,
//! following Flutter's ClipRectLayer and ClipRRectLayer architecture.

use flui_types::Rect;
use crate::layer::{Layer, BoxedLayer};
use crate::painter::{Painter, RRect};
use std::sync::Arc;
use parking_lot::RwLock;

/// Type of clipping to apply (legacy)
#[derive(Debug, Clone)]
pub enum ClipBehavior {
    /// Clip to rectangle
    Rect(Rect),

    /// Clip to rounded rectangle
    RRect(RRect),
}

/// Legacy clip layer - applies clipping to child layer
///
/// **Deprecated**: Use `ClipRectLayer` or `ClipRRectLayer` instead for proper
/// lifecycle management and better performance.
#[deprecated(
    since = "0.1.0",
    note = "Use ClipRectLayer or ClipRRectLayer instead. Will be removed in 0.2.0."
)]
pub struct ClipLayer {
    /// The child layer to clip
    child: BoxedLayer,

    /// The clipping behavior
    clip: ClipBehavior,
}

impl ClipLayer {
    /// Create a new clip layer
    pub fn new(child: BoxedLayer, clip: ClipBehavior) -> Self {
        Self { child, clip }
    }

    /// Create a rectangular clip layer
    pub fn rect(child: BoxedLayer, rect: Rect) -> Self {
        Self::new(child, ClipBehavior::Rect(rect))
    }

    /// Create a rounded rectangular clip layer
    pub fn rrect(child: BoxedLayer, rrect: RRect) -> Self {
        Self::new(child, ClipBehavior::RRect(rrect))
    }
}

impl Layer for ClipLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        painter.save();

        match &self.clip {
            ClipBehavior::Rect(rect) => painter.clip_rect(*rect),
            ClipBehavior::RRect(rrect) => painter.clip_rrect(*rrect),
        }

        self.child.paint(painter);
        painter.restore();
    }

    fn bounds(&self) -> Rect {
        let child_bounds = self.child.bounds();
        match &self.clip {
            ClipBehavior::Rect(rect) => {
                child_bounds.intersection(rect).unwrap_or(Rect::ZERO)
            }
            ClipBehavior::RRect(rrect) => {
                child_bounds.intersection(&rrect.rect).unwrap_or(Rect::ZERO)
            }
        }
    }

    fn is_visible(&self) -> bool {
        let has_area = match &self.clip {
            ClipBehavior::Rect(rect) => rect.width() > 0.0 && rect.height() > 0.0,
            ClipBehavior::RRect(rrect) => {
                rrect.rect.width() > 0.0 && rrect.rect.height() > 0.0
            }
        };
        has_area && self.child.is_visible()
    }
}

// ============================================================================
// New Architecture: ClipRectLayer
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
    children: Vec<Arc<RwLock<dyn Layer>>>,

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
    /// * `old_layer` - Optional old layer to reuse resources from
    pub fn new(clip_rect: Rect, old_layer: Option<Arc<RwLock<Self>>>) -> Arc<RwLock<Self>> {
        // Try to reuse old layer
        if let Some(old) = old_layer {
            let mut layer = old.write();
            layer.clip_rect = clip_rect;
            layer.cached_bounds = None;
            layer.mark_needs_paint();
            drop(layer);
            return old;
        }

        // Create new layer
        Arc::new(RwLock::new(Self {
            clip_rect,
            children: Vec::new(),
            disposed: false,
            cached_bounds: None,
        }))
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
    pub fn add_child(&mut self, child: Arc<RwLock<dyn Layer>>) {
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
            let child_layer = child.read();
            if child_layer.is_visible() {
                child_layer.paint(painter);
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
            let child_bounds = child.read().bounds();
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
            && self.children.iter().any(|c| c.read().is_visible())
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
        format!("ClipRectLayer(clip_rect: {:?}, children: {})", self.clip_rect, self.children.len())
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
    children: Vec<Arc<RwLock<dyn Layer>>>,

    /// Whether this layer has been disposed
    disposed: bool,

    /// Cached bounds
    cached_bounds: Option<Rect>,
}

impl ClipRRectLayer {
    /// Create a new clip rrect layer
    pub fn new(clip_rrect: RRect, old_layer: Option<Arc<RwLock<Self>>>) -> Arc<RwLock<Self>> {
        if let Some(old) = old_layer {
            let mut layer = old.write();
            layer.clip_rrect = clip_rrect;
            layer.cached_bounds = None;
            layer.mark_needs_paint();
            drop(layer);
            return old;
        }

        Arc::new(RwLock::new(Self {
            clip_rrect,
            children: Vec::new(),
            disposed: false,
            cached_bounds: None,
        }))
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
    pub fn add_child(&mut self, child: Arc<RwLock<dyn Layer>>) {
        self.children.push(child);
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
            let child_layer = child.read();
            if child_layer.is_visible() {
                child_layer.paint(painter);
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
            let child_bounds = child.read().bounds();
            if i == 0 {
                bounds = child_bounds;
            } else {
                bounds = bounds.union(&child_bounds);
            }
        }

        bounds.intersection(&self.clip_rrect.rect).unwrap_or(Rect::ZERO)
    }

    fn is_visible(&self) -> bool {
        !self.disposed
            && self.clip_rrect.rect.width() > 0.0
            && self.clip_rrect.rect.height() > 0.0
            && self.children.iter().any(|c| c.read().is_visible())
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
        format!("ClipRRectLayer(clip_rrect: {:?}, children: {})", self.clip_rrect, self.children.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clip_rect_layer_lifecycle() {
        let layer = ClipRectLayer::new(
            Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
            None,
        );

        {
            let l = layer.read();
            assert!(!l.is_disposed());
            assert_eq!(l.clip_rect(), Rect::from_xywh(0.0, 0.0, 100.0, 100.0));
        }

        {
            let mut l = layer.write();
            l.dispose();
        }

        {
            let l = layer.read();
            assert!(l.is_disposed());
        }
    }

    #[test]
    fn test_clip_rect_layer_reuse() {
        let layer1 = ClipRectLayer::new(
            Rect::from_xywh(0.0, 0.0, 50.0, 50.0),
            None,
        );

        // Reuse layer with different clip rect
        let layer2 = ClipRectLayer::new(
            Rect::from_xywh(10.0, 10.0, 100.0, 100.0),
            Some(layer1.clone()),
        );

        // Should be same Arc
        assert!(Arc::ptr_eq(&layer1, &layer2));

        let l = layer2.read();
        assert_eq!(l.clip_rect(), Rect::from_xywh(10.0, 10.0, 100.0, 100.0));
    }
}
