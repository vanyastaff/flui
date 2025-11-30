//! Pointer event types
//!
//! This module provides types for pointer events (mouse, touch, stylus).
//! Based on Flutter's pointer event system.

use crate::gestures::PointerDeviceKind;
use crate::Offset;

/// Mouse button that was pressed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PointerButton {
    /// Primary button (usually left mouse button)
    Primary,
    /// Secondary button (usually right mouse button)
    Secondary,
    /// Middle button
    Middle,
    /// Additional button
    Other(u8),
}

/// Base pointer event data
///
/// Contains common fields for all pointer events.
#[derive(Debug, Clone)]
pub struct PointerEventData {
    /// Position in global coordinates
    pub position: Offset,
    /// Position in local widget coordinates (set during hit testing)
    pub local_position: Offset,
    /// Device that generated the event
    pub device_kind: PointerDeviceKind,
    /// Pointer device ID
    pub device: i32,
    /// Button that was pressed (for down/up events)
    pub button: Option<PointerButton>,
    /// Buttons currently pressed
    pub buttons: u8,
    /// Pressure of the touch (0.0 to 1.0, or None if not available)
    ///
    /// - `0.0` = no pressure (hovering)
    /// - `1.0` = maximum pressure
    /// - `None` = device doesn't support pressure
    pub pressure: Option<f32>,
    /// Maximum pressure the device can report
    ///
    /// Typically 1.0 for most touch devices.
    pub pressure_max: f32,
    /// Minimum detectable pressure (usually 0.0)
    pub pressure_min: f32,

    // ========================================================================
    // Stylus-specific fields
    // ========================================================================
    /// Tilt of the stylus along the X axis (radians, -π/2 to π/2)
    ///
    /// - `0.0` = perpendicular to surface
    /// - Positive = tilted right
    /// - Negative = tilted left
    /// - `None` = device doesn't support tilt
    pub tilt_x: Option<f32>,

    /// Tilt of the stylus along the Y axis (radians, -π/2 to π/2)
    ///
    /// - `0.0` = perpendicular to surface
    /// - Positive = tilted toward user
    /// - Negative = tilted away from user
    /// - `None` = device doesn't support tilt
    pub tilt_y: Option<f32>,

    /// Rotation of the stylus around its axis (radians, 0 to 2π)
    ///
    /// Also known as "twist" or "barrel rotation".
    /// - `0.0` = natural orientation
    /// - `None` = device doesn't support rotation
    pub rotation: Option<f32>,

    /// Distance of the stylus from the surface (normalized 0.0 to 1.0)
    ///
    /// - `0.0` = touching/on surface
    /// - `1.0` = maximum detectable distance (hovering)
    /// - `None` = device doesn't support distance sensing
    pub distance: Option<f32>,

    /// Maximum detectable distance for hover
    pub distance_max: f32,
}

impl PointerEventData {
    /// Create new pointer event data
    pub fn new(position: Offset, device_kind: PointerDeviceKind) -> Self {
        Self {
            position,
            local_position: position,
            device_kind,
            device: 0,
            button: None,
            buttons: 0,
            pressure: None,
            pressure_max: 1.0,
            pressure_min: 0.0,
            tilt_x: None,
            tilt_y: None,
            rotation: None,
            distance: None,
            distance_max: 1.0,
        }
    }

    /// Create with button
    pub fn with_button(mut self, button: PointerButton) -> Self {
        self.button = Some(button);
        self
    }

    /// Create with pressure
    pub fn with_pressure(mut self, pressure: f32) -> Self {
        self.pressure = Some(pressure.clamp(0.0, 1.0));
        self
    }

    /// Create with pressure range
    pub fn with_pressure_range(mut self, min: f32, max: f32) -> Self {
        self.pressure_min = min;
        self.pressure_max = max;
        self
    }

