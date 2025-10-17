//! Alignment types for layout widgets
//!
//! This module contains enums and utilities for aligning children
//! within parent containers, similar to Flutter's alignment system.

use egui::Align;
use crate::types::typography::text::TextDirection;

/// How much space should be occupied in the main axis.
///
/// Similar to Flutter's `MainAxisSize`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MainAxisSize {
    /// Minimize the amount of space occupied by the children.
    Min,

    /// Maximize the amount of space occupied by the children.
    #[default]
    Max,
}

/// How the children should be placed along the main axis in a flex layout.
///
/// This is similar to CSS `justify-content` property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MainAxisAlignment {
    /// Place children at the start of the main axis.
    #[default]
    Start,
    
    /// Place children at the end of the main axis.
    End,
    
    /// Place children in the center of the main axis.
    Center,
    
    /// Place children with equal space between them.
    ///
    /// The first child is at the start, the last child is at the end,
    /// and the remaining space is distributed evenly between children.
    SpaceBetween,
    
    /// Place children with equal space around them.
    ///
    /// Each child has equal space on both sides, resulting in
    /// half-sized space at the start and end.
    SpaceAround,
    
    /// Place children with equal space around them, including start and end.
    ///
    /// All children have equal space between them and at the start/end.
    SpaceEvenly,
}

impl MainAxisAlignment {
    /// Convert to egui's alignment for horizontal layouts
    pub fn to_egui_horizontal(self) -> Align {
        match self {
            MainAxisAlignment::Start => Align::LEFT,
            MainAxisAlignment::End => Align::RIGHT,
            MainAxisAlignment::Center => Align::Center,
            MainAxisAlignment::SpaceBetween => Align::LEFT, // Handled separately
            MainAxisAlignment::SpaceAround => Align::LEFT,  // Handled separately
            MainAxisAlignment::SpaceEvenly => Align::LEFT,  // Handled separately
        }
    }
    
    /// Convert to egui's alignment for vertical layouts
    pub fn to_egui_vertical(self) -> Align {
        match self {
            MainAxisAlignment::Start => Align::TOP,
            MainAxisAlignment::End => Align::BOTTOM,
            MainAxisAlignment::Center => Align::Center,
            MainAxisAlignment::SpaceBetween => Align::TOP, // Handled separately
            MainAxisAlignment::SpaceAround => Align::TOP,  // Handled separately
            MainAxisAlignment::SpaceEvenly => Align::TOP,  // Handled separately
        }
    }
    
    /// Check if this alignment requires custom spacing logic
    pub fn requires_custom_spacing(self) -> bool {
        matches!(
            self,
            MainAxisAlignment::SpaceBetween |
            MainAxisAlignment::SpaceAround |
            MainAxisAlignment::SpaceEvenly
        )
    }
}

/// How the children should be placed along the cross axis in a flex layout.
///
/// This is similar to CSS `align-items` property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrossAxisAlignment {
    /// Place children at the start of the cross axis.
    #[default]
    Start,
    
    /// Place children at the end of the cross axis.
    End,
    
    /// Place children in the center of the cross axis.
    Center,
    
    /// Stretch children to fill the cross axis.
    Stretch,
    
    /// Place children along the cross axis such that their baselines match.
    ///
    /// This only applies to text and requires baseline information.
    Baseline,
}

impl CrossAxisAlignment {
    /// Convert to egui's alignment for horizontal layouts
    pub fn to_egui_horizontal(self) -> Align {
        match self {
            CrossAxisAlignment::Start => Align::TOP,
            CrossAxisAlignment::End => Align::BOTTOM,
            CrossAxisAlignment::Center => Align::Center,
            CrossAxisAlignment::Stretch => Align::Center, // Handled separately
            CrossAxisAlignment::Baseline => Align::TOP,   // Handled separately
        }
    }
    
    /// Convert to egui's alignment for vertical layouts
    pub fn to_egui_vertical(self) -> Align {
        match self {
            CrossAxisAlignment::Start => Align::LEFT,
            CrossAxisAlignment::End => Align::RIGHT,
            CrossAxisAlignment::Center => Align::Center,
            CrossAxisAlignment::Stretch => Align::Center, // Handled separately
            CrossAxisAlignment::Baseline => Align::LEFT,  // Handled separately
        }
    }
    
