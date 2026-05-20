//! Text layout engine using cosmic-text.
//!
//! Provides text shaping, measurement, and layout capabilities.
//!
//! # Concern split (Mythos chain U6)
//!
//! The 1,243-LOC `text_layout.rs` god module was split into a
//! `text_layout/` directory. The unnecessary
//! `#[cfg(feature = "text")] mod inner` indirection was flattened:
//! the cfg attribute now sits on the `pub mod text_layout;`
//! declaration in `lib.rs`, not on a `mod inner` wrapper.
//!
//! Files:
//!
//! - [`detect`]   -- `#[cfg(feature = "text")]` RTL/LTR detection helpers.
//! - [`layout`]   -- `#[cfg(feature = "text")]` `FONT_SYSTEM` static + `TextLayout` struct + cursor/hit-test methods.
//! - [`measure`]  -- `#[cfg(feature = "text")]` `measure_text` + `measure_inline_span` + `style_to_attrs` helpers.
//! - [`fallback`] -- `#[cfg(not(feature = "text"))]` stub `TextLayout` + stub `detect_text_direction` + stub `measure_*`.
//!
//! `TextLayoutResult` and `LineInfo` are structurally identical
//! between the cosmic-text impl and the fallback impl, so they live
//! at the module root (no cfg gate).

use flui_types::{
    geometry::{Pixels, Size, px},
    typography::TextDirection,
};

#[cfg(feature = "text")]
pub mod detect;
#[cfg(not(feature = "text"))]
pub mod fallback;
#[cfg(feature = "text")]
pub mod layout;
#[cfg(feature = "text")]
pub mod measure;

#[cfg(feature = "text")]
pub use detect::detect_text_direction;
#[cfg(not(feature = "text"))]
pub use fallback::{TextLayout, detect_text_direction, measure_inline_span, measure_text};
#[cfg(feature = "text")]
pub use layout::TextLayout;
#[cfg(feature = "text")]
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

#[cfg(all(test, feature = "text"))]
mod tests {
    use flui_types::{
        geometry::{Offset, px},
        typography::{TextPosition, TextRange},
    };

    use super::*;

    #[test]
    fn test_measure_simple_text() {
        let result = measure_text("Hello, World!", None, 14.0, None, None);

        assert!(result.width > 0.0);
        assert!(result.height > 0.0);
        assert_eq!(result.line_count, 1);
    }

    #[test]
    fn test_measure_multiline_text() {
        let result = measure_text("Line 1\nLine 2\nLine 3", None, 14.0, None, None);

        assert!(result.height > 0.0);
        assert_eq!(result.line_count, 3);
    }

    #[test]
    fn test_measure_with_width_constraint() {
        let text = "This is a very long line of text that should wrap when constrained";

        let unconstrained = measure_text(text, None, 14.0, None, None);
        let constrained = measure_text(text, None, 14.0, Some(100.0), None);

        assert!(constrained.line_count >= unconstrained.line_count);
        assert!(constrained.max_line_width <= 100.0 + 1.0);
    }

    #[test]
    fn test_measure_empty_text() {
        let result = measure_text("", None, 14.0, None, None);

        assert_eq!(result.line_count, 1);
        assert!(result.height > 0.0);
    }

    #[test]
    fn test_text_layout_creation() {
        let layout = TextLayout::new("Hello, World!", None, 14.0, None, None, TextDirection::Ltr);

        let metrics = layout.metrics();
        assert!(metrics.width > 0.0);
        assert!(metrics.height > 0.0);
        assert_eq!(metrics.line_count, 1);
    }

    #[test]
    fn test_text_layout_caret_position() {
        let mut layout = TextLayout::new("Hello", None, 14.0, None, None, TextDirection::Ltr);

        let start_offset = layout.get_offset_for_caret(TextPosition::upstream(0));
        assert!(start_offset.dx >= px(0.0));

        let mid_offset = layout.get_offset_for_caret(TextPosition::upstream(2));
        assert!(mid_offset.dx > start_offset.dx);

        let end_offset = layout.get_offset_for_caret(TextPosition::upstream(5));
        assert!(end_offset.dx >= mid_offset.dx);
    }

