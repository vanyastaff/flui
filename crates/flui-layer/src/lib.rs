//! # FLUI Layer - Compositor Layer Tree
//!
//! This crate provides the Layer tree - the fourth tree in FLUI's 5-tree
//! architecture: View → Element → Render → **Layer** → Semantics
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
//! - **ClipSuperellipseLayer**: iOS-style squircle clipping
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
//! - **ShaderMaskLayer**: GPU shader masking effects (gradient fades,
//!   vignettes)
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
//! LayerTree implements `TreeRead<LayerId>` and `TreeNav<LayerId>` from
//! `flui-tree`, enabling generic tree algorithms and visitors.
//!
//! ```rust,ignore
//! use flui_layer::{LayerTree, Layer, CanvasLayer};
//! use flui_foundation::LayerId;
//! use flui_tree::{TreeRead, TreeNav};
//!
//! let mut tree = LayerTree::new();
//! let id = tree.insert(Layer::from(CanvasLayer::new()));
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

// Lint levels come from `[workspace.lints]` (Cargo.toml `[lints] workspace = true`).
// Ship bar (wave 2): every public item is documented; keep it that way.
#![deny(missing_docs)]

// ============================================================================
// MODULES
// ============================================================================

mod compositor;
pub mod damage;
mod error;
mod link_registry;
mod scene;
// The owned per-window per-frame raster package.
mod scene_snapshot;

pub mod layer;
// Layer-tree test harness. Compiled only for this crate's own tests
// (`cfg(test)`) or when a consumer enables the `testing` feature. Provides a
// declarative `LayerTree` builder, structural/bounds inspection, and a
// `Diagnosticable`-backed tree dump. See [`testing`] for the overview.
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod tree;

pub use error::{LayerError, LayerResult};

// ============================================================================
// RE-EXPORTS - Layer Types
// ============================================================================

// ============================================================================
// RE-EXPORTS - Compositor
// ============================================================================
pub use compositor::{CompositorStats, SceneBuilder, SceneCompositor};
// ============================================================================
// RE-EXPORTS - Foundation Types
// ============================================================================
pub use flui_foundation::LayerId;
// Re-export annotation search types
pub use layer::annotation::{AnnotationEntry, AnnotationResult, AnnotationSearchOptions};
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
    ClipSuperellipseLayer,
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
    PerformanceStats,
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
// ============================================================================
// RE-EXPORTS - Link Registry
// ============================================================================
pub use link_registry::{LeaderInfo, LinkRegistry, resolve_follower_offset};
pub use scene::{CompositionCallback, Scene};
pub use scene_snapshot::{DamageRegion, SceneSnapshot};
// ============================================================================
// RE-EXPORTS - Tree
// ============================================================================
pub use tree::{LayerNode, LayerTree};

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
    // Re-export tree traits for convenience
    pub use flui_tree::{TreeNav, TreeRead};

    // Annotation layers
    pub use crate::{AnnotatedRegionLayer, SemanticLabel, SystemUiOverlayStyle};
    // Annotation search
    pub use crate::{AnnotationEntry, AnnotationResult, AnnotationSearchOptions};
    // Effect layers
    pub use crate::{
        BackdropFilterLayer, ColorFilterLayer, ImageFilterLayer, OpacityLayer, ShaderMaskLayer,
    };
    pub use crate::{
        CanvasLayer, PerformanceOverlayLayer, PerformanceOverlayOption, PerformanceStats,
        PlatformViewLayer, TextureLayer,
    };
    // Clip layers
    pub use crate::{ClipPathLayer, ClipRRectLayer, ClipRectLayer, ClipSuperellipseLayer};
    // Composition callbacks
    pub use crate::CompositionCallback;
    // Linking layers
    pub use crate::{FollowerLayer, LayerLink, LeaderLayer};
    // Core types
    pub use crate::{Layer, LayerBounds, LayerId, LayerNode, LayerTree};
    // Compositor
    pub use crate::{LinkRegistry, Scene, SceneBuilder, SceneCompositor, resolve_follower_offset};
    // Raster boundary
    pub use crate::{DamageRegion, SceneSnapshot};
    // Transform layers
    pub use crate::{OffsetLayer, TransformLayer};
    // Platform types
    pub use crate::{PlatformViewHitTestBehavior, PlatformViewId};
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
    use flui_types::{
        geometry::{Rect, px},
        painting::Clip,
    };

    use super::*;

    #[test]
    fn test_version() {
        // `VERSION` is wired from the package version (`env!("CARGO_PKG_VERSION")`);
        // assert its shape, not a pinned literal — a hardcoded value breaks on
        // every workspace version bump (it broke at the 0.1.0 -> 0.2.0 bump).
        let parts: Vec<&str> = VERSION.split('.').collect();
        assert_eq!(
            parts.len(),
            3,
            "VERSION should be semver `major.minor.patch`, got {VERSION:?}",
        );
        assert!(
            parts.iter().all(|part| part.parse::<u64>().is_ok()),
            "VERSION components should be numeric, got {VERSION:?}",
        );
    }

    #[test]
    fn test_layer_tree_basic() {
        let mut tree = LayerTree::new();
        assert!(tree.is_empty());

        let canvas = CanvasLayer::new();
        let id = tree.insert(Layer::from(canvas));

        assert!(!tree.is_empty());
        assert_eq!(tree.len(), 1);
        assert!(tree.contains(id));
    }

    #[test]
    fn test_all_layer_types() {
        let mut tree = LayerTree::new();

        // Leaf
        let _ = tree.insert(Layer::from(CanvasLayer::new()));

        // Clip layers
        let _ = tree.insert(Layer::ClipRect(ClipRectLayer::new(
            Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)),
            Clip::HardEdge,
        )));

        // Transform layers
        let _ = tree.insert(Layer::Offset(OffsetLayer::from_xy(10.0, 20.0)));
        let _ = tree.insert(Layer::Transform(TransformLayer::identity()));

        // Effect layers
        let _ = tree.insert(Layer::Opacity(OpacityLayer::new(0.5)));
        let _ = tree.insert(Layer::ColorFilter(ColorFilterLayer::new(
            flui_types::painting::ColorFilter::grayscale(),
        )));
        let _ = tree.insert(Layer::ImageFilter(ImageFilterLayer::blur(5.0)));

        assert_eq!(tree.len(), 7);
    }
}
