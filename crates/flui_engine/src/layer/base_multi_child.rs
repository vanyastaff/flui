//! Base implementation for multi-child layers
//!
//! This module provides shared functionality for layers that can have multiple children,
//! reducing code duplication across ContainerLayer and all clip layers.

use crate::layer::BoxedLayer;
use flui_types::{
    events::{Event, HitTestResult},
    Offset, Rect,
};

/// Base struct for layers with multiple children
///
/// Provides common fields and default implementations for multi-child layers.
/// This eliminates ~100-150 lines of duplicated code per layer.
///
/// # Usage
///
/// ```rust,ignore
/// pub struct MyLayer {
///     base: MultiChildLayerBase,
///     // ... layer-specific fields
/// }
///
/// impl Layer for MyLayer {
///     fn bounds(&self) -> Rect {
///         self.base.children_bounds_union()
///     }
///
///     fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
///         // Custom check first...
///         self.base.hit_test_children_reverse(position, result)
///     }
///     // ... other methods
/// }
/// ```
pub struct MultiChildLayerBase {
    /// Child layers
    children: Vec<BoxedLayer>,

    /// Whether this layer has been disposed
    disposed: bool,

    /// Cached bounds (optional optimization)
    cached_bounds: Option<Rect>,
}

impl MultiChildLayerBase {
    /// Create a new empty multi-child base
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            disposed: false,
            cached_bounds: None,
        }
    }

    /// Create with initial capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            children: Vec::with_capacity(capacity),
            disposed: false,
            cached_bounds: None,
        }
    }

    /// Get reference to children
    #[inline]
    pub fn children(&self) -> &[BoxedLayer] {
        &self.children
    }

    /// Get mutable reference to children
    #[inline]
    pub fn children_mut(&mut self) -> &mut Vec<BoxedLayer> {
        &mut self.children
    }

    /// Add a child
    pub fn add_child(&mut self, child: BoxedLayer) {
        self.children.push(child);
        self.invalidate_cache();
    }

    /// Remove all children
    pub fn clear_children(&mut self) {
        self.children.clear();
        self.invalidate_cache();
    }

    /// Check if has any children
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// Get number of children
    #[inline]
    pub fn len(&self) -> usize {
        self.children.len()
    }

    /// Check if disposed
    #[inline]
    pub fn is_disposed(&self) -> bool {
        self.disposed
    }

    /// Get cached bounds
    #[inline]
    pub fn cached_bounds(&self) -> Option<Rect> {
        self.cached_bounds
    }

    /// Set cached bounds
    pub fn set_cached_bounds(&mut self, bounds: Option<Rect>) {
        self.cached_bounds = bounds;
    }

    /// Invalidate cached bounds
    pub fn invalidate_cache(&mut self) {
        self.cached_bounds = None;
    }

    /// Calculate union of all children bounds
    pub fn children_bounds_union(&self) -> Rect {
        if self.children.is_empty() {
            return Rect::ZERO;
        }

        let mut bounds = self.children[0].bounds();
        for child in &self.children[1..] {
            bounds = bounds.union(&child.bounds());
        }
        bounds
    }

    /// Calculate union of all children bounds with caching
    pub fn children_bounds_union_cached(&mut self) -> Rect {
        if let Some(cached) = self.cached_bounds {
            return cached;
        }

        let bounds = self.children_bounds_union();
        self.cached_bounds = Some(bounds);
        bounds
    }

    /// Default visibility check - any child visible
    pub fn is_any_child_visible(&self) -> bool {
        !self.disposed && self.children.iter().any(|c| c.is_visible())
    }

    /// Hit test children in reverse order (front to back)
    ///
    /// This is the standard pattern for multi-child layers where
    /// children are painted in order (back to front) but hit tested
    /// in reverse (front to back).
    ///
    /// Returns true if any child was hit.
    pub fn hit_test_children_reverse(&self, position: Offset, result: &mut HitTestResult) -> bool {
        if self.disposed {
            return false;
        }

        let mut hit = false;
        for child in self.children.iter().rev() {
            if child.is_visible() && child.hit_test(position, result) {
                hit = true;
            }
        }
        hit
    }

    /// Hit test children in forward order
    ///
    /// Less common, but used by some layer types.
    pub fn hit_test_children_forward(&self, position: Offset, result: &mut HitTestResult) -> bool {
        if self.disposed {
            return false;
        }

        let mut hit = false;
        for child in &self.children {
            if child.is_visible() && child.hit_test(position, result) {
                hit = true;
            }
        }
        hit
    }

    /// Handle event on children in reverse order
    ///
    /// Returns true if any child handled the event.
    /// Stops at first handler (first child to return true).
    pub fn handle_event_children_reverse(&mut self, event: &Event) -> bool {
        if self.disposed {
            return false;
        }

        for child in self.children.iter_mut().rev() {
            if child.handle_event(event) {
                return true;
            }
        }
        false
    }

    /// Handle event on children in forward order
    pub fn handle_event_children_forward(&mut self, event: &Event) -> bool {
        if self.disposed {
            return false;
        }

        for child in &mut self.children {
            if child.handle_event(event) {
                return true;
            }
        }
        false
    }

    /// Paint all children in order (back to front)
    pub fn paint_children(&self, painter: &mut dyn crate::painter::Painter) {
        if self.disposed {
            return;
        }

        for child in &self.children {
            if child.is_visible() {
                child.paint(painter);
            }
        }
    }

    /// Dispose all children and mark as disposed
    pub fn dispose_children(&mut self) {
        for child in &mut self.children {
            child.dispose();
        }
        self.children.clear();
        self.disposed = true;
        self.cached_bounds = None;
    }
}

impl Default for MultiChildLayerBase {
    fn default() -> Self {
        Self::new()
    }
}
