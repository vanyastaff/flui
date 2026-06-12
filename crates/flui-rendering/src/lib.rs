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

pub mod binding;
pub mod constraints;
pub mod context;
// Cycle 4 R-16: delegates gated behind `experimental-delegates`
// (default off). The 6 delegate trait modules (~1,800 LOC at
// `delegates/{custom_painter, flow_delegate, multi_child_layout_delegate,
// single_child_layout_delegate, sliver_grid_delegate,
// custom_clipper}.rs`) had zero production impls -- only test mocks.
// The 2026-05-20 audit flagged at MEDIUM with the verdict "wait for
// companion render-objects"; 18 months later those render-objects
// still don't exist. Gating removes the dead surface from default
// builds + prelude while preserving the design work behind a feature
// flag. Opt in via `--features experimental-delegates` when the
// companion render-objects (RenderCustomPaint, RenderFlow,
// RenderCustomMultiChildLayoutBox, RenderSliverGrid, RenderCustomClip,
// RenderSingleChildLayoutBox) land.
#[cfg(feature = "experimental-delegates")]
pub mod delegates;
pub mod error;
pub mod hit_testing;
// Cycle 4 U-6 deleted the rendering-side `input` module entirely.
// Canonical `MouseTracker` + `MouseTrackerAnnotation` + cursor types
// live in `flui_interaction` (Flutter's `gestures/mouse_tracker.dart`
// equivalent). Consumers go through `flui_interaction::MouseTracker`
// directly, or via the prelude re-export at the bottom of this file.
pub mod parent_data;
pub mod pipeline;
pub mod protocol;
/// Re-export semantics from flui-semantics crate.
pub use flui_semantics as semantics;
pub mod objects;
pub mod storage;
// Render-object test harness. Compiled only for this crate's own tests
// (`cfg(test)`) or when a consumer enables the `testing` feature. Builds
// real `PipelineOwner` trees through the production pipeline and exposes a
// protocol-agnostic inspection surface for Box and Sliver render objects.
// See [`testing`] for the module overview.
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod traits;
pub mod view;

/// Re-export layer types from flui-layer crate for convenience.
pub mod layer {
    pub use flui_layer::*;
}

/// Prelude module for convenient imports.
pub mod prelude {
    // Arity system
    // Re-export RenderId from flui_foundation
    pub use flui_foundation::{RenderId, SemanticsId};
    pub use flui_interaction::{HitTestBehavior, HitTestEntry, HitTestResult, HitTestTarget};
    // Re-export commonly used types from flui_types
    pub use flui_types::{Offset, Point, RRect, Rect, Size};

    // Per-child layout state (lives in box_protocol since it's a
    // BoxLayoutCtx implementation detail; re-exported here for
    // convenience via the public-facing `protocol` module surface).
    pub use crate::protocol::ChildState;
    // Constraints from this crate
    pub use crate::constraints::{BoxConstraints, Constraints, SliverConstraints, SliverGeometry};
    // Context types for RenderBox and RenderSliver
    pub use crate::context::{
        BoxHitTestContext, BoxLayoutContext, FragmentRecorder, PaintCx, PaintFragment,
        SliverHitTestContext, SliverLayoutContext,
    };
    // Error types
    pub use crate::error::{RenderError, RenderResult};
    // Hit testing. Cycle 4 U-3 removed the parallel
    // `BoxHitTestEntry`/`BoxHitTestResult`/`SliverHitTestEntry`/
    // `SliverHitTestResult` exports here; the protocol-canonical
    // versions live in `crate::protocol` and are re-exported alongside
    // each `BoxProtocol`/`SliverProtocol` (see lib.rs protocol prelude).
    // Cycle 4 U-5 dropped `PointerEventKind` alongside the deletion of
    // the rendering-side `target.rs` module; canonical pointer-event
    // types live in `flui_interaction::events` (re-exported at line 82
    // via `flui_interaction::{HitTestTarget, ...}`).
    pub use crate::hit_testing::MatrixTransformPart;
    // Mouse-tracking surface (cycle 4 U-6: migrated from the deleted
    // rendering-side `input` module to `flui_interaction`'s canonical
    // types). `MouseCursorSession` / `PointerEnterEvent` /
    // `PointerExitEvent` / `PointerHoverEvent` / `MouseTrackerHitTest`
    // were rendering-specific helpers without flui-interaction-side
    // equivalents; consumers needing them migrated to
    // `flui_interaction::events`-based pointer-event handling.
    pub use flui_interaction::{CursorIcon, MouseTracker, MouseTrackerAnnotation};
    // Protocol adapters for RenderBox -> RenderObject<BoxProtocol> bridging
    pub use crate::protocol::IntoRenderObject;
    // Arity types (canonical home: flui_tree)
    pub use flui_tree::{Arity, Leaf, Optional, Single, Variable};
    // Tree types
    pub use crate::storage::{RenderNode, RenderTree};
    pub use crate::{
        binding::{
            RendererBinding, debug_dump_layer_tree, debug_dump_pipeline_owner_tree,
            debug_dump_render_tree, debug_dump_semantics_tree,
        },
        parent_data::{
            BoxParentData, ContainerBoxParentData, FlexFit, FlexParentData, ParentData,
            SliverGridParentData, SliverMultiBoxAdaptorParentData, SliverParentData,
            SliverPhysicalParentData, StackParentData,
        },
        pipeline::{Canvas, Paint, PaintStyle, PipelineOwner},
        protocol::{BoxProtocol, Protocol, SliverProtocol},
        semantics::{
            SemanticsAction, SemanticsConfiguration, SemanticsNode, SemanticsNodeUpdate,
            SemanticsOwner, SemanticsTreeUpdate,
        },
        traits::{RenderBox, RenderObject, TextBaseline},
        view::{
            CacheExtentStyle, CompositeResult, FixedViewportOffset, RenderAbstractViewport,
            RenderView, RevealedOffset, ScrollDirection, ScrollableViewportOffset,
            SliverPaintOrder, ViewConfiguration, ViewportOffset,
        },
    };
    // Cycle 4 R-16: delegates surface only when
    // `experimental-delegates` is enabled (default off).
    #[cfg(feature = "experimental-delegates")]
    pub use crate::delegates::{
        AspectRatioDelegate, CenterLayoutDelegate, CustomClipper, CustomPainter, FlowDelegate,
        FlowPaintingContext, MultiChildLayoutContext, MultiChildLayoutDelegate, RectClipper,
        SemanticsBuilder, SingleChildLayoutDelegate, SliverGridDelegate,
        SliverGridDelegateWithFixedCrossAxisCount, SliverGridDelegateWithMaxCrossAxisExtent,
        SliverGridLayout,
    };
}

// Re-export key types at crate root
// Context system
pub use context::{
    BoxHitTestContext, BoxLayoutContext, FragmentRecorder, HitTestContext, LayoutContext, PaintCx,
    PaintFragment, SliverHitTestContext, SliverLayoutContext,
};
pub use error::{RenderError, RenderResult};
pub use parent_data::ParentData;
pub use pipeline::PipelineOwner;
pub use protocol::{
    // Marker traits
    BidirectionalProtocol,
    // Concrete capabilities
    BoxHitTest,
    BoxLayout,
    // Core protocol trait and implementations
    BoxProtocol,
    // Capability traits
    HitTestCapability,
    HitTestContextApi,
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
pub use traits::RenderObject;
