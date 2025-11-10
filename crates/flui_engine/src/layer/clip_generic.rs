//! Generic clip layer implementation - eliminates code duplication
//!
//! This module provides a generic ClipLayer<S: ClipStrategy> that works with
//! any clip shape (Rect, RRect, Oval, Path). This reduces ~400 lines of
//! duplicated code to a single generic implementation.
//!
//! # Architecture
//!
//! ```text
//! ClipLayer<S: ClipStrategy>
//!     ├─ RectStrategy     → ClipRectLayer
//!     ├─ RRectStrategy    → ClipRRectLayer
//!     ├─ OvalStrategy     → ClipOvalLayer
//!     └─ PathStrategy     → ClipPathLayer
//! ```

use crate::layer::{base_multi_child::MultiChildLayerBase, BoxedLayer, Layer};
use crate::painter::{Painter, RRect};
use flui_types::events::{Event, HitTestResult};
use flui_types::painting::Path;
use flui_types::{Offset, Point, Rect};
use parking_lot::RwLock;

// ============================================================================
// ClipStrategy Trait
// ============================================================================

/// Strategy trait for different clip shapes
///
/// Defines how each clip shape type behaves:
/// - Bounds extraction
/// - Containment testing
/// - Painter clipping
///
/// # Type Safety
///
/// The associated `Shape` type must be:
/// - `Clone` - for efficient copying
/// - `PartialEq` - for change detection
/// - `Send + Sync` - for thread safety
/// - `'static` - for layer storage
pub trait ClipStrategy: 'static + Send + Sync + Default {
    /// The shape type used for clipping
    type Shape: Clone + PartialEq + Send + Sync + 'static;

    /// Get the bounding rectangle of the clip shape
    ///
    /// Note: Takes mutable reference because some shapes (like Path)
    /// compute and cache bounds lazily.
    fn bounds(&self, shape: &mut Self::Shape) -> Rect;

    /// Check if a point is contained within the clip shape
    fn contains(&self, shape: &Self::Shape, point: Offset) -> bool;

    /// Apply clipping to the painter
    fn apply_clip(&self, shape: &Self::Shape, painter: &mut dyn Painter);

    /// Get debug name for this strategy
    fn debug_name(&self) -> &'static str;
}

// ============================================================================
// Generic ClipLayer
// ============================================================================

/// Generic clip layer that works with any ClipStrategy
///
/// This single generic type replaces ClipRectLayer, ClipRRectLayer,
/// ClipOvalLayer, and ClipPathLayer, eliminating ~400 lines of duplication.
///
/// # Example
///
/// ```rust,ignore
/// use flui_engine::layer::{ClipLayer, RectStrategy};
///
/// // Create a rect clip layer
/// let mut layer = ClipLayer::<RectStrategy>::new(
///     Rect::from_xywh(0.0, 0.0, 100.0, 100.0)
/// );
///
/// // Or use the type alias
/// let mut layer = ClipRectLayer::new(
///     Rect::from_xywh(0.0, 0.0, 100.0, 100.0)
/// );
/// ```
pub struct ClipLayer<S: ClipStrategy> {
    /// Base multi-child layer functionality
    base: MultiChildLayerBase,

    /// The clip shape (Rect, RRect, Oval, or Path)
    /// Wrapped in RwLock to allow thread-safe interior mutability for bounds computation
    clip_shape: RwLock<S::Shape>,

    /// Strategy for handling this shape type
    strategy: S,
}

impl<S: ClipStrategy> ClipLayer<S> {
    /// Create a new clip layer with the given shape
    pub fn new(clip_shape: S::Shape) -> Self {
        Self {
            base: MultiChildLayerBase::new(),
            clip_shape: RwLock::new(clip_shape),
            strategy: S::default(),
        }
    }

    /// Set the clip shape
    pub fn set_clip_shape(&mut self, shape: S::Shape) {
        if *self.clip_shape.read() != shape {
            *self.clip_shape.write() = shape;
            self.base.invalidate_cache();
            self.mark_needs_paint();
        }
    }

    /// Get a reference to the clip shape
    ///
    /// Note: Returns a RwLock read guard, not a direct reference
    pub fn clip_shape(&self) -> parking_lot::RwLockReadGuard<'_, S::Shape> {
        self.clip_shape.read()
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

impl<S: ClipStrategy> Layer for ClipLayer<S> {
    fn paint(&self, painter: &mut dyn Painter) {
        painter.save();
        self.strategy.apply_clip(&self.clip_shape.read(), painter);
        self.base.paint_children(painter);
        painter.restore();
    }

    fn bounds(&self) -> Rect {
        if let Some(bounds) = self.base.cached_bounds() {
            return bounds;
        }

        if self.base.is_empty() {
            return self.strategy.bounds(&mut self.clip_shape.write());
        }

        // Union of all children bounds, clipped to shape bounds
        let children_bounds = self.base.children_bounds_union();
        let clip_bounds = self.strategy.bounds(&mut self.clip_shape.write());
        children_bounds
            .intersection(&clip_bounds)
            .unwrap_or(Rect::ZERO)
    }

    fn is_visible(&self) -> bool {
        let clip_bounds = self.strategy.bounds(&mut self.clip_shape.write());
        !self.base.is_disposed()
            && clip_bounds.width() > 0.0
            && clip_bounds.height() > 0.0
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
            "ClipLayer<{}>(children: {})",
            self.strategy.debug_name(),
            self.base.len()
        )
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // First check if position is within clip shape
        if !self.strategy.contains(&self.clip_shape.read(), position) {
            return false; // Outside clip region, no hit
        }