    #[test]
    fn test_text_layout_hit_test() {
        let layout = TextLayout::new("Hello", None, 14.0, None, None, TextDirection::Ltr);

        let pos = layout.get_position_for_offset(Offset::new(px(0.0), px(5.0)));
        assert_eq!(pos.offset, 0);

        let pos = layout.get_position_for_offset(Offset::new(px(1000.0), px(5.0)));
        assert!(pos.offset <= 5);
    }

    #[test]
    fn test_text_layout_line_metrics() {
        let layout = TextLayout::new("Line 1\nLine 2", None, 14.0, None, None, TextDirection::Ltr);

        let metrics = layout.get_line_metrics();
        assert_eq!(metrics.len(), 2);

        assert_eq!(metrics[0].line_number, 0);
        assert!(metrics[0].width > 0.0);

        assert_eq!(metrics[1].line_number, 1);
    }

    #[test]
    fn test_text_layout_selection_boxes() {
        let layout = TextLayout::new("Hello, World!", None, 14.0, None, None, TextDirection::Ltr);

        let boxes = layout.get_boxes_for_range(TextRange::new(1, 5));
        assert!(!boxes.is_empty());

        let first_box = &boxes[0];
        assert!(first_box.rect.width() > px(0.0));
        assert!(first_box.rect.height() > px(0.0));
    }

    #[test]
    fn test_text_layout_word_boundary() {
        let layout = TextLayout::new("Hello World", None, 14.0, None, None, TextDirection::Ltr);

        let boundary = layout.get_word_boundary(TextPosition::upstream(2));
        assert!(boundary.start <= 2);
        assert!(boundary.end >= 2);
    }

    #[test]
    fn test_detect_text_direction_ltr() {
        assert_eq!(detect_text_direction("Hello"), Some(TextDirection::Ltr));
        assert_eq!(detect_text_direction("Привет"), Some(TextDirection::Ltr));
        assert_eq!(detect_text_direction("日本語"), Some(TextDirection::Ltr));
    }

    #[test]
    fn test_detect_text_direction_rtl() {
        assert_eq!(detect_text_direction("مرحبا"), Some(TextDirection::Rtl));
        assert_eq!(detect_text_direction("שלום"), Some(TextDirection::Rtl));
    }

    #[test]
    fn test_detect_text_direction_neutral() {
        assert_eq!(detect_text_direction("123"), None);
        assert_eq!(detect_text_direction("   "), None);
        assert_eq!(detect_text_direction("!@#$%"), None);
    }

    #[test]
    fn test_detect_text_direction_mixed() {
        assert_eq!(detect_text_direction("123 Hello"), Some(TextDirection::Ltr));
        assert_eq!(detect_text_direction("123 مرحبا"), Some(TextDirection::Rtl));
    }

    #[test]
    fn test_line_info_ltr() {
        let layout = TextLayout::new("Hello World", None, 14.0, None, None, TextDirection::Ltr);

        let info = layout.get_line_info();
        assert_eq!(info.len(), 1);
        assert_eq!(info[0].line_number, 0);
        assert!(!info[0].is_rtl);
        assert_eq!(info[0].direction(), TextDirection::Ltr);
    }

    #[test]
    fn test_line_info_rtl() {
        let layout = TextLayout::new("مرحبا بالعالم", None, 14.0, None, None, TextDirection::Rtl);

        let info = layout.get_line_info();
        assert_eq!(info.len(), 1);
    }

    #[test]
    fn test_has_rtl_content() {
        let ltr_layout = TextLayout::new("Hello World", None, 14.0, None, None, TextDirection::Ltr);
        assert!(!ltr_layout.has_rtl_content());
    }

    #[test]
    fn test_line_info_methods() {
        let info = LineInfo {
            line_number: 0,
            is_rtl: true,
            width: 100.0,
            height: 20.0,
            top: 0.0,
            start_index: 0,
            end_index: 10,
        };

        assert_eq!(info.direction(), TextDirection::Rtl);
        assert_eq!(info.bottom(), 20.0);
        assert_eq!(info.len(), 10);
        assert!(!info.is_empty());
    }

    #[test]
    fn test_line_info_empty() {
        let info = LineInfo {
            line_number: 0,
            is_rtl: false,
            width: 0.0,
            height: 20.0,
            top: 0.0,
            start_index: 5,
            end_index: 5,
        };

        assert!(info.is_empty());
        assert_eq!(info.len(), 0);
    }
}
