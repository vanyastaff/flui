//! Logical geometry on the wire: a point, a size, a rectangle and its insets in a window's surface
//! coordinates.
//!
//! Logical pixels from the top-left of the window's drawing surface, the space input is
//! injected in and the render tree lays out in. [`Rect`](crate::tree::Rect) stays the physical-pixel
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

    /// Whether `other`'s four edges each lie within `tolerance` logical pixels of this
    /// rectangle's, the boundary included: Flutter's `rectMoreOrLessEquals`, which compares the
    /// left, top, right and bottom edges.
    ///
    /// A non-finite edge on either side, or a tolerance that is negative or not a number, is
    /// close to nothing, so a rectangle laid out from a stand-in value never passes.
    #[must_use]
    pub fn approx_eq(&self, other: &Self, tolerance: f64) -> bool {
        let edges = |rect: &Self| [rect.x, rect.y, rect.x + rect.width, rect.y + rect.height];
        tolerance >= 0.0
            && edges(self)
                .into_iter()
                .zip(edges(other))
                .all(|(a, b)| a.is_finite() && b.is_finite() && (a - b).abs() <= tolerance)
    }
}

/// Insets on a rectangle's four edges, in logical pixels: what covers a window's surface, such as
/// its safe area.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct LogicalInsets {
    /// From the top edge, downwards.
    pub top: f64,
    /// From the right edge, leftwards.
    pub right: f64,
    /// From the bottom edge, upwards.
    pub bottom: f64,
    /// From the left edge, rightwards.
    pub left: f64,
}

impl LogicalInsets {
    /// `top`, `right`, `bottom` and `left`, in the order CSS names them.
    #[must_use]
    pub const fn new(top: f64, right: f64, bottom: f64, left: f64) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
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
