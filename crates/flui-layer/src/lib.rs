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
//! - **CanvasLayer**: Standard canvas drawing commands
//! - **ShaderMaskLayer**: GPU shader masking effects (gradient fades, vignettes)
//! - **BackdropFilterLayer**: Backdrop filtering effects (frosted glass, blur)
//! - **CachedLayer**: Cached layer for RepaintBoundary optimization
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

pub mod layer;
pub mod tree;

// ============================================================================
// RE-EXPORTS - Layer Types
// ============================================================================

pub use layer::{
    BackdropFilterLayer, CachedLayer, CanvasLayer, Layer, LayerBounds, ShaderMaskLayer,
};

// ============================================================================
// RE-EXPORTS - Tree
// ============================================================================

pub use tree::{ConcreteLayerNode, LayerNode, LayerTree};

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
    pub use crate::{
        BackdropFilterLayer, CachedLayer, CanvasLayer, ConcreteLayerNode, Layer, LayerBounds,
        LayerId, LayerNode, LayerTree, ShaderMaskLayer,
    };

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
}
