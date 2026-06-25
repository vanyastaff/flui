//! Concrete [`RenderBox`] and [`RenderSliver`] catalog for Flui.
//!
//! This crate contains all ready-to-use render objects, organized into domain
//! families. It sits directly above the [`flui_rendering`] engine crate (which
//! owns traits, pipeline, arena, protocol, and contexts) and validates that the
//! engine's custom-object-authoring API is complete — 37 real objects compiling
//! from outside the engine crate proves the authoring surface needs no additions.
//!
//! # Organization
//!
//! Objects are grouped into six domain families:
//!
//! - `layout` — sizing, alignment, flex, stack, transform, fitted-box
//! - `proxy` — paint-effect proxies (opacity, clip, decoration, color, repaint boundary)
//! - `interaction` — hit-test and visibility proxies (absorb/ignore pointer, offstage, metadata)
//! - `text` — [`RenderParagraph`]
//! - `image` — [`RenderImage`]
//! - `sliver` — all `RenderSliver*` objects and [`RenderViewport`] (the Box viewport that drives them)
//!
//! # Flat public surface
//!
//! All 37 types are re-exported flat from this crate root so the consumer
//! import path is simply `flui_objects::RenderPadding` — identical depth to the
//! old `flui_rendering::objects::RenderPadding`.
//!
//! [`RenderBox`]: flui_rendering::traits::RenderBox
//! [`RenderSliver`]: flui_rendering::traits::RenderSliver

#![warn(missing_docs)]
#![warn(clippy::all)]
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
    AspectRatioFactor, FractionFactor, RenderAlign, RenderAspectRatio, RenderBaseline,
    RenderCenter, RenderConstrainedBox, RenderFittedBox, RenderFlex, RenderFractionalTranslation,
    RenderFractionallySizedBox, RenderLimitedBox, RenderPadding, RenderSizedBox, RenderStack,
    RenderTransform,
};
pub use layout::{
    CrossAxisAlignment, FlexDirection, MainAxisAlignment, MainAxisSize, PositionedSpec, StackFit,
    TranslationFraction,
};

// --- flat re-exports (proxy) ---
pub use proxy::{
    ClipGeometry, CustomClipper, DecorationPosition, Oval, RenderClip, RenderClipOval,
    RenderClipPath, RenderClipRRect, RenderClipRect, RenderColoredBox, RenderDecoratedBox,
    RenderOpacity, RenderRepaintBoundary,
};

// --- flat re-exports (interaction) ---
pub use interaction::{
    MetaDataPayload, RenderAbsorbPointer, RenderIgnorePointer, RenderMetaData, RenderOffstage,
};

// --- flat re-exports (text) ---
pub use text::RenderParagraph;

// --- flat re-exports (image) ---
pub use image::{ImageAlignment, ImageFit, RenderImage};

// --- flat re-exports (sliver) ---
pub use sliver::{
    RenderSliverFillRemaining, RenderSliverFillRemainingAndOverscroll,
    RenderSliverFillRemainingWithScrollable, RenderSliverFillViewport, RenderSliverFixedExtentList,
    RenderSliverIgnorePointer, RenderSliverListLazy, RenderSliverOffstage, RenderSliverOpacity,
    RenderSliverPadding, RenderSliverToBoxAdapter, RenderViewport,
};
