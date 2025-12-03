// ============================================================================
// CORE MODULES
// ============================================================================

// Arity system re-exports and rendering extensions
pub mod arity;

// Rendering contexts with GAT integration
mod contexts;

// Protocol definitions (Box, Sliver)
pub mod protocol;

// Parent data system for per-child layout metadata
mod parent_data;

// Base render object trait
mod render_object;

// Atomic render flags for lock-free state management
mod render_flags;

// Per-render state storage with atomic flags
mod render_state;

// Box protocol render trait
mod render_box;

// Sliver protocol render trait
mod render_sliver;

// Full render tree traits (combines LayoutTree + PaintTree + HitTestTree)
// Note: tree_ops.rs was removed - all tree traits are now in render_tree.rs
mod render_tree;

// Proxy traits for pass-through render objects
mod render_proxy;

// Utility wrappers and proxies
mod wrappers;

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

// ============================================================================
// CORE RENDER TRAITS
// ============================================================================

pub use render_box::RenderBox;
pub use render_object::RenderObject;
pub use render_sliver::RenderSliver;

// ============================================================================
// PARENT DATA SYSTEM
// ============================================================================

pub use parent_data::{
    BoxParentData, ContainerBoxParentData, ContainerParentData, ParentData, ParentDataWithOffset,
};

// ============================================================================
// RENDER FLAGS AND STATE
// ============================================================================

pub use render_flags::{AtomicRenderFlags, RenderFlags};
pub use render_state::{BoxRenderState, RenderState, SliverRenderState};

// ============================================================================
// CONTEXTS (GAT-based)
// ============================================================================

pub use contexts::{
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

pub use render_tree::{
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
    RenderTreeOps,
};

// ============================================================================
// PROXY TRAITS
// ============================================================================

pub use render_proxy::{RenderProxyBox, RenderProxySliver};

// ============================================================================
// WRAPPERS AND UTILITIES
// ============================================================================

pub use wrappers::{BoxRenderWrapper, SliverRenderWrapper};

// ============================================================================
// FLUI-TREE INTEGRATION
// ============================================================================

// Re-export commonly used flui-tree types for convenience
pub use flui_tree::{
    // Utility functions
    collect_render_children,
    count_render_children,
    find_render_ancestor,
    render_depth,
    AtomicDirtyFlags,
    // Dirty tracking
    DirtyTracking,
    DirtyTrackingExt,
    RenderAncestors,
    // Iterators
    RenderChildren,
    RenderDescendants,
    // Render tree access
    RenderTreeAccess,
    RenderTreeAccessExt,
    // Tree navigation
    TreeNav,
    TreeRead,
    TreeWrite,
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

    // Foundation types
    pub use super::{Canvas, ElementId, HitTestResult, Paint};

    // Error handling
    pub use super::{RenderError, RenderResult};
}
