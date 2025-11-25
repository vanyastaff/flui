//! # flui_rendering
//!
//! Rendering infrastructure for Flui using the Generic Three-Tree Architecture.
//!
//! This crate provides the RenderObject layer that handles layout and painting.
//! It is independent of the concrete Element implementation, using trait
//! abstractions from `flui-tree`.
//!
//! ## Architecture
//!
//! ```text
//! flui-tree (traits)
//!     │
//!     ├── TreeRead, TreeNav, TreeWrite
//!     ├── RenderTreeAccess, DirtyTracking
//!     │
//!     ▼
//! flui-rendering (this crate)
//!     │
//!     ├── RenderObject (type-erased trait)
//!     ├── RenderBox<A> (box protocol)
//!     ├── SliverRender<A> (sliver protocol)
//!     ├── LayoutTree, PaintTree, HitTestTree (concrete ops)
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
//! - **LayoutContext/PaintContext**: Operation contexts for render methods
//! - **Constraints/Geometry**: Type-erased layout types
//!
//! ## Design Principles
//!
//! 1. **No concrete tree dependency**: Uses traits from `flui-tree`
//! 2. **Callback-based operations**: Layout/paint via closures, not tree refs
//! 3. **Zero-cost abstractions**: Generic types compile to concrete code
//! 4. **Separation of concerns**: Rendering logic separate from tree management

#![warn(missing_docs)]

pub mod core;
pub mod error;
// TODO: Migrate objects/ to use new trait-based architecture
// pub mod objects;

// Re-export from core module
pub use core::{
    // Arity types
    Arity,
    AtLeast,
    // Tree traits (re-exported from flui-tree)
    AtomicDirtyFlags,
    // Flags and state
    AtomicRenderFlags,
    // Protocol types
    BoxConstraints,
    // Parent data
    BoxParentData,
    BoxProtocol,
    // Wrappers
    BoxRenderWrapper,
    ChildrenAccess,
    // Geometry types
    Constraints,
    ContainerBoxParentData,
    ContainerParentData,
    DirtyTracking,
    DirtyTrackingExt,
    // From flui_foundation
    ElementId,
    // Render traits
    EmptyRender,
    Exact,
    FixedChildren,
    FullRenderTree,
    Geometry,
    // Contexts
    HasTypedChildren,
    // From flui_interaction
    HitTestBehavior,
    HitTestContext,
    HitTestEntry,
    HitTestResult,
    HitTestTree,
    HitTestable,
    LayoutContext,
    LayoutProtocol,
    LayoutTree,
    Leaf,
    NoChildren,
    Optional,
    OptionalChild,
    PaintContext,
    PaintTree,
    ParentData,
    ParentDataWithOffset,
    Protocol,
    RenderBox,
    RenderBoxExt,
    RenderFlags,
    RenderObject,
    RenderState,
    RenderTreeAccess,
    RenderTreeAccessExt,
    RuntimeArity,
    Single,
    SliceChildren,
    SliverProtocol,
    SliverRender,
    SliverRenderExt,
    SliverRenderWrapper,
    TreeNav,
    TreeRead,
    TreeWrite,
    Variable,
};

// Re-export from flui_types for convenience
pub use flui_types::layout::{FlexFit, StackFit};
pub use flui_types::{SliverConstraints, SliverGeometry};

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::core::{
        // Core traits
        Arity,
        BoxConstraints,
        BoxProtocol,
        Constraints,
        ElementId,
        EmptyRender,
        Geometry,
        LayoutProtocol,
        LayoutTree,
        Leaf,
        Optional,
        PaintTree,
        Protocol,
        RenderBox,
        RenderBoxExt,
        RenderFlags,
        RenderObject,
        RenderState,
        RuntimeArity,
        Single,
        SliverProtocol,
        SliverRender,
        Variable,
    };
}
