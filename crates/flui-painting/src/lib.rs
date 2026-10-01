//! Recording 2D drawing into a [`DisplayList`], and shaping text with
//! Parley.
//!
//! [`Canvas`] is where a render object (or a `CustomPaint`
//! painter) draws: every command is recorded with the current
//! transform baked in, and [`Canvas::finish`] hands back the immutable list
//! that `flui-engine` replays on the GPU. Nothing is rasterised here.
//!
//! ```rust
//! use flui_painting::{Canvas, Paint};
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
//! realm's [`TextContext`], paints the layout that measured as a
//! [`ShapedParagraph`], whose glyphs the engine rasterizes through
//! [`glyphs::SwashRasterizer`], and answers carets, selection, hit-testing
//! and line metrics from that same layout. A face registered through
//! [`FontCollection::register_font`] reaches measurement, paint and carets
//! alike.
//!
//! # Decorations
//!
//! [`paint_box_decoration`] and [`paint_table_border`] are the two
//! painters (`BoxDecoration`, `TableBorder`) that record
//! straight into a canvas; [`box_decoration_hit_test`] is the matching
//! hit-test.
//!
//! # Threading
//!
//! Every type here is `Send + Sync` value data; a `Canvas` is mutated through
//! `&mut self` by one owner. Text shaping takes no shared lock: each realm
//! shapes through its own [`TextContext`], used through `&mut`, over the
//! app's [`FontCollection`]. The host's fonts are scanned once per app, as a
//! [`HostFonts`] value the collection is fed from; no font state is
//! process-global.
//!
//! The paint vocabulary (`Paint`, `Shader`, `BlendMode`, …) is defined in
//! [`paint`] and re-exported here; the style values live in [`styling`] and
//! [`typography`].

#![deny(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]
#![warn(rustdoc::private_intra_doc_links)]
#![forbid(unsafe_code)]
// Every `expect` on a shipped path names its invariant.
#![warn(clippy::expect_used)]
#![warn(clippy::panic)]
#![expect(clippy::uninlined_format_args)]
#![expect(clippy::match_same_arms)]

pub mod alignment;
pub mod box_fit;
pub mod canvas;
pub mod decoration;
pub mod display_list;
pub mod error;
#[cfg(feature = "bundled-fonts")]
pub mod fonts;
pub mod glyphs;
mod lerp_impls;
pub mod paint;
pub mod styling;
pub mod typography;

pub mod table_border;
pub mod text_boundaries;
pub mod text_layout;
pub mod text_painter;

// Paragraph shaping on Parley (ADR-0092): the runtime builds the app's
// `FontCollection`, each realm owns a `TextContext` over it, and
// `TextPainter` measures on it and paints the same layout's runs.
pub mod parley_text;

// Test harness: `record` (`cfg(test)`, or the `testing` feature).
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use alignment::{Alignment, AlignmentDirectional, AlignmentGeometry};
pub use box_fit::{BoxFit, BoxShape, FittedSizes};
pub use canvas::Canvas;
pub use decoration::{DecorationPaintOptions, box_decoration_hit_test, paint_box_decoration};
pub use display_list::{DamageExtent, DisplayList, DrawCommand, DrawOp, ShapedParagraph};
pub use error::RegisterFontError;
pub use glyphs::{GlyphContent, GlyphImage, GlyphKey, GlyphRasterizer, PlacedGlyph};
pub use table_border::paint_table_border;
pub use text_layout::{FontCollection, HostFontFeed, HostFonts, TextContext, TextLayoutResult};
pub use text_painter::{Invalidation, TextBaseline, TextPainter};

// The paint vocabulary, defined in `crate::paint`.
pub use crate::paint::{
    BlendMode, Paint, PaintBuilder, PaintStyle, PointMode, Shader, StrokeCap, StrokeJoin,
};
