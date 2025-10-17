//! Aspect ratio types for maintaining proportional dimensions
//!
//! This module contains types for representing and working with aspect ratios,
//! similar to Flutter's aspect ratio system but adapted for egui.

use egui::Vec2;

/// A representation of an aspect ratio (width:height).
///
/// Similar to Flutter's aspect ratio concept but as a dedicated type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AspectRatio {
    /// The ratio value (width / height).
    ratio: f32,
}

impl AspectRatio {
    /// Create a new aspect ratio from a ratio value.
    ///
    /// # Panics
    /// Panics if the ratio is not finite and positive.
    pub fn new(ratio: f32) -> Self {
        assert!(ratio.is_finite() && ratio > 0.0, "Aspect ratio must be finite and positive");
        Self { ratio }
    }
    
    /// Create an aspect ratio from width and height.
    pub fn from_wh(width: f32, height: f32) -> Self {
        assert!(height != 0.0, "Height cannot be zero");
        Self::new(width / height)
    }
    
    /// Create an aspect ratio from a size.
    pub fn from_size(size: Vec2) -> Self {
        Self::from_wh(size.x, size.y)
    }
    
    /// Get the ratio value (width / height).
    pub fn ratio(&self) -> f32 {
        self.ratio
    }
    
    /// Get the inverse ratio (height / width).
    pub fn inverse(&self) -> Self {
        Self::new(1.0 / self.ratio)
    }
    
    /// Calculate the width for a given height.
    pub fn width_for_height(&self, height: f32) -> f32 {
        height * self.ratio
    }
    
    /// Calculate the height for a given width.
    pub fn height_for_width(&self, width: f32) -> f32 {
        width / self.ratio
    }
    
    /// Create a size with this aspect ratio and the given width.
    pub fn size_with_width(&self, width: f32) -> Vec2 {
        Vec2::new(width, self.height_for_width(width))
    }
    
    /// Create a size with this aspect ratio and the given height.
    pub fn size_with_height(&self, height: f32) -> Vec2 {
        Vec2::new(self.width_for_height(height), height)
    }
    
    /// Create a size that fits within the given bounds while preserving aspect ratio.
    pub fn fit_size(&self, bounds: Vec2) -> Vec2 {
        let bounds_ratio = bounds.x / bounds.y;
        
        if self.ratio > bounds_ratio {
            // Ratio is wider - fit to width
            self.size_with_width(bounds.x)
        } else {
            // Ratio is taller - fit to height
            self.size_with_height(bounds.y)
        }
    }
    
    /// Create a size that covers the given bounds while preserving aspect ratio.
    pub fn cover_size(&self, bounds: Vec2) -> Vec2 {
        let bounds_ratio = bounds.x / bounds.y;
        
        if self.ratio > bounds_ratio {
            // Ratio is wider - cover height
            self.size_with_height(bounds.y)
        } else {
            // Ratio is taller - cover width
            self.size_with_width(bounds.x)
        }
    }
    
    /// Check if this aspect ratio is wider than another.
    pub fn is_wider_than(&self, other: &Self) -> bool {
        self.ratio > other.ratio
    }
    
    /// Check if this aspect ratio is taller than another.
    pub fn is_taller_than(&self, other: &Self) -> bool {
        self.ratio < other.ratio
    }
    
    /// Linearly interpolate between two aspect ratios.
    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        Self::new(self.ratio + (other.ratio - self.ratio) * t)
    }
}

// Common aspect ratio constants
impl AspectRatio {
    /// Square aspect ratio (1:1).
    pub const SQUARE: Self = Self { ratio: 1.0 };
    
    /// Standard photo aspect ratio (4:3).
    pub const PHOTO_4_3: Self = Self { ratio: 4.0 / 3.0 };
    
    /// HD video aspect ratio (16:9).
    pub const HD_VIDEO: Self = Self { ratio: 16.0 / 9.0 };
    
    /// Cinema aspect ratio (21:9).
    pub const CINEMA: Self = Self { ratio: 21.0 / 9.0 };
    
    /// Portrait aspect ratio (2:3).
    pub const PORTRAIT: Self = Self { ratio: 2.0 / 3.0 };
    
    /// Landscape aspect ratio (3:2).
    pub const LANDSCAPE: Self = Self { ratio: 3.0 / 2.0 };
    
    /// Golden ratio aspect ratio (≈1.618:1).
    pub const GOLDEN: Self = Self { ratio: 1.618_034 };
}

impl Default for AspectRatio {
    fn default() -> Self {
        Self::SQUARE
    }
}

impl From<f32> for AspectRatio {
    fn from(ratio: f32) -> Self {
        Self::new(ratio)
    }
}

impl From<Vec2> for AspectRatio {
    fn from(size: Vec2) -> Self {
        Self::from_size(size)
    }
}

impl From<(f32, f32)> for AspectRatio {
    fn from((width, height): (f32, f32)) -> Self {
        Self::from_wh(width, height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_aspect_ratio_creation() {
        let ratio = AspectRatio::new(1.5);
        assert_eq!(ratio.ratio(), 1.5);
        
        let from_wh = AspectRatio::from_wh(16.0, 9.0);
        assert_eq!(from_wh.ratio(), 16.0 / 9.0);
        
        let from_size = AspectRatio::from_size(Vec2::new(4.0, 3.0));
        assert_eq!(from_size.ratio(), 4.0 / 3.0);
    }
    
    #[test]
    fn test_aspect_ratio_calculations() {
        let ratio = AspectRatio::new(2.0);
        
        assert_eq!(ratio.width_for_height(50.0), 100.0);
        assert_eq!(ratio.height_for_width(100.0), 50.0);
        
        assert_eq!(ratio.size_with_width(100.0), Vec2::new(100.0, 50.0));
        assert_eq!(ratio.size_with_height(50.0), Vec2::new(100.0, 50.0));
    }
    
    #[test]
    fn test_aspect_ratio_fitting() {
        let hd_ratio = AspectRatio::HD_VIDEO; // 16:9 ≈ 1.777
        let bounds = Vec2::new(100.0, 100.0);

        let fit = hd_ratio.fit_size(bounds);
        assert_eq!(fit, Vec2::new(100.0, 56.25)); // 100 / (16/9) = 56.25

        let cover = hd_ratio.cover_size(bounds);
        // 100 * (16/9) ≈ 177.78, check with epsilon
        assert!((cover.x - 177.77777).abs() < 0.01);
        assert_eq!(cover.y, 100.0);
    }
    
    #[test]
    fn test_aspect_ratio_comparisons() {
        let square = AspectRatio::SQUARE;
        let hd = AspectRatio::HD_VIDEO;
        let portrait = AspectRatio::PORTRAIT;
        
        assert!(hd.is_wider_than(&square));
        assert!(portrait.is_taller_than(&square));
        assert!(!square.is_wider_than(&hd));
    }
    
    #[test]
    fn test_common_aspect_ratios() {
        assert_eq!(AspectRatio::SQUARE.ratio(), 1.0);
        assert_eq!(AspectRatio::PHOTO_4_3.ratio(), 4.0 / 3.0);
        assert_eq!(AspectRatio::HD_VIDEO.ratio(), 16.0 / 9.0);
        assert_eq!(AspectRatio::PORTRAIT.ratio(), 2.0 / 3.0);
    }
    
    #[test]
    #[should_panic]
    fn test_invalid_aspect_ratio() {
        AspectRatio::new(0.0);
    }
}