//! [`ParticleConfig`]: what a [`ParticleEmitter`](super::ParticleEmitter) emits, built and
//! checked once by [`ParticleConfigBuilder::build`].

use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::time::Duration;

use flui_foundation::geometry::{Offset, Point, Rect};
use flui_painting::paint::Image;
use flui_painting::styling::Color;

/// The most particles one emitter keeps alive.
pub const MAX_CAPACITY: usize = 4096;
/// The capacity of a configuration that names none.
pub const DEFAULT_CAPACITY: usize = 512;

/// Whether an area source emits from anywhere inside it or only from its edge.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    /// Anywhere inside, uniformly by area.
    Area,
    /// Only on the outline, uniformly by length.
    Outline,
}

/// Where new particles appear, in the emitter's box as fractions of its size: `(0, 0)` is
/// the top-left corner, `(1, 1)` the bottom-right, so the source follows the box when it
/// resizes. A circle or ring is mapped onto the box the same way, so in a box that is not
/// square it is an ellipse.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EmitterSource {
    /// One point.
    Point(Point<f64>),
    /// Anywhere on the segment between two points.
    Line {
        /// One end.
        from: Point<f64>,
        /// The other end.
        to: Point<f64>,
    },
    /// A rectangle's area or outline.
    Rect {
        /// The rectangle.
        rect: Rect<f64>,
        /// Area or outline.
        coverage: Coverage,
    },
    /// A circle's area or outline.
    Circle {
        /// The centre.
        center: Point<f64>,
        /// The radius, a fraction of the box like the centre.
        radius: f64,
        /// Area or outline.
        coverage: Coverage,
    },
    /// The band between two circles about one centre, uniformly by area.
    Ring {
        /// The centre.
        center: Point<f64>,
        /// The inner radius.
        inner: f64,
        /// The outer radius, at least the inner one.
        outer: f64,
    },
}

impl EmitterSource {
    /// One point.
    #[must_use]
    pub const fn point(at: Point<f64>) -> Self {
        Self::Point(at)
    }

    /// The segment from `from` to `to`.
    #[must_use]
    pub const fn line(from: Point<f64>, to: Point<f64>) -> Self {
        Self::Line { from, to }
    }

    /// `rect`'s area or outline.
    #[must_use]
    pub const fn rect(rect: Rect<f64>, coverage: Coverage) -> Self {
        Self::Rect { rect, coverage }
    }

    /// A circle's area or outline.
    #[must_use]
    pub const fn circle(center: Point<f64>, radius: f64, coverage: Coverage) -> Self {
        Self::Circle {
            center,
            radius,
            coverage,
        }
    }

    /// The band between radii `inner` and `outer` about `center`.
    #[must_use]
    pub const fn ring(center: Point<f64>, inner: f64, outer: f64) -> Self {
        Self::Ring {
            center,
            inner,
            outer,
        }
    }

    /// The point of the source that `u` and `v`, each in `[0, 1)`, pick, in fractions of the
    /// box.
    pub(super) fn sample(&self, u: f64, v: f64) -> Point<f64> {
        match *self {
            Self::Point(at) => at,
            Self::Line { from, to } => lerp_point(from, to, u),
            Self::Rect {
                rect,
                coverage: Coverage::Area,
            } => Point::new(
                rect.left() + u * rect.width(),
                rect.top() + v * rect.height(),
            ),
            Self::Rect {
                rect,
                coverage: Coverage::Outline,
            } => {
                let (w, h) = (rect.width(), rect.height());
                let along = u * 2.0 * (w + h);
                let (left, top, right, bottom) =
                    (rect.left(), rect.top(), rect.right(), rect.bottom());
                if along < w {
                    Point::new(left + along, top)
                } else if along < w + h {
                    Point::new(right, top + (along - w))
                } else if along < 2.0 * w + h {
                    Point::new(right - (along - w - h), bottom)
                } else {
                    Point::new(left, bottom - (along - 2.0 * w - h))
                }
            }
            Self::Circle {
                center,
                radius,
                coverage,
            } => {
                let distance = match coverage {
                    Coverage::Area => radius * u.sqrt(),
                    Coverage::Outline => radius,
                };
                polar(center, distance, v * TAU)
            }
            Self::Ring {
                center,
                inner,
                outer,
            } => {
                let squared = inner * inner + u * (outer * outer - inner * inner);
                polar(center, squared.sqrt(), v * TAU)
            }
        }
    }

