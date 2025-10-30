//! Base implementation for single-child layers
//!
//! This module provides shared functionality for layers that have exactly one child,
//! reducing code duplication across OpacityLayer, OffsetLayer, TransformLayer, BlurLayer,
//! FilterLayer, and BackdropFilterLayer.

use crate::layer::BoxedLayer;
use flui_types::{
    events::{Event, HitTestResult},
    Offset, Rect,
};

/// Base struct for layers with a single child
///
/// Provides common fields and default implementations for single-child layers.
/// This eliminates ~150-200 lines of duplicated code per layer.
///
/// # Usage
///
/// ```rust,ignore
/// pub struct MyLayer {
///     base: SingleChildLayerBase,
///     // ... layer-specific fields
/// }
///
/// impl Layer for MyLayer {
///     fn bounds(&self) -> Rect {
///         self.base.child_bounds()
///     }
///
///     fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
///         self.base.child_hit_test(position, result)
///     }
///     // ... other methods
/// }
/// ```
#[derive(Default)]
pub struct SingleChildLayerBase {
    /// The single child layer
    child: Option<BoxedLayer>,

    /// Whether this layer has been disposed
    disposed: bool,

    /// Cached bounds (optional optimization)
    cached_bounds: Option<Rect>,
}

impl SingleChildLayerBase {
    /// Create a new base with the given child
    pub fn new(child: BoxedLayer) -> Self {
        Self {
            child: Some(child),
            disposed: false,
            cached_bounds: None,
        }
    }

    /// Create a new base with optional child
    pub fn new_optional(child: Option<BoxedLayer>) -> Self {
        Self {
            child,
            disposed: false,
            cached_bounds: None,
        }
    }

    /// Get reference to the child
    #[inline]
    pub fn child(&self) -> Option<&BoxedLayer> {
        self.child.as_ref()
    }

    /// Get mutable reference to the child
    #[inline]
    pub fn child_mut(&mut self) -> Option<&mut BoxedLayer> {
        self.child.as_mut()
    }

    /// Set the child
    pub fn set_child(&mut self, child: BoxedLayer) {
        self.child = Some(child);
        self.invalidate_cache();
    }

    /// Take ownership of the child
    pub fn take_child(&mut self) -> Option<BoxedLayer> {
        self.invalidate_cache();
        self.child.take()
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

    /// Default bounds implementation - returns child's bounds
    pub fn child_bounds(&self) -> Rect {
        self.child.as_ref().map_or(Rect::ZERO, |c| c.bounds())
    }

    /// Default visibility check
    pub fn is_child_visible(&self) -> bool {
        !self.disposed && self.child.as_ref().is_some_and(|c| c.is_visible())
    }

    /// Default hit test - delegates to child
    pub fn child_hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        if self.disposed {
            return false;
        }
        self.child
            .as_ref()
            .is_some_and(|c| c.hit_test(position, result))
    }

    /// Default event handling - delegates to child
    pub fn child_handle_event(&mut self, event: &Event) -> bool {
        if self.disposed {
            return false;
        }
        self.child.as_mut().is_some_and(|c| c.handle_event(event))
    }

    /// Default disposal - disposes child and marks as disposed
    pub fn dispose_child(&mut self) {
        if let Some(mut child) = self.child.take() {
            child.dispose();
        }
        self.disposed = true;
        self.cached_bounds = None;
    }
}
