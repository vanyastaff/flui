//! `RenderSliverFixedExtentList` — lazily built Box children that all share one
//! main-axis extent.
//!
//! The fixed extent makes every offset a multiplication: the first and last
//! index of the window, the layout offset of any child, and the total scroll
//! extent are all index arithmetic, so the sliver never needs to measure a
//! child it has not built and never needs an estimate. That is the whole
//! reason this type exists beside [`RenderSliverList`](super::RenderSliverList),
//! whose extents are measured and virtualized.
//!
//! Children are built through the request strategy shared with the list and
//! the grid: the sliver lays out the residents inside its window, asks the
//! element tree for the absent ones (`request_child_build`), and emits the
//! window as its retain band (`emit_retain_band`) so the element tree evicts
//! everything outside it. It never disposes a child itself.
//!
//! # Mapping decisions
//!
//! Flutter's `RenderSliverFixedExtentBoxAdaptor` (`rendering/sliver_fixed_extent_list.dart`)
//! is the behavioural reference for the index math (`getMinChildIndexForScrollOffset`,
//! `getMaxChildIndexForScrollOffset`, `indexToLayoutOffset`, `computeMaxScrollOffset`),
//! the geometry (`paintExtent` / `cacheExtent` from the leading and trailing
//! layout offsets, `hasVisualOverflow` from `targetLastIndexForPaint`) and the
//! empty / past-the-end arms. What differs, and why:
//!
//! - **No `scrollOffsetCorrection`.** Flutter teleports the viewport when a
//!   *leading* child fails to build mid-layout. Under the request strategy the
//!   sliver never builds mid-layout, so an absent index is a request, not a
//!   failure; a data source that shrinks is reported by the builder to the
//!   element tree, which clamps the item count, and the next pass reports the
//!   real extent and the viewport clamps its pixels. A non-monotone builder
//!   therefore truncates at its first `None` where Flutter would teleport.
//! - **The precision tolerance is `f32`-scaled.** Flutter compares layout
//!   offsets in doubles against `precisionErrorTolerance = 1e-10`; FLUI's
//!   pixels are `f32`, whose spacing at a few thousand pixels is already
//!   `~1e-4`, so `PRECISION_ERROR_TOLERANCE` is `1e-3` px — far below any
//!   layout-relevant distance, wide enough to absorb the rounding the
//!   reference's own regression tests exist for.
//! - **An unbounded window is bounded here.** With an infinite
//!   `remainingCacheExtent` Flutter lays out to the end of the data; so does
//!   this sliver for a real count (shrink-wrap materialises everything, as
//!   Flutter does), but a `usize::MAX` "unknown" count is read as the sentinel
//!   it is and served as a small bounded window, exactly as the lazy grid does
//!   (`MAX_UNBOUNDED_WINDOW_CHILDREN`, `UNBOUNDED_SENTINEL_WINDOW`).
//! - **Non-finite scroll-window inputs never reach `as usize`.** Flutter's
//!   `RenderSliverFixedExtentBoxAdaptor` crashes on `Infinity or NaN toInt`
//!   (flutter/flutter#105630). Rust's float-to-int cast saturates instead
//!   (`+∞ → usize::MAX`, `NaN → 0`), which is worse operationally: a poisoned
//!   retain band or build window can freeze the viewport without a crisp
//!   error. Index helpers and `window` reject `NaN` and `+∞` leading edges
//!   (and `NaN`/`−∞` trailing edges) with a `tracing::error!` and an empty
//!   window; `−∞` on the leading edge still clamps to `0` via `.max(0.0)`,
//!   matching a negative finite offset. Only a positive-infinite trailing
//!   cache extent keeps the intentional unbounded-window meaning. The grid
//!   applies the same leading/trailing policy.

use std::collections::BTreeMap;

use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
use flui_tree::Variable;
use flui_types::geometry::px;

