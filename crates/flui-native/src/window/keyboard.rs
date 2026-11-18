//! The soft keyboard over a window, from the frame its platform reports in
//! the window's coordinates (ADR-0277 §4, ADR-0371): UIKit's
//! `UIKeyboardWillChangeFrameNotification`, `UIKeyboardWillShowNotification`
//! and `UIKeyboardWillHideNotification`, which also carry the animation's
//! duration and `UIViewAnimationCurve`, and a browser's visual viewport or
//! `navigator.virtualKeyboard`.

#[cfg(any(target_os = "ios", test))]
use std::time::Duration;

use flui_foundation::geometry::{EdgeInsets, Rect, Size};
use flui_platform::KeyboardPlacement;
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

#[cfg(any(target_os = "ios", test))]
impl Frame {
    /// UIKit's `CGRectZero`: the end frame it sends while the user drags a
    /// floating keyboard, which says nothing of where the keyboard ends up.
    const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };
}

/// Where the keyboard at `frame` rests over a view `view` in size, `floor`
/// being the bottom a docked keyboard reaches in the view's coordinates: the
/// view's own bottom on iOS (an iPad reports a Slide Over window's keyboard
/// ending at the window's bottom); on the Web the layout viewport's bottom,
/// or the canvas's where it ends above that (a canvas taller than the layout
/// viewport goes on below a docked keyboard). The keyboard is
///
/// - hidden for a frame that is no number, has no area, or overlaps the view
///   nowhere (off screen, or beside the view in an iPad's Split View);
/// - floating for one whose bottom lies above `floor` (an iPad's floating,
///   undocked or split keyboard, or a shortcuts bar dragged up): it covers
///   what lies under its frame and no edge;
/// - docked otherwise, covering the view from the keyboard's top down.
///
/// The bottoms are compared rounded to the point, since a keyboard extension
/// can report a docked frame a fraction of a point off.
#[must_use]
pub(crate) fn placement(view: Size<f64>, frame: Frame, floor: f64) -> KeyboardPlacement {
    let finite = [
        frame.x,
        frame.y,
        frame.width,
        frame.height,
        view.width,
        view.height,
        floor,
    ]
    .iter()
    .all(|value| value.is_finite());
    let overlaps = frame.x < view.width
        && frame.x + frame.width > 0.0
        && frame.y < view.height
        && frame.y + frame.height > 0.0;
    if !finite || !overlaps || frame.width <= 0.0 || frame.height <= 0.0 {
        return KeyboardPlacement::Hidden;
    }
    if (frame.y + frame.height).round() < floor.round() {
        return KeyboardPlacement::Floating {
            frame: Rect::from_xywh(frame.x, frame.y, frame.width, frame.height),
        };
    }
    KeyboardPlacement::Docked {
        inset: EdgeInsets::new(0.0, 0.0, (view.height - frame.y.max(0.0)).max(0.0), 0.0),
    }
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
/// One change of the keyboard as UIKit announced it: where its inset was
/// when it began, when that was, and how it moves.
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
    /// The keyboard at `placement` once this change ends: its inset moving
    /// there while the change lasts, at rest for a change with no duration
    /// (a keyboard that appears without animation) or one that does not
    /// make a valid motion. `None` for a placement the platform's types
    /// refuse.
    #[must_use]
    pub(crate) fn keyboard(self, placement: KeyboardPlacement) -> Option<KeyboardInset> {
        let duration = Duration::try_from_secs_f64(self.seconds)
            .ok()
            .filter(|duration| !duration.is_zero());
        let moving = duration
            .and_then(|duration| {
                InsetMotion::try_new(self.from, self.start, duration, self.curve).ok()
            })
            .and_then(|motion| KeyboardInset::try_moving(placement, motion).ok());
        moving.or_else(|| KeyboardInset::try_at_rest(placement).ok())
    }
}

