//! `OpacityLayer` — composites its subtree at an alpha, optionally with a blend mode.

use flui_types::{Offset, geometry::Pixels, painting::BlendMode};

/// Layer that applies opacity (alpha blending) to its children.
///
/// The opacity layer renders children to an offscreen buffer and then
/// composites the result with the specified alpha value.
///
/// # Performance
///
/// Opacity layers require offscreen rendering, which has a performance cost.
/// For static opacity, consider using `Color.withOpacity()` directly on
/// paint operations when possible.
///
/// # Optimization
///
/// - If `alpha == 0.0`, children can be skipped entirely
/// - If `alpha == 1.0`, the layer is a no-op and can be skipped
///
/// # Architecture
///
/// ```text
/// OpacityLayer
///   │
///   │ Render children to offscreen buffer
///   │ Composite with alpha value
///   ▼
/// Children rendered with transparency
/// ```
///
/// # Example
///
/// ```rust
/// use flui_layer::OpacityLayer;
///
/// // Create 50% transparent layer
/// let layer = OpacityLayer::new(0.5);
///
/// assert_eq!(layer.alpha(), 0.5);
/// assert!(!layer.is_invisible());
/// assert!(!layer.is_opaque());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpacityLayer {
    /// Opacity value (0.0 = fully transparent, 1.0 = fully opaque)
    alpha: f32,

    /// Optional offset (for optimization, avoids extra OffsetLayer)
    offset: Offset<Pixels>,

    /// Blend mode applied when compositing this layer onto its parent.
    ///
    /// `SrcOver` for plain opacity layers; an advanced mode (e.g. Multiply)
    /// when the layer was created by a `saveLayer` with an explicit blend mode.
    blend: BlendMode,
}

impl OpacityLayer {
    /// Composites the subtree at `alpha` (clamped to `0.0..=1.0`) with `BlendMode::SrcOver`.
    #[inline]
    pub fn new(alpha: f32) -> Self {
        Self {
            alpha: super::unit_alpha(alpha),
            offset: Offset::ZERO,
            blend: BlendMode::SrcOver,
        }
    }

    /// Like [`Self::new`], also translating the subtree by `offset`.
    #[inline]
    pub fn with_offset(alpha: f32, offset: Offset<Pixels>) -> Self {
        Self {
            alpha: super::unit_alpha(alpha),
            offset,
            blend: BlendMode::SrcOver,
        }
    }

    /// An opacity group with an explicit blend mode, for `saveLayer` paths that carry an
    /// advanced mode (Multiply, Screen, …); plain opacity is always `SrcOver`.
    #[inline]
    pub fn with_blend(alpha: f32, offset: Offset<Pixels>, blend: BlendMode) -> Self {
        Self {
            alpha: super::unit_alpha(alpha),
            offset,
            blend,
        }
    }

    /// Alpha 0: the subtree composites to nothing.
    #[inline]
    pub const fn transparent() -> Self {
        Self {
            alpha: 0.0,
            offset: Offset::ZERO,
            blend: BlendMode::SrcOver,
        }
    }

    /// Alpha 1: the group is the identity for `SrcOver`.
    #[inline]
    pub const fn opaque() -> Self {
        Self {
            alpha: 1.0,
            offset: Offset::ZERO,
            blend: BlendMode::SrcOver,
        }
    }

    /// The group's alpha in `0.0..=1.0`.
    #[inline]
    pub const fn alpha(&self) -> f32 {
        self.alpha
    }

    /// The translation applied to the subtree (see [`Self::with_offset`]).
    #[inline]
    pub const fn offset(&self) -> Offset<Pixels> {
        self.offset
    }

    /// Whether alpha is 0.
    #[inline]
    pub fn is_invisible(&self) -> bool {
        self.alpha <= 0.0
    }

    /// Whether alpha is 1 (an identity group under `SrcOver`; not under an advanced blend).
    #[inline]
    pub fn is_opaque(&self) -> bool {
        self.alpha >= 1.0
    }

    /// Whether the layer translates its subtree.
    #[inline]
    pub fn has_offset(&self) -> bool {
        !self.offset.is_zero()
    }

    /// The blend mode: `SrcOver` for plain opacity, the caller's mode for advanced-blend groups.
    #[inline]
    pub const fn blend(&self) -> BlendMode {
        self.blend
    }
}

impl Default for OpacityLayer {
    fn default() -> Self {
        Self::opaque()
    }
}

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    #[test]
    fn test_opacity_layer_new() {
        let layer = OpacityLayer::new(0.5);

        assert_eq!(layer.alpha(), 0.5);
        assert_eq!(layer.offset(), Offset::ZERO);
    }

    #[test]
    fn test_opacity_layer_clamping() {
        let layer1 = OpacityLayer::new(-0.5);
        assert_eq!(layer1.alpha(), 0.0);

        let layer2 = OpacityLayer::new(1.5);
        assert_eq!(layer2.alpha(), 1.0);
    }

    #[test]
    fn test_opacity_layer_with_offset() {
        let layer = OpacityLayer::with_offset(0.75, Offset::new(px(10.0), px(20.0)));

        assert_eq!(layer.alpha(), 0.75);
        assert_eq!(layer.offset().dx, px(10.0));
        assert_eq!(layer.offset().dy, px(20.0));
        assert!(layer.has_offset());
    }

    #[test]
    fn test_opacity_layer_transparent() {
        let layer = OpacityLayer::transparent();

        assert_eq!(layer.alpha(), 0.0);
        assert!(layer.is_invisible());
        assert!(!layer.is_opaque());
    }

    #[test]
    fn test_opacity_layer_opaque() {
        let layer = OpacityLayer::opaque();

        assert_eq!(layer.alpha(), 1.0);
        assert!(!layer.is_invisible());
        assert!(layer.is_opaque());
    }

    #[test]
    fn test_opacity_layer_default() {
        let layer = OpacityLayer::default();

        assert!(layer.is_opaque());
    }
}
