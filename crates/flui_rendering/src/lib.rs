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
//! ## Flat Module Structure
//!
//! This crate uses a flat file structure for clarity:
//!
//! ```text
//! src/
//! ├── lib.rs              # This file - crate entry point
//! ├── object.rs           # RenderObject trait
//! ├── box.rs              # RenderBox<A> trait
//! ├── sliver.rs           # RenderSliver<A> trait
//! ├── protocol.rs         # Protocol system (BoxProtocol, SliverProtocol)
//! ├── arity.rs            # Arity re-exports from flui-tree
//! ├── layout_context.rs   # Layout context types
//! ├── paint_context.rs    # Paint context types
//! ├── hit_test_context.rs # Hit test context types
//! ├── layout_tree.rs      # LayoutTree trait
//! ├── paint_tree.rs       # PaintTree trait
//! ├── hit_test_tree.rs    # HitTestTree trait
//! ├── render_tree.rs      # Combined FullRenderTree trait
//! ├── flags.rs            # AtomicRenderFlags
//! ├── state.rs            # RenderState<P>
//! ├── parent_data.rs      # ParentData system
//! ├── element.rs          # RenderElement
//! ├── lifecycle.rs        # RenderLifecycle
//! ├── proxy.rs            # RenderProxyBox, RenderProxySliver
//! ├── wrapper.rs          # BoxRenderWrapper, SliverRenderWrapper
//! ├── semantics.rs        # Accessibility tree
//! ├── unified.rs          # Constraints/Geometry enums
//! ├── storage.rs          # RenderTreeWrapper<T>
//! ├── error.rs            # Error types
//! ├── into_render.rs      # IntoRender trait
//! └── tree/               # RenderTree storage
//! ```

// ============================================================================
// FLAT MODULE DECLARATIONS
// ============================================================================

// Core traits
#[path = "box.rs"]
pub mod box_render;
pub mod object;
pub mod protocol;
pub mod sliver;

// Arity system
pub mod arity;

// Context types
pub mod hit_test_context;
pub mod layout_context;
pub mod paint_context;

// Tree operation traits
pub mod hit_test_tree;
pub mod layout_tree;
pub mod paint_tree;
pub mod render_tree;

// State management
pub mod flags;
pub mod parent_data;
pub mod state;

// Element types
pub mod element;
pub mod element_node;
pub mod lifecycle;

// Proxy and wrapper types
pub mod proxy;
pub mod wrapper;

// Semantics / accessibility
pub mod semantics;

// Unified protocol types
pub mod unified;

// Storage wrapper
pub mod storage;

// Error handling
pub mod error;

// IntoRender trait
pub mod into_render;

// Debug utilities and assertions
pub mod debug;

// RenderTree storage (kept as directory for now)
pub mod tree;

// Legacy core module (for backwards compatibility during transition)
pub mod core;

// View module
pub mod view;

// ============================================================================
// RE-EXPORTS FROM FLAT MODULES
// ============================================================================

// Core rendering traits
pub use box_render::RenderBox;
pub use object::{new_layer_handle, LayerHandle, LayerRef, RenderObject};
pub use sliver::RenderSliver;

// Re-export downcast-rs for downcasting RenderObject
pub use downcast_rs::DowncastSync;

// Protocol system
pub use protocol::{
    BoxProtocol, IsBoxProtocol, IsSliverProtocol, Protocol, ProtocolCast, ProtocolId,
    SliverProtocol,
};

// Arity system (re-exported from flui-tree)
pub use arity::{
    Arity, AtLeast, ChildrenAccess, Exact, FixedChildren, Leaf, NoChildren, Optional,
    OptionalChild, Range, RuntimeArity, Single, SliceChildren, Variable,
};

// Context types for layout/paint/hit-test
pub use hit_test_context::{BoxHitTestContext, HitTestContext, SliverHitTestContext};
pub use layout_context::{BoxLayoutContext, LayoutContext, SliverLayoutContext};
pub use paint_context::{BoxPaintContext, PaintContext, SliverPaintContext};

// Short context aliases
pub type BoxLayoutCtx<'a, A, T = Box<dyn layout_tree::LayoutTree + Send + Sync>> =
    BoxLayoutContext<'a, A, T>;
pub type BoxPaintCtx<'a, A, T = Box<dyn paint_tree::PaintTree + Send + Sync>> =
    BoxPaintContext<'a, A, T>;
pub type BoxHitTestCtx<'a, A, T = Box<dyn hit_test_tree::HitTestTree + Send + Sync>> =
    BoxHitTestContext<'a, A, T>;
pub type SliverLayoutCtx<'a, A, T = Box<dyn layout_tree::LayoutTree + Send + Sync>> =
    SliverLayoutContext<'a, A, T>;
pub type SliverPaintCtx<'a, A, T = Box<dyn paint_tree::PaintTree + Send + Sync>> =
    SliverPaintContext<'a, A, T>;

// Tree operation traits (dyn-compatible)
pub use hit_test_tree::{HitTestTree, HitTestTreeExt};
pub use layout_tree::{LayoutTree, LayoutTreeExt};
pub use paint_tree::{PaintTree, PaintTreeExt};
pub use render_tree::{
    debug_element_info, format_element_debug, format_tree_node, FullRenderTree,
    RenderElementDebugInfo, RenderTreeOps,
};

// Flags and state
pub use flags::{AtomicRenderFlags, RenderFlags};
pub use state::{BoxRenderState, RenderState, RenderStateExt, SliverRenderState};

// Parent data
pub use parent_data::{
    BoxParentData, ContainerBoxParentData, ContainerParentData, ParentData, ParentDataWithOffset,
};

// RenderElement and lifecycle
pub use element::{BoxRenderElement, RenderElement, SliverRenderElement};
pub use element_node::{ElementNodeStorage, RenderElementNode};
pub use lifecycle::RenderLifecycle;

// Proxy traits
pub use proxy::{RenderProxyBox, RenderProxySliver};

// Wrapper types
pub use wrapper::{BoxRenderWrapper, SliverRenderWrapper};

// Semantics / accessibility
pub use semantics::{SemanticsHandle, SemanticsNode, SemanticsNodeId, SemanticsOwner};

// Unified protocol types
pub use unified::{Constraints, Geometry};

// Storage wrapper
pub use storage::{RenderTreeStorage, RenderTreeWrapper};

// Geometry and constraints
pub use flui_types::BoxConstraints;

// Tree types (RenderTree, RenderNode, RenderId)
pub use tree::{ConcreteRenderNode, RenderId, RenderNode, RenderTree};

// Error handling
pub use error::{RenderError, Result as RenderResult};

// IntoRender trait
pub use into_render::{IntoRender, IntoRenderState};

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
/// This brings the most commonly used rendering types and traits into scope.
///
/// ```rust,ignore
/// use flui_rendering::prelude::*;
///
/// // Now you can use common types directly
/// impl RenderBox<Single> for MyRenderObject {
///     fn layout(&mut self, ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
///         // Implementation here
///         Ok(Size::new(100.0, 100.0))
///     }
/// }
/// ```
pub mod prelude {
    // Most commonly used traits
    pub use super::{RenderBox, RenderObject, RenderSliver};

    // Most commonly used context types
    pub use super::{BoxHitTestContext, BoxLayoutContext, BoxPaintContext};
    pub use super::{SliverHitTestContext, SliverLayoutContext, SliverPaintContext};

    // Most commonly used arity types
    pub use super::{Arity, Leaf, Optional, Single, Variable};

    // Protocols
    pub use super::{BoxProtocol, Protocol, SliverProtocol};

    // Most commonly used geometry types
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
