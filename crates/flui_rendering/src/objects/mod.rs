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

/// Text render objects (paragraph, editable text)
pub mod text;

/// Debug render objects (error box, placeholder, performance overlay)
pub mod debug;

/// Media render objects (images, textures)
pub mod media;

// TODO: Re-enable after migration
// pub mod basic;
// pub mod sliver;
// pub mod viewport;

// ============================================================================
// Re-exports - Single Arity (Migrated ✅)
// ============================================================================

// Layout objects ✅
pub use layout::{
    // Additional layout exports
    FlexItemMetadata,
    PositionedMetadata,
    // Optional arity
    RenderAlign,
    // Single arity
    RenderAspectRatio,
    RenderBaseline,
    RenderConstrainedBox,
    // Leaf arity
    RenderEmpty,
    // Variable arity
    RenderFlex,
    RenderFlexItem,
    RenderFractionallySizedBox,
    RenderIndexedStack,
    RenderIntrinsicHeight,
    RenderIntrinsicWidth,
    RenderLimitedBox,
    RenderListBody,
    RenderOverflowBox,
    RenderPadding,
    RenderPositioned,
    RenderPositionedBox,
    RenderRotatedBox,
    RenderScrollView,
    RenderShiftedBox,
    RenderSizedBox,
    RenderSizedOverflowBox,
    RenderStack,
    RenderWrap,
    WrapAlignment,
    WrapCrossAlignment,
};

// Visual Effects Single objects (13 objects from effects) ✅
pub use effects::{
    DecorationPosition,
    // Additional effects exports
    PhysicalShape,
    RRectShape,
    RectShape,
    RenderAnimatedOpacity,
    RenderBackdropFilter,
    RenderClipOval,
    RenderClipPath,
    RenderClipRRect,
    RenderClipRect,
    RenderCustomPaint,
    RenderDecoratedBox,
    RenderOffstage,
    RenderOpacity,
    RenderPhysicalModel,
    RenderRepaintBoundary,
    RenderShaderMask,
    RenderTransform,
    RenderVisibility,
};

// Interaction Single objects (4 objects) ✅
pub use interaction::{
    MouseCallbacks, PointerCallbacks, RenderAbsorbPointer, RenderIgnorePointer, RenderMouseRegion,
    RenderPointerListener,
};

// Semantics Single objects (6 objects) ✅
pub use special::{
    RenderAnnotatedRegion, RenderBlockSemantics, RenderColoredBox, RenderExcludeSemantics,
    RenderFittedBox, RenderMergeSemantics, RenderMetaData, RenderView,
};

// Text Leaf objects (1 object) ✅
pub use text::{ParagraphData, RenderParagraph};

// Debug objects (3 objects) ✅
pub use debug::{RenderErrorBox, RenderPlaceholder};

// Media objects (2 objects) ✅
pub use media::{FilterQuality, ImageFit, RenderImage, RenderTexture, TextureId};

// ============================================================================
// TODO: Re-enable after migration
// ============================================================================

// // Sliver objects
// pub use sliver::*;
//
// // Viewport objects
// pub use viewport::*;
