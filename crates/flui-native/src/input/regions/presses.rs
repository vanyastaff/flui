//! What a press does on a window whose application draws the title bar, on a
//! host that keeps no frame around it (ADR-0286 §9): winit, whose window has
//! no decorations then. The host answers each press from the chrome regions
//! the last composited frame handed it and from a resize band of its own along
//! the window's edges, and moves, resizes or maximizes the window through the
//! window system, which then owns the pointer until the release. A press on
//! the bar waits for the pointer to travel before the move starts, so a
//! double-click reaches the window.

#![forbid(unsafe_code)]

use std::time::{Duration, Instant};

use flui_foundation::geometry::{DevicePoint, DeviceSize};
use flui_platform::input_regions::{ChromeHit, DeviceInputRegion, ResizeEdge, chrome_hit};

/// The longest wait between the two presses of a double-click on the bar.
/// winit reads no system setting for it; 500 ms is the default of Windows and
/// GNOME alike.
pub(crate) const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);

/// How far, in logical pixels, the second press of a double-click may land from
/// the first, and a press on the bar may travel before it moves the window.
pub(crate) const DOUBLE_CLICK_SLOP: f64 = 4.0;

/// The thickness, in logical pixels, of the band along a frameless window's
/// edges a press resizes it from.
pub(crate) const RESIZE_BAND: f64 = 6.0;

/// Which button pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PressButton {
    /// The primary button: a move, a resize or a double-click.
    Primary,
    /// The secondary button: the window menu over the bar.
    Secondary,
}

/// What a press does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChromePress {
    /// It reaches the application.
    App,
    /// The press is the window's: the window system moves the window once the
    /// pointer travels ([`ChromePresses::moved`]).
    Move,
    /// The window system resizes the window from the edge.
    Resize(ResizeEdge),
    /// The second press of a double-click on the bar: the window maximizes, or
    /// a maximized one restores.
    ToggleMaximize,
    /// The window system's menu for the window opens at the press.
    WindowMenu,
}

/// The band along a window's edges, in device pixels, that resizes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResizeBand {
    size: DeviceSize,
    thickness: i32,
}

impl ResizeBand {
    /// The band of a window of `size` device pixels at `scale`, or `None` for one
    /// that cannot be resized by the user now: not `resizable`, maximized, full
    /// screen, or a scale that is no positive finite number.
    pub(crate) fn of(size: DeviceSize, scale: f64, resizable_now: bool) -> Option<Self> {
        let thickness = (RESIZE_BAND * scale).ceil();
        (resizable_now && thickness.is_finite() && thickness >= 1.0).then(|| Self {
            size,
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a finite band of a few logical pixels at a window's scale fits i32"
            )]
            thickness: thickness.min(f64::from(i32::MAX)) as i32,
        })
    }

    /// The edge `point` resizes from, `None` inside the band. A corner reaches
    /// twice the band along each of its edges, so it is not a single pixel to aim
    /// for.
    fn edge(self, point: DevicePoint) -> Option<ResizeEdge> {
        let Self { size, thickness } = self;
        let corner = thickness.saturating_mul(2);
        let near_start = |at: i32, reach: i32| at < reach;
        let near_end = |at: i32, extent: i32, reach: i32| at >= extent.saturating_sub(reach);
        let left = near_start(point.x, thickness);
        let right = near_end(point.x, size.width, thickness);
        let top = near_start(point.y, thickness);
        let bottom = near_end(point.y, size.height, thickness);
        if !(left || right || top || bottom) {
            return None;
        }
        let left = near_start(point.x, corner);
        let right = !left && near_end(point.x, size.width, corner);
        let top = near_start(point.y, corner);
        let bottom = !top && near_end(point.y, size.height, corner);
        Some(match (top, bottom, left, right) {
            (true, _, true, _) => ResizeEdge::TopLeft,
            (true, _, _, true) => ResizeEdge::TopRight,
            (_, true, true, _) => ResizeEdge::BottomLeft,
            (_, true, _, true) => ResizeEdge::BottomRight,
            // Off the corners, the band the point is in names the edge.
            _ if point.y < thickness => ResizeEdge::Top,
            _ if point.y >= size.height.saturating_sub(thickness) => ResizeEdge::Bottom,
            _ if point.x < thickness => ResizeEdge::Left,
            _ => ResizeEdge::Right,
        })
    }
}

