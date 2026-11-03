//! The frame of a Win32 window whose application draws its title bar
//! (`WindowFrame::AppTitleBar`, ADR-0286): what `WM_NCCALCSIZE` leaves of the
//! system's frame, what `WM_NCHITTEST` answers from the chrome regions the last
//! composited frame handed the window.
//!
//! The pure half of the Win32 window: the rules need no Win32 call, so every
//! host runs their tests. The window procedure reads the system's structures
//! and metrics and writes the answers back (`proc`).

#![forbid(unsafe_code)]
// Elsewhere than on Windows this module is compiled for its tests alone.
#![cfg_attr(
    not(windows),
    allow(
        dead_code,
        reason = "off Windows the frame rules are compiled for their tests"
    )
)]

use flui_foundation::geometry::DevicePoint;
use flui_platform::input_regions::{
    CaptionButton, ChromeHit, DeviceInputRegion, ResizeEdge, chrome_hit,
};

/// The `WM_NCHITTEST` answers this module gives (`winuser.h`).
pub(crate) mod hit {
    pub(crate) const CLIENT: u32 = 1;
    pub(crate) const CAPTION: u32 = 2;
    pub(crate) const MIN_BUTTON: u32 = 8;
    pub(crate) const MAX_BUTTON: u32 = 9;
    pub(crate) const LEFT: u32 = 10;
    pub(crate) const RIGHT: u32 = 11;
    pub(crate) const TOP: u32 = 12;
    pub(crate) const TOP_LEFT: u32 = 13;
    pub(crate) const TOP_RIGHT: u32 = 14;
    pub(crate) const BOTTOM: u32 = 15;
    pub(crate) const BOTTOM_LEFT: u32 = 16;
    pub(crate) const BOTTOM_RIGHT: u32 = 17;
    pub(crate) const CLOSE: u32 = 20;
}

/// The `WM_NCHITTEST` answer for what the chrome regions say lies under a point.
///
/// A caption button answers as the system's own (`HTMAXBUTTON` is what shows
/// Windows 11's snap layouts over it); its pointer messages still reach the
/// application's button (`input::windows::caption`).
pub(crate) const fn hit_code(hit: ChromeHit) -> u32 {
    match hit {
        ChromeHit::Move => hit::CAPTION,
        ChromeHit::Resize(edge) => match edge {
            ResizeEdge::Top => hit::TOP,
            ResizeEdge::Bottom => hit::BOTTOM,
            ResizeEdge::Left => hit::LEFT,
            ResizeEdge::Right => hit::RIGHT,
            ResizeEdge::TopLeft => hit::TOP_LEFT,
            ResizeEdge::TopRight => hit::TOP_RIGHT,
            ResizeEdge::BottomLeft => hit::BOTTOM_LEFT,
            ResizeEdge::BottomRight => hit::BOTTOM_RIGHT,
            _ => hit::CLIENT,
        },
        ChromeHit::CaptionButton(button) => match button {
            CaptionButton::Minimize => hit::MIN_BUTTON,
            CaptionButton::Maximize => hit::MAX_BUTTON,
            CaptionButton::Close => hit::CLOSE,
            _ => hit::CLIENT,
        },
        // `Content`, and any hit a later contract adds: the application's.
        _ => hit::CLIENT,
    }
}

/// What `WM_NCHITTEST` answers for `point`, in client device pixels, of a point
/// the system's own frame left to the client.
///
/// `top_band` is the height of the resize band along the top of a window whose
/// caption `WM_NCCALCSIZE` removed, `None` where the window has none (it is
/// maximized, not resizable, or has no frame): the top border went with the
/// caption, so the band stands in for it, its ends the top corners, above any
/// region. Below it, the regions decide ([`chrome_hit`]).
pub(crate) fn client_hit_code(
    regions: &[DeviceInputRegion],
    point: DevicePoint,
    client_width: i32,
    top_band: Option<i32>,
) -> u32 {
    if let Some(band) = top_band.filter(|band| *band > 0)
        && (0..band).contains(&point.y)
    {
        return if point.x < band {
            hit::TOP_LEFT
        } else if point.x >= client_width.saturating_sub(band) {
            hit::TOP_RIGHT
        } else {
            hit::TOP
        };
    }
    hit_code(chrome_hit(regions, point))
}

/// A rectangle's edges in device pixels, as `RECT` holds them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Edges {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
}

/// The client rectangle `WM_NCCALCSIZE` gives a window whose application draws
/// its title bar.
///
/// `proposed` is the window rectangle the message proposed, `defaulted` the
/// client rectangle the default procedure made of it. The side and bottom
/// borders stay the system's (they resize the window and carry its shadow);
/// the caption and the top border go, so the client reaches the window's top.
/// A maximized window hangs its frame off the monitor by `maximized_inset`, so
/// its client starts that far down and stays on the monitor.
pub(crate) const fn app_title_bar_client(
    proposed: Edges,
    defaulted: Edges,
    maximized_inset: Option<i32>,
) -> Edges {
    let inset = match maximized_inset {
        Some(inset) => inset,
        None => 0,
    };
    Edges {
        top: proposed.top.saturating_add(inset),
        ..defaulted
    }
}

