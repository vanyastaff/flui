//! Where a window is on a desktop its window system lays out in device pixels
//! (Win32, X11): the one conversion between a [`WindowPosition`] and the
//! desktop's device pixels (ADR-0360).
//!
//! A position is a point of one display, in that display's logical pixels
//! from its top-left corner, so it converts through that display's own
//! origin and scale, never through the scale of the window that is asked to
//! move: the same position lands on the same device pixel whichever display
//! the window is on, and a position read back from the device pixel the
//! window is at is the position it was asked for. Pure, so a table of
//! desktops checks it on every host.

#![forbid(unsafe_code)]

use flui_foundation::geometry::{Bounds, DevicePixelRatio, Point, Size, snap};
use flui_platform::{DisplayId, DisplayInfo, DisplaySnapshot, WindowPosition};

/// One display of a device-pixel desktop: which display it is, where its
/// top-left corner sits in the desktop's device pixels, and how many device
/// pixels make one of its logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DisplayFrame {
    display: DisplayId,
    origin: Point<i32>,
    /// The display's extent in its logical pixels.
    extent: Size<f64>,
    scale: DevicePixelRatio,
}

impl DisplayFrame {
    /// The frame of `display`, whose origin its host published in the
    /// desktop's device pixels.
    pub(crate) fn of(display: &DisplayInfo) -> Self {
        Self {
            display: display.id,
            origin: display.origin.get(),
            extent: display.bounds.extent(),
            scale: display.scale_factor,
        }
    }

    /// The display's rectangle in the desktop's device pixels, as left, top,
    /// right and bottom: wide enough that no `i32` origin plus an extent
    /// overflows.
    fn edges(self) -> [i64; 4] {
        let side = |logical: f64| {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a display's extent over its scale is a whole number of pixels far inside i64"
            )]
            let pixels = snap(self.scale.to_device(logical)) as i64;
            pixels
        };
        let (x, y) = (i64::from(self.origin.x), i64::from(self.origin.y));
        [
            x,
            y,
            x + side(self.extent.width),
            y + side(self.extent.height),
        ]
    }

    /// The frame of display `id` in `snapshot`; `None` when the snapshot no
    /// longer holds it.
    pub(crate) fn find(snapshot: &DisplaySnapshot, id: DisplayId) -> Option<Self> {
        snapshot.get(id).map(Self::of)
    }

    /// The display.
    pub(crate) const fn display(self) -> DisplayId {
        self.display
    }

    /// The display's device pixels per logical pixel.
    pub(crate) const fn scale(self) -> DevicePixelRatio {
        self.scale
    }

    /// The desktop pixel `at` names on this display: its origin plus `at`
    /// at its scale, snapped to the pixel grid. `None` for a point past what
    /// the desktop's 32-bit coordinates hold, which a finite position far
    /// from every display can be: such a place is refused, never clamped.
    pub(crate) fn to_desktop(self, at: Point<f64>) -> Option<Point<i32>> {
        // A finite coordinate times a ratio of at most 32 can still overflow
        // to infinity, and an `i32` origin plus it can leave the range; both
        // are refused by the one range check.
        let pixel = |offset: f64, origin: i32| {
            let pixel = snap(self.scale.to_device(offset)) + f64::from(origin);
            (pixel >= f64::from(i32::MIN) && pixel <= f64::from(i32::MAX)).then(|| {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "a whole number checked inside the i32 range just above"
                )]
                let pixel = pixel as i32;
                pixel
            })
        };
        Some(Point::new(
            pixel(at.x, self.origin.x)?,
            pixel(at.y, self.origin.y)?,
        ))
    }

    /// The position of desktop pixel `desktop` on this display: the pixel
    /// less its origin, over its scale. The difference of two `i32`s is exact
    /// in `f64` and a ratio of at least 1/32 keeps the quotient finite, so
    /// every desktop pixel has a position; one left of or above the display,
    /// or past its far edge, is outside its extent and still in its logical
    /// pixels.
    pub(crate) fn position(self, desktop: Point<i32>) -> Option<WindowPosition> {
        let offset = |pixel: i32, origin: i32| {
            #[expect(
                clippy::cast_precision_loss,
                reason = "the difference of two i32s is at most 2^33, exact in an f64"
            )]
            let pixels = (i64::from(pixel) - i64::from(origin)) as f64;
            self.scale.to_logical(pixels)
        };
        WindowPosition::try_new(
            self.display,
            Point::new(
                offset(desktop.x, self.origin.x),
                offset(desktop.y, self.origin.y),
            ),
        )
        .ok()
    }
}

