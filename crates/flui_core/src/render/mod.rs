//! Render system - Unified architecture
//!
//! # Architecture
//!
//! - `Render` trait: Unified trait for all render objects
//! - `Children` enum: Unified child representation (None/Single/Multi)
//! - `LayoutContext` / `PaintContext`: Context structs for operations
//! - `Arity`: Runtime child count validation
//! - `ParentData`: Metadata system (stored in RenderElement)
//!
//! # Pattern
//!
//! ```text
//! View (trait) → Element (enum) → RenderNode → Render trait
//!                                      ↓
//!                                  LayoutContext / PaintContext
//! ```
//!
//! # Implementation Guide
//!
//! To create a renderer, implement the unified `Render` trait:
//!
//! ```rust,ignore
//! impl Render for MyRenderer {
//!     fn layout(&mut self, ctx: &LayoutContext) -> Size { /* ... */ }
//!     fn paint(&self, ctx: &PaintContext) -> BoxedLayer { /* ... */ }
//!     fn arity(&self) -> Arity { Arity::Variable }  // or Exact(n)
//! }
//! ```

// Core modules
pub mod arity;
pub mod cache;
pub mod children;
pub mod parent_data;
pub mod protocol;
pub mod render_ext;
pub mod render_flags;
pub mod render_sliver_state;
pub mod render_state;
pub mod traits;
pub mod type_erasure;
pub mod wrappers;

/// Generic Render trait - box protocol with compile-time arity validation (NEW)
///
/// This is the new public trait for implementing layout and painting with
/// compile-time child count validation via the `A` type parameter.
/// This will eventually replace the legacy `render::Render` trait.
///
/// # Example
///
/// ```rust,ignore
/// impl traits::Render<Single> for RenderPadding {
///     fn layout(&mut self, ctx: &BoxLayoutContext<Single>) -> BoxGeometry { /* ... */ }
///     fn paint(&self, ctx: &BoxPaintContext<Single>) { /* ... */ }
///     fn hit_test(&self, ctx: &BoxHitTestContext<Single>, result: &mut BoxHitTestResult) -> bool { /* ... */ }
/// }
/// ```
pub use traits::Render;

/// Generic SliverRender trait - sliver protocol with compile-time arity validation (NEW)
///
/// This is the new public trait for implementing scrollable layouts with
/// compile-time child count validation via the `A` type parameter.
/// This will eventually replace the legacy `render_sliver::RenderSliver` trait.
///
/// # Example
///
/// ```rust,ignore
/// impl traits::SliverRender<Variable> for RenderSliverList {
///     fn layout(&mut self, ctx: &SliverLayoutContext<Variable>) -> SliverGeometry { /* ... */ }
///     fn paint(&self, ctx: &SliverPaintContext<Variable>) { /* ... */ }
///     fn hit_test(&self, ctx: &SliverHitTestContext<Variable>, result: &mut SliverHitTestResult) -> bool { /* ... */ }
/// }
/// ```
pub use traits::SliverRender;

// Legacy traits removed - use generic `Render<A>` and `SliverRender<A>` instead

/// Children enum - unified child representation
pub use children::Children;

/// Type-safe arity system for compile-time child count validation
pub use arity::{
    Arity, AtLeast, ChildrenAccess, Exact, FixedChildren, Leaf, NoChildren, Optional,
    OptionalChild, Pair, RuntimeArity, Single, SliceChildren, Triple, Variable,
};

/// Protocol system for unified rendering
pub use protocol::{
    BoxConstraints, BoxGeometry, BoxHitTestContext as ProtocolBoxHitTestContext,
    BoxLayoutContext as ProtocolBoxLayoutContext, BoxPaintContext as ProtocolBoxPaintContext,
    BoxProtocol, HasTypedChildren, LayoutProtocol, Protocol, SliverConstraints, SliverGeometry,
    SliverHitTestContext as ProtocolSliverHitTestContext,
    SliverLayoutContext as ProtocolSliverLayoutContext,
    SliverPaintContext as ProtocolSliverPaintContext, SliverProtocol,
};

/// Type erasure for render objects
pub use type_erasure::{DynConstraints, DynGeometry, DynHitTestResult, DynRenderObject};

/// Safe wrappers for type-erased render objects
pub use wrappers::{BoxRenderObjectWrapper, SliverRenderObjectWrapper};

// Legacy contexts removed - use protocol-based contexts from protocol.rs instead

// Core types
/// Parent data and metadata
pub use parent_data::{
    BoxParentData, ContainerBoxParentData, ContainerParentData, ParentData, ParentDataWithOffset,
};

/// Supporting types
pub use cache::{LayoutCache, LayoutCacheKey, LayoutResult};
pub use render_flags::{AtomicRenderFlags, RenderFlags};
pub use render_sliver_state::RenderSliverState;
pub use render_state::RenderState;

// Extension traits for ergonomic render object construction
pub use render_ext::{RenderExt, SliverExt};
