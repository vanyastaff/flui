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
//! flui_rendering (this crate)
//!     │
//!     ├── RenderObject (type-erased trait)
//!     ├── RenderBox<A> (box protocol with arity)
//!     ├── RenderSliver<A> (sliver protocol with arity)
//!     ├── LayoutTree, PaintTree, HitTestTree (concrete ops)
//!     ├── Contexts (LayoutContext, PaintContext, HitTestContext)
//!     ├── AtomicRenderFlags (lock-free dirty tracking)
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
//! Flat crate structure - all modules at root level.

// ============================================================================
// MODULE DECLARATIONS
// ============================================================================

// Core rendering types
mod box_render;
mod context;
mod element;
mod flags;
mod lifecycle;
mod object;
mod parent_data;
mod pipeline_owner;
mod protocol;
mod proxy;
mod render_tree;

mod sliver;
mod state;
mod tree;

mod wrapper;

// Other modules
pub mod error;

// ============================================================================
// RE-EXPORTS
// ============================================================================

// Core rendering traits
pub use box_render::RenderBox;
pub use object::{new_layer_handle, LayerHandle, LayerRef, RenderObject};
pub use sliver::RenderSliver;

// Re-export downcast-rs for downcasting RenderObject
pub use downcast_rs::DowncastSync;

// Protocol system
pub use protocol::{BoxProtocol, Protocol, ProtocolId, SliverProtocol};

// Arity system (from flui-tree)
pub use flui_tree::arity::{
    Arity, AtLeast, ChildrenAccess, Exact, FixedChildren, Leaf, NoChildren, Optional,
    OptionalChild, Range, RuntimeArity, Single, SliceChildren, Variable,
};

// Context types for layout/paint/hit-test
pub use context::{BoxHitTestContext, HitTestContext, SliverHitTestContext};
pub use context::{BoxLayoutContext, LayoutContext, SliverLayoutContext};
pub use context::{BoxPaintContext, PaintContext, SliverPaintContext};

// Short context aliases
pub type BoxLayoutCtx<'a, A, T = Box<dyn LayoutTree + Send + Sync>> =
    context::BoxLayoutContext<'a, A, T>;
pub type BoxPaintCtx<'a, A, T = Box<dyn PaintTree + Send + Sync>> =
    context::BoxPaintContext<'a, A, T>;
pub type BoxHitTestCtx<'a, A, T = Box<dyn HitTestTree + Send + Sync>> =
    context::BoxHitTestContext<'a, A, T>;
pub type SliverLayoutCtx<'a, A, T = Box<dyn LayoutTree + Send + Sync>> =
    context::SliverLayoutContext<'a, A, T>;
pub type SliverPaintCtx<'a, A, T = Box<dyn PaintTree + Send + Sync>> =
    context::SliverPaintContext<'a, A, T>;

// Tree operation traits (dyn-compatible)
pub use tree::{
    debug_element_info, format_element_debug, format_tree_node, FullRenderTree,
    RenderElementDebugInfo, RenderTreeOps,
};
pub use tree::{HitTestTree, HitTestTreeExt};
pub use tree::{LayoutTree, LayoutTreeExt};
pub use tree::{PaintTree, PaintTreeExt};

// Flags and state
pub use flags::{AtomicRenderFlags, RenderFlags};
pub use state::{BoxRenderState, RenderState, RenderStateExt, SliverRenderState};

// Parent data
pub use parent_data::{
    BoxParentData, ContainerBoxParentData, ContainerParentData, ParentData, ParentDataWithOffset,
};

// RenderElement and lifecycle
pub use element::RenderElement;
pub use lifecycle::RenderLifecycle;

// Proxy traits
pub use proxy::{RenderProxyBox, RenderProxySliver};

// Wrapper types
pub use wrapper::{BoxRenderWrapper, SliverRenderWrapper};

// RenderTree and RenderNode
pub use render_tree::{RenderNode, RenderTree};

// RenderPipelineOwner (Flutter's PipelineOwner equivalent)
pub use pipeline_owner::RenderPipelineOwner;

// ============================================================================
// RE-EXPORTS FROM FOUNDATION
// ============================================================================

// RenderId from foundation
pub use flui_foundation::RenderId;

// ============================================================================
// RE-EXPORTS FROM OTHER MODULES
// ============================================================================

// Error handling
pub use error::{RenderError, Result as RenderResult};

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
