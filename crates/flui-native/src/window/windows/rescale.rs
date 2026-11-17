//! The size a Win32 window takes at a new DPI: the one that keeps its
//! content's logical size.
//!
//! Left to itself, the system scales a window's outer rectangle linearly when
//! its DPI changes (the default answer to `WM_GETDPISCALEDSIZE`). The frame
//! does not scale linearly: the caption and border metrics are rounded at
//! each DPI, so the content of a linearly scaled window comes out a few pixels
//! off its logical size, and the error grows with each change. The window
//! measures the frame at both DPIs instead and keeps the content.
//!
//! Each change rounds the content to whole device pixels, so a logical size
//! read back from the rounded pixels at every change would still walk by a
//! pixel over several changes (801 logical across 100%, 150%, 125% and back
//! comes home as 802). The window keeps the logical size it rescaled from
//! instead, for as long as its content is still that size in device pixels:
//! a resize by the user or a command changes the pixels, and the kept size
//! gives way to the new one.
//!
//! The pure half of the Win32 window: the frame at each DPI is an input (the
//! window reads it from `AdjustWindowRectExForDpi`), so every host runs the
//! tests; the window procedure answers the system with it (`proc`).

#![forbid(unsafe_code)]

use flui_foundation::geometry::{DevicePixelRatio, Size};

use super::limits::PixelSize;
use crate::window::state::device_extent;

/// A window's scale and the extent its frame adds around its content at that
/// scale, in device pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FramedScale {
    pub(crate) ratio: DevicePixelRatio,
    pub(crate) frame: PixelSize,
}

/// A window's outer size at a new scale, and the logical size of its content
/// there, which the window keeps for the next change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rescaled {
    pub(crate) outer: PixelSize,
    pub(crate) logical: Size<f64>,
}

