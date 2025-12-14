//! FLUI Rendering - Flutter-inspired render object system for Rust.
//!
//! This crate provides the rendering layer for FLUI, implementing Flutter's
//! proven three-tree architecture with Rust's type safety guarantees.
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
//! ```ignore
//! use flui_rendering::prelude::*;
//!
//! // Implement a simple render object
//! struct MyRenderBox {
//!     size: Size,
//! }
//!
//! impl RenderBox for MyRenderBox {
//!     fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
//!         self.size = constraints.biggest();
//!         self.size
//!     }
//!
//!     fn size(&self) -> Size {
//!         self.size
//!     }
//!
//!     fn paint(&self, context: &mut PaintingContext, offset: Offset) {
//!         // Paint implementation
//!     }
//! }
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod containers;
pub mod parent_data;
pub mod pipeline;
pub mod protocol;
pub mod traits;

/// Prelude module for convenient imports.
pub mod prelude {
    pub use crate::containers::{
        Aligning, AligningBox, AligningSliver, BoxChild, BoxChildren, Children, Proxy, ProxyBox,
        Shifted, ShiftedBox, ShiftedSliver, Single, SliverChild, SliverChildren, SliverProxy,
    };
    pub use crate::parent_data::{
        BoxParentData, ContainerBoxParentData, FlexFit, FlexParentData, ParentData,
        SliverGridParentData, SliverMultiBoxAdaptorParentData, SliverParentData,
        SliverPhysicalParentData, StackParentData,
    };
    pub use crate::pipeline::{Canvas, Paint, PaintStyle, PaintingContext, PipelineOwner};
    pub use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};
    pub use crate::traits::{
        BoxHitTestEntry, BoxHitTestResult, MultiChildRenderBox, RenderBox, RenderObject,
        RenderObjectExt, RenderProxySliver, RenderSliver, RenderSliverMultiBoxAdaptor,
        RenderSliverSingleBoxAdapter, SingleChildRenderBox, SliverHitTestEntry,
        SliverHitTestResult, TextBaseline,
    };

    // Re-export commonly used types from flui_types
    pub use flui_types::{
        BoxConstraints, Offset, Point, RRect, Rect, Size, SliverConstraints, SliverGeometry,
    };
}

// Re-export key types at crate root
pub use parent_data::ParentData;
pub use pipeline::{PaintingContext, PipelineOwner};
pub use protocol::{BoxProtocol, Protocol, SliverProtocol};
pub use traits::{RenderBox, RenderObject, RenderSliver};