    fn check(&self) -> Result<(), ParticleConfigError> {
        let field = ParticleField::Source;
        let finite = |values: &[f64]| {
            values
                .iter()
                .all(|value| value.is_finite())
                .then_some(())
                .ok_or(ParticleConfigError::NonFinite { field })
        };
        match *self {
            Self::Point(at) => finite(&[at.x, at.y]),
            Self::Line { from, to } => {
                finite(&[from.x, from.y, to.x, to.y, to.x - from.x, to.y - from.y])
            }
            Self::Rect { rect, .. } => {
                let (w, h) = (rect.width(), rect.height());
                finite(&[
                    rect.left(),
                    rect.top(),
                    rect.right(),
                    rect.bottom(),
                    2.0 * (w + h),
                ])?;
                if w < 0.0 || h < 0.0 {
                    return Err(ParticleConfigError::Negative { field });
                }
                Ok(())
            }
            Self::Circle { center, radius, .. } => {
                finite(&[
                    center.x,
                    center.y,
                    radius,
                    center.x + radius,
                    center.y + radius,
                ])?;
                nonnegative(field, radius)
            }
            Self::Ring {
                center,
                inner,
                outer,
            } => {
                finite(&[
                    center.x,
                    center.y,
                    inner,
                    outer,
                    outer * outer,
                    center.x + outer,
                    center.y + outer,
                ])?;
                nonnegative(field, inner)?;
                if inner > outer {
                    return Err(ParticleConfigError::InvertedRange { field });
                }
                Ok(())
            }
        }
    }
}

fn lerp_point(a: Point<f64>, b: Point<f64>, t: f64) -> Point<f64> {
    Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}

fn polar(center: Point<f64>, distance: f64, angle: f64) -> Point<f64> {
    let (sin, cos) = angle.sin_cos();
    Point::new(center.x + distance * cos, center.y + distance * sin)
}

fn nonnegative(field: ParticleField, value: f64) -> Result<(), ParticleConfigError> {
    if value < 0.0 {
        Err(ParticleConfigError::Negative { field })
    } else {
        Ok(())
    }
}

/// How long an emitter emits at its rate.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmitterDuration {
    /// For as long as it is mounted.
    Forever,
    /// For this long after it starts; then it finishes once its last particle is gone.
    For(Duration),
}

/// What each particle is drawn with: a sprite drawn once into the emitter's atlas and tinted
/// by the particle's color.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum Sprite {
    /// A filled square.
    Square,
    /// A filled circle.
    Circle,
    /// A five-pointed star.
    Star,
    /// An image of the app's, tinted like the built-in shapes: a white image takes the
    /// particle's color exactly.
    Image(Image),
}

/// The value of a configuration that [`ParticleConfigError`] names.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParticleField {
    /// [`ParticleConfigBuilder::source`].
    Source,
    /// [`ParticleConfigBuilder::rate`].
    Rate,
    /// [`ParticleConfigBuilder::lifetime`].
    Lifetime,
    /// [`ParticleConfigBuilder::speed`].
    Speed,
    /// [`ParticleConfigBuilder::direction`].
    Direction,
    /// [`ParticleConfigBuilder::size`].
    Size,
    /// [`ParticleConfigBuilder::rotation`].
    Rotation,
    /// [`ParticleConfigBuilder::spin`].
    Spin,
    /// [`ParticleConfigBuilder::gravity`].
    Gravity,
    /// [`ParticleConfigBuilder::drag`].
    Drag,
    /// [`ParticleConfigBuilder::opacity`].
    Opacity,
    /// [`ParticleConfigBuilder::scale`].
    Scale,
    /// [`ParticleConfigBuilder::palette`].
    Palette,
}

/// Why [`ParticleConfigBuilder::build`] refused a configuration.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ParticleConfigError {
    /// A value, or a quantity derived from it (a range's width, a source's extent), is not
    /// finite.
    #[error("particle {field:?} is not finite")]
    NonFinite {
        /// The value.
        field: ParticleField,
    },
    /// A value that must be at least zero is negative.
    #[error("particle {field:?} is negative")]
    Negative {
        /// The value.
        field: ParticleField,
    },
    /// A range's lower bound is above its upper bound.
    #[error("particle {field:?} range is inverted")]
    InvertedRange {
        /// The value.
        field: ParticleField,
    },
    /// An opacity is outside `[0, 1]`.
    #[error("particle {field:?} is outside [0, 1]")]
    OutOfUnitRange {
        /// The value.
        field: ParticleField,
    },
    /// The shortest lifetime is not positive.
    #[error("particle lifetime must be positive")]
    NonPositiveLifetime,
    /// A palette was given with no colors.
    #[error("particle palette is empty")]
    EmptyPalette,
    /// The capacity is above [`MAX_CAPACITY`].
    #[error("particle capacity {capacity} is above {max}")]
    CapacityTooLarge {
        /// The capacity asked for.
        capacity: usize,
        /// [`MAX_CAPACITY`].
        max: usize,
    },
}