#[cfg(any(target_os = "ios", test))]
/// Which of UIKit's keyboard notifications a [`Note`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Notice {
    /// `UIKeyboardWillChangeFrameNotification`: the keyboard moves to its end
    /// frame.
    FrameChange,
    /// `UIKeyboardWillShowNotification`: the keyboard comes to its end frame.
    /// UIKit's zero frame here is a shortcuts bar dropped where the user
    /// dragged it: it floats, and UIKit does not say where.
    Show,
    /// `UIKeyboardWillHideNotification`, which UIKit posts after the hide's
    /// frame change: the keyboard leaves the content, whatever its end frame
    /// says (one not yet fully off screen is a hide all the same).
    Hide,
}

#[cfg(any(target_os = "ios", test))]
/// A keyboard notification as a view keeps it: which one, its end frame in
/// screen coordinates if it carried one, and the change it starts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Note {
    pub(crate) notice: Notice,
    /// `UIKeyboardFrameEndUserInfoKey`.
    pub(crate) end: Option<Frame>,
    pub(crate) change: Change,
}

#[cfg(any(target_os = "ios", test))]
impl Note {
    /// The note a view keeps once this one arrives while `standing` stands:
    /// this one, unless it says nothing of where the keyboard ends up, and
    /// then `standing` stays. A hide always says. A frame change or a show
    /// that names no end frame is not the keyboard's. A frame change that
    /// names UIKit's zero frame is the one it sends while the user drags a
    /// floating keyboard. A show that names the end frame of the frame
    /// change standing is the second notice of one show, and its motion is
    /// already under way.
    #[must_use]
    pub(crate) fn replacing(self, standing: Option<Self>) -> Option<Self> {
        let says_nothing = match (self.notice, self.end) {
            (Notice::Hide, _) => false,
            (Notice::FrameChange | Notice::Show, None) => true,
            (Notice::FrameChange, Some(end)) => end == Frame::ZERO,
            (Notice::Show, Some(_)) => standing.is_some_and(|standing| {
                standing.notice != Notice::Hide && standing.end == self.end
            }),
        };
        if says_nothing { standing } else { Some(self) }
    }

    /// The keyboard this note puts over a view `view` in size, `to_view`
    /// taking its end frame from screen coordinates into the view's: hidden
    /// for a hide, the end frame's [`placement`] for a frame change or a
    /// show. A show with UIKit's zero frame is a keyboard that floats where
    /// UIKit does not say: it covers no edge and has no frame to publish, so
    /// its placement is hidden, as any frame with no area is. `None` for a
    /// placement the platform's types refuse.
    #[must_use]
    pub(crate) fn keyboard(
        self,
        view: Size<f64>,
        to_view: impl FnOnce(Frame) -> Frame,
    ) -> Option<KeyboardInset> {
        let placement = match (self.notice, self.end) {
            (Notice::FrameChange | Notice::Show, Some(end)) => {
                placement(view, to_view(end), view.height)
            }
            (Notice::Hide, _) | (Notice::FrameChange | Notice::Show, None) => {
                KeyboardPlacement::Hidden
            }
        };
        self.change.keyboard(placement)
    }
}

/// The soft keyboard as a page reports it: what the web window publishes as
/// its inset and what the web text input reads to decide whether a tap on
/// the focused field reopens the keyboard.
#[cfg(target_arch = "wasm32")]
pub(crate) mod page {
    use wasm_bindgen::{JsCast as _, JsValue};

    use super::Frame;

    /// `navigator.virtualKeyboard`, where the browser has it.
    pub(crate) fn virtual_keyboard(window: &web_sys::Window) -> Option<web_sys::EventTarget> {
        js_sys::Reflect::get(&window.navigator(), &JsValue::from_str("virtualKeyboard"))
            .ok()?
            .dyn_into::<web_sys::EventTarget>()
            .ok()
    }

    /// The keyboard's rectangle from `navigator.virtualKeyboard`, which the
    /// browser reports only while the page has the keyboard overlay its content;
    /// an empty one while it is hidden.
    pub(crate) fn overlaid_keyboard(window: &web_sys::Window) -> Option<Frame> {
        let keyboard = virtual_keyboard(window)?;
        let overlays = js_sys::Reflect::get(&keyboard, &JsValue::from_str("overlaysContent"))
            .ok()?
            .as_bool()?;
        if !overlays {
            return None;
        }
        // The API gives a `DOMRect`; only its four sides are read.
        let rect = js_sys::Reflect::get(&keyboard, &JsValue::from_str("boundingRect"))
            .ok()
            .filter(JsValue::is_object)?
            .unchecked_into::<web_sys::DomRect>();
        Some(Frame {
            x: rect.x(),
            y: rect.y(),
            width: rect.width(),
            height: rect.height(),
        })
    }