    /// Returns the normalized pressure (0.0 to 1.0)
    ///
    /// Returns 0.0 if pressure is not available.
    pub fn normalized_pressure(&self) -> f32 {
        self.pressure.unwrap_or(0.0)
    }

    /// Returns true if the device supports pressure sensing
    pub fn supports_pressure(&self) -> bool {
        self.pressure.is_some()
    }

    /// Returns true if this is a force press (pressure > threshold)
    ///
    /// Default threshold is 0.4 (40% of max pressure).
    pub fn is_force_press(&self) -> bool {
        self.is_force_press_at(0.4)
    }

    /// Returns true if pressure exceeds the given threshold (0.0 to 1.0)
    pub fn is_force_press_at(&self, threshold: f32) -> bool {
        self.normalized_pressure() >= threshold
    }

    // ========================================================================
    // Stylus methods
    // ========================================================================

    /// Create with stylus tilt (in radians)
    ///
    /// Both values should be in the range -π/2 to π/2.
    pub fn with_tilt(mut self, tilt_x: f32, tilt_y: f32) -> Self {
        use std::f32::consts::FRAC_PI_2;
        self.tilt_x = Some(tilt_x.clamp(-FRAC_PI_2, FRAC_PI_2));
        self.tilt_y = Some(tilt_y.clamp(-FRAC_PI_2, FRAC_PI_2));
        self
    }

    /// Create with stylus rotation (in radians, 0 to 2π)
    pub fn with_rotation(mut self, rotation: f32) -> Self {
        use std::f32::consts::TAU;
        self.rotation = Some(rotation.rem_euclid(TAU));
        self
    }

    /// Create with stylus hover distance (normalized 0.0 to 1.0)
    pub fn with_distance(mut self, distance: f32) -> Self {
        self.distance = Some(distance.clamp(0.0, 1.0));
        self
    }

    /// Returns true if the device supports stylus tilt sensing
    pub fn supports_tilt(&self) -> bool {
        self.tilt_x.is_some() && self.tilt_y.is_some()
    }

    /// Returns true if the device supports stylus rotation sensing
    pub fn supports_rotation(&self) -> bool {
        self.rotation.is_some()
    }

    /// Returns true if the device supports distance/hover sensing
    pub fn supports_distance(&self) -> bool {
        self.distance.is_some()
    }

    /// Returns true if this is a stylus with extended capabilities
    pub fn is_stylus_with_capabilities(&self) -> bool {
        self.device_kind == PointerDeviceKind::Stylus
            && (self.supports_tilt() || self.supports_rotation() || self.supports_pressure())
    }

    /// Get the stylus tilt as (x, y) tuple in radians
    ///
    /// Returns (0.0, 0.0) if tilt is not available.
    pub fn tilt(&self) -> (f32, f32) {
        (self.tilt_x.unwrap_or(0.0), self.tilt_y.unwrap_or(0.0))
    }

    /// Get the stylus altitude angle (radians from surface, 0 to π/2)
    ///
    /// - `0.0` = parallel to surface
    /// - `π/2` = perpendicular to surface
    ///
    /// Calculated from tilt_x and tilt_y using spherical coordinates.
    pub fn altitude(&self) -> f32 {
        if !self.supports_tilt() {
            return std::f32::consts::FRAC_PI_2; // Default: perpendicular
        }

        let (tx, ty) = self.tilt();
        // Altitude = π/2 - arcsin(sqrt(sin²(tilt_x) + sin²(tilt_y)))
        let sin_tx = tx.sin();
        let sin_ty = ty.sin();
        let combined = (sin_tx * sin_tx + sin_ty * sin_ty).sqrt().min(1.0);
        std::f32::consts::FRAC_PI_2 - combined.asin()
    }

    /// Get the stylus azimuth angle (radians around Z axis, 0 to 2π)
    ///
    /// The angle where the stylus is pointing when projected onto the surface.
    /// - `0.0` = pointing right (+X)
    /// - `π/2` = pointing up (+Y)
    pub fn azimuth(&self) -> f32 {
        if !self.supports_tilt() {
            return 0.0;
        }

        let (tx, ty) = self.tilt();
        ty.atan2(tx).rem_euclid(std::f32::consts::TAU)
    }