/// The display of `snapshot` that holds the largest part of `rect`, a
/// rectangle in the desktop's device pixels, or the nearest one to it when
/// it lies on none: the display a window there is on, as a window system
/// that names a monitor for a rectangle chooses it (Win32's
/// `MonitorFromRect` with `MONITOR_DEFAULTTONEAREST`). `None` for a snapshot
/// with no displays.
pub(crate) fn display_of_rect(
    snapshot: &DisplaySnapshot,
    rect: Bounds<i32>,
) -> Option<DisplayFrame> {
    let left = i64::from(rect.origin.x);
    let top = i64::from(rect.origin.y);
    let right = left + i64::from(rect.size.width.max(0));
    let bottom = top + i64::from(rect.size.height.max(0));
    let shared = |frame: &DisplayFrame| {
        let [l, t, r, b] = frame.edges();
        let width = (right.min(r) - left.max(l)).max(0);
        let height = (bottom.min(b) - top.max(t)).max(0);
        // Each side is below 2^34, so the product needs 128 bits, not 64.
        i128::from(width) * i128::from(height)
    };
    let distance = |frame: &DisplayFrame| {
        let [l, t, r, b] = frame.edges();
        let gap = |low: i64, high: i64, from: i64, to: i64| (from - high).max(low - to).max(0);
        let (dx, dy) = (gap(l, r, left, right), gap(t, b, top, bottom));
        i128::from(dx) * i128::from(dx) + i128::from(dy) * i128::from(dy)
    };
    // The first of equals either way, so the answer does not depend on how
    // a tie rounds.
    let mut most: Option<(i128, DisplayFrame)> = None;
    let mut nearest: Option<(i128, DisplayFrame)> = None;
    for frame in snapshot.displays().iter().map(DisplayFrame::of) {
        let area = shared(&frame);
        if area > 0 && most.is_none_or(|(best, _)| area > best) {
            most = Some((area, frame));
        }
        let gap = distance(&frame);
        if nearest.is_none_or(|(best, _)| gap < best) {
            nearest = Some((gap, frame));
        }
    }
    most.or(nearest).map(|(_, frame)| frame)
}

#[cfg(test)]
mod tests {

    use flui_platform::{DisplayKey, DisplayRegistry};

    use super::*;

    fn ratio(scale: f64) -> DevicePixelRatio {
        DevicePixelRatio::try_new(scale).expect("a ratio")
    }

    /// A primary display at scale 1 and, right of it, one at scale 2; a
    /// third, at 1.5, left of and above the primary.
    fn desktop() -> (DisplayFrame, DisplayFrame, DisplayFrame) {
        let registry = DisplayRegistry::new();
        let frame = |key, x, y, width, height, scale| {
            let id = registry.mint(&DisplayKey::Injected(key)).expect("an id");
            let bounds = Bounds::new(Point::new(x, y), Size::new(width, height));
            DisplayFrame::of(
                &DisplayInfo::from_device(id, "display", bounds, bounds, ratio(scale))
                    .expect("a display"),
            )
        };
        (
            frame(1, 0, 0, 1920, 1080, 1.0),
            frame(2, 1920, 0, 3840, 2160, 2.0),
            frame(3, -2400, -300, 2400, 1350, 1.5),
        )
    }

