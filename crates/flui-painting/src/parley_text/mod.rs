//! Paragraph shaping on a realm's [`TextContext`](crate::TextContext)
//! (ADR-0092 §1).
//!
//! `TextPainter` measures every paragraph here and paints the same layout:
//! size, baselines and intrinsics come from [`ParagraphLayout`], and
//! [`ParagraphLayout::to_shaped`] turns it into the
//! [`ShapedParagraph`](crate::display_list::ShapedParagraph) the display list
//! carries. It holds no `static` and takes no lock; every object is owned and
//! used through `&mut`. The raster side is [`crate::glyphs`].
//!
//! - `shape` — [`ParagraphSpec`] in, [`ParagraphLayout`] out, through
//!   `TextContext::shape`.

mod shape;

pub(crate) use shape::SpanBrush;
#[cfg(test)]
pub(crate) use shape::holds_exactly;
pub use shape::{ParagraphLayout, ParagraphSpec};
