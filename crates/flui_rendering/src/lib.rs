//! # flui_rendering
//!
//! Rendering infrastructure for Flui using the Generic Four-Tree Architecture
//! with unified `flui-tree` integration.
//!
//! This crate provides the RenderObject layer that handles layout and painting.
//! It leverages trait abstractions from `flui-tree` for tree operations and
//! uses the unified arity system for compile-time child count validation.
//!
//! ## Architecture
//!
//! ```text
//! flui-tree (unified abstractions)
//!     │
//!     ├── Arity system (GAT, HRTB, const generics)
//!     ├── TreeRead, TreeNav, TreeWrite
//!     ├── RenderTreeAccess, DirtyTracking
//!     ├── Iterators (RenderChildren, RenderDescendants, etc.)
//!     │
//!     ▼
//! flui_rendering (this crate)
//!     │
//!     ├── RenderObject (type-erased trait)
//!     ├── RenderBox<A> (box protocol with arity)
//!     ├── RenderSliver<A> (sliver protocol with arity)
//!     ├── LayoutTree, PaintTree, HitTestTree (concrete ops)
//!     ├── Contexts (LayoutContext, PaintContext, HitTestContext)
//!     │
//!     ▼
//! flui-objects (concrete implementations)
//!     │
//!     ├── RenderPadding, RenderFlex, RenderStack, etc.
//!     └── All built-in render objects
//! ```
//!
//! ## Key Types
//!
//! - **RenderObject**: Type-erased render trait for uniform storage
//! - **RenderBox<A>**: Box protocol render trait with compile-time arity
//! - **RenderSliver<A>**: Sliver protocol render trait for scrollables
//! - **LayoutContext/PaintContext**: Operation contexts with tree access
//! - **Constraints/Geometry**: Type-erased layout types
//!
//! ## Module Structure
//!
//! ```text
//! src/
//! ├── lib.rs       # Crate entry point with re-exports
//! ├── core/        # Core rendering types and traits
//! ├── tree/        # RenderTree storage
//! ├── view/        # View-related rendering
//! ├── error.rs     # Error types
//! ├── debug.rs     # Debug utilities
//! └── into_render.rs # IntoRender trait
//! ```

// ============================================================================
// MODULE DECLARATIONS
// ============================================================================

/// Core rendering types and traits
pub mod core;

/// RenderTree storage and tree operations
pub mod tree;

/// View-related rendering
pub mod view;

/// Error handling
pub mod error;

/// Debug utilities and assertions
pub mod debug;

/// IntoRender trait
pub mod into_render;

// ============================================================================
// RE-EXPORTS FROM CORE MODULE
// ============================================================================

// Core rendering traits
pub use core::RenderBox;
pub use core::RenderSliver;
pub use core::{new_layer_handle, LayerHandle, LayerRef, RenderObject};

// Re-export downcast-rs for downcasting RenderObject
pub use downcast_rs::DowncastSync;

// Protocol system
pub use core::{BoxProtocol, Protocol, ProtocolId, SliverProtocol};

// Arity system
pub use core::{
    Arity, AtLeast, ChildrenAccess, Exact, FixedChildren, Leaf, NoChildren, Optional,
    OptionalChild, Range, RuntimeArity, Single, SliceChildren, Variable,
};

// Context types for layout/paint/hit-test
pub use core::{BoxHitTestContext, HitTestContext, SliverHitTestContext};
pub use core::{BoxLayoutContext, LayoutContext, SliverLayoutContext};
pub use core::{BoxPaintContext, PaintContext, SliverPaintContext};

// Short context aliases
pub type BoxLayoutCtx<'a, A, T = Box<dyn core::LayoutTree + Send + Sync>> =
    core::BoxLayoutContext<'a, A, T>;
pub type BoxPaintCtx<'a, A, T = Box<dyn core::PaintTree + Send + Sync>> =
    core::BoxPaintContext<'a, A, T>;
pub type BoxHitTestCtx<'a, A, T = Box<dyn core::HitTestTree + Send + Sync>> =
    core::BoxHitTestContext<'a, A, T>;
pub type SliverLayoutCtx<'a, A, T = Box<dyn core::LayoutTree + Send + Sync>> =
    core::SliverLayoutContext<'a, A, T>;
pub type SliverPaintCtx<'a, A, T = Box<dyn core::PaintTree + Send + Sync>> =
    core::SliverPaintContext<'a, A, T>;

// Tree operation traits (dyn-compatible)
pub use core::{
    debug_element_info, format_element_debug, format_tree_node, FullRenderTree,
    RenderElementDebugInfo, RenderTreeOps,
};
pub use core::{HitTestTree, HitTestTreeExt};
pub use core::{LayoutTree, LayoutTreeExt};
pub use core::{PaintTree, PaintTreeExt};

