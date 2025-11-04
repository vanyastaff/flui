//! # flui_rendering
//!
//! Rendering infrastructure for Flui using the Generic Three-Tree Architecture.
//!
//! This crate provides the RenderObject layer that handles layout and painting.
//! It implements all 81 RenderObjects from Flutter using a generic architecture
//! with zero-cost abstractions.
//!
//! ## Architecture
//!
//! ```text
//! Widget (flui_widgets)
//!     |
//!     v
//! Element (flui_core) - manages LayoutCache
//!     |
//!     v
//! RenderObject (flui_rendering - this crate)
//!     |
//!     v
//! Painting (flui_painting)
//!     |
//!     v
//! egui::Painter
//! ```
//!
//! ## Generic Architecture
//!
//! Instead of implementing 200+ lines per RenderObject, we use 3 generic base types:
//!
//! - **LeafRenderBox<T>**: For widgets with no children (9 types)
//! - **SingleRenderBox<T>**: For widgets with one child (34 types)
//! - **ContainerRenderBox<T>**: For widgets with multiple children (38 types)
//!
//! This reduces code to ~20 lines per RenderObject and provides 10-100x performance
//! improvements through Element-level caching.
//!
//! ## Key Principles
//!
//! 1. **Element manages caching**: LayoutCache lives in Element, not RenderObject
//! 2. **Pure layout logic**: RenderObjects are pure functions with no side effects
//! 3. **Zero-cost abstractions**: Generic types compile to concrete code
//! 4. **Separation of concerns**: RenderObject (logic) vs flui_painting (rendering)

#![warn(missing_docs)]

pub mod error;
pub mod objects;

// Re-export from flui_core - the new trait-based RenderObject architecture
pub use flui_core::render::{
    LeafRender, MultiRender, ParentData, ParentDataWithOffset, RenderFlags, RenderPipeline,
    RenderState, SingleRender,
};

// Re-export from flui_engine for Layer types
pub use flui_engine::BoxedLayer;

// Re-export from flui_types for convenience
pub use flui_types::layout::{FlexFit, StackFit};

// Re-export all RenderObjects
pub use objects::{
    DecorationPosition,
    // Metadata types
    FlexItemMetadata,
    MouseCallbacks,
    ParagraphData,
    PositionedMetadata,
    RRectShape,
    RectShape,
    // Interaction objects
    RenderAbsorbPointer,
    // Layout objects
    RenderAlign,
    RenderAspectRatio,
    RenderBaseline,
    RenderClipOval,
    RenderClipRRect,
    RenderClipRect,
    RenderColoredBox,
    RenderConstrainedBox,
    RenderDecoratedBox,
    RenderFittedBox,
    RenderFlex,
    RenderFlexItem,
    RenderFractionallySizedBox,
    RenderIgnorePointer,
    RenderIndexedStack,
    RenderIntrinsicHeight,
    RenderIntrinsicWidth,
    RenderLimitedBox,
    RenderListBody,
    RenderMouseRegion,
    RenderOffstage,
    RenderOverflowBox,

    // Effects objects
    PhysicalShape,
    RenderBackdropFilter,
    RenderOpacity,
    RenderPadding,
    RenderPhysicalModel,
    RenderPositioned,
    RenderRepaintBoundary,
    // Text objects
    RenderParagraph,
    RenderPointerListener,
    RenderPositionedBox,
    RenderRotatedBox,
    RenderSizedBox,
    RenderSizedOverflowBox,
    RenderStack,
    RenderTransform,
    RenderWrap,
    WrapAlignment,
    WrapCrossAlignment,
};

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::{LeafRender, MultiRender, RenderFlags, RenderState, SingleRender};

    pub use crate::objects::*;
}
