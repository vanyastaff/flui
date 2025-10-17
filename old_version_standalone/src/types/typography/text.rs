//! Text-related types and utilities
//!
//! This module contains enums and types for text rendering and layout,
//! similar to Flutter's text system but adapted for egui.

/// How overflowing text should be handled.
///
/// Similar to CSS `text-overflow` property and Flutter's `TextOverflow`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextOverflow {
    /// Clip the overflowing text to its container.
    ///
    /// This is the default behavior.
    #[default]
    Clip,
    
    /// Use an ellipsis to indicate that text has been clipped.
    ///
    /// Example: "Long text that do..." 
    Ellipsis,
    
    /// Fade the overflowing text out transparently.
    ///
    /// Creates a smooth gradient fade at the edge of the container.
    Fade,
    
    /// Render the text outside its container.
    ///
    /// The text may extend beyond the boundaries of its parent.
    Visible,
}

impl TextOverflow {
    /// Check if this overflow mode requires custom clipping
    pub fn requires_clipping(self) -> bool {
        matches!(self, TextOverflow::Clip | TextOverflow::Ellipsis | TextOverflow::Fade)
    }
    
    /// Check if this overflow mode requires custom painting
    pub fn requires_custom_painting(self) -> bool {
        matches!(self, TextOverflow::Ellipsis | TextOverflow::Fade)
    }
}

/// How text should be aligned horizontally.
///
/// Similar to CSS `text-align` property and Flutter's `TextAlign`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    /// Align text to the left edge.
    #[default]
    Left,
    
    /// Align text to the right edge.
    Right,
    
    /// Center the text horizontally.
    Center,
    
    /// Justify the text, stretching lines to fill the available width.
    Justify,
    
    /// Align text to the start edge, which depends on the text direction.
    ///
    /// For left-to-right text, this is the same as `Left`.
    /// For right-to-left text, this is the same as `Right`.
    Start,
    
    /// Align text to the end edge, which depends on the text direction.
    ///
    /// For left-to-right text, this is the same as `Right`.
    /// For right-to-left text, this is the same as `Left`.
    End,
}

impl TextAlign {
    /// Convert to egui's alignment
    pub fn to_egui_align(self) -> egui::Align {
        match self {
            TextAlign::Left | TextAlign::Start => egui::Align::LEFT,
            TextAlign::Right | TextAlign::End => egui::Align::RIGHT,
            TextAlign::Center => egui::Align::Center,
            TextAlign::Justify => egui::Align::LEFT, // Handled separately
        }
    }
    
    /// Resolve start/end alignments based on text direction
    pub fn resolve(self, text_direction: TextDirection) -> Self {
        match self {
            TextAlign::Start => match text_direction {
                TextDirection::Ltr => TextAlign::Left,
                TextDirection::Rtl => TextAlign::Right,
            },
            TextAlign::End => match text_direction {
                TextDirection::Ltr => TextAlign::Right,
                TextDirection::Rtl => TextAlign::Left,
            },
            _ => self,
        }
    }
    
    /// Check if this alignment requires custom layout logic
    pub fn requires_custom_layout(self) -> bool {
        matches!(self, TextAlign::Justify)
    }
}

/// The direction in which text flows.
///
/// Similar to CSS `direction` property and Flutter's `TextDirection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextDirection {
    /// Left to right text direction.
    #[default]
    Ltr,
    
    /// Right to left text direction.
    Rtl,
}

impl TextDirection {
    /// Check if this is left-to-right direction
    pub fn is_ltr(self) -> bool {
        matches!(self, TextDirection::Ltr)
    }
    
    /// Check if this is right-to-left direction  
    pub fn is_rtl(self) -> bool {
        matches!(self, TextDirection::Rtl)
    }
    
    /// Get the opposite direction
    pub fn opposite(self) -> Self {
        match self {
            TextDirection::Ltr => TextDirection::Rtl,
            TextDirection::Rtl => TextDirection::Ltr,
        }
    }
}

/// A horizontal line used for aligning text.
///
/// Similar to Flutter's `TextBaseline`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextBaseline {
    /// The horizontal line used to align the bottom of alphabetic characters.
    ///
    /// This is the default baseline for most Latin-based scripts.
    #[default]
    Alphabetic,
    
    /// The horizontal line used to align ideographic characters.
    ///
    /// This is used for Chinese, Japanese, Korean, and other ideographic scripts.
    Ideographic,
}

/// How the "leading" (line height) is distributed over and under the text.
///
/// Similar to Flutter's `TextLeadingDistribution`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextLeadingDistribution {
    /// Distribute the leading evenly above and below the text.
    ///
    /// This is the typical behavior in most layout systems.
    #[default]
    Even,
    
    /// Place all the leading below the text.
    ///
    /// This can be useful for certain typographic layouts.
    Bottom,
    
    /// Place all the leading above the text.
    ///
    /// Less common, but useful for specific design requirements.
    Top,
}