/// The outer size of a window that measures `outer` with the frame and scale
/// of `from`, once it is at the scale of `to` with the frame it has there: its
/// content at the same logical size, by the hosts' one extent rule
/// ([`device_extent`]), and the frame of `to` around it.
///
/// The logical size is `kept`, the one the window rescaled from at its last
/// change, on each side where the content still has that size's pixels at
/// `from`; elsewhere it is the content's pixels over `from`'s scale.
///
/// `None` when the content is no size at `from` (an outer size within its
/// frame) or the result is past what Win32 holds: the system's own linear
/// scaling then stands.
pub(crate) fn rescaled_outer(
    outer: PixelSize,
    from: FramedScale,
    to: FramedScale,
    kept: Option<Size<f64>>,
) -> Option<Rescaled> {
    let side = |outer: i32, frame_from: i32, frame_to: i32, kept: Option<f64>| {
        let content = outer
            .checked_sub(frame_from)
            .filter(|content| *content > 0)?;
        let logical = kept
            .filter(|kept| {
                device_extent(*kept, from.ratio)
                    .is_some_and(|pixels| i64::from(pixels) == i64::from(content))
            })
            .unwrap_or_else(|| from.ratio.to_logical(f64::from(content)));
        let rescaled = i32::try_from(device_extent(logical, to.ratio)?).ok()?;
        Some((rescaled.checked_add(frame_to)?, logical))
    };
    let (width, logical_width) = side(
        outer.width,
        from.frame.width,
        to.frame.width,
        kept.map(|kept| kept.width),
    )?;
    let (height, logical_height) = side(
        outer.height,
        from.frame.height,
        to.frame.height,
        kept.map(|kept| kept.height),
    )?;
    Some(Rescaled {
        outer: PixelSize::new(width, height),
        logical: Size::new(logical_width, logical_height),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ratio(dpi: u32) -> DevicePixelRatio {
        DevicePixelRatio::try_new(f64::from(dpi) / 96.0).expect("a ratio a screen has")
    }

    /// What `AdjustWindowRectExForDpi` answers for `WS_OVERLAPPEDWINDOW`
    /// (Windows 11, build 26300): the extent its frame adds around the content
    /// at each DPI. Neither side scales linearly from 96: 16 wide there is 18,
    /// not 20, at 120.
    fn system_frame(dpi: u32) -> FramedScale {
        let frame = match dpi {
            96 => PixelSize::new(16, 39),
            120 => PixelSize::new(18, 47),
            144 => PixelSize::new(22, 56),
            168 => PixelSize::new(24, 64),
            _ => unreachable!("no frame measured at {dpi}"),
        };
        FramedScale {
            ratio: ratio(dpi),
            frame,
        }
    }

    /// The system's default: the outer size scaled linearly, rounded.
    fn linearly_scaled(outer: PixelSize, from: u32, to: u32) -> PixelSize {
        let scale = |side: i32| (f64::from(side) * f64::from(to) / f64::from(from)).round() as i32;
        PixelSize::new(scale(outer.width), scale(outer.height))
    }

    /// An 800×600 content at 96 DPI moved across 144, 168 and 120 DPI and
    /// back: the rescaled window holds 800×600 logical at every step, where
    /// the system's linear scaling of the outer size leaves the content off by
    /// pixels at the first step and further off on the way back.
    #[test]
    fn a_rescaled_window_keeps_its_content_where_linear_scaling_drifts() {
        let content_at = |outer: PixelSize, dpi: u32| {
            let frame = system_frame(dpi).frame;
            PixelSize::new(outer.width - frame.width, outer.height - frame.height)
        };
        let wanted = |dpi: u32| {
            let side = |logical: f64| {
                i32::try_from(device_extent(logical, ratio(dpi)).expect("a size"))
                    .expect("in range")
            };
            PixelSize::new(side(800.0), side(600.0))
        };
        let start = PixelSize::new(816, 639);
        let mut rescaled = start;
        let mut kept = None;
        let mut linear = start;
        let mut at = 96;
        let mut linear_drifted = false;
        for dpi in [144, 168, 120, 96] {
            let next = rescaled_outer(rescaled, system_frame(at), system_frame(dpi), kept)
                .expect("a window with content");
            (rescaled, kept) = (next.outer, Some(next.logical));
            linear = linearly_scaled(linear, at, dpi);
            at = dpi;
            assert_eq!(content_at(rescaled, dpi), wanted(dpi), "rescaled at {dpi}");
            linear_drifted |= content_at(linear, dpi) != wanted(dpi);
        }
        assert_eq!(rescaled, start, "back at 96 DPI, the window it started as");
        assert!(
            linear_drifted,
            "the system's linear scaling keeps this content too: the frames do not show the drift"
        );
    }

    /// Every content side from 300 to 3000 logical pixels, moved across 100%,
    /// 150%, 125% and back, comes home at the pixels it left: the window keeps
    /// the logical size across the changes. Read back from the rounded pixels
    /// at each change, 675 of those sides came home a pixel larger (801 as
    /// 802).
    #[test]
    fn a_kept_logical_size_comes_home_across_several_changes() {
        let frame = |dpi: u32| FramedScale {
            ratio: ratio(dpi),
            frame: PixelSize::new(0, 0),
        };
        let mut read_back_drifted = 0;
        for side in 300..=3000 {
            let start = PixelSize::new(side, side);
            let (mut kept_outer, mut kept) = (start, None);
            let mut read_back = start;
            let mut at = 96;
            for dpi in [144, 120, 96] {
                let next = rescaled_outer(kept_outer, frame(at), frame(dpi), kept)
                    .expect("a window with content");
                (kept_outer, kept) = (next.outer, Some(next.logical));
                read_back = rescaled_outer(read_back, frame(at), frame(dpi), None)
                    .expect("a window with content")
                    .outer;
                at = dpi;
            }
            assert_eq!(kept_outer, start, "{side} logical comes home");
            if read_back != start {
                read_back_drifted += 1;
            }
        }
        assert_eq!(read_back_drifted, 675, "the drift the kept size removes");
    }

    /// A resize between two changes gives the kept size way: the content's
    /// pixels at the old scale are no longer the kept size's, so the new
    /// content's own logical size is rescaled and kept.
    #[test]
    fn a_resize_gives_the_kept_logical_size_way() {
        let frame = |dpi: u32| FramedScale {
            ratio: ratio(dpi),
            frame: PixelSize::new(0, 0),
        };
        let kept = Some(Size::new(801.0, 600.0));
        let resized = rescaled_outer(PixelSize::new(1500, 900), frame(144), frame(96), kept)
            .expect("a window with content");
        assert_eq!(resized.outer, PixelSize::new(1000, 600));
        assert_eq!(resized.logical, Size::new(1000.0, 600.0));
    }

    #[test]
    fn a_window_with_no_content_or_past_win32_keeps_the_systems_answer() {
        let small = system_frame(96);
        let large = system_frame(144);
        for (name, outer) in [
            ("as wide as its frame", PixelSize::new(16, 639)),
            ("shorter than its frame", PixelSize::new(816, 20)),
            ("negative", PixelSize::new(-1, 639)),
        ] {
            assert_eq!(rescaled_outer(outer, small, large, None), None, "{name}");
        }
        let past = FramedScale {
            frame: PixelSize::new(i32::MAX, 0),
            ..large
        };
        assert_eq!(
            rescaled_outer(PixelSize::new(816, 639), small, past, None),
            None,
            "an outer size past i32"
        );
    }
}
