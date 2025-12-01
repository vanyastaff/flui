//! RenderAbstractViewport - Abstract interface for viewport render objects

use flui_core::element::{ElementId, ElementTree};
use flui_types::layout::Axis;
use flui_types::prelude::*;

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
///
/// fn scroll_to_item(viewport: &dyn RenderAbstractViewport, target_id: ElementId) {
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
    /// * `tree` - Element tree for traversal
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
        tree: &ElementTree,
        target: ElementId,
        alignment: f32,
        rect: Option<Rect>,
        axis: Option<Axis>,
    ) -> RevealedOffset;

    /// Get the viewport's main axis direction
    ///
    /// Returns the axis along which the viewport scrolls.
    fn axis(&self) -> Axis;

    /// Find the nearest ancestor viewport in the tree
    ///
    /// Walks up the tree from `start` to find the first RenderAbstractViewport.
    /// Returns None if no viewport ancestor is found.
    ///
    /// # Arguments
    ///
    /// * `tree` - Element tree for traversal
    /// * `start` - Element to start searching from
    fn find_ancestor_viewport(tree: &ElementTree, start: ElementId) -> Option<ElementId>
    where
        Self: Sized,
    {
        let mut current = start;

        loop {
            // Get parent
            let Some(parent_id) = tree.parent(current) else {
                return None;
            };

            // Check if parent is a viewport
            // In a real implementation, we'd check if the render object
            // implements RenderAbstractViewport
            // For now, we return None as we can't do trait downcasting easily

            current = parent_id;

            // Simplified: return None after checking root
            if tree.parent(current).is_none() {
                return None;
            }
        }
    }
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
        let revealed = RevealedOffset::new(
            100.0,
            Rect::from_xywh(0.0, 100.0, 50.0, 50.0),
        );

        assert_eq!(revealed.offset, 100.0);
        assert_eq!(revealed.rect.x, 0.0);
        assert_eq!(revealed.rect.y, 100.0);
        assert_eq!(revealed.rect.width, 50.0);
        assert_eq!(revealed.rect.height, 50.0);
    }

    #[test]
    fn test_revealed_offset_equality() {
        let revealed1 = RevealedOffset::new(
            100.0,
            Rect::from_xywh(0.0, 100.0, 50.0, 50.0),
        );
        let revealed2 = RevealedOffset::new(
            100.0,
            Rect::from_xywh(0.0, 100.0, 50.0, 50.0),
        );

        assert_eq!(revealed1, revealed2);
    }

    #[test]
    fn test_default_cache_extent() {
        assert_eq!(DEFAULT_CACHE_EXTENT, 250.0);
    }
}
