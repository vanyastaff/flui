//! # flui-objects
//!
//! Concrete RenderObject implementations for the FLUI framework.
//!
//! This crate provides all the built-in render objects organized by category:
//! - **layout**: Flex, Stack, Padding, Align, etc.
//! - **effects**: Opacity, Transform, Clip, BackdropFilter, etc.
//! - **interaction**: PointerListener, MouseRegion, AbsorbPointer, etc.
//! - **special**: ColoredBox, FittedBox, Semantics, etc.
//! - **text**: Paragraph rendering
//! - **media**: Image, Texture
//!
//! ## Architecture
//!
//! All render objects implement traits from `flui_rendering`:
//! - `RenderObject` - Base trait for all render objects
//! - `RenderBox<A>` - Box protocol with arity (Leaf, Single, Optional, Variable)
//!
//! ## Example
//!
//! ```rust,ignore
//! use flui_objects::{RenderPadding, RenderFlex, RenderOpacity};
//! use flui_types::EdgeInsets;
//!
//! let padding = RenderPadding::new(EdgeInsets::all(16.0));
//! let flex = RenderFlex::row();
//! let opacity = RenderOpacity::new(0.5);
//! ```

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

/// Media render objects (images, textures)
pub mod media;

// TODO: Re-enable after full migration
// pub mod basic;
// pub mod debug;
// pub mod sliver;
// pub mod viewport;

// ============================================================================
// Re-exports - Layout objects
// ============================================================================

pub use layout::{
    // Metadata types
    FlexItemMetadata,
    PositionedMetadata,
    // Render objects
    RenderAlign,
    RenderAspectRatio,
    RenderBaseline,
    RenderConstrainedBox,
    RenderEmpty,
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
    // Alignment enums
    WrapAlignment,
    WrapCrossAlignment,
};

// ============================================================================
// Re-exports - Visual Effects objects
// ============================================================================

pub use effects::{
    // Types and enums
    DecorationPosition,
    PhysicalShape,
    RRectShape,
    RectShape,
    // Render objects
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

// ============================================================================
// Re-exports - Interaction objects
// ============================================================================

pub use interaction::{
    MouseCallbacks, PointerCallbacks, RenderAbsorbPointer, RenderIgnorePointer, RenderMouseRegion,
    RenderPointerListener,
};

// ============================================================================
// Re-exports - Special objects (Semantics, etc.)
// ============================================================================

pub use special::{
    RenderAnnotatedRegion, RenderBlockSemantics, RenderColoredBox, RenderExcludeSemantics,
    RenderFittedBox, RenderMergeSemantics, RenderMetaData, RenderView,
};

// ============================================================================
// Re-exports - Text objects
// ============================================================================

pub use text::{ParagraphData, RenderParagraph};

// ============================================================================
// Re-exports - Debug objects (TODO: re-enable after migration)
// ============================================================================

// pub use debug::{RenderErrorBox, RenderPlaceholder};

// ============================================================================
// Re-exports - Media objects
// ============================================================================

pub use media::{FilterQuality, ImageFit, RenderImage, RenderTexture, TextureId};

// ============================================================================
// Prelude for convenient imports
// ============================================================================

/// Commonly used types for convenient importing.
pub mod prelude {
    // Layout
    pub use crate::layout::{
        RenderAlign, RenderEmpty, RenderFlex, RenderPadding, RenderSizedBox, RenderStack,
    };

    // Effects
    pub use crate::effects::{RenderClipRect, RenderOpacity, RenderTransform};

    // Interaction
    pub use crate::interaction::{RenderIgnorePointer, RenderPointerListener};

    // Special
    pub use crate::special::{RenderColoredBox, RenderFittedBox};

    // Text
    pub use crate::text::RenderParagraph;
}
