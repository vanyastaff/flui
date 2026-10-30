//! [`CarouselGeometry`]: a layout's keylines over one viewport, read by the sliver that draws
//! the items and by whatever moves the carousel between them.

use flui_foundation::geometry::Rect;

use super::keylines::{Keylines, Strategy};
use super::layout::CarouselLayout;

/// A [`CarouselLayout`] measured against one viewport, item spacing and item count: where every
/// item rests and how it is drawn at any scroll offset.
///
/// The sliver lays its items out from it, and a snap policy or a controller reads the same
/// value to move between resting items, so the offsets they agree on are one computation.
/// Offsets are along the scroll axis from the start of the strip, in logical pixels.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct CarouselGeometry {
    strategy: Strategy,
    count: usize,
}

/// Where an item is drawn at one scroll offset.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct CarouselItemPlacement {
    /// The extent of the item's mask along the scroll axis, centred on the item.
    pub mask_extent: f64,
    /// Where the item's centre is drawn, from the viewport's leading edge.
    pub center: f64,
    /// How far the item has travelled from the keyline before it to the one after, in
    /// `[0, 1]`: the share a parallax effect reads.
    pub progress: f64,
}

impl CarouselGeometry {
    /// `layout` over a `viewport` long scroll axis with `spacing` between `count` items;
    /// `None` when the viewport is not finite or not greater than zero, the spacing is not
    /// finite or negative, or the layout has nothing to show in that viewport.
    #[must_use]
    pub fn try_new(
        layout: &CarouselLayout,
        viewport: f64,
        spacing: f64,
        count: usize,
    ) -> Option<Self> {
        Some(Self {
            strategy: Strategy::try_new(layout, viewport, spacing, count)?,
            count,
        })
    }

    /// The number of items.
    #[must_use]
    pub fn count(&self) -> usize {
        self.count
    }

    /// The extent every item is laid out at along the scroll axis.
    #[must_use]
    pub fn item_extent(&self) -> f64 {
        self.strategy.item_extent()
    }

    /// The distance from one item's start to the next one's in the strip.
    #[must_use]
    pub fn stride(&self) -> f64 {
        self.strategy.stride()
    }

    /// The largest scroll offset.
    #[must_use]
    pub fn max_offset(&self) -> f64 {
        self.strategy.max_offset(self.count)
    }

    /// The offset at which item `index` rests on a large keyline, clamped to the scroll range;
    /// an index past the last item is the last item.
    #[must_use]
    pub fn rest_offset(&self, index: usize) -> f64 {
        if self.count == 0 {
            return 0.0;
        }
        self.strategy
            .rest_offset(index.min(self.count - 1), self.count)
    }

    /// The item whose resting offset is nearest `offset`, the lower index on a tie; `None`
    /// without items.
    #[must_use]
    pub fn nearest_item(&self, offset: f64) -> Option<usize> {
        self.strategy.nearest_item(offset, self.count)
    }

    /// The keylines at `offset`, for placing several items at it.
    pub(crate) fn keylines_at(&self, offset: f64) -> Keylines {
        self.strategy.keylines_at(offset, self.max_offset())
    }

    /// Where item `index` is drawn at `offset` under `keylines` (from
    /// [`keylines_at`](Self::keylines_at) of the same offset).
    pub(crate) fn place_with(
        &self,
        keylines: &Keylines,
        index: usize,
        offset: f64,
    ) -> CarouselItemPlacement {
        let placement = self.strategy.place(keylines, index, offset);
        CarouselItemPlacement {
            mask_extent: placement.mask_extent,
            center: placement.center,
            progress: placement.progress,
        }
    }

    /// Where item `index` is drawn at `offset`.
    #[must_use]
    pub fn place(&self, index: usize, offset: f64) -> CarouselItemPlacement {
        self.place_with(&self.keylines_at(offset), index, offset)
    }

    /// The items that can be seen at `offset`: those whose centre lies between the first and
    /// last keylines, one more on each side so an item entering is built before it shows.
    pub(crate) fn window(&self, keylines: &Keylines, offset: f64) -> Option<(usize, usize)> {
        if self.count == 0 || !offset.is_finite() {
            return None;
        }
        let (first, last) = keylines.strip_span();
        let stride = self.stride();
        let half = self.item_extent() / 2.0;
        let low = ((offset + first - half) / stride).floor() - 1.0;
        let high = ((offset + last - half) / stride).ceil() + 1.0;
        let to_index = |value: f64| {
            if value <= 0.0 {
                0
            } else if value >= (self.count - 1) as f64 {
                self.count - 1
            } else {
                value as usize
            }
        };
        let (low, high) = (to_index(low), to_index(high));
        (low <= high).then_some((low, high))
    }
}

/// The mask of an item laid out `extent` long, `mask_extent` of it shown about its centre, in
/// the item's own coordinates along a horizontal or vertical scroll axis.
#[must_use]
pub(crate) fn mask_rect(extent: f64, mask_extent: f64, cross: f64, horizontal: bool) -> Rect {
    let start = (extent - mask_extent) / 2.0;
    if horizontal {
        Rect::from_ltrb(start, 0.0, start + mask_extent, cross)
    } else {
        Rect::from_ltrb(0.0, start, cross, start + mask_extent)
    }
}