// Flags and state
pub use core::{AtomicRenderFlags, RenderFlags};
pub use core::{BoxRenderState, RenderState, RenderStateExt, SliverRenderState};

// Parent data
pub use core::{
    BoxParentData, ContainerBoxParentData, ContainerParentData, ParentData, ParentDataWithOffset,
};

// RenderElement and lifecycle
pub use core::RenderElement;
pub use core::RenderLifecycle;

// Proxy traits
pub use core::{RenderProxyBox, RenderProxySliver};

// Wrapper types
pub use core::{BoxRenderWrapper, SliverRenderWrapper};

// Semantics / accessibility
pub use core::{SemanticsHandle, SemanticsNode, SemanticsNodeId, SemanticsOwner};

// Unified protocol types
pub use core::{Constraints, Geometry};

// RenderTreeStorage from core
pub use core::RenderTreeStorage;

// ============================================================================
// RE-EXPORTS FROM TREE MODULE
// ============================================================================

// Tree types (RenderTree, RenderNode, RenderId)
pub use tree::{RenderId, RenderNode, RenderTree};

// Tree access and iteration
pub use tree::{
    AtomicDirtyFlags, DirtyTracking, DirtyTrackingExt, RenderAncestors, RenderChildren,
    RenderChildrenCollector, RenderDescendants, RenderTreeAccess, RenderTreeAccessExt,
    RenderTreeExt,
};

// ============================================================================
// RE-EXPORTS FROM OTHER MODULES
// ============================================================================

// Error handling
pub use error::{RenderError, Result as RenderResult};

// IntoRender trait
pub use into_render::{IntoRender, IntoRenderState};

// Geometry and constraints from flui_types
pub use flui_types::BoxConstraints;

// Foundation types
pub use flui_foundation::ElementId;

// External dependencies
pub use flui_interaction::{HitTestBehavior, HitTestResult, HitTestable};
pub use flui_painting::{Canvas, Paint};
pub use flui_types::prelude::TextBaseline;
pub use flui_types::{Offset, Rect, Size, SliverConstraints, SliverGeometry};

// ============================================================================
// PRELUDE MODULE
// ============================================================================

/// The rendering prelude - commonly used types and traits.
///
/// ```rust,ignore
/// use flui_rendering::prelude::*;
/// ```
pub mod prelude {
    // Core traits
    pub use super::{RenderBox, RenderObject, RenderSliver};

    // Context types
    pub use super::{BoxHitTestContext, BoxLayoutContext, BoxPaintContext};
    pub use super::{SliverHitTestContext, SliverLayoutContext, SliverPaintContext};

    // Arity types
    pub use super::{Arity, Leaf, Optional, Single, Variable};

    // Protocols
    pub use super::{BoxProtocol, Protocol, SliverProtocol};

    // Geometry types
    pub use super::{BoxConstraints, Offset, Rect, Size};

    // Tree operations
    pub use super::{HitTestTree, LayoutTree, PaintTree, RenderTreeOps};

    // Foundation
    pub use super::{Canvas, ElementId, HitTestResult, Paint};

    // Error handling
    pub use super::{RenderError, RenderResult};

    // IntoRender trait
    pub use super::{IntoRender, IntoRenderState};
}

// ============================================================================
// FEATURE FLAGS
// ============================================================================

#[cfg(feature = "serde")]
pub use flui_types::serde;

// ============================================================================
// DOCUMENTATION TESTS
// ============================================================================

#[doc = include_str!("../README.md")]
#[cfg(doctest)]
pub struct ReadmeDoctests;

// ============================================================================
// VERSION INFORMATION
// ============================================================================

/// Version information for flui_rendering
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Get version as a tuple (major, minor, patch)
pub const fn version_tuple() -> (u32, u32, u32) {
    const fn parse_digit(b: u8) -> u32 {
        (b - b'0') as u32
    }

    const fn parse_number(bytes: &[u8], start: usize, end: usize) -> u32 {
        let mut result = 0u32;
        let mut i = start;
        while i < end {
            result = result * 10 + parse_digit(bytes[i]);
            i += 1;
        }
        result
    }

    let bytes = VERSION.as_bytes();
    let len = bytes.len();

    let mut first_dot = 0;
    while first_dot < len && bytes[first_dot] != b'.' {
        first_dot += 1;
    }

    let mut second_dot = first_dot + 1;
    while second_dot < len && bytes[second_dot] != b'.' {
        second_dot += 1;
    }

    let mut patch_end = second_dot + 1;
    while patch_end < len && bytes[patch_end] != b'-' && bytes[patch_end].is_ascii_digit() {
        patch_end += 1;
    }

    let major = parse_number(bytes, 0, first_dot);
    let minor = parse_number(bytes, first_dot + 1, second_dot);
    let patch = parse_number(bytes, second_dot + 1, patch_end);

    (major, minor, patch)
}
