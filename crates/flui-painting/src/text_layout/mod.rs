//! The app's fonts and the per-realm text context Parley shapes through.
//!
//! - `context` — [`FontCollection`], the app's add-only font collection, and
//!   [`TextContext`], the per-realm service built from it (ADR-0092 §2–§3).
//! - `host` — [`HostFonts`], the one scan of the host's installed fonts a
//!   collection is fed from, with its generic families and fallback lists.
//! - `fallback_chain` — the fallback order a host-fed collection walks past
//!   a style's family; `fallback_tables` holds each platform's lists.
//! - `font_resolve` — picking a family the collection actually holds.

use flui_foundation::geometry::Size;

use crate::typography::TextStyle;

mod context;
pub(crate) mod fallback_chain;
#[cfg(test)]
mod fallback_recorded;
mod fallback_tables;
pub(crate) mod font_resolve;
mod host;

pub(crate) use context::FontsKey;
pub use context::{FontCollection, TextContext};
pub use host::HostFonts;

/// The colour a style paints its glyphs with: `foreground` wins over
/// `color`.
pub(crate) fn paint_color(style: &TextStyle) -> Option<crate::styling::Color> {
    style.foreground.or(style.color)
}

/// Text layout result containing computed metrics.
#[derive(Debug, Clone)]
pub struct TextLayoutResult {
    /// Total width of the laid out text.
    pub width: f64,
    /// Total height of the laid out text.
    pub height: f64,
    /// Number of lines after layout.
    pub line_count: usize,
    /// Width of the longest line.
    pub max_line_width: f64,
    /// Distance to alphabetic baseline from top.
    pub alphabetic_baseline: f64,
    /// Distance to the ideographic baseline from top.
    ///
    /// The first line's bottom edge: Parley reports no per-font ideographic
    /// baseline.
    pub ideographic_baseline: f64,
    /// Whether the layout was truncated to a maximum line count.
    pub truncated: bool,
}

impl TextLayoutResult {
    /// Returns the size as a `Size` struct.
    #[inline]
    #[must_use]
    pub fn size(&self) -> Size<f64> {
        Size::new(self.width, self.height)
    }
}