    /// Check if this alignment requires custom sizing logic
    pub fn requires_custom_sizing(self) -> bool {
        matches!(
            self,
            CrossAxisAlignment::Stretch |
            CrossAxisAlignment::Baseline
        )
    }
}

/// How to align a child within its parent container.
///
/// This is similar to Flutter's `Alignment` class but simplified.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Alignment {
    /// Horizontal alignment: -1.0 = left, 0.0 = center, 1.0 = right
    pub x: f32,
    
    /// Vertical alignment: -1.0 = top, 0.0 = center, 1.0 = bottom
    pub y: f32,
}

impl Alignment {
    /// Create a new alignment with the given x and y values.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
    
    /// Top left alignment.
    pub const TOP_LEFT: Self = Self::new(-1.0, -1.0);
    
    /// Top center alignment.
    pub const TOP_CENTER: Self = Self::new(0.0, -1.0);
    
    /// Top right alignment.
    pub const TOP_RIGHT: Self = Self::new(1.0, -1.0);
    
    /// Center left alignment.
    pub const CENTER_LEFT: Self = Self::new(-1.0, 0.0);
    
    /// Center alignment.
    pub const CENTER: Self = Self::new(0.0, 0.0);
    
    /// Center right alignment.
    pub const CENTER_RIGHT: Self = Self::new(1.0, 0.0);
    
    /// Bottom left alignment.
    pub const BOTTOM_LEFT: Self = Self::new(-1.0, 1.0);
    
    /// Bottom center alignment.
    pub const BOTTOM_CENTER: Self = Self::new(0.0, 1.0);
    
    /// Bottom right alignment.
    pub const BOTTOM_RIGHT: Self = Self::new(1.0, 1.0);
    
    /// Convert to egui's align2
    pub fn to_egui_align2(self) -> egui::Align2 {
        let horizontal = match self.x {
            x if x < -0.5 => egui::Align::LEFT,
            x if x > 0.5 => egui::Align::RIGHT,
            _ => egui::Align::Center,
        };
        
        let vertical = match self.y {
            y if y < -0.5 => egui::Align::TOP,
            y if y > 0.5 => egui::Align::BOTTOM,
            _ => egui::Align::Center,
        };
        
        egui::Align2([horizontal, vertical])
    }
    
    /// Calculate the offset for a child of given size within a parent of given size.
    pub fn calculate_offset(self, child_size: egui::Vec2, parent_size: egui::Vec2) -> egui::Vec2 {
        let available_space = parent_size - child_size;
        egui::vec2(
            available_space.x * (self.x + 1.0) / 2.0,
            available_space.y * (self.y + 1.0) / 2.0,
        )
    }
}

impl Default for Alignment {
    fn default() -> Self {
        Self::CENTER
    }
}

/// Convert Alignment to egui::Align (horizontal alignment).
///
/// This uses the x-axis of the alignment to determine horizontal alignment.
/// -1.0 = Min (Left), 0.0 = Center, 1.0 = Max (Right)
impl From<Alignment> for egui::Align {
    fn from(alignment: Alignment) -> Self {
        if alignment.x < -0.33 {
            egui::Align::Min // Left
        } else if alignment.x > 0.33 {
            egui::Align::Max // Right
        } else {
            egui::Align::Center // Center
        }
    }
}

/// Text direction aware alignment.
///
/// Similar to Flutter's `AlignmentDirectional` for handling RTL/LTR layouts.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AlignmentDirectional {
    /// Start alignment: depends on text direction
    pub start: f32,
    
    /// Vertical alignment: -1.0 = top, 0.0 = center, 1.0 = bottom
    pub y: f32,
}

impl AlignmentDirectional {
    /// Create a new directional alignment.
    pub const fn new(start: f32, y: f32) -> Self {
        Self { start, y }
    }
    
    /// Top start alignment.
    pub const TOP_START: Self = Self::new(-1.0, -1.0);
    
    /// Top center alignment.
    pub const TOP_CENTER: Self = Self::new(0.0, -1.0);
    
    /// Top end alignment.
    pub const TOP_END: Self = Self::new(1.0, -1.0);
    
