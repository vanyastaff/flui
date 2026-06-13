//! Protocol-agnostic virtualization (windowing) math.
//!
//! This module owns *windowing math only*: given a [`ScrollWindow`] and a set of
//! per-item extents (measured or estimated), it answers "which item indices are
//! visible (plus a cache buffer)?" in `O(log n)`, supports `O(log n)` structural
//! edits (mid-list insert/delete via [`Virtualizer::set_count`]), and corrects
//! the scroll anchor — by **item identity**, not raw pixel — when a measured
//! extent differs from its estimate.
//!
//! # Agnostic by contract (the API-GATE invariant)
//!
//! Nothing in this module's public surface names a render, sliver, or protocol
//! type. The only types crossing the boundary are [`ScrollWindow`],
//! [`VisibleRange`], [`AnchorCorrection`], [`Extent`], [`ItemExtent`], the
//! [`Virtualizer`] itself, and primitives (`usize`, `f32`). This is deliberate:
//! it keeps the core a general-purpose abstraction (equally the math behind a
//! virtualized list, grid, data table, timeline, or text view) and keeps it
//! cheaply extractable into a standalone crate later, without coupling it to the
//! render layer or creating a dependency cycle. The consumer-side
//! `SliverConstraints → ScrollWindow` adapter lives *outside* this module.
//!
//! # Backbone
//!
//! The [`Virtualizer`] is backed by a focused augmented B+-tree
//! ([`sumtree::ExtentTree`]) over per-item extents — `{ count, total_extent }`
//! summaries at every node give `O(log n)` seek in both directions
//! (offset→index and index→offset) *and* `O(log n)` insert/delete. Each item is
//! type-level [`ItemExtent::Unmeasured`] | [`ItemExtent::Measured`] so
//! "estimated vs. measured" is unrepresentable as anything but a variant — not a
//! side boolean.
//!
//! # Anchor is item-identity, not raw pixel
//!
//! The scroll anchor is `(index, sub_offset)`. A raw-pixel anchor silently jumps
//! when an item *above* the viewport is re-measured (the canonical "items jump"
//! bug). [`Virtualizer::set_measured`] therefore returns a **signed**
//! [`AnchorCorrection`] iff a re-measure shifts content above the anchor item;
//! the caller accumulates and applies it (sync or deferred) at its discretion.
//! The *policy* of when to apply, defer, or suppress a correction is a consumer
//! concern and lives with the consumer, not here.
//!
//! # Example
//!
//! ```
//! use flui_rendering::virtualization::{Extent, ScrollWindow, Virtualizer};
//!
//! // 1000 items, each estimated at 24px until measured.
//! let mut v = Virtualizer::new(1000, 24.0);
//! assert_eq!(v.total_extent(), Extent::Estimated(24_000.0));
//!
//! // Which items intersect a 600px viewport scrolled to 1200px (+ cache)?
//! let window = ScrollWindow { offset: 1200.0, main_extent: 600.0, cache_before: 100.0, cache_after: 100.0 };
//! let range = v.query(&window);
//! assert!(range.first <= range.last);
//! assert!(range.cache_first <= range.first && range.last <= range.cache_last);
//!
//! // Feed back a real measurement; a larger-than-estimated item above the
//! // anchor yields a correction the consumer applies to stay stationary.
//! let anchor = v.anchor_item();
//! if let Some(correction) = v.set_measured(0, 40.0, (anchor.0.max(1), 0.0)) {
//!     assert_eq!(correction.delta, 16.0); // 40 measured - 24 estimated
//! }
//! ```

mod sumtree;

use sumtree::ExtentTree;