/// Defines how to apply text height behavior.
///
/// Similar to Flutter's `TextHeightBehavior`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextHeightBehavior {
    /// Whether to apply height to the text.
    pub apply_height_to_first_ascent: bool,
    
    /// Whether to apply height to the last descent.
    pub apply_height_to_last_descent: bool,
    
    /// How to distribute the leading.
    pub leading_distribution: TextLeadingDistribution,
}

impl TextHeightBehavior {
    /// Create a new text height behavior with default values.
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Create a text height behavior that applies to both ascent and descent.
    pub fn all() -> Self {
        Self {
            apply_height_to_first_ascent: true,
            apply_height_to_last_descent: true,
            leading_distribution: TextLeadingDistribution::Even,
        }
    }
    
    /// Create a text height behavior that doesn't apply to ascent or descent.
    pub fn none() -> Self {
        Self {
            apply_height_to_first_ascent: false,
            apply_height_to_last_descent: false,
            leading_distribution: TextLeadingDistribution::Even,
        }
    }
}

/// Different ways of measuring the width of text.
///
/// Similar to Flutter's `TextWidthBasis`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextWidthBasis {
    /// Measure the width of the longest line.
    ///
    /// This is the default behavior.
    #[default]
    LongestLine,
    
    /// Measure the width of the parent container.
    ///
    /// The text will be constrained to the parent's width.
    Parent,
}

/// Configuration for text selection and manipulation.
///
/// Similar to Flutter's `TextSelection` but simplified.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextSelection {
    /// The start offset of the selection.
    pub base_offset: usize,
    
    /// The end offset of the selection.
    pub extent_offset: usize,
    
    /// The direction of the selection.
    pub affinity: TextAffinity,
}

impl TextSelection {
    /// Create a new text selection.
    pub fn new(base_offset: usize, extent_offset: usize) -> Self {
        Self {
            base_offset,
            extent_offset,
            affinity: TextAffinity::default(),
        }
    }
    
    /// Create a collapsed selection (cursor) at the given offset.
    pub fn collapsed(offset: usize) -> Self {
        Self {
            base_offset: offset,
            extent_offset: offset,
            affinity: TextAffinity::default(),
        }
    }
    
    /// Check if the selection is collapsed (cursor position).
    pub fn is_collapsed(&self) -> bool {
        self.base_offset == self.extent_offset
    }
    
    /// Get the start offset of the selection.
    pub fn start(&self) -> usize {
        self.base_offset.min(self.extent_offset)
    }
    
    /// Get the end offset of the selection.
    pub fn end(&self) -> usize {
        self.base_offset.max(self.extent_offset)
    }
    
    /// Get the length of the selection.
    pub fn length(&self) -> usize {
        self.end() - self.start()
    }
}

/// A way to disambiguate a text position when its offset could match
/// two different locations in the rendered string.
///
/// Similar to Flutter's `TextAffinity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAffinity {
    /// The position has affinity for the upstream side of the text position.
    ///
    /// This means the cursor prefers to position itself at the end of the
    /// previous line rather than the start of the next line when the offset
    /// is at a line break.
    Upstream,
    
    /// The position has affinity for the downstream side of the text position.
    ///
    /// This means the cursor prefers to position itself at the start of the
    /// next line rather than the end of the previous line when the offset
    /// is at a line break.
    #[default]
    Downstream,
}

/// Configuration for text scaling.
///
/// Similar to Flutter's `TextScaler` but simplified.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextScaler {
    /// The scale factor to apply to text.
    pub scale_factor: f32,
}

impl TextScaler {
    /// Create a new text scaler with the given scale factor.
    pub fn new(scale_factor: f32) -> Self {
        Self { scale_factor }
    }
    
    /// No scaling (scale factor of 1.0).
    pub const fn none() -> Self {
        Self { scale_factor: 1.0 }
    }
    
    /// Scale the given font size.
    pub fn scale(&self, font_size: f32) -> f32 {
        font_size * self.scale_factor
    }
}

impl Default for TextScaler {
    fn default() -> Self {
        Self::none()
    }
}

/// A range of characters in a string of text.
///
/// Similar to Flutter's `TextRange`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextRange {
    /// The start offset of the range (inclusive).
    pub start: usize,
    
    /// The end offset of the range (exclusive).
    pub end: usize,
}

impl TextRange {
    /// Create a new text range.
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
    
    /// Create a collapsed range (single position).
    pub const fn collapsed(offset: usize) -> Self {
        Self {
            start: offset,
            end: offset,
        }
    }
    
    /// Check if the range is valid (start <= end).
    pub fn is_valid(&self) -> bool {
        self.start <= self.end
    }
    
    /// Check if the range is collapsed (start == end).
    pub fn is_collapsed(&self) -> bool {
        self.start == self.end
    }
    
    /// Get the length of the range.
    pub fn length(&self) -> usize {
        if self.is_valid() {
            self.end - self.start
        } else {
            0
        }
    }
    
    /// Check if the range contains the given offset.
    pub fn contains(&self, offset: usize) -> bool {
        self.is_valid() && offset >= self.start && offset < self.end
    }
    
