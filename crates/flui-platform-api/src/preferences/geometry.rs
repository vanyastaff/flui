//! Native observations and validated presentation-specific gesture geometry.

use flui_foundation::geometry::{DevicePixelRatio, DeviceSize, Size};

use super::InvalidPreference;

/// A finite, nonnegative distance in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Distance(f64);

impl Distance {
    /// Admit a logical distance, including zero.
    ///
    /// # Errors
    /// Refuses negative and nonfinite values.
    pub fn new(value: f64) -> Result<Self, InvalidPreference> {
        if value.is_finite() && value >= 0.0 {
            Ok(Self(value))
        } else {
            Err(InvalidPreference::Distance)
        }
    }

    /// The distance in logical pixels.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

/// Finite positive minimum and maximum fling speeds in logical pixels/second.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlingSpeeds {
    min: f64,
    max: f64,
}

impl FlingSpeeds {
    /// Admit a speed range.
    ///
    /// # Errors
    /// Refuses nonpositive/nonfinite speeds and a minimum greater than maximum.
    pub fn new(min: f64, max: f64) -> Result<Self, InvalidPreference> {
        if !min.is_finite() || !max.is_finite() || min <= 0.0 || max <= 0.0 {
            return Err(InvalidPreference::Speed);
        }
        if min > max {
            return Err(InvalidPreference::FlingRange);
        }
        Ok(Self { min, max })
    }

    /// Minimum fling speed in logical pixels/second.
    #[must_use]
    pub const fn min(self) -> f64 {
        self.min
    }

    /// Maximum fling speed in logical pixels/second.
    #[must_use]
    pub const fn max(self) -> f64 {
        self.max
    }
}

/// Mouse geometry sampled in an explicit native coordinate context.
///
/// Host-wide observations detect settings changes without a user window. They
/// are not an exact projection for another DPI: that presentation queries its
/// native backend separately. Double-click dimensions are the full rectangle;
/// drag tolerance is the permitted displacement on either side of the origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NativeMouseGeometry {
    double_click_area: DeviceSize,
    drag_tolerance: DeviceSize,
    pixel_ratio: DevicePixelRatio,
}

impl NativeMouseGeometry {
    /// Record full double-click dimensions and drag half-extents in device pixels.
    ///
    /// Zero is supported independently on each axis.
    ///
    /// # Errors
    /// Refuses negative dimensions or half-extents.
    pub fn new(
        double_click_area: DeviceSize,
        drag_tolerance: DeviceSize,
        pixel_ratio: DevicePixelRatio,
    ) -> Result<Self, InvalidPreference> {
        if double_click_area.width < 0
            || double_click_area.height < 0
            || drag_tolerance.width < 0
            || drag_tolerance.height < 0
        {
            return Err(InvalidPreference::GestureArea);
        }
        Ok(Self {
            double_click_area,
            drag_tolerance,
            pixel_ratio,
        })
    }

    /// Full double-click rectangle dimensions in sampled device pixels.
    #[must_use]
    pub const fn double_click_area(self) -> DeviceSize {
        self.double_click_area
    }

    /// Drag tolerance on either side of the origin, in sampled device pixels.
    #[must_use]
    pub const fn drag_tolerance(self) -> DeviceSize {
        self.drag_tolerance
    }

    /// Device/logical scale at which this observation was sampled.
    #[must_use]
    pub const fn pixel_ratio(self) -> DevicePixelRatio {
        self.pixel_ratio
    }
}

/// Touch measurements retained in their native physical sampling context.
///
/// Host snapshots use this observation to detect a changed context or setting.
/// A presentation queries its own native context before projecting these units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NativeTouchGeometry {
    touch_slop: i32,
    double_tap_slop: i32,
    min_fling: i32,
    max_fling: i32,
    pixel_ratio: DevicePixelRatio,
}