/// A scroll window: the viewport plus the cache buffer the caller wants kept
/// built ahead of and behind it. All fields are main-axis pixels.
///
/// This is the sole *input* value type the [`Virtualizer`] reads for a query. It
/// names nothing render- or sliver-specific; a consumer adapts its own
/// scroll/viewport state onto it (the `SliverConstraints → ScrollWindow` adapter
/// lives outside this module).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollWindow {
    /// Scroll offset of the leading visible edge, in main-axis pixels.
    pub offset: f32,
    /// Size of the viewport along the main axis, in pixels.
    pub main_extent: f32,
    /// Extra main-axis pixels to keep built *ahead of* the leading edge.
    pub cache_before: f32,
    /// Extra main-axis pixels to keep built *past* the trailing edge.
    pub cache_after: f32,
}

impl ScrollWindow {
    /// Convenience constructor with no cache buffer on either side.
    #[must_use]
    pub fn new(offset: f32, main_extent: f32) -> Self {
        Self {
            offset,
            main_extent,
            cache_before: 0.0,
            cache_after: 0.0,
        }
    }

    /// Leading edge of the cache region (`offset - cache_before`, floored at 0).
    #[inline]
    #[must_use]
    fn cache_start(&self) -> f32 {
        (self.offset - self.cache_before).max(0.0)
    }

    /// Trailing edge of the cache region (`offset + main_extent + cache_after`).
    #[inline]
    #[must_use]
    fn cache_end(&self) -> f32 {
        self.offset + self.main_extent + self.cache_after
    }

    /// Trailing edge of the tight visible band (`offset + main_extent`).
    #[inline]
    #[must_use]
    fn visible_end(&self) -> f32 {
        self.offset + self.main_extent
    }
}

/// The result of a [`Virtualizer::query`]: a **dual** range of item indices.
///
/// The tight visible band `[first, last)` is what intersects the viewport; the
/// extended cache band `[cache_first, cache_last)` additionally covers the cache
/// buffer. `cache_first <= first <= last <= cache_last` always holds, so a
/// caller can prioritize rendering the visible band while measuring/prefetching
/// the cache band. All ranges are half-open and empty when `first == last`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[must_use]
pub struct VisibleRange {
    /// First item index intersecting the viewport (inclusive).
    pub first: usize,
    /// One past the last item index intersecting the viewport (exclusive).
    pub last: usize,
    /// First item index in the cache band (inclusive); `<= first`.
    pub cache_first: usize,
    /// One past the last item index in the cache band (exclusive); `>= last`.
    pub cache_last: usize,
    /// The first *visible* item's offset minus `window.offset`. Always `<= 0`
    /// (the item starts at or before the leading edge), so a consumer places the
    /// leading child at this relative offset. `0.0` for an empty range.
    pub leading_offset: f32,
}

impl VisibleRange {
    /// The empty range (no items visible or cached). `leading_offset` is `0.0`.
    const EMPTY: Self = Self {
        first: 0,
        last: 0,
        cache_first: 0,
        cache_last: 0,
        leading_offset: 0.0,
    };
}

/// A signed scroll-anchor correction, in main-axis pixels.
///
/// Emitted by [`Virtualizer::set_measured`] when re-measuring an item *above*
/// the anchor changes the total extent of the content preceding the anchor: the
/// anchor's pixel position would shift by `delta`, so the caller adjusts the
/// scroll offset by `delta` to keep the anchored content visually stationary.
/// The sign is meaningful (a larger-than-estimated item yields a positive delta;
/// smaller yields negative). The caller *accumulates* corrections and decides
/// when to apply them — this type carries no policy.
#[derive(Debug, Clone, Copy, PartialEq)]
#[must_use]
pub struct AnchorCorrection {
    /// Signed pixel delta to add to the scroll offset to keep the anchored
    /// content stationary.
    pub delta: f32,
}

/// The total scroll extent of all items, distinguishing "fully known" from
/// "still estimated".
///
/// While any item is still [`ItemExtent::Unmeasured`] the total is
/// [`Extent::Estimated`] (it includes estimate hints and may change as items are
/// measured); once every item is [`ItemExtent::Measured`] it is
/// [`Extent::Exact`]. A consumer uses this for scrollbar stability — an exact
/// extent need not be re-derived, an estimated one is provisional.
#[derive(Debug, Clone, Copy, PartialEq)]
#[must_use]
pub enum Extent {
    /// Every item is measured; this total is final.
    Exact(f32),
    /// At least one item is still estimated; this total is provisional.
    Estimated(f32),
}

