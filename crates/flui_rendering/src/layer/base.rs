//! Base layer types and traits.

use std::any::Any;
use std::fmt::Debug;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use flui_types::Rect;

// ============================================================================
// LayerId
// ============================================================================

/// Unique identifier for a layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayerId(u64);

impl LayerId {
    /// Creates a new unique layer ID.
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Returns the raw ID value.
    pub fn get(&self) -> u64 {
        self.0
    }
}

impl Default for LayerId {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Layer Trait
// ============================================================================

/// Base trait for all layers.
///
/// Layers form a tree structure that represents the composited output
/// of the render tree. Each frame, the layer tree is built during paint
/// and then submitted to the compositor.
///
/// # Layer Lifecycle
///
/// 1. Created during paint when compositing is needed
/// 2. Added to parent layer via `append_child`
/// 3. Scene built from layer tree via `SceneBuilder`
/// 4. Scene submitted to compositor
/// 5. Layers may be reused if retained
pub trait Layer: Debug + Send + Sync {
    /// Returns the unique ID of this layer.
    fn id(&self) -> LayerId;

    /// Returns the engine layer handle, if attached.
    fn engine_layer(&self) -> Option<&EngineLayer>;

    /// Sets the engine layer handle.
    fn set_engine_layer(&mut self, layer: Option<EngineLayer>);

    /// Returns the parent layer, if any.
    fn parent(&self) -> Option<&dyn Layer>;

    /// Removes this layer from its parent.
    fn remove(&mut self);

    /// Returns whether this layer needs to be added to the scene.
    fn needs_add_to_scene(&self) -> bool;

    /// Marks this layer as needing to be added to the scene.
    fn mark_needs_add_to_scene(&mut self);

    /// Updates the subtree needs compositing flag.
    fn update_subtree_needs_add_to_scene(&mut self);

    /// Builds this layer into the scene.
    fn add_to_scene(&mut self, builder: &mut SceneBuilder, layer_offset: flui_types::Offset);

    /// Finds the layer at the given point.
    fn find(&self, offset: flui_types::Offset) -> Option<&dyn Layer>;

    /// Returns the bounds of this layer's content.
    fn bounds(&self) -> Rect;

    /// Returns this layer as Any for downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Returns this layer as mutable Any for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// ============================================================================
// LayerHandle
// ============================================================================

/// A handle to a layer that manages ownership.
///
/// Similar to Flutter's `LayerHandle<T>` which ensures layers are properly
/// disposed when no longer needed.
#[derive(Debug)]
pub struct LayerHandle<T: Layer> {
    layer: Option<Arc<parking_lot::RwLock<T>>>,
}

impl<T: Layer> LayerHandle<T> {
    /// Creates a new empty layer handle.
    pub fn new() -> Self {
        Self { layer: None }
    }

    /// Creates a layer handle with the given layer.
    pub fn with_layer(layer: T) -> Self {
        Self {
            layer: Some(Arc::new(parking_lot::RwLock::new(layer))),
        }
    }

    /// Returns the layer, if any.
    pub fn layer(&self) -> Option<&Arc<parking_lot::RwLock<T>>> {
        self.layer.as_ref()
    }

    /// Sets the layer.
    pub fn set(&mut self, layer: Option<T>) {
        self.layer = layer.map(|l| Arc::new(parking_lot::RwLock::new(l)));
    }

    /// Returns whether this handle has a layer.
    pub fn is_some(&self) -> bool {
        self.layer.is_some()
    }

    /// Returns whether this handle is empty.
    pub fn is_none(&self) -> bool {
        self.layer.is_none()
    }
}

impl<T: Layer> Default for LayerHandle<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Layer> Clone for LayerHandle<T> {
    fn clone(&self) -> Self {
        Self {
            layer: self.layer.clone(),
        }
    }
}

// ============================================================================
// EngineLayer
// ============================================================================

/// A handle to a layer in the compositor/engine.
///
/// This represents the actual layer object managed by the rendering engine.
/// It's an opaque handle that can be retained across frames for efficient
/// layer reuse.
#[derive(Debug, Clone)]
pub struct EngineLayer {
    /// Unique identifier for this engine layer.
    id: u64,

    /// Whether this layer is still valid.
    disposed: bool,
}

impl EngineLayer {
    /// Creates a new engine layer with the given ID.
    pub fn new(id: u64) -> Self {
        Self {
            id,
            disposed: false,
        }
    }

