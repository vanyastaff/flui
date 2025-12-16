//! # FLUI Layer - Compositor Layer Tree
//!
//! This crate provides the Layer tree - the fourth tree in FLUI's 5-tree architecture:
//! View → Element → Render → **Layer** → Semantics
//!
//! ## Architecture
//!
//! Layers handle compositing and GPU optimization. They're created at repaint
//! boundaries and cached for efficient rendering.
//!
//! ```text
//! RenderObject (flui_rendering)
//!     │
//!     │ paint() generates Canvas OR pushes Layer
//!     ▼
//! Layer (this crate)
//!     │
//!     │ render() → CommandRenderer
//!     ▼
//! GPU Rendering (wgpu via flui_engine)
//! ```
//!
//! ## Layer Types
//!
//! ### Leaf Layers
//! - **CanvasLayer**: Standard canvas drawing commands
//! - **TextureLayer**: External GPU texture rendering (video, camera)
//! - **PlatformViewLayer**: Native platform view embedding
//!
//! ### Clip Layers
//! - **ClipRectLayer**: Rectangular clipping
//! - **ClipRRectLayer**: Rounded rectangle clipping
//! - **ClipPathLayer**: Arbitrary path clipping
//!
//! ### Transform Layers
//! - **OffsetLayer**: Simple translation (optimized for repaint boundaries)
//! - **TransformLayer**: Full matrix transformation
//!
//! ### Effect Layers
//! - **OpacityLayer**: Alpha blending
//! - **ColorFilterLayer**: Color matrix transformation
//! - **ImageFilterLayer**: Blur, dilate, erode effects
//! - **ShaderMaskLayer**: GPU shader masking effects (gradient fades, vignettes)
//! - **BackdropFilterLayer**: Backdrop filtering effects (frosted glass, blur)
//!
//! ### Linking Layers
//! - **LeaderLayer**: Anchor point for linked positioning
//! - **FollowerLayer**: Positions content relative to a leader
//!
//! ### Annotation Layers
//! - **AnnotatedRegionLayer**: Metadata regions for system UI integration
//!
//! ## Tree Integration
//!
//! LayerTree implements `TreeRead<LayerId>` and `TreeNav<LayerId>` from `flui-tree`,
//! enabling generic tree algorithms and visitors.
//!
//! ```rust,ignore
//! use flui_layer::{LayerTree, Layer, CanvasLayer};
//! use flui_foundation::LayerId;
//! use flui_tree::{TreeRead, TreeNav};
//!
//! let mut tree = LayerTree::new();
//! let id = tree.insert(Layer::Canvas(CanvasLayer::new()));
//!
//! // Use generic tree operations
//! assert!(tree.contains(id));
//! ```
//!
//! ## Design Principles
//!
//! 1. **Uses canonical IDs**: `LayerId` from `flui-foundation`
//! 2. **Implements tree traits**: `TreeRead<LayerId>`, `TreeNav<LayerId>`
//! 3. **Separation of concerns**: Layer types here, rendering in `flui_engine`
//! 4. **Thread-safe**: All types are `Send + Sync`

#![warn(rust_2018_idioms, clippy::all, clippy::pedantic)]
#![allow(
    dead_code,
    unused_variables,
    missing_docs,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use,
    clippy::doc_markdown,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

// ============================================================================
// MODULES
// ============================================================================

mod compositor;
mod handle;
mod link_registry;
mod scene;

pub mod layer;
pub mod tree;

// ============================================================================
// RE-EXPORTS - Layer Types
// ============================================================================

pub use layer::{
    // Annotation layers
    AnnotatedRegionLayer,
    AnnotationValue,
    // Effect layers
    BackdropFilterLayer,
    // Leaf layers
    CanvasLayer,
    // Clip layers
    ClipPathLayer,
    ClipRRectLayer,
    ClipRectLayer,
    ColorFilterLayer,
    // Linking layers
    FollowerLayer,
    ImageFilterLayer,
    // Enum and trait
    Layer,
    LayerBounds,
    LayerLink,
    LeaderLayer,
    // Transform layers
    OffsetLayer,
    OpacityLayer,
    // Performance overlay
    PerformanceOverlayLayer,
    PerformanceOverlayOption,
    PictureLayer,
    // Platform layers
    PlatformViewHitTestBehavior,
    PlatformViewId,
    PlatformViewLayer,
    // Annotation types
    SemanticLabel,
    ShaderMaskLayer,
    SystemUiOverlayStyle,
    TextureLayer,
    TransformLayer,
};

// Re-export annotation search types
pub use layer::annotation::{AnnotationEntry, AnnotationResult, AnnotationSearchOptions};

