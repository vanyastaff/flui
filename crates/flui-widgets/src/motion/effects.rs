//! [`Effects`] — an ordered choreography of effects on one timeline.

use std::ops::Range;
use std::time::Duration;

use flui_animation::curve::{ArcCurve, Curve};
use flui_animation::{MotionSpec, Stagger, curves};
use flui_objects::TranslationFraction;
use flui_painting::Alignment;

use super::effect::{Effect, Pose, Stages};

/// An ordered timeline of [`Effect`]s, played by [`Animate`](super::Animate).
///
/// Each entry starts at a delay after the timeline's *base* and runs for a
/// duration along a curve. [`delay`](Self::delay), [`duration`](Self::duration)
/// and [`curve`](Self::curve) configure every entry added after them, so an
/// entry inherits its predecessor's timing until told otherwise.
/// [`then`](Self::then) moves the base to the end of the last entry and resets
/// the delay, so what follows plays after it. The timeline lasts until its
/// latest entry ends.
///
/// Without configuration an entry starts at the base and runs for
/// [`MotionSpec::DEFAULT_DURATION`] along `EASE_IN_OUT`, the implicit widgets'
/// default motion.
///
/// ```
/// use std::time::Duration;
/// use flui_widgets::motion::Effects;
///
/// let ms = Duration::from_millis;
/// let effects = Effects::new()
///     .duration(ms(300))
///     .fade_in()
///     .slide_y(0.2)
///     .then()
///     .scale(0.9);
/// let spans: Vec<_> = effects.spans().collect();
/// assert_eq!(spans, [ms(0)..ms(300), ms(0)..ms(300), ms(300)..ms(600)]);
/// assert_eq!(effects.total(), ms(600));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Effects {
    entries: Vec<Entry>,
    /// Where an entry's delay is measured from.
    base: Duration,
    /// The timing the next entry takes.
    delay: Duration,
    duration: Duration,
    curve: ArcCurve,
    /// Added to every entry: the stagger of this timeline's index.
    offset: Duration,
}

/// One effect placed on the timeline.
#[derive(Clone, Debug, PartialEq)]
struct Entry {
    effect: Effect,
    start: Duration,
    duration: Duration,
    curve: ArcCurve,
}

impl Default for Effects {
    fn default() -> Self {
        Self::new()
    }
}

impl Effects {
    /// An empty timeline.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            base: Duration::ZERO,
            delay: Duration::ZERO,
            duration: MotionSpec::DEFAULT_DURATION,
            curve: ArcCurve::new(curves::EASE_IN_OUT),
            offset: Duration::ZERO,
        }
    }

    /// Start the following entries `delay` after the base.
    #[must_use]
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// Run the following entries for `duration`.
    #[must_use]
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Ease the following entries along `curve`.
    #[must_use]
    pub fn curve(mut self, curve: impl Curve + Send + Sync + 'static) -> Self {
        self.curve = ArcCurve::new(curve);
        self
    }

    /// Play the following entries after the last one ends: the base moves to
    /// its end and the delay resets to zero; duration and curve carry on.
    /// Before any entry, the base stays at the start.
    #[must_use]
    pub fn then(mut self) -> Self {
        if let Some(last) = self.entries.last() {
            self.base = last.start.saturating_add(last.duration);
        }
        self.delay = Duration::ZERO;
        self
    }

    /// Delay the whole timeline by `stagger`'s delay for element `index` of
    /// `count`, so the same effects on siblings play one after another. The
    /// delay adds to every entry, wherever this call sits in the chain; a
    /// later call replaces an earlier one.
    #[must_use]
    pub fn stagger(mut self, stagger: Stagger, index: usize, count: usize) -> Self {
        self.offset = stagger.delay(index, count);
        self
    }

    /// Add `effect` with the configured delay, duration and curve.
    #[must_use]
    pub fn effect(mut self, effect: Effect) -> Self {
        self.entries.push(Entry {
            effect,
            start: self.base.saturating_add(self.delay),
            duration: self.duration,
            curve: self.curve.clone(),
        });
        self
    }

    /// Fade from transparent to opaque.
    #[must_use]
    pub fn fade_in(self) -> Self {
        self.effect(Effect::FADE_IN)
    }

    /// Fade from opaque to transparent.
    #[must_use]
    pub fn fade_out(self) -> Self {
        self.effect(Effect::FADE_OUT)
    }

    /// Slide in from `from`, a fraction of the child's size.
    #[must_use]
    pub fn slide(self, from: TranslationFraction) -> Self {
        self.effect(Effect::Slide { from })
    }

    /// Slide in vertically from `dy` child heights (positive starts below).
    #[must_use]
    pub fn slide_y(self, dy: f64) -> Self {
        self.slide(TranslationFraction::new(0.0, dy))
    }

    /// Slide in horizontally from `dx` child widths (positive starts right).
    #[must_use]
    pub fn slide_x(self, dx: f64) -> Self {
        self.slide(TranslationFraction::new(dx, 0.0))
    }

    /// Scale about the centre from `from` to the natural size.
    #[must_use]
    pub fn scale(self, from: f64) -> Self {
        self.effect(Effect::Scale {
            from,
            alignment: Alignment::CENTER,
        })
    }

    /// Rotate about the centre from `turns` to upright.
    #[must_use]
    pub fn rotate(self, turns: f64) -> Self {
        self.effect(Effect::Rotate { turns })
    }

    /// Each entry's span on the timeline, in the order the entries were added,
    /// stagger included.
    pub fn spans(&self) -> impl ExactSizeIterator<Item = Range<Duration>> + '_ {
        self.entries.iter().map(|entry| {
            let start = entry.start.saturating_add(self.offset);
            start..start.saturating_add(entry.duration)
        })
    }

    /// When the latest entry ends; zero for an empty timeline.
    #[must_use]
    pub fn total(&self) -> Duration {
        self.spans()
            .map(|span| span.end)
            .max()
            .unwrap_or(Duration::ZERO)
    }

    /// The stages the composed tree needs.
    pub(super) fn stages(&self) -> Stages {
        let mut stages = Stages::default();
        for entry in &self.entries {
            stages.include(&entry.effect);
        }
        stages
    }

    /// The pose at `position`, a fraction of [`total`](Self::total). Before
    /// its span an entry shows its start, after it its rest; a zero-length
    /// entry switches at its start.
    pub(super) fn pose(&self, position: f64) -> Pose {
        let now = self.total().as_secs_f64() * position;
        Pose::compose(self.spans().zip(&self.entries).map(|(span, entry)| {
            let start = span.start.as_secs_f64();
            let length = entry.duration.as_secs_f64();
            let linear = if length > 0.0 {
                ((now - start) / length).clamp(0.0, 1.0)
            } else if now >= start {
                1.0
            } else {
                0.0
            };
            (&entry.effect, entry.curve.transform(linear))
        }))
    }
}