impl NativeTouchGeometry {
    /// Admit physical pixel distances and physical pixel/second fling bounds.
    ///
    /// # Errors
    /// Refuses negative distances, nonpositive speeds, or an inverted range.
    pub fn new(
        touch_slop: i32,
        double_tap_slop: i32,
        min_fling: i32,
        max_fling: i32,
        pixel_ratio: DevicePixelRatio,
    ) -> Result<Self, InvalidPreference> {
        if touch_slop < 0 || double_tap_slop < 0 {
            return Err(InvalidPreference::Distance);
        }
        if min_fling <= 0 || max_fling <= 0 {
            return Err(InvalidPreference::Speed);
        }
        if min_fling > max_fling {
            return Err(InvalidPreference::FlingRange);
        }
        Ok(Self {
            touch_slop,
            double_tap_slop,
            min_fling,
            max_fling,
            pixel_ratio,
        })
    }

    /// The native context's device/logical scale.
    #[must_use]
    pub const fn pixel_ratio(self) -> DevicePixelRatio {
        self.pixel_ratio
    }

    /// Physical touch slop in pixels.
    #[must_use]
    pub const fn touch_slop(self) -> i32 {
        self.touch_slop
    }

    /// Physical distance between double taps, in pixels.
    #[must_use]
    pub const fn double_tap_slop(self) -> i32 {
        self.double_tap_slop
    }

    /// Minimum physical fling speed, in pixels/second.
    #[must_use]
    pub const fn min_fling(self) -> i32 {
        self.min_fling
    }

    /// Maximum physical fling speed, in pixels/second.
    #[must_use]
    pub const fn max_fling(self) -> i32 {
        self.max_fling
    }
}

/// Validated logical gesture geometry for one presentation's coordinate context.
///
/// Missing fields retain consumer fallbacks. This is separate from the host's
/// observation used to detect changes; obtain it from `PlatformWindow` whenever
/// accepted system preferences or that window's DPI change.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GestureGeometry {
    pixel_ratio: DevicePixelRatio,
    mouse_double_click_area: Option<Size>,
    mouse_drag_tolerance: Option<Size>,
    touch_slop: Option<Distance>,
    touch_double_tap_slop: Option<Distance>,
    fling_speeds: Option<FlingSpeeds>,
}

fn checked_area(size: Size) -> Result<Size, InvalidPreference> {
    if size.width.is_finite() && size.height.is_finite() && size.width >= 0.0 && size.height >= 0.0
    {
        Ok(size)
    } else {
        Err(InvalidPreference::GestureArea)
    }
}

impl GestureGeometry {
    /// Start with unknown geometry in the presentation's validated pixel ratio.
    #[must_use]
    pub const fn new(pixel_ratio: DevicePixelRatio) -> Self {
        Self {
            pixel_ratio,
            mouse_double_click_area: None,
            mouse_drag_tolerance: None,
            touch_slop: None,
            touch_double_tap_slop: None,
            fling_speeds: None,
        }
    }

    /// Project a native mouse reading sampled for this exact presentation.
    ///
    /// Do not use the host's bootstrap reading as an exact metric at another DPI.
    /// This converts units only; double-click dimensions remain full dimensions,
    /// while drag tolerances remain half-extents.
    ///
    /// # Errors
    /// Refuses nonfinite division results.
    pub fn from_native_mouse(native: &NativeMouseGeometry) -> Result<Self, InvalidPreference> {
        let ratio = native.pixel_ratio;
        Self::new(ratio)
            .with_mouse_double_click_area(Size::new(
                ratio.to_logical(f64::from(native.double_click_area.width)),
                ratio.to_logical(f64::from(native.double_click_area.height)),
            ))?
            .with_mouse_drag_tolerance(Size::new(
                ratio.to_logical(f64::from(native.drag_tolerance.width)),
                ratio.to_logical(f64::from(native.drag_tolerance.height)),
            ))
    }