    /// Get the normalized distance from surface (0.0 = touching, 1.0 = max hover)
    pub fn normalized_distance(&self) -> f32 {
        self.distance.unwrap_or(0.0)
    }

    /// Returns true if the stylus is hovering (not touching surface)
    pub fn is_hovering(&self) -> bool {
        self.distance.is_some_and(|d| d > 0.0)
    }
}

/// Pointer event types
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum PointerEvent {
    /// Pointer device was added (connected)
    Added {
        /// Device ID
        device: i32,
        /// Device kind
        device_kind: PointerDeviceKind,
    },
    /// Pointer device was removed (disconnected)
    Removed {
        /// Device ID
        device: i32,
    },
    /// Pointer pressed down
    Down(PointerEventData),
    /// Pointer released
    Up(PointerEventData),
    /// Pointer moved
    Move(PointerEventData),
    /// Pointer hover (moved without button pressed)
    Hover(PointerEventData),
    /// Pointer entered widget bounds
    Enter(PointerEventData),
    /// Pointer exited widget bounds
    Exit(PointerEventData),
    /// Event cancelled
    Cancel(PointerEventData),
    /// Scroll event (mouse wheel, trackpad scroll)
    Scroll {
        /// Device ID
        device: i32,
        /// Position where scroll occurred
        position: Offset,
        /// Scroll delta
        scroll_delta: Offset,
    },
}

impl PointerEvent {
    /// Get the event data (if available)
    ///
    /// Returns None for Added, Removed, and Scroll events which don't have PointerEventData
    pub fn data(&self) -> Option<&PointerEventData> {
        match self {
            PointerEvent::Down(data)
            | PointerEvent::Up(data)
            | PointerEvent::Move(data)
            | PointerEvent::Hover(data)
            | PointerEvent::Enter(data)
            | PointerEvent::Exit(data)
            | PointerEvent::Cancel(data) => Some(data),
            PointerEvent::Added { .. }
            | PointerEvent::Removed { .. }
            | PointerEvent::Scroll { .. } => None,
        }
    }

    /// Get mutable event data (if available)
    ///
    /// Returns None for Added, Removed, and Scroll events which don't have PointerEventData
    pub fn data_mut(&mut self) -> Option<&mut PointerEventData> {
        match self {
            PointerEvent::Down(data)
            | PointerEvent::Up(data)
            | PointerEvent::Move(data)
            | PointerEvent::Hover(data)
            | PointerEvent::Enter(data)
            | PointerEvent::Exit(data)
            | PointerEvent::Cancel(data) => Some(data),
            PointerEvent::Added { .. }
            | PointerEvent::Removed { .. }
            | PointerEvent::Scroll { .. } => None,
        }
    }

    /// Get position in global coordinates
    pub fn position(&self) -> Offset {
        match self {
            PointerEvent::Scroll { position, .. } => *position,
            _ => self.data().map(|d| d.position).unwrap_or(Offset::ZERO),
        }
    }

    /// Get device ID
    pub fn device(&self) -> i32 {
        match self {
            PointerEvent::Added { device, .. }
            | PointerEvent::Removed { device }
            | PointerEvent::Scroll { device, .. } => *device,
            _ => self.data().map(|d| d.device).unwrap_or(0),
        }
    }

    /// Get position in local widget coordinates
    pub fn local_position(&self) -> Offset {
        self.data()
            .map(|d| d.local_position)
            .unwrap_or(Offset::ZERO)
    }

    /// Set local position (used during hit testing)
    pub fn set_local_position(&mut self, position: Offset) {
        if let Some(data) = self.data_mut() {
            data.local_position = position;
        }
    }

