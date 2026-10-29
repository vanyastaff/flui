//! The one wheel step: what a wheel event's amounts become in FLUI's
//! [`ScrollDelta`], the same way for every backend.
//!
//! FLUI's delta grows the consumer's scroll offset: positive scrolls down and
//! right. A platform reports its amounts in its own unit, which it picks
//! (Win32's detents, the DOM's `deltaMode`, winit's line or pixel delta,
//! Android's axis factors, AppKit's precise deltas), and in its own
//! direction ([`WheelDirection`]); [`wheel_step`] turns each axis to FLUI's
//! direction and refuses an amount that is not finite. What a refused step
//! becomes is the backend's: winit and AppKit deliver a zero step in the
//! event's unit, the Web and Android drop the event.

use flui_platform::pointer::{InputValueError, ScrollDelta, ScrollUnit};

/// Which way a platform's wheel amount points on one axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WheelDirection {
    /// A positive amount grows the scroll offset, scrolling down or right, as
    /// FLUI's delta does: the DOM's `WheelEvent` deltas and the Win32
    /// horizontal wheel, tilted right.
    Offset,
    /// A positive amount moves the content down or right, so the offset
    /// shrinks: winit's and AppKit's deltas, Android's scroll axes and the
    /// Win32 vertical wheel, turned away from the user.
    Content,
}

/// One axis of a wheel event: the amount a platform reports, already in the
/// step's unit, and which way it points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WheelAmount {
    amount: f64,
    direction: WheelDirection,
}

impl WheelAmount {
    /// An amount that grows the scroll offset when positive.
    pub(crate) const fn offset(amount: f64) -> Self {
        Self {
            amount,
            direction: WheelDirection::Offset,
        }
    }

    /// An amount that moves the content when positive.
    pub(crate) const fn content(amount: f64) -> Self {
        Self {
            amount,
            direction: WheelDirection::Content,
        }
    }

    /// The amount in FLUI's direction, with an axis that reports nothing
    /// `+0.0` whichever way the platform points and whichever zero it
    /// reports: a turned amount is subtracted from zero rather than negated,
    /// and a kept one has zero added (`-0.0 + 0.0` is `+0.0`).
    fn in_flui_direction(self) -> f64 {
        match self.direction {
            WheelDirection::Offset => self.amount + 0.0,
            WheelDirection::Content => 0.0 - self.amount,
        }
    }
}

/// The step a wheel event makes: `x` and `y` in `unit`, each turned to FLUI's
/// direction.
///
/// # Errors
/// [`InputValueError`] when an amount is not finite.
pub(crate) fn wheel_step(
    unit: ScrollUnit,
    x: WheelAmount,
    y: WheelAmount,
) -> Result<ScrollDelta, InputValueError> {
    ScrollDelta::try_new(unit, x.in_flui_direction(), y.in_flui_direction())
}

/// The table of the one wheel rule, which every backend's wheel step is held
/// to: one turn of a wheel, as each backend reports it and read by that
/// backend's own step, steps the row's amounts in FLUI's direction, and an
/// axis that turned not at all steps `+0.0`.
#[cfg(test)]
mod mapping {
    use flui_platform::PlatformInput;
    use flui_platform::pointer::{PointerEvent, ScrollDelta};
    use flui_platform::{EventTime, keyboard::Modifiers};

    use crate::input::android::scroll::AxisPolicy;
    use crate::input::assembly::{
        ContactRegistry, NativeClock, WHEEL_DETENT, WheelAxis, WheelReading,
    };

    /// One turn: how far it scrolls in FLUI's direction (positive scrolls down
    /// and right), and the step every backend makes of it.
    struct Turn {
        case: &'static str,
        amounts: (f64, f64),
        step: (f64, f64),
    }

    const TURNS: [Turn; 7] = [
        Turn {
            case: "a notch down",
            amounts: (0.0, 1.0),
            step: (0.0, 1.0),
        },
        Turn {
            case: "a notch up",
            amounts: (0.0, -1.0),
            step: (0.0, -1.0),
        },
        Turn {
            case: "half a notch up",
            amounts: (0.0, -0.5),
            step: (0.0, -0.5),
        },
        Turn {
            case: "two notches right",
            amounts: (2.0, 0.0),
            step: (2.0, 0.0),
        },
        Turn {
            case: "a notch left",
            amounts: (-1.0, 0.0),
            step: (-1.0, 0.0),
        },
        Turn {
            case: "down and right at once",
            amounts: (1.5, 3.0),
            step: (1.5, 3.0),
        },
        Turn {
            case: "a still axis reported as negative zero",
            amounts: (-0.0, 1.0),
            step: (0.0, 1.0),
        },
    ];

