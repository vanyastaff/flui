//! Text layout engine using cosmic-text.
//!
//! Provides text shaping, measurement, and layout capabilities.
//!
//! # Concern split (Mythos chain U6)
//!
//! The 1,243-LOC `text_layout.rs` god module was split into a
//! `text_layout/` directory. Following plan U8 / audit P-3, the
//! parallel stub `TextLayout` (used when the optional text feature
//! was disabled) was deleted along with the feature flag itself.
//! cosmic-text-backed layout is now the only path; the shared
//! `TextLayoutResult` and `LineInfo` types live at the module root.
//!
//! Files:
//!
//! - `detect`   -- RTL/LTR detection helpers.
//! - `layout`   -- `FONT_SYSTEM` static + `TextLayout` struct + cursor/hit-test methods.
//! - `measure`  -- `measure_text` + `measure_inline_span` + `style_to_attrs` helpers.

use flui_types::{
    geometry::{Pixels, Size, px},
    typography::TextDirection,
};

pub(crate) mod detect;
pub(crate) mod layout;
pub(crate) mod measure;

pub use detect::detect_text_direction;
pub use layout::TextLayout;
pub use measure::{measure_inline_span, measure_text};

// ===== Shared types (identical between cosmic-text impl and fallback) =====

/// Text layout result containing computed metrics.
#[derive(Debug, Clone)]
pub struct TextLayoutResult {
    /// Total width of the laid out text.
    pub width: f32,
    /// Total height of the laid out text.
    pub height: f32,
    /// Number of lines after layout.
    pub line_count: usize,
    /// Width of the longest line.
    pub max_line_width: f32,
    /// Distance to alphabetic baseline from top.
    pub alphabetic_baseline: f32,
    /// Distance to the ideographic baseline from top.
    ///
    /// Derived from the first line's descent edge (`line_top +
    /// line_height`) — the closest shaper-derived bound until per-font
    /// ideographic metrics are plumbed (cosmic-text does not expose
    /// them per run).
    pub ideographic_baseline: f32,
    /// Whether the layout was truncated to a maximum line count.
    pub truncated: bool,
}

impl TextLayoutResult {
    /// Returns the size as a `Size` struct.
    #[inline]
    #[must_use]
    pub fn size(&self) -> Size<Pixels> {
        Size::new(px(self.width), px(self.height))
    }
}

/// Extended line information including directionality.
#[derive(Debug, Clone, PartialEq)]
pub struct LineInfo {
    /// Line number (0-indexed).
    pub line_number: usize,
    /// Whether this line is rendered right-to-left.
    pub is_rtl: bool,
    /// Width of the line in pixels.
    pub width: f32,
    /// Height of the line in pixels.
    pub height: f32,
    /// Top position of the line.
    pub top: f32,
    /// Start text index for this line.
    pub start_index: usize,
    /// End text index for this line (exclusive).
    pub end_index: usize,
}

impl LineInfo {
    /// Returns the text direction for this line.
    #[inline]
    #[must_use]
    pub fn direction(&self) -> TextDirection {
        if self.is_rtl {
            TextDirection::Rtl
        } else {
            TextDirection::Ltr
        }
    }

    /// Returns the bottom position of the line.
    #[inline]
    #[must_use]
    pub fn bottom(&self) -> f32 {
        self.top + self.height
    }

    /// Returns the length of text in this line.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.end_index.saturating_sub(self.start_index)
    }

    /// Returns true if the line is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
