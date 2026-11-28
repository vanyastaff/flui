//! Recording 2D drawing into a [`DisplayList`], and shaping text with
//! Parley.
//!
//! [`Canvas`] is where a render object (or a `CustomPaint`
//! painter) draws: every command is recorded with the current
//! transform baked in, and [`Canvas::finish`] hands back the immutable list
//! that `flui-engine` replays on the GPU. Nothing is rasterised here.
//!
//! ```rust
//! use flui_painting::{Canvas, paint::Paint};
//! use flui_foundation::geometry::Rect;
//! use flui_painting::styling::Color;
//!
//! let mut canvas = Canvas::new();
//! canvas.save();
//! canvas.translate(10.0, 10.0);
//! canvas.draw_rect(
//!     Rect::from_ltrb(0.0, 0.0, 40.0, 40.0),
//!     &Paint::fill(Color::RED),
//! );
//! canvas.restore();
//!
//! let list = canvas.finish();
//! assert_eq!(list.len(), 3); // Save, DrawRect, Restore
//! ```
//!
//! # Text
//!
//! [`TextPainter`] lays out an inline span against a width constraint,
//! answers caret / hit-test / line queries on the result, and paints it —
//! the shape a `RenderParagraph` drives. It shapes on Parley through the
//! UI runtime's [`TextContext`], paints the layout that measured as a
//! [`ShapedParagraph`], whose glyphs the engine rasterizes through
//! [`glyphs::SwashRasterizer`], and answers carets, selection, hit-testing
//! and line metrics from that same layout. A face registered through
//! [`FontCollection::register_font`] reaches measurement, paint and carets
//! alike.
//!
//! # Decorations
//!
//! A decoration value records itself: [`styling::BoxDecoration::paint`] and
//! [`styling::TableBorder::paint`] draw straight into a canvas, and
//! [`styling::BoxDecoration::hit_test`] answers against the same shape.
//!
//! # Threading
//!
//! Every type here is `Send + Sync` value data; a `Canvas` is mutated through
//! `&mut self` by one owner. Text shaping takes no shared lock: each UI runtime
//! shapes through its own [`TextContext`], used through `&mut`, over the
//! app's [`FontCollection`]. The host's fonts are scanned once per app, as a
//! [`HostFonts`] value the collection is fed from; no font state is
//! process-global.
//!
//! # Where an item lives
//!
//! Each item has one path. The crate root holds the recording types
//! ([`Canvas`], [`DisplayList`], [`DrawOp`], …), the layout values
//! ([`Alignment`], [`BoxFit`]) and the text engine ([`TextPainter`],
//! [`FontCollection`], [`TextContext`]); [`paint`] holds how a command fills,
//! strokes and blends, [`styling`] the colours, borders and decorations,
//! [`typography`] the text values, and [`glyphs`] the rasterizer seam.

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "test code catches a panic to assert on it (ADR-0303 §5)"
    )
)]
#![deny(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]
#![warn(rustdoc::private_intra_doc_links)]
#![forbid(unsafe_code)]
// Every `expect` on a shipped path names its invariant.
#![warn(clippy::expect_used)]
#![warn(clippy::panic)]
// No FLUI lock on the text path and no SipHash table (clippy.toml `disallowed-types`).
#![warn(clippy::disallowed_types)]

mod alignment;
mod box_fit;
mod canvas;
mod display_list;
mod error;
#[cfg(any(
    feature = "bundled-fonts",
    feature = "material-icons",
    feature = "cupertino-icons"
))]
pub mod fonts;
pub mod glyphs;
mod image_placement;
mod lerp_impls;
pub mod paint;
pub mod styling;
pub mod text_boundaries;
mod text_layout;
mod text_painter;
pub mod typography;

// Paragraph shaping on Parley (ADR-0092): the runtime builds the app's
// `FontCollection`, each ui_runtime owns a `TextContext` over it, and
// `TextPainter` measures on it and paints the same layout's runs.
pub mod parley_text;

// Test harness: `record` (`cfg(test)`, or the `testing` feature).
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use alignment::{Alignment, AlignmentDirectional, AlignmentGeometry};
pub use box_fit::{BoxFit, BoxShape, FittedSizes};
pub use canvas::Canvas;
pub use display_list::{
    BlurSigma, DamageExtent, DisplayList, DrawCommand, DrawCommandBuf, DrawCommands, DrawOp,
    FontBlob, FontFace, ImageRegionPlacement, NestedRRects, NonFiniteOps, Payload, PayloadId,
    ShapedGlyph, ShapedParagraph, ShapedRun,
};
pub use error::RegisterFontError;
pub use image_placement::ImagePlacement;
pub use text_layout::{
    FaceRef, FallbackOrder, FontCollection, HostFaceCache, HostFontFeed, HostFonts, ScriptCode,
    SystemFallback, TextContext, TextLayoutError, TextLayoutResult,
};
pub use text_painter::{
    Invalidation, RetiredTextSizingCohort, TextMeasurement, TextMeasurementError, TextPainter,
    TextPreparationPending, TextResolvedSize, TextSizing, TextSizingAdmission,
    TextSizingAdmissionError, TextSizingCohort, TextSizingConflict, TextSizingSource,
};
