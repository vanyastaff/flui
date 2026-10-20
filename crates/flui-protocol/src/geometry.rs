//! Logical geometry on the wire: a point and a rectangle in a window's surface coordinates.
//!
//! Logical pixels from the top-left of the window's drawing surface, the space input is
//! injected in and the render tree lays out in. [`Rect`](crate::Rect) stays the physical-pixel
//! rectangle of ADR-0080's `rect` and `surface_rect`.

/// A point in logical pixels from the top-left of a window's surface.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct LogicalPoint {
    /// Horizontal, rightwards.
    pub x: f64,
    /// Vertical, downwards.
    pub y: f64,
}

impl LogicalPoint {
    /// The point `(x, y)`.
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// A rectangle in logical pixels: origin and size.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct LogicalRect {
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
}

impl LogicalRect {
    /// The rectangle at `(x, y)` of `width` by `height`.
    #[must_use]
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// The rectangle's centre.
    #[must_use]
    pub fn center(&self) -> LogicalPoint {
        LogicalPoint::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

/// A size in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct LogicalSize {
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
}

impl LogicalSize {
    /// `width` by `height`.
    #[must_use]
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}