    /// Get the pressure value (if available)
    pub fn pressure(&self) -> Option<f32> {
        self.data().and_then(|d| d.pressure)
    }

    /// Get the normalized pressure (0.0 to 1.0)
    ///
    /// Returns 0.0 if pressure is not available.
    pub fn normalized_pressure(&self) -> f32 {
        self.data().map(|d| d.normalized_pressure()).unwrap_or(0.0)
    }

    /// Returns true if the device supports pressure sensing
    pub fn supports_pressure(&self) -> bool {
        self.data().is_some_and(|d| d.supports_pressure())
    }

    /// Returns true if this is a force press (pressure > 0.4)
    pub fn is_force_press(&self) -> bool {
        self.data().is_some_and(|d| d.is_force_press())
    }

    /// Returns true if pressure exceeds the given threshold
    pub fn is_force_press_at(&self, threshold: f32) -> bool {
        self.data().is_some_and(|d| d.is_force_press_at(threshold))
    }

    // ========================================================================
    // Stylus methods
    // ========================================================================

    /// Returns true if the device supports stylus tilt sensing
    pub fn supports_tilt(&self) -> bool {
        self.data().is_some_and(|d| d.supports_tilt())
    }

    /// Returns true if the device supports stylus rotation sensing
    pub fn supports_rotation(&self) -> bool {
        self.data().is_some_and(|d| d.supports_rotation())
    }

    /// Returns true if the device supports distance/hover sensing
    pub fn supports_distance(&self) -> bool {
        self.data().is_some_and(|d| d.supports_distance())
    }

    /// Get the stylus tilt as (x, y) tuple in radians
    pub fn tilt(&self) -> (f32, f32) {
        self.data().map(|d| d.tilt()).unwrap_or((0.0, 0.0))
    }

    /// Get the stylus rotation in radians
    pub fn rotation(&self) -> Option<f32> {
        self.data().and_then(|d| d.rotation)
    }

    /// Get the stylus altitude angle
    pub fn altitude(&self) -> f32 {
        self.data()
            .map(|d| d.altitude())
            .unwrap_or(std::f32::consts::FRAC_PI_2)
    }

    /// Get the stylus azimuth angle
    pub fn azimuth(&self) -> f32 {
        self.data().map(|d| d.azimuth()).unwrap_or(0.0)
    }

    /// Get the normalized distance from surface
    pub fn normalized_distance(&self) -> f32 {
        self.data().map(|d| d.normalized_distance()).unwrap_or(0.0)
    }

