//! Whether the windows above a window leave any of it on screen: the pure
//! half of the Win32 covering walk (`cover`), which feeds it the monitors'
//! rectangles and the opaque windows over the target, top first, as the
//! desktop's z-order lists them.
//!
//! The walk is Chromium's `WindowOcclusionCalculator`: what is left of the
//! target's content on the monitors, with each window above it subtracted.
//! A window none of whose content is left on any monitor is covered, which
//! includes one moved off every monitor.

#![forbid(unsafe_code)]

/// A rectangle of the virtual screen in device pixels, half-open: `right`
/// and `bottom` are past its last pixel, as a Win32 `RECT` is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScreenRect {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
}

impl ScreenRect {
    /// Whether the rectangle holds no pixel.
    fn is_empty(self) -> bool {
        self.left >= self.right || self.top >= self.bottom
    }

    /// The pixels both rectangles hold, if any.
    fn intersection(self, other: Self) -> Option<Self> {
        let common = Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        };
        (!common.is_empty()).then_some(common)
    }

    /// What is left of this rectangle once `cut` is taken out of it, as at
    /// most four disjoint rectangles: the bands above and below the cut, the
    /// width of this one, and the parts left and right of it between them.
    fn subtract(self, cut: Self, left_over: &mut Vec<Self>) {
        let Some(cut) = self.intersection(cut) else {
            left_over.push(self);
            return;
        };
        let bands = [
            Self {
                bottom: cut.top,
                ..self
            },
            Self {
                top: cut.bottom,
                ..self
            },
            Self {
                top: cut.top,
                bottom: cut.bottom,
                right: cut.left,
                ..self
            },
            Self {
                top: cut.top,
                bottom: cut.bottom,
                left: cut.right,
                ..self
            },
        ];
        left_over.extend(bands.into_iter().filter(|band| !band.is_empty()));
    }
}

/// What the windows above a window leave of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Coverage {
    /// Some of its content is on screen.
    Seen,
    /// None of it is: the windows above cover what of it is on the monitors.
    Covered,
}

/// How many pieces the uncovered part of a window may break into before the
/// walk stops and reads it seen: a window under a few hundred small windows
/// is not worth the subtraction, and reading it seen keeps its frames.
const MAX_PIECES: usize = 256;

/// What `above`, the opaque windows over `target` with the topmost first,
/// leave of `target` on `screens`, the monitors' rectangles. Seen once more
/// than [`MAX_PIECES`] pieces of it are left, the answer that never stops a
/// frame a user could see.
pub(crate) fn coverage(
    screens: &[ScreenRect],
    above: impl IntoIterator<Item = ScreenRect>,
    target: ScreenRect,
) -> Coverage {
    let mut left: Vec<ScreenRect> = screens
        .iter()
        .filter_map(|screen| screen.intersection(target))
        .collect();
    let mut next = Vec::with_capacity(left.len());
    for window in above {
        if left.is_empty() {
            break;
        }
        next.clear();
        for piece in &left {
            piece.subtract(window, &mut next);
        }
        std::mem::swap(&mut left, &mut next);
        if left.len() > MAX_PIECES {
            return Coverage::Seen;
        }
    }
    if left.is_empty() {
        Coverage::Covered
    } else {
        Coverage::Seen
    }
}

#[cfg(test)]
mod tests {
    use super::{Coverage, MAX_PIECES, ScreenRect, coverage};

    const fn rect(left: i32, top: i32, right: i32, bottom: i32) -> ScreenRect {
        ScreenRect {
            left,
            top,
            right,
            bottom,
        }
    }

    /// The walk reads a window covered exactly when the windows above it
    /// leave none of its content on any monitor: one window over all of it,
    /// several that tile it between them, or no monitor under it at all.
    /// Red-check: answer `Seen` while pieces are left after the first
    /// window only, and the tiled rows read covered.
    #[test]
    fn a_window_is_covered_once_the_windows_above_leave_none_of_it_on_screen() {
        let one = [rect(0, 0, 1920, 1080)];
        let two = [rect(0, 0, 1920, 1080), rect(1920, 0, 3840, 1080)];
        let inside = rect(100, 100, 900, 700);
        let edge = rect(1800, 100, 2000, 700);
        let across = rect(1800, 100, 2100, 700);
        let nowhere = rect(-900, -900, -100, -100);
        // (case, monitors, target, windows above it top first, expected)
        let table: &[(&str, &[ScreenRect], ScreenRect, &[ScreenRect], Coverage)] = &[
            ("nothing above", &one, inside, &[], Coverage::Seen),
            (
                "one window over all of it",
                &one,
                inside,
                &[rect(0, 0, 1000, 800)],
                Coverage::Covered,
            ),
            (
                "one window over its exact rectangle",
                &one,
                inside,
                &[inside],
                Coverage::Covered,
            ),
            (
                "one pixel column left",
                &one,
                inside,
                &[rect(101, 0, 1000, 800)],
                Coverage::Seen,
            ),
            (
                "two windows tiling it",
                &one,
                inside,
                &[rect(0, 0, 500, 800), rect(500, 0, 1000, 800)],
                Coverage::Covered,
            ),
            (
                "four windows leaving a hole in the middle",
                &one,
                inside,
                &[
                    rect(0, 0, 1000, 300),
                    rect(0, 500, 1000, 800),
                    rect(0, 300, 400, 500),
                    rect(600, 300, 1000, 500),
                ],
                Coverage::Seen,
            ),
            (
                "a window beside it",
                &one,
                inside,
                &[rect(1000, 0, 1900, 1000)],
                Coverage::Seen,
            ),
            (
                "the part off the monitors needs no cover",
                &one,
                edge,
                &[rect(1800, 100, 1920, 700)],
                Coverage::Covered,
            ),
            ("off every monitor", &one, nowhere, &[], Coverage::Covered),
            (
                "across two monitors, covered on one",
                &two,
                across,
                &[rect(1800, 0, 1920, 1080)],
                Coverage::Seen,
            ),
            (
                "across two monitors, covered on both",
                &two,
                across,
                &[rect(1800, 0, 1920, 1080), rect(1920, 0, 2100, 1080)],
                Coverage::Covered,
            ),
        ];
        for (case, screens, target, above, expected) in table {
            assert_eq!(
                coverage(screens, above.iter().copied(), *target),
                *expected,
                "{case}"
            );
        }
    }

    /// A window broken into more pieces than the walk keeps reads seen, the
    /// side that never stops a frame a user could see; extreme coordinates
    /// do not overflow.
    #[test]
    fn a_window_too_fragmented_to_follow_reads_seen() {
        let screen = [rect(i32::MIN, i32::MIN, i32::MAX, i32::MAX)];
        let target = rect(0, 0, 10_000, 10);
        // One-pixel windows a pixel apart split the strip into one piece per
        // gap, past the bound.
        let pins = (0..=MAX_PIECES as i32).map(|x| rect(2 * x, 0, 2 * x + 1, 10));
        assert_eq!(coverage(&screen, pins, target), Coverage::Seen);
        assert_eq!(
            coverage(
                &screen,
                [rect(i32::MIN, i32::MIN, i32::MAX, i32::MAX)],
                target
            ),
            Coverage::Covered
        );
    }
}
