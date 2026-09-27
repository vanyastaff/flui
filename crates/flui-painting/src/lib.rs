//! Recording 2D drawing into a [`DisplayList`], and shaping text with
//! cosmic-text.
//!
//! [`Canvas`] is `dart:ui`'s `Canvas`: a render object (or a `CustomPaint`
//! painter) draws against it, every command is recorded with the current
//! transform baked in, and [`Canvas::finish`] hands back the immutable list
//! that `flui-engine` replays on the GPU. Nothing is rasterised here.
//!
//! ```rust
//! use flui_painting::{Canvas, Paint};
//! use flui_types::{Rect, geometry::px, styling::Color};
//!
//! let mut canvas = Canvas::new();
//! canvas.save();
//! canvas.translate(10.0, 10.0);
//! canvas.draw_rect(
//!     Rect::from_ltrb(px(0.0), px(0.0), px(40.0), px(40.0)),
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
//! the shape a `RenderParagraph` drives. [`TextLayout`] underneath shapes
//! through the process-wide font system, which the engine's glyph pipeline
//! shares ([`shared_font_system`]) so a face registered through
//! [`SharedFontSystem::register_font`] measures and paints alike.
//!
//! # Decorations
//!
//! [`paint_box_decoration`] and [`paint_table_border`] are the two
//! Flutter-shaped painters (`BoxDecoration`, `TableBorder`) that record
//! straight into a canvas; [`box_decoration_hit_test`] is the matching
//! hit-test.
//!
//! # Threading
//!
//! Every type here is `Send + Sync` value data; a `Canvas` is mutated through
//! `&mut self` by one owner. The one shared resource is the font system
//! behind [`SharedFontSystem`], taken for one shape at a time through
//! [`SharedFontSystem::shape`].
//!
//! The paint vocabulary (`Paint`, `Shader`, `BlendMode`, …) is defined in
//! `flui_types::painting` and re-exported here; a type error names the
//! `flui_types` path.

#![deny(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]
#![warn(rustdoc::private_intra_doc_links)]
#![forbid(unsafe_code)]
// The lock discipline around the font system is the one place a panic
// would matter; every `expect` on a shipped path names its invariant.
#![warn(clippy::expect_used)]
#![warn(clippy::panic)]
#![expect(clippy::uninlined_format_args)]
#![expect(clippy::match_same_arms)]

pub mod canvas;
pub mod decoration;
pub mod display_list;
pub mod error;
#[cfg(feature = "bundled-fonts")]
pub mod fonts;

pub mod table_border;
pub mod text_layout;
pub mod text_painter;

// The Parley path's raster side (ADR-0092 §10 step 1); no production caller yet.
#[cfg(feature = "parley")]
pub mod parley_text;

// Test harness: `record` (`cfg(test)`, or the `testing` feature).
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use canvas::Canvas;
pub use decoration::{DecorationPaintOptions, box_decoration_hit_test, paint_box_decoration};
pub use display_list::{DisplayList, DrawCommand, DrawOp};
pub use error::RegisterFontError;
// `ResolvedFont` carries a `Family`, and a consumer that cannot name it
// cannot hold the result. The one cosmic-text type on this crate's surface
// (ADR-0016 boundary): the shaped buffer never crosses, and glyphs cross as
// opaque `GlyphKey`s (ADR-0067).
pub use cosmic_text::fontdb::Family;
pub use table_border::paint_table_border;
pub use text_layout::{
    GlyphContent, GlyphImage, GlyphKey, GlyphRasterizer, PlacedGlyph, ResolvedFont, Shaper,
    SharedFontSystem, TextLayout, TextLayoutResult, shared_font_system,
};
pub use text_painter::{Invalidation, TextBaseline, TextPainter};

// The paint vocabulary, defined in `flui_types::painting`.
pub use flui_types::painting::{
    BlendMode, Paint, PaintBuilder, PaintStyle, PointMode, Shader, StrokeCap, StrokeJoin,
};
