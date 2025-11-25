//! Render system - Unified architecture
//!
//! # Architecture
//!
//! - `Render` trait: Box protocol render objects
//! - `SliverRender` trait: Sliver protocol render objects
//! - `RenderObject` trait: Type-erased render object interface
//! - `Arity`: Compile-time child count validation
//! - `ParentData`: Metadata system (stored in RenderElement)
//!
//! # Pattern
//!
//! ```text
//! View (trait) → Element (enum) → RenderElement → RenderObject trait
//!                                      ↓
//!                                  LayoutContext / PaintContext
//! ```
//!
//! # Implementation Guide
//!
//! To create a box renderer, implement the `Render` trait:
//!
//! ```rust,ignore
//! impl RenderBox<Leaf> for MyRenderer {
//!     fn layout(&mut self, ctx: LayoutContext<'_, Leaf, BoxProtocol>) -> Size { /* ... */ }
//!     fn paint(&self, ctx: &mut PaintContext<'_, Leaf>) { /* ... */ }
//! }
//! ```
//!
//! To create a sliver renderer, implement the `SliverRender` trait:
//!
//! ```rust,ignore
//! impl SliverRender<Variable> for MySliverRenderer {
//!     fn layout(&mut self, ctx: LayoutContext<'_, Variable, SliverProtocol>) -> SliverGeometry { /* ... */ }
//!     fn paint(&self, ctx: &mut PaintContext<'_, Variable>) { /* ... */ }
//! }
//! ```

// Core modules
pub mod arity;
pub mod contexts;
pub mod parent_data;
pub mod protocol;
pub mod render_box;
pub mod render_flags;
pub mod render_object;
pub mod render_proxy;
pub mod render_silver;
pub mod render_state;
pub mod wrappers;

// Re-export main traits
pub use render_box::{
    EmptyRender, RenderBox, RenderBoxExt, WithChild, WithChildren, WithLeaf, WithMaybeChild,
    WithOptionalChild,
};
pub use render_silver::{
    SliverExt, SliverRender, SliverWithChild, SliverWithChildren, SliverWithLeaf,
    SliverWithOptionalChild,
};

// Re-export contexts
pub use contexts::{HasTypedChildren, HitTestContext, LayoutContext, PaintContext};

/// Type-safe arity system for compile-time child count validation
pub use arity::{
    Arity, AtLeast, ChildrenAccess, Exact, FixedChildren, Leaf, NoChildren, Optional,
    OptionalChild, RuntimeArity, Single, SliceChildren, Variable,
};

/// Protocol system for unified rendering
pub use protocol::{BoxConstraints, BoxProtocol, LayoutProtocol, Protocol, SliverProtocol};

// Re-export from flui_types
pub use flui_types::{SliverConstraints, SliverGeometry};

/// Type erasure for render objects
pub use render_object::{Constraints as DynConstraints, Geometry as DynGeometry, RenderObject};

/// Safe wrappers for type-erased render objects
pub use wrappers::{BoxRenderWrapper, SliverRenderWrapper};

/// Render element
pub use render_element::RenderElement;

// Core types
/// Parent data and metadata
pub use parent_data::{
    BoxParentData, ContainerBoxParentData, ContainerParentData, ParentData, ParentDataWithOffset,
};

/// Supporting types
pub use render_flags::{AtomicRenderFlags, RenderFlags};
pub use render_state::RenderState;

/// Proxy traits for pass-through render objects
pub use render_proxy::{RenderBoxProxy, RenderSliverProxy};
