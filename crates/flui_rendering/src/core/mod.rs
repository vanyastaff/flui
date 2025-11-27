//! Core rendering types and traits for FLUI
//!
//! This module contains foundational rendering types. Hit testing is provided
//! by `flui_interaction` crate.
//!
//! # Architecture
//!
//! **flui_rendering::core** (this module):
//! - `Arity`: Compile-time child count validation
//! - `Protocol`: Layout protocol abstraction (Box, Sliver)
//! - `Geometry`, `Constraints`: Type-erased layout types
//! - `RenderFlags`: Dirty tracking flags
//! - `RenderState`: Render object lifecycle state
//! - `ParentData`: Metadata system for parent-child communication
//! - `RenderObject`: Type-erased render trait
//! - `RenderBox`: Box protocol render trait
//! - `SliverRender`: Sliver protocol render trait
//! - `LayoutContext`, `PaintContext`, `HitTestContext`: Operation contexts
//!
//! **flui_interaction** (separate crate):
//! - `HitTestResult`, `HitTestEntry`: Hit testing types
//! - `HitTestBehavior`: Hit test behavior control
//! - `HitTestable`: Trait for hit-testable objects
//!
//! # Pattern
//!
//! ```text
//! View (trait) → Element (enum) → RenderObject trait
//!                                      ↓
//!                                  LayoutContext / PaintContext
//! ```

// Core modules
pub mod arity;
pub mod contexts;
pub mod geometry;
pub mod parent_data;
pub mod protocol;
pub mod render_box;
pub mod render_flags;
pub mod render_object;
pub mod render_proxy;
pub mod render_sliver;
pub mod render_state;
pub mod render_tree;
pub mod wrappers;

// Re-export arity types
pub use arity::{
    Arity, AtLeast, ChildrenAccess, Exact, FixedChildren, Leaf, NoChildren, Optional,
    OptionalChild, RuntimeArity, Single, SliceChildren, Variable,
};

// Re-export protocol types
pub use protocol::{BoxConstraints, BoxProtocol, LayoutProtocol, Protocol, SliverProtocol};

// Re-export geometry types
pub use geometry::{Constraints, Geometry};

// Re-export from flui_types
pub use flui_types::{SliverConstraints, SliverGeometry};

// Re-export parent data types
pub use parent_data::{
    BoxParentData, ContainerBoxParentData, ContainerParentData, FlexParentData, ParentData,
    ParentDataWithOffset, StackParentData,
};

// Re-export flags and state
pub use render_flags::{AtomicRenderFlags, RenderFlags};
pub use render_state::{Baselines, RenderState};

// Re-export hit testing from flui_interaction
pub use flui_interaction::{HitTestBehavior, HitTestEntry, HitTestResult, HitTestable};

// Re-export ElementId from flui-foundation
pub use flui_foundation::ElementId;

// Re-export render tree traits (concrete types from this crate)
pub use render_tree::{FullRenderTree, HitTestTree, LayoutTree, PaintTree};

// Re-export base traits from flui-tree (type-erased)
pub use render_tree::{
    AtomicDirtyFlags, DirtyTracking, DirtyTrackingExt, RenderTreeAccess, RenderTreeAccessExt,
};

// Re-export tree navigation traits from flui-tree
pub use flui_tree::{TreeNav, TreeRead, TreeWrite};

// Re-export contexts
pub use contexts::{HasTypedChildren, HitTestContext, LayoutContext, PaintContext};

// Re-export render object traits
pub use render_box::{
    EmptyRender, RenderBox, RenderBoxExt, RenderBoxLeaf, RenderBoxWithChild, RenderBoxWithChildren,
    RenderBoxWithOptionalChild,
};
pub use render_object::RenderObject;
pub use render_sliver::{SliverRender, SliverRenderExt};

// Re-export proxy traits
pub use render_proxy::{RenderBoxProxy, RenderSliverProxy};

// Note: BoxRenderWrapper and SliverRenderWrapper are deprecated.
// RenderObjectWrapper now directly wraps RenderBox<A> without type erasure.
// pub use wrappers::{BoxRenderWrapper, SliverRenderWrapper};
