//! Where a window's content is covered or contested: the safe area, the soft
//! keyboard and its motion, and the edges system gestures take (ADR-0277).
//!
//! Every inset is logical pixels relative to the content area, finite and
//! non-negative on every edge: the constructors refuse anything else, so a
//! non-finite inset is never published.

use std::time::Duration;

use flui_foundation::geometry::{EdgeInsets, Rect};

use crate::EventTime;

/// Where the window's content is covered or contested, as one fact of the
/// window's snapshot ([`WindowState::insets`](crate::WindowState::insets)).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowInsets {
    safe_area: EdgeInsets,
    keyboard: KeyboardInset,
    system_gestures: EdgeInsets,
}

impl WindowInsets {
    /// No inset on any edge, the keyboard hidden.
    pub const ZERO: Self = Self {
        safe_area: EdgeInsets::ZERO,
        keyboard: KeyboardInset::HIDDEN,
        system_gestures: EdgeInsets::ZERO,
    };

    /// These insets.
    ///
    /// # Errors
    /// [`InvalidInsets`] when `safe_area` or `system_gestures` has an edge that
    /// is not finite or is negative.
    pub fn try_new(
        safe_area: EdgeInsets,
        keyboard: KeyboardInset,
        system_gestures: EdgeInsets,
    ) -> Result<Self, InvalidInsets> {
        check_edges(safe_area)?;
        check_edges(system_gestures)?;
        Ok(Self {
            safe_area,
            keyboard,
            system_gestures,
        })
    }

    /// These insets with `safe_area` in place of theirs.
    ///
    /// # Errors
    /// [`InvalidInsets`] when an edge of `safe_area` is not finite or is
    /// negative.
    pub fn try_with_safe_area(self, safe_area: EdgeInsets) -> Result<Self, InvalidInsets> {
        Self::try_new(safe_area, self.keyboard, self.system_gestures)
    }

    /// These insets with `keyboard` in place of theirs.
    #[must_use]
    pub const fn with_keyboard(mut self, keyboard: KeyboardInset) -> Self {
        self.keyboard = keyboard;
        self
    }

    /// These insets with `system_gestures` in place of theirs.
    ///
    /// # Errors
    /// [`InvalidInsets`] when an edge of `system_gestures` is not finite or is
    /// negative.
    pub fn try_with_system_gestures(
        self,
        system_gestures: EdgeInsets,
    ) -> Result<Self, InvalidInsets> {
        Self::try_new(self.safe_area, self.keyboard, system_gestures)
    }

    /// Intrusions the system reserves whether or not they cover content right
    /// now: status and navigation bars, a display cutout, the home indicator.
    /// Not the keyboard.
    #[must_use]
    pub const fn safe_area(&self) -> EdgeInsets {
        self.safe_area
    }

    /// The soft keyboard, and any input-method panel docked with it, where it
    /// covers the content.
    #[must_use]
    pub const fn keyboard(&self) -> KeyboardInset {
        self.keyboard
    }

    /// Edges where a system gesture (back, home, the notification shade)
    /// takes precedence over the application's own drag. Parts the
    /// application claims back through its input regions are still included.
    #[must_use]
    pub const fn system_gestures(&self) -> EdgeInsets {
        self.system_gestures
    }
}

impl Default for WindowInsets {
    fn default() -> Self {
        Self::ZERO
    }
}

/// Where the soft keyboard rests over the window's content (ADR-0372), in
/// logical pixels relative to the content area. The host tells the three
/// apart; the layout decides what each means for it.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum KeyboardPlacement {
    /// No keyboard over the content: none shown, one off screen, or one
    /// beside the window (an iPad's Split View).
    Hidden,
    /// Docked to the window's edges, covering `inset` of the content: the
    /// layout makes room above it.
    Docked {
        /// The edges the keyboard covers, each finite and not negative.
        inset: EdgeInsets,
    },
    /// Floating over the content at `frame`: an iPad's floating, undocked or
    /// split keyboard, whose bottom lies above the window's. It covers no
    /// edge, so it is not part of [`KeyboardInset::target`]; a layout that
    /// keeps something in view (a focused field, a toolbar) moves it out from
    /// under the frame itself.
    Floating {
        /// The keyboard's frame in the content's coordinates: finite, with a
        /// positive width and height.
        frame: Rect<f64>,
    },
}