impl Extent {
    /// The pixel value, regardless of exact/estimated.
    #[inline]
    #[must_use]
    pub fn value(self) -> f32 {
        match self {
            Extent::Exact(v) | Extent::Estimated(v) => v,
        }
    }
}

/// A single item's extent: either an estimate (not yet laid out) or a measured
/// value. Type-level so "estimated vs. measured" is a variant, never a side
/// boolean — illegal states (e.g. "measured but no value") are unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ItemExtent {
    /// Not yet laid out; `hint` is the estimate used for windowing until a real
    /// measurement arrives.
    Unmeasured {
        /// Estimated main-axis extent in pixels.
        hint: f32,
    },
    /// Laid out; `extent` is the real measured main-axis extent in pixels.
    Measured {
        /// Measured main-axis extent in pixels.
        extent: f32,
    },
}

impl ItemExtent {
    /// The pixel extent this item contributes, whether estimated or measured.
    ///
    /// A negative value would corrupt the prefix sums and break the seek
    /// monotonicity the backing tree relies on; the constructors floor extents
    /// at zero (see [`Virtualizer::new`] / [`Virtualizer::set_measured`]), so the
    /// stored value is already non-negative and is returned as-is.
    #[inline]
    #[must_use]
    pub fn extent(&self) -> f32 {
        match *self {
            ItemExtent::Unmeasured { hint } => hint,
            ItemExtent::Measured { extent } => extent,
        }
    }

    /// Whether this item has been measured (vs. still carrying an estimate).
    #[inline]
    #[must_use]
    pub fn is_measured(&self) -> bool {
        matches!(self, ItemExtent::Measured { .. })
    }
}

/// Protocol-agnostic windowing engine over a list of `n` items with per-item
/// extents.
///
/// Answers *visible-range* and *anchor* queries only — it is **build-agnostic**:
/// it never builds, lays out, or names a child render object. It is pure
/// arithmetic over `usize` indices and `f32` extents, backed by a focused
/// augmented B+-tree so every operation below is `O(log n)` (worst case as well
/// as average; the tree is balanced by construction).
///
/// # Estimate-then-correct
///
/// New items start [`ItemExtent::Unmeasured`] with a caller-supplied default
/// estimate, so [`total_extent`](Self::total_extent) and the scrollbar are
/// stable before every item is laid out. As real extents arrive via
/// [`set_measured`](Self::set_measured), the total converges and may emit an
/// [`AnchorCorrection`] to keep on-screen content stationary.
#[derive(Debug, Clone)]
pub struct Virtualizer {
    /// Augmented B+-tree over per-item extents.
    tree: ExtentTree,
    /// Default estimate seeded into newly-created [`ItemExtent::Unmeasured`]
    /// items (by `set_count` growth and `invalidate_from`).
    default_estimate: f32,
    /// How many items are currently [`ItemExtent::Measured`]. Maintained
    /// incrementally so [`total_extent`](Self::total_extent) /
    /// [`measured_count`](Self::measured_count) are `O(1)`.
    measured: usize,
    /// The current scroll anchor `(index, sub_offset)`. Item-identity, not raw
    /// pixel — see the module docs.
    anchor: (usize, f32),
    /// Main-axis viewport extent recorded from the most recent
    /// [`query`](Self::query). [`scroll_to_item`](Self::scroll_to_item) needs it
    /// to honour non-leading alignment; `0.0` before any query, which makes
    /// every alignment collapse to leading-edge (a safe default).
    viewport_extent: f32,
}

