//! Where an app's window sits on a macOS desktop, in the units the driver maps with, and which
//! window the window server shows at a point.
//!
//! Every source the lane reads speaks one space: global display points with the origin at the top
//! left of the main display and y growing downwards (the window list's bounds, the accessibility
//! API's positions, a Quartz event's location). A display left of or above the main one has
//! negative coordinates; nothing is flipped on the way in. What macOS does flip is where a
//! window's content sits in its frame: AppKit lays a window out from the bottom left, so the
//! content view is flush with the frame's bottom and left edges and the title bar takes what is
//! left above it ([`content_origin`]); a full-size content view fills the frame and leaves no
//! title bar.
//!
//! A window's logical pixels are its points, and its scale is its display's backing scale (2 on
//! a Retina display), which the app reports. The driver maps a surface point to the device pixel
//! `floor((origin + x) * scale)` (`coordinates::to_device`); the lane posts at the centre of the
//! part of that pixel inside the content, back in points ([`points_of`]), which lies in the same
//! backing pixel and inside the window on every display.

use flui_protocol::geometry::{LogicalPoint, LogicalSize};

use crate::coordinates::{DevicePoint, WindowGeometry};
use crate::error::{Error, Result};
use crate::lane::macos::pointer::Point;
use crate::pixels::Image;

/// A rectangle in global display points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointRect {
    /// The left edge.
    pub x: f64,
    /// The top edge.
    pub y: f64,
    /// The width.
    pub width: f64,
    /// The height.
    pub height: f64,
}

impl PointRect {
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

    /// Whether `point` is inside, its right and bottom edges excluded.
    #[must_use]
    pub fn contains(&self, point: Point) -> bool {
        (self.x..self.x + self.width).contains(&point.x)
            && (self.y..self.y + self.height).contains(&point.y)
    }
}

/// How far a window frame's width may be from the app's logical width: the window server rounds
/// a frame to whole points.
const SIZE_TOLERANCE: f64 = 1.0;

/// The tallest title bar and toolbar a frame may have above the content and still be the app's
/// window of that size: a window whose frame is much taller is another window.
const MAX_CHROME: f64 = 160.0;

/// The top-left of the content of a window whose frame is `frame`, when the content is `size`:
/// see the [module docs](self).
#[must_use]
pub fn content_origin(frame: PointRect, size: LogicalSize) -> Point {
    Point::new(frame.x, frame.y + frame.height - size.height)
}

/// The content of a window whose frame is `frame`, when the content is `size`, in global points.
#[must_use]
pub fn content_rect(frame: PointRect, size: LogicalSize) -> PointRect {
    let origin = content_origin(frame, size);
    PointRect::new(origin.x, origin.y, size.width, size.height)
}

/// Whether a window whose frame is `frame` can hold the app's content of `size`: as wide, and as
/// tall or taller by no more than a title bar and a toolbar.
#[must_use]
pub fn is_window(frame: PointRect, size: LogicalSize) -> bool {
    let chrome = frame.height - size.height;
    (frame.width - size.width).abs() <= SIZE_TOLERANCE
        && (-SIZE_TOLERANCE..=MAX_CHROME).contains(&chrome)
}

/// The geometry the driver maps a window's points with: its content at `content` in global
/// points, the app's logical `size`, at `scale`.
#[must_use]
pub fn window_geometry(content: Point, size: LogicalSize, scale: f64) -> WindowGeometry {
    WindowGeometry::new(LogicalPoint::new(content.x, content.y), size, scale)
}

/// Where the lane posts an event the driver aimed at device pixel `device` at `scale`, in global
/// points: the centre of the part of that pixel inside `content`, the window's content. A content
/// edge at a fraction of a pixel cuts the pixel, and its own centre may lie outside the content.
///
/// # Errors
///
/// `invalid_argument` for a scale that is not positive and finite; `outside_target` for a pixel
/// that has no part inside the content.
pub fn points_of(device: DevicePoint, scale: f64, content: PointRect) -> Result<Point> {
    if !(scale.is_finite() && scale > 0.0) {
        return Err(Error::invalid_argument(format!(
            "the window's scale {scale} is not a positive number"
        )));
    }
    let axis = |pixel: i32, start: f64, length: f64| -> Option<f64> {
        let low = (f64::from(pixel) / scale).max(start);
        let high = ((f64::from(pixel) + 1.0) / scale).min(start + length);
        (low < high).then_some(low + (high - low) / 2.0)
    };
    match (
        axis(device.x, content.x, content.width),
        axis(device.y, content.y, content.height),
    ) {
        (Some(x), Some(y)) => Ok(Point::new(x, y)),
        _ => Err(Error::outside_target(format!(
            "device pixel ({}, {}) at scale {scale} is outside the window's content; nothing was \
             sent there",
            device.x, device.y
        ))),
    }
}