/// A closed range a particle's value is drawn from, uniformly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Spread {
    pub(super) min: f64,
    pub(super) max: f64,
}

impl Spread {
    const fn fixed(value: f64) -> Self {
        Self {
            min: value,
            max: value,
        }
    }

    const fn new(min: f64, max: f64) -> Self {
        Self { min, max }
    }

    /// The value `u` in `[0, 1)` picks.
    pub(super) fn at(self, u: f64) -> f64 {
        self.min + u * (self.max - self.min)
    }

    fn check(self, field: ParticleField) -> Result<Self, ParticleConfigError> {
        if !self.min.is_finite() || !self.max.is_finite() || !(self.max - self.min).is_finite() {
            return Err(ParticleConfigError::NonFinite { field });
        }
        if self.min > self.max {
            return Err(ParticleConfigError::InvertedRange { field });
        }
        Ok(self)
    }
}

/// A checked particle configuration; build one with [`ParticleConfig::builder`] or start
/// from a preset ([`confetti`](Self::confetti), [`sparkle`](Self::sparkle),
/// [`snow`](Self::snow)).
///
/// # Examples
///
/// ```
/// use flui_widgets::motion::particles::{EmitterSource, ParticleConfig, Sprite};
/// use flui_foundation::geometry::Point;
///
/// let embers = ParticleConfig::builder()
///     .source(EmitterSource::point(Point::new(0.5, 1.0)))
///     .rate(40.0)
///     .lifetime(0.8, 1.6)
///     .speed(40.0, 120.0)
///     .direction(-2.0, -1.1)
///     .sprite(Sprite::Circle)
///     .build()?;
/// # let _ = embers;
/// # Ok::<(), flui_widgets::motion::particles::ParticleConfigError>(())
/// ```
#[derive(Debug, Clone)]
pub struct ParticleConfig {
    pub(super) source: EmitterSource,
    pub(super) rate: f64,
    pub(super) duration: EmitterDuration,
    pub(super) capacity: usize,
    pub(super) lifetime: Spread,
    pub(super) speed: Spread,
    pub(super) direction: Spread,
    pub(super) size: Spread,
    pub(super) rotation: Spread,
    pub(super) spin: Spread,
    pub(super) gravity: Offset<f64>,
    pub(super) drag: f64,
    pub(super) color: (Color, Color),
    pub(super) palette: Option<Vec<Color>>,
    pub(super) opacity: (f64, f64),
    pub(super) scale: (f64, f64),
    pub(super) sprite: Sprite,
    pub(super) seed: u64,
}

impl ParticleConfig {
    /// A builder starting from one white square a second from the box's centre, living a
    /// second, at rest, for ever.
    pub fn builder() -> ParticleConfigBuilder {
        ParticleConfigBuilder {
            config: Self {
                source: EmitterSource::Point(Point::new(0.5, 0.5)),
                rate: 1.0,
                duration: EmitterDuration::Forever,
                capacity: DEFAULT_CAPACITY,
                lifetime: Spread::fixed(1.0),
                speed: Spread::fixed(0.0),
                direction: Spread::fixed(0.0),
                size: Spread::fixed(8.0),
                rotation: Spread::fixed(0.0),
                spin: Spread::fixed(0.0),
                gravity: Offset::new(0.0, 0.0),
                drag: 0.0,
                color: (Color::WHITE, Color::WHITE),
                palette: None,
                opacity: (1.0, 1.0),
                scale: (1.0, 1.0),
                sprite: Sprite::Square,
                seed: 0,
            },
        }
    }

