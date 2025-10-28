//! Layer handle for resource management
//!
//! Layers retain resources between frames. A LayerHandle maintains a reference
//! to a layer and automatically handles cleanup when the handle is dropped.

use std::sync::Arc;
use parking_lot::RwLock;

/// A handle to a layer that manages its lifecycle
///
/// # Resource Management
///
/// Layers retain GPU resources (textures, buffers, etc.) between frames for
/// performance. When a handle is dropped, the layer's resources are released.
///
/// # Example
///
/// ```rust,ignore
/// struct ClippingRenderObject {
///     clip_layer: LayerHandle<ClipRectLayer>,
/// }
///
/// impl ClippingRenderObject {
///     fn paint(&mut self, context: &mut PaintingContext, offset: Offset) {
///         // Reuse existing layer or create new one
///         self.clip_layer.layer = Some(context.push_clip_rect(
///             self.needs_compositing,
///             offset,
///             Offset::ZERO,
///             self.size,
///             |painter| self.paint_children(painter),
///             old_layer: self.clip_layer.layer.take(),
///         ));
///     }
///
///     fn dispose(&mut self) {
///         // Automatically releases resources
///         self.clip_layer.layer = None;
///     }
/// }
/// ```
pub struct LayerHandle<L> {
    /// The layer being managed (None if disposed)
    layer: Option<Arc<RwLock<L>>>,
}

impl<L> LayerHandle<L> {
    /// Create a new empty layer handle
    pub fn new() -> Self {
        Self { layer: None }
    }

    /// Get a reference to the layer, if it exists
    pub fn get(&self) -> Option<Arc<RwLock<L>>> {
        self.layer.clone()
    }

    /// Set the layer
    pub fn set(&mut self, layer: Option<L>) {
        self.layer = layer.map(|l| Arc::new(RwLock::new(l)));
    }

    /// Set the layer from an Arc
    pub fn set_arc(&mut self, layer: Option<Arc<RwLock<L>>>) {
        self.layer = layer;
    }

    /// Take the layer, leaving None in its place
    pub fn take(&mut self) -> Option<Arc<RwLock<L>>> {
        self.layer.take()
    }

    /// Check if this handle has a layer
    pub fn is_some(&self) -> bool {
        self.layer.is_some()
    }

    /// Check if this handle is empty
    pub fn is_none(&self) -> bool {
        self.layer.is_none()
    }

    /// Clear the handle, releasing the layer
    pub fn clear(&mut self) {
        self.layer = None;
    }
}

impl<L> Default for LayerHandle<L> {
    fn default() -> Self {
        Self::new()
    }
}

impl<L> Clone for LayerHandle<L> {
    fn clone(&self) -> Self {
        Self {
            layer: self.layer.clone(),
        }
    }
}

impl<L> Drop for LayerHandle<L> {
    fn drop(&mut self) {
        // Layer resources are automatically released when Arc ref count reaches 0
        self.layer = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestLayer {
        id: u32,
    }

    #[test]
    fn test_layer_handle_lifecycle() {
        let mut handle = LayerHandle::<TestLayer>::new();
        assert!(handle.is_none());

        // Set a layer
        handle.set(Some(TestLayer { id: 42 }));
        assert!(handle.is_some());

        // Get the layer
        let layer_ref = handle.get().unwrap();
        assert_eq!(layer_ref.read().id, 42);

        // Clear the handle
        handle.clear();
        assert!(handle.is_none());
    }

    #[test]
    fn test_layer_handle_clone() {
        let mut handle1 = LayerHandle::<TestLayer>::new();
        handle1.set(Some(TestLayer { id: 100 }));

        let handle2 = handle1.clone();
        assert!(handle2.is_some());

        // Both handles point to the same layer
        assert_eq!(
            Arc::strong_count(&handle1.get().unwrap()),
            2
        );
    }

    #[test]
    fn test_layer_handle_take() {
        let mut handle = LayerHandle::<TestLayer>::new();
        handle.set(Some(TestLayer { id: 7 }));

        let taken = handle.take();
        assert!(taken.is_some());
        assert!(handle.is_none());

        assert_eq!(taken.unwrap().read().id, 7);
    }
}
