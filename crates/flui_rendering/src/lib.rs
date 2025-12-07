//! # flui_rendering
//!
//! Rendering infrastructure for Flui using the Generic Three-Tree Architecture
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
//! flui-rendering (this crate)
//!     │
//!     ├── RenderObject (type-erased trait)
//!     ├── RenderBox<A> (box protocol with arity)
//!     ├── SliverRender<A> (sliver protocol with arity)
//!     ├── LayoutTree, PaintTree, HitTestTree (concrete ops)
//!     ├── Contexts (LayoutContext, PaintContext, HitTestContext)
//!     │
//!     ▼
//! flui-pipeline (implements traits)
//! ```
//!
//! ## Key Types
//!
//! - **RenderObject**: Type-erased render trait for uniform storage
//! - **RenderBox<A>**: Box protocol render trait with compile-time arity
//! - **SliverRender<A>**: Sliver protocol render trait for scrollables
//! - **LayoutContext/PaintContext**: Operation contexts with tree access
//! - **Constraints/Geometry**: Type-erased layout types
//!
//! ## Unified Arity System
//!
//! The arity system from `flui-tree` provides compile-time child count
//! validation with advanced type features:
//!
//! ```rust,ignore
//! use flui_rendering::{RenderBox, Single, Variable, ChildrenAccess};
//!
//! // Single child wrapper
//! impl RenderBox<Single> for RenderPadding {
//!     fn layout(&mut self, ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
//!         if let Some(child) = ctx.children.single_child() {
//!             let child_size = ctx.layout_child(child, constraints.deflate(&self.padding))?;
//!             Ok(child_size + self.padding.size())
//!         } else {
//!             Ok(constraints.smallest())
//!         }
//!     }
//! }
//!
//! // Variable children container
//! impl RenderBox<Variable> for RenderFlex {
//!     fn layout(&mut self, ctx: BoxLayoutContext<'_, Variable>) -> RenderResult<Size> {
//!         let mut total_size = Size::ZERO;
//!         for child in ctx.children.element_ids() {
//!             let child_size = ctx.layout_child(child, flex_constraints)?;
//!             total_size = self.combine_sizes(total_size, child_size);
//!         }
//!         Ok(total_size)
//!     }
//! }
//! ```

// ============================================================================
// MODULE DECLARATIONS
// ============================================================================

pub mod core;
pub mod error;
pub mod into_render;
pub mod objects;
pub mod tree;  // Four-tree architecture: RenderTree for RenderObject storage
pub mod view;

// ============================================================================
// RE-EXPORTS FROM CORE MODULE
// ============================================================================

// Core rendering traits
pub use core::{RenderBox, RenderObject, RenderSliver};

// Re-export downcast-rs for downcasting RenderObject
pub use downcast_rs::DowncastSync;

// Context types for layout/paint/hit-test
pub use core::{
    BoxHitTestContext, BoxLayoutContext, BoxPaintContext, HitTestContext, LayoutContext,
    PaintContext, SliverHitTestContext, SliverLayoutContext, SliverPaintContext,
};

// Tree operation traits (dyn-compatible)
pub use core::{HitTestTree, HitTestTreeExt, LayoutTree, LayoutTreeExt, PaintTree, PaintTreeExt};

// Geometry and constraints
pub use core::BoxConstraints;

// Unified protocol types
pub use core::{Constraints, Geometry};

// Protocol system
pub use core::{BoxProtocol, Protocol, ProtocolId, SliverProtocol};

// RenderElement and lifecycle
pub use core::{RenderElement, RenderLifecycle};

// Arity system (re-exported from flui-tree)
pub use core::{Arity, AtLeast, ChildrenAccess, Exact, Leaf, Optional, Range, Single, Variable};

// Wrappers and proxies
pub use core::{BoxRenderWrapper, RenderProxyBox, RenderProxySliver, SliverRenderWrapper};

// Error handling
pub use core::{RenderError, RenderResult};

// IntoRender trait
pub use into_render::{IntoRender, IntoRenderState};

// Foundation types
pub use core::ElementId;

// Re-export commonly used render objects
pub use objects::{ParagraphData, RenderParagraph};

// External dependencies
pub use flui_interaction::{HitTestBehavior, HitTestResult, HitTestable};
pub use flui_painting::{Canvas, Paint};
pub use flui_types::{Offset, Rect, Size, SliverConstraints, SliverGeometry};

// ============================================================================
// RE-EXPORTS FROM OBJECTS MODULE
// ============================================================================

// ============================================================================
// RE-EXPORTS FROM VIEW MODULE
// ============================================================================

// TODO: Re-enable after fixing view module for typed protocols
// pub use view::{RenderObjectWrapper, RenderView, RenderViewObject, RenderViewWrapper};

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
    pub use super::core::prelude::*;

    // IntoRender trait
    pub use super::into_render::{IntoRender, IntoRenderState};

    // Most commonly used traits
    pub use super::{RenderBox, RenderObject, RenderSliver};

    // Most commonly used context types
    pub use super::{BoxHitTestContext, BoxLayoutContext, BoxPaintContext};
    pub use super::{SliverHitTestContext, SliverLayoutContext, SliverPaintContext};

    // Most commonly used arity types
    pub use super::{Arity, Leaf, Optional, Single, Variable};

    // Most commonly used geometry types
    pub use super::{BoxConstraints, Offset, Size};

    // Error handling
    pub use super::{RenderError, RenderResult};

    // Foundation
    pub use super::ElementId;
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
///
/// Parses the `CARGO_PKG_VERSION` at compile time.
/// Assumes semantic versioning format: "MAJOR.MINOR.PATCH" or "MAJOR.MINOR.PATCH-suffix"
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

    // Find first dot (end of major)
    let mut first_dot = 0;
    while first_dot < len && bytes[first_dot] != b'.' {
        first_dot += 1;
    }

    // Find second dot (end of minor)
    let mut second_dot = first_dot + 1;
    while second_dot < len && bytes[second_dot] != b'.' {
        second_dot += 1;
    }

    // Find end of patch (stop at '-' for pre-release or end of string)
    let mut patch_end = second_dot + 1;
    while patch_end < len && bytes[patch_end] != b'-' && bytes[patch_end].is_ascii_digit() {
        patch_end += 1;
    }

    let major = parse_number(bytes, 0, first_dot);
    let minor = parse_number(bytes, first_dot + 1, second_dot);
    let patch = parse_number(bytes, second_dot + 1, patch_end);

    (major, minor, patch)
}
