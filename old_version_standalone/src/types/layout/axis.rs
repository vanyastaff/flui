//! Axis and direction types for layout systems
//!
//! This module contains types for representing axes, directions, and orientation,
//! similar to Flutter's axis system but adapted for egui.

/// The two cardinal directions in two dimensions.
///
/// Similar to Flutter's `Axis`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Axis {
    /// The horizontal axis (left to right).
    #[default]
    Horizontal,
    
    /// The vertical axis (top to bottom).
    Vertical,
}

impl Axis {
    /// Get the opposite axis.
    pub fn opposite(self) -> Self {
        match self {
            Axis::Horizontal => Axis::Vertical,
            Axis::Vertical => Axis::Horizontal,
        }
    }
    
    /// Check if this is the horizontal axis.
    pub fn is_horizontal(self) -> bool {
        matches!(self, Axis::Horizontal)
    }
    
    /// Check if this is the vertical axis.
    pub fn is_vertical(self) -> bool {
        matches!(self, Axis::Vertical)
    }
    
    /// Extract the coordinate from a vector based on this axis.
    pub fn select(self, vector: egui::Vec2) -> f32 {
        match self {
            Axis::Horizontal => vector.x,
            Axis::Vertical => vector.y,
        }
    }
    
    /// Set the coordinate in a vector based on this axis.
    pub fn set(self, vector: &mut egui::Vec2, value: f32) {
        match self {
            Axis::Horizontal => vector.x = value,
            Axis::Vertical => vector.y = value,
        }
    }
    
    /// Create a vector with the given value on this axis and zero on the other.
    pub fn vector(self, value: f32) -> egui::Vec2 {
        match self {
            Axis::Horizontal => egui::Vec2::new(value, 0.0),
            Axis::Vertical => egui::Vec2::new(0.0, value),
        }
    }
    
    /// Get the size component from a size based on this axis.
    pub fn size_component(self, size: egui::Vec2) -> f32 {
        match self {
            Axis::Horizontal => size.x,
            Axis::Vertical => size.y,
        }
    }
    
    /// Create a size with the given value on this axis and zero on the other.
    pub fn size(self, value: f32) -> egui::Vec2 {
        match self {
            Axis::Horizontal => egui::Vec2::new(value, 0.0),
            Axis::Vertical => egui::Vec2::new(0.0, value),
        }
    }
    
}

/// A direction along either the horizontal or vertical axis.
///
/// Similar to Flutter's `AxisDirection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AxisDirection {
    /// From left to right.
    LeftToRight,
    
    /// From right to left.
    RightToLeft,
    
    /// From top to bottom.
    TopToBottom,
    
    /// From bottom to top.
    BottomToTop,
}

impl AxisDirection {
    /// Get the axis for this direction.
    pub fn axis(self) -> Axis {
        match self {
            AxisDirection::LeftToRight | AxisDirection::RightToLeft => Axis::Horizontal,
            AxisDirection::TopToBottom | AxisDirection::BottomToTop => Axis::Vertical,
        }
    }
    
    /// Get the opposite direction.
    pub fn opposite(self) -> Self {
        match self {
            AxisDirection::LeftToRight => AxisDirection::RightToLeft,
            AxisDirection::RightToLeft => AxisDirection::LeftToRight,
            AxisDirection::TopToBottom => AxisDirection::BottomToTop,
            AxisDirection::BottomToTop => AxisDirection::TopToBottom,
        }
    }
    
    /// Check if this direction is positive (left-to-right or top-to-bottom).
    pub fn is_positive(self) -> bool {
        matches!(self, AxisDirection::LeftToRight | AxisDirection::TopToBottom)
    }
    
    /// Check if this direction is negative (right-to-left or bottom-to-top).
    pub fn is_negative(self) -> bool {
        !self.is_positive()
    }
    
    /// Check if this direction is reversed relative to the natural reading direction.
    pub fn is_reversed(self) -> bool {
        matches!(self, AxisDirection::RightToLeft | AxisDirection::BottomToTop)
    }
    
    /// Convert to a sign multiplier (1.0 for positive, -1.0 for negative).
    pub fn sign(self) -> f32 {
        if self.is_positive() { 1.0 } else { -1.0 }
    }

    /// Create from an axis and whether it's reversed.
    pub fn from_axis(axis: Axis, reversed: bool) -> Self {
        match (axis, reversed) {
            (Axis::Horizontal, false) => AxisDirection::LeftToRight,
            (Axis::Horizontal, true) => AxisDirection::RightToLeft,
            (Axis::Vertical, false) => AxisDirection::TopToBottom,
            (Axis::Vertical, true) => AxisDirection::BottomToTop,
        }
    }
}

