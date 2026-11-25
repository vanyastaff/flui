//! Concrete [`RenderBox`] and [`RenderSliver`] catalog for Flui.
//!
//! This crate contains all ready-to-use render objects, organized into domain
//! families. It sits directly above the [`flui_rendering`] engine crate (which
//! owns traits, pipeline, arena, protocol, and contexts) and validates that the
//! engine's custom-object-authoring API through real consumers outside the
//! engine crate. `RENDER_OBJECT_TYPES` in `tests/render_object_harness.rs`
//! tracks exported concrete types; `render_object_types_match_exports` checks
//! that inventory. Behavioral harness rows separately pin their contracts.
//!
//! # Organization
//!
//! Objects are grouped into six domain families:
//!
//! - `layout` — sizing, padding, alignment, baseline, aspect ratio, flex, wrap, flow, list body,
//!   stack, indexed stack, theater, table, transform, fractional translation, fitted box,
//!   intrinsics, overflow and limits, rotation, custom layout delegates, layout builder,
//!   container, toolbar items
//! - `proxy` — paint-effect and annotation proxies (opacity, clips, decoration, color, custom
//!   paint, backdrop filter, shader mask, physical model, repaint boundary, leader/follower,
//!   paint and subtree anchors including [`RenderSliverSubtreeAnchor`], semantics annotations,
//!   annotated region, visibility, skeleton and shimmer mask, ignore baseline, error box)
//! - `interaction` — hit-test and visibility proxies (absorb/ignore pointer, listener, mouse
//!   region, hit slop, content shape, input region, interactive, press pass-through, offstage,
//!   metadata)
//! - `text` — [`RenderEditable`], [`RenderIcon`], [`RenderParagraph`]
//! - `image` — [`RenderImage`], [`RenderTexture`]
//! - `sliver` — the `RenderSliver*` objects other than [`RenderSliverSubtreeAnchor`], and
//!   [`RenderViewport`] and [`RenderShrinkWrappingViewport`] (the Box viewports that drive them)
//!
//! # Flat public surface
//!
//! Render-object types are re-exported flat from this crate root, so the import
//! path is `flui_objects::RenderPadding`.
//!
//! [`RenderBox`]: flui_rendering::traits::RenderBox
//! [`RenderSliver`]: flui_rendering::traits::RenderSliver

// Every public item is documented; keep it that way.
#![deny(missing_docs)]

mod animation_sample;
mod dynamic_type;
mod image;
mod interaction;
mod layout;
mod opacity_sample;
mod proxy;
mod shimmer;
mod shown;
mod sliver;
mod subscription;
mod text;

// --- flat re-exports (layout) ---
pub use layout::{
    AlignMotion, AnimatedSizeState, CrossAxisAlignment, FlexDirection, MainAxisAlignment,
    MainAxisSize, OverflowBoxFit, PositionedSpec, StackFit, TableColumnWidth, TranslationFraction,
    VerticalDirection, WrapAlignment, WrapCrossAlignment,
};
pub use layout::{
    AspectRatioFactor, BuildDuringLayoutCell, FractionFactor, HeaderShrink, HeaderShrinkCell,
    LayoutConstraintsCell, RenderAlign, RenderAnimatedAlign, RenderAnimatedInset,
    RenderAnimatedSize, RenderAspectRatio, RenderBaseline, RenderCenter, RenderConstrainedBox,
    RenderConstrainedOverflowBox, RenderConstraintsTransformBox, RenderContainer,
    RenderCustomMultiChildLayoutBox, RenderCustomSingleChildLayoutBox, RenderFittedBox, RenderFlex,
    RenderFlow, RenderFractionalTranslation, RenderFractionallySizedBox, RenderIndexedStack,
    RenderIntrinsicHeight, RenderIntrinsicWidth, RenderLayoutBuilder, RenderLimitedBox,
    RenderListBody, RenderPadding, RenderRotatedBox, RenderSizedBox, RenderSizedOverflowBox,
    RenderStack, RenderTable, RenderTheater, RenderToolbarItems, RenderTransform, RenderWrap,
    SizeMotion, SizeMotionHandle, ToolbarItemsSplit,
};