/// The device pixel a point in global points covers at `scale`, for the rectangles an
/// accessibility read reports; `None` when it is not finite or past what an `i32` addresses.
#[must_use]
pub fn device_of(point: Point, scale: f64) -> Option<DevicePoint> {
    let axis = |value: f64| -> Option<i32> {
        let device = (value * scale).floor();
        let fits =
            device.is_finite() && device >= f64::from(i32::MIN) && device <= f64::from(i32::MAX);
        fits.then(|| {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a whole number checked inside i32's range"
            )]
            let device = device as i32;
            device
        })
    };
    Some(DevicePoint::new(axis(point.x)?, axis(point.y)?))
}

/// The window level of an ordinary app window (`kCGNormalWindowLevel`).
pub const NORMAL_LEVEL: i32 = 0;

/// One window as the window server lists it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct WindowInfo {
    /// Its number (`kCGWindowNumber`).
    pub number: u32,
    /// The process that owns it.
    pub pid: i32,
    /// Its level: [`NORMAL_LEVEL`] for an app's window, higher for the Dock, the menu bar and
    /// panels that float.
    pub level: i32,
    /// Its frame in global points.
    pub bounds: PointRect,
    /// Its opacity, 0 to 1.
    pub alpha: f64,
    /// Its owner's name, for people.
    pub owner: String,
}

impl WindowInfo {
    /// A window `number` of `pid` at `level`, framed by `bounds`, of opacity `alpha`, its owner
    /// named `owner`.
    #[must_use]
    pub fn new(
        number: u32,
        pid: i32,
        level: i32,
        bounds: PointRect,
        alpha: f64,
        owner: impl Into<String>,
    ) -> Self {
        Self {
            number,
            pid,
            level,
            bounds,
            alpha,
            owner: owner.into(),
        }
    }

    /// The window for people: its owner and number.
    #[must_use]
    pub fn describe(&self) -> String {
        format!("{} (window {})", self.owner, self.number)
    }
}

/// The window the window server shows at `point` among `windows`, listed front to back: the first
/// that covers it and is not fully transparent.
#[must_use]
pub fn window_at(windows: &[WindowInfo], point: Point) -> Option<&WindowInfo> {
    windows
        .iter()
        .find(|window| window.alpha > 0.0 && window.bounds.contains(point))
}

/// The frontmost ordinary window among `windows`, listed front to back.
#[must_use]
pub fn front_window(windows: &[WindowInfo]) -> Option<&WindowInfo> {
    windows
        .iter()
        .find(|window| window.level == NORMAL_LEVEL && window.alpha > 0.0)
}

/// `capture`, the whole frame of a window whose content is `size` at `scale`, cut to the content:
/// the bottom rows and left columns, where AppKit lays the content out.
///
/// # Errors
///
/// `invalid_argument` when the capture is smaller than the content.
pub fn crop_content(capture: &Image, size: LogicalSize, scale: f64) -> Result<Image> {
    let device = |logical: f64| -> Option<u32> {
        let value = (logical * scale).round();
        (value.is_finite() && value >= 0.0 && value <= f64::from(u32::MAX)).then(|| {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a whole, non-negative number checked inside u32's range"
            )]
            let value = value as u32;
            value
        })
    };
    let (Some(width), Some(height)) = (device(size.width), device(size.height)) else {
        return Err(Error::invalid_argument(format!(
            "a {} x {} content at scale {scale} has no size in device pixels",
            size.width, size.height
        )));
    };
    if width > capture.width() || height > capture.height() {
        return Err(Error::invalid_argument(format!(
            "the window's capture of {} x {} is smaller than its {width} x {height} content",
            capture.width(),
            capture.height()
        )));
    }
    let top = capture.height() - height;
    let row = |y: u32| -> Result<&[u8]> {
        let start = (u64::from(y) * u64::from(capture.width())) * 4;
        let end = start + u64::from(width) * 4;
        let range = usize::try_from(start)
            .ok()
            .zip(usize::try_from(end).ok())
            .ok_or_else(|| Error::invalid_argument("the capture does not fit in memory"))?;
        capture
            .rgba()
            .get(range.0..range.1)
            .ok_or_else(|| Error::invalid_argument("the capture holds fewer rows than it says"))
    };
    let mut rgba = Vec::with_capacity(
        usize::try_from(u64::from(width) * u64::from(height) * 4)
            .map_err(|_| Error::invalid_argument("the content does not fit in memory"))?,
    );
    for y in top..capture.height() {
        rgba.extend_from_slice(row(y)?);
    }
    Image::try_new(width, height, rgba)
}
