//! `OffsetLayer` — translates its subtree.

use flui_types::Offset;

/// Layer that applies a simple offset to its children.
///
/// `OffsetLayer` is used primarily for repaint boundaries where
/// the entire subtree can be offset without repainting. This is
/// more efficient than a full `TransformLayer` when only translation
/// is needed.
///
/// # Use Cases
///
/// - Repaint boundary layers
/// - Scrolling content
/// - Animated translations
///
/// # Architecture
///
/// ```text
/// OffsetLayer
///   │
///   │ Apply offset to child coordinates
///   ▼
/// Children rendered at offset position
/// ```
///
/// # Example
///
/// ```rust
/// use flui_layer::OffsetLayer;
/// use flui_types::Offset;
///
/// let layer = OffsetLayer::new(Offset::new(10.0, 20.0));
///
/// assert_eq!(layer.offset().dx, 10.0);
/// assert_eq!(layer.offset().dy, 20.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct OffsetLayer {
    /// The offset to apply to children
    offset: Offset<f64>,
}

impl OffsetLayer {
    /// Translates the subtree by `offset`.
    #[inline]
    pub const fn new(offset: Offset<f64>) -> Self {
        Self { offset }
    }

    /// The identity translation.
    #[inline]
    pub const fn zero() -> Self {
        Self::new(Offset::ZERO)
    }

    /// The translation applied to the subtree.
    #[inline]
    pub const fn offset(&self) -> Offset<f64> {
        self.offset
    }

    /// Whether this layer translates by nothing (the engine pushes no transform for it).
    #[inline]
    pub fn is_zero(&self) -> bool {
        self.offset.is_zero()
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_offset_layer_new() {
        let layer = OffsetLayer::new(Offset::new(10.0, 20.0));

        assert_eq!(layer.offset().dx, 10.0);
        assert_eq!(layer.offset().dy, 20.0);
    }

    #[test]
    fn test_offset_layer_zero() {
        let layer = OffsetLayer::zero();

        assert!(layer.is_zero());
        assert_eq!(layer.offset().dx, 0.0);
        assert_eq!(layer.offset().dy, 0.0);
    }

    #[test]
    fn test_offset_layer_default() {
        let layer = OffsetLayer::default();

        assert!(layer.is_zero());
    }
}