        self.base.hit_test_children_reverse(position, result)
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        self.base.handle_event_children_reverse(event)
    }
}

// ============================================================================
// Concrete Strategies
// ============================================================================

/// Strategy for rectangular clipping
#[derive(Debug, Default)]
pub struct RectStrategy;

impl ClipStrategy for RectStrategy {
    type Shape = Rect;

    fn bounds(&self, shape: &mut Rect) -> Rect {
        *shape
    }

    fn contains(&self, shape: &Rect, point: Offset) -> bool {
        shape.contains(point)
    }

    fn apply_clip(&self, shape: &Rect, painter: &mut dyn Painter) {
        painter.clip_rect(*shape);
    }

    fn debug_name(&self) -> &'static str {
        "Rect"
    }
}

/// Strategy for rounded rectangle clipping
#[derive(Debug, Default)]
pub struct RRectStrategy;

impl ClipStrategy for RRectStrategy {
    type Shape = RRect;

    fn bounds(&self, shape: &mut RRect) -> Rect {
        shape.rect
    }

    fn contains(&self, shape: &RRect, point: Offset) -> bool {
        shape.contains(point.into())
    }

    fn apply_clip(&self, shape: &RRect, painter: &mut dyn Painter) {
        painter.clip_rrect(*shape);
    }

    fn debug_name(&self) -> &'static str {
        "RRect"
    }
}

/// Strategy for oval clipping
#[derive(Debug, Default)]
pub struct OvalStrategy;

impl ClipStrategy for OvalStrategy {
    type Shape = Rect;

    fn bounds(&self, shape: &mut Rect) -> Rect {
        *shape
    }

    fn contains(&self, shape: &Rect, point: Offset) -> bool {
        // Check if point is inside ellipse
        let center = shape.center();
        let rx = shape.width() / 2.0;
        let ry = shape.height() / 2.0;

        if rx == 0.0 || ry == 0.0 {
            return false;
        }

        let dx = (point.dx - center.x) / rx;
        let dy = (point.dy - center.y) / ry;
        (dx * dx + dy * dy) <= 1.0
    }

    fn apply_clip(&self, shape: &Rect, painter: &mut dyn Painter) {
        painter.clip_oval(*shape);
    }

    fn debug_name(&self) -> &'static str {
        "Oval"
    }
}

/// Strategy for path clipping
#[derive(Debug, Default)]
pub struct PathStrategy;

impl ClipStrategy for PathStrategy {
    type Shape = Path;

    fn bounds(&self, shape: &mut Path) -> Rect {
        shape.bounds()
    }

    fn contains(&self, shape: &Path, point: Offset) -> bool {
        shape.contains(Point::new(point.dx, point.dy))
    }

    fn apply_clip(&self, _shape: &Path, painter: &mut dyn Painter) {
        // TODO: clip_path currently takes &str (stub), need to implement proper path clipping
        // For now, we just log a warning
        #[cfg(debug_assertions)]
        tracing::warn!("PathStrategy::apply_clip: path clipping not yet fully implemented");
        painter.clip_path("");
    }

    fn debug_name(&self) -> &'static str {
        "Path"
    }
}

// ============================================================================
// Type Aliases (for backward compatibility)
// ============================================================================

/// A composited layer that clips its children to a rectangle
pub type ClipRectLayer = ClipLayer<RectStrategy>;

/// A composited layer that clips its children to a rounded rectangle
pub type ClipRRectLayer = ClipLayer<RRectStrategy>;

/// A composited layer that clips its children to an oval
pub type ClipOvalLayer = ClipLayer<OvalStrategy>;

/// A composited layer that clips its children to a path
pub type ClipPathLayer = ClipLayer<PathStrategy>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::PictureLayer;
    use crate::painter::Paint;

    #[test]
    fn test_generic_clip_rect_layer() {
        let mut layer = ClipRectLayer::new(Rect::from_xywh(0.0, 0.0, 100.0, 100.0));

        assert!(!layer.is_disposed());
        assert_eq!(*layer.clip_shape(), Rect::from_xywh(0.0, 0.0, 100.0, 100.0));

        layer.dispose();
        assert!(layer.is_disposed());
    }

    #[test]
    fn test_generic_clip_rrect_layer() {
        let rrect = RRect::from_rect_circular(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), 20.0);
        let layer = ClipRRectLayer::new(rrect.clone());

        assert!(!layer.is_disposed());
        assert_eq!(*layer.clip_shape(), rrect);
    }

    #[test]
    fn test_rect_strategy_hit_test() {
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let mut layer = ClipRectLayer::new(rect);

        // Add a child
        let mut picture = PictureLayer::new();
        picture.draw_rect(rect, Paint::default());
        layer.add_child(Box::new(picture));

        let mut result = HitTestResult::new();

        // Inside
        assert!(layer.hit_test(Offset::new(50.0, 50.0), &mut result));

        // Outside
        assert!(!layer.hit_test(Offset::new(150.0, 150.0), &mut result));
    }

    #[test]
    fn test_oval_strategy_contains() {
        let strategy = OvalStrategy;
        let oval = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);

        // Center should be inside
        assert!(strategy.contains(&oval, Offset::new(50.0, 50.0)));

        // Corner should be outside
        assert!(!strategy.contains(&oval, Offset::new(0.0, 0.0)));

        // Edge point on ellipse circumference should be inside (or very close)
        assert!(strategy.contains(&oval, Offset::new(100.0, 50.0)));
    }
}