    /// Project a touch observation queried for this exact presentation context.
    ///
    /// # Errors
    /// Refuses nonfinite division results or a degenerate projected fling range.
    pub fn from_native_touch(native: &NativeTouchGeometry) -> Result<Self, InvalidPreference> {
        let ratio = native.pixel_ratio;
        Ok(Self::new(ratio)
            .with_touch_slop(Distance::new(
                ratio.to_logical(f64::from(native.touch_slop)),
            )?)
            .with_touch_double_tap_slop(Distance::new(
                ratio.to_logical(f64::from(native.double_tap_slop)),
            )?)
            .with_fling_speeds(FlingSpeeds::new(
                ratio.to_logical(f64::from(native.min_fling)),
                ratio.to_logical(f64::from(native.max_fling)),
            )?))
    }

    /// The native sampling context's device/logical pixel ratio.
    #[must_use]
    pub const fn pixel_ratio(self) -> DevicePixelRatio {
        self.pixel_ratio
    }

    /// Full logical dimensions of the mouse double-click rectangle.
    #[must_use]
    pub const fn mouse_double_click_area(self) -> Option<Size> {
        self.mouse_double_click_area
    }

    /// Record full logical dimensions, permitting zero independently per axis.
    ///
    /// # Errors
    /// Refuses negative or nonfinite dimensions.
    pub fn with_mouse_double_click_area(mut self, value: Size) -> Result<Self, InvalidPreference> {
        self.mouse_double_click_area = Some(checked_area(value)?);
        Ok(self)
    }

    /// Logical mouse displacement tolerated on each side of the origin.
    #[must_use]
    pub const fn mouse_drag_tolerance(self) -> Option<Size> {
        self.mouse_drag_tolerance
    }

    /// Record logical drag half-extents, permitting zero independently per axis.
    ///
    /// # Errors
    /// Refuses negative or nonfinite half-extents.
    pub fn with_mouse_drag_tolerance(mut self, value: Size) -> Result<Self, InvalidPreference> {
        self.mouse_drag_tolerance = Some(checked_area(value)?);
        Ok(self)
    }

    /// Observed touch slop in logical pixels.
    #[must_use]
    pub const fn touch_slop(self) -> Option<Distance> {
        self.touch_slop
    }

    /// Record the presentation's observed touch slop.
    #[must_use]
    pub const fn with_touch_slop(mut self, value: Distance) -> Self {
        self.touch_slop = Some(value);
        self
    }

    /// Observed distance between touch double taps, in logical pixels.
    #[must_use]
    pub const fn touch_double_tap_slop(self) -> Option<Distance> {
        self.touch_double_tap_slop
    }

    /// Record observed distance between touch double taps.
    #[must_use]
    pub const fn with_touch_double_tap_slop(mut self, value: Distance) -> Self {
        self.touch_double_tap_slop = Some(value);
        self
    }

    /// Observed fling range in logical pixels/second.
    #[must_use]
    pub const fn fling_speeds(self) -> Option<FlingSpeeds> {
        self.fling_speeds
    }

    /// Record observed fling speed bounds.
    #[must_use]
    pub const fn with_fling_speeds(mut self, value: FlingSpeeds) -> Self {
        self.fling_speeds = Some(value);
        self
    }
}

/// A failed presentation-specific preference query.
///
/// Unsupported observation is `Ok(None)`, rather than an error. On failure the
/// consumer preserves its last accepted projection and arranges a bounded retry.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PreferenceQueryError {
    /// The native window or its owner no longer exists.
    #[error("the preference query owner is unavailable")]
    Unavailable,
    /// The query was attempted outside the platform owner thread.
    #[error("the preference query requires the platform owner thread")]
    WrongThread,
    /// A numeric observation or projection failed validation.
    #[error(transparent)]
    Invalid(#[from] InvalidPreference),
    /// The native query failed.
    #[error("native preference query failed: {message}")]
    Native {
        /// Description from the failing native operation.
        message: String,
    },
}
