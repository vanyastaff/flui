//! Concrete [`RenderBox`] and [`RenderSliver`] catalog for Flui.
//!
//! This crate contains all ready-to-use render objects, organized into domain
//! families. It sits directly above the [`flui_rendering`] engine crate (which
//! owns traits, pipeline, arena, protocol, and contexts) and validates that the
//! engine's custom-object-authoring API is complete — 74 real objects compiling
//! from outside the engine crate proves the authoring surface needs no additions.
//!
//! # Organization
//!
//! Objects are grouped into six domain families:
//!
//! - `layout` — sizing, alignment, flex, stack, transform, fitted-box, intrinsic, overflow, rotation
//! - `proxy` — paint-effect proxies (opacity, clip, decoration, color, repaint boundary)
//! - `interaction` — hit-test and visibility proxies (absorb/ignore pointer, offstage, metadata)
//! - `text` — [`RenderEditable`], [`RenderParagraph`]
//! - `image` — [`RenderImage`]
//! - `sliver` — all `RenderSliver*` objects and [`RenderViewport`] (the Box viewport that drives them)
//!
//! # Flat public surface
//!
//! All 74 render-object types are re-exported flat from this crate root so the consumer
//! import path is simply `flui_objects::RenderPadding` — identical depth to the
//! old `flui_rendering::objects::RenderPadding`.
//!
//! [`RenderBox`]: flui_rendering::traits::RenderBox
//! [`RenderSliver`]: flui_rendering::traits::RenderSliver

// Ship bar (wave 3): every public item is documented; keep it that way.
#![deny(missing_docs)]
// Crate-specific relaxations (same rationale as flui-rendering):
#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]

mod image;
mod interaction;
mod layout;
mod proxy;
mod sliver;
mod text;

// --- flat re-exports (layout) ---
pub use layout::{
    AnimatedSizeState, CrossAxisAlignment, DelegateChange, FlexDirection, MainAxisAlignment,
    MainAxisSize, OverflowBoxFit, PositionedSpec, StackFit, TranslationFraction, WrapAlignment,
    WrapCrossAlignment,
};
pub use layout::{
    AspectRatioFactor, FractionFactor, RenderAlign, RenderAnimatedSize, RenderAspectRatio,
    RenderBaseline, RenderCenter, RenderConstrainedBox, RenderConstrainedOverflowBox,
    RenderCustomMultiChildLayoutBox, RenderCustomSingleChildLayoutBox, RenderFittedBox, RenderFlex,
    RenderFlow, RenderFractionalTranslation, RenderFractionallySizedBox, RenderIndexedStack,
    RenderIntrinsicHeight, RenderIntrinsicWidth, RenderLimitedBox, RenderListBody, RenderPadding,
    RenderRotatedBox, RenderSizedBox, RenderSizedOverflowBox, RenderStack, RenderTable,
    RenderTransform, RenderWrap,
};

// --- flat re-exports (proxy) ---
pub use proxy::{
    ClipGeometry, CustomClipper, DecorationPosition, Oval, RenderBackdropFilter, RenderClip,
    RenderClipOval, RenderClipPath, RenderClipRRect, RenderClipRect, RenderColoredBox,
    RenderCustomPaint, RenderDecoratedBox, RenderFollowerLayer, RenderLeaderLayer, RenderOpacity,
    RenderPhysicalModel, RenderPhysicalShape, RenderRepaintBoundary, RenderSemanticsAnnotations,
    RenderShaderMask, ShaderCallback,
};
pub use proxy::{RenderExcludeSemantics, RenderMergeSemantics};

// --- flat re-exports (interaction) ---
pub use interaction::{
    MetaDataPayload, MouseRegionCallback, RenderAbsorbPointer, RenderIgnorePointer, RenderListener,
    RenderMetaData, RenderMouseRegion, RenderOffstage,
};

// --- flat re-exports (text) ---
pub use text::{RenderEditable, RenderParagraph};

// --- flat re-exports (image) ---
pub use image::{ImageAlignment, ImageFit, RenderImage};

// --- flat re-exports (sliver) ---
pub use sliver::{
    FloatingHeaderSnapConfiguration, OverScrollHeaderStretchConfiguration,
    RenderShrinkWrappingViewport, RenderSliverFillRemaining,
    RenderSliverFillRemainingAndOverscroll, RenderSliverFillRemainingWithScrollable,
    RenderSliverFillViewport, RenderSliverFixedExtentList, RenderSliverFloatingPersistentHeader,
    RenderSliverFloatingPinnedPersistentHeader, RenderSliverGrid, RenderSliverGridLazy,
    RenderSliverIgnorePointer, RenderSliverList, RenderSliverListLazy, RenderSliverOffstage,
    RenderSliverOpacity, RenderSliverPadding, RenderSliverPinnedPersistentHeader,
    RenderSliverScrollingPersistentHeader, RenderSliverToBoxAdapter, RenderViewport,
};
