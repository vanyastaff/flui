//! LayerHandle - Reference-counted handle for layer lifecycle management
//!
//! Prevents a Layer's GPU resources from being disposed prematurely.
//! Similar to Flutter's `LayerHandle<T extends Layer>`.
//!
//! # Architecture
//!
//! ```text
//! RenderObject
//!   │
//!   ├─ LayerHandle<OpacityLayer>  ─────┐
//!   │                                   │
//!   └─ LayerHandle<OffsetLayer>  ──────┼──► LayerTree
//!                                       │     (actual storage)
//! Another RenderObject                  │
//!   │                                   │
//!   └─ LayerHandle<CanvasLayer>  ──────┘
//! ```
//!
//! # Lifecycle
//!
//! 1. RenderObject creates LayerHandle during paint
//! 2. Handle holds reference to layer in tree
//! 3. When handle is dropped or set to None, ref count decreases
//! 4. When ref count reaches 0, GPU resources can be released

use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use flui_foundation::LayerId;

use crate::layer::Layer;

// ============================================================================
// LAYER HANDLE
// ============================================================================

/// A handle that manages the lifecycle of a Layer's GPU resources.
///
/// LayerHandle prevents a Layer's platform graphics resources from being
/// disposed prematurely. GPU resources like textures and engine layers can
/// consume significant memory, so proper lifecycle management is critical.
///
/// # Usage
///
/// ```rust
/// use flui_layer::{LayerHandle, Layer, OpacityLayer};
///
/// // Create a handle (initially empty)
/// let mut handle: LayerHandle<OpacityLayer> = LayerHandle::new();
///
/// // Set the layer
/// let layer = OpacityLayer::new(0.5);
/// handle.set(layer);
///
/// // Access the layer
/// if let Some(opacity) = handle.get() {
///     println!("Alpha: {}", opacity.alpha());
/// }
///
/// // Clear the handle (releases reference)
/// handle.clear();
/// ```
///
/// # RenderObject Integration
///
/// RenderObjects typically hold handles to their layers:
///
/// ```rust,ignore
/// struct MyRenderObject {
///     layer_handle: LayerHandle<OpacityLayer>,
/// }
///
/// impl MyRenderObject {
///     fn paint(&mut self, context: &mut PaintContext) {
///         // Reuse existing layer or create new one
///         let layer = self.layer_handle.get_or_insert_with(|| {
///             OpacityLayer::new(self.opacity)
///         });
///
///         // Update layer properties
///         layer.set_alpha(self.opacity);
///
///         // Push to context
///         context.push_layer(layer);
///     }
/// }
/// ```
pub struct LayerHandle<T> {
    /// The layer data (owned)
    layer: Option<T>,

    /// Reference count for tracking usage
    ref_count: Arc<AtomicUsize>,

    /// Associated LayerId (if inserted into tree)
    layer_id: Option<LayerId>,
}

impl<T> LayerHandle<T> {
    /// Creates a new empty LayerHandle.
    #[inline]
    pub fn new() -> Self {
        Self {
            layer: None,
            ref_count: Arc::new(AtomicUsize::new(0)),
            layer_id: None,
        }
    }

    /// Creates a LayerHandle with an initial layer.
    #[inline]
    pub fn with_layer(layer: T) -> Self {
        Self {
            layer: Some(layer),
            ref_count: Arc::new(AtomicUsize::new(1)),
            layer_id: None,
        }
    }

    /// Returns true if this handle has a layer.
    #[inline]
    pub fn has_layer(&self) -> bool {
        self.layer.is_some()
    }

    /// Returns the layer reference, if any.
    #[inline]
    pub fn get(&self) -> Option<&T> {
        self.layer.as_ref()
    }

    /// Returns a mutable layer reference, if any.
    #[inline]
    pub fn get_mut(&mut self) -> Option<&mut T> {
        self.layer.as_mut()
    }

    /// Sets the layer, replacing any existing one.
    ///
    /// Returns the previous layer, if any.
    pub fn set(&mut self, layer: T) -> Option<T> {
        let old = self.layer.take();

        // Update ref count
        if old.is_none() {
            self.ref_count.fetch_add(1, Ordering::SeqCst);
        }

        self.layer = Some(layer);
        old
    }

    /// Clears the layer from this handle.
    ///
    /// Returns the removed layer, if any.
    pub fn clear(&mut self) -> Option<T> {
        let old = self.layer.take();

        if old.is_some() {
            self.ref_count.fetch_sub(1, Ordering::SeqCst);
        }

        self.layer_id = None;
        old
    }

