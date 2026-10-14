//! [`Effect`], the one visual change both compositions share, and the pose a
//! set of effects resolves to at one instant.

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
    /// Each intermediate saturates at `±f64::MAX`, so finite inputs never
    /// overflow into a non-finite value on the way: a zero scale after huge
    /// ones still collapses the child. A non-finite input rests instead of
    /// reaching the render tree.
    pub(super) fn compose<'a>(effects: impl IntoIterator<Item = (&'a Effect, f64)>) -> Self {
        let mut pose = Self::REST;
        // Scales about their alignments, which slides do not pass through.
        let mut offset = TranslationFraction::ZERO;
        let mut slide = TranslationFraction::ZERO;
        for (effect, t) in effects {
            match *effect {
                Effect::Fade { from, to } => {
                    pose.opacity = saturated(pose.opacity * saturated(from + (to - from) * t));
                }
                // Rotation about the centre commutes with a uniform scale
                // about the centre, so rotations apply innermost and leave
                // the scales' alignment offset untouched.
                Effect::Rotate { turns } => {
                    pose.turns = saturated(pose.turns + saturated(turns * (1.0 - t)));
                }
                // Scaling by `s` about the alignment point `a` (in half-sizes
                // from the centre) is scaling about the centre plus a
                // translation of `(1 − s)·a/2` child sizes; an earlier scale's
                // translation scales with it.
                Effect::Scale { from, alignment } => {
                    let s = saturated(from + (1.0 - from) * t);
                    pose.scale = saturated(pose.scale * s);
                    let pivot = |offset: f64, alignment: f64| {
                        saturated(saturated(s * offset) + saturated((1.0 - s) * alignment / 2.0))
                    };
                    offset = TranslationFraction::new(
                        pivot(offset.dx, alignment.x),
                        pivot(offset.dy, alignment.y),
                    );
                }
                Effect::Slide { from } => {
                    slide.dx = saturated(slide.dx + saturated(from.dx * (1.0 - t)));
                    slide.dy = saturated(slide.dy + saturated(from.dy * (1.0 - t)));
                }
            }
        }
        pose.slide = TranslationFraction::new(
            saturated(offset.dx + slide.dx),
            saturated(offset.dy + slide.dy),
        );
        pose.sanitized()
    }

    fn sanitized(self) -> Self {
        let or_rest = |value: f64, rest: f64| if value.is_finite() { value } else { rest };
        let slide = if self.slide.dx.is_finite() && self.slide.dy.is_finite() {
            self.slide
        } else {
            TranslationFraction::ZERO
        };
        Self {
            opacity: or_rest(self.opacity, 1.0).clamp(0.0, 1.0),
            turns: or_rest(self.turns, 0.0),
            scale: or_rest(self.scale, 1.0),
            slide,
        }
    }
}

/// `value` held within the finite range; NaN stays NaN for
/// [`Pose::sanitized`] to rest.
fn saturated(value: f64) -> f64 {
    if value.is_nan() {
        value
    } else {
        value.clamp(-f64::MAX, f64::MAX)
    }
}