    /// Returns true if the stylus is hovering
    pub fn is_hovering(&self) -> bool {
        self.data().is_some_and(|d| d.is_hovering())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pointer_event_data() {
        let data = PointerEventData::new(Offset::new(10.0, 20.0), PointerDeviceKind::Mouse);
        assert_eq!(data.position, Offset::new(10.0, 20.0));
        assert_eq!(data.device_kind, PointerDeviceKind::Mouse);
        assert!(data.pressure.is_none());
    }

    #[test]
    fn test_pointer_event() {
        let data = PointerEventData::new(Offset::new(5.0, 10.0), PointerDeviceKind::Touch);
        let event = PointerEvent::Down(data);

        assert_eq!(event.position(), Offset::new(5.0, 10.0));
    }

    #[test]
    fn test_pressure_support() {
        let data = PointerEventData::new(Offset::new(0.0, 0.0), PointerDeviceKind::Touch)
            .with_pressure(0.5);

        assert!(data.supports_pressure());
        assert_eq!(data.normalized_pressure(), 0.5);
        assert!(data.is_force_press()); // 0.5 >= 0.4 threshold
    }

    #[test]
    fn test_force_press_threshold() {
        // Below threshold
        let data_low = PointerEventData::new(Offset::new(0.0, 0.0), PointerDeviceKind::Touch)
            .with_pressure(0.3);
        assert!(!data_low.is_force_press());
        assert!(!data_low.is_force_press_at(0.4));

        // At threshold
        let data_at = PointerEventData::new(Offset::new(0.0, 0.0), PointerDeviceKind::Touch)
            .with_pressure(0.4);
        assert!(data_at.is_force_press());

        // Above threshold
        let data_high = PointerEventData::new(Offset::new(0.0, 0.0), PointerDeviceKind::Touch)
            .with_pressure(0.8);
        assert!(data_high.is_force_press());
        assert!(data_high.is_force_press_at(0.7));
    }

    #[test]
    fn test_pressure_clamp() {
        // Pressure should be clamped to 0.0-1.0
        let data_over = PointerEventData::new(Offset::new(0.0, 0.0), PointerDeviceKind::Touch)
            .with_pressure(1.5);
        assert_eq!(data_over.normalized_pressure(), 1.0);

        let data_under = PointerEventData::new(Offset::new(0.0, 0.0), PointerDeviceKind::Touch)
            .with_pressure(-0.5);
        assert_eq!(data_under.normalized_pressure(), 0.0);
    }

    #[test]
    fn test_no_pressure_defaults() {
        let data = PointerEventData::new(Offset::new(0.0, 0.0), PointerDeviceKind::Mouse);
        assert!(!data.supports_pressure());
        assert_eq!(data.normalized_pressure(), 0.0);
        assert!(!data.is_force_press());
    }

    #[test]
    fn test_pointer_event_pressure() {
        let data = PointerEventData::new(Offset::new(0.0, 0.0), PointerDeviceKind::Touch)
            .with_pressure(0.6);
        let event = PointerEvent::Down(data);

        assert!(event.supports_pressure());
        assert_eq!(event.pressure(), Some(0.6));
        assert_eq!(event.normalized_pressure(), 0.6);
        assert!(event.is_force_press());
    }

    #[test]
    fn test_pressure_range() {
        let data = PointerEventData::new(Offset::new(0.0, 0.0), PointerDeviceKind::Stylus)
            .with_pressure(0.5)
            .with_pressure_range(0.0, 2.0);

        assert_eq!(data.pressure_min, 0.0);
        assert_eq!(data.pressure_max, 2.0);
    }

    // ========================================================================
    // Stylus tests
    // ========================================================================

    #[test]
    fn test_stylus_tilt() {
        use std::f32::consts::FRAC_PI_4;

        let data = PointerEventData::new(Offset::new(0.0, 0.0), PointerDeviceKind::Stylus)
            .with_tilt(FRAC_PI_4, -FRAC_PI_4);

        assert!(data.supports_tilt());
        let (tx, ty) = data.tilt();
        assert!((tx - FRAC_PI_4).abs() < 0.001);
        assert!((ty - (-FRAC_PI_4)).abs() < 0.001);
    }

    #[test]
    fn test_stylus_tilt_clamping() {
        use std::f32::consts::{FRAC_PI_2, PI};

        // Tilt values beyond ±π/2 should be clamped
        let data = PointerEventData::new(Offset::new(0.0, 0.0), PointerDeviceKind::Stylus)
            .with_tilt(PI, -PI);

        let (tx, ty) = data.tilt();
        assert!((tx - FRAC_PI_2).abs() < 0.001);
        assert!((ty - (-FRAC_PI_2)).abs() < 0.001);
    }

    #[test]
    fn test_stylus_rotation() {
        use std::f32::consts::PI;

        let data = PointerEventData::new(Offset::new(0.0, 0.0), PointerDeviceKind::Stylus)
            .with_rotation(PI);

        assert!(data.supports_rotation());
        assert!((data.rotation.unwrap() - PI).abs() < 0.001);
    }

    #[test]
    fn test_stylus_rotation_wrapping() {
        use std::f32::consts::{PI, TAU};

        // Rotation wraps around at 2π
        let data = PointerEventData::new(Offset::new(0.0, 0.0), PointerDeviceKind::Stylus)
            .with_rotation(TAU + PI); // 3π should wrap to π

        assert!((data.rotation.unwrap() - PI).abs() < 0.001);
    }

    #[test]
    fn test_stylus_distance() {
        let data = PointerEventData::new(Offset::new(0.0, 0.0), PointerDeviceKind::Stylus)
            .with_distance(0.5);

        assert!(data.supports_distance());
        assert_eq!(data.normalized_distance(), 0.5);
        assert!(data.is_hovering());
    }

    #[test]
    fn test_stylus_distance_touching() {
        let data = PointerEventData::new(Offset::new(0.0, 0.0), PointerDeviceKind::Stylus)
            .with_distance(0.0);

        assert!(data.supports_distance());
        assert_eq!(data.normalized_distance(), 0.0);
        assert!(!data.is_hovering()); // 0.0 means touching
    }

    #[test]
    fn test_stylus_altitude_perpendicular() {
        use std::f32::consts::FRAC_PI_2;

        // No tilt = perpendicular (altitude = π/2)
        let data = PointerEventData::new(Offset::new(0.0, 0.0), PointerDeviceKind::Stylus)
            .with_tilt(0.0, 0.0);

        let altitude = data.altitude();
        assert!((altitude - FRAC_PI_2).abs() < 0.001);
    }

    #[test]
    fn test_stylus_altitude_tilted() {
        use std::f32::consts::FRAC_PI_4;

        // Tilted 45 degrees on X axis
        let data = PointerEventData::new(Offset::new(0.0, 0.0), PointerDeviceKind::Stylus)
            .with_tilt(FRAC_PI_4, 0.0);

        let altitude = data.altitude();
        // Altitude should be less than π/2 when tilted
        assert!(altitude < std::f32::consts::FRAC_PI_2);
        assert!(altitude > 0.0);
    }

    #[test]
    fn test_stylus_azimuth() {
        use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};

        // Tilted only on Y axis = pointing up (azimuth ≈ π/2)
        let data = PointerEventData::new(Offset::new(0.0, 0.0), PointerDeviceKind::Stylus)
            .with_tilt(0.0, FRAC_PI_4);

        let azimuth = data.azimuth();
        assert!((azimuth - FRAC_PI_2).abs() < 0.001);
    }