    /// The layout viewport below the visual viewport, in client coordinates;
    /// `None` while the page is zoomed in.
    pub(crate) fn viewport_keyboard(window: &web_sys::Window) -> Option<Frame> {
        let visual = window.visual_viewport()?;
        if visual.scale() > 1.0 + 1e-3 {
            return None;
        }
        let layout = window.document()?.document_element()?;
        let bottom = visual.offset_top() + visual.height();
        Some(Frame {
            x: 0.0,
            y: bottom,
            width: f64::from(layout.client_width()),
            height: f64::from(layout.client_height()) - bottom,
        })
    }

    /// The layout viewport's bottom in client coordinates, which a docked
    /// keyboard's rectangle reaches: the smaller of the root element's client
    /// height and the window's inner height, whichever the page has. Quirks
    /// mode makes the first the document's height, and a horizontal scroll
    /// bar makes the second longer than the viewport's content; the smaller
    /// one never reads a docked keyboard as floating.
    pub(crate) fn layout_bottom(window: &web_sys::Window) -> Option<f64> {
        let root = window
            .document()
            .and_then(|document| document.document_element())
            .map(|root| f64::from(root.client_height()));
        let inner = window.inner_height().ok().and_then(|inner| inner.as_f64());
        match (root, inner) {
            (Some(root), Some(inner)) => Some(root.min(inner)),
            (bottom, None) | (None, bottom) => bottom,
        }
    }