impl Virtualizer {
    /// Creates a virtualizer over `item_count` items, each seeded as
    /// [`ItemExtent::Unmeasured`] with `default_estimate`.
    ///
    /// Complexity: `O(item_count)` (bulk-loads the backing tree bottom-up).
    #[must_use]
    pub fn new(item_count: usize, default_estimate: f32) -> Self {
        let est = default_estimate.max(0.0);
        let tree = ExtentTree::from_fn(item_count, |_| ItemExtent::Unmeasured { hint: est });
        Self {
            tree,
            default_estimate: est,
            measured: 0,
            anchor: (0, 0.0),
            viewport_extent: 0.0,
        }
    }

    /// Number of items.
    ///
    /// Complexity: `O(1)`.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.tree.len()
    }

    /// Whether there are no items.
    ///
    /// Complexity: `O(1)`.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tree.len() == 0
    }

    /// Resizes to `n` items: appends new [`ItemExtent::Unmeasured`] items (with
    /// the default estimate) when growing, or removes trailing items when
    /// shrinking.
    ///
    /// Each appended/removed item is an `O(log n)` tree edit, so this is
    /// `O(|n - len()| · log n)` — *not* the `O(n)` array shift a flat-array
    /// Fenwick/BIT would pay for a structural change. The anchor index is
    /// clamped into range if shrinking dropped it.
    pub fn set_count(&mut self, n: usize) {
        let cur = self.tree.len();
        if n > cur {
            for index in cur..n {
                self.tree.insert(
                    index,
                    ItemExtent::Unmeasured {
                        hint: self.default_estimate,
                    },
                );
            }
        } else {
            for _ in n..cur {
                // Remove from the tail; trailing items are never measured-tracked
                // out of order, so decrement `measured` for any measured tail.
                let last = self.tree.len() - 1;
                let removed = self.tree.remove(last);
                if removed.is_measured() {
                    self.measured -= 1;
                }
            }
        }
        self.clamp_anchor();
    }

    /// Records the real measured `extent` for the item at `index`, replacing its
    /// estimate (or a previous measurement).
    ///
    /// `anchor` is the caller's current anchor `(index, sub_offset)`; it becomes
    /// the virtualizer's anchor. Returns `Some(AnchorCorrection)` **iff** the
    /// measured item lies strictly *above* the anchor item and its extent
    /// actually changed — in which case `delta` is the signed pixel shift the
    /// anchored content would otherwise undergo (`new_extent - old_extent`),
    /// which the caller adds to its scroll offset to stay stationary. Returns
    /// `None` for a measure at or below the anchor, or one that did not change
    /// the extent.
    ///
    /// Complexity: `O(log n)` (one tree point-update plus the anchor compare).
    pub fn set_measured(
        &mut self,
        index: usize,
        extent: f32,
        anchor: (usize, f32),
    ) -> Option<AnchorCorrection> {
        debug_assert!(index < self.tree.len(), "set_measured index out of range");
        if index >= self.tree.len() {
            return None;
        }
        self.anchor = anchor;
        self.clamp_anchor();

        let new_extent = extent.max(0.0);
        let old = self
            .tree
            .set(index, ItemExtent::Measured { extent: new_extent });
        if !old.is_measured() {
            self.measured += 1;
        }
        let old_extent = old.extent();

        let delta = new_extent - old_extent;
        // A correction is owed only when the changed item sits strictly above
        // the anchor item: only then does the anchor's pixel position move. A
        // measure at or below the anchor leaves everything above it untouched.
        if index < self.anchor.0 && delta != 0.0 {
            Some(AnchorCorrection { delta })
        } else {
            None
        }
    }

    /// Returns the dual visible/cache [`VisibleRange`] for `window`, and records
    /// `window.main_extent` as the viewport extent
    /// [`scroll_to_item`](Self::scroll_to_item) aligns against.
    ///
    /// `[first, last)` is the tight band intersecting the viewport;
    /// `[cache_first, cache_last)` additionally spans the cache buffer.
    /// `leading_offset` is the first visible item's offset minus `window.offset`
    /// (`<= 0`). Empty ([`VisibleRange::EMPTY`]) when there are no items.
    ///
    /// Takes `&mut self` (not `&self`) only to record the viewport extent — a
    /// `Virtualizer` is the stateful windowing engine for one scroll view, so it
    /// learns that view's extent here. The returned range is a pure function of
    /// the window and the current extents.
    ///
    /// Complexity: `O(log n)` — four boundary seeks, each `O(log n)`. (The
    /// returned [`VisibleRange`] is itself `#[must_use]`.)
    pub fn query(&mut self, window: &ScrollWindow) -> VisibleRange {
        self.viewport_extent = window.main_extent.max(0.0);
        let count = self.tree.len();
        if count == 0 {
            return VisibleRange::EMPTY;
        }

        // Visible band: items intersecting [offset, offset + main_extent).
        let (first, leading_into) = self.tree.seek_offset(window.offset.max(0.0));
        let leading_offset = -leading_into;
        let last = self.exclusive_end(window.visible_end(), first);

        // Cache band: items intersecting [cache_start, cache_end).
        let (cache_first, _) = self.tree.seek_offset(window.cache_start());
        let cache_last = self.exclusive_end(window.cache_end(), cache_first);

        VisibleRange {
            first,
            last,
            cache_first,
            cache_last,
            leading_offset,
        }
    }

    /// Computes the exclusive end index for a band whose first item is `first`
    /// and which ends at pixel `end`.
    ///
    /// The end item is the one whose span contains `end`; it is *included* (so
    /// the exclusive end is its index + 1) when `end` falls strictly inside it,
    /// and *excluded* when `end` lands exactly on its leading edge (a half-open
    /// `[start, end)` band touching a boundary does not intersect the next
    /// item). The result is clamped to `[first, count]`, so an empty band
    /// (`end <= band start`) yields `first` — never an inverted range.
    fn exclusive_end(&self, end: f32, first: usize) -> usize {
        let count = self.tree.len();
        let total = self.tree.total_extent();
        if end >= total {
            return count;
        }
        let band_start = self.tree.offset_of(first);
        if end <= band_start {
            // Zero-or-negative-width band: empty.
            return first;
        }
        let (end_idx, into) = self.tree.seek_offset(end);
        // `into == 0` means `end` is exactly on item `end_idx`'s leading edge:
        // that item is not intersected, so the exclusive end is `end_idx`.
        let exclusive = if into <= 0.0 { end_idx } else { end_idx + 1 };
        exclusive.clamp(first, count)
    }

    /// The offset (in main-axis pixels) at which item `index` starts; equals the
    /// sum of all earlier items' extents. `offset_of(0) == 0.0`,
    /// `offset_of(len())` is the total extent.
    ///
    /// Complexity: `O(log n)`.
    ///
    /// # Panics
    /// Panics if `index > len()`.
    #[must_use]
    pub fn offset_of(&self, index: usize) -> f32 {
        assert!(index <= self.tree.len(), "offset_of index out of range");
        self.tree.offset_of(index)
    }

    /// Whether the item at `index` has been measured (vs. still estimated).
    /// Returns `false` for an out-of-range index.
    ///
    /// Complexity: `O(log n)`.
    #[must_use]
    pub fn is_measured(&self, index: usize) -> bool {
        if index >= self.tree.len() {
            return false;
        }
        self.tree.get(index).is_measured()
    }

    /// Resets items at `index..len()` back to [`ItemExtent::Unmeasured`] with the
    /// default estimate — a watermark to discard stale measurements after a
    /// structural change invalidates everything from `index` onward.
    ///
    /// Complexity: `O((len() - index) · log n)`.
    pub fn invalidate_from(&mut self, index: usize) {
        let count = self.tree.len();
        for i in index..count {
            let old = self.tree.set(
                i,
                ItemExtent::Unmeasured {
                    hint: self.default_estimate,
                },
            );
            if old.is_measured() {
                self.measured -= 1;
            }
        }
    }

    /// The current scroll anchor `(index, sub_offset)`. A consumer reads this to
    /// restore position after a layout invalidation.
    ///
    /// Complexity: `O(1)`.
    #[must_use]
    pub fn anchor_item(&self) -> (usize, f32) {
        self.anchor
    }

    /// The total scroll extent, tagged [`Extent::Exact`] once every item is
    /// measured and [`Extent::Estimated`] while any estimate remains.
    ///
    /// Complexity: `O(1)` (the total is a cached tree summary; the
    /// exact/estimated tag is an `O(1)` measured-count compare). (The returned
    /// [`Extent`] is itself `#[must_use]`.)
    pub fn total_extent(&self) -> Extent {
        let total = self.tree.total_extent();
        if self.measured == self.tree.len() {
            Extent::Exact(total)
        } else {
            Extent::Estimated(total)
        }
    }

    /// How many items have been measured.
    ///
    /// Complexity: `O(1)`.
    #[inline]
    #[must_use]
    pub fn measured_count(&self) -> usize {
        self.measured
    }

    /// How many items are still estimated (`len() - measured_count()`).
    ///
    /// Complexity: `O(1)`.
    #[inline]
    #[must_use]
    pub fn estimated_count(&self) -> usize {
        self.tree.len() - self.measured
    }

    /// Computes the scroll offset that brings item `index` into view at the
    /// given `alignment` within the viewport, and adopts `index` (leading edge)
    /// as the anchor.
    ///
    /// `alignment` is `0.0` for the item's leading edge flush with the viewport
    /// leading edge, `1.0` for the item's trailing edge flush with the viewport
    /// trailing edge, `0.5` for centered; it is clamped to `[0, 1]`. The
    /// viewport extent used is the one recorded by the most recent
    /// [`query`](Self::query); before any query it is `0.0`, so the offset
    /// positions the item's `alignment`-fraction point at the scroll origin
    /// (`alignment = 0.0` still yields the item's leading edge). The returned
    /// offset is clamped to `[0, max_scroll]` where
    /// `max_scroll = max(0, total_extent - viewport)`.
    ///
    /// **Fixpoint-measure caveat:** if `index` (or any item before it) is still
    /// [`ItemExtent::Unmeasured`], the offset is computed from estimates; a
    /// consumer needing pixel-exact alignment to an unmeasured target must
    /// measure it (lay it out) and re-query, iterating to a fixpoint.
    ///
    /// Complexity: `O(log n)`.
    ///
    /// # Panics
    /// Panics if `index >= len()` (there is no such item to scroll to).
    #[must_use]
    pub fn scroll_to_item(&mut self, index: usize, alignment: f32) -> f32 {
        assert!(index < self.tree.len(), "scroll_to_item index out of range");
        let item_start = self.tree.offset_of(index);
        let item_extent = self.tree.get(index).extent();
        let viewport = self.viewport_extent;
        let a = alignment.clamp(0.0, 1.0);

        // Offset that places the item's leading edge at fraction `a` of the way
        // the item can travel across the viewport: a=0 → leading flush, a=1 →
        // trailing flush, a=0.5 → centered.
        let target = item_start - a * (viewport - item_extent);

        self.anchor = (index, 0.0);

        let max_scroll = (self.tree.total_extent() - viewport).max(0.0);
        target.clamp(0.0, max_scroll)
    }

    /// Clamps the anchor index into `[0, len())` (or `(0, 0.0)` when empty),
    /// after a structural change may have dropped the anchored item.
    fn clamp_anchor(&mut self) {
        let count = self.tree.len();
        if count == 0 {
            self.anchor = (0, 0.0);
        } else if self.anchor.0 >= count {
            self.anchor = (count - 1, 0.0);
        }
    }
}

#[cfg(test)]
mod tests;
