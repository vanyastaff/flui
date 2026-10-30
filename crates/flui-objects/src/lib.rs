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
//! - `layout` — sizing, alignment, flex, stack, transform, fitted-box, intrinsic, overflow, rotation
//! - `proxy` — paint-effect proxies (opacity, clip, decoration, color, repaint boundary)
//! - `interaction` — hit-test and visibility proxies (absorb/ignore pointer, offstage, metadata)
//! - `text` — [`RenderEditable`], [`RenderIcon`], [`RenderParagraph`]
//! - `image` — [`RenderImage`]
//! - `sliver` — all `RenderSliver*` objects and [`RenderViewport`] (the Box viewport that drives them)
//!
//! # Flat public surface
//!
//! Render-object types are re-exported flat from this crate root so the consumer
//! import path is simply `flui_objects::RenderPadding` — identical depth to the
//! old `flui_rendering::objects::RenderPadding`.
//!
//! [`RenderBox`]: flui_rendering::traits::RenderBox
//! [`RenderSliver`]: flui_rendering::traits::RenderSliver

// Every public item is documented; keep it that way.
#![deny(missing_docs)]
// Crate-specific relaxations (same rationale as flui-rendering):
#![expect(clippy::too_many_arguments)]

mod animation_sample;
mod image;
mod interaction;
mod layout;
mod opacity_sample;
mod proxy;
mod shimmer;
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
    RenderStack, RenderTable, RenderTheater, RenderTransform, RenderWrap, SizeMotion,
    SizeMotionHandle,
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
    RenderShimmerMask, RenderSkeleton, RenderSubtreeAnchor, RenderVisibility, RoundedClipGeometry,
    SubtreeAnchor, TransformMotion,
};
pub use proxy::{
    GatedCustomAction, RenderExcludeSemantics, RenderIndexedSemantics, RenderMergeSemantics,
    SemanticsActionRoute,
};
pub use shimmer::{ShimmerBand, ShimmerSweep};

// --- flat re-exports (interaction) ---
pub use interaction::{
    FocusRingStyle, HitShape, MinTarget, MouseRegionCallback, RenderAbsorbPointer,
    RenderContentShape, RenderHitSlop, RenderIgnorePointer, RenderInputRegion, RenderListener,
    RenderMetaData, RenderMouseRegion, RenderOffstage,
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
    CarouselLayoutError, MAX_ITEM_EXTENT, RenderSliverCarousel,
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
    SnapCommand, StretchTriggerSignal,
};
pub use sliver::{Placement, PlacementSlot};
