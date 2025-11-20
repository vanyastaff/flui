//! RenderObjects organized by category

// ============================================================================
// Module declarations
// ============================================================================

/// Effect render objects (opacity, transforms, clips)
pub mod effects;

/// Interaction render objects (pointer listeners, gesture detection)
pub mod interaction;

/// Layout render objects (flex, padding, align, etc.)
pub mod layout;

/// Special render objects (custom paint, metadata, semantics, etc.)
pub mod special;

// TODO: Re-enable after migration
// pub mod basic;
// pub mod debug;
// pub mod media;
// pub mod sliver;
// pub mod text;
// pub mod viewport;

// ============================================================================
// Re-exports - Single Arity (Migrated ✅)
// ============================================================================

// Layout Single objects (10 objects) ✅
pub use layout::{
    RenderAspectRatio, RenderBaseline, RenderFractionallySizedBox, RenderIntrinsicHeight,
    RenderIntrinsicWidth, RenderPadding, RenderPositionedBox, RenderRotatedBox, RenderShiftedBox,
    RenderSizedOverflowBox,
};

// Visual Effects Single objects (13 objects from effects) ✅
pub use effects::{
    RenderAnimatedOpacity, RenderBackdropFilter, RenderClipOval, RenderClipPath, RenderClipRRect,
    RenderClipRect, RenderCustomPaint, RenderOffstage, RenderOpacity, RenderRepaintBoundary,
    RenderShaderMask, RenderTransform, RenderVisibility,
};

// Interaction Single objects (4 objects) ✅
pub use interaction::{
    RenderAbsorbPointer, RenderIgnorePointer, RenderMouseRegion, RenderPointerListener,
};

// Semantics Single objects (6 objects) ✅
pub use special::{
    RenderAnnotatedRegion, RenderBlockSemantics, RenderExcludeSemantics, RenderMergeSemantics,
    RenderMetaData, RenderView,
};

// ============================================================================
// TODO: Re-enable after migration
// ============================================================================

// // Debug objects
// pub use debug::{RenderErrorBox, RenderPlaceholder};
//
// // Media objects
// pub use media::*;
//
// // Sliver objects
// pub use sliver::*;
//
// // Text objects
// pub use text::*;
//
// // Viewport objects
// pub use viewport::*;
