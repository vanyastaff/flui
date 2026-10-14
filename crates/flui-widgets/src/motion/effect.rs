//! [`Effect`], the one visual change both compositions share, and the pose a
//! set of effects resolves to at one instant.

use flui_foundation::geometry::Lerp;
use flui_objects::TranslationFraction;
use flui_painting::Alignment;

/// One visual change of a child, from a starting look to its resting look.
///
/// An effect is evaluated at a local progress `t` (already eased): `0` is its
/// starting look, `1` its resting look. Every effect except [`Fade`] rests at
/// the untransformed child, so a settled effect leaves the child as laid out.
///
/// The set is closed: [`Transition`](super::Transition) gives each kind one
/// slot, and [`Animate`](super::Animate) composes kinds in a fixed order
/// (opacity, then rotation and scale, then translation) whatever order they
/// were listed in, so a slide is never scaled by a later scale.
///
/// [`Fade`]: Effect::Fade
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Effect {
    /// Opacity from `from` to `to`, each in `0..=1`.
    Fade {
        /// Opacity at the start.
        from: f64,
        /// Opacity at rest.
        to: f64,
    },
    /// Translation by a fraction of the child's own size, settling at zero.
    /// `dx` is in canvas coordinates: positive moves right.
    Slide {
        /// Translation at the start.
        from: TranslationFraction,
    },
    /// Uniform scale about `alignment` within the child, settling at `1`.
    Scale {
        /// Scale factor at the start.
        from: f64,
        /// The fixed point of the scale: [`Alignment::CENTER`] scales about
        /// the middle, [`Alignment::TOP_LEFT`] grows out of the top-left
        /// corner.
        alignment: Alignment,
    },
    /// Rotation about the child's centre, settling upright.
    Rotate {
        /// Rotation at the start, in turns (`1.0` is one clockwise
        /// revolution in the y-down canvas).
        turns: f64,
    },
}

impl Effect {
    /// A fade from transparent to opaque.
    pub const FADE_IN: Self = Self::Fade { from: 0.0, to: 1.0 };
    /// A fade from opaque to transparent.
    pub const FADE_OUT: Self = Self::Fade { from: 1.0, to: 0.0 };

    /// The slot this effect occupies in a [`Transition`](super::Transition).
    pub(super) const fn kind(&self) -> Kind {
        match self {
            Self::Fade { .. } => Kind::Fade,
            Self::Slide { .. } => Kind::Slide,
            Self::Scale { .. } => Kind::Scale,
            Self::Rotate { .. } => Kind::Rotate,
        }
    }

    /// Whether resolving this effect moves the child, so the composed tree
    /// needs a translation stage for it.
    pub(super) fn translates(&self) -> bool {
        match self {
            Self::Slide { .. } => true,
            Self::Scale { alignment, .. } => *alignment != Alignment::CENTER,
            Self::Fade { .. } | Self::Rotate { .. } => false,
        }
    }
}

/// The slot an [`Effect`] fills; also its rank in the fixed composition order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Fade,
    Rotate,
    Scale,
    Slide,
}

/// The stages a composed tree needs, fixed by which effects are configured
/// (never by progress), so a tick never changes the tree's shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Stages {
    pub(super) fade: bool,
    pub(super) rotate: bool,
    pub(super) scale: bool,
    pub(super) slide: bool,
}

impl Stages {
    pub(super) fn include(&mut self, effect: &Effect) {
        match effect.kind() {
            Kind::Fade => self.fade = true,
            Kind::Rotate => self.rotate = true,
            Kind::Scale => self.scale = true,
            Kind::Slide => {}
        }
        self.slide |= effect.translates();
    }
}

/// What a set of effects resolves to at one instant: one value per stage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Pose {
    pub(super) opacity: f64,
    pub(super) turns: f64,
    pub(super) scale: f64,
    pub(super) slide: TranslationFraction,
}

impl Pose {
    /// The untransformed, opaque child.
    pub(super) const REST: Self = Self {
        opacity: 1.0,
        turns: 0.0,
        scale: 1.0,
        slide: TranslationFraction::ZERO,
    };

