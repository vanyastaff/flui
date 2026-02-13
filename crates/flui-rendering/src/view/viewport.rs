//! Viewport abstractions for render objects that are bigger on the inside.
//!
//! This module provides the interface for viewports - render objects that display
//! a portion of their content, which can be controlled by a scroll offset.
//!
//! # Flutter Equivalence
//!
//! This corresponds to parts of Flutter's `rendering/viewport.dart`.

use crate::protocol::BoxProtocol;
use crate::traits::RenderObject;
use flui_types::{Axis, Rect};

/// The unit of measurement for a viewport's cache extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CacheExtentStyle {
    /// Treat the cache extent as logical pixels.
    #[default]
    Pixel,
    /// Treat the cache extent as a multiplier of the main axis extent.
    Viewport,
}

/// Specifies an order in which to paint the slivers of a viewport.
///
/// The slivers are painted in the specified order and hit-tested in the
/// opposite order.
///
/// This can also be thought of as an ordering in the z-direction:
/// whichever sliver is painted last (and hit-tested first) is on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SliverPaintOrder {
    /// The first sliver paints on top, and the last sliver on bottom.
    ///
    /// Slivers are painted in reverse order and hit-tested in forward order.
    /// This is the default.
    #[default]
    FirstIsTop,
    /// The last sliver paints on top, and the first sliver on bottom.
    ///
    /// Slivers are painted in forward order and hit-tested in reverse order.
    LastIsTop,
}

/// Return value for [`RenderAbstractViewport::get_offset_to_reveal`].
///
/// It indicates the offset required to reveal an element in a viewport and
/// the rect position said element would have in the viewport at that offset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RevealedOffset {
    /// Offset for the viewport to reveal a specific element.
    ///
    /// This is the scroll offset that should be applied to bring the target
    /// element into view.
    pub offset: f32,

    /// The rect in the outer coordinate system of the viewport where the
    /// to-be-revealed element would be located if the viewport's offset is
    /// set to [`Self::offset`].
    ///
    /// The outer coordinate system has its origin at the top-left corner of
    /// the visible part of the viewport. This origin stays at the same position
    /// regardless of the current viewport offset.
    pub rect: Rect,
}

impl RevealedOffset {
    /// Creates a new `RevealedOffset`.
    pub fn new(offset: f32, rect: Rect) -> Self {
        Self { offset, rect }
    }

    /// Determines which provided leading or trailing edge of the viewport
    /// will be used to reveal an element, accounting for the size and already
    /// visible portion of the render object being revealed.
    ///
    /// If the target render object is already fully visible, returns `None`.
    ///
    /// # Arguments
    ///
    /// * `leading_edge_offset` - The offset that would align the leading edge
    /// * `trailing_edge_offset` - The offset that would align the trailing edge
    /// * `current_offset` - The current scroll offset
    pub fn clamp_offset(
        leading_edge_offset: RevealedOffset,
        trailing_edge_offset: RevealedOffset,
        current_offset: f32,
    ) -> Option<RevealedOffset> {
        let inverted = leading_edge_offset.offset < trailing_edge_offset.offset;

        let (smaller, larger) = if inverted {
            (leading_edge_offset, trailing_edge_offset)
        } else {
            (trailing_edge_offset, leading_edge_offset)
        };

        if current_offset > larger.offset {
            Some(larger)
        } else if current_offset < smaller.offset {
            Some(smaller)
        } else {
            None
        }
    }
}

/// An interface for render objects that are bigger on the inside.
///
/// Some render objects, such as `RenderViewport`, present a portion of their
/// content, which can be controlled by a [`ViewportOffset`]. This interface
/// lets the framework recognize such render objects and interact with them
/// without having specific knowledge of all the various types of viewports.
///
/// [`ViewportOffset`]: super::ViewportOffset
pub trait RenderAbstractViewport: RenderObject<BoxProtocol> {
    /// Returns the offset that would be needed to reveal the target render object.
    ///
    /// # Arguments
    ///
    /// * `target` - The render object to reveal
    /// * `alignment` - Where the target should be positioned:
    ///   - 0.0: as close to the leading edge as possible
    ///   - 1.0: as close to the trailing edge as possible
    ///   - 0.5: as close to the center as possible
    /// * `rect` - Optional area of the target to reveal. If `None`, reveals
    ///   the entire target's paint bounds.
    /// * `axis` - Optional axis for 2D viewports. Ignored by 1D viewports.
    fn get_offset_to_reveal(
        &self,
        target: &dyn RenderObject<BoxProtocol>,
        alignment: f32,
        rect: Option<Rect>,
        axis: Option<Axis>,
    ) -> RevealedOffset;

    /// The default cache extent for viewports (in pixels).
    ///
    /// This assumes [`CacheExtentStyle::Pixel`].
    const DEFAULT_CACHE_EXTENT: f32 = 250.0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::px;
    use flui_types::Rect;

    #[test]
    fn test_cache_extent_style_default() {
        let style: CacheExtentStyle = Default::default();
        assert_eq!(style, CacheExtentStyle::Pixel);
    }

    #[test]
    fn test_sliver_paint_order_default() {
        let order: SliverPaintOrder = Default::default();
        assert_eq!(order, SliverPaintOrder::FirstIsTop);
    }

    #[test]
    fn test_revealed_offset_new() {
        let rect = Rect::from_ltwh(px(10.0), px(20.0), px(100.0), px(50.0));
        let offset = RevealedOffset::new(100.0, rect);

        assert_eq!(offset.offset, 100.0);
        assert_eq!(offset.rect, rect);
    }

    #[test]
    fn test_revealed_offset_clamp_already_visible() {
        let leading = RevealedOffset::new(50.0, Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0)));
        let trailing = RevealedOffset::new(150.0, Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0)));

        // Current offset is between leading and trailing - already visible
        let result = RevealedOffset::clamp_offset(leading, trailing, 100.0);
        assert!(result.is_none());
    }

    #[test]
    fn test_revealed_offset_clamp_needs_scroll_down() {
        let leading = RevealedOffset::new(50.0, Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0)));
        let trailing = RevealedOffset::new(150.0, Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0)));

        // Current offset is above the visible range - need to scroll down
        let result = RevealedOffset::clamp_offset(leading, trailing, 200.0);
        assert!(result.is_some());
        assert_eq!(result.unwrap().offset, 150.0);
    }

    #[test]
    fn test_revealed_offset_clamp_needs_scroll_up() {
        let leading = RevealedOffset::new(50.0, Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0)));
        let trailing = RevealedOffset::new(150.0, Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0)));

        // Current offset is below the visible range - need to scroll up
        let result = RevealedOffset::clamp_offset(leading, trailing, 30.0);
        assert!(result.is_some());
        assert_eq!(result.unwrap().offset, 50.0);
    }

    #[test]
    fn test_revealed_offset_clamp_inverted() {
        // When leading > trailing (inverted order)
        let leading = RevealedOffset::new(150.0, Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0)));
        let trailing = RevealedOffset::new(50.0, Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0)));

        // Current offset is between - already visible
        let result = RevealedOffset::clamp_offset(leading, trailing, 100.0);
        assert!(result.is_none());
    }
}