// Re-export composition callback types
pub use layer::composition_callback::{
    CompositionCallbackHandle, CompositionCallbackId, CompositionCallbackRegistry,
    HasCompositionCallbacks,
};

// ============================================================================
// RE-EXPORTS - Tree
// ============================================================================

pub use tree::{LayerNode, LayerTree};

// ============================================================================
// RE-EXPORTS - Compositor
// ============================================================================

pub use compositor::{CompositorStats, SceneBuilder, SceneCompositor};
pub use scene::Scene;

// ============================================================================
// RE-EXPORTS - Link Registry
// ============================================================================

pub use link_registry::{LeaderInfo, LinkRegistry};

// ============================================================================
// RE-EXPORTS - Handle
// ============================================================================

pub use handle::{
    AnnotatedRegionLayerHandle, AnyLayerHandle, BackdropFilterLayerHandle, CanvasLayerHandle,
    ClipPathLayerHandle, ClipRRectLayerHandle, ClipRectLayerHandle, ColorFilterLayerHandle,
    FollowerLayerHandle, ImageFilterLayerHandle, LayerHandle, LeaderLayerHandle, OffsetLayerHandle,
    OpacityLayerHandle, PlatformViewLayerHandle, ShaderMaskLayerHandle, TextureLayerHandle,
    TransformLayerHandle,
};

// ============================================================================
// RE-EXPORTS - Foundation Types
// ============================================================================

pub use flui_foundation::LayerId;

// ============================================================================
// PRELUDE
// ============================================================================

/// The layer prelude - commonly used types and traits.
///
/// ```rust,ignore
/// use flui_layer::prelude::*;
/// ```
pub mod prelude {
    // Leaf layers
    pub use crate::{
        CanvasLayer, PerformanceOverlayLayer, PerformanceOverlayOption, PlatformViewLayer,
        TextureLayer,
    };

    // Clip layers
    pub use crate::{ClipPathLayer, ClipRRectLayer, ClipRectLayer};

    // Transform layers
    pub use crate::{OffsetLayer, TransformLayer};

    // Effect layers
    pub use crate::{
        BackdropFilterLayer, ColorFilterLayer, ImageFilterLayer, OpacityLayer, ShaderMaskLayer,
    };

    // Linking layers
    pub use crate::{FollowerLayer, LayerLink, LeaderLayer};

    // Annotation layers
    pub use crate::{AnnotatedRegionLayer, SemanticLabel, SystemUiOverlayStyle};

    // Annotation search
    pub use crate::{AnnotationEntry, AnnotationResult, AnnotationSearchOptions};

    // Composition callbacks
    pub use crate::{CompositionCallbackRegistry, HasCompositionCallbacks};

    // Platform types
    pub use crate::{PlatformViewHitTestBehavior, PlatformViewId};

    // Core types
    pub use crate::{Layer, LayerBounds, LayerId, LayerNode, LayerTree};

    // Compositor
    pub use crate::{LinkRegistry, Scene, SceneBuilder, SceneCompositor};

    // Handle
    pub use crate::{AnyLayerHandle, LayerHandle};

    // Re-export tree traits for convenience
    pub use flui_tree::{TreeNav, TreeRead};
}

// ============================================================================
// VERSION INFO
// ============================================================================

/// The version of the flui-layer crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::Rect;
    use flui_types::painting::Clip;

    #[test]
    fn test_version() {
        assert_eq!(VERSION, "0.1.0");
    }

    #[test]
    fn test_layer_tree_basic() {
        let mut tree = LayerTree::new();
        assert!(tree.is_empty());

        let canvas = CanvasLayer::new();
        let id = tree.insert(Layer::Canvas(canvas));

        assert!(!tree.is_empty());
        assert_eq!(tree.len(), 1);
        assert!(tree.contains(id));
    }

    #[test]
    fn test_all_layer_types() {
        let mut tree = LayerTree::new();

        // Leaf
        let _ = tree.insert(Layer::Canvas(CanvasLayer::new()));

        // Clip layers
        let _ = tree.insert(Layer::ClipRect(ClipRectLayer::new(
            Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
            Clip::HardEdge,
        )));

        // Transform layers
        let _ = tree.insert(Layer::Offset(OffsetLayer::from_xy(10.0, 20.0)));
        let _ = tree.insert(Layer::Transform(TransformLayer::identity()));

        // Effect layers
        let _ = tree.insert(Layer::Opacity(OpacityLayer::new(0.5)));
        let _ = tree.insert(Layer::ColorFilter(ColorFilterLayer::grayscale()));
        let _ = tree.insert(Layer::ImageFilter(ImageFilterLayer::blur(5.0)));

        assert_eq!(tree.len(), 7);
    }
}
