//! The geometry a window's snapshot holds, valid by construction (ADR-0254
//! §1).
//!
//! A [`WindowState`](crate::WindowState) describes what the window system
//! reports, and a report can be numbers no window has: a size divided by a
//! scale of zero, a rectangle a compositor sent as NaN. [`WindowExtent`] and
//! [`WindowGeometry`] cannot hold such numbers, so a snapshot cannot either:
//! a host that reads them builds the value with `try_new`, and the report it
//! cannot build one from is not a state change. A snapshot with a NaN in it
//! would also never equal itself, so every read of the window would publish
//! a new revision.

use flui_foundation::geometry::{Point, Size};

/// Why numbers describe no window.
///
/// Two errors are equal when they carry the same numbers bit for bit, so an
/// error read from a NaN equals itself.
#[derive(Debug, Clone, Copy, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidGeometry {
    /// A coordinate of the position is NaN or infinite.
    #[error("window position ({x}, {y}) is not finite")]
    Position {
        /// The reported horizontal coordinate.
        x: f64,
        /// The reported vertical coordinate.
        y: f64,
    },
    /// A side of the extent is NaN, infinite or negative.
    #[error("window extent {width}x{height} is not finite and non-negative")]
    Extent {
        /// The reported width.
        width: f64,
        /// The reported height.
        height: f64,
    },
    /// A size asked of a window, or a limit on it, encloses no area: a side
    /// is zero. A window system may report a zero extent (a window not
    /// mapped yet), but a window is never asked to take one.
    #[error("window size {width}x{height} encloses no area")]
    Empty {
        /// The asked width.
        width: f64,
        /// The asked height.
        height: f64,
    },
    /// The minimum size exceeds the maximum in a dimension.
    #[error(
        "the minimum size {}x{} exceeds the maximum {}x{}",
        min.width(),
        min.height(),
        max.width(),
        max.height()
    )]
    Limits {
        /// The asked minimum.
        min: WindowExtent,
        /// The asked maximum.
        max: WindowExtent,
    },
}

impl PartialEq for InvalidGeometry {
    fn eq(&self, other: &Self) -> bool {
        let same = |a: f64, b: f64| a.to_bits() == b.to_bits();
        match (*self, *other) {
            (Self::Position { x, y }, Self::Position { x: x2, y: y2 }) => {
                same(x, x2) && same(y, y2)
            }
            (
                Self::Extent { width, height },
                Self::Extent {
                    width: width2,
                    height: height2,
                },
            )
            | (
                Self::Empty { width, height },
                Self::Empty {
                    width: width2,
                    height: height2,
                },
            ) => same(width, width2) && same(height, height2),
            (
                Self::Limits { min, max },
                Self::Limits {
                    min: min2,
                    max: max2,
                },
            ) => min == min2 && max == max2,
            _ => false,
        }
    }
}

impl Eq for InvalidGeometry {}

/// The extent of a window, of its content or of a limit on it, in logical
/// pixels: each side is finite and not negative.
///
/// Zero is an extent: a window that is not mapped yet has one.
///
/// ```
/// use flui_foundation::geometry::Size;
/// use flui_platform::WindowExtent;
///
/// let extent = WindowExtent::try_new(Size::new(800.0, 600.0))?;
/// assert_eq!(extent.width(), 800.0);
/// assert_eq!(extent, Size::new(800.0, 600.0));
/// assert!(WindowExtent::try_new(Size::new(f64::INFINITY, 600.0)).is_err());
/// assert!(WindowExtent::try_new(Size::new(800.0, f64::NAN)).is_err());
/// assert!(WindowExtent::try_new(Size::new(-1.0, 600.0)).is_err());
/// # Ok::<(), flui_platform::InvalidGeometry>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowExtent(Size<f64>);

impl WindowExtent {
    /// No extent in either dimension.
    pub const ZERO: Self = Self(Size::new(0.0, 0.0));

    /// `size` as a window extent.
    ///
    /// # Errors
    /// [`InvalidGeometry::Extent`] when a side is NaN, infinite or negative.
    pub fn try_new(size: Size<f64>) -> Result<Self, InvalidGeometry> {
        let valid = |side: f64| side.is_finite() && side >= 0.0;
        if valid(size.width) && valid(size.height) {
            Ok(Self(size))
        } else {
            Err(InvalidGeometry::Extent {
                width: size.width,
                height: size.height,
            })
        }
    }