    /// Center start alignment.
    pub const CENTER_START: Self = Self::new(-1.0, 0.0);
    
    /// Center alignment.
    pub const CENTER: Self = Self::new(0.0, 0.0);
    
    /// Center end alignment.
    pub const CENTER_END: Self = Self::new(1.0, 0.0);
    
    /// Bottom start alignment.
    pub const BOTTOM_START: Self = Self::new(-1.0, 1.0);
    
    /// Bottom center alignment.
    pub const BOTTOM_CENTER: Self = Self::new(0.0, 1.0);
    
    /// Bottom end alignment.
    pub const BOTTOM_END: Self = Self::new(1.0, 1.0);
    
    /// Convert to regular alignment based on text direction.
    pub fn resolve(self, text_direction: TextDirection) -> Alignment {
        let x = match text_direction {
            TextDirection::Ltr => self.start,
            TextDirection::Rtl => -self.start,
        };
        Alignment::new(x, self.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_main_axis_alignment_conversion() {
        assert_eq!(MainAxisAlignment::Start.to_egui_horizontal(), Align::LEFT);
        assert_eq!(MainAxisAlignment::End.to_egui_horizontal(), Align::RIGHT);
        assert_eq!(MainAxisAlignment::Center.to_egui_horizontal(), Align::Center);
        
        assert_eq!(MainAxisAlignment::Start.to_egui_vertical(), Align::TOP);
        assert_eq!(MainAxisAlignment::End.to_egui_vertical(), Align::BOTTOM);
        assert_eq!(MainAxisAlignment::Center.to_egui_vertical(), Align::Center);
    }
    
    #[test]
    fn test_cross_axis_alignment_conversion() {
        assert_eq!(CrossAxisAlignment::Start.to_egui_vertical(), Align::LEFT);
        assert_eq!(CrossAxisAlignment::End.to_egui_vertical(), Align::RIGHT);
        assert_eq!(CrossAxisAlignment::Center.to_egui_vertical(), Align::Center);
    }
    
    #[test]
    fn test_alignment_calculate_offset() {
        let alignment = Alignment::CENTER;
        let child_size = egui::vec2(50.0, 30.0);
        let parent_size = egui::vec2(100.0, 60.0);
        
        let offset = alignment.calculate_offset(child_size, parent_size);
        assert_eq!(offset, egui::vec2(25.0, 15.0));
        
        let top_left = Alignment::TOP_LEFT;
        let offset = top_left.calculate_offset(child_size, parent_size);
        assert_eq!(offset, egui::vec2(0.0, 0.0));
        
        let bottom_right = Alignment::BOTTOM_RIGHT;
        let offset = bottom_right.calculate_offset(child_size, parent_size);
        assert_eq!(offset, egui::vec2(50.0, 30.0));
    }
    
    #[test]
    fn test_alignment_directional_resolve() {
        let directional = AlignmentDirectional::TOP_START;
        
        let ltr = directional.resolve(TextDirection::Ltr);
        assert_eq!(ltr, Alignment::TOP_LEFT);
        
        let rtl = directional.resolve(TextDirection::Rtl);
        assert_eq!(rtl, Alignment::TOP_RIGHT);
    }
    
    #[test]
    fn test_custom_spacing_detection() {
        assert!(!MainAxisAlignment::Start.requires_custom_spacing());
        assert!(!MainAxisAlignment::End.requires_custom_spacing());
        assert!(!MainAxisAlignment::Center.requires_custom_spacing());
        assert!(MainAxisAlignment::SpaceBetween.requires_custom_spacing());
        assert!(MainAxisAlignment::SpaceAround.requires_custom_spacing());
        assert!(MainAxisAlignment::SpaceEvenly.requires_custom_spacing());
    }
    
    #[test]
    fn test_custom_sizing_detection() {
        assert!(!CrossAxisAlignment::Start.requires_custom_sizing());
        assert!(!CrossAxisAlignment::End.requires_custom_sizing());
        assert!(!CrossAxisAlignment::Center.requires_custom_sizing());
        assert!(CrossAxisAlignment::Stretch.requires_custom_sizing());
        assert!(CrossAxisAlignment::Baseline.requires_custom_sizing());
    }
}