// --- flat re-exports (proxy) ---
pub use proxy::{
    ClipGeometry, ClipSourceToken, DecorationPosition, ERROR_BOX_FALLBACK_EXTENT, Oval,
    PathClipConfiguration, RenderAnimatedDecoratedBox, RenderAnimatedOpacity,
    RenderAnimatedTransform, RenderAnnotatedRegion, RenderBackdropFilter, RenderClip,
    RenderClipOval, RenderClipPath, RenderClipRRect, RenderClipRSuperellipse, RenderClipRect,
    RenderColoredBox, RenderCustomPaint, RenderDecoratedBox, RenderErrorBox, RenderFollowerLayer,
    RenderIgnoreBaseline, RenderLeaderLayer, RenderOpacity, RenderPaintAnchor, RenderPhysicalModel,
    RenderPhysicalShape, RenderRepaintBoundary, RenderSemanticsAnnotations, RenderShaderMask,
    RenderShimmerMask, RenderSkeleton, RenderSliverSubtreeAnchor, RenderSubtreeAnchor,
    RenderVisibility, RoundedClipGeometry, SubtreeAnchor, TransformMotion,
};
pub use proxy::{
    GatedCustomAction, RenderExcludeSemantics, RenderIndexedSemantics, RenderMergeSemantics,
    SemanticsActionRoute,
};
pub use proxy::{
    PathClip, PhysicalClipShape, PhysicalClipSource, RectangleClip, RenderPhysicalModelBase,
};
pub use shimmer::{ShimmerBand, ShimmerSweep};

// --- flat re-exports (interaction) ---
pub use interaction::{
    FocusRingStyle, HitShape, MinTarget, MouseRegionCallback, RenderAbsorbPointer,
    RenderContentShape, RenderHitSlop, RenderIgnorePointer, RenderInputRegion, RenderInteractive,
    RenderListener, RenderMetaData, RenderMouseRegion, RenderOffstage, RenderPressPassThrough,
};

// --- flat re-exports (text) ---
pub use text::{
    RenderEditable, RenderIcon, RenderParagraph, ShownLinksListener, TextLinkFocus, TextLinkKey,
    TextLinkRoute,
};

// --- flat re-exports (image) ---
pub use image::{RenderImage, RenderTexture};

// --- flat re-exports (sliver) ---
pub use sliver::{
    CarouselConfig, CarouselGeometry, CarouselItemFrames, CarouselItemPlacement, CarouselLayout,
    CarouselLayoutError, CarouselLayoutKind, MAX_ITEM_EXTENT, RenderSliverCarousel,
};
pub use sliver::{
    FloatingHeaderMode, FloatingMode, FloatingPinnedMode, RenderSliverFloatingHeaderBase,
};
pub use sliver::{
    FloatingHeaderSnapConfiguration, InvalidSliverCrossAxisExtent,
    OverScrollHeaderStretchConfiguration, RenderShrinkWrappingViewport,
    RenderSliverAnimatedOpacity, RenderSliverConstrainedCrossAxis, RenderSliverCrossAxisGroup,
    RenderSliverFillRemaining, RenderSliverFillRemainingAndOverscroll,
    RenderSliverFillRemainingWithScrollable, RenderSliverFillViewport, RenderSliverFixedExtentList,
    RenderSliverFloatingPersistentHeader, RenderSliverFloatingPinnedPersistentHeader,
    RenderSliverGrid, RenderSliverIgnorePointer, RenderSliverList, RenderSliverMainAxisGroup,
    RenderSliverOffstage, RenderSliverOpacity, RenderSliverPadding,
    RenderSliverPinnedPersistentHeader, RenderSliverScrollingPersistentHeader,
    RenderSliverSemanticsAnnotations, RenderSliverToBoxAdapter, RenderViewport, SnapAction,
    SnapCommand, StretchTrigger,
};
pub use sliver::{Placement, PlacementSlot};