/// A window's presses on its chrome: the last press on the bar, which the next
/// one may make a double-click of, and the press on the bar the window has not
/// moved for yet.
///
/// A press on the bar moves the window only once the pointer travels past
/// [`DOUBLE_CLICK_SLOP`] while held ([`moved`](Self::moved)), as the system's own
/// title bars do: a window manager handed the move at the press holds the
/// pointer from then on, so a second click of a double-click would end its move
/// instead of reaching the window.
#[derive(Debug, Default)]
pub(crate) struct ChromePresses {
    last_move: Option<(Instant, DevicePoint)>,
    armed: Option<DevicePoint>,
}

impl ChromePresses {
    /// What a press of `button` at `point` (window device pixels, from the content's
    /// top-left corner) at `at` does, against `regions` and the window's resize
    /// `band`, the band above any region; a second press within [`DOUBLE_CLICK_SLOP`]
    /// of the first at the window's `scale` makes a double-click.
    pub(crate) fn press(
        &mut self,
        regions: &[DeviceInputRegion],
        point: DevicePoint,
        button: PressButton,
        band: Option<ResizeBand>,
        at: Instant,
        scale: f64,
    ) -> ChromePress {
        let last_move = self.last_move.take();
        self.armed = None;
        let slop = double_click_slop(scale);
        if let Some(edge) = band.and_then(|band| band.edge(point)) {
            return match button {
                PressButton::Primary => ChromePress::Resize(edge),
                PressButton::Secondary => ChromePress::App,
            };
        }
        match (chrome_hit(regions, point), button) {
            (ChromeHit::Move, PressButton::Primary) => {
                let second = last_move.is_some_and(|(then, there)| {
                    at.saturating_duration_since(then) <= DOUBLE_CLICK_INTERVAL
                        && within(point, there, slop)
                });
                if second {
                    ChromePress::ToggleMaximize
                } else {
                    self.last_move = Some((at, point));
                    self.armed = Some(point);
                    ChromePress::Move
                }
            }
            (ChromeHit::Move, PressButton::Secondary) => ChromePress::WindowMenu,
            (ChromeHit::Resize(edge), PressButton::Primary) => ChromePress::Resize(edge),
            // The caption buttons are the application's own buttons here: no window
            // system around the window draws or answers them.
            _ => ChromePress::App,
        }
    }

    /// The pointer is at `point` with a press on the bar held: whether the window
    /// system should move the window now, which it does once per press, when the
    /// pointer has travelled past [`DOUBLE_CLICK_SLOP`] at `scale`. A travel this
    /// far is no double-click either.
    pub(crate) fn moved(&mut self, point: DevicePoint, scale: f64) -> bool {
        let Some(from) = self.armed else {
            return false;
        };
        if within(point, from, double_click_slop(scale)) {
            return false;
        }
        self.armed = None;
        self.last_move = None;
        true
    }

    /// The press on the bar was released.
    pub(crate) fn released(&mut self) {
        self.armed = None;
    }
}

/// Whether `point` lies within `slop` device pixels of `from` on both axes.
fn within(point: DevicePoint, from: DevicePoint, slop: u32) -> bool {
    point.x.abs_diff(from.x) <= slop && point.y.abs_diff(from.y) <= slop
}

/// [`DOUBLE_CLICK_SLOP`] in device pixels at `scale`, none for a scale that is no
/// positive finite number.
fn double_click_slop(scale: f64) -> u32 {
    let slop = (DOUBLE_CLICK_SLOP * scale).ceil();
    if slop.is_finite() && slop > 0.0 {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a positive slop of a few logical pixels at a window's scale fits u32"
        )]
        let slop = slop.min(f64::from(u32::MAX)) as u32;
        slop
    } else {
        0
    }
}

