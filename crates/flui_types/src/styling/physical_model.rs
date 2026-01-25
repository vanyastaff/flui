//! Physical model types for Material Design elevation effects.
//!
//! These types support Material Design's concept of elevation, where UI elements
//! are positioned at different heights with corresponding shadow effects.

use crate::geometry::{px, Pixels};
use crate::Offset;

/// Shape type for physical model layers.
///
/// Determines the clipping shape and shadow outline for Material Design elevation.
/// Similar to Flutter's `BoxShape`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PhysicalShape {
    /// Rectangular shape (possibly with rounded corners via border radius).
    #[default]
    Rectangle,

    /// Circular/oval shape.
    Circle,
}

/// Material type for physical model rendering.
///
/// Different material types may render with different visual characteristics
/// in the future (e.g., different shadow styles, surface finishes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MaterialType {
    /// Standard material with normal elevation shadows.
    #[default]
    Standard,

    /// Canvas material (typically for backgrounds, may have different shadow behavior).
    Canvas,

    /// Card material (commonly elevated surfaces in Material Design).
    Card,

    /// Transparent material (shadows only, no background color).
    Transparency,
}

/// Elevation levels following Material Design guidelines.
///
/// These constants represent common elevation levels in Material Design.
/// Custom elevations can be used by specifying f32 values directly.
///
/// Reference: Material Design 3 elevation scale
pub struct Elevation;

impl Elevation {
    /// Level 0: Surface level (no elevation).
    pub const LEVEL_0: f32 = 0.0;

    /// Level 1: Raised elements (1dp).
    pub const LEVEL_1: f32 = 1.0;

    /// Level 2: Floating action button at rest (3dp).
    pub const LEVEL_2: f32 = 3.0;

    /// Level 3: Navigation drawer, modal bottom sheet (6dp).
    pub const LEVEL_3: f32 = 6.0;

    /// Level 4: App bar (8dp).
    pub const LEVEL_4: f32 = 8.0;

    /// Level 5: Dialog, picker (12dp).
    pub const LEVEL_5: f32 = 12.0;

    /// Maximum reasonable elevation (24dp).
    pub const MAX: f32 = 24.0;

    /// Calculate shadow blur radius from elevation.
    ///
    /// Uses Material Design's shadow algorithm where blur radius increases
    /// with elevation to simulate light scattering.
    #[inline]
    pub fn blur_radius(elevation: f32) -> f32 {
        // Material Design shadow blur formula
        // Higher elevations have softer, more diffuse shadows
        elevation * 0.5 + elevation.sqrt() * 1.5
    }

    /// Calculate shadow offset from elevation.
    ///
    /// Simulates a light source positioned above and slightly offset.
    /// Higher elevations cast shadows further from the element.
    #[inline]
    pub fn shadow_offset(elevation: f32) -> Offset<Pixels> {
        // Material Design assumes light from top-left at ~45 degrees
        // Vertical offset increases more than horizontal
        Offset::new(
            px(elevation * 0.2), // Slight horizontal offset
            px(elevation * 0.4), // More pronounced vertical offset
        )
    }

    /// Calculate shadow spread from elevation.
    ///
    /// Spread simulates penumbra (soft edge) of the shadow.
    #[inline]
    pub fn spread_radius(elevation: f32) -> f32 {
        // Small negative spread for sharper definition
        -elevation * 0.1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_physical_shape_default() {
        assert_eq!(PhysicalShape::default(), PhysicalShape::Rectangle);
    }

    #[test]
    fn test_material_type_default() {
        assert_eq!(MaterialType::default(), MaterialType::Standard);
    }

    #[test]
    fn test_elevation_levels() {
        assert_eq!(Elevation::LEVEL_0, 0.0);
        assert_eq!(Elevation::LEVEL_1, 1.0);
        assert_eq!(Elevation::LEVEL_2, 3.0);
        assert_eq!(Elevation::LEVEL_3, 6.0);
        assert_eq!(Elevation::LEVEL_4, 8.0);
        assert_eq!(Elevation::LEVEL_5, 12.0);
        assert_eq!(Elevation::MAX, 24.0);
    }

    #[test]
    fn test_elevation_blur_radius() {
        // At elevation 0, minimal blur
        assert_eq!(Elevation::blur_radius(0.0), 0.0);

        // At elevation 4, moderate blur
        let blur_4 = Elevation::blur_radius(4.0);
        assert!(blur_4 > 2.0 && blur_4 < 6.0);

        // Higher elevation = more blur
        let blur_12 = Elevation::blur_radius(12.0);
        assert!(blur_12 > blur_4);
    }

    #[test]
    fn test_elevation_shadow_offset() {
        use crate::geometry::px;
        let offset_0 = Elevation::shadow_offset(0.0);
        assert_eq!(offset_0, Offset::ZERO);

        let offset_8 = Elevation::shadow_offset(8.0);
        assert!(offset_8.dx > px(0.0) && offset_8.dy > px(0.0));

        // Vertical offset should be larger than horizontal
        assert!(offset_8.dy > offset_8.dx);
    }

    #[test]
    fn test_elevation_spread_radius() {
        // Spread should be negative (inset) and proportional to elevation
        assert_eq!(Elevation::spread_radius(0.0), 0.0);

        let spread_10 = Elevation::spread_radius(10.0);
        assert!(spread_10 < 0.0);
        assert!(spread_10 > -2.0);
    }
}