    /// Takes the layer out of this handle, leaving None.
    #[inline]
    pub fn take(&mut self) -> Option<T> {
        self.clear()
    }

    /// Gets the layer or inserts a default value.
    pub fn get_or_insert(&mut self, layer: T) -> &mut T {
        if self.layer.is_none() {
            self.set(layer);
        }
        self.layer.as_mut().unwrap()
    }

    /// Gets the layer or inserts with a function.
    pub fn get_or_insert_with<F>(&mut self, f: F) -> &mut T
    where
        F: FnOnce() -> T,
    {
        if self.layer.is_none() {
            self.set(f());
        }
        self.layer.as_mut().unwrap()
    }

    /// Returns the current reference count.
    #[inline]
    pub fn ref_count(&self) -> usize {
        self.ref_count.load(Ordering::SeqCst)
    }

    /// Returns the associated LayerId, if any.
    #[inline]
    pub fn layer_id(&self) -> Option<LayerId> {
        self.layer_id
    }

    /// Sets the associated LayerId.
    #[inline]
    pub fn set_layer_id(&mut self, id: Option<LayerId>) {
        self.layer_id = id;
    }
}

impl<T: Default> LayerHandle<T> {
    /// Gets the layer or inserts the default value.
    pub fn get_or_default(&mut self) -> &mut T {
        self.get_or_insert_with(T::default)
    }
}

