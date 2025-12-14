//! Unit type markers for type-safe coordinate systems.
//!
//! This module provides zero-sized marker types that can be used with geometry types
//! to prevent mixing coordinates from different coordinate spaces at compile time.
//!
//! # Design
//!
//! Inspired by [euclid](https://docs.rs/euclid)'s unit system, these markers use
//! `PhantomData` to provide compile-time safety without runtime overhead.
//!
//! # Usage
//!
//! ```rust
//! use flui_types::geometry::{Point, Vec2, ScreenSpace, WorldSpace};
//!
//! // Create points in different coordinate spaces
//! let screen_pos: Point<f64, ScreenSpace> = Point::new(100.0, 200.0);
//! let world_pos: Point<f64, WorldSpace> = Point::new(50.0, 75.0);
//!
//! // This won't compile - different coordinate spaces!
//! // let mixed = screen_pos + world_pos; // ERROR!
//!
//! // Explicit conversion when needed
//! let converted: Point<f64, WorldSpace> = screen_pos.cast_unit();
//! ```
//!
//! # When to Use Unit Types
//!
//! - **ScreenSpace**: Pixel coordinates on screen (origin at top-left)
//! - **WorldSpace**: Scene/world coordinates (may have different origin/scale)
//! - **LocalSpace**: Widget-local coordinates (relative to widget bounds)
//! - **UnknownUnit**: Default when coordinate space doesn't matter
//!
//! # Custom Unit Types
//!
//! You can define your own unit types for domain-specific coordinate systems:
//!
//! ```rust
//! use flui_types::geometry::Point;
//!
//! // Custom unit for a specific coordinate system
//! struct MapTileSpace;
//!
//! let tile_pos: Point<f64, MapTileSpace> = Point::new(5.0, 3.0);
//! ```

use std::fmt;

/// Default unit for when no specific coordinate space is needed.
///
/// This is a zero-sized type that provides no compile-time coordinate safety.
/// Use this when you don't need to distinguish between coordinate spaces,
/// or when working with simple, single-coordinate-system code.
///
/// # Example
///
/// ```rust
/// use flui_types::geometry::{Point, UnknownUnit};
///
/// // These are equivalent:
/// let p1: Point<f64, UnknownUnit> = Point::new(10.0, 20.0);
/// let p2: Point<f64> = Point::new(10.0, 20.0); // UnknownUnit is default
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct UnknownUnit;

impl fmt::Display for UnknownUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown")
    }
}

/// Screen space coordinates (pixels).
///
/// Use this for coordinates that represent positions on the screen or window.
/// Typically, the origin (0, 0) is at the top-left corner, with Y increasing downward.
///
/// # Coordinate Convention
///
/// - Origin: Top-left corner of the screen/window
/// - X: Increases to the right
/// - Y: Increases downward
/// - Units: Physical pixels (or logical pixels on HiDPI displays)
///
/// # Example
///
/// ```rust
/// use flui_types::geometry::{Point, ScreenSpace};
///
/// // Mouse position in screen coordinates
/// let mouse_pos: Point<f64, ScreenSpace> = Point::new(512.0, 384.0);
///
/// // Window size in screen coordinates
/// let window_size: Point<f64, ScreenSpace> = Point::new(1024.0, 768.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ScreenSpace;

impl fmt::Display for ScreenSpace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "screen")
    }
}

/// World/scene space coordinates.
///
/// Use this for coordinates in a larger scene or world that may be
/// transformed (scaled, rotated, translated) before being displayed on screen.
///
/// # Coordinate Convention
///
/// - Origin: Application-defined (often center of scene or specific anchor)
/// - X/Y: Application-defined (often Y increases upward for math convention)
/// - Units: Application-defined (meters, units, etc.)
///
/// # Example
///
/// ```rust
/// use flui_types::geometry::{Point, WorldSpace};
///
/// // Position of an entity in the game world
/// let entity_pos: Point<f64, WorldSpace> = Point::new(150.0, -30.0);
///
/// // Camera position in world coordinates
/// let camera_pos: Point<f64, WorldSpace> = Point::new(0.0, 0.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct WorldSpace;

impl fmt::Display for WorldSpace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "world")
    }
}

/// Widget-local space coordinates.
///
/// Use this for coordinates relative to a specific widget's bounds.
/// The origin is typically the top-left corner of the widget.
///
/// # Coordinate Convention
///
/// - Origin: Top-left corner of the widget
/// - X: Increases to the right (within widget bounds)
/// - Y: Increases downward (within widget bounds)
/// - Units: Logical pixels
///
/// # Example
///
/// ```rust
/// use flui_types::geometry::{Point, LocalSpace};
///
/// // Click position relative to a button widget
/// let local_click: Point<f64, LocalSpace> = Point::new(15.0, 8.0);
///
/// // Check if click is within widget bounds (100x50)
/// let in_bounds = local_click.x >= 0.0 && local_click.x <= 100.0
///              && local_click.y >= 0.0 && local_click.y <= 50.0;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct LocalSpace;

impl fmt::Display for LocalSpace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "local")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unit_types_are_zero_sized() {
        assert_eq!(std::mem::size_of::<UnknownUnit>(), 0);
        assert_eq!(std::mem::size_of::<ScreenSpace>(), 0);
        assert_eq!(std::mem::size_of::<WorldSpace>(), 0);
        assert_eq!(std::mem::size_of::<LocalSpace>(), 0);
    }

    #[test]
    fn test_unit_display() {
        assert_eq!(format!("{}", UnknownUnit), "unknown");
        assert_eq!(format!("{}", ScreenSpace), "screen");
        assert_eq!(format!("{}", WorldSpace), "world");
        assert_eq!(format!("{}", LocalSpace), "local");
    }

    #[test]
    fn test_unit_equality() {
        assert_eq!(UnknownUnit, UnknownUnit);
        assert_eq!(ScreenSpace, ScreenSpace);
        assert_eq!(WorldSpace, WorldSpace);
        assert_eq!(LocalSpace, LocalSpace);
    }

    #[test]
    fn test_unit_default() {
        let _: UnknownUnit = Default::default();
        let _: ScreenSpace = Default::default();
        let _: WorldSpace = Default::default();
        let _: LocalSpace = Default::default();
    }
}