    /// Compose `effects`, each at its own eased progress, in the fixed order:
    /// opacity multiplies, rotation adds about the centre, each scale applies
    /// about its alignment after the rotations, and translation adds last.
    /// One pass, without allocating: every stage's rule is order-free except
    /// scale against scale, which keeps the given order.
    ///
    /// Each effect interpolates endpoint-exactly (its start at `t = 0`, its
    /// rest at `t = 1`). An effect with a non-finite parameter, progress or
    /// interpolated value is the identity, so its siblings still apply.
    /// Opacities, scales and the scales' alignment offset accumulate as
    /// mantissa and exponent, so a representable result is published whatever
    /// the order of huge and tiny factors; a composed component that is not
    /// representable rests.
    pub(super) fn compose<'a>(effects: impl IntoIterator<Item = (&'a Effect, f64)>) -> Self {
        let mut opacity = Wide::ONE;
        let mut scale = Wide::ONE;
        let mut turns = 0.0;
        // The translation of the scales about their alignments, which slides
        // do not pass through; wide, so a huge intermediate offset that later
        // scales shrink back stays exact.
        let mut offset = (Wide::ZERO, Wide::ZERO);
        let mut slide = TranslationFraction::ZERO;
        for (effect, t) in effects {
            if !t.is_finite() {
                continue;
            }
            match *effect {
                Effect::Fade { from, to } => {
                    if let Some(value) = interpolate(from, to, t) {
                        opacity = opacity.times(Wide::of(value));
                    }
                }
                // Rotation about the centre commutes with a uniform scale
                // about the centre, so rotations apply innermost and leave
                // the scales' alignment offset untouched.
                Effect::Rotate { turns: start } => {
                    if let Some(value) = interpolate(start, 0.0, t) {
                        turns += value;
                    }
                }
                // Scaling by `s` about the alignment point `a` (in half-sizes
                // from the centre) is scaling about the centre plus a
                // translation of `(1 − s)·a/2` child sizes; an earlier scale's
                // translation scales with it.
                Effect::Scale { from, alignment } => {
                    let Some(s) = interpolate(from, 1.0, t) else {
                        continue;
                    };
                    if !(alignment.x.is_finite() && alignment.y.is_finite()) {
                        continue;
                    }
                    let pivot = Wide::of(1.0 - s);
                    let s = Wide::of(s);
                    let step = |offset: Wide, alignment: f64| {
                        offset
                            .times(s)
                            .plus(pivot.times(Wide::of(alignment)).times(Wide::HALF))
                    };
                    scale = scale.times(s);
                    offset = (step(offset.0, alignment.x), step(offset.1, alignment.y));
                }
                Effect::Slide { from } => {
                    if let (Some(dx), Some(dy)) =
                        (interpolate(from.dx, 0.0, t), interpolate(from.dy, 0.0, t))
                    {
                        slide.dx += dx;
                        slide.dy += dy;
                    }
                }
            }
        }
        Self {
            opacity: opacity.value(),
            turns,
            scale: scale.value(),
            slide: TranslationFraction::new(
                translation(offset.0, slide.dx),
                translation(offset.1, slide.dy),
            ),
        }
        .sanitized()
    }

    fn sanitized(self) -> Self {
        let or_rest = |value: f64, rest: f64| if value.is_finite() { value } else { rest };
        let rest = Self::REST;
        let slide = if self.slide.dx.is_finite() && self.slide.dy.is_finite() {
            self.slide
        } else {
            rest.slide
        };
        Self {
            opacity: or_rest(self.opacity, rest.opacity).clamp(0.0, 1.0),
            turns: or_rest(self.turns, rest.turns),
            scale: or_rest(self.scale, rest.scale),
            slide,
        }
    }
}

/// One axis of the composed translation: the scales' wide offset plus the
/// slides' sum, non-finite (so it rests) when the slides overflowed.
fn translation(offset: Wide, slide: f64) -> f64 {
    if slide.is_finite() {
        offset.plus(Wide::of(slide)).value()
    } else {
        f64::NAN
    }
}

