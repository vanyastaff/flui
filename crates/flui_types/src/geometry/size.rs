//! Size type for 2D dimensions

use std::fmt;

/// A 2D size with width and height
///
/// Similar to Flutter's Size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    /// Width in logical pixels
    pub width: f32,
    /// Height in logical pixels
    pub height: f32,
}

impl Size {
    /// Create a new size
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    /// Create a size with both dimensions set to zero
    pub const fn zero() -> Self {
        Self::new(0.0, 0.0)
    }

    /// Create a size with infinite dimensions
    pub fn infinite() -> Self {
        Self::new(f32::INFINITY, f32::INFINITY)
    }

    /// Check if this size is zero
    pub fn is_zero(&self) -> bool {
        self.width == 0.0 && self.height == 0.0
    }

    /// Check if this size has finite dimensions
    pub fn is_finite(&self) -> bool {
        self.width.is_finite() && self.height.is_finite()
    }

    /// Check if this size is empty (width or height is zero)
    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    /// Get the aspect ratio (width / height)
    pub fn aspect_ratio(&self) -> f32 {
        if self.height == 0.0 {
            0.0
        } else {
            self.width / self.height
        }
    }

    /// Calculate the shortest side
    pub fn shortest_side(&self) -> f32 {
        self.width.min(self.height)
    }

    /// Calculate the longest side
    pub fn longest_side(&self) -> f32 {
        self.width.max(self.height)
    }

    /// Get the area (width * height)
    pub fn area(&self) -> f32 {
        self.width * self.height
    }
}

impl Default for Size {
    fn default() -> Self {
        Self::zero()
    }
}

impl From<(f32, f32)> for Size {
    fn from((width, height): (f32, f32)) -> Self {
        Self::new(width, height)
    }
}

impl From<[f32; 2]> for Size {
    fn from([width, height]: [f32; 2]) -> Self {
        Self::new(width, height)
    }
}

impl From<Size> for (f32, f32) {
    fn from(size: Size) -> Self {
        (size.width, size.height)
    }
}

impl From<Size> for [f32; 2] {
    fn from(size: Size) -> Self {
        [size.width, size.height]
    }
}

impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Size({}x{})", self.width, self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_size_zero() {
        let size = Size::zero();
        assert_eq!(size.width, 0.0);
        assert_eq!(size.height, 0.0);
        assert!(size.is_zero());
    }

    #[test]
    fn test_size_finite() {
        let size = Size::new(100.0, 50.0);
        assert!(size.is_finite());

        let infinite = Size::infinite();
        assert!(!infinite.is_finite());
    }

    #[test]
    fn test_size_aspect_ratio() {
        let size = Size::new(100.0, 50.0);
        assert_eq!(size.aspect_ratio(), 2.0);
    }

    #[test]
    fn test_size_shortest_longest() {
        let size = Size::new(100.0, 50.0);
        assert_eq!(size.shortest_side(), 50.0);
        assert_eq!(size.longest_side(), 100.0);
    }

    #[test]
    fn test_size_area() {
        let size = Size::new(10.0, 20.0);
        assert_eq!(size.area(), 200.0);
    }

    #[test]
    fn test_size_is_empty() {
        assert!(!Size::new(10.0, 20.0).is_empty());
        assert!(Size::new(0.0, 20.0).is_empty());
        assert!(Size::new(10.0, 0.0).is_empty());
        assert!(Size::new(-5.0, 20.0).is_empty());
    }

    #[test]
    fn test_size_conversions() {
        let size = Size::new(10.0, 20.0);

        let tuple: (f32, f32) = size.into();
        assert_eq!(tuple, (10.0, 20.0));

        let array: [f32; 2] = size.into();
        assert_eq!(array, [10.0, 20.0]);

        let from_tuple: Size = (15.0, 25.0).into();
        assert_eq!(from_tuple, Size::new(15.0, 25.0));
    }
}
