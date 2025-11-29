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

/// Sliver render objects (scrolling, lists, grids)
pub mod sliver;
/// Viewport and scrolling infrastructure for sliver-based layouts.
///
/// This module provides the core viewport render objects for scrolling:
/// - [`viewport::RenderViewport`] - Full viewport with bidirectional scrolling support
/// - [`viewport::RenderShrinkWrappingViewport`] - Viewport that sizes to content
/// - [`viewport::ViewportOffset`] - Scroll position management
///
/// # Architecture
///
/// Viewports are box protocol render objects that contain sliver children.
/// They convert box constraints into sliver constraints and manage the
/// scroll offset to determine which slivers are visible.
pub mod viewport;

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
    // Variable arity
    RenderCustomMultiChildLayoutBox, // Phase 4: Custom delegate layout
    // Leaf arity
    RenderEmpty,
    RenderFlex,
    RenderFlexItem,
    RenderFlow, // Phase 4: Flow delegate layout
    RenderFractionallySizedBox,
    RenderIndexedStack,
    RenderIntrinsicHeight,
    RenderIntrinsicWidth,
    RenderLimitedBox,
    RenderListBody,
    RenderListWheelViewport, // Phase 6: 3D wheel viewport
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
    RenderTable, // Phase 6: Table layout
    RenderWrap,
    TableCellVerticalAlignment, // Phase 6: Table types
    TableColumnWidth,           // Phase 6: Table types
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
    RenderPhysicalShape,
    RenderRepaintBoundary,
    RenderShaderMask,
    RenderTransform,
    RenderVisibility,
    ShapeClipper,
};

// Interaction Single objects (6 objects) ✅
pub use interaction::{
    MouseCallbacks, MouseCursor, PointerCallbacks, RenderAbsorbPointer, RenderIgnorePointer,
    RenderMouseRegion, RenderPointerListener, RenderSemanticsGestureHandler, RenderTapRegion,
    SemanticsGestureCallbacks, TapRegionCallbacks, TapRegionGroupId,
};

// Semantics Single objects (6 objects) ✅
pub use special::{
    RenderAnnotatedRegion, RenderBlockSemantics, RenderColoredBox, RenderExcludeSemantics,
    RenderFittedBox, RenderMergeSemantics, RenderMetaData, RenderView,
};

// Text Leaf objects (1 object) ✅
pub use text::{ParagraphData, RenderParagraph};

// Debug objects (3 objects) ✅
pub use debug::{RenderErrorBox, RenderPerformanceOverlay, RenderPlaceholder};

// Media objects (2 objects) ✅
pub use media::{FilterQuality, ImageFit, RenderImage, RenderTexture, TextureId};

// Sliver objects (Phase 1-5: 15 slivers migrated) ✅
pub use sliver::{
    // Phase 1: Proxy slivers (5)
    RenderSliverAnimatedOpacity,
    RenderSliverAppBar,
    RenderSliverConstrainedCrossAxis,
    RenderSliverCrossAxisGroup,
    // Phase 2: Manual slivers (5)
    RenderSliverEdgeInsetsPadding,
    RenderSliverFillRemaining,
    RenderSliverFillViewport,
    // Phase 5: Essential slivers (3 + grid delegates)
    RenderSliverFixedExtentList,
    RenderSliverFloatingPersistentHeader,
    RenderSliverGrid,
    RenderSliverIgnorePointer,
    // Phase 3: Multi-box infrastructure (2 + 1 trait)
    RenderSliverList,
    RenderSliverMainAxisGroup,
    RenderSliverMultiBoxAdaptor,
    RenderSliverOffstage,
    RenderSliverOpacity,
    RenderSliverOverlapAbsorber,
    RenderSliverPadding,
    RenderSliverPersistentHeader,
    RenderSliverPinnedPersistentHeader,
    RenderSliverPrototypeExtentList,
    // Phase 7: Advanced slivers (8)
    RenderSliverSafeArea,
    RenderSliverToBoxAdapter,
    SliverGridDelegate,
    SliverGridDelegateFixedCrossAxisCount,
    SliverMultiBoxAdaptorParentData,
    SliverOverlapAbsorberHandle,
};

// Viewport objects
pub use viewport::{
    CacheExtentStyle, Clip, RenderAbstractViewport, RenderShrinkWrappingViewport, RenderViewport,
    RevealedOffset, ScrollDirection, ViewportOffset, ViewportOffsetCallback, DEFAULT_CACHE_EXTENT,
};
