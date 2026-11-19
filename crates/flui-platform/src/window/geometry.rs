//! The geometry a window's snapshot holds, valid by construction (ADR-0254
//! §1).
//!
//! A [`WindowState`](crate::WindowState) describes what the window system
//! reports, and a report can be numbers no window has: a size divided by a
//! scale of zero, a rectangle a compositor sent as NaN. [`WindowExtent`],
//! [`WindowPosition`] and [`WindowGeometry`] cannot hold such numbers, so a
//! snapshot cannot either: a host that reads them builds the value with
//! `try_new`, and the report it cannot build one from is not a state change.
//! A snapshot with a NaN in it would also never equal itself, so every read
//! of the window would publish a new revision. The commands and the options
//! that ask a window for a size or a place carry the same types, so a number
//! no snapshot could hold is not a number a window can be asked for either.
//!
//! # Where a window is
//!
//! A position is always measured on one display, named by its
//! [`DisplayId`], in that display's logical pixels, from that display's
//! top-left corner ([`WindowPosition`], ADR-0364). There is no global logical
//! space: on a desktop of mixed scales, logical pixels of one display are not
//! logical pixels of another, so a position that names no display has no
//! fixed meaning.
//!
//! - A snapshot's outer position is measured on the display the window is
//!   on ([`WindowState::display`](crate::WindowState::display)): the one that
//!   holds the largest part of it. A window that spans displays, or hangs
//!   past the edge of its own, has coordinates outside that display's
//!   extent, negative ones included; they stay in that display's logical
//!   pixels.
//! - A display left of or above the primary one moves no window's
//!   coordinates: a position starts at its own display's corner, wherever
//!   the window system put that display in its desktop.
//! - A host that is not told where its windows are (a Wayland toplevel, a
//!   phone, a page's canvas) publishes no position and takes none
//!   ([`WindowCapabilities::observes_position`](crate::WindowCapabilities::observes_position),
//!   [`places_at_open`](crate::WindowCapabilities::places_at_open)).
//! - A position whose display has left the display snapshot is still the
//!   position it was: a window opened at one is placed by the window system,
//!   and a command that asks for one is refused with
//!   [`ApplyError::DisplayGone`](crate::ApplyError::DisplayGone). A
//!   snapshot keeps naming the display its window was on until the window
//!   system reports where the window went.

use flui_foundation::geometry::{Point, Size};

use crate::DisplayId;

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

/// The longest side, in device pixels, of a surface a window may ask a
/// renderer to draw: an upper bound, not a promise.
///
/// The renderer asks its GPU for textures no larger than this or the GPU's
/// own limit, whichever is smaller, and refuses a surface past the limit its
/// device took, which can be lower (the GLES 3.0 and WebGL2 fallback adapters
/// offer 8 192). A host clamps the window extents it hands its window system
/// to this bound, and a host that chooses the scale its buffer is drawn at
/// lowers the scale until the content fits it; a window between the device's
/// limit and this bound is still refused by the renderer.
pub const MAX_SURFACE_EXTENT: u32 = 16 * 1024;

impl WindowExtent {
    /// No extent in either dimension.
    pub const ZERO: Self = Self(Size::new(0.0, 0.0));

    /// `size` as a window extent.
    ///
    /// # Errors
    /// [`InvalidGeometry::Extent`] when a side is NaN, infinite or negative.
    pub const fn try_new(size: Size<f64>) -> Result<Self, InvalidGeometry> {
        const fn valid(side: f64) -> bool {
            side.is_finite() && side >= 0.0
        }
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
    pub(crate) const fn area(self) -> Result<Self, InvalidGeometry> {
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

/// Where a window's outer top-left corner is: a finite point in the logical
/// pixels of one display, measured from that display's top-left corner (the
/// module's "Where a window is" says what that means across displays).
///
/// The one shape of a window's place: what a snapshot publishes
/// ([`WindowGeometry::position`]), what
/// [`WindowCommand::SetPosition`](crate::WindowCommand::SetPosition) asks for
/// and what [`WindowOptions::position`](crate::WindowOptions::position) opens
/// at. A host converts it through that display's own scale and origin, never
/// through the window's, so the same position lands on the same spot of the
/// same display whichever display the window is on when it is asked.
///
/// ```
/// use flui_foundation::geometry::Point;
/// use flui_platform::{DisplayKey, DisplayRegistry, WindowPosition};
///
/// let display = DisplayRegistry::new().mint(&DisplayKey::Injected(1))?;
/// let at = WindowPosition::try_new(display, Point::new(-40.0, 12.0))?;
/// assert_eq!((at.display(), at.at()), (display, Point::new(-40.0, 12.0)));
/// assert!(WindowPosition::try_new(display, Point::new(f64::NAN, 0.0)).is_err());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowPosition {
    display: DisplayId,
    at: Point<f64>,
}

// Both coordinates are finite, never NaN, so equality is total.
impl Eq for WindowPosition {}

impl WindowPosition {
    /// The point `at` of `display`, in its logical pixels from its top-left
    /// corner.
    ///
    /// # Errors
    /// [`InvalidGeometry::Position`] when a coordinate is NaN or infinite.
    pub const fn try_new(display: DisplayId, at: Point<f64>) -> Result<Self, InvalidGeometry> {
        if at.x.is_finite() && at.y.is_finite() {
            Ok(Self { display, at })
        } else {
            Err(InvalidGeometry::Position { x: at.x, y: at.y })
        }
    }

    /// The display the position is measured on.
    #[must_use]
    pub const fn display(self) -> DisplayId {
        self.display
    }

    /// The point, in the display's logical pixels from its top-left corner.
    #[must_use]
    pub const fn at(self) -> Point<f64> {
        self.at
    }
}

/// A window rectangle: its outer top-left corner as a [`WindowPosition`],
/// where the window system reports one, and a [`WindowExtent`] in the logical
/// pixels of the window.
///
/// ```
/// use flui_foundation::geometry::{Point, Size};
/// use flui_platform::{DisplayKey, DisplayRegistry, WindowGeometry, WindowPosition};
///
/// let display = DisplayRegistry::new().mint(&DisplayKey::Injected(1))?;
/// let at = WindowPosition::try_new(display, Point::new(-40.0, 12.0))?;
/// let placed = WindowGeometry::try_new(Some(at), Size::new(800.0, 600.0))?;
/// assert_eq!(placed.position(), Some(at));
/// assert_eq!(placed.size(), Size::new(800.0, 600.0));
/// assert!(WindowGeometry::try_new(None, Size::new(f64::NAN, 1.0)).is_err());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowGeometry {
    position: Option<WindowPosition>,
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
    /// [`InvalidGeometry::Extent`] when a side is NaN, infinite or negative.
    pub const fn try_new(
        position: Option<WindowPosition>,
        size: Size<f64>,
    ) -> Result<Self, InvalidGeometry> {
        match WindowExtent::try_new(size) {
            Ok(extent) => Ok(Self::unplaced(extent).at(position)),
            Err(refused) => Err(refused),
        }
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
    #[must_use]
    pub const fn at(self, position: Option<WindowPosition>) -> Self {
        Self {
            position,
            extent: self.extent,
        }
    }

    /// The outer top-left corner, on the display it is measured on; `None`
    /// where the window system does not tell a window where it is (Wayland,
    /// Android, iOS, a page's canvas).
    #[must_use]
    pub const fn position(self) -> Option<WindowPosition> {
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
