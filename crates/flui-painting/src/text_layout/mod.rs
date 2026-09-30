//! The app's fonts and the per-realm text context Parley shapes through.
//!
//! - `context` — [`FontCollection`], the app's add-only font collection, and
//!   [`TextContext`], the per-realm service built from it (ADR-0092 §2–§3).
//! - `fallback_chain` — the host's fallback order, which a collection fed
//!   from the host walks past a style's family.
//! - `font_resolve` — picking a family the collection actually holds.
//! - `layout` — the process-wide font system: host discovery, the generic
//!   bindings and the fallback lists a host-fed collection is built from.

use flui_foundation::geometry::Size;

mod context;
pub(crate) mod fallback_chain;
#[cfg(test)]
mod fallback_recorded;
pub(crate) mod font_resolve;
pub(crate) mod layout;

pub(crate) use context::FontsKey;
pub use context::{FontCollection, TextContext};
pub(crate) use layout::paint_color;
pub use layout::{SharedFontSystem, shared_font_system};
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
