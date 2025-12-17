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
// Active development - many incomplete features
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(unused_assignments)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]
#![allow(clippy::large_enum_variant)]

pub mod binding;
pub mod constraints;
pub mod containers;
pub mod delegates;
pub mod hit_testing;
pub mod input;
pub mod lifecycle;
pub mod objects;
pub mod parent_data;
pub mod pipeline;
pub mod protocol;
/// Re-export semantics from flui-semantics crate.
pub use flui_semantics as semantics;
pub mod traits;
pub mod tree;
pub mod view;

/// Re-export layer types from flui-layer crate for convenience.
pub mod layer {
    pub use flui_layer::*;
}

/// Prelude module for convenient imports.
pub mod prelude {
    pub use crate::binding::{
        debug_dump_layer_tree, debug_dump_pipeline_owner_tree, debug_dump_render_tree,
        debug_dump_semantics_tree, HitTestDispatcher, HitTestable, PipelineManifold,
        RendererBinding,
    };
    pub use crate::containers::{
        // Box protocol
        BoxChild,
        BoxChildList,
        BoxChildRequired,
        BoxChildren,
        // Primary types
        Child,
        ChildEntry,
        ChildList,
        // Layout aliases
        FlexChildren,
        // Traits
        HasOffset,
        MultiChildContainer,
        MultiChildContainerWithData,
        SingleChildContainer,
        // Sliver protocol
        SliverChild,
        SliverChildRequired,
        SliverChildren,
        StackChildren,
        WrapChildren,
    };
    pub use crate::delegates::{
        AspectRatioDelegate, CenterLayoutDelegate, CustomClipper, CustomPainter, FlowDelegate,
        FlowPaintingContext, MultiChildLayoutContext, MultiChildLayoutDelegate, RectClipper,
        SemanticsBuilder, SingleChildLayoutDelegate, SliverGridDelegate,
        SliverGridDelegateWithFixedCrossAxisCount, SliverGridDelegateWithMaxCrossAxisExtent,
        SliverGridLayout,
    };
    // Hit testing - only protocol-specific types (base types come from flui_interaction)
    pub use crate::hit_testing::{
        BoxHitTestEntry, BoxHitTestResult, MatrixTransformPart, PointerEventKind,
        SliverHitTestEntry, SliverHitTestResult,
    };
    // Re-export base hit testing types from flui_interaction (source of truth)
    pub use crate::input::{
        CursorIcon, MouseCursorSession, MouseTracker, MouseTrackerAnnotation, MouseTrackerHitTest,
        PointerEnterEvent, PointerExitEvent, PointerHoverEvent,
    };
    pub use crate::parent_data::{
        BoxParentData, ContainerBoxParentData, FlexFit, FlexParentData, ParentData,
        SliverGridParentData, SliverMultiBoxAdaptorParentData, SliverParentData,
        SliverPhysicalParentData, StackParentData,
    };
    pub use crate::pipeline::{Canvas, Paint, PaintStyle, PaintingContext, PipelineOwner};
    pub use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};
    pub use crate::semantics::{
        SemanticsAction, SemanticsConfiguration, SemanticsNode, SemanticsNodeUpdate,
        SemanticsOwner, SemanticsTreeUpdate,
    };
    pub use crate::traits::{
        MultiChildRenderBox, RenderAligningShiftedBox, RenderBox, RenderObject, RenderProxyBox,
        RenderProxySliver, RenderShiftedBox, RenderSliver, RenderSliverMultiBoxAdaptor,
        RenderSliverPersistentHeader, RenderSliverSingleBoxAdapter, SingleChildRenderBox,
        TextBaseline,
    };
    pub use crate::view::{
        CacheExtentStyle, CompositeResult, FixedViewportOffset, RenderAbstractViewport, RenderView,
        RevealedOffset, ScrollDirection, ScrollableViewportOffset, SliverPaintOrder,
        ViewConfiguration, ViewportOffset,
    };
    pub use flui_foundation::SemanticsId;
    pub use flui_interaction::{HitTestBehavior, HitTestEntry, HitTestResult, HitTestTarget};

    // Constraints from this crate
    pub use crate::constraints::{BoxConstraints, Constraints, SliverConstraints, SliverGeometry};

    // Lifecycle types
    pub use crate::lifecycle::{
        BaseRenderObject, DirtyFlags, RelayoutBoundary, RenderObjectFlags, RenderObjectState,
    };

    // Tree types
    pub use crate::tree::{RenderNode, RenderTree};

    // Re-export commonly used types from flui_types
    pub use flui_types::{Offset, Point, RRect, Rect, Size};

    // Re-export RenderId from flui_foundation
    pub use flui_foundation::RenderId;

    // Re-export all concrete RenderObjects
    pub use crate::objects::*;
}

// Re-export key types at crate root
pub use parent_data::ParentData;
pub use pipeline::{PaintingContext, PipelineOwner};
pub use protocol::{BoxProtocol, Protocol, SliverProtocol};
pub use traits::{RenderBox, RenderObject, RenderSliver};