    #[test]
    fn test_stylus_no_capabilities() {
        let data = PointerEventData::new(Offset::new(0.0, 0.0), PointerDeviceKind::Stylus);

        assert!(!data.supports_tilt());
        assert!(!data.supports_rotation());
        assert!(!data.supports_distance());
        assert!(!data.is_stylus_with_capabilities());
    }

    #[test]
    fn test_stylus_with_all_capabilities() {
        let data = PointerEventData::new(Offset::new(0.0, 0.0), PointerDeviceKind::Stylus)
            .with_pressure(0.5)
            .with_tilt(0.1, 0.2)
            .with_rotation(1.0)
            .with_distance(0.3);

        assert!(data.supports_pressure());
        assert!(data.supports_tilt());
        assert!(data.supports_rotation());
        assert!(data.supports_distance());
        assert!(data.is_stylus_with_capabilities());
    }

    #[test]
    fn test_pointer_event_stylus_methods() {
        use std::f32::consts::FRAC_PI_4;

        let data = PointerEventData::new(Offset::new(0.0, 0.0), PointerDeviceKind::Stylus)
            .with_tilt(FRAC_PI_4, 0.0)
            .with_rotation(1.5)
            .with_distance(0.2);

        let event = PointerEvent::Move(data);

        assert!(event.supports_tilt());
        assert!(event.supports_rotation());
        assert!(event.supports_distance());

        let (tx, _) = event.tilt();
        assert!((tx - FRAC_PI_4).abs() < 0.001);
        assert!((event.rotation().unwrap() - 1.5).abs() < 0.001);
        assert!((event.normalized_distance() - 0.2).abs() < 0.001);
        assert!(event.is_hovering());
    }
}
