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
//! The index math (first and last index for a scroll offset, layout offset of
//! an index, max scroll offset), the geometry (paint and cache extent from the
//! leading and trailing layout offsets, visual overflow from the last painted
//! index) and the empty / past-the-end arms follow directly from the fixed
//! extent. Design notes:
//!
//! - **No scroll-offset correction.** A viewport teleport when a *leading*
//!   child fails to build mid-layout is not needed: under the request strategy
//!   the sliver never builds mid-layout, so an absent index is a request, not a
//!   failure; a data source that shrinks is reported by the builder to the
//!   element tree, which clamps the item count, and the next pass reports the
//!   real extent and the viewport clamps its pixels. A non-monotone builder
//!   therefore truncates at its first `None`.
//! - **The precision tolerance.** Offsets are `f64`, so
//!   `PRECISION_ERROR_TOLERANCE` is `1e-10` px (ADR-0098): wide enough for the
//!   division's rounding, narrow enough that an edge `1e-4` px past a boundary
//!   reaches the next child.
//! - **An unbounded window is bounded here.** With an infinite
//!   `remaining_cache_extent` this sliver lays out to the end of the data for
//!   a real count (shrink-wrap materialises everything), but a `usize::MAX`
//!   "unknown" count is read as the sentinel
//!   it is and served as a small bounded window, exactly as the lazy grid does
//!   (`MAX_UNBOUNDED_WINDOW_CHILDREN`, `UNBOUNDED_SENTINEL_WINDOW`).
//! - **Non-finite scroll-window inputs never reach `as usize`.** Rust's
//!   float-to-int cast saturates on an infinite or NaN input
//!   (`+∞ → usize::MAX`, `NaN → 0`), which is worse operationally: a poisoned
//!   retain band or build window can freeze the viewport without a crisp
//!   error. Index helpers and `window` reject `NaN` and `+∞` leading edges
//!   (and `NaN`/`−∞` trailing edges) with a `tracing::error!` and an empty
//!   window; `−∞` on the leading edge still clamps to `0` via `.max(0.0)`,
//!   matching a negative finite offset. Only a positive-infinite trailing
//!   cache extent keeps the intentional unbounded-window meaning. The grid
//!   applies the same leading/trailing policy. Cross-object ledger:
//!   [`ARCHITECTURE.md`](../../ARCHITECTURE.md) §Mapping decisions.

use std::collections::BTreeMap;

use flui_foundation::Variable;
use flui_foundation::{Diagnosticable, DiagnosticsBuilder};

use flui_rendering::{
    constraints::{SliverConstraints, SliverGeometry, child_paint_offset},
    context::{PaintCx, SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverMultiBoxAdaptorParentData,
    protocol::ChildLayout,
    traits::RenderSliver,
};

use super::sliver_grid::{MAX_UNBOUNDED_WINDOW_CHILDREN, UNBOUNDED_SENTINEL_WINDOW};

/// How far a layout offset may miss an exact multiple of the item extent and
/// still count as that multiple, in pixels.
pub const PRECISION_ERROR_TOLERANCE: f64 = flui_foundation::EPSILON;

/// A sliver that places lazily built Box children one after another along the
/// scroll axis, each with the same main-axis extent.
///
/// The element tree owns the children: the sliver requests the indices its
/// window needs and retains the window as its band. Layout of a child is a
/// tight constraint on the main axis (`item_extent`) and the sliver's cross
/// axis; its position is `index × item_extent`.
#[derive(Debug, Clone)]
pub struct RenderSliverFixedExtentList {
    item_extent: f64,
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
    pub fn new(item_extent: f64, item_count: usize) -> Self {
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
    pub const fn item_extent(&self) -> f64 {
        self.item_extent
    }

    /// Sets the per-child main-axis extent.
    ///
    /// # Panics
    ///
    /// Panics if `item_extent` is not finite or not greater than zero.
    #[inline]
    pub fn set_item_extent(&mut self, item_extent: f64) -> flui_rendering::RenderUpdateImpact {
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

    /// The first child index whose extent reaches `scroll_offset`.
    ///
    /// An offset within `PRECISION_ERROR_TOLERANCE` of an item boundary
    /// counts as that boundary, so accumulated rounding never pulls in the
    /// child that ends exactly there.
    ///
    /// # Non-finite offsets
    ///
    /// `NaN` and `+∞` are rejected: the helper returns `0` and emits a
    /// `tracing::error!`. Rust's `f64 as usize` would otherwise saturate `+∞`
    /// to [`usize::MAX`], which must never become a retain-band or
    /// build-request edge. `−∞` is treated like any negative offset and
    /// selects index `0` without an error.
    #[must_use]
    pub fn min_child_index_for_scroll_offset(&self, scroll_offset: f64) -> usize {
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

    /// The last child index that starts before `scroll_offset`: the child that
    /// ends exactly at the offset is not included.
    ///
    /// # Non-finite offsets
    ///
    /// Same contract as [`Self::min_child_index_for_scroll_offset`]: `NaN` /
    /// `+∞` return `0` with a `tracing::error!`; `−∞` selects index `0`.
    #[must_use]
    pub fn max_child_index_for_scroll_offset(&self, scroll_offset: f64) -> usize {
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

    /// The layout offset of child `index`.
    #[inline]
    #[must_use]
    pub fn index_to_layout_offset(&self, index: usize) -> f64 {
        self.item_extent * index as f64
    }

    /// The scroll extent of `item_count` children.
    #[inline]
    #[must_use]
    pub fn compute_max_scroll_offset(&self, item_count: usize) -> f64 {
        self.item_extent * item_count as f64
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
fn float_to_index(index: f64) -> usize {
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
fn is_poison_scroll_offset(value: f64) -> bool {
    value.is_nan() || (value.is_infinite() && value.is_sign_positive())
}

/// Leading edge of the cache-extended scroll window, or `None` when the sum
/// is `NaN` or `+∞`.
///
/// `−∞` and negative finites clamp to `0.0` — the pre-guard `.max(0.0)`
/// behaviour — so a pathological negative-infinite leading edge still lays
/// out from the origin instead of evicting the window. `NaN` is rejected
/// before `.max(0.0)` because Rust's `f64::max` would otherwise return `0.0`
/// and hide the contract violation.
fn finite_leading_cache_edge(constraints: &SliverConstraints) -> Option<f64> {
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
            // implies and let the viewport clamp. A non-finite leading edge must never be
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
                    self.index_to_layout_offset(logical_index),
                    self.item_extent,
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