    /// Check if the range contains the given range.
    pub fn contains_range(&self, other: &TextRange) -> bool {
        self.is_valid() && other.is_valid() && 
        self.start <= other.start && self.end >= other.end
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_text_overflow_properties() {
        assert!(!TextOverflow::Clip.requires_custom_painting());
        assert!(TextOverflow::Ellipsis.requires_custom_painting());
        assert!(TextOverflow::Fade.requires_custom_painting());
        assert!(!TextOverflow::Visible.requires_custom_painting());
        
        assert!(TextOverflow::Clip.requires_clipping());
        assert!(TextOverflow::Ellipsis.requires_clipping());
        assert!(TextOverflow::Fade.requires_clipping());
        assert!(!TextOverflow::Visible.requires_clipping());
    }
    
    #[test]
    fn test_text_align_resolution() {
        assert_eq!(
            TextAlign::Start.resolve(TextDirection::Ltr),
            TextAlign::Left
        );
        assert_eq!(
            TextAlign::Start.resolve(TextDirection::Rtl),
            TextAlign::Right
        );
        assert_eq!(
            TextAlign::End.resolve(TextDirection::Ltr),
            TextAlign::Right
        );
        assert_eq!(
            TextAlign::End.resolve(TextDirection::Rtl),
            TextAlign::Left
        );
        
        // Other alignments should remain unchanged
        assert_eq!(
            TextAlign::Center.resolve(TextDirection::Ltr),
            TextAlign::Center
        );
        assert_eq!(
            TextAlign::Justify.resolve(TextDirection::Rtl),
            TextAlign::Justify
        );
    }
    
    #[test]
    fn test_text_align_requires_custom_layout() {
        assert!(!TextAlign::Left.requires_custom_layout());
        assert!(!TextAlign::Right.requires_custom_layout());
        assert!(!TextAlign::Center.requires_custom_layout());
        assert!(TextAlign::Justify.requires_custom_layout());
        assert!(!TextAlign::Start.requires_custom_layout());
        assert!(!TextAlign::End.requires_custom_layout());
    }
    
    #[test]
    fn test_text_direction_utilities() {
        assert!(TextDirection::Ltr.is_ltr());
        assert!(!TextDirection::Ltr.is_rtl());
        assert!(TextDirection::Rtl.is_rtl());
        assert!(!TextDirection::Rtl.is_ltr());
        
        assert_eq!(TextDirection::Ltr.opposite(), TextDirection::Rtl);
        assert_eq!(TextDirection::Rtl.opposite(), TextDirection::Ltr);
    }
    
    #[test]
    fn test_text_selection_utilities() {
        let selection = TextSelection::new(5, 10);
        assert!(!selection.is_collapsed());
        assert_eq!(selection.start(), 5);
        assert_eq!(selection.end(), 10);
        assert_eq!(selection.length(), 5);
        
        let collapsed = TextSelection::collapsed(7);
        assert!(collapsed.is_collapsed());
        assert_eq!(collapsed.start(), 7);
        assert_eq!(collapsed.end(), 7);
        assert_eq!(collapsed.length(), 0);
        
        // Test with reversed offsets
        let reversed = TextSelection::new(10, 5);
        assert_eq!(reversed.start(), 5);
        assert_eq!(reversed.end(), 10);
        assert_eq!(reversed.length(), 5);
    }
    
    #[test]
    fn test_text_scaler() {
        let scaler = TextScaler::new(1.5);
        assert_eq!(scaler.scale(12.0), 18.0);
        assert_eq!(scaler.scale(16.0), 24.0);
        
        let none = TextScaler::none();
        assert_eq!(none.scale(12.0), 12.0);
    }
    
    #[test]
    fn test_text_range_utilities() {
        let range = TextRange::new(5, 10);
        assert!(range.is_valid());
        assert!(!range.is_collapsed());
        assert_eq!(range.length(), 5);
        assert!(range.contains(7));
        assert!(!range.contains(3));
        assert!(!range.contains(10)); // end is exclusive
        assert!(!range.contains(15));
        
        let collapsed = TextRange::collapsed(7);
        assert!(collapsed.is_valid());
        assert!(collapsed.is_collapsed());
        assert_eq!(collapsed.length(), 0);
        assert!(!collapsed.contains(7)); // collapsed range doesn't contain any offset
        
        let invalid = TextRange::new(10, 5);
        assert!(!invalid.is_valid());
        assert_eq!(invalid.length(), 0);
        
        // Test range containment
        let outer = TextRange::new(0, 20);
        let inner = TextRange::new(5, 15);
        assert!(outer.contains_range(&inner));
        assert!(!inner.contains_range(&outer));
    }
    
    #[test]
    fn test_text_height_behavior() {
        let all = TextHeightBehavior::all();
        assert!(all.apply_height_to_first_ascent);
        assert!(all.apply_height_to_last_descent);
        
        let none = TextHeightBehavior::none();
        assert!(!none.apply_height_to_first_ascent);
        assert!(!none.apply_height_to_last_descent);
        
        let default = TextHeightBehavior::default();
        assert!(!default.apply_height_to_first_ascent);
        assert!(!default.apply_height_to_last_descent);
    }
}