impl<T> Default for LayerHandle<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> Clone for LayerHandle<T> {
    fn clone(&self) -> Self {
        if self.layer.is_some() {
            self.ref_count.fetch_add(1, Ordering::SeqCst);
        }
        Self {
            layer: self.layer.clone(),
            ref_count: Arc::clone(&self.ref_count),
            layer_id: self.layer_id,
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for LayerHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LayerHandle")
            .field("layer", &self.layer)
            .field("ref_count", &self.ref_count())
            .field("layer_id", &self.layer_id)
            .finish()
    }
}

impl<T> Drop for LayerHandle<T> {
    fn drop(&mut self) {
        if self.layer.is_some() {
            self.ref_count.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

// Thread safety
unsafe impl<T: Send> Send for LayerHandle<T> {}
unsafe impl<T: Sync> Sync for LayerHandle<T> {}

// ============================================================================
// TYPE ALIASES
// ============================================================================

use crate::layer::{
    AnnotatedRegionLayer, BackdropFilterLayer, CanvasLayer, ClipPathLayer, ClipRRectLayer,
    ClipRectLayer, ColorFilterLayer, FollowerLayer, ImageFilterLayer, LeaderLayer, OffsetLayer,
    OpacityLayer, PlatformViewLayer, ShaderMaskLayer, TextureLayer, TransformLayer,
};

/// Handle for CanvasLayer
pub type CanvasLayerHandle = LayerHandle<CanvasLayer>;

/// Handle for ClipRectLayer
pub type ClipRectLayerHandle = LayerHandle<ClipRectLayer>;

/// Handle for ClipRRectLayer
pub type ClipRRectLayerHandle = LayerHandle<ClipRRectLayer>;

/// Handle for ClipPathLayer
pub type ClipPathLayerHandle = LayerHandle<ClipPathLayer>;

/// Handle for OffsetLayer
pub type OffsetLayerHandle = LayerHandle<OffsetLayer>;

/// Handle for TransformLayer
pub type TransformLayerHandle = LayerHandle<TransformLayer>;

/// Handle for OpacityLayer
pub type OpacityLayerHandle = LayerHandle<OpacityLayer>;

/// Handle for ColorFilterLayer
pub type ColorFilterLayerHandle = LayerHandle<ColorFilterLayer>;

/// Handle for ImageFilterLayer
pub type ImageFilterLayerHandle = LayerHandle<ImageFilterLayer>;

/// Handle for ShaderMaskLayer
pub type ShaderMaskLayerHandle = LayerHandle<ShaderMaskLayer>;

/// Handle for BackdropFilterLayer
pub type BackdropFilterLayerHandle = LayerHandle<BackdropFilterLayer>;

/// Handle for TextureLayer
pub type TextureLayerHandle = LayerHandle<TextureLayer>;

/// Handle for PlatformViewLayer
pub type PlatformViewLayerHandle = LayerHandle<PlatformViewLayer>;

/// Handle for LeaderLayer
pub type LeaderLayerHandle = LayerHandle<LeaderLayer>;

/// Handle for FollowerLayer
pub type FollowerLayerHandle = LayerHandle<FollowerLayer>;

/// Handle for AnnotatedRegionLayer
pub type AnnotatedRegionLayerHandle = LayerHandle<AnnotatedRegionLayer>;

/// Handle for the Layer enum (polymorphic)
pub type AnyLayerHandle = LayerHandle<Layer>;

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::px;

    #[test]
    fn test_layer_handle_new() {
        let handle: LayerHandle<OpacityLayer> = LayerHandle::new();
        assert!(!handle.has_layer());
        assert!(handle.get().is_none());
        assert_eq!(handle.ref_count(), 0);
    }

    #[test]
    fn test_layer_handle_with_layer() {
        let handle = LayerHandle::with_layer(OpacityLayer::new(0.5));
        assert!(handle.has_layer());
        assert_eq!(handle.ref_count(), 1);
        assert_eq!(handle.get().unwrap().alpha(), 0.5);
    }

    #[test]
    fn test_layer_handle_set() {
        let mut handle: LayerHandle<OpacityLayer> = LayerHandle::new();
        assert_eq!(handle.ref_count(), 0);

        handle.set(OpacityLayer::new(0.8));
        assert!(handle.has_layer());
        assert_eq!(handle.ref_count(), 1);
    }

    #[test]
    fn test_layer_handle_clear() {
        let mut handle = LayerHandle::with_layer(OpacityLayer::new(0.5));
        assert_eq!(handle.ref_count(), 1);

        let old = handle.clear();
        assert!(old.is_some());
        assert!(!handle.has_layer());
        assert_eq!(handle.ref_count(), 0);
    }

    #[test]
    fn test_layer_handle_get_or_insert() {
        let mut handle: LayerHandle<OpacityLayer> = LayerHandle::new();

        let layer = handle.get_or_insert(OpacityLayer::new(0.3));
        assert_eq!(layer.alpha(), 0.3);
        assert_eq!(handle.ref_count(), 1);

        // Second call should return existing layer
        let layer2 = handle.get_or_insert(OpacityLayer::new(0.9));
        assert_eq!(layer2.alpha(), 0.3); // Still 0.3, not 0.9
    }

    #[test]
    fn test_layer_handle_get_or_insert_with() {
        let mut handle: LayerHandle<OffsetLayer> = LayerHandle::new();

        let layer = handle.get_or_insert_with(|| OffsetLayer::from_xy(10.0, 20.0));
        assert_eq!(layer.offset().dx, px(10.0));
    }

    #[test]
    fn test_layer_handle_clone_shares_ref_count() {
        let handle1 = LayerHandle::with_layer(OpacityLayer::new(0.5));
        assert_eq!(handle1.ref_count(), 1);

        let handle2 = handle1.clone();
        assert_eq!(handle1.ref_count(), 2);
        assert_eq!(handle2.ref_count(), 2);

        drop(handle2);
        assert_eq!(handle1.ref_count(), 1);
    }

    #[test]
    fn test_layer_handle_drop_decrements_ref_count() {
        let ref_count;
        {
            let handle = LayerHandle::with_layer(OpacityLayer::new(0.5));
            ref_count = Arc::clone(&handle.ref_count);
            assert_eq!(ref_count.load(Ordering::SeqCst), 1);
        }
        // After drop
        assert_eq!(ref_count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_layer_handle_layer_id() {
        let mut handle = LayerHandle::with_layer(OpacityLayer::new(0.5));
        assert!(handle.layer_id().is_none());

        let id = LayerId::new(42);
        handle.set_layer_id(Some(id));
        assert_eq!(handle.layer_id(), Some(id));
    }

    #[test]
    fn test_layer_handle_get_mut() {
        let mut handle = LayerHandle::with_layer(OpacityLayer::new(0.5));

        if let Some(layer) = handle.get_mut() {
            layer.set_alpha(0.8);
        }

        assert_eq!(handle.get().unwrap().alpha(), 0.8);
    }

    #[test]
    fn test_layer_handle_take() {
        let mut handle = LayerHandle::with_layer(OpacityLayer::new(0.5));
        assert!(handle.has_layer());

        let taken = handle.take();
        assert!(taken.is_some());
        assert!(!handle.has_layer());
    }

    #[test]
    fn test_any_layer_handle() {
        let mut handle: AnyLayerHandle = LayerHandle::new();

        handle.set(Layer::Opacity(OpacityLayer::new(0.5)));
        assert!(handle.has_layer());
        assert!(handle.get().unwrap().is_opacity());
    }

    #[test]
    fn test_layer_handle_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<LayerHandle<OpacityLayer>>();
        assert_sync::<LayerHandle<OpacityLayer>>();
    }
}