    /// Confetti: bursts only, squares in six colors thrown up and spread from where the
    /// burst is, falling and tumbling under gravity for three seconds or so.
    #[must_use]
    pub fn confetti() -> Self {
        Self::builder()
            .source(EmitterSource::line(
                Point::new(0.0, 0.0),
                Point::new(1.0, 0.0),
            ))
            .rate(0.0)
            .lifetime(2.5, 4.0)
            .speed(250.0, 650.0)
            .direction(-PI + 0.6, -0.6)
            .size(6.0, 11.0)
            .rotation(0.0, TAU)
            .spin(-8.0, 8.0)
            .gravity(Offset::new(0.0, 700.0))
            .drag(1.2)
            .palette(vec![
                Color::rgb(0xF4, 0x43, 0x36),
                Color::rgb(0xFF, 0xC1, 0x07),
                Color::rgb(0x4C, 0xAF, 0x50),
                Color::rgb(0x21, 0x96, 0xF3),
                Color::rgb(0x9C, 0x27, 0xB0),
                Color::rgb(0xFF, 0x98, 0x00),
            ])
            .opacity(1.0, 0.0)
            .sprite(Sprite::Square)
            .build()
            .expect("BUG: the confetti preset is a valid configuration")
    }

    /// Sparkle: stars twinkling out of the box's centre, white fading to gold, each fading
    /// and shrinking over its second of life so the glow never flashes.
    #[must_use]
    pub fn sparkle() -> Self {
        Self::builder()
            .source(EmitterSource::circle(
                Point::new(0.5, 0.5),
                0.15,
                Coverage::Area,
            ))
            .rate(24.0)
            .lifetime(0.7, 1.3)
            .speed(10.0, 50.0)
            .direction(0.0, TAU)
            .size(6.0, 12.0)
            .rotation(0.0, TAU)
            .spin(-1.5, 1.5)
            .color(Color::WHITE, Color::rgb(0xFF, 0xD5, 0x4F))
            .opacity(0.9, 0.0)
            .scale(1.0, 0.3)
            .sprite(Sprite::Star)
            .build()
            .expect("BUG: the sparkle preset is a valid configuration")
    }

    /// Snow: white flakes drifting down from the box's top edge for ever.
    #[must_use]
    pub fn snow() -> Self {
        Self::builder()
            .source(EmitterSource::line(
                Point::new(0.0, 0.0),
                Point::new(1.0, 0.0),
            ))
            .rate(18.0)
            .lifetime(6.0, 10.0)
            .speed(20.0, 50.0)
            .direction(FRAC_PI_2 - 0.35, FRAC_PI_2 + 0.35)
            .size(2.0, 5.0)
            .gravity(Offset::new(0.0, 6.0))
            .drag(0.1)
            .color(Color::WHITE, Color::WHITE)
            .opacity(0.9, 0.9)
            .sprite(Sprite::Circle)
            .build()
            .expect("BUG: the snow preset is a valid configuration")
    }

    /// A builder starting from this configuration.
    pub fn to_builder(&self) -> ParticleConfigBuilder {
        ParticleConfigBuilder {
            config: self.clone(),
        }
    }

    /// How many particles the emitter keeps alive at most.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

/// Builds a [`ParticleConfig`]; [`build`](Self::build) checks it.
#[derive(Debug, Clone)]
#[must_use]
pub struct ParticleConfigBuilder {
    config: ParticleConfig,
}

impl ParticleConfigBuilder {
    /// Where particles appear when a rate or [`ParticleController::emit_burst`](super::ParticleController::emit_burst)
    /// emits them.
    pub fn source(mut self, source: EmitterSource) -> Self {
        self.config.source = source;
        self
    }

    /// Particles a second, at least zero; zero emits only in bursts.
    pub fn rate(mut self, per_second: f64) -> Self {
        self.config.rate = per_second;
        self
    }

    /// How long the rate emits.
    pub fn duration(mut self, duration: EmitterDuration) -> Self {
        self.config.duration = duration;
        self
    }

    /// The most particles alive at once, at most [`MAX_CAPACITY`].
    pub fn capacity(mut self, capacity: usize) -> Self {
        self.config.capacity = capacity;
        self
    }

    /// Each particle lives between `min` and `max` seconds; `min` must be positive.
    pub fn lifetime(mut self, min: f64, max: f64) -> Self {
        self.config.lifetime = Spread::new(min, max);
        self
    }

    /// The starting speed, in logical pixels a second, at least zero.
    pub fn speed(mut self, min: f64, max: f64) -> Self {
        self.config.speed = Spread::new(min, max);
        self
    }

