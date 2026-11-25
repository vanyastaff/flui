//! FLUI Rendering - the render object system for Rust.
//!
//! This crate provides the rendering layer for FLUI: the render-object tree
//! with layout, paint and hit-test protocols, backed by Rust's type safety
//! guarantees.
//!
//! # Architecture
//!
//! The rendering system is built around these core concepts:
//!
//! - **RenderObject**: Base trait for all renderable objects
//! - **RenderBox**: 2D cartesian layout (most widgets)
//! - **RenderSliver**: Scrollable content layout
//! - **Protocol**: Type-safe abstraction over layout protocols
//!
//! # Module Structure
//!
//! - [`parent_data`]: Metadata stored on children by parents
//! - [`traits`]: Core trait definitions (RenderObject, RenderBox, RenderSliver)
//! - [`protocol`]: Protocol trait and implementations
//! - [`pipeline`]: Rendering pipeline management
//!
//! # Example
//!
//! ```
//! use flui_foundation::{Leaf, geometry::Size};
//! use flui_rendering::{
//!     RenderResult,
//!     context::{BoxLayoutContext, PaintContext},
//!     parent_data::BoxParentData,
//!     traits::RenderBox,
//! };
//!
//! #[derive(Debug)]
//! struct MyRenderBox { preferred_size: Size }
//! impl flui_foundation::inspect::Inspect for MyRenderBox {
//!     fn kind(&self) -> &'static str { "MyRenderBox" }
//!     fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
//! }
//!
//! impl RenderBox for MyRenderBox {
//!     type Arity = Leaf;
//!     type ParentData = BoxParentData;
//!
//!     fn perform_layout(
//!         &mut self,
//!         ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
//!     ) -> RenderResult<Size> {
//!         Ok(ctx.constrain(self.preferred_size))
//!     }
//!
//!     fn paint(&self, _ctx: &mut PaintContext<'_, Leaf>) {}
//! }
//! ```

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "test code catches a panic to assert on it (ADR-0303 §5)"
    )
)]
// Lint levels come from `[workspace.lints]` (Cargo.toml `[lints] workspace = true`).
#![deny(missing_docs)]

// Public for exported macro expansion in downstream crates, not an authoring alias.
#[doc(hidden)]
pub use flui_foundation::Single as __MacroSingle;
#[doc(hidden)]
pub use flui_foundation::geometry::Size as __MacroSize;

pub mod audit;
pub mod constraints;
pub mod context;
pub mod delegates;
pub mod error;
pub mod input_regions;
pub mod inspect;
pub mod parent_data;
pub mod pipeline;
pub mod protocol;
// `objects` module removed: concrete render objects live in the `flui-objects`
// crate (the flui-objects extraction). flui-rendering now exports
// only engine primitives (traits, pipeline, protocol, contexts, arena).
pub mod storage;
// Protocol-agnostic windowing math (ADR-0003). Its public surface names no
// render/sliver/protocol type, so it stays a general-purpose abstraction and is
// cheaply extractable into a standalone crate once a 2nd direct consumer
// appears. The `SliverConstraints -> ScrollWindow` adapter lives outside it.
pub mod virtualization;
// Render-object test harness. Compiled only for this crate's own tests
// (`cfg(test)`) or when a consumer enables the `testing` feature. Builds
// real `PipelineOwner` trees through the production pipeline and exposes a
// protocol-agnostic inspection surface for Box and Sliver render objects.
// See [`testing`] for the module overview.
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod traits;
mod update;
pub mod view;

// Re-export key types at crate root
// Context system
pub use context::{
    BoxHitTestContext, BoxLayoutContext, FragmentRecorder, HitTestContext, LayoutContext,
    PaintContext, PaintFragment, SliverHitTestContext, SliverLayoutContext,
};
pub use error::{PoisonPhase, RenderError, RenderResult, SemanticsError};
pub use parent_data::ParentData;
pub use pipeline::{
    LayoutPremise, PipelineCell, PipelineOwner, TextContextHandle, TextContextMut, TextSource,
};
pub use protocol::{
    BoxChildSize, BoxProtocol, ChildLayout, HitTestContextApi, LayoutContextApi, Protocol,
    SliverProtocol,
};
pub use traits::RenderObject;
pub use update::RenderUpdateImpact;