    /// This extent, when it encloses an area: what a size asked of a window,
    /// or a limit on it, must be.
    ///
    /// # Errors
    /// [`InvalidGeometry::Empty`] when a side is zero.
    pub(crate) fn area(self) -> Result<Self, InvalidGeometry> {
        if self.0.width > 0.0 && self.0.height > 0.0 {
            Ok(self)
        } else {
            Err(InvalidGeometry::Empty {
                width: self.0.width,
                height: self.0.height,
            })
        }
    }

    /// The extent as a size.
    #[must_use]
    pub const fn get(self) -> Size<f64> {
        self.0
    }

    /// The horizontal side.
    #[must_use]
    pub const fn width(self) -> f64 {
        self.0.width
    }

    /// The vertical side.
    #[must_use]
    pub const fn height(self) -> f64 {
        self.0.height
    }
}

// Each side is finite, never NaN, so equality is total.
impl Eq for WindowExtent {}

impl Default for WindowExtent {
    fn default() -> Self {
        Self::ZERO
    }
}

impl From<WindowExtent> for Size<f64> {
    fn from(extent: WindowExtent) -> Self {
        extent.0
    }
}

impl PartialEq<Size<f64>> for WindowExtent {
    fn eq(&self, other: &Size<f64>) -> bool {
        self.0 == *other
    }
}

impl PartialEq<WindowExtent> for Size<f64> {
    fn eq(&self, other: &WindowExtent) -> bool {
        *self == other.0
    }
}

/// A window rectangle in logical pixels: a finite position, where the window
/// system reports one, and a [`WindowExtent`].
///
/// ```
/// use flui_foundation::geometry::{Point, Size};
/// use flui_platform::WindowGeometry;
///
/// let placed = WindowGeometry::try_new(Some(Point::new(-40.0, 12.0)), Size::new(800.0, 600.0))?;
/// assert_eq!(placed.position(), Some(Point::new(-40.0, 12.0)));
/// assert_eq!(placed.size(), Size::new(800.0, 600.0));
/// assert!(WindowGeometry::try_new(Some(Point::new(f64::NAN, 0.0)), Size::new(1.0, 1.0)).is_err());
/// # Ok::<(), flui_platform::InvalidGeometry>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowGeometry {
    position: Option<Point<f64>>,
    extent: WindowExtent,
}

// The position and the extent are finite, never NaN, so equality is total.
impl Eq for WindowGeometry {}

impl WindowGeometry {
    /// No position and no extent: what a window holds before its window
    /// system has reported a rectangle.
    pub const EMPTY: Self = Self::unplaced(WindowExtent::ZERO);

    /// A rectangle with this position and size.
    ///
    /// # Errors
    /// [`InvalidGeometry::Position`] when a coordinate is NaN or infinite,
    /// [`InvalidGeometry::Extent`] when a side is NaN, infinite or negative.
    pub fn try_new(position: Option<Point<f64>>, size: Size<f64>) -> Result<Self, InvalidGeometry> {
        Self::unplaced(WindowExtent::try_new(size)?).at(position)
    }

    /// A rectangle of this extent whose position the window system does not
    /// report.
    #[must_use]
    pub const fn unplaced(extent: WindowExtent) -> Self {
        Self {
            position: None,
            extent,
        }
    }

    /// This rectangle at `position`; `None` for one whose position is not
    /// reported.
    ///
    /// # Errors
    /// [`InvalidGeometry::Position`] when a coordinate is NaN or infinite.
    pub fn at(self, position: Option<Point<f64>>) -> Result<Self, InvalidGeometry> {
        if let Some(at) = position
            && !(at.x.is_finite() && at.y.is_finite())
        {
            return Err(InvalidGeometry::Position { x: at.x, y: at.y });
        }
        Ok(Self {
            position,
            extent: self.extent,
        })
    }

    /// The top-left corner in global screen coordinates; `None` where the OS
    /// reports no global position (Wayland, Android, iOS).
    #[must_use]
    pub const fn position(self) -> Option<Point<f64>> {
        self.position
    }

    /// The extent, as a size.
    #[must_use]
    pub const fn size(self) -> Size<f64> {
        self.extent.get()
    }

    /// The extent.
    #[must_use]
    pub const fn extent(self) -> WindowExtent {
        self.extent
    }
}
