//! The soft keyboard over a window, from the frame its platform reports in
//! the window's coordinates (ADR-0277 §4): UIKit's
//! `UIKeyboardWillChangeFrameNotification`, which also carries the
//! animation's duration and `UIViewAnimationCurve`, and a browser's visual
//! viewport or `navigator.virtualKeyboard`.

#[cfg(any(target_os = "ios", test))]
use std::time::Duration;

use flui_foundation::geometry::{EdgeInsets, Size};
#[cfg(any(target_os = "ios", test))]
use flui_platform::{EventTime, InsetCurve, InsetMotion, KeyboardInset};

/// The keyboard's frame in the window's coordinates: its origin and size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Frame {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

/// How much of a view `view` in size the keyboard at `frame` covers at the
/// bottom: from the keyboard's top to the view's bottom where the two
/// overlap, nothing for a keyboard off screen, beside the view or floating
/// clear of it, and nothing for a frame that is no number.
#[must_use]
pub(crate) fn occlusion(view: Size<f64>, frame: Frame) -> EdgeInsets {
    let finite = [
        frame.x,
        frame.y,
        frame.width,
        frame.height,
        view.width,
        view.height,
    ]
    .iter()
    .all(|value| value.is_finite());
    let overlaps = frame.x < view.width
        && frame.x + frame.width > 0.0
        && frame.y < view.height
        && frame.y + frame.height > 0.0;
    if !finite || !overlaps || frame.width <= 0.0 || frame.height <= 0.0 {
        return EdgeInsets::ZERO;
    }
    EdgeInsets::new(0.0, 0.0, (view.height - frame.y.max(0.0)).max(0.0), 0.0)
}

#[cfg(any(target_os = "ios", test))]
/// The curve a raw `UIViewAnimationCurve` names: 7, the keyboard's own,
/// which no public constant names, and the four public ones as Core
/// Animation's named timing functions draw them. Any other value is taken
/// as the keyboard's, the only one UIKit sends it with today.
#[must_use]
pub(crate) fn curve_of(raw: isize) -> InsetCurve {
    match raw {
        // `UIViewAnimationCurveEaseInOut`: `kCAMediaTimingFunctionEaseInEaseOut`.
        0 => InsetCurve::CubicBezier {
            x1: 0.42,
            y1: 0.0,
            x2: 0.58,
            y2: 1.0,
        },
        // `UIViewAnimationCurveEaseIn`.
        1 => InsetCurve::CubicBezier {
            x1: 0.42,
            y1: 0.0,
            x2: 1.0,
            y2: 1.0,
        },
        // `UIViewAnimationCurveEaseOut`.
        2 => InsetCurve::CubicBezier {
            x1: 0.0,
            y1: 0.0,
            x2: 0.58,
            y2: 1.0,
        },
        3 => InsetCurve::Linear,
        _ => InsetCurve::SystemKeyboard,
    }
}

#[cfg(any(target_os = "ios", test))]
/// One change of the keyboard's frame as UIKit announced it: where the
/// keyboard was when it began, when that was, and how it moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Change {
    pub(crate) from: EdgeInsets,
    pub(crate) start: EventTime,
    /// `UIKeyboardAnimationDurationUserInfoKey`, in seconds.
    pub(crate) seconds: f64,
    pub(crate) curve: InsetCurve,
}

#[cfg(any(target_os = "ios", test))]
impl Change {
    /// The keyboard that covers `target` once this change ends: moving
    /// toward it while the change lasts, at rest for a change with no
    /// duration (a keyboard that appears without animation) or one that
    /// does not make a valid motion.
    #[must_use]
    pub(crate) fn keyboard(self, target: EdgeInsets) -> Option<KeyboardInset> {
        let duration = Duration::try_from_secs_f64(self.seconds)
            .ok()
            .filter(|duration| !duration.is_zero());
        let moving = duration
            .and_then(|duration| {
                InsetMotion::try_new(self.from, self.start, duration, self.curve).ok()
            })
            .and_then(|motion| KeyboardInset::try_moving(target, motion).ok());
        moving.or_else(|| KeyboardInset::try_at_rest(target).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: Size<f64> = Size::new(390.0, 844.0);

    fn frame(x: f64, y: f64, width: f64, height: f64) -> Frame {
        Frame {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn the_keyboard_covers_the_bottom_it_overlaps() {
        let bottom = |frame| occlusion(VIEW, frame).bottom;
        // Docked at the bottom.
        assert_eq!(bottom(frame(0.0, 508.0, 390.0, 336.0)), 336.0);
        // Hidden: its end frame lies below the screen.
        assert_eq!(bottom(frame(0.0, 844.0, 390.0, 336.0)), 0.0);
        // Beside a Split View window.
        assert_eq!(bottom(frame(400.0, 508.0, 390.0, 336.0)), 0.0);
        // Taller than the view, from above it.
        assert_eq!(bottom(frame(0.0, -10.0, 390.0, 900.0)), 844.0);
        // Nothing for frames that are no number or have no area.
        assert_eq!(bottom(frame(f64::NAN, 508.0, 390.0, 336.0)), 0.0);
        assert_eq!(bottom(frame(0.0, 508.0, 0.0, 336.0)), 0.0);
    }

    #[test]
    fn a_raw_curve_names_its_timing_function() {
        assert_eq!(curve_of(7), InsetCurve::SystemKeyboard);
        assert_eq!(curve_of(3), InsetCurve::Linear);
        assert_eq!(
            curve_of(0),
            InsetCurve::CubicBezier {
                x1: 0.42,
                y1: 0.0,
                x2: 0.58,
                y2: 1.0
            }
        );
        assert_eq!(curve_of(-1), InsetCurve::SystemKeyboard);
    }

    #[test]
    fn a_change_moves_the_keyboard_while_it_lasts() {
        let target = EdgeInsets::new(0.0, 0.0, 336.0, 0.0);
        let change = Change {
            from: EdgeInsets::ZERO,
            start: EventTime::from_nanos(1_000),
            seconds: 0.25,
            curve: InsetCurve::SystemKeyboard,
        };
        let keyboard = change.keyboard(target).expect("a valid keyboard");
        let motion = keyboard.motion().expect("a keyboard in motion");
        assert_eq!(motion.from(), EdgeInsets::ZERO);
        assert_eq!(motion.duration(), Duration::from_millis(250));
        assert_eq!(keyboard.at(EventTime::from_nanos(1_000)), EdgeInsets::ZERO);
        assert_eq!(
            keyboard.at(EventTime::from_nanos(1_000 + 250_000_000)),
            target
        );
        // No duration, or one that is no number: at rest at the target.
        for seconds in [0.0, f64::NAN, -1.0] {
            let rest = Change { seconds, ..change }
                .keyboard(target)
                .expect("a valid keyboard");
            assert_eq!(rest.motion(), None, "{seconds}");
            assert_eq!(rest.target(), target);
        }
    }
}