    /// Win32's wheel message for the turn: one axis, in `WHEEL_DELTA` units,
    /// the vertical one turned away from the user when positive; `None` for
    /// a turn of both axes, which no single message reports.
    fn win32((x, y): (f64, f64)) -> Option<ScrollDelta> {
        let detent = f64::from(WHEEL_DETENT);
        let (axis, distance) = match (x == 0.0, y == 0.0) {
            (true, false) => (WheelAxis::Vertical, -y * detent),
            (false, true) => (WheelAxis::Horizontal, x * detent),
            _ => return None,
        };
        let reading = WheelReading {
            axis,
            distance: distance as i16,
            position: (10.0, 10.0),
            scale: 1.0,
            time: EventTime::from_nanos(1),
            modifiers: Modifiers::NONE,
        };
        let input = ContactRegistry::new(NativeClock::epoch_only()).wheel(&reading);
        let Some(PlatformInput::Pointer(PointerEvent::Scroll(scroll))) = input else {
            panic!("a Win32 wheel message scrolls: {input:?}")
        };
        Some(scroll.delta)
    }

    /// The DOM's `WheelEvent` for the turn, in lines: its deltas point FLUI's
    /// way, and a still axis may be `-0`.
    fn dom((x, y): (f64, f64)) -> Option<ScrollDelta> {
        crate::input::web::wheel::step(1, x, y).ok()
    }

    /// An amount as a platform whose amounts move the content reports it: the
    /// other way round, and an axis that turned not at all as `+0.0`.
    fn moving_the_content(amount: f64) -> f64 {
        0.0 - amount
    }

    /// Android's scroll axes for the turn, before any factor is known.
    fn android((x, y): (f64, f64)) -> Option<ScrollDelta> {
        AxisPolicy::AuthoredLines.delta(moving_the_content(x) as f32, moving_the_content(y) as f32)
    }

    /// AppKit's `scrollingDeltaX`/`Y` for the turn, from a notched wheel.
    fn appkit((x, y): (f64, f64)) -> ScrollDelta {
        crate::input::apple::wheel::step(false, moving_the_content(x), moving_the_content(y)).0
    }

    /// winit's line delta for the turn.
    #[cfg(all(
        feature = "winit-backend",
        any(target_os = "windows", target_os = "macos", target_os = "linux")
    ))]
    fn winit((x, y): (f64, f64)) -> ScrollDelta {
        let delta = winit::event::MouseScrollDelta::LineDelta(
            moving_the_content(x) as f32,
            moving_the_content(y) as f32,
        );
        crate::input::winit::scroll_step(&delta, 1.0, false).delta
    }

    /// Every backend whose wheel step runs on this host takes each turn alike.
    #[test]
    fn every_backend_steps_a_turn_alike() {
        for Turn {
            case,
            amounts,
            step,
        } in TURNS
        {
            let steps = [
                ("Win32", win32(amounts)),
                ("DOM", dom(amounts)),
                ("Android", android(amounts)),
                ("AppKit", Some(appkit(amounts))),
                #[cfg(all(
                    feature = "winit-backend",
                    any(target_os = "windows", target_os = "macos", target_os = "linux")
                ))]
                ("winit", Some(winit(amounts))),
            ];
            for (backend, delta) in steps {
                let Some(delta) = delta else {
                    assert_eq!(backend, "Win32", "{backend}: {case} refused");
                    continue;
                };
                // Bit for bit: an axis that turned not at all is `+0.0`.
                assert_eq!(
                    (delta.x().to_bits(), delta.y().to_bits()),
                    (step.0.to_bits(), step.1.to_bits()),
                    "{backend}: {case} stepped {delta:?}"
                );
            }
        }
    }

    /// An amount that is not finite is refused: the Web and Android drop the
    /// event, winit and AppKit deliver a zero step in the event's unit.
    #[test]
    fn a_turn_that_is_not_finite_steps_nothing() {
        for amount in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(dom((amount, 0.0)), None, "DOM: {amount}");
            assert_eq!(android((0.0, amount)), None, "Android: {amount}");
            let zero = appkit((amount, 0.0));
            assert_eq!((zero.x(), zero.y()), (0.0, 0.0), "AppKit: {amount}");
            #[cfg(all(
                feature = "winit-backend",
                any(target_os = "windows", target_os = "macos", target_os = "linux")
            ))]
            {
                let zero = winit((amount, 0.0));
                assert_eq!((zero.x(), zero.y()), (0.0, 0.0), "winit: {amount}");
            }
        }
    }
}