/// The device pixel a window-physical position lies in, `None` when it is not
/// finite or past `i32`.
pub(crate) fn device_point(x: f64, y: f64) -> Option<DevicePoint> {
    let pixel = |value: f64| {
        let floor = value.floor();
        (floor.is_finite() && floor >= f64::from(i32::MIN) && floor <= f64::from(i32::MAX)).then(
            || {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "the floor is finite and checked to lie in i32's range"
                )]
                let pixel = floor as i32;
                pixel
            },
        )
    };
    Some(DevicePoint::new(pixel(x)?, pixel(y)?))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use flui_foundation::geometry::{DevicePoint, DeviceRect, DeviceSize};
    use flui_platform::input_regions::{DeviceInputRegion, InputRegionKind, ResizeEdge};

    use super::{ChromePress, ChromePresses, PressButton, ResizeBand, device_point};

    fn region(kind: InputRegionKind, x: i32, y: i32, width: i32, height: i32) -> DeviceInputRegion {
        DeviceInputRegion::new(
            kind,
            DeviceRect::from_min_max(
                DevicePoint::new(x, y),
                DevicePoint::new(x + width, y + height),
            ),
        )
    }

    /// A 400 × 300 window at scale 1 with a bar along its top and a button in it.
    fn bar() -> Vec<DeviceInputRegion> {
        vec![
            region(InputRegionKind::WindowMove, 0, 0, 400, 32),
            region(InputRegionKind::WindowContent, 300, 0, 32, 32),
        ]
    }

    fn band() -> Option<ResizeBand> {
        ResizeBand::of(DeviceSize::new(400, 300), 1.0, true)
    }

    #[test]
    fn a_press_on_a_frameless_window_moves_resizes_or_reaches_the_app() {
        use PressButton::{Primary, Secondary};
        // (point, button, band, expected)
        let cases = [
            ((100, 16), Primary, band(), ChromePress::Move),
            ((310, 16), Primary, band(), ChromePress::App),
            ((100, 16), Secondary, band(), ChromePress::WindowMenu),
            ((100, 200), Primary, band(), ChromePress::App),
            (
                (200, 2),
                Primary,
                band(),
                ChromePress::Resize(ResizeEdge::Top),
            ),
            (
                (2, 2),
                Primary,
                band(),
                ChromePress::Resize(ResizeEdge::TopLeft),
            ),
            (
                (10, 2),
                Primary,
                band(),
                ChromePress::Resize(ResizeEdge::TopLeft),
            ),
            (
                (398, 2),
                Primary,
                band(),
                ChromePress::Resize(ResizeEdge::TopRight),
            ),
            (
                (2, 150),
                Primary,
                band(),
                ChromePress::Resize(ResizeEdge::Left),
            ),
            (
                (398, 150),
                Primary,
                band(),
                ChromePress::Resize(ResizeEdge::Right),
            ),
            (
                (200, 297),
                Primary,
                band(),
                ChromePress::Resize(ResizeEdge::Bottom),
            ),
            (
                (2, 297),
                Primary,
                band(),
                ChromePress::Resize(ResizeEdge::BottomLeft),
            ),
            (
                (398, 297),
                Primary,
                band(),
                ChromePress::Resize(ResizeEdge::BottomRight),
            ),
            ((200, 2), Secondary, band(), ChromePress::App),
            // Maximized, full screen or fixed in size: no band, the bar moves it.
            ((200, 2), Primary, None, ChromePress::Move),
            ((2, 150), Primary, None, ChromePress::App),
        ];
        for (index, ((x, y), button, band, expected)) in cases.into_iter().enumerate() {
            let mut presses = ChromePresses::default();
            let got = presses.press(
                &bar(),
                DevicePoint::new(x, y),
                button,
                band,
                Instant::now(),
                1.0,
            );
            assert_eq!(got, expected, "case {index}: ({x}, {y}) {button:?}");
        }
    }

    #[test]
    fn a_second_press_on_the_bar_soon_and_near_toggles_maximize() {
        let start = Instant::now();
        let soon = start + Duration::from_millis(300);
        let late = start + Duration::from_millis(800);
        // (second press point, its time, expected)
        let cases = [
            ((102, 17), soon, ChromePress::ToggleMaximize),
            ((120, 17), soon, ChromePress::Move),
            ((102, 17), late, ChromePress::Move),
            ((310, 16), soon, ChromePress::App),
        ];
        for (index, ((x, y), at, expected)) in cases.into_iter().enumerate() {
            let mut presses = ChromePresses::default();
            let first = presses.press(
                &bar(),
                DevicePoint::new(100, 16),
                PressButton::Primary,
                band(),
                start,
                1.0,
            );
            assert_eq!(first, ChromePress::Move, "case {index}");
            let second = presses.press(
                &bar(),
                DevicePoint::new(x, y),
                PressButton::Primary,
                band(),
                at,
                1.0,
            );
            assert_eq!(second, expected, "case {index}");
        }

        // A third press starts over: it moves the window again.
        let mut presses = ChromePresses::default();
        let point = DevicePoint::new(100, 16);
        let press = |presses: &mut ChromePresses, at| {
            presses.press(&bar(), point, PressButton::Primary, band(), at, 1.0)
        };
        assert_eq!(press(&mut presses, start), ChromePress::Move);
        assert_eq!(press(&mut presses, soon), ChromePress::ToggleMaximize);
        assert_eq!(
            press(&mut presses, soon + Duration::from_millis(100)),
            ChromePress::Move
        );
    }

    #[test]
    fn a_press_on_the_bar_moves_the_window_once_the_pointer_travels() {
        let start = Instant::now();
        let held = |presses: &mut ChromePresses| {
            presses.press(
                &bar(),
                DevicePoint::new(100, 16),
                PressButton::Primary,
                band(),
                start,
                1.0,
            )
        };
        let mut presses = ChromePresses::default();
        assert_eq!(held(&mut presses), ChromePress::Move);
        assert!(
            !presses.moved(DevicePoint::new(103, 18), 1.0),
            "within the slop"
        );
        assert!(presses.moved(DevicePoint::new(110, 16), 1.0), "past it");
        assert!(
            !presses.moved(DevicePoint::new(140, 16), 1.0),
            "once per press"
        );
        // A press that moved the window makes no double-click of the next.
        assert_eq!(held(&mut presses), ChromePress::Move);
        presses.released();
        assert!(!presses.moved(DevicePoint::new(140, 16), 1.0), "released");

        // The slop grows with the scale, and a press elsewhere arms nothing.
        let mut presses = ChromePresses::default();
        assert_eq!(held(&mut presses), ChromePress::Move);
        assert!(!presses.moved(DevicePoint::new(107, 16), 2.0));
        let mut presses = ChromePresses::default();
        let content = presses.press(
            &bar(),
            DevicePoint::new(100, 200),
            PressButton::Primary,
            band(),
            start,
            1.0,
        );
        assert_eq!(content, ChromePress::App);
        assert!(!presses.moved(DevicePoint::new(140, 200), 1.0));
    }
    #[test]
    fn the_band_and_the_point_refuse_what_is_no_pixel() {
        assert_eq!(ResizeBand::of(DeviceSize::new(400, 300), 1.0, false), None);
        assert_eq!(
            ResizeBand::of(DeviceSize::new(400, 300), f64::NAN, true),
            None
        );
        assert_eq!(ResizeBand::of(DeviceSize::new(400, 300), 0.0, true), None);
        assert_eq!(device_point(1.5, 2.9), Some(DevicePoint::new(1, 2)));
        assert_eq!(device_point(f64::INFINITY, 2.0), None);
        assert_eq!(device_point(1.0, 1e12), None);
    }
}