/// The outer rectangle's extent beyond a client area, from the one
/// `AdjustWindowRectEx` computed for the window's style.
///
/// A window whose application draws its title bar has no caption or top border
/// ([`app_title_bar_client`]), so nothing lies above its client.
pub(crate) const fn outer_extent(adjusted: Edges, app_title_bar: bool) -> Edges {
    if app_title_bar {
        Edges { top: 0, ..adjusted }
    } else {
        adjusted
    }
}

#[cfg(test)]
mod tests {
    use flui_foundation::geometry::DeviceRect;
    use flui_platform::input_regions::InputRegionKind;

    use super::*;

    fn region(
        kind: InputRegionKind,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    ) -> DeviceInputRegion {
        DeviceInputRegion::new(
            kind,
            DeviceRect::from_min_max(DevicePoint::new(left, top), DevicePoint::new(right, bottom)),
        )
    }

    /// A 400-pixel-wide bar 32 high with a field at x 100 and a maximize button
    /// at x 340.
    fn bar() -> Vec<DeviceInputRegion> {
        vec![
            region(InputRegionKind::WindowMove, 0, 0, 400, 32),
            region(InputRegionKind::WindowContent, 100, 4, 200, 28),
            region(
                InputRegionKind::CaptionButton(CaptionButton::Maximize),
                340,
                0,
                370,
                32,
            ),
        ]
    }

    #[test]
    fn the_hit_test_answers_from_the_regions_below_the_top_band() {
        let regions = bar();
        let at = |x, y, band| client_hit_code(&regions, DevicePoint::new(x, y), 400, band);
        for (name, x, y, band, code) in [
            ("bar", 50, 16, Some(6), hit::CAPTION),
            ("field", 150, 16, Some(6), hit::CLIENT),
            ("maximize", 350, 16, Some(6), hit::MAX_BUTTON),
            ("content", 50, 100, Some(6), hit::CLIENT),
            ("top band", 150, 2, Some(6), hit::TOP),
            ("top-left corner", 3, 2, Some(6), hit::TOP_LEFT),
            ("top-right corner", 396, 2, Some(6), hit::TOP_RIGHT),
            (
                "maximized: no band, the bar reaches the edge",
                50,
                0,
                None,
                hit::CAPTION,
            ),
            (
                "maximized: the button reaches the edge",
                350,
                0,
                None,
                hit::MAX_BUTTON,
            ),
            ("below the band", 50, 6, Some(6), hit::CAPTION),
        ] {
            assert_eq!(at(x, y, band), code, "{name}");
        }
    }

    #[test]
    fn every_chrome_hit_has_its_win32_answer() {
        for (hit, code) in [
            (ChromeHit::Content, hit::CLIENT),
            (ChromeHit::Move, hit::CAPTION),
            (ChromeHit::Resize(ResizeEdge::Top), hit::TOP),
            (ChromeHit::Resize(ResizeEdge::Bottom), hit::BOTTOM),
            (ChromeHit::Resize(ResizeEdge::Left), hit::LEFT),
            (ChromeHit::Resize(ResizeEdge::Right), hit::RIGHT),
            (ChromeHit::Resize(ResizeEdge::TopLeft), hit::TOP_LEFT),
            (ChromeHit::Resize(ResizeEdge::TopRight), hit::TOP_RIGHT),
            (ChromeHit::Resize(ResizeEdge::BottomLeft), hit::BOTTOM_LEFT),
            (
                ChromeHit::Resize(ResizeEdge::BottomRight),
                hit::BOTTOM_RIGHT,
            ),
            (
                ChromeHit::CaptionButton(CaptionButton::Minimize),
                hit::MIN_BUTTON,
            ),
            (
                ChromeHit::CaptionButton(CaptionButton::Maximize),
                hit::MAX_BUTTON,
            ),
            (ChromeHit::CaptionButton(CaptionButton::Close), hit::CLOSE),
        ] {
            assert_eq!(hit_code(hit), code, "{hit:?}");
        }
    }

    #[test]
    fn the_client_reaches_the_top_and_a_maximized_one_stays_on_the_monitor() {
        let proposed = Edges {
            left: -8,
            top: -8,
            right: 1928,
            bottom: 1048,
        };
        let defaulted = Edges {
            left: 0,
            top: 23,
            right: 1920,
            bottom: 1040,
        };
        assert_eq!(
            app_title_bar_client(proposed, defaulted, None),
            Edges {
                top: -8,
                ..defaulted
            }
        );
        assert_eq!(
            app_title_bar_client(proposed, defaulted, Some(8)),
            Edges {
                top: 0,
                ..defaulted
            }
        );
    }

    #[test]
    fn nothing_lies_above_the_client_of_an_app_title_bar() {
        let adjusted = Edges {
            left: -8,
            top: -31,
            right: 8,
            bottom: 8,
        };
        assert_eq!(outer_extent(adjusted, false), adjusted);
        assert_eq!(outer_extent(adjusted, true), Edges { top: 0, ..adjusted });
    }
}
