//! Abstract Layer base class
//!
//! A composited layer that represents a visual element in the scene graph.

use flui_types::Rect;
use crate::painter::Painter;
use std::sync::Arc;
use parking_lot::RwLock;

/// Layer lifecycle state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerState {
    /// Layer is active and can be used
    Active,
    /// Layer has been disposed and must not be used
    Disposed,
}

/// Abstract base for all composited layers
///
/// This is the main Layer trait that all layer types must implement.
/// It provides the core functionality for the composited layer system.
///
/// # Layer Tree
///
/// During painting, the render tree generates a tree of composited layers that
/// are uploaded to the engine and displayed by the compositor.
///
/// ```text
/// ContainerLayer (root)
///   ├─ TransformLayer
///   │   └─ OpacityLayer
///   │       └─ PictureLayer (actual drawing)
///   └─ ClipRectLayer
///       └─ PictureLayer
/// ```
///
/// # Paint vs Add to Scene
///
/// This trait provides `paint()` for immediate rendering and will later include
/// `add_to_scene()` when SceneBuilder is implemented. For now, all layers must
/// implement `paint()` which is used by the current Compositor.
///
/// # Mutability
///
/// Most layers can have their properties mutated, and layers can be moved to
/// different parents. The scene must be explicitly recomposited after such
/// changes; the layer tree does not maintain its own dirty state.
///
/// # Memory Management
///
/// Layers retain resources between frames to speed up rendering. A layer will
/// retain these resources until all `LayerHandle`s referring to the layer have
/// been dropped or nulled out.
///
/// **IMPORTANT**: Layers must not be used after disposal.
///
/// # Example
///
/// ```rust,ignore
/// use flui_engine::layer::{Layer, LayerHandle, ClipLayer};
///
/// struct ClippingRenderObject {
///     clip_layer_handle: LayerHandle<ClipLayer>,
/// }
///
/// impl ClippingRenderObject {
///     fn paint(&mut self, context: &mut PaintingContext, offset: Offset) {
///         // Create or reuse layer
///         let old_layer = self.clip_layer_handle.take();
///         // ... paint logic ...
///         self.clip_layer_handle.set(Some(new_layer));
///     }
///
///     fn dispose(&mut self) {
///         self.clip_layer_handle.clear();
///     }
/// }
/// ```
pub trait Layer: Send + Sync {
    /// Paint this layer using the given painter
    ///
    /// This is the current method used by the Compositor to render layers.
    /// In the future, this will be supplemented by `add_to_scene()` when
    /// SceneBuilder is implemented.
    ///
    /// # Arguments
    ///
    /// * `painter` - The painter to render with
    fn paint(&self, painter: &mut dyn Painter);

    /// Get the bounding rectangle of this layer
    ///
    /// Used for culling and optimization. Layers outside the viewport
    /// don't need to be painted.
    fn bounds(&self) -> Rect;

    /// Check if this layer is visible
    ///
    /// Invisible layers can be skipped during painting.
    fn is_visible(&self) -> bool {
        true
    }

    /// Mark this layer as needing to be repainted
    ///
    /// This is typically called when the layer's visual appearance has changed.
    fn mark_needs_paint(&mut self) {
        // Default implementation does nothing
        // Subclasses can override to implement dirty tracking
    }

    /// Dispose of this layer and release its resources
    ///
    /// After calling dispose, the layer must not be used.
    fn dispose(&mut self) {
        // Default implementation does nothing
        // Subclasses should override to clean up resources
    }

    /// Check if this layer has been disposed
    fn is_disposed(&self) -> bool {
        false // Default: not disposed
    }

    /// Attach this layer to a parent
    ///
    /// Called when the layer is added to a parent container.
    fn attach(&mut self, _parent: Option<Arc<RwLock<dyn Layer>>>) {
        // Default implementation does nothing
    }

    /// Detach this layer from its parent
    ///
    /// Called when the layer is removed from a parent container.
    fn detach(&mut self) {
        // Default implementation does nothing
    }

    /// Get a debug description of this layer
    fn debug_description(&self) -> String {
        format!("Layer({:?})", self.bounds())
    }
}

/// Extension trait for downcasting Layer trait objects
pub trait LayerExt {
    /// Attempt to downcast to a concrete layer type
    fn downcast_ref<T: 'static>(&self) -> Option<&T>;

    /// Attempt to downcast to a mutable concrete layer type
    fn downcast_mut<T: 'static>(&mut self) -> Option<&mut T>;
}

// Helper struct for type-erased layers
pub struct AnyLayer {
    inner: Arc<RwLock<dyn Layer>>,
}

impl AnyLayer {
    pub fn new<L: Layer + 'static>(layer: L) -> Self {
        Self {
            inner: Arc::new(RwLock::new(layer)),
        }
    }

    pub fn get(&self) -> Arc<RwLock<dyn Layer>> {
        self.inner.clone()
    }
}

impl Clone for AnyLayer {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestLayer {
        bounds: Rect,
        disposed: bool,
    }

    impl TestLayer {
        fn new(bounds: Rect) -> Self {
            Self {
                bounds,
                disposed: false,
            }
        }
    }

    impl Layer for TestLayer {
        fn paint(&self, painter: &mut dyn Painter) {
            assert!(!self.disposed, "Cannot use disposed layer");
            let _ = painter; // Suppress unused variable warning
        }

        fn bounds(&self) -> Rect {
            self.bounds
        }

        fn dispose(&mut self) {
            self.disposed = true;
        }

        fn is_disposed(&self) -> bool {
            self.disposed
        }
    }

    #[test]
    fn test_layer_lifecycle() {
        let mut layer = TestLayer::new(Rect::from_xywh(0.0, 0.0, 100.0, 100.0));

        assert!(!layer.is_disposed());
        assert_eq!(layer.bounds(), Rect::from_xywh(0.0, 0.0, 100.0, 100.0));

        layer.dispose();
        assert!(layer.is_disposed());
    }

    #[test]
    fn test_any_layer() {
        let layer = TestLayer::new(Rect::from_xywh(10.0, 20.0, 50.0, 50.0));
        let any = AnyLayer::new(layer);

        let layer_ref = any.get();
        let bounds = layer_ref.read().bounds();
        assert_eq!(bounds, Rect::from_xywh(10.0, 20.0, 50.0, 50.0));
    }
}
