//! Physical model types for Material Design elevation effects.
//!
//! These types support Material Design's concept of elevation, where UI
//! elements are positioned at different heights with corresponding shadow
//! effects.

use flui_foundation::geometry::Offset;

/// Shape type for physical model layers.
///
/// Determines the clipping shape and shadow outline for Material Design
/// elevation. Similar to Flutter's `BoxShape`.
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

    /// Canvas material (typically for backgrounds, may have different shadow
    /// behavior).
    Canvas,

    /// Card material (commonly elevated surfaces in Material Design).
    Card,

    /// Transparent material (shadows only, no background color).
    Transparency,
}

/// Elevation levels following Material Design guidelines.
///
/// These constants represent common elevation levels in Material Design.
/// Custom elevations can be used by specifying f64 values directly.
///
/// Reference: Material Design 3 elevation scale
#[derive(Debug)]
pub struct Elevation;

impl Elevation {
    /// Level 0: Surface level (no elevation).
    pub const LEVEL_0: f64 = 0.0;

    /// Level 1: Raised elements (1dp).
    pub const LEVEL_1: f64 = 1.0;

    /// Level 2: Floating action button at rest (3dp).
    pub const LEVEL_2: f64 = 3.0;

    /// Level 3: Navigation drawer, modal bottom sheet (6dp).
    pub const LEVEL_3: f64 = 6.0;

    /// Level 4: App bar (8dp).
    pub const LEVEL_4: f64 = 8.0;

    /// Level 5: Dialog, picker (12dp).
    pub const LEVEL_5: f64 = 12.0;

    /// Maximum reasonable elevation (24dp).
    pub const MAX: f64 = 24.0;

    /// Calculate shadow blur radius from elevation.
    ///
    /// Uses Material Design's shadow algorithm where blur radius increases
    /// with elevation to simulate light scattering.
    #[inline]
    pub fn blur_radius(elevation: f64) -> f64 {
        // Material Design shadow blur formula
        // Higher elevations have softer, more diffuse shadows
        elevation * 0.5 + elevation.sqrt() * 1.5
    }

    /// Calculate shadow offset from elevation.
    ///
    /// Simulates a light source positioned above and slightly offset.
    /// Higher elevations cast shadows further from the element.
    #[inline]
    pub fn shadow_offset(elevation: f64) -> Offset<f64> {
        // Material Design assumes light from top-left at ~45 degrees
        // Vertical offset increases more than horizontal
        Offset::new(
            elevation * 0.2, // Slight horizontal offset
            elevation * 0.4, // More pronounced vertical offset
        )
    }

    /// Calculate shadow spread from elevation.
    ///
    /// Spread simulates penumbra (soft edge) of the shadow.
    #[inline]
    pub fn spread_radius(elevation: f64) -> f64 {
        // Small negative spread for sharper definition
        -elevation * 0.1
    }
}