impl KeyboardPlacement {
    /// The edges this placement covers: the docked inset, none otherwise.
    const fn covered(&self) -> EdgeInsets {
        match self {
            Self::Docked { inset } => *inset,
            Self::Hidden | Self::Floating { .. } => EdgeInsets::ZERO,
        }
    }

    /// This placement as [`KeyboardInset`] holds it: a docked keyboard that
    /// covers no edge is [`Hidden`](Self::Hidden), so one placement has one
    /// spelling.
    fn checked(self) -> Result<Self, InvalidInsets> {
        match self {
            Self::Hidden => Ok(Self::Hidden),
            Self::Docked { inset } => {
                check_edges(inset)?;
                Ok(if inset == EdgeInsets::ZERO {
                    Self::Hidden
                } else {
                    self
                })
            }
            Self::Floating { frame } => {
                let sides = [frame.left(), frame.top(), frame.right(), frame.bottom()];
                if !sides.iter().all(|side| side.is_finite()) {
                    return Err(InvalidInsets::NonFinite);
                }
                if frame.width() <= 0.0 || frame.height() <= 0.0 {
                    return Err(InvalidInsets::EmptyFrame);
                }
                Ok(self)
            }
        }
    }
}

/// The soft keyboard over the content: where it comes to rest, and how the
/// inset it covers moves there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyboardInset {
    placement: KeyboardPlacement,
    motion: Option<InsetMotion>,
}

impl KeyboardInset {
    /// The keyboard hidden and at rest.
    pub const HIDDEN: Self = Self {
        placement: KeyboardPlacement::Hidden,
        motion: None,
    };

    /// The keyboard at rest at `placement`.
    ///
    /// A docked placement that covers no edge is taken as
    /// [`Hidden`](KeyboardPlacement::Hidden).
    ///
    /// # Errors
    /// [`InvalidInsets`] when a docked inset has an edge that is not finite or
    /// is negative, or a floating frame is not finite or has no area.
    pub fn try_at_rest(placement: KeyboardPlacement) -> Result<Self, InvalidInsets> {
        Ok(Self {
            placement: placement.checked()?,
            motion: None,
        })
    }

    /// The keyboard moving to `placement`, its inset along `motion`: toward
    /// the docked inset, or toward none for a keyboard that hides or comes
    /// to float.
    ///
    /// # Errors
    /// As [`try_at_rest`](Self::try_at_rest).
    pub fn try_moving(
        placement: KeyboardPlacement,
        motion: InsetMotion,
    ) -> Result<Self, InvalidInsets> {
        Ok(Self {
            placement: placement.checked()?,
            motion: Some(motion),
        })
    }

    /// This keyboard with its motion ended: at rest at its placement.
    #[must_use]
    pub const fn settled(self) -> Self {
        Self {
            placement: self.placement,
            motion: None,
        }
    }

    /// Where the keyboard comes to rest.
    #[must_use]
    pub const fn placement(&self) -> KeyboardPlacement {
        self.placement
    }

    /// The inset once any motion ends: the docked keyboard's, none for one
    /// hidden or floating.
    #[must_use]
    pub const fn target(&self) -> EdgeInsets {
        self.placement.covered()
    }

    /// The frame of a keyboard that floats, in the content's coordinates;
    /// `None` for one docked or hidden. It is where the keyboard comes to
    /// rest: the motion moves the inset, not the frame.
    #[must_use]
    pub const fn floating_frame(&self) -> Option<Rect<f64>> {
        match self.placement {
            KeyboardPlacement::Floating { frame } => Some(frame),
            KeyboardPlacement::Hidden | KeyboardPlacement::Docked { .. } => None,
        }
    }

    /// The motion toward [`target`](Self::target); `None` at rest.
    #[must_use]
    pub const fn motion(&self) -> Option<InsetMotion> {
        self.motion
    }