use flui_rendering::{
    constraints::{SliverConstraints, SliverGeometry, child_paint_offset},
    context::{PaintCx, SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverMultiBoxAdaptorParentData,
    protocol::ChildLayout,
    traits::RenderSliver,
};

use super::sliver_grid::{MAX_UNBOUNDED_WINDOW_CHILDREN, UNBOUNDED_SENTINEL_WINDOW};

/// How far a layout offset may miss an exact multiple of the item extent and
/// still count as that multiple, in pixels. Flutter's `precisionErrorTolerance`
/// scaled from doubles to `f32` (see the module's mapping decisions).
pub const PRECISION_ERROR_TOLERANCE: f32 = 1e-3;

/// A sliver that places lazily built Box children one after another along the
/// scroll axis, each with the same main-axis extent.
///
/// The element tree owns the children: the sliver requests the indices its
/// window needs and retains the window as its band. Layout of a child is a
/// tight constraint on the main axis (`item_extent`) and the sliver's cross
/// axis; its position is `index × item_extent`.
#[derive(Debug, Clone)]
pub struct RenderSliverFixedExtentList {
    item_extent: f32,
    item_count: usize,
    /// Logical index → dense slot of every attached child, rebuilt each pass
    /// from the children's parent data.
    logical_to_slot: BTreeMap<usize, usize>,
    /// Attached children at the end of the last layout, for hit-testing.
    attached_child_count: usize,
    /// The count the unbounded-window truncation last warned about, so the
    /// warning fires once per count rather than once per frame.
    warned_truncation_for: Option<usize>,
}

impl RenderSliverFixedExtentList {
    /// Creates a list of `item_count` children, each `item_extent` pixels along
    /// the main axis.
    ///
    /// # Panics
    ///
    /// Panics if `item_extent` is not finite or not greater than zero.
    #[inline]
    #[must_use]
    pub fn new(item_extent: f32, item_count: usize) -> Self {
        assert!(
            item_extent.is_finite() && item_extent > 0.0,
            "item_extent must be finite and greater than zero"
        );
        Self {
            item_extent,
            item_count,
            logical_to_slot: BTreeMap::new(),
            attached_child_count: 0,
            warned_truncation_for: None,
        }
    }

    /// The main-axis extent every child is laid out to.
    #[inline]
    #[must_use]
    pub const fn item_extent(&self) -> f32 {
        self.item_extent
    }

    /// Sets the per-child main-axis extent.
    ///
    /// # Panics
    ///
    /// Panics if `item_extent` is not finite or not greater than zero.
    #[inline]
    pub fn set_item_extent(&mut self, item_extent: f32) -> flui_rendering::RenderUpdateImpact {
        assert!(
            item_extent.is_finite() && item_extent > 0.0,
            "item_extent must be finite and greater than zero"
        );
        if self.item_extent == item_extent {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.item_extent = item_extent;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// The number of children the data source declares.
    #[inline]
    #[must_use]
    pub const fn item_count(&self) -> usize {
        self.item_count
    }

    /// Sets the declared child count. The element tree calls this when the
    /// builder declines an index below the count (the data source shrank).
    #[inline]
    pub fn set_item_count(&mut self, item_count: usize) -> flui_rendering::RenderUpdateImpact {
        if self.item_count == item_count {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.item_count = item_count;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// The first child index whose extent reaches `scroll_offset`
    /// (Flutter's `getMinChildIndexForScrollOffset`).
    ///
    /// An offset within `PRECISION_ERROR_TOLERANCE` of an item boundary
    /// counts as that boundary, so accumulated rounding never pulls in the
    /// child that ends exactly there.
    ///
    /// # Non-finite offsets
    ///
    /// `NaN` and `+∞` are rejected: the helper returns `0` and emits a
    /// `tracing::error!`. Rust's `f32 as usize` would otherwise saturate `+∞`
    /// to [`usize::MAX`], which must never become a retain-band or
    /// build-request edge. `−∞` is treated like any negative offset and
    /// selects index `0` without an error.
    #[must_use]
    pub fn min_child_index_for_scroll_offset(&self, scroll_offset: f32) -> usize {
        if is_poison_scroll_offset(scroll_offset) {
            tracing::error!(
                scroll_offset,
                item_extent = self.item_extent,
                render_object = "RenderSliverFixedExtentList",
                "min_child_index_for_scroll_offset rejected a NaN or +∞ scroll offset; \
                 returning 0 so index math cannot saturate to usize::MAX"
            );
            return 0;
        }
        if self.item_extent <= 0.0 {
            return 0;
        }
        let actual = scroll_offset / self.item_extent;
        let round = actual.round();
        let index = if ((actual - round) * self.item_extent).abs() < PRECISION_ERROR_TOLERANCE {
            round
        } else {
            actual.floor()
        };
        float_to_index(index)
    }

    /// The last child index that starts before `scroll_offset`
    /// (Flutter's `getMaxChildIndexForScrollOffset`): the child that ends
    /// exactly at the offset is not included.
    ///
    /// # Non-finite offsets
    ///
    /// Same contract as [`Self::min_child_index_for_scroll_offset`]: `NaN` /
    /// `+∞` return `0` with a `tracing::error!`; `−∞` selects index `0`.
    #[must_use]
    pub fn max_child_index_for_scroll_offset(&self, scroll_offset: f32) -> usize {
        if is_poison_scroll_offset(scroll_offset) {
            tracing::error!(
                scroll_offset,
                item_extent = self.item_extent,
                render_object = "RenderSliverFixedExtentList",
                "max_child_index_for_scroll_offset rejected a NaN or +∞ scroll offset; \
                 returning 0 so index math cannot saturate to usize::MAX"
            );
            return 0;
        }
        if self.item_extent <= 0.0 {
            return 0;
        }
        let actual = scroll_offset / self.item_extent - 1.0;
        let round = actual.round();
        let index = if ((actual - round) * self.item_extent).abs() < PRECISION_ERROR_TOLERANCE {
            round
        } else {
            actual.ceil()
        };
        float_to_index(index)
    }

    /// The layout offset of child `index` (Flutter's `indexToLayoutOffset`).
    #[inline]
    #[must_use]
    pub fn index_to_layout_offset(&self, index: usize) -> f32 {
        self.item_extent * index as f32
    }

    /// The scroll extent of `item_count` children
    /// (Flutter's `computeMaxScrollOffset`).
    #[inline]
    #[must_use]
    pub fn compute_max_scroll_offset(&self, item_count: usize) -> f32 {
        self.item_extent * item_count as f32
    }

    /// The window `[first, last]` of logical indices the constraints ask for,
    /// and the count the reported extent covers, or `None` when the window
    /// starts past the last item or the leading edge is `NaN`/`+∞`.
    fn window(&mut self, constraints: &SliverConstraints) -> Option<(usize, usize, usize)> {
        let Some(cache_start) = finite_leading_cache_edge(constraints) else {
            tracing::error!(
                scroll_offset = constraints.scroll_offset,
                cache_origin = constraints.cache_origin,
                remaining_cache_extent = constraints.remaining_cache_extent,
                item_extent = self.item_extent,
                item_count = self.item_count,
                render_object = "RenderSliverFixedExtentList",
                "fixed-extent list received a NaN or +∞ leading cache/scroll edge; \
                 treating the window as empty so index math cannot saturate to usize::MAX"
            );
            return None;
        };
        let cache_end = cache_start + constraints.remaining_cache_extent;
        let first = self.min_child_index_for_scroll_offset(cache_start);
        let (last, effective_count) = if cache_end.is_finite() {
            let last = self
                .max_child_index_for_scroll_offset(cache_end)
                .min(self.item_count - 1);
            (last, self.item_count)
        } else if cache_end.is_infinite() && cache_end.is_sign_positive() {
            // Intentional unbounded trailing edge (shrink-wrap / unknown extent).
            if self.item_count > MAX_UNBOUNDED_WINDOW_CHILDREN {
                if self.warned_truncation_for != Some(self.item_count) {
                    self.warned_truncation_for = Some(self.item_count);
                    tracing::warn!(
                        item_count = self.item_count,
                        threshold = MAX_UNBOUNDED_WINDOW_CHILDREN,
                        window = UNBOUNDED_SENTINEL_WINDOW,
                        "fixed-extent list asked to fill an unbounded main axis declares \
                         more children than any real data source has; reading the count \
                         as an undefined-count stand-in and serving a small bounded window \
                         instead, so the committed extent is far short of the declared \
                         content"
                    );
                }
                (UNBOUNDED_SENTINEL_WINDOW - 1, UNBOUNDED_SENTINEL_WINDOW)
            } else {
                (self.item_count - 1, self.item_count)
            }
        } else {
            // NaN or −∞ trailing edge: not a meaningful unbounded window.
            tracing::error!(
                scroll_offset = constraints.scroll_offset,
                cache_origin = constraints.cache_origin,
                remaining_cache_extent = constraints.remaining_cache_extent,
                cache_end,
                item_extent = self.item_extent,
                item_count = self.item_count,
                render_object = "RenderSliverFixedExtentList",
                "fixed-extent list received a non-finite trailing cache edge that is not \
                 +∞; treating the window as empty so index math cannot saturate to usize::MAX"
            );
            return None;
        };
        (first <= last).then_some((first, last, effective_count))
    }
}

/// Clamp a rounded index to `usize`: negative offsets (a cache origin above
/// the content) and any rounding below zero mean "the first child".
///
/// Non-finite values are a defensive second line: the public helpers and
/// [`RenderSliverFixedExtentList::window`] reject `NaN`/`+∞` first, but
/// `+∞ as usize` would otherwise silently become [`usize::MAX`].
fn float_to_index(index: f32) -> usize {
    if !index.is_finite() || index <= 0.0 {
        0
    } else {
        index as usize
    }
}

/// `NaN` or `+∞` — the values that must not reach float→index conversion.
///
/// `−∞` is intentionally excluded: like any negative offset it clamps to the
/// origin via `.max(0.0)` / [`float_to_index`].
#[inline]
fn is_poison_scroll_offset(value: f32) -> bool {
    value.is_nan() || (value.is_infinite() && value.is_sign_positive())
}

/// Leading edge of the cache-extended scroll window, or `None` when the sum
/// is `NaN` or `+∞`.
///
/// `−∞` and negative finites clamp to `0.0` — the pre-guard `.max(0.0)`
/// behaviour — so a pathological negative-infinite leading edge still lays
/// out from the origin instead of evicting the window. `NaN` is rejected
/// before `.max(0.0)` because Rust's `f32::max` would otherwise return `0.0`
/// and hide the contract violation.
fn finite_leading_cache_edge(constraints: &SliverConstraints) -> Option<f32> {
    let leading = constraints.scroll_offset + constraints.cache_origin;
    if is_poison_scroll_offset(leading) {
        return None;
    }
    Some(leading.max(0.0))
}

impl Diagnosticable for RenderSliverFixedExtentList {
    fn debug_fill_properties(&self, properties: &mut DiagnosticsBuilder) {
        properties.add_double("item_extent", self.item_extent, Some("px"));
        properties.add_int("item_count", self.item_count as i64, None);
        properties.add_int(
            "attached_child_count",
            self.attached_child_count as i64,
            None,
        );
    }
}

impl RenderSliver for RenderSliverFixedExtentList {
    type Arity = Variable;
    type ParentData = SliverMultiBoxAdaptorParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Variable, Self::ParentData>,
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();

        if self.item_count == 0 {
            self.attached_child_count = 0;
            ctx.emit_retain_band(0, 0);
            return SliverGeometry::ZERO;
        }

        let Some((first, mut last, effective_count)) = self.window(&constraints) else {
            // The window starts past the last item (scrolled beyond the end,
            // or the source shrank under the viewport), or the leading /
            // trailing cache edge was non-finite: report the extent the count
            // implies and let the viewport clamp. Flutter's `addInitialChild`
            // failing for `firstIndex > 0` reports the same `scrollExtent` /
            // `maxPaintExtent` pair. A non-finite leading edge must never be
            // converted to an index — that path saturates at `usize::MAX`.
            let scroll_extent = self.compute_max_scroll_offset(effective_count_for_past_end(
                self.item_count,
                &constraints,
            ));
            self.attached_child_count = ctx.child_count();
            let first = finite_leading_cache_edge(&constraints)
                .map_or(0, |start| self.min_child_index_for_scroll_offset(start));
            ctx.emit_retain_band(first, first);
            return SliverGeometry {
                scroll_extent,
                max_paint_extent: scroll_extent,
                ..SliverGeometry::ZERO
            };
        };

        self.logical_to_slot.clear();
        let dense_child_count = ctx.child_count();
        for slot in 0..dense_child_count {
            if let Some(pd) = ctx.child_parent_data(slot) {
                let previous = self.logical_to_slot.insert(pd.index, slot);
                debug_assert!(
                    previous.is_none(),
                    "BUG: fixed-extent list has two attached children stamped with logical \
                     index {} (dense slots {:?} and {slot})",
                    pd.index,
                    previous,
                );
            }
        }

        let child_constraints =
            constraints.as_box_constraints(self.item_extent, self.item_extent, None);
        let mut effective_count = effective_count;
        for logical_index in first..=last {
            if let Some(&slot) = self.logical_to_slot.get(&logical_index) {
                ctx.layout_box_child(slot, child_constraints);
                if let Some(pd) = ctx.child_parent_data_mut(slot) {
                    pd.layout_offset = self.index_to_layout_offset(logical_index);
                }
                continue;
            }
            match ctx.request_child_build(logical_index) {
                ChildLayout::NoChild => {
                    // The data source ends here: the count follows so this
                    // pass already reports the real extent (the element tree
                    // clamps the same way once it services the request).
                    self.item_count = logical_index;
                    effective_count = effective_count.min(logical_index);
                    last = logical_index.saturating_sub(1);
                    break;
                }
                ChildLayout::Unwired => break,
                _ => {}
            }
        }
        if self.item_count == 0 || first > last {
            self.attached_child_count = ctx.child_count();
            ctx.emit_retain_band(first, first);
            let scroll_extent = self.compute_max_scroll_offset(effective_count);
            return SliverGeometry {
                scroll_extent,
                max_paint_extent: scroll_extent,
                ..SliverGeometry::ZERO
            };
        }
        ctx.emit_retain_band(first, last + 1);

        let scroll_extent = self.compute_max_scroll_offset(effective_count);
        let leading_scroll_offset = self.index_to_layout_offset(first);
        let trailing_scroll_offset = self.index_to_layout_offset(last + 1);
        let paint_extent = self.calculate_paint_offset(
            &constraints,
            leading_scroll_offset,
            trailing_scroll_offset,
        );
        let cache_extent = self.calculate_cache_offset(
            &constraints,
            leading_scroll_offset,
            trailing_scroll_offset,
        );
        let target_end_for_paint = constraints.scroll_offset + constraints.remaining_paint_extent;
        let overflows_paint_window = target_end_for_paint.is_finite()
            && last >= self.max_child_index_for_scroll_offset(target_end_for_paint);
        let geometry = SliverGeometry {
            scroll_extent,
            paint_extent,
            layout_extent: paint_extent,
            max_paint_extent: scroll_extent,
            cache_extent,
            hit_test_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: overflows_paint_window || constraints.scroll_offset > 0.0,
            ..SliverGeometry::ZERO
        };

        // Committed only for a pass the pipeline will accept (see the grid).
        if geometry.validation_error().is_none() {
            self.attached_child_count = ctx.child_count();
        }

        for logical_index in first..=last {
            if let Some(&slot) = self.logical_to_slot.get(&logical_index) {
                let paint_offset = child_paint_offset(
                    &constraints,
                    &geometry,
                    px(self.index_to_layout_offset(logical_index)),
                    px(self.item_extent),
                );
                ctx.position_child(slot, paint_offset);
            }
        }

        geometry
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Variable>) {
        ctx.paint_children();
    }

    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, Variable, Self::ParentData>) -> bool {
        for slot in (0..self.attached_child_count).rev() {
            if ctx.hit_test_child_at_layout_offset(slot) {
                return true;
            }
        }
        false
    }
}

/// The count whose extent a past-the-end window reports: the declared count,
/// unless the window is unbounded and the count is the undefined-count
/// sentinel, in which case the same truncated window the in-band arm serves.
fn effective_count_for_past_end(item_count: usize, constraints: &SliverConstraints) -> usize {
    let Some(cache_start) = finite_leading_cache_edge(constraints) else {
        return item_count;
    };
    let cache_end = cache_start + constraints.remaining_cache_extent;
    if cache_end.is_infinite()
        && cache_end.is_sign_positive()
        && item_count > MAX_UNBOUNDED_WINDOW_CHILDREN
    {
        UNBOUNDED_SENTINEL_WINDOW
    } else {
        item_count
    }
}

#[cfg(test)]
mod tests {
    //! The index math, against Flutter's `rendering/sliver_fixed_extent_layout_test.dart`
    //! (`group('getMaxChildIndexForScrollOffset')` and the two
    //! `'… correctly references itemExtent …'` cases). The reference nudges
    //! offsets by `1e-10` / `1e-11` doubles around `precisionErrorTolerance`;
    //! here the nudges are `1e-2` / `1e-4` px around the `f32`-scaled
    //! `PRECISION_ERROR_TOLERANCE` — the same side of the tolerance each time.

    use super::*;

    const GENERIC_ITEM_EXTENT: f32 = 600.0;
    const OUTSIDE_TOLERANCE: f32 = 1e-2;
    const INSIDE_TOLERANCE: f32 = 1e-4;

    fn list(item_extent: f32) -> RenderSliverFixedExtentList {
        RenderSliverFixedExtentList::new(item_extent, 100)
    }

    #[test]
    fn max_index_is_zero_when_offset_is_zero() {
        assert_eq!(
            list(GENERIC_ITEM_EXTENT).max_child_index_for_scroll_offset(0.0),
            0
        );
    }

    #[test]
    fn max_index_is_zero_when_offset_equals_item_extent() {
        assert_eq!(
            list(GENERIC_ITEM_EXTENT).max_child_index_for_scroll_offset(GENERIC_ITEM_EXTENT),
            0
        );
    }

    #[test]
    fn max_index_is_one_when_offset_is_greater_than_item_extent() {
        assert_eq!(
            list(GENERIC_ITEM_EXTENT).max_child_index_for_scroll_offset(GENERIC_ITEM_EXTENT + 1.0),
            1
        );
    }

    #[test]
    fn max_index_is_one_when_offset_is_slightly_greater_than_item_extent() {
        assert_eq!(
            list(GENERIC_ITEM_EXTENT)
                .max_child_index_for_scroll_offset(GENERIC_ITEM_EXTENT + OUTSIDE_TOLERANCE),
            1
        );
    }

    #[test]
    fn max_index_is_four_when_offset_is_four_and_a_half_item_extents() {
        assert_eq!(
            list(GENERIC_ITEM_EXTENT).max_child_index_for_scroll_offset(GENERIC_ITEM_EXTENT * 4.5),
            4
        );
    }

    #[test]
    fn max_index_is_five_when_offset_is_six_item_extents() {
        const ANOTHER_GENERIC_ITEM_EXTENT: f32 = 414.0;
        assert_eq!(
            list(ANOTHER_GENERIC_ITEM_EXTENT)
                .max_child_index_for_scroll_offset(ANOTHER_GENERIC_ITEM_EXTENT * 6.0),
            5
        );
    }

    #[test]
    fn max_index_is_five_for_a_problematic_screen_extent_with_rounding_noise() {
        const PROBLEMATIC_ITEM_EXTENT: f32 = 411.428_57;
        assert_eq!(
            list(PROBLEMATIC_ITEM_EXTENT).max_child_index_for_scroll_offset(
                PROBLEMATIC_ITEM_EXTENT * 6.0 + INSIDE_TOLERANCE
            ),
            5
        );
    }

    #[test]
    fn max_index_is_zero_when_offset_is_a_hair_over_item_extent() {
        assert_eq!(
            list(GENERIC_ITEM_EXTENT)
                .max_child_index_for_scroll_offset(GENERIC_ITEM_EXTENT + INSIDE_TOLERANCE),
            0
        );
    }

    /// `'RenderSliverFixedExtentList correctly references itemExtent, non-zero
    /// offset'`: three 30 px items, scrolled to 45 px.
    #[test]
    fn index_math_references_the_configured_item_extent_at_a_non_zero_offset() {
        let list = RenderSliverFixedExtentList::new(30.0, 3);
        assert_eq!(list.index_to_layout_offset(10), 300.0);
        assert_eq!(list.min_child_index_for_scroll_offset(45.0), 1);
        assert_eq!(list.max_child_index_for_scroll_offset(45.0), 1);
        assert_eq!(list.compute_max_scroll_offset(3), 90.0);
    }

    /// `'… correctly references itemExtent, zero offset'`.
    #[test]
    fn index_math_references_the_configured_item_extent_at_zero_offset() {
        let list = RenderSliverFixedExtentList::new(30.0, 3);
        assert_eq!(list.min_child_index_for_scroll_offset(0.0), 0);
        assert_eq!(list.max_child_index_for_scroll_offset(0.0), 0);
        assert_eq!(list.compute_max_scroll_offset(3), 90.0);
    }

    /// The `'layout test - rounding error'` case: an offset a rounding error
    /// short of an item boundary selects the child at that boundary.
    #[test]
    fn min_index_absorbs_rounding_below_a_boundary() {
        let list = list(GENERIC_ITEM_EXTENT);
        assert_eq!(
            list.min_child_index_for_scroll_offset(GENERIC_ITEM_EXTENT * 2.0 - INSIDE_TOLERANCE),
            2
        );
        assert_eq!(
            list.min_child_index_for_scroll_offset(GENERIC_ITEM_EXTENT * 2.0 - OUTSIDE_TOLERANCE),
            1
        );
    }

    #[test]
    fn a_negative_offset_selects_the_first_child() {
        assert_eq!(
            list(GENERIC_ITEM_EXTENT).min_child_index_for_scroll_offset(-250.0),
            0
        );
        assert_eq!(
            list(GENERIC_ITEM_EXTENT).max_child_index_for_scroll_offset(-250.0),
            0
        );
    }

    #[test]
    fn set_item_count_reports_layout_only_on_change() {
        let mut list = RenderSliverFixedExtentList::new(30.0, 3);
        assert!(list.set_item_count(3).is_none());
        assert!(!list.set_item_count(2).is_none());
        assert_eq!(list.item_count(), 2);
    }

    #[test]
    #[should_panic(expected = "item_extent must be finite")]
    fn new_rejects_a_zero_extent() {
        let _ = RenderSliverFixedExtentList::new(0.0, 1);
    }

    /// `NaN` / `+∞` offsets must never become `usize::MAX` via saturating
    /// `f32 as usize`. `−∞` is treated like a negative offset → index `0`.
    #[test]
    fn index_helpers_reject_poison_offsets() {
        let list = list(GENERIC_ITEM_EXTENT);
        for offset in [f32::NAN, f32::INFINITY] {
            assert_eq!(
                list.min_child_index_for_scroll_offset(offset),
                0,
                "min helper for {offset}"
            );
            assert_eq!(
                list.max_child_index_for_scroll_offset(offset),
                0,
                "max helper for {offset}"
            );
        }
        assert_eq!(list.min_child_index_for_scroll_offset(f32::NEG_INFINITY), 0);
        assert_eq!(list.max_child_index_for_scroll_offset(f32::NEG_INFINITY), 0);
    }

    fn vertical_window_constraints(
        scroll_offset: f32,
        cache_origin: f32,
        remaining_cache_extent: f32,
    ) -> SliverConstraints {
        SliverConstraints {
            scroll_offset,
            cache_origin,
            remaining_cache_extent,
            remaining_paint_extent: 100.0,
            cross_axis_extent: 300.0,
            viewport_main_axis_extent: 100.0,
            ..SliverConstraints::default()
        }
    }

    /// `NaN` / `+∞` leading edges yield no window: layout takes the empty-band
    /// path instead of converting `+∞` into `usize::MAX`.
    #[test]
    fn window_rejects_poison_leading_edge() {
        let mut list = RenderSliverFixedExtentList::new(25.0, 1000);
        for (scroll_offset, cache_origin) in [
            (f32::INFINITY, 0.0),
            (f32::NAN, 0.0),
            (0.0, f32::NAN),
            (f32::NEG_INFINITY, f32::INFINITY), // sum is NaN
        ] {
            let constraints = vertical_window_constraints(scroll_offset, cache_origin, 250.0);
            assert!(
                list.window(&constraints).is_none(),
                "leading edge scroll_offset={scroll_offset} cache_origin={cache_origin}"
            );
            assert_eq!(
                finite_leading_cache_edge(&constraints)
                    .map_or(0, |start| list.min_child_index_for_scroll_offset(start)),
                0,
                "empty-band index must stay 0, never usize::MAX"
            );
        }
    }

    /// `−∞` on the leading edge clamps to the origin — same as a negative
    /// finite offset — and keeps a normal window instead of evicting.
    #[test]
    fn window_clamps_negative_infinite_leading_edge_to_origin() {
        let mut list = RenderSliverFixedExtentList::new(25.0, 10);
        let constraints = vertical_window_constraints(f32::NEG_INFINITY, 0.0, 250.0);
        assert_eq!(finite_leading_cache_edge(&constraints), Some(0.0));
        assert_eq!(list.window(&constraints), Some((0, 9, 10)));
    }

    /// `NaN` / `−∞` trailing edges are not the shrink-wrap unbounded contract;
    /// only `+∞` keeps that meaning.
    #[test]
    fn window_rejects_nan_or_negative_infinite_trailing_edge() {
        let mut list = RenderSliverFixedExtentList::new(25.0, 10);
        for remaining_cache_extent in [f32::NAN, f32::NEG_INFINITY] {
            let constraints = vertical_window_constraints(0.0, 0.0, remaining_cache_extent);
            assert!(
                list.window(&constraints).is_none(),
                "trailing extent {remaining_cache_extent}"
            );
        }
    }

    /// Positive-infinite trailing cache extent still means "unbounded window"
    /// and materialises the full finite source (below the sentinel threshold).
    #[test]
    fn window_keeps_positive_infinite_trailing_edge_as_unbounded() {
        let mut list = RenderSliverFixedExtentList::new(25.0, 10);
        let constraints = vertical_window_constraints(0.0, 0.0, f32::INFINITY);
        assert_eq!(list.window(&constraints), Some((0, 9, 10)));
    }

    /// Defense in depth: even a rounded non-finite index clamps to 0 instead
    /// of saturating at `usize::MAX`.
    #[test]
    fn float_to_index_never_saturates_non_finite_to_usize_max() {
        assert_eq!(float_to_index(f32::INFINITY), 0);
        assert_eq!(float_to_index(f32::NEG_INFINITY), 0);
        assert_eq!(float_to_index(f32::NAN), 0);
        assert_eq!(float_to_index(-1.0), 0);
        assert_eq!(float_to_index(42.7), 42);
    }
}