    /// Returns the ID of this engine layer.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Returns whether this layer has been disposed.
    pub fn is_disposed(&self) -> bool {
        self.disposed
    }

    /// Disposes this layer.
    pub fn dispose(&mut self) {
        self.disposed = true;
    }
}

// ============================================================================
// SceneBuilder (forward declaration)
// ============================================================================

/// Builder for constructing a Scene from layers.
///
/// This is forward-declared here to allow Layer trait to reference it.
/// Full implementation is in the scene module.
#[derive(Debug, Default)]
pub struct SceneBuilder {
    /// Stack of layer operations.
    operations: Vec<SceneOperation>,
}

impl SceneBuilder {
    /// Creates a new scene builder.
    pub fn new() -> Self {
        Self {
            operations: Vec::new(),
        }
    }

    /// Pushes a transform onto the scene.
    pub fn push_transform(&mut self, matrix: [f32; 16]) {
        self.operations
            .push(SceneOperation::PushTransform { matrix });
    }

    /// Pushes an offset transform onto the scene.
    pub fn push_offset(&mut self, dx: f32, dy: f32) {
        self.operations.push(SceneOperation::PushOffset { dx, dy });
    }

    /// Pushes a clip rect onto the scene.
    pub fn push_clip_rect(&mut self, rect: Rect) {
        self.operations.push(SceneOperation::PushClipRect { rect });
    }

    /// Pushes a clip rounded rect onto the scene.
    pub fn push_clip_rrect(&mut self, rrect: flui_types::RRect) {
        self.operations
            .push(SceneOperation::PushClipRRect { rrect });
    }

    /// Pushes an opacity layer onto the scene.
    pub fn push_opacity(&mut self, alpha: u8, offset: flui_types::Offset) {
        self.operations
            .push(SceneOperation::PushOpacity { alpha, offset });
    }

    /// Pops the current layer from the scene.
    pub fn pop(&mut self) {
        self.operations.push(SceneOperation::Pop);
    }

    /// Adds a picture to the scene.
    pub fn add_picture(&mut self, offset: flui_types::Offset, picture_id: u64) {
        self.operations
            .push(SceneOperation::AddPicture { offset, picture_id });
    }

    /// Returns the operations.
    pub fn operations(&self) -> &[SceneOperation] {
        &self.operations
    }

    /// Returns the number of operations.
    pub fn operation_count(&self) -> usize {
        self.operations.len()
    }

    /// Returns whether the builder is empty.
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    /// Clears all operations.
    pub fn clear(&mut self) {
        self.operations.clear();
    }

    /// Takes ownership of the operations.
    pub fn take_operations(&mut self) -> Vec<SceneOperation> {
        std::mem::take(&mut self.operations)
    }
}

/// An operation in the scene builder.
#[derive(Debug, Clone)]
pub enum SceneOperation {
    /// Push a transform.
    PushTransform {
        /// The 4x4 transformation matrix (column-major).
        matrix: [f32; 16],
    },
    /// Push an offset.
    PushOffset {
        /// X offset.
        dx: f32,
        /// Y offset.
        dy: f32,
    },
    /// Push a clip rect.
    PushClipRect {
        /// The clip rectangle.
        rect: Rect,
    },
    /// Push a clip rounded rect.
    PushClipRRect {
        /// The clip rounded rectangle.
        rrect: flui_types::RRect,
    },
    /// Push opacity.
    PushOpacity {
        /// Opacity (0-255).
        alpha: u8,
        /// Offset for the opacity layer.
        offset: flui_types::Offset,
    },
    /// Pop a layer.
    Pop,
    /// Add a picture.
    AddPicture {
        /// Offset for the picture.
        offset: flui_types::Offset,
        /// Picture ID.
        picture_id: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer_id_unique() {
        let id1 = LayerId::new();
        let id2 = LayerId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_layer_handle_empty() {
        let handle: LayerHandle<crate::layer::OffsetLayer> = LayerHandle::new();
        assert!(handle.is_none());
    }

    #[test]
    fn test_engine_layer() {
        let mut layer = EngineLayer::new(42);
        assert_eq!(layer.id(), 42);
        assert!(!layer.is_disposed());

        layer.dispose();
        assert!(layer.is_disposed());
    }
}
