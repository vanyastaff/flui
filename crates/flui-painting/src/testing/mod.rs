//! Painting test harness, compiled for this crate's own tests and for a
//! consumer that enables the `testing` feature.
//!
//! ```
//! use flui_painting::{Paint, testing::record};
//! use flui_foundation::geometry::Rect;
//! use flui_painting::styling::Color;
//!
//! let list = record(|canvas| {
//!     canvas.draw_rect(
//!         Rect::from_ltrb(0.0, 0.0, 40.0, 40.0),
//!         &Paint::fill(Color::RED),
//!     );
//! });
//! assert_eq!(list.len(), 1);
//! ```

use crate::{Canvas, DisplayList, FontCollection, TextContext};

/// Records drawing commands into a fresh [`DisplayList`]: runs `f` against
/// a new [`Canvas`] and finishes it.
pub fn record(f: impl FnOnce(&mut Canvas)) -> DisplayList {
    let mut canvas = Canvas::new();
    f(&mut canvas);
    canvas.finish()
}

/// How many handles hold `fonts`: the caller's own clones plus one inside
/// each [`TextContext`] built from it. A consumer's tests use it to pin who
/// holds the collection and when they let go.
#[must_use]
pub fn font_collection_holders(fonts: &FontCollection) -> usize {
    fonts.holders()
}

/// How many measurements `text` was lent for: one per layout, intrinsic or
/// dry query a [`TextPainter`](crate::TextPainter) made through it. A
/// consumer's tests use it to show which realm's context a layout measured
/// on.
#[must_use]
pub fn text_context_lends(text: &TextContext) -> u64 {
    text.lends()
}