    /// When the motion ends; `None` at rest, or for a motion whose end lies
    /// past the end of the event timeline.
    #[must_use]
    pub fn settles_at(&self) -> Option<EventTime> {
        self.motion.and_then(|motion| motion.end())
    }

    /// The occlusion at `time`: the motion's start value before it starts,
    /// its curve's value while it runs, [`target`](Self::target) at rest and
    /// from its end on.
    #[must_use]
    pub fn at(&self, time: EventTime) -> EdgeInsets {
        let target = self.target();
        let Some(motion) = self.motion else {
            return target;
        };
        if time <= motion.start {
            return motion.from;
        }
        let elapsed = time.saturating_duration_since(motion.start);
        if elapsed >= motion.duration {
            return target;
        }
        let t = elapsed.as_secs_f64() / motion.duration.as_secs_f64();
        let progress = motion.curve.progress(t);
        // An overshooting curve can leave the range of `f64` between finite
        // endpoints; such an edge takes the target, so no frame lays out
        // with a non-finite inset.
        let lerp = |from: f64, to: f64| {
            let edge = from + (to - from) * progress;
            if edge.is_finite() { edge.max(0.0) } else { to }
        };
        EdgeInsets::new(
            lerp(motion.from.top, target.top),
            lerp(motion.from.right, target.right),
            lerp(motion.from.bottom, target.bottom),
            lerp(motion.from.left, target.left),
        )
    }
}

impl Default for KeyboardInset {
    fn default() -> Self {
        Self::HIDDEN
    }
}

/// A motion of the keyboard on the event timeline, as the operating system
/// animates it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InsetMotion {
    from: EdgeInsets,
    start: EventTime,
    duration: Duration,
    curve: InsetCurve,
}

impl InsetMotion {
    /// A motion from `from` that starts at `start` and lasts `duration` along
    /// `curve`.
    ///
    /// # Errors
    /// [`InvalidInsets`] when an edge of `from` is not finite or is negative,
    /// `duration` is zero, or `curve` is not a valid curve.
    pub fn try_new(
        from: EdgeInsets,
        start: EventTime,
        duration: Duration,
        curve: InsetCurve,
    ) -> Result<Self, InvalidInsets> {
        check_edges(from)?;
        if duration.is_zero() {
            return Err(InvalidInsets::ZeroDuration);
        }
        curve.check()?;
        Ok(Self {
            from,
            start,
            duration,
            curve,
        })
    }

    /// The occlusion the motion starts from.
    #[must_use]
    pub const fn from(&self) -> EdgeInsets {
        self.from
    }

    /// When the motion starts.
    #[must_use]
    pub const fn start(&self) -> EventTime {
        self.start
    }

    /// How long the motion lasts.
    #[must_use]
    pub const fn duration(&self) -> Duration {
        self.duration
    }

    /// The motion's timing curve.
    #[must_use]
    pub const fn curve(&self) -> InsetCurve {
        self.curve
    }

    /// When the motion ends; `None` past the end of the event timeline.
    #[must_use]
    pub const fn end(&self) -> Option<EventTime> {
        self.start.checked_add(self.duration)
    }
}

/// The operating system's timing curve for a keyboard motion.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum InsetCurve {
    /// Progress proportional to time.
    Linear,
    /// A cubic Bézier through (0, 0), the two control points and (1, 1), as
    /// CSS's `cubic-bezier()` and UIKit's standard curves are. Each `x` lies
    /// in `0..=1`.
    CubicBezier {
        /// The first control point's time.
        x1: f64,
        /// The first control point's progress.
        y1: f64,
        /// The second control point's time.
        x2: f64,
        /// The second control point's progress.
        y2: f64,
    },
    /// UIKit's keyboard curve (`UIViewAnimationCurve` 7), which no public
    /// constant names. Approximated by
    /// [`SYSTEM_KEYBOARD_BEZIER`](Self::SYSTEM_KEYBOARD_BEZIER).
    SystemKeyboard,
}

