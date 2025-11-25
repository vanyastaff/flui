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

// Core modules - no element dependencies
pub mod arity;
pub mod geometry;
pub mod parent_data;
pub mod protocol;
pub mod render_flags;
pub mod render_state;

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
    BoxParentData, ContainerBoxParentData, ContainerParentData, ParentData, ParentDataWithOffset,
};

// Re-export flags and state
pub use render_flags::{AtomicRenderFlags, RenderFlags};
pub use render_state::RenderState;

// Re-export hit testing from flui_interaction
pub use flui_interaction::{HitTestBehavior, HitTestEntry, HitTestResult, HitTestable};

// Re-export ElementId from flui-foundation
pub use flui_foundation::ElementId;
