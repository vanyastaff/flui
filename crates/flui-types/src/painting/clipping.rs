//! Clipping types for painting.

use crate::geometry::{Offset, Pixels, Rect, Size, px};

/// How a new clip region combines with the current clip.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ClipOp {
    /// The new region is intersected with the current clip.
    /// Only pixels inside both regions remain visible.
    #[default]
    Intersect,
    /// The new region is subtracted from the current clip.
    /// Pixels inside the new region become invisible (creates a "hole").
    Difference,
}

/// The quality (and cost) with which content is clipped.
///
/// Ordered from cheapest to most expensive; mirrors Flutter's `Clip` enum.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Clip {
    /// No clipping whatsoever.
    ///
    /// This is the most efficient option. If you know that your content
    /// will not exceed the bounds of the box, use this.
    None,

    /// Clip, but do not apply anti-aliasing.
    ///
    /// Faster than `AntiAlias`, but jagged on non-axis-aligned edges. This is
    /// the default and is appropriate for rectangular clips.
    #[default]
    HardEdge,

    /// Clip with anti-aliasing.
    ///
    /// This mode is more expensive than `HardEdge` but produces smoother
    /// edges. Use this for non-rectangular clips or when you need smooth edges.
    AntiAlias,

    /// Clip with anti-aliasing and save a layer.
    ///
    /// This is the most expensive option and is only necessary when you have
    /// content that needs to be clipped with anti-aliasing AND has
    /// transparency. In most cases, `AntiAlias` is sufficient.
    AntiAliasWithSaveLayer,
}

impl Clip {
    /// Returns `true` if this mode applies anti-aliasing to clip edges.
    #[must_use]
    #[inline]
    pub const fn is_anti_aliased(&self) -> bool {
        matches!(self, Clip::AntiAlias | Clip::AntiAliasWithSaveLayer)
    }

    /// Returns `true` if this mode saves a layer in addition to clipping.
    #[must_use]
    #[inline]
    pub const fn saves_layer(&self) -> bool {
        matches!(self, Clip::AntiAliasWithSaveLayer)
    }

    /// Returns `true` if this mode performs any clipping at all.
    #[must_use]
    #[inline]
    pub const fn clips(&self) -> bool {
        !matches!(self, Clip::None)
    }

    /// Returns `true` for the cheap modes (`None` and `HardEdge`) that need
    /// neither anti-aliasing nor a saved layer.
    #[must_use]
    #[inline]
    pub const fn is_efficient(&self) -> bool {
        matches!(self, Clip::None | Clip::HardEdge)
    }
}

/// Widget-level clipping behavior, convertible to a low-level [`Clip`] mode.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ClipBehavior {
    /// No clipping.
    None,

    /// Clip without anti-aliasing; the default.
    #[default]
    HardEdge,

    /// Clip with anti-aliasing applied.
    ///
    /// This is appropriate for shapes with smooth curves or diagonal edges.
    AntiAlias,

    /// Clip with anti-aliasing and save a layer immediately following the clip.
    ///
    /// This is rarely needed, but can be used when a clip is applied to a
    /// widget with transparent children.
    AntiAliasWithSaveLayer,
}

impl ClipBehavior {
    /// Converts this behavior to the equivalent low-level [`Clip`] mode.
    #[must_use]
    #[inline]
    pub const fn to_clip(self) -> Clip {
        match self {
            ClipBehavior::None => Clip::None,
            ClipBehavior::HardEdge => Clip::HardEdge,
            ClipBehavior::AntiAlias => Clip::AntiAlias,
            ClipBehavior::AntiAliasWithSaveLayer => Clip::AntiAliasWithSaveLayer,
        }
    }

    /// Returns `true` if this behavior performs any clipping at all.
    #[must_use]
    #[inline]
    pub const fn clips(self) -> bool {
        !matches!(self, ClipBehavior::None)
    }

    /// Returns `true` if this behavior applies anti-aliasing to clip edges.
    #[must_use]
    #[inline]
    pub const fn is_anti_aliased(self) -> bool {
        matches!(
            self,
            ClipBehavior::AntiAlias | ClipBehavior::AntiAliasWithSaveLayer
        )
    }
}

impl From<ClipBehavior> for Clip {
    #[inline]
    fn from(behavior: ClipBehavior) -> Self {
        behavior.to_clip()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `(anti-aliased, saves layer, clips, efficient)` per variant, and the
    /// `ClipBehavior` -> `Clip` mapping.
    #[test]
    fn clip_predicates_and_conversion() {
        for (behavior, clip, expected) in [
            (ClipBehavior::None, Clip::None, (false, false, false, true)),
            (
                ClipBehavior::HardEdge,
                Clip::HardEdge,
                (false, false, true, true),
            ),
            (
                ClipBehavior::AntiAlias,
                Clip::AntiAlias,
                (true, false, true, false),
            ),
            (
                ClipBehavior::AntiAliasWithSaveLayer,
                Clip::AntiAliasWithSaveLayer,
                (true, true, true, false),
            ),
        ] {
            let got = (
                clip.is_anti_aliased(),
                clip.saves_layer(),
                clip.clips(),
                clip.is_efficient(),
            );
            assert_eq!(got, expected, "{clip:?}");
            assert_eq!(behavior.to_clip(), clip);
            assert_eq!(Clip::from(behavior), clip);
            assert_eq!(
                (behavior.is_anti_aliased(), behavior.clips()),
                (expected.0, expected.2)
            );
        }
    }
}
