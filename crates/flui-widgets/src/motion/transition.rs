//! [`Transition`] — how a child appears and disappears, one effect per slot.

use std::ops::Add;
use std::time::Duration;

use flui_animation::curve::{ArcCurve, Curve};
use flui_animation::{MotionSpec, curves};
use flui_objects::TranslationFraction;
use flui_painting::Alignment;

use super::effect::{Effect, Kind, Pose, Stages};

/// How a child enters and exits: at most one [`Effect`] of each kind per
/// direction, played over one shared progress.
///
/// Progress runs from `0` (absent) to `1` (present) on entry and back on
/// exit, and each effect is evaluated directly at it: entering plays the
/// enter effects forwards, exiting plays the exit effects backwards. The
/// direction is chosen when a run starts from rest and kept until the run
/// settles, so an interrupted exit reverses from the progress it reached,
/// along the same effects, without a jump.
///
/// `a + b` merges slot by slot and the right operand wins a slot both fill:
/// `Transition::fade() + Transition::slide_in(..)` fades and slides, and
/// `(fade + slide) + Transition::effect(Effect::Fade { .. })` replaces only
/// the fade. The duration and curve progress runs along are slots too; unset,
/// a full run takes [`MotionSpec::DEFAULT_DURATION`] along `EASE_IN_OUT`, and
/// a partial one its share of that. [`asymmetric`](Self::asymmetric) takes the
/// entry of one transition and the exit of another.
///
/// ```
/// use flui_objects::TranslationFraction;
/// use flui_widgets::motion::Transition;
/// use std::time::Duration;
///
/// let enter = Transition::fade() + Transition::slide_in(TranslationFraction::new(0.0, 0.25));
/// let pop = Transition::asymmetric(enter, Transition::scale_in(0.8))
///     .duration(Duration::from_millis(250));
/// # let _ = pop;
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transition {
    enter: Slots,
    exit: Slots,
    duration: Option<Duration>,
    curve: Option<ArcCurve>,
}

/// One effect per kind.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Slots {
    fade: Option<Effect>,
    rotate: Option<Effect>,
    scale: Option<Effect>,
    slide: Option<Effect>,
}

impl Slots {
    fn of(effect: Effect) -> Self {
        let mut slots = Self::default();
        *slots.slot(effect.kind()) = Some(effect);
        slots
    }

    fn slot(&mut self, kind: Kind) -> &mut Option<Effect> {
        match kind {
            Kind::Fade => &mut self.fade,
            Kind::Rotate => &mut self.rotate,
            Kind::Scale => &mut self.scale,
            Kind::Slide => &mut self.slide,
        }
    }

    fn effects(&self) -> impl Iterator<Item = &Effect> + Clone {
        [&self.fade, &self.rotate, &self.scale, &self.slide]
            .into_iter()
            .flatten()
    }

    fn merged(self, right: Self) -> Self {
        Self {
            fade: right.fade.or(self.fade),
            rotate: right.rotate.or(self.rotate),
            scale: right.scale.or(self.scale),
            slide: right.slide.or(self.slide),
        }
    }
}

/// Which side of a [`Transition`] is playing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Phase {
    #[default]
    Enter,
    Exit,
}

impl Transition {
    /// No effect: the child appears and disappears at once.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// `effect` on both entry and exit.
    #[must_use]
    pub fn effect(effect: Effect) -> Self {
        let slots = Slots::of(effect);
        Self {
            enter: slots,
            exit: slots,
            duration: None,
            curve: None,
        }
    }

    /// Fade in from transparent, and out to transparent.
    #[must_use]
    pub fn fade() -> Self {
        Self::effect(Effect::FADE_IN)
    }

    /// Slide in from `from`, a fraction of the child's size, and back out to it.
    #[must_use]
    pub fn slide_in(from: TranslationFraction) -> Self {
        Self::effect(Effect::Slide { from })
    }

    /// Scale about the centre from `from`, and back down to it.
    #[must_use]
    pub fn scale_in(from: f64) -> Self {
        Self::effect(Effect::Scale {
            from,
            alignment: Alignment::CENTER,
        })
    }

    /// Enter as `enter` enters and exit as `exit` exits. Duration and curve
    /// are `exit`'s where it sets them, otherwise `enter`'s.
    #[must_use]
    pub fn asymmetric(enter: Self, exit: Self) -> Self {
        Self {
            enter: enter.enter,
            exit: exit.exit,
            duration: exit.duration.or(enter.duration),
            curve: exit.curve.or(enter.curve),
        }
    }

    /// Run a full entry or exit over `duration`.
    #[must_use]
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Ease progress along `curve`, in both directions.
    #[must_use]
    pub fn curve(mut self, curve: impl Curve + Send + Sync + 'static) -> Self {
        self.curve = Some(ArcCurve::new(curve));
        self
    }

    /// How long a full run takes: nothing, when neither side has an effect,
    /// so an empty transition never drives frames.
    pub(crate) fn run_duration(&self) -> Duration {
        if self
            .enter
            .effects()
            .chain(self.exit.effects())
            .next()
            .is_none()
        {
            return Duration::ZERO;
        }
        self.duration.unwrap_or(MotionSpec::DEFAULT_DURATION)
    }

    /// The easing of progress.
    pub(crate) fn run_curve(&self) -> ArcCurve {
        self.curve
            .clone()
            .unwrap_or_else(|| ArcCurve::new(curves::EASE_IN_OUT))
    }

    /// The stages both directions need: the tree keeps one shape across them.
    pub(super) fn stages(&self) -> Stages {
        let mut stages = Stages::default();
        for effect in self.enter.effects().chain(self.exit.effects()) {
            stages.include(effect);
        }
        stages
    }

    /// The pose at `progress` (`0` absent, `1` present) on `phase`'s side.
    pub(super) fn pose(&self, phase: Phase, progress: f64) -> Pose {
        let slots = match phase {
            Phase::Enter => &self.enter,
            Phase::Exit => &self.exit,
        };
        Pose::compose(slots.effects().map(|effect| (effect, progress)))
    }
}

impl Add for Transition {
    type Output = Self;

    fn add(self, right: Self) -> Self {
        Self {
            enter: self.enter.merged(right.enter),
            exit: self.exit.merged(right.exit),
            duration: right.duration.or(self.duration),
            curve: right.curve.or(self.curve),
        }
    }
}
