//! The soft keyboard over a window, from the frame its platform reports in
//! the window's coordinates (ADR-0277 §4, ADR-0373): UIKit's
//! `UIKeyboardWillChangeFrameNotification`, `UIKeyboardDidChangeFrameNotification`,
//! `UIKeyboardWillShowNotification` and `UIKeyboardWillHideNotification`,
//! which also carry the animation's duration and `UIViewAnimationCurve`, and
//! a browser's visual viewport or `navigator.virtualKeyboard`.

#[cfg(any(target_os = "ios", test))]
use std::time::Duration;

use flui_foundation::geometry::{EdgeInsets, Rect, Size};
use flui_platform::KeyboardPlacement;
#[cfg(any(target_os = "ios", test))]
use flui_platform::{EventTime, InsetCurve, InsetMotion, SoftKeyboard};

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

/// How far above the floor a keyboard's bottom lies before the keyboard
/// floats, in the frame's own unit (points on iOS, CSS pixels on the Web). A
/// keyboard extension reports a docked frame a fraction of a point off, and
/// the arithmetic that brings a frame into a view leaves an error far below
/// this.
const FLOATING_CLEARANCE: f64 = 0.5;

/// Where the keyboard at `frame` rests over a view `view` in size, `floor`
/// being the bottom a docked keyboard reaches in the view's coordinates: the
/// view's own bottom on iOS (an iPad reports a Slide Over window's keyboard
/// ending at the window's bottom); on the Web the layout viewport's bottom,
/// or the canvas's where it ends above that (a canvas taller than the layout
/// viewport goes on below a docked keyboard). The keyboard is
///
/// - hidden for a frame that is no number, has no area, or overlaps the view
///   nowhere (off screen, or beside the view in an iPad's Split View);
/// - floating for one whose bottom lies [`FLOATING_CLEARANCE`] or more above
///   `floor` (an iPad's floating, undocked or split keyboard, or a shortcuts
///   bar dragged up): it covers what lies under its frame and no edge;
/// - docked otherwise, covering the view from the keyboard's top down.
///
/// All of it is in the unit the platform reports the frame in: a caller that
/// scales the result ([`scaled`]) does so afterwards, so the clearance is
/// never measured in scaled units.
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
    if floor - (frame.y + frame.height) >= FLOATING_CLEARANCE {
        return KeyboardPlacement::Floating {
            frame: Rect::from_xywh(frame.x, frame.y, frame.width, frame.height),
        };
    }
    KeyboardPlacement::Docked {
        inset: EdgeInsets::new(0.0, 0.0, (view.height - frame.y.max(0.0)).max(0.0), 0.0),
    }
}

/// `placement` with every length multiplied by `scale`: a placement read in
/// the box a page measures goes out in the window's logical pixels, since a
/// transform on the canvas or an ancestor scales what the page measures and
/// not the window's size.
#[cfg(any(target_arch = "wasm32", test))]
#[must_use]
pub(crate) fn scaled(placement: KeyboardPlacement, scale: f64) -> KeyboardPlacement {
    match placement {
        KeyboardPlacement::Docked { inset } => KeyboardPlacement::Docked {
            inset: EdgeInsets::new(
                inset.top * scale,
                inset.right * scale,
                inset.bottom * scale,
                inset.left * scale,
            ),
        },
        KeyboardPlacement::Floating { frame } => KeyboardPlacement::Floating {
            frame: Rect::from_ltrb(
                frame.left() * scale,
                frame.top() * scale,
                frame.right() * scale,
                frame.bottom() * scale,
            ),
        },
        // Hidden, which has nothing to scale.
        unscaled => unscaled,
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
    pub(crate) fn keyboard(self, placement: KeyboardPlacement) -> Option<SoftKeyboard> {
        let duration = Duration::try_from_secs_f64(self.seconds)
            .ok()
            .filter(|duration| !duration.is_zero());
        let moving = duration
            .and_then(|duration| {
                InsetMotion::try_new(self.from, self.start, duration, self.curve).ok()
            })
            .and_then(|motion| SoftKeyboard::try_moving(placement, motion).ok());
        moving.or_else(|| SoftKeyboard::try_at_rest(placement).ok())
    }
}

#[cfg(any(target_os = "ios", test))]
/// Which of UIKit's keyboard notifications a view heard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Notice {
    /// `UIKeyboardWillChangeFrameNotification`: the keyboard moves to its end
    /// frame.
    Moving,
    /// `UIKeyboardDidChangeFrameNotification`: the keyboard came to rest at
    /// its end frame. It is the only notice of where a keyboard the user
    /// dragged or undocked ended up.
    Moved,
    /// `UIKeyboardWillShowNotification`: the keyboard comes to its end frame.
    /// UIKit's zero frame here is a shortcuts bar dropped where the user
    /// dragged it: it floats, and UIKit does not say where.
    Showing,
    /// `UIKeyboardWillHideNotification`, which UIKit posts after the hide's
    /// frame change: the keyboard leaves its dock, whatever its end frame
    /// says (one not yet fully off screen is a hide all the same).
    Hiding,
}

