//! RenderAbstractViewport - Abstract interface for viewport render objects
//!
//! Flutter reference: <https://api.flutter.dev/flutter/rendering/RenderAbstractViewport-class.html>

use crate::core::ElementId;
use flui_types::layout::Axis;
use flui_types::Rect;

/// Offset and metadata needed to reveal a target in a viewport
///
/// Returned by `getOffsetToReveal` to describe where the viewport
/// needs to scroll to make a target visible.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RevealedOffset {
    /// The scroll offset needed to reveal the target
    pub offset: f32,

    /// The rect of the target in viewport coordinates at that offset
    pub rect: Rect,
}

impl RevealedOffset {
    /// Create a new RevealedOffset
    pub fn new(offset: f32, rect: Rect) -> Self {
        Self { offset, rect }
    }

    /// Create with zero offset and empty rect
    pub fn zero() -> Self {
        Self {
            offset: 0.0,
            rect: Rect::ZERO,
        }
    }
}

impl Default for RevealedOffset {
    fn default() -> Self {
        Self::zero()
    }
}

/// Abstract interface for viewport render objects
///
/// A viewport is a render object that is "bigger on the inside" - it displays
/// a portion of its content controlled by a scroll offset. This trait provides
/// a common interface for all viewport types without requiring knowledge of
/// specific implementations.
///
/// # Implementers
///
/// - `RenderViewport` - Standard sliver-based viewport
/// - `RenderShrinkWrappingViewport` - Viewport that sizes to its content
/// - `RenderListWheelViewport` - 3D cylindrical viewport
/// - `RenderTwoDimensionalViewport` - 2D scrolling viewport
///
/// # Use Cases
///
/// - Scroll-to-item functionality
/// - Revealing hidden content
/// - Coordinating nested scroll views
/// - Accessibility (screen reader navigation)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderAbstractViewport, RevealedOffset};
/// use flui_rendering::core::ElementId;
///
/// fn scroll_to_item<V: RenderAbstractViewport>(viewport: &V, target_id: ElementId) {
///     let revealed = viewport.get_offset_to_reveal(
///         target_id,
///         0.5,  // Center alignment
///         None,
///         None,
///     );
///     // Update viewport's scroll offset to revealed.offset
/// }
/// ```
pub trait RenderAbstractViewport {
    /// Get the offset needed to reveal a target element
    ///
    /// Returns the scroll offset and target rect needed to make the target
    /// element visible in the viewport.
    ///
    /// # Arguments
    ///
    /// * `target` - Element to reveal
    /// * `alignment` - Where to position the target:
    ///   - `0.0` = leading edge (top/left)
    ///   - `0.5` = center
    ///   - `1.0` = trailing edge (bottom/right)
    /// * `rect` - Optional rect within target to reveal (defaults to full target)
    /// * `axis` - Optional axis to reveal along (defaults to viewport's axis)
    ///
    /// # Returns
    ///
    /// `RevealedOffset` containing the scroll offset and target rect.
    fn get_offset_to_reveal(
        &self,
        target: ElementId,
        alignment: f32,
        rect: Option<Rect>,
        axis: Option<Axis>,
    ) -> RevealedOffset;

    /// Get the viewport's main axis direction
    ///
    /// Returns the axis along which the viewport scrolls.
    fn axis(&self) -> Axis;
}

/// Default cache extent for viewports (in pixels)
///
/// This is the amount of content to render outside the visible viewport
/// to improve scrolling performance.
pub const DEFAULT_CACHE_EXTENT: f32 = 250.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_revealed_offset_creation() {
        let revealed = RevealedOffset::new(100.0, Rect::from_xywh(0.0, 100.0, 50.0, 50.0));

        assert_eq!(revealed.offset, 100.0);
        assert_eq!(revealed.rect.left(), 0.0);
        assert_eq!(revealed.rect.top(), 100.0);
        assert_eq!(revealed.rect.width(), 50.0);
        assert_eq!(revealed.rect.height(), 50.0);
    }

    #[test]
    fn test_revealed_offset_equality() {
        let revealed1 = RevealedOffset::new(100.0, Rect::from_xywh(0.0, 100.0, 50.0, 50.0));
        let revealed2 = RevealedOffset::new(100.0, Rect::from_xywh(0.0, 100.0, 50.0, 50.0));

        assert_eq!(revealed1, revealed2);
    }

    #[test]
    fn test_revealed_offset_zero() {
        let revealed = RevealedOffset::zero();
        assert_eq!(revealed.offset, 0.0);
        assert_eq!(revealed.rect, Rect::ZERO);
    }

    #[test]
    fn test_revealed_offset_default() {
        let revealed = RevealedOffset::default();
        assert_eq!(revealed.offset, 0.0);
    }

    #[test]
    fn test_default_cache_extent() {
        assert_eq!(DEFAULT_CACHE_EXTENT, 250.0);
    }
}
