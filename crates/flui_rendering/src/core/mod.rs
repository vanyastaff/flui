// ============================================================================
// CORE MODULES
// ============================================================================

// Arity system re-exports and rendering extensions
pub mod arity;

// Rendering contexts with GAT integration
mod context;

// Protocol definitions (Box, Sliver)
pub mod protocol;

// Parent data system for per-child layout metadata
mod parent_data;

// Base render object trait
mod object;

// Atomic render flags for lock-free state management
mod flags;

// Per-render state storage with atomic flags
mod state;

// Box protocol render trait
mod box_render;

// Sliver protocol render trait
mod sliver;

// Render tree traits (LayoutTree, PaintTree, HitTestTree, FullRenderTree)
mod tree;

// RenderTree<T> - Wrapper that adds rendering capabilities to any tree storage
mod tree_storage;

// Unified protocol types (Constraints, Geometry enums for multi-protocol support)
mod unified;

// Proxy traits for pass-through render objects
mod proxy;

// Utility wrappers for render objects
mod wrapper;

// Render lifecycle states
mod lifecycle;

// Render element - core element type for rendering
mod element;

// Semantics tree for accessibility
mod semantics;

// ============================================================================
// ARITY SYSTEM RE-EXPORTS
// ============================================================================

pub use arity::*;

// ============================================================================
// PROTOCOL SYSTEM
// ============================================================================

pub use protocol::*;

// ============================================================================
// GEOMETRY AND CONSTRAINTS
// ============================================================================

// Re-export flui_types for convenience
pub use flui_types::{
    Axis, BoxConstraints, EdgeInsets, Offset, Rect, Size, SliverConstraints, SliverGeometry,
};

// Unified protocol types
pub use unified::{Constraints, Geometry};

// ============================================================================
// CORE RENDER TRAITS
// ============================================================================

pub use box_render::RenderBox;
pub use object::{new_layer_handle, LayerHandle, LayerRef, RenderObject};
pub use sliver::RenderSliver;

// ============================================================================
// PARENT DATA SYSTEM
// ============================================================================

pub use parent_data::{
    BoxParentData, ContainerBoxParentData, ContainerParentData, ParentData, ParentDataWithOffset,
};

// ============================================================================
// RENDER FLAGS AND STATE
// ============================================================================

pub use flags::{AtomicRenderFlags, RenderFlags};
pub use state::{BoxRenderState, RenderState, RenderStateExt, SliverRenderState};

// ============================================================================
// CONTEXTS (GAT-based)
// ============================================================================

pub use context::{
    BoxHitTestContext,
    BoxLayoutContext,
    BoxPaintContext,
    // Hit test contexts
    HitTestContext,
    // Layout contexts
    LayoutContext,
    // Paint contexts
    PaintContext,
    SliverHitTestContext,
    SliverLayoutContext,
    SliverPaintContext,
};

// Short aliases for convenience (used by render objects)
pub type BoxLayoutCtx<'a, A, T = Box<dyn LayoutTree + Send + Sync>> = BoxLayoutContext<'a, A, T>;
pub type BoxPaintCtx<'a, A, T = Box<dyn PaintTree + Send + Sync>> = BoxPaintContext<'a, A, T>;
pub type BoxHitTestCtx<'a, A, T = Box<dyn HitTestTree + Send + Sync>> = BoxHitTestContext<'a, A, T>;
pub type SliverLayoutCtx<'a, A, T = Box<dyn LayoutTree + Send + Sync>> =
    SliverLayoutContext<'a, A, T>;
pub type SliverPaintCtx<'a, A, T = Box<dyn PaintTree + Send + Sync>> = SliverPaintContext<'a, A, T>;

// ============================================================================
// TREE OPERATIONS (dyn-compatible)
// ============================================================================

pub use tree::{
    // Debug utilities
    debug_element_info,
    format_element_debug,
    format_tree_node,
    // Utility functions
    hit_test_subtree,
    layout_batch,
    layout_subtree,
    paint_batch,
    paint_subtree,
    // Combined traits
    FullRenderTree,
    // Phase-specific traits
    HitTestTree,
    // Extension traits
    HitTestTreeExt,
    LayoutTree,
    LayoutTreeExt,
    PaintTree,
    PaintTreeExt,
    RenderElementDebugInfo,
    RenderTreeOps,
};

// ============================================================================
// RENDER TREE WRAPPER
// ============================================================================

pub use tree_storage::{RenderTree, RenderTreeStorage};

// ============================================================================
// PROXY TRAITS
// ============================================================================

pub use proxy::{RenderProxyBox, RenderProxySliver};

// ============================================================================
// WRAPPERS AND UTILITIES
// ============================================================================

pub use wrapper::{BoxRenderWrapper, SliverRenderWrapper};

// ============================================================================
// RENDER ELEMENT
// ============================================================================

pub use element::RenderElement;
pub use lifecycle::RenderLifecycle;

// ============================================================================
// SEMANTICS / ACCESSIBILITY
// ============================================================================

pub use semantics::{SemanticsHandle, SemanticsNode, SemanticsNodeId, SemanticsOwner};

// ============================================================================
// FLUI-TREE INTEGRATION
// ============================================================================

// Re-export generic tree types from flui-tree
pub use flui_tree::{TreeNav, TreeRead, TreeWrite};

// Re-export render-specific types from our tree module
pub use crate::tree::{
    AtomicDirtyFlags, DirtyTracking, DirtyTrackingExt, RenderAncestors, RenderChildren,
    RenderChildrenCollector, RenderDescendants, RenderTreeAccess, RenderTreeAccessExt,
    RenderTreeExt,
};

// ============================================================================
// FOUNDATION RE-EXPORTS
// ============================================================================

pub use flui_foundation::ElementId;
pub use flui_interaction::{HitTestBehavior, HitTestResult, HitTestable};
pub use flui_painting::{Canvas, Paint};

// ============================================================================
// ERROR HANDLING
// ============================================================================

// Re-export error types and result alias
pub use crate::error::{RenderError, Result as RenderResult};

// ============================================================================
// PRELUDE FOR COMMON USAGE
// ============================================================================

/// The rendering prelude - commonly used types and traits.
///
/// ```rust
/// use flui_rendering::core::prelude::*;
/// ```
pub mod prelude {
    // Core traits
    pub use super::{RenderBox, RenderObject, RenderSliver};

    // Arity system
    pub use super::{Arity, AtLeast, Exact, Leaf, Optional, Single, Variable};

    // Protocols
    pub use super::{BoxProtocol, Protocol, SliverProtocol};

    // Contexts
    pub use super::{BoxHitTestContext, BoxLayoutContext, BoxPaintContext};
    pub use super::{HitTestContext, LayoutContext, PaintContext};

    // Geometry
    pub use super::{BoxConstraints, Offset, Rect, Size};

    // Tree operations
    pub use super::{HitTestTree, LayoutTree, PaintTree, RenderTreeOps};

    // RenderTree wrapper
    pub use super::{RenderTree, RenderTreeStorage};

    // Foundation types
    pub use super::{Canvas, ElementId, HitTestResult, Paint};

    // Error handling
    pub use super::{RenderError, RenderResult};
}