impl Default for AxisDirection {
    fn default() -> Self {
        AxisDirection::LeftToRight
    }
}

/// Whether in portrait or landscape orientation.
///
/// Similar to Flutter's `Orientation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Orientation {
    /// Portrait orientation (height > width).
    #[default]
    Portrait,
    
    /// Landscape orientation (width > height).
    Landscape,
}

impl Orientation {
    /// Determine orientation from a size.
    pub fn from_size(size: egui::Vec2) -> Self {
        if size.y > size.x {
            Orientation::Portrait
        } else {
            Orientation::Landscape
        }
    }
    
    /// Get the main axis for this orientation.
    pub fn main_axis(self) -> Axis {
        match self {
            Orientation::Portrait => Axis::Vertical,
            Orientation::Landscape => Axis::Horizontal,
        }
    }
    
    /// Get the cross axis for this orientation.
    pub fn cross_axis(self) -> Axis {
        self.main_axis().opposite()
    }
    
    /// Check if this is portrait orientation.
    pub fn is_portrait(self) -> bool {
        matches!(self, Orientation::Portrait)
    }
    
    /// Check if this is landscape orientation.
    pub fn is_landscape(self) -> bool {
        matches!(self, Orientation::Landscape)
    }
}

/// The direction in which boxes flow vertically.
///
/// Similar to Flutter's `VerticalDirection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VerticalDirection {
    /// Boxes flow from top to bottom.
    #[default]
    Down,
    
    /// Boxes flow from bottom to top.
    Up,
}

impl VerticalDirection {
    /// Get the axis direction for this vertical direction.
    pub fn to_axis_direction(self) -> AxisDirection {
        match self {
            VerticalDirection::Down => AxisDirection::TopToBottom,
            VerticalDirection::Up => AxisDirection::BottomToTop,
        }
    }
    
    /// Check if this direction is down (top to bottom).
    pub fn is_down(self) -> bool {
        matches!(self, VerticalDirection::Down)
    }
    
    /// Check if this direction is up (bottom to top).
    pub fn is_up(self) -> bool {
        matches!(self, VerticalDirection::Up)
    }
    
    /// Get the opposite direction.
    pub fn opposite(self) -> Self {
        match self {
            VerticalDirection::Down => VerticalDirection::Up,
            VerticalDirection::Up => VerticalDirection::Down,
        }
    }
}

/// Utility functions for working with axes and directions.
#[derive(Debug, Clone, Copy)]
pub struct AxisUtils;

impl AxisUtils {
    /// Flip a size based on the given axis.
    pub fn flip_size(axis: Axis, size: egui::Vec2) -> egui::Vec2 {
        match axis {
            Axis::Horizontal => size,
            Axis::Vertical => egui::Vec2::new(size.y, size.x),
        }
    }
    
    /// Get the main size for a given axis.
    pub fn main_size(axis: Axis, size: egui::Vec2) -> f32 {
        axis.size_component(size)
    }
    
    /// Get the cross size for a given axis.
    pub fn cross_size(axis: Axis, size: egui::Vec2) -> f32 {
        axis.opposite().size_component(size)
    }
    
    /// Create a size with the given main and cross values.
    pub fn make_size(axis: Axis, main: f32, cross: f32) -> egui::Vec2 {
        match axis {
            Axis::Horizontal => egui::Vec2::new(main, cross),
            Axis::Vertical => egui::Vec2::new(cross, main),
        }
    }
    
    /// Calculate the scroll delta for a given axis and direction.
    pub fn scroll_delta(axis: Axis, delta: f32, direction: AxisDirection) -> egui::Vec2 {
        let sign = direction.sign();
        match axis {
            Axis::Horizontal => egui::Vec2::new(delta * sign, 0.0),
            Axis::Vertical => egui::Vec2::new(0.0, delta * sign),
        }
    }
    
    /// Convert a vector from one axis direction to another.
    pub fn convert_vector(
        vector: egui::Vec2,
        from_direction: AxisDirection,
        to_direction: AxisDirection,
    ) -> egui::Vec2 {
        if from_direction.axis() != to_direction.axis() {
            // Cross-axis conversion - flip coordinates
            egui::Vec2::new(vector.y, vector.x)
        } else if from_direction != to_direction {
            // Same axis but reversed - negate the axis component
            let mut result = vector;
            from_direction.axis().set(&mut result, -from_direction.axis().select(vector));
            result
        } else {
            // Same direction - no change
            vector
        }
    }
}

