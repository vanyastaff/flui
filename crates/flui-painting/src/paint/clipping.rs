//! Clipping types for painting.

/// How a new clip region combines with the current clip.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[expect(
    clippy::exhaustive_enums,
    reason = "a clip intersects or takes the difference"
)]
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
/// Ordered from cheapest to most expensive.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(flui_foundation::inspect::Inspect)]
#[expect(
    clippy::exhaustive_enums,
    reason = "every backend clips by each behavior, and a `_` arm would draw a new one unclipped"
)]
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
