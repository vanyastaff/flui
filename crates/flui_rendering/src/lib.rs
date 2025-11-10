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
//! ## Unified Render Trait Architecture
//!
//! All render objects implement a single unified `Render` trait with:
//!
//! - **layout()**: Computes size given constraints (uses LayoutContext)
//! - **paint()**: Generates layers for rendering (uses PaintContext)
//! - **arity()**: Specifies child count (Exact(0), Exact(1), or Variable)
//! - **as_any()**: Enables metadata access via downcasting
//!
//! This unified approach provides clean abstractions while maintaining
//! type safety and zero-cost performance.
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

// Re-export from flui_core - the unified Render trait architecture
pub use flui_core::render::{
    Arity, Children, LayoutContext, PaintContext, ParentData, ParentDataWithOffset, Render,
    RenderFlags, RenderPipeline, RenderState,
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
    // Effects objects
    PhysicalShape,
    PositionedMetadata,
    RRectShape,
    RectShape,
    // Interaction objects
    RenderAbsorbPointer,
    // Layout objects
    RenderAlign,
    RenderAspectRatio,
    RenderBackdropFilter,
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
    RenderOpacity,
    RenderOverflowBox,

    RenderPadding,
    // Text objects
    RenderParagraph,
    RenderPhysicalModel,
    RenderPointerListener,
    RenderPositioned,
    RenderPositionedBox,
    RenderRepaintBoundary,
    RenderRotatedBox,
    RenderSizedBox,
    RenderSizedOverflowBox,
    RenderStack,
    RenderTransform,
    RenderVisibility,
    RenderWrap,
    WrapAlignment,
    WrapCrossAlignment,
};

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::{Render, RenderFlags, RenderState};

    pub use crate::objects::*;
}