    /// What the page knows of its soft keyboard now.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum KeyboardState {
        /// The browser reports a keyboard over part of the page.
        Shown,
        /// The browser reports no keyboard.
        Hidden,
        /// Nothing the page reads says: an inset inferred from the visual
        /// viewport is no report (Safari pans the visual viewport over a
        /// field near the bottom and the inset reads zero with the keyboard
        /// up, a keyboard that resizes the layout leaves none, a hardware
        /// keyboard shows none).
        Unknown,
    }

    /// The page's soft keyboard as the browser reports it: only
    /// `navigator.virtualKeyboard`'s geometry, read while the page has the
    /// keyboard overlay its content (the window asks for that while it
    /// lives), where an empty rectangle is a hidden keyboard.
    pub(crate) fn keyboard_state() -> KeyboardState {
        let Some(frame) = web_sys::window().and_then(|window| overlaid_keyboard(&window)) else {
            return KeyboardState::Unknown;
        };
        if frame.width > 0.0 && frame.height > 0.0 {
            KeyboardState::Shown
        } else {
            KeyboardState::Hidden
        }
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

    fn docked(bottom: f64) -> KeyboardPlacement {
        KeyboardPlacement::Docked {
            inset: EdgeInsets::new(0.0, 0.0, bottom, 0.0),
        }
    }

    #[test]
    fn a_frame_names_where_the_keyboard_rests() {
        let floating = frame(40.0, 300.0, 310.0, 260.0);
        let rows = [
            (
                "docked at the bottom",
                frame(0.0, 508.0, 390.0, 336.0),
                docked(336.0),
            ),
            (
                "docked, its bottom a fraction of a point above the view's",
                frame(0.0, 508.0, 390.0, 335.7),
                docked(336.0),
            ),
            (
                "taller than the view, from above it",
                frame(0.0, -10.0, 390.0, 900.0),
                docked(844.0),
            ),
            (
                "a frame whose bottom is above the view's bottom floats and covers no edge",
                floating,
                KeyboardPlacement::Floating {
                    frame: Rect::from_xywh(40.0, 300.0, 310.0, 260.0),
                },
            ),
            (
                "below the screen",
                frame(0.0, 844.0, 390.0, 336.0),
                KeyboardPlacement::Hidden,
            ),
            (
                "beside a Split View window",
                frame(400.0, 508.0, 390.0, 336.0),
                KeyboardPlacement::Hidden,
            ),
            (
                "no number",
                frame(f64::NAN, 508.0, 390.0, 336.0),
                KeyboardPlacement::Hidden,
            ),
            (
                "no area",
                frame(0.0, 508.0, 0.0, 336.0),
                KeyboardPlacement::Hidden,
            ),
        ];
        for (case, frame, expected) in rows {
            assert_eq!(placement(VIEW, frame, VIEW.height), expected, "{case}");
        }
        // A canvas that goes on below the page's visible bottom, where the
        // keyboard docks: covered from the keyboard's top down.
        assert_eq!(
            placement(VIEW, frame(0.0, 300.0, 390.0, 300.0), 600.0),
            docked(544.0),
            "docked at the floor of a taller view"
        );
    }

    #[test]
    fn a_notice_keeps_or_replaces_the_keyboard() {
        let change = Change {
            from: EdgeInsets::ZERO,
            start: EventTime::from_nanos(1_000),
            seconds: 0.0,
            curve: InsetCurve::SystemKeyboard,
        };
        let note = |notice, end| Note {
            notice,
            end,
            change,
        };
        let shown = note(Notice::FrameChange, Some(frame(0.0, 508.0, 390.0, 336.0)));
        let rows = [
            (
                "a zero frame while a floating keyboard is dragged keeps the inset",
                note(Notice::FrameChange, Some(Frame::ZERO)),
                docked(336.0),
            ),
            (
                "a change with no end frame keeps the inset",
                note(Notice::FrameChange, None),
                docked(336.0),
            ),
            (
                "a later frame replaces it",
                note(Notice::FrameChange, Some(frame(0.0, 544.0, 390.0, 300.0))),
                docked(300.0),
            ),
            (
                "a show with no end frame keeps the inset",
                note(Notice::Show, None),
                docked(336.0),
            ),
            (
                "a show at another frame replaces it",
                note(Notice::Show, Some(frame(0.0, 544.0, 390.0, 300.0))),
                docked(300.0),
            ),
            (
                "a show with a zero frame is a shortcuts bar dropped floating: no inset",
                note(Notice::Show, Some(Frame::ZERO)),
                KeyboardPlacement::Hidden,
            ),
            (
                "a hide notification is hidden with its frame still on screen",
                note(Notice::Hide, Some(frame(0.0, 600.0, 390.0, 336.0))),
                KeyboardPlacement::Hidden,
            ),
            (
                "a hide notification is hidden with a zero frame",
                note(Notice::Hide, Some(Frame::ZERO)),
                KeyboardPlacement::Hidden,
            ),
            (
                "a hide notification is hidden with no frame",
                note(Notice::Hide, None),
                KeyboardPlacement::Hidden,
            ),
        ];
        for (case, arriving, expected) in rows {
            let kept = arriving.replacing(Some(shown)).expect("a note is kept");
            let keyboard = kept
                .keyboard(VIEW, |end| end)
                .expect("a placement the platform takes");
            assert_eq!(keyboard.placement(), expected, "{case}");
        }
        assert_eq!(
            note(Notice::FrameChange, Some(Frame::ZERO)).replacing(None),
            None,
            "a zero frame before any other is no keyboard"
        );

        // The show UIKit posts after a frame change names the same end
        // frame: the motion the frame change started is not started again.
        let later = Change {
            start: EventTime::from_nanos(2_000),
            ..change
        };
        let second = Note {
            notice: Notice::Show,
            end: shown.end,
            change: later,
        };
        assert_eq!(second.replacing(Some(shown)), Some(shown));
        // After a hide the same frame is a keyboard that comes back.
        let hidden = note(Notice::Hide, shown.end);
        assert_eq!(second.replacing(Some(hidden)), Some(second));
        assert_eq!(second.replacing(None), Some(second));
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
        let keyboard = change
            .keyboard(KeyboardPlacement::Docked { inset: target })
            .expect("a valid keyboard");
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
                .keyboard(KeyboardPlacement::Docked { inset: target })
                .expect("a valid keyboard");
            assert_eq!(rest.motion(), None, "{seconds}");
            assert_eq!(rest.target(), target);
        }
    }
}