#[cfg(any(target_os = "ios", test))]
/// A keyboard notification as it arrives: which one, its end frame in
/// screen coordinates if it carried one, and the change it starts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Heard {
    pub(crate) notice: Notice,
    /// `UIKeyboardFrameEndUserInfoKey`.
    pub(crate) end: Option<Frame>,
    pub(crate) change: Change,
}

#[cfg(any(target_os = "ios", test))]
/// What a view keeps of the keyboard notifications it heard: where the
/// keyboard ends up, in screen coordinates, and the change that takes its
/// inset there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Note {
    /// `WillHide`: the keyboard left its dock along the change, and UIKit has
    /// not yet said where it came to rest.
    Hide(Change),
    /// The keyboard goes to `end` along `change`, which `notice` started.
    /// [`Notice::Hiding`] here is a hide whose `DidChangeFrame` named where
    /// the keyboard came to rest: a keyboard the user undocked is announced
    /// as a hide and then rests floating.
    At {
        notice: Notice,
        end: Frame,
        change: Change,
    },
}

#[cfg(any(target_os = "ios", test))]
impl Heard {
    /// The note a view keeps once this notification arrives while `standing`
    /// stands: `standing` itself when the notification says nothing new of
    /// where the keyboard ends up.
    ///
    /// - A hide always stands, and starts its change.
    /// - A notification with no end frame is not the keyboard's.
    /// - UIKit's zero frame on a frame change, before or after it, is what it
    ///   sends while the user drags a floating keyboard; on a show it is a
    ///   shortcuts bar dropped floating, which stands (with no area, so no
    ///   placement).
    /// - A show or a finished frame change that names the frame a frame
    ///   change or a show left standing is a later notice of the same move,
    ///   whose motion is already under way. After a hide the same frame is a
    ///   keyboard that comes back.
    /// - The first finished frame change after a hide keeps the hide's change
    ///   and names where the keyboard came to rest.
    #[must_use]
    pub(crate) fn kept(self, standing: Option<Note>) -> Option<Note> {
        let Self {
            notice,
            end,
            change,
        } = self;
        match (notice, end) {
            (Notice::Hiding, _) => Some(Note::Hide(change)),
            (Notice::Moving | Notice::Moved | Notice::Showing, None) => standing,
            (Notice::Moving | Notice::Moved, Some(end)) if end == Frame::ZERO => standing,
            (Notice::Moving, Some(end)) => Some(Note::At {
                notice,
                end,
                change,
            }),
            (Notice::Moved | Notice::Showing, Some(end)) => match standing {
                Some(Note::At {
                    notice: started_by,
                    end: resting,
                    ..
                }) if started_by != Notice::Hiding && resting == end => standing,
                Some(Note::Hide(hide)) if notice == Notice::Moved => Some(Note::At {
                    notice: Notice::Hiding,
                    end,
                    change: hide,
                }),
                Some(Note::At { .. } | Note::Hide(_)) | None => Some(Note::At {
                    notice,
                    end,
                    change,
                }),
            },
        }
    }
}