/// `from` toward `to` at `t`, exact at both ends; `None` for a non-finite
/// endpoint or result, which leaves the effect out.
fn interpolate(from: f64, to: f64, t: f64) -> Option<f64> {
    if !(from.is_finite() && to.is_finite()) {
        return None;
    }
    Some(from.lerp_to(&to, t)).filter(|value| value.is_finite())
}

/// A number held as `mantissa · 2^exponent`, with `|mantissa|` in `[1, 2)`
/// (or zero), so products and sums of finite values never overflow or
/// underflow on the way, whatever order huge and tiny values arrive in; only
/// the final [`value`](Self::value) is rounded into `f64`.
#[derive(Clone, Copy)]
struct Wide {
    mantissa: f64,
    exponent: i32,
}

impl Wide {
    const ZERO: Self = Self {
        mantissa: 0.0,
        exponent: 0,
    };
    const ONE: Self = Self {
        mantissa: 1.0,
        exponent: 0,
    };
    /// One half, exact.
    const HALF: Self = Self {
        mantissa: 1.0,
        exponent: -1,
    };

    /// A finite `value`.
    fn of(value: f64) -> Self {
        if value == 0.0 {
            return Self::ZERO;
        }
        let (mantissa, exponent) = split(value);
        Self { mantissa, exponent }
    }

    fn times(self, other: Self) -> Self {
        if self.mantissa == 0.0 || other.mantissa == 0.0 {
            return Self::ZERO;
        }
        let (mantissa, carry) = split(self.mantissa * other.mantissa);
        Self {
            mantissa,
            exponent: self
                .exponent
                .saturating_add(other.exponent)
                .saturating_add(carry),
        }
    }

    fn plus(self, other: Self) -> Self {
        if other.mantissa == 0.0 {
            return self;
        }
        if self.mantissa == 0.0 {
            return other;
        }
        let (large, small) = if self.exponent >= other.exponent {
            (self, other)
        } else {
            (other, self)
        };
        // The smaller term, aligned to the larger one's exponent; it rounds
        // away entirely once it is more than a mantissa's width below.
        let aligned = scaled(
            small.mantissa,
            small.exponent.saturating_sub(large.exponent),
        );
        let sum = large.mantissa + aligned;
        if sum == 0.0 {
            return Self::ZERO;
        }
        let (mantissa, carry) = split(sum);
        Self {
            mantissa,
            exponent: large.exponent.saturating_add(carry),
        }
    }

    /// The number, or a non-finite value when it is not representable.
    fn value(self) -> f64 {
        scaled(self.mantissa, self.exponent)
    }
}

/// `mantissa · 2^exponent` for `|mantissa|` in `[1, 2)` (or zero), stepping
/// the exponent so no intermediate power of two overflows.
fn scaled(mantissa: f64, exponent: i32) -> f64 {
    const STEP: i32 = 1000;
    // Beyond these the result is infinite or zero whatever the mantissa.
    if exponent > 1100 {
        return mantissa * f64::INFINITY;
    }
    if exponent < -1200 {
        return mantissa * 0.0;
    }
    let mut value = mantissa;
    let mut exponent = exponent;
    while exponent > STEP {
        value *= 2f64.powi(STEP);
        exponent -= STEP;
    }
    while exponent < -STEP {
        value *= 2f64.powi(-STEP);
        exponent += STEP;
    }
    value * 2f64.powi(exponent)
}

/// A finite, non-zero `value` as `mantissa · 2^exponent` with `|mantissa|`
/// in `[1, 2)`; subnormals are normalised first.
fn split(value: f64) -> (f64, i32) {
    const EXPONENT_MASK: u64 = 0x7ff << 52;
    let biased = |value: f64| {
        i32::try_from((value.to_bits() & EXPONENT_MASK) >> 52)
            .expect("BUG: an 11-bit exponent fits in i32")
    };
    let (normal, shift) = if biased(value) == 0 {
        (value * 2f64.powi(54), -54)
    } else {
        (value, 0)
    };
    let mantissa = f64::from_bits((normal.to_bits() & !EXPONENT_MASK) | (1023 << 52));
    (mantissa, biased(normal) - 1023 + shift)
}