impl InsetCurve {
    /// The cubic Bézier that stands for [`SystemKeyboard`](Self::SystemKeyboard):
    /// `(0.380, 0.700, 0.125, 1.000)`, a measurement of UIKit's keyboard
    /// motion (ADR-0277 §1).
    pub const SYSTEM_KEYBOARD_BEZIER: Self = Self::CubicBezier {
        x1: 0.380,
        y1: 0.700,
        x2: 0.125,
        y2: 1.000,
    };

    /// Whether the curve is one [`progress`](Self::progress) can evaluate.
    fn check(self) -> Result<(), InvalidInsets> {
        match self {
            Self::Linear | Self::SystemKeyboard => Ok(()),
            Self::CubicBezier { x1, y1, x2, y2 } => {
                let finite = [x1, y1, x2, y2].iter().all(|value| value.is_finite());
                let times = (0.0..=1.0).contains(&x1) && (0.0..=1.0).contains(&x2);
                (finite && times)
                    .then_some(())
                    .ok_or(InvalidInsets::InvalidCurve)
            }
        }
    }

    /// The progress at the fraction `t` of the motion's duration, `t` in
    /// `0..=1`.
    #[must_use]
    pub fn progress(self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::CubicBezier { x1, y1, x2, y2 } => cubic_bezier(x1, y1, x2, y2, t),
            Self::SystemKeyboard => Self::SYSTEM_KEYBOARD_BEZIER.progress(t),
        }
    }
}

/// The `y` of the cubic Bézier through (0,0), (x1,y1), (x2,y2), (1,1) at the
/// point whose `x` is `x`: Newton's method on `x(s)`, bisection where its
/// slope vanishes.
fn cubic_bezier(x1: f64, y1: f64, x2: f64, y2: f64, x: f64) -> f64 {
    // B(s) = 3(1-s)^2 s p1 + 3(1-s) s^2 p2 + s^3, in polynomial form.
    let coefficients = |p1: f64, p2: f64| {
        let c = 3.0 * p1;
        let b = 3.0 * (p2 - p1) - c;
        let a = 1.0 - c - b;
        (a, b, c)
    };
    let (ax, bx, cx) = coefficients(x1, x2);
    let (ay, by, cy) = coefficients(y1, y2);
    let sample = |a: f64, b: f64, c: f64, s: f64| ((a * s + b) * s + c) * s;
    let slope = |s: f64| (3.0 * ax * s + 2.0 * bx) * s + cx;
    let mut s = x;
    for _ in 0..8 {
        let error = sample(ax, bx, cx, s) - x;
        if error.abs() < 1e-7 {
            return sample(ay, by, cy, s);
        }
        let derivative = slope(s);
        if derivative.abs() < 1e-6 {
            break;
        }
        s -= error / derivative;
    }
    let (mut low, mut high) = (0.0_f64, 1.0_f64);
    s = x;
    for _ in 0..64 {
        let value = sample(ax, bx, cx, s);
        if (value - x).abs() < 1e-7 {
            break;
        }
        if value < x {
            low = s;
        } else {
            high = s;
        }
        s = low.midpoint(high);
    }
    sample(ay, by, cy, s)
}

/// Why insets were refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidInsets {
    /// An edge is NaN or infinite.
    #[error("an inset edge is not finite")]
    NonFinite,
    /// An edge is negative.
    #[error("an inset edge is negative")]
    Negative,
    /// A motion has no duration.
    #[error("a keyboard motion has a zero duration")]
    ZeroDuration,
    /// A cubic Bézier control point is not finite, or its time lies outside
    /// `0..=1`.
    #[error("a keyboard motion's curve is not a valid cubic Bézier")]
    InvalidCurve,
    /// A floating keyboard's frame has no width or no height.
    #[error("a floating keyboard's frame has no area")]
    EmptyFrame,
}

fn check_edges(edges: EdgeInsets) -> Result<(), InvalidInsets> {
    if !edges.all_satisfy(|edge| edge.is_finite()) {
        return Err(InvalidInsets::NonFinite);
    }
    if !edges.all_satisfy(|edge| *edge >= 0.0) {
        return Err(InvalidInsets::Negative);
    }
    Ok(())
}