    /// The same position lands on the same desktop pixel whichever display
    /// a window is on when it is asked, and the pixel a window is at reads
    /// back as the position it was asked for: a position converts through
    /// its own display's origin and scale, never the window's.
    #[test]
    fn a_position_converts_through_its_own_display() {
        let (primary, hidpi, left) = desktop();
        // (case, display, position, desktop pixel)
        let rows = [
            (
                "the primary at scale one",
                primary,
                (100.0, 50.0),
                Some((100, 50)),
            ),
            (
                "a scale-two display right of it",
                hidpi,
                (40.0, 50.0),
                Some((2000, 100)),
            ),
            (
                "left of and above the primary at 1.5",
                left,
                (10.0, 10.0),
                Some((-2385, -285)),
            ),
            (
                "past its display's left edge, on the display beside",
                hidpi,
                (-200.0, 0.0),
                Some((1520, 0)),
            ),
            ("fractions snap", primary, (0.4, 0.5), Some((0, 1))),
            (
                "past the desktop's coordinate range",
                primary,
                (3.0e9, 0.0),
                None,
            ),
            ("scaled past every number", hidpi, (f64::MAX, 0.0), None),
            ("far left of every display", left, (-1.0e10, 0.0), None),
        ];
        for (case, frame, (x, y), expected) in rows {
            let pixel = frame.to_desktop(Point::new(x, y));
            assert_eq!(pixel.map(|pixel| (pixel.x, pixel.y)), expected, "{case}");
            if let Some(pixel) = pixel
                && x.fract() == 0.0
            {
                let back = frame.position(pixel).expect("every pixel has a position");
                assert_eq!(back.display(), frame.display(), "{case}");
                assert_eq!(back.at(), Point::new(x, y), "{case}: round trip");
            }
        }
    }

    /// A rectangle is on the display that holds the largest part of it, and
    /// on the nearest one when it lies on none.
    #[test]
    fn a_rectangle_is_on_the_display_holding_most_of_it() {
        let registry = DisplayRegistry::new();
        let display = |key, x, width, scale| {
            let id = registry.mint(&DisplayKey::Injected(key)).expect("an id");
            let bounds = Bounds::new(Point::new(x, 0), Size::new(width, 1080));
            DisplayInfo::from_device(id, "display", bounds, bounds, ratio(scale))
                .expect("a display")
        };
        let (primary, hidpi) = (display(1, 0, 1920, 1.0), display(2, 1920, 3840, 2.0));
        let (primary_id, hidpi_id) = (primary.id, hidpi.id);
        let snapshot = DisplaySnapshot::empty()
            .successor(vec![primary, hidpi])
            .expect("a snapshot");
        let rect = |x, width| Bounds::new(Point::new(x, 100), Size::new(width, 600));
        // (case, rectangle, display)
        let rows = [
            ("wholly on the primary", rect(100, 800), Some(primary_id)),
            ("mostly on the primary", rect(1500, 800), Some(primary_id)),
            ("mostly on the right", rect(1800, 800), Some(hidpi_id)),
            (
                "off every display, nearest the right",
                rect(9000, 800),
                Some(hidpi_id),
            ),
            (
                "off every display, nearest the left",
                rect(-5000, 800),
                Some(primary_id),
            ),
            (
                "an empty rectangle on the right",
                rect(2500, 0),
                Some(hidpi_id),
            ),
        ];
        for (case, rect, expected) in rows {
            assert_eq!(
                display_of_rect(&snapshot, rect).map(DisplayFrame::display),
                expected,
                "{case}"
            );
        }
        assert_eq!(
            display_of_rect(&DisplaySnapshot::empty(), rect(0, 10)),
            None,
            "no displays"
        );
    }

    /// A window at one desktop pixel reads as the same place on whichever
    /// display it is measured: the position on its own display, and the
    /// position on another that names that same pixel.
    #[test]
    fn a_window_spanning_two_displays_has_one_pixel_and_two_names() {
        let (primary, hidpi, _) = desktop();
        let corner = Point::new(1800, 40);
        let on_primary = primary.position(corner).expect("a position");
        let on_hidpi = hidpi.position(corner).expect("a position");
        assert_eq!(on_primary.at(), Point::new(1800.0, 40.0));
        assert_eq!(on_hidpi.at(), Point::new(-60.0, 20.0));
        assert_eq!(primary.to_desktop(on_primary.at()), Some(corner));
        assert_eq!(hidpi.to_desktop(on_hidpi.at()), Some(corner));
    }
}
