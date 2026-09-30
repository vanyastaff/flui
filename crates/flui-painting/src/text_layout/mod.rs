//! Text shaping and layout over cosmic-text, and the Parley path's
//! per-realm text context.
//!
//! - `context` — [`FontCollection`], the app's add-only font collection, and
//!   [`TextContext`], the per-realm service built from it (ADR-0092 §2–§3).
//! - `fallback_chain` — the fallback order both shapers walk past a style's
//!   family.
//! - `font_resolve` — picking a family the host actually carries.
//! - `layout` — the process-wide font system, `TextLayout` (shape, truncate,
//!   caret/hit-test/line queries), and the style → `Attrs` mapping.

use flui_foundation::geometry::Size;

mod context;
pub(crate) mod fallback_chain;
pub(crate) mod font_resolve;
pub(crate) mod layout;

pub(crate) use context::FontsKey;
pub use context::{FontCollection, TextContext};
pub(crate) use layout::paint_color;
pub use layout::{ResolvedFont, Shaper, SharedFontSystem, TextLayout, shared_font_system};
// Test-support only: pinning the process-wide font system is irreversible, so
// it stays off the shipped surface. See its docs.
#[cfg(any(test, feature = "testing"))]
pub use layout::{font_system_initialized, init_font_system_with_faces};

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
    /// Derived from the first line's descent edge (`line_top +
    /// line_height`) — the closest shaper-derived bound until per-font
    /// ideographic metrics are plumbed (cosmic-text does not expose
    /// them per run).
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
