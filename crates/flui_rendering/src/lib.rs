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
//!     fn paint(&self, context: &mut CanvasContext, offset: Offset) {
//!         // Paint implementation
//!     }
//! }
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]
// Rendering crate uses complex generic types for type-safe protocols
#![allow(clippy::type_complexity)]
// Some render objects have many configuration parameters
#![allow(clippy::too_many_arguments)]

pub mod arity;
pub mod binding;
pub mod child_handle;
pub mod children_access;
pub mod constraints;
pub mod context;
pub mod delegates;
pub mod error;
pub mod hit_testing;
pub mod input;
pub mod parent_data;
pub mod pipeline;
pub mod protocol;
/// Re-export semantics from flui-semantics crate.
pub use flui_semantics as semantics;
pub mod objects;
pub mod storage;
pub mod traits;
pub mod view;

/// Re-export layer types from flui-layer crate for convenience.
pub mod layer {
    pub use flui_layer::*;
}

/// Prelude module for convenient imports.
pub mod prelude {
    // Arity system
    pub use crate::arity::{Arity, Leaf, Optional, Single, Variable};

    // Child handles
    pub use crate::child_handle::ChildHandle;

    // Children access
    pub use crate::children_access::{ChildState, ChildrenAccess};

    // Context types for RenderBox and RenderSliver
    pub use crate::context::{
        BoxHitTestContext, BoxLayoutContext, BoxPaintContext, PaintContext, SliverHitTestContext,
        SliverLayoutContext, SliverPaintContext,
    };

    pub use crate::binding::{
        debug_dump_layer_tree, debug_dump_pipeline_owner_tree, debug_dump_render_tree,
        debug_dump_semantics_tree, HitTestDispatcher, HitTestable, PipelineManifold,
        RendererBinding,
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
    pub use crate::pipeline::{Canvas, CanvasContext, Paint, PaintStyle, PipelineOwner};
    pub use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};
    pub use crate::semantics::{
        SemanticsAction, SemanticsConfiguration, SemanticsNode, SemanticsNodeUpdate,
        SemanticsOwner, SemanticsTreeUpdate,
    };
    pub use crate::traits::{RenderBox, RenderObject, TextBaseline};

    // Error types
    pub use crate::error::{RenderError, RenderResult};

    pub use crate::view::{
        CacheExtentStyle, CompositeResult, FixedViewportOffset, RenderAbstractViewport, RenderView,
        RevealedOffset, ScrollDirection, ScrollableViewportOffset, SliverPaintOrder,
        ViewConfiguration, ViewportOffset,
    };
    pub use flui_foundation::SemanticsId;
    pub use flui_interaction::{HitTestBehavior, HitTestEntry, HitTestResult, HitTestTarget};

    // Constraints from this crate
    pub use crate::constraints::{BoxConstraints, Constraints, SliverConstraints, SliverGeometry};

    // Tree types
    pub use crate::storage::{RenderNode, RenderTree};

    // Protocol adapters for RenderBox -> RenderObject<BoxProtocol> bridging
    pub use crate::protocol::IntoRenderObject;

    // Re-export commonly used types from flui_types
    pub use flui_types::{Offset, Point, RRect, Rect, Size};

    // Re-export RenderId from flui_foundation
    pub use flui_foundation::RenderId;
}

// Re-export key types at crate root
pub use error::{RenderError, RenderResult};
pub use parent_data::ParentData;
pub use pipeline::{CanvasContext, PipelineOwner};
pub use traits::RenderObject;

// Context system
pub use context::{
    BoxHitTestContext, BoxLayoutContext, BoxPaintContext, HitTestContext, LayoutContext,
    PaintContext, SliverHitTestContext, SliverLayoutContext, SliverPaintContext,
};
pub use protocol::{
    // Marker traits
    BaselineProtocol,
    BidirectionalProtocol,
    // Concrete capabilities
    BoxHitTest,
    BoxLayout,
    // Core protocol trait and implementations
    BoxProtocol,
    // Capability traits
    HitTestCapability,
    HitTestContextApi,
    IntrinsicProtocol,
    LayoutCapability,
    LayoutContextApi,
    Protocol,
    ProtocolCompatible,
    // Type aliases
    ProtocolConstraints,
    ProtocolGeometry,
    ProtocolHitResult,
    ProtocolHitTestCtx,
    ProtocolLayoutCtx,
    ProtocolPosition,
    ProtocolRenderObject,
    SliverHitTest,
    SliverLayout,
    SliverProtocol,
};