#[cfg(any(target_os = "ios", test))]
impl Note {
    /// The keyboard this note puts over a view `view` in size, `to_view`
    /// taking a frame from screen coordinates into the view's: the
    /// [`placement`] of the frame the keyboard goes to. A hide is hidden
    /// unless the frame it came to rest at floats: a hide whose frame still
    /// lies on the bottom of the screen is a hide all the same. `None` for a
    /// placement the platform's types refuse.
    #[must_use]
    pub(crate) fn keyboard(
        self,
        view: Size<f64>,
        to_view: impl FnOnce(Frame) -> Frame,
    ) -> Option<SoftKeyboard> {
        let (placement, change) = match self {
            Self::Hide(change) => (KeyboardPlacement::Hidden, change),
            Self::At {
                notice: Notice::Hiding,
                end,
                change,
            } => match placement(view, to_view(end), view.height) {
                floating @ KeyboardPlacement::Floating { .. } => (floating, change),
                _ => (KeyboardPlacement::Hidden, change),
            },
            Self::At { end, change, .. } => (placement(view, to_view(end), view.height), change),
        };
        change.keyboard(placement)
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

    fn floating(x: f64, y: f64, width: f64, height: f64) -> KeyboardPlacement {
        KeyboardPlacement::Floating {
            frame: Rect::from_xywh(x, y, width, height),
        }
    }

    #[test]
    fn a_frame_names_where_the_keyboard_rests() {
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
                frame(40.0, 300.0, 310.0, 260.0),
                floating(40.0, 300.0, 310.0, 260.0),
            ),
            (
                "half a point above the view's bottom floats",
                frame(0.0, 508.0, 390.0, 335.5),
                floating(0.0, 508.0, 390.0, 335.5),
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

    /// The clearance is a difference, never two roundings: bottoms that
    /// straddle a half read docked however close they lie.
    #[test]
    fn a_docked_keyboard_a_fraction_off_its_floor_is_docked() {
        // A canvas drawn at three tenths of its size: the keyboard's bottom
        // comes out one unit in the last place under the floor.
        let bottom = 115.499_999_999_999_99_f64;
        assert!(bottom < 115.5 && bottom.round() < 115.5_f64.round());
        assert_eq!(
            placement(
                Size::new(90.0, 115.5),
                frame(0.0, 60.0, 90.0, bottom - 60.0),
                115.5
            ),
            KeyboardPlacement::Docked {
                inset: EdgeInsets::new(0.0, 0.0, 55.5, 0.0)
            },
            "a scaled canvas"
        );
        // A view half a point taller than the screen's whole points, and a
        // keyboard extension's frame three tenths short of it.
        assert_eq!(
            placement(
                Size::new(390.0, 844.5),
                frame(0.0, 508.5, 390.0, 335.7),
                844.5
            ),
            docked(336.0),
            "a view on a half point"
        );
    }

    #[test]
    fn a_placement_scales_into_the_windows_pixels() {
        assert_eq!(scaled(docked(5.0), 2.0), docked(10.0));
        assert_eq!(
            scaled(floating(40.0, 10.0, 100.0, 30.0), 2.0),
            floating(80.0, 20.0, 200.0, 60.0)
        );
        assert_eq!(
            scaled(KeyboardPlacement::Hidden, 2.0),
            KeyboardPlacement::Hidden
        );
    }

    const DOCKED: Frame = Frame {
        x: 0.0,
        y: 508.0,
        width: 390.0,
        height: 336.0,
    };
    const FLOATING: Frame = Frame {
        x: 40.0,
        y: 300.0,
        width: 310.0,
        height: 260.0,
    };
    const DROPPED: Frame = Frame {
        x: 60.0,
        y: 120.0,
        width: 310.0,
        height: 260.0,
    };
    const OFF_SCREEN: Frame = Frame {
        x: 0.0,
        y: 844.0,
        width: 390.0,
        height: 336.0,
    };

    fn change_at(nanos: u64) -> Change {
        Change {
            from: EdgeInsets::ZERO,
            start: EventTime::from_nanos(nanos),
            seconds: 0.0,
            curve: InsetCurve::SystemKeyboard,
        }
    }

    /// A notification as a test announces it: which one, and its end frame.
    type Announced = (Notice, Option<Frame>);

    /// The note standing after `heard`, each notification a tick later than
    /// the one before it.
    fn after(heard: &[Announced]) -> Option<Note> {
        heard
            .iter()
            .zip(1_u64..)
            .fold(None, |standing, (&(notice, end), tick)| {
                Heard {
                    notice,
                    end,
                    change: change_at(tick * 1_000),
                }
                .kept(standing)
            })
    }

    #[test]
    fn the_notifications_heard_name_where_the_keyboard_rests() {
        use Notice::{Hiding, Moved, Moving, Showing};
        let floats = |frame: Frame| floating(frame.x, frame.y, frame.width, frame.height);
        let rows: [(&str, &[Announced], KeyboardPlacement); 18] = [
            (
                "a show",
                &[
                    (Moving, Some(DOCKED)),
                    (Showing, Some(DOCKED)),
                    (Moved, Some(DOCKED)),
                ],
                docked(336.0),
            ),
            (
                "a zero frame while a floating keyboard is dragged keeps the inset",
                &[(Moving, Some(DOCKED)), (Moving, Some(Frame::ZERO))],
                docked(336.0),
            ),
            (
                "a change with no end frame keeps the inset",
                &[(Moving, Some(DOCKED)), (Moving, None)],
                docked(336.0),
            ),
            (
                "a later frame replaces it",
                &[
                    (Moving, Some(DOCKED)),
                    (Moving, Some(frame(0.0, 544.0, 390.0, 300.0))),
                ],
                docked(300.0),
            ),
            (
                "a show with no end frame keeps the inset",
                &[(Moving, Some(DOCKED)), (Showing, None)],
                docked(336.0),
            ),
            (
                "a show with a zero frame is a shortcuts bar dropped floating: no inset",
                &[(Moving, Some(DOCKED)), (Showing, Some(Frame::ZERO))],
                KeyboardPlacement::Hidden,
            ),
            (
                "a hide notification is hidden with its frame still on screen",
                &[
                    (Moving, Some(frame(0.0, 600.0, 390.0, 336.0))),
                    (Hiding, Some(frame(0.0, 600.0, 390.0, 336.0))),
                    (Moved, Some(frame(0.0, 600.0, 390.0, 336.0))),
                ],
                KeyboardPlacement::Hidden,
            ),
            (
                "a hide notification is hidden with a zero frame",
                &[(Moving, Some(DOCKED)), (Hiding, Some(Frame::ZERO))],
                KeyboardPlacement::Hidden,
            ),
            (
                "a hide notification is hidden with no frame",
                &[(Moving, Some(DOCKED)), (Hiding, None)],
                KeyboardPlacement::Hidden,
            ),
            (
                "a hide",
                &[
                    (Moving, Some(DOCKED)),
                    (Moving, Some(OFF_SCREEN)),
                    (Hiding, Some(OFF_SCREEN)),
                    (Moved, Some(OFF_SCREEN)),
                ],
                KeyboardPlacement::Hidden,
            ),
            (
                "a keyboard that comes back after a hide",
                &[
                    (Moving, Some(DOCKED)),
                    (Hiding, Some(OFF_SCREEN)),
                    (Moving, Some(DOCKED)),
                    (Showing, Some(DOCKED)),
                ],
                docked(336.0),
            ),
            (
                "a floating keyboard dragged and dropped rests where it was dropped",
                &[
                    (Moving, Some(FLOATING)),
                    (Moved, Some(FLOATING)),
                    (Moving, Some(Frame::ZERO)),
                    (Moved, Some(Frame::ZERO)),
                    (Moving, Some(Frame::ZERO)),
                    (Moved, Some(DROPPED)),
                ],
                floats(DROPPED),
            ),
            (
                "an undocked keyboard is announced as a hide and floats where it came to rest",
                &[
                    (Moving, Some(DOCKED)),
                    (Moving, Some(FLOATING)),
                    (Hiding, Some(FLOATING)),
                    (Moved, Some(FLOATING)),
                ],
                floats(FLOATING),
            ),
            (
                "an undocked keyboard dragged on rests where it was dropped",
                &[
                    (Moving, Some(DOCKED)),
                    (Hiding, Some(FLOATING)),
                    (Moved, Some(FLOATING)),
                    (Moving, Some(Frame::ZERO)),
                    (Moved, Some(DROPPED)),
                ],
                floats(DROPPED),
            ),
            (
                "a docked keyboard dragged off its dock floats where it was dropped",
                &[
                    (Moving, Some(DOCKED)),
                    (Moved, Some(DOCKED)),
                    (Moving, Some(Frame::ZERO)),
                    (Moved, Some(DROPPED)),
                ],
                floats(DROPPED),
            ),
            (
                "a zero frame before any other is no keyboard",
                &[(Moving, Some(Frame::ZERO)), (Moved, Some(Frame::ZERO))],
                KeyboardPlacement::Hidden,
            ),
            (
                "a keyboard that comes to rest off screen is hidden",
                &[
                    (Moving, Some(FLOATING)),
                    (Moved, Some(FLOATING)),
                    (Moving, Some(Frame::ZERO)),
                    (Moved, Some(OFF_SCREEN)),
                ],
                KeyboardPlacement::Hidden,
            ),
            (
                "a keyboard shown again after a hide that rested on its dock",
                &[
                    (Moving, Some(DOCKED)),
                    (Hiding, Some(DOCKED)),
                    (Moved, Some(DOCKED)),
                    (Showing, Some(DOCKED)),
                ],
                docked(336.0),
            ),
        ];
        // Every row is read before the assertion, so a failure names each
        // case it breaks.
        let wrong: Vec<String> = rows
            .into_iter()
            .filter_map(|(case, heard, expected)| {
                let placement = after(heard).map_or(KeyboardPlacement::Hidden, |note| {
                    note.keyboard(VIEW, |end| end)
                        .expect("a placement the platform takes")
                        .placement()
                });
                (placement != expected)
                    .then(|| format!("{case}: got {placement:?}, wanted {expected:?}"))
            })
            .collect();
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    /// One move starts one motion: the notices that repeat its frame leave
    /// the change the first of them started, and a frame that comes to rest
    /// after a hide leaves the hide's.
    #[test]
    fn a_later_notice_of_one_move_starts_no_second_motion() {
        use Notice::{Hiding, Moved, Moving, Showing};
        let shown = after(&[(Moving, Some(DOCKED))]);
        assert_eq!(
            shown,
            Some(Note::At {
                notice: Moving,
                end: DOCKED,
                change: change_at(1_000)
            })
        );
        assert_eq!(
            after(&[
                (Moving, Some(DOCKED)),
                (Showing, Some(DOCKED)),
                (Moved, Some(DOCKED))
            ]),
            shown
        );
        assert_eq!(
            after(&[
                (Moving, Some(DOCKED)),
                (Hiding, Some(OFF_SCREEN)),
                (Moved, Some(FLOATING))
            ]),
            Some(Note::At {
                notice: Hiding,
                end: FLOATING,
                change: change_at(2_000)
            })
        );
        // After a hide the same frame is a keyboard that comes back, also
        // when the hide came to rest at it.
        assert_eq!(
            after(&[
                (Moving, Some(DOCKED)),
                (Hiding, Some(OFF_SCREEN)),
                (Showing, Some(DOCKED))
            ]),
            Some(Note::At {
                notice: Showing,
                end: DOCKED,
                change: change_at(3_000)
            })
        );
        assert_eq!(
            after(&[
                (Moving, Some(DOCKED)),
                (Hiding, Some(DOCKED)),
                (Moved, Some(DOCKED)),
                (Showing, Some(DOCKED))
            ]),
            Some(Note::At {
                notice: Showing,
                end: DOCKED,
                change: change_at(4_000)
            })
        );
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