    /// The starting direction, in radians clockwise from the positive x axis (`π/2` is
    /// down).
    pub fn direction(mut self, min: f64, max: f64) -> Self {
        self.config.direction = Spread::new(min, max);
        self
    }

    /// The starting size, the sprite's side in logical pixels, at least zero.
    pub fn size(mut self, min: f64, max: f64) -> Self {
        self.config.size = Spread::new(min, max);
        self
    }

    /// The starting rotation, in radians.
    pub fn rotation(mut self, min: f64, max: f64) -> Self {
        self.config.rotation = Spread::new(min, max);
        self
    }

    /// The angular velocity, in radians a second.
    pub fn spin(mut self, min: f64, max: f64) -> Self {
        self.config.spin = Spread::new(min, max);
        self
    }

    /// The acceleration every particle feels, in logical pixels a second squared.
    pub fn gravity(mut self, gravity: Offset<f64>) -> Self {
        self.config.gravity = gravity;
        self
    }

    /// How fast motion decays, a fraction of the velocity a second, at least zero.
    pub fn drag(mut self, drag: f64) -> Self {
        self.config.drag = drag;
        self
    }

    /// The color over each particle's life, from `start` to `end`. A palette replaces it.
    pub fn color(mut self, start: Color, end: Color) -> Self {
        self.config.color = (start, end);
        self.config.palette = None;
        self
    }

    /// One color per particle, picked at random from `colors`, for its whole life.
    pub fn palette(mut self, colors: Vec<Color>) -> Self {
        self.config.palette = Some(colors);
        self
    }

    /// The opacity over each particle's life, from `start` to `end`, each in `[0, 1]`.
    pub fn opacity(mut self, start: f64, end: f64) -> Self {
        self.config.opacity = (start, end);
        self
    }

    /// The size multiplier over each particle's life, from `start` to `end`, each at least
    /// zero.
    pub fn scale(mut self, start: f64, end: f64) -> Self {
        self.config.scale = (start, end);
        self
    }

    /// What each particle is drawn with.
    pub fn sprite(mut self, sprite: Sprite) -> Self {
        self.config.sprite = sprite;
        self
    }

    /// The seed of the emitter's random numbers: the same seed, configuration and frames
    /// give the same particles.
    pub fn seed(mut self, seed: u64) -> Self {
        self.config.seed = seed;
        self
    }

    /// The checked configuration.
    ///
    /// # Errors
    ///
    /// [`ParticleConfigError`] when a value, or a quantity derived from it, is not finite,
    /// a rate, drag, size, speed or scale is negative, a range is inverted, an opacity is
    /// outside `[0, 1]`, the shortest lifetime is not positive, a palette is empty, or the
    /// capacity is above [`MAX_CAPACITY`].
    pub fn build(self) -> Result<ParticleConfig, ParticleConfigError> {
        use ParticleConfigError as E;
        use ParticleField as F;
        let config = self.config;
        config.source.check()?;
        let scalar = |field, value: f64| {
            if value.is_finite() {
                nonnegative(field, value)
            } else {
                Err(E::NonFinite { field })
            }
        };
        scalar(F::Rate, config.rate)?;
        scalar(F::Drag, config.drag)?;
        let lifetime = config.lifetime.check(F::Lifetime)?;
        if lifetime.min <= 0.0 {
            return Err(E::NonPositiveLifetime);
        }
        for (field, spread) in [(F::Speed, config.speed), (F::Size, config.size)] {
            scalar(field, spread.check(field)?.min)?;
        }
        config.direction.check(F::Direction)?;
        config.rotation.check(F::Rotation)?;
        config.spin.check(F::Spin)?;
        if !config.gravity.dx.is_finite() || !config.gravity.dy.is_finite() {
            return Err(E::NonFinite { field: F::Gravity });
        }
        for value in [config.opacity.0, config.opacity.1] {
            if !value.is_finite() {
                return Err(E::NonFinite { field: F::Opacity });
            }
            if !(0.0..=1.0).contains(&value) {
                return Err(E::OutOfUnitRange { field: F::Opacity });
            }
        }
        for value in [config.scale.0, config.scale.1] {
            scalar(F::Scale, value)?;
        }
        if config.palette.as_ref().is_some_and(Vec::is_empty) {
            return Err(E::EmptyPalette);
        }
        if config.capacity > MAX_CAPACITY {
            return Err(E::CapacityTooLarge {
                capacity: config.capacity,
                max: MAX_CAPACITY,
            });
        }
        Ok(config)
    }
}