/// Extension trait for working with vectors in specific axes.
pub trait Vec2AxisExt {
    /// Get the component along the given axis.
    fn axis_component(self, axis: Axis) -> f32;
    
    /// Set the component along the given axis.
    fn with_axis_component(self, axis: Axis, value: f32) -> Self;
    
    /// Get the main component for layout in the given axis.
    fn main_component(self, axis: Axis) -> f32;
    
    /// Get the cross component for layout in the given axis.
    fn cross_component(self, axis: Axis) -> f32;
    
    /// Create a vector with the given main and cross components.
    fn with_main_cross(axis: Axis, main: f32, cross: f32) -> Self;
}

impl Vec2AxisExt for egui::Vec2 {
    fn axis_component(self, axis: Axis) -> f32 {
        axis.select(self)
    }
    
    fn with_axis_component(self, axis: Axis, value: f32) -> Self {
        let mut result = self;
        axis.set(&mut result, value);
        result
    }
    
    fn main_component(self, axis: Axis) -> f32 {
        self.axis_component(axis)
    }
    
    fn cross_component(self, axis: Axis) -> f32 {
        self.axis_component(axis.opposite())
    }
    
    fn with_main_cross(axis: Axis, main: f32, cross: f32) -> Self {
        match axis {
            Axis::Horizontal => egui::Vec2::new(main, cross),
            Axis::Vertical => egui::Vec2::new(cross, main),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_axis_operations() {
        let horizontal = Axis::Horizontal;
        let vertical = Axis::Vertical;
        
        assert!(horizontal.is_horizontal());
        assert!(!horizontal.is_vertical());
        assert!(vertical.is_vertical());
        assert!(!vertical.is_horizontal());
        
        assert_eq!(horizontal.opposite(), Axis::Vertical);
        assert_eq!(vertical.opposite(), Axis::Horizontal);
        
        let vector = egui::Vec2::new(10.0, 20.0);
        assert_eq!(horizontal.select(vector), 10.0);
        assert_eq!(vertical.select(vector), 20.0);
        
        let mut vec = egui::Vec2::new(1.0, 2.0);
        horizontal.set(&mut vec, 5.0);
        assert_eq!(vec, egui::Vec2::new(5.0, 2.0));
        
        assert_eq!(horizontal.vector(3.0), egui::Vec2::new(3.0, 0.0));
        assert_eq!(vertical.vector(3.0), egui::Vec2::new(0.0, 3.0));
        
        let size = egui::Vec2::new(100.0, 50.0);
        assert_eq!(horizontal.size_component(size), 100.0);
        assert_eq!(vertical.size_component(size), 50.0);
    }
    
    #[test]
    fn test_axis_direction_operations() {
        let ltr = AxisDirection::LeftToRight;
        let rtl = AxisDirection::RightToLeft;
        let ttb = AxisDirection::TopToBottom;
        let btt = AxisDirection::BottomToTop;
        
        assert_eq!(ltr.axis(), Axis::Horizontal);
        assert_eq!(rtl.axis(), Axis::Horizontal);
        assert_eq!(ttb.axis(), Axis::Vertical);
        assert_eq!(btt.axis(), Axis::Vertical);
        
        assert!(ltr.is_positive());
        assert!(!rtl.is_positive());
        assert!(ttb.is_positive());
        assert!(!btt.is_positive());
        
        assert!(!ltr.is_negative());
        assert!(rtl.is_negative());
        assert!(!ttb.is_negative());
        assert!(btt.is_negative());
        
        assert!(!ltr.is_reversed());
        assert!(rtl.is_reversed());
        assert!(!ttb.is_reversed());
        assert!(btt.is_reversed());
        
        assert_eq!(ltr.sign(), 1.0);
        assert_eq!(rtl.sign(), -1.0);
        assert_eq!(ttb.sign(), 1.0);
        assert_eq!(btt.sign(), -1.0);
        
        assert_eq!(ltr.opposite(), rtl);
        assert_eq!(rtl.opposite(), ltr);
        assert_eq!(ttb.opposite(), btt);
        assert_eq!(btt.opposite(), ttb);
        
        assert_eq!(AxisDirection::from_axis(Axis::Horizontal, false), ltr);
        assert_eq!(AxisDirection::from_axis(Axis::Horizontal, true), rtl);
        assert_eq!(AxisDirection::from_axis(Axis::Vertical, false), ttb);
        assert_eq!(AxisDirection::from_axis(Axis::Vertical, true), btt);
    }
    
    #[test]
    fn test_orientation_operations() {
        let portrait = Orientation::Portrait;
        let landscape = Orientation::Landscape;
        
        assert!(portrait.is_portrait());
        assert!(!portrait.is_landscape());
        assert!(landscape.is_landscape());
        assert!(!landscape.is_portrait());
        
        assert_eq!(portrait.main_axis(), Axis::Vertical);
        assert_eq!(portrait.cross_axis(), Axis::Horizontal);
        assert_eq!(landscape.main_axis(), Axis::Horizontal);
        assert_eq!(landscape.cross_axis(), Axis::Vertical);
        
        assert_eq!(Orientation::from_size(egui::Vec2::new(100.0, 200.0)), Orientation::Portrait);
        assert_eq!(Orientation::from_size(egui::Vec2::new(200.0, 100.0)), Orientation::Landscape);
        assert_eq!(Orientation::from_size(egui::Vec2::new(100.0, 100.0)), Orientation::Landscape); // tie goes to landscape
    }
    
    #[test]
    fn test_vertical_direction_operations() {
        let down = VerticalDirection::Down;
        let up = VerticalDirection::Up;
        
        assert!(down.is_down());
        assert!(!down.is_up());
        assert!(up.is_up());
        assert!(!up.is_down());
        
        assert_eq!(down.to_axis_direction(), AxisDirection::TopToBottom);
        assert_eq!(up.to_axis_direction(), AxisDirection::BottomToTop);
        
        assert_eq!(down.opposite(), up);
        assert_eq!(up.opposite(), down);
    }
    
    #[test]
    fn test_axis_utils() {
        let size = egui::Vec2::new(100.0, 50.0);
        
        assert_eq!(AxisUtils::flip_size(Axis::Horizontal, size), egui::Vec2::new(100.0, 50.0));
        assert_eq!(AxisUtils::flip_size(Axis::Vertical, size), egui::Vec2::new(50.0, 100.0));
        
        assert_eq!(AxisUtils::main_size(Axis::Horizontal, size), 100.0);
        assert_eq!(AxisUtils::main_size(Axis::Vertical, size), 50.0);
        
        assert_eq!(AxisUtils::cross_size(Axis::Horizontal, size), 50.0);
        assert_eq!(AxisUtils::cross_size(Axis::Vertical, size), 100.0);
        
        assert_eq!(AxisUtils::make_size(Axis::Horizontal, 100.0, 50.0), egui::Vec2::new(100.0, 50.0));
        assert_eq!(AxisUtils::make_size(Axis::Vertical, 100.0, 50.0), egui::Vec2::new(50.0, 100.0));
        
        assert_eq!(
            AxisUtils::scroll_delta(Axis::Horizontal, 10.0, AxisDirection::LeftToRight),
            egui::Vec2::new(10.0, 0.0)
        );
        assert_eq!(
            AxisUtils::scroll_delta(Axis::Horizontal, 10.0, AxisDirection::RightToLeft),
            egui::Vec2::new(-10.0, 0.0)
        );
        
        let vector = egui::Vec2::new(10.0, 20.0);
        assert_eq!(
            AxisUtils::convert_vector(vector, AxisDirection::LeftToRight, AxisDirection::TopToBottom),
            egui::Vec2::new(20.0, 10.0) // Cross-axis conversion flips coordinates
        );
        assert_eq!(
            AxisUtils::convert_vector(vector, AxisDirection::LeftToRight, AxisDirection::RightToLeft),
            egui::Vec2::new(-10.0, 20.0) // Same axis but reversed negates x
        );
    }
    
    #[test]
    fn test_vec2_axis_ext() {
        let vec = egui::Vec2::new(10.0, 20.0);
        
        assert_eq!(vec.axis_component(Axis::Horizontal), 10.0);
        assert_eq!(vec.axis_component(Axis::Vertical), 20.0);
        
        assert_eq!(vec.with_axis_component(Axis::Horizontal, 5.0), egui::Vec2::new(5.0, 20.0));
        assert_eq!(vec.with_axis_component(Axis::Vertical, 5.0), egui::Vec2::new(10.0, 5.0));
        
        assert_eq!(vec.main_component(Axis::Horizontal), 10.0);
        assert_eq!(vec.main_component(Axis::Vertical), 20.0);
        
        assert_eq!(vec.cross_component(Axis::Horizontal), 20.0);
        assert_eq!(vec.cross_component(Axis::Vertical), 10.0);
        
        assert_eq!(
            egui::Vec2::with_main_cross(Axis::Horizontal, 100.0, 50.0),
            egui::Vec2::new(100.0, 50.0)
        );
        assert_eq!(
            egui::Vec2::with_main_cross(Axis::Vertical, 100.0, 50.0),
            egui::Vec2::new(50.0, 100.0)
        );
    }
}