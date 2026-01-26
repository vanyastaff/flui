//! OpacityLayer - Alpha blending layer
//!
//! This layer applies an opacity (alpha) value to its children.
//! Corresponds to Flutter's `OpacityLayer`.

use flui_types::geometry::Pixels;
use flui_types::Offset;

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
}

impl OpacityLayer {
    /// Creates a new opacity layer.
    ///
    /// # Arguments
    ///
    /// * `alpha` - Opacity value (0.0 to 1.0, will be clamped)
    #[inline]
    pub fn new(alpha: f32) -> Self {
        Self {
            alpha: alpha.clamp(0.0, 1.0),
            offset: Offset::ZERO,
        }
    }

    /// Creates an opacity layer with an offset.
    ///
    /// Combining offset with opacity avoids needing a separate OffsetLayer.
    #[inline]
    pub fn with_offset(alpha: f32, offset: Offset<Pixels>) -> Self {
        Self {
            alpha: alpha.clamp(0.0, 1.0),
            offset,
        }
    }

    /// Creates a fully transparent layer.
    #[inline]
    pub const fn transparent() -> Self {
        Self {
            alpha: 0.0,
            offset: Offset::ZERO,
        }
    }

    /// Creates a fully opaque layer.
    #[inline]
    pub const fn opaque() -> Self {
        Self {
            alpha: 1.0,
            offset: Offset::ZERO,
        }
    }

    /// Returns the alpha value.
    #[inline]
    pub const fn alpha(&self) -> f32 {
        self.alpha
    }

    /// Sets the alpha value.
    ///
    /// Value will be clamped to 0.0..=1.0.
    #[inline]
    pub fn set_alpha(&mut self, alpha: f32) {
        self.alpha = alpha.clamp(0.0, 1.0);
    }

    /// Returns the offset.
    #[inline]
    pub const fn offset(&self) -> Offset<Pixels> {
        self.offset
    }

    /// Sets the offset.
    #[inline]
    pub fn set_offset(&mut self, offset: Offset<Pixels>) {
        self.offset = offset;
    }

    /// Returns true if fully transparent (can skip rendering).
    #[inline]
    pub fn is_invisible(&self) -> bool {
        self.alpha <= 0.0
    }

    /// Returns true if fully opaque (can skip alpha blending).
    #[inline]
    pub fn is_opaque(&self) -> bool {
        self.alpha >= 1.0
    }

    /// Returns true if this layer has a non-zero offset.
    #[inline]
    pub fn has_offset(&self) -> bool {
        use flui_types::geometry::px;
        self.offset.dx != px(0.0) || self.offset.dy != px(0.0)
    }

    /// Returns the alpha as a byte value (0-255).
    ///
    /// Useful for GPU operations that expect integer alpha.
    #[inline]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn alpha_byte(&self) -> u8 {
        (self.alpha * 255.0).round() as u8
    }

    /// Returns true if this layer needs compositing.
    ///
    /// Returns false if fully opaque (no compositing needed) or
    /// fully transparent (nothing to render).
    #[inline]
    pub fn needs_compositing(&self) -> bool {
        self.alpha > 0.0 && self.alpha < 1.0
    }
}

impl Default for OpacityLayer {
    fn default() -> Self {
        Self::opaque()
    }
}

// Thread safety (Copy type)
unsafe impl Send for OpacityLayer {}
unsafe impl Sync for OpacityLayer {}

#[cfg(test)]
mod tests {
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
        let layer = OpacityLayer::with_offset(0.75, Offset::new(10.0, 20.0));

        assert_eq!(layer.alpha(), 0.75);
        assert_eq!(layer.offset().dx, 10.0);
        assert_eq!(layer.offset().dy, 20.0);
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
    fn test_opacity_layer_setters() {
        let mut layer = OpacityLayer::new(0.5);

        layer.set_alpha(0.75);
        assert_eq!(layer.alpha(), 0.75);

        layer.set_offset(Offset::new(5.0, 10.0));
        assert_eq!(layer.offset().dx, 5.0);
    }

    #[test]
    fn test_opacity_layer_set_alpha_clamping() {
        let mut layer = OpacityLayer::new(0.5);

        layer.set_alpha(-1.0);
        assert_eq!(layer.alpha(), 0.0);

        layer.set_alpha(2.0);
        assert_eq!(layer.alpha(), 1.0);
    }

    #[test]
    fn test_opacity_layer_alpha_byte() {
        assert_eq!(OpacityLayer::new(0.0).alpha_byte(), 0);
        assert_eq!(OpacityLayer::new(0.5).alpha_byte(), 128);
        assert_eq!(OpacityLayer::new(1.0).alpha_byte(), 255);
    }

    #[test]
    fn test_opacity_layer_needs_compositing() {
        assert!(!OpacityLayer::new(0.0).needs_compositing()); // Fully transparent
        assert!(!OpacityLayer::new(1.0).needs_compositing()); // Fully opaque
        assert!(OpacityLayer::new(0.5).needs_compositing()); // Semi-transparent
        assert!(OpacityLayer::new(0.01).needs_compositing());
        assert!(OpacityLayer::new(0.99).needs_compositing());
    }

    #[test]
    fn test_opacity_layer_default() {
        let layer = OpacityLayer::default();

        assert!(layer.is_opaque());
    }

    #[test]
    fn test_opacity_layer_copy() {
        let layer = OpacityLayer::new(0.5);
        let copied = layer; // Copy

        assert_eq!(layer, copied);
    }

    #[test]
    fn test_opacity_layer_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<OpacityLayer>();
        assert_sync::<OpacityLayer>();
    }
}
