//! Render-object update invalidation contracts.

use std::ops::{BitOr, BitOrAssign};

const LAYOUT_BIT: u8 = 1 << 0;
const PAINT_BIT: u8 = 1 << 1;
const COMPOSITING_BITS_BIT: u8 = 1 << 2;
const SEMANTICS_BIT: u8 = 1 << 3;

/// The pipeline work required after a render object's configuration changes.
///
/// The representation is intentionally opaque. Use the named constants and
/// [`union`](Self::union) (or `|`) to compose independent effects.
///
/// ```
/// use flui_rendering::RenderUpdateImpact;
///
/// let impact = RenderUpdateImpact::PAINT | RenderUpdateImpact::SEMANTICS;
/// assert!(impact.needs_paint());
/// assert!(impact.needs_semantics_update());
/// assert!(!impact.needs_layout());
/// ```
#[must_use = "render update impacts must be applied to the PipelineOwner"]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RenderUpdateImpact(u8);

impl RenderUpdateImpact {
    /// No pipeline work is required.
    pub const NONE: Self = Self(0);
    /// Repaint the render object.
    pub const PAINT: Self = Self(PAINT_BIT);
    /// Relayout the render object; layout will schedule the resulting paint.
    pub const LAYOUT: Self = Self(LAYOUT_BIT | PAINT_BIT);
    /// Recompute compositing bits and repaint the render object.
    pub const COMPOSITING_BITS: Self = Self(COMPOSITING_BITS_BIT | PAINT_BIT);
    /// Rebuild semantics for the render object.
    pub const SEMANTICS: Self = Self(SEMANTICS_BIT);

    /// Returns whether no pipeline work is required.
    #[inline]
    pub const fn is_none(self) -> bool {
        self.0 == 0
    }

    /// Returns whether layout is required.
    #[inline]
    pub const fn needs_layout(self) -> bool {
        self.0 & LAYOUT_BIT != 0
    }

    /// Returns whether a paint is required.
    #[inline]
    pub const fn needs_paint(self) -> bool {
        self.0 & PAINT_BIT != 0
    }

    /// Returns whether compositing bits must be recomputed.
    #[inline]
    pub const fn needs_compositing_bits_update(self) -> bool {
        self.0 & COMPOSITING_BITS_BIT != 0
    }

    /// Returns whether semantics must be rebuilt.
    #[inline]
    pub const fn needs_semantics_update(self) -> bool {
        self.0 & SEMANTICS_BIT != 0
    }

    /// Combines the pipeline work required by two independent changes.
    #[inline]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl BitOr for RenderUpdateImpact {
    type Output = Self;

    #[inline]
    fn bitor(self, rhs: Self) -> Self::Output {
        self.union(rhs)
    }
}

impl BitOrAssign for RenderUpdateImpact {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
}

#[cfg(test)]
mod tests {
    use super::RenderUpdateImpact;

    #[test]
    fn named_impacts_have_the_documented_truth_table() {
        assert_eq!(RenderUpdateImpact::LAYOUT.0, (1 << 0) | (1 << 1));
        assert_eq!(RenderUpdateImpact::PAINT.0, 1 << 1);
        assert_eq!(RenderUpdateImpact::COMPOSITING_BITS.0, (1 << 2) | (1 << 1));
        assert_eq!(RenderUpdateImpact::SEMANTICS.0, 1 << 3);
        let cases = [
            (RenderUpdateImpact::NONE, [false, false, false, false]),
            (RenderUpdateImpact::PAINT, [false, true, false, false]),
            (RenderUpdateImpact::LAYOUT, [true, true, false, false]),
            (
                RenderUpdateImpact::COMPOSITING_BITS,
                [false, true, true, false],
            ),
            (RenderUpdateImpact::SEMANTICS, [false, false, false, true]),
        ];

        for (impact, expected) in cases {
            assert_eq!(
                [
                    impact.needs_layout(),
                    impact.needs_paint(),
                    impact.needs_compositing_bits_update(),
                    impact.needs_semantics_update(),
                ],
                expected,
            );
            assert_eq!(impact.is_none(), impact == RenderUpdateImpact::NONE);
        }
    }

    #[test]
    fn union_is_an_idempotent_commutative_monoid() {
        let values = [
            RenderUpdateImpact::NONE,
            RenderUpdateImpact::PAINT,
            RenderUpdateImpact::LAYOUT,
            RenderUpdateImpact::COMPOSITING_BITS,
            RenderUpdateImpact::SEMANTICS,
            RenderUpdateImpact::PAINT | RenderUpdateImpact::SEMANTICS,
            RenderUpdateImpact::LAYOUT | RenderUpdateImpact::SEMANTICS,
            RenderUpdateImpact::COMPOSITING_BITS | RenderUpdateImpact::SEMANTICS,
            RenderUpdateImpact::LAYOUT | RenderUpdateImpact::COMPOSITING_BITS,
            RenderUpdateImpact::LAYOUT
                | RenderUpdateImpact::COMPOSITING_BITS
                | RenderUpdateImpact::SEMANTICS,
        ];

        for &left in &values {
            assert_eq!(left | RenderUpdateImpact::NONE, left);
            assert_eq!(left | left, left);
            for &right in &values {
                assert_eq!(left | right, right | left);
                for &third in &values {
                    assert_eq!((left | right) | third, left | (right | third));
                }
            }
        }
    }

    #[test]
    fn bit_or_assign_matches_union() {
        let mut impact = RenderUpdateImpact::PAINT;
        impact |= RenderUpdateImpact::SEMANTICS;
        assert_eq!(
            impact,
            RenderUpdateImpact::PAINT.union(RenderUpdateImpact::SEMANTICS),
        );
    }
}
