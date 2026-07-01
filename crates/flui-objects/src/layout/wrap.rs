//! `RenderWrap` — lays children out in runs along the main axis,
//! wrapping to a new run when the next child would overflow.
//!
//! # Flutter equivalence
//!
//! Faithful port of Flutter's `RenderWrap`
//! (`packages/flutter/lib/src/rendering/wrap.dart`).
//!
//! The layout algorithm — run-building loop, `_RunMetrics`, main/cross-axis
//! sizing, and the two-pass positioning (`runAlignment` distributes free space
//! between runs; `alignment` distributes free space within a run;
//! `crossAxisAlignment` places each child within its run's cross extent) — is
//! ported 1:1 from Flutter.
//!
//! # RTL / vertical-direction caveat
//!
//! FLUI has not yet plumbed `TextDirection` into layout, so
//! `WrapAlignment::Start` / `End` and `WrapCrossAlignment::Start` / `End` are
//! always interpreted as LTR and TTB respectively. No axis flipping.

use flui_tree::Variable;
use flui_types::{Axis, Offset, Pixels, Size, geometry::px};

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext},
    parent_data::WrapParentData,
    traits::RenderBox,
};

// Re-export the canonical alignment types so `layout::*` / `flui_objects::*`
// exposes them without requiring callers to depend on `flui-types` directly.
// This `pub use` also serves as the module-level import for the code below.
pub use flui_types::layout::{WrapAlignment, WrapCrossAlignment};

/// Precision tolerance for run-overflow detection.
///
/// Mirrors Flutter's `precisionErrorTolerance` (1e-10 in Dart `double`),
/// adapted for f32.
const PRECISION_TOLERANCE: f32 = 1e-6;

// ── Layout helpers ────────────────────────────────────────────────────────────

/// Compute `(leading, between)` pixel spacings for `alignment`.
///
/// * `free_space` — unused extent (already `max(0, …)`-clamped by the caller),
/// * `item_spacing` — mandatory gap between adjacent items,
/// * `item_count` — number of items (children in the run, or runs).
///
/// Mirrors Flutter `WrapAlignment._distributeSpace` with
/// `flipped = false` (LTR / TTB — no RTL support in FLUI yet).
fn distribute_space(
    alignment: WrapAlignment,
    free_space: f32,
    item_spacing: f32,
    item_count: usize,
) -> (f32, f32) {
    match alignment {
        WrapAlignment::Start => (0.0, item_spacing),
        WrapAlignment::End => (free_space, item_spacing),
        WrapAlignment::Center => (free_space / 2.0, item_spacing),
        WrapAlignment::SpaceBetween => {
            if item_count < 2 {
                (0.0, item_spacing)
            } else {
                let between = free_space / (item_count - 1) as f32 + item_spacing;
                (0.0, between)
            }
        }
        WrapAlignment::SpaceAround => {
            let per_item = free_space / item_count as f32;
            (per_item / 2.0, per_item + item_spacing)
        }
        WrapAlignment::SpaceEvenly => {
            let per_gap = free_space / (item_count + 1) as f32;
            (per_gap, per_gap + item_spacing)
        }
    }
}

/// Cross-axis pixel offset for a child of `child_cross` pixels in a run whose
/// cross extent is `run_cross` pixels.
fn cross_axis_child_offset(alignment: WrapCrossAlignment, run_cross: f32, child_cross: f32) -> f32 {
    match alignment {
        WrapCrossAlignment::Start => 0.0,
        WrapCrossAlignment::End => run_cross - child_cross,
        WrapCrossAlignment::Center => (run_cross - child_cross) / 2.0,
    }
}

// ── Run descriptor ────────────────────────────────────────────────────────────

/// Metrics accumulated for one complete run during `perform_layout`.
///
/// Mirrors Flutter's `_RunMetrics`.
struct RunMetrics {
    /// Index of the first child in this run.
    first_child_index: usize,
    /// Number of children in this run.
    child_count: usize,
    /// Total main-axis extent of this run: sum of child main extents plus the
    /// spacing gaps between them.
    main_axis_extent: f32,
    /// Maximum cross-axis extent among all children in this run.
    cross_axis_extent: f32,
}

/// Intermediate result of the wrap sizing pass (Phases 1-2), shared between
/// `perform_layout` (which continues to Phase 3 positioning) and
/// `compute_dry_layout` (which only needs the container size).
struct WrapSizes {
    /// Constrained container size.
    container: Size,
    /// Run descriptors built in Phase 1; used by Phase 3 for positioning.
    runs: Vec<RunMetrics>,
    /// Per-child sizes in child order, parallel to the child index; used by
    /// Phase 3 to compute each child's main/cross extent and offset.
    child_sizes: Vec<Size>,
}

// ── RenderWrap ────────────────────────────────────────────────────────────────

/// Lays children out sequentially along `direction`,
/// starting a new run in the cross axis when the next child would overflow the
/// available main-axis extent.
///
/// Child paint offsets are stored in
/// [`WrapParentData::offset`](flui_rendering::parent_data::WrapParentData).
///
/// # Flutter parity
///
/// Faithful port of `RenderWrap.performLayout` and `_positionChildren`.
/// [`WrapAlignment::Start`] / [`WrapAlignment::End`] and
/// [`WrapCrossAlignment::Start`] / [`WrapCrossAlignment::End`] are always
/// LTR/TTB — FLUI does not yet support RTL text direction.
#[derive(Debug, Clone)]
pub struct RenderWrap {
    /// The axis along which children are arranged before wrapping.
    direction: Axis,
    /// Alignment of children within each run on the main axis.
    alignment: WrapAlignment,
    /// Minimum gap between adjacent children within a run.
    spacing: f32,
    /// Alignment of runs along the cross axis.
    run_alignment: WrapAlignment,
    /// Minimum gap between adjacent runs.
    run_spacing: f32,
    /// Alignment of each child within its run on the cross axis.
    cross_axis_alignment: WrapCrossAlignment,
    /// Cached child count from the most recent `perform_layout` call; used by
    /// `hit_test` which executes after layout.
    child_count: usize,
}

impl Default for RenderWrap {
    fn default() -> Self {
        Self {
            direction: Axis::Horizontal,
            alignment: WrapAlignment::Start,
            spacing: 0.0,
            run_alignment: WrapAlignment::Start,
            run_spacing: 0.0,
            cross_axis_alignment: WrapCrossAlignment::Start,
            child_count: 0,
        }
    }
}

impl RenderWrap {
    /// Creates a `RenderWrap` with Flutter's defaults: horizontal direction,
    /// all alignments [`Start`](WrapAlignment::Start), zero spacing.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder: sets the main-axis direction.
    #[must_use]
    pub fn with_direction(mut self, direction: Axis) -> Self {
        self.direction = direction;
        self
    }

    /// Builder: sets alignment of children within each run on the main axis.
    #[must_use]
    pub fn with_alignment(mut self, alignment: WrapAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Builder: sets the minimum gap between children within a run.
    #[must_use]
    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Builder: sets alignment of runs along the cross axis.
    #[must_use]
    pub fn with_run_alignment(mut self, run_alignment: WrapAlignment) -> Self {
        self.run_alignment = run_alignment;
        self
    }

    /// Builder: sets the minimum gap between adjacent runs.
    #[must_use]
    pub fn with_run_spacing(mut self, run_spacing: f32) -> Self {
        self.run_spacing = run_spacing;
        self
    }

    /// Builder: sets alignment of each child within its run on the cross axis.
    #[must_use]
    pub fn with_cross_axis_alignment(mut self, alignment: WrapCrossAlignment) -> Self {
        self.cross_axis_alignment = alignment;
        self
    }

    // ── Axis helpers ─────────────────────────────────────────────────────────

    fn main_extent(&self, size: Size) -> f32 {
        match self.direction {
            Axis::Horizontal => size.width.get(),
            Axis::Vertical => size.height.get(),
        }
    }

    fn cross_extent(&self, size: Size) -> f32 {
        match self.direction {
            Axis::Horizontal => size.height.get(),
            Axis::Vertical => size.width.get(),
        }
    }

    fn make_offset(&self, main: f32, cross: f32) -> Offset {
        match self.direction {
            Axis::Horizontal => Offset::new(px(main), px(cross)),
            Axis::Vertical => Offset::new(px(cross), px(main)),
        }
    }

    /// Maximum main-axis extent allowed by the incoming constraints.
    fn main_limit(&self, constraints: &BoxConstraints) -> f32 {
        match self.direction {
            Axis::Horizontal => constraints.max_width.get(),
            Axis::Vertical => constraints.max_height.get(),
        }
    }

    /// Child constraints: loose on the cross axis, bounded by the incoming
    /// max on the main axis. Mirrors Flutter's `_childConstraints`.
    fn child_constraints(&self, parent: &BoxConstraints) -> BoxConstraints {
        match self.direction {
            Axis::Horizontal => BoxConstraints::new(
                Pixels::ZERO,
                parent.max_width,
                Pixels::ZERO,
                Pixels::INFINITY,
            ),
            Axis::Vertical => BoxConstraints::new(
                Pixels::ZERO,
                Pixels::INFINITY,
                Pixels::ZERO,
                parent.max_height,
            ),
        }
    }

    /// Constrain `(main, cross)` extents against `constraints` and return a
    /// [`Size`] (swapping axes for vertical direction).
    fn constrain_size(&self, constraints: &BoxConstraints, main: f32, cross: f32) -> Size {
        match self.direction {
            Axis::Horizontal => Size::new(
                constraints.constrain_width(px(main)),
                constraints.constrain_height(px(cross)),
            ),
            Axis::Vertical => Size::new(
                constraints.constrain_width(px(cross)),
                constraints.constrain_height(px(main)),
            ),
        }
    }

    // ── Shared sizing core ────────────────────────────────────────────────────

    /// Phases 1-2 of wrap layout, shared between `perform_layout` and
    /// `compute_dry_layout`.
    ///
    /// **Phase 1 — run building**: iterates children, calling `measure` for
    /// each, and accumulates [`RunMetrics`] descriptors. A new run starts
    /// when the current run's main extent plus `spacing` plus the next
    /// child's main extent exceeds the main-axis limit by more than
    /// [`PRECISION_TOLERANCE`].
    ///
    /// **Phase 2 — container sizing**: constrains the union of run extents
    /// against `constraints` to produce the final [`Size`].
    ///
    /// `measure(i, constraints)` is either `ctx.layout_child` (real layout)
    /// or `ctx.child_dry_layout` (dry layout).
    fn compute_runs(
        &self,
        constraints: BoxConstraints,
        child_count: usize,
        mut measure: impl FnMut(usize, BoxConstraints) -> Size,
    ) -> WrapSizes {
        if child_count == 0 {
            return WrapSizes {
                container: constraints.smallest(),
                runs: vec![],
                child_sizes: vec![],
            };
        }

        let child_constraints = self.child_constraints(&constraints);
        let main_limit = self.main_limit(&constraints);

        // Phase 1 — lay out children + build run descriptors.
        let mut child_sizes: Vec<Size> = Vec::with_capacity(child_count);
        let mut runs: Vec<RunMetrics> = Vec::new();

        let mut run_first_child = 0_usize;
        let mut run_child_count = 0_usize;
        let mut run_main = 0.0_f32;
        let mut run_cross = 0.0_f32;

        for i in 0..child_count {
            let child_size = measure(i, child_constraints);
            child_sizes.push(child_size);

            let child_main = self.main_extent(child_size);
            let child_cross = self.cross_extent(child_size);

            // PORT: FLUI's shared core applies PRECISION_TOLERANCE in both
            // the real and dry layout paths. Flutter's _computeDryLayout
            // (wrap.dart:656-698) re-runs the run-break loop without
            // PRECISION_TOLERANCE, so a sub-PRECISION_TOLERANCE overflow
            // could diverge between Flutter's dry and real sizing. Using
            // PRECISION_TOLERANCE here makes FLUI's dry == real, which is
            // more correct for a floating-point layout engine.
            let needs_new_run = run_child_count > 0
                && run_main + child_main + self.spacing - main_limit > PRECISION_TOLERANCE;

            if needs_new_run {
                runs.push(RunMetrics {
                    first_child_index: run_first_child,
                    child_count: run_child_count,
                    main_axis_extent: run_main,
                    cross_axis_extent: run_cross,
                });
                run_first_child = i;
                run_child_count = 1;
                run_main = child_main;
                run_cross = child_cross;
            } else {
                if run_child_count > 0 {
                    run_main += self.spacing;
                }
                run_main += child_main;
                run_cross = run_cross.max(child_cross);
                run_child_count += 1;
            }
        }
        // Flush the last run (always non-empty because child_count > 0).
        runs.push(RunMetrics {
            first_child_index: run_first_child,
            child_count: run_child_count,
            main_axis_extent: run_main,
            cross_axis_extent: run_cross,
        });

        // Phase 2 — compute container size.
        let num_runs = runs.len();
        let total_run_cross_gap = self.run_spacing * num_runs.saturating_sub(1) as f32;
        let total_cross: f32 =
            runs.iter().map(|r| r.cross_axis_extent).sum::<f32>() + total_run_cross_gap;
        let max_run_main: f32 = runs
            .iter()
            .map(|r| r.main_axis_extent)
            .fold(0.0_f32, |a, b| a.max(b));

        let container = self.constrain_size(&constraints, max_run_main, total_cross);

        WrapSizes {
            container,
            runs,
            child_sizes,
        }
    }

    // ── Intrinsics simulation helper ──────────────────────────────────────────

    /// Simulate the run-building loop at `max_main` using each child's
    /// max-intrinsic main extent as a proxy size and return the resulting
    /// total cross-axis extent.
    ///
    /// This is an approximation (Flutter would call `getDryLayout`), but it
    /// gives reasonable intrinsic values and is the standard approach for
    /// `RenderWrap`-style widgets.
    fn simulate_wrap_cross(&self, max_main: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        let child_count = ctx.child_count();
        if child_count == 0 {
            return 0.0;
        }

        let mut total_cross = 0.0_f32;
        let mut run_main = 0.0_f32;
        let mut run_cross = 0.0_f32;
        let mut run_child_count = 0_usize;
        let mut num_runs = 0_usize;

        for i in 0..child_count {
            let (child_main, child_cross) = match self.direction {
                Axis::Horizontal => {
                    let w = ctx.child_max_intrinsic_width(i, f32::INFINITY);
                    let h = ctx.child_min_intrinsic_height(i, w);
                    (w, h)
                }
                Axis::Vertical => {
                    let h = ctx.child_max_intrinsic_height(i, f32::INFINITY);
                    let w = ctx.child_min_intrinsic_width(i, h);
                    (h, w)
                }
            };

            let needs_new_run = run_child_count > 0
                && run_main + child_main + self.spacing - max_main > PRECISION_TOLERANCE;

            if needs_new_run {
                if num_runs > 0 {
                    total_cross += self.run_spacing;
                }
                total_cross += run_cross;
                num_runs += 1;
                run_main = child_main;
                run_cross = child_cross;
                run_child_count = 1;
            } else {
                if run_child_count > 0 {
                    run_main += self.spacing;
                }
                run_main += child_main;
                run_cross = run_cross.max(child_cross);
                run_child_count += 1;
            }
        }

        // Flush the last run.
        if run_child_count > 0 {
            if num_runs > 0 {
                total_cross += self.run_spacing;
            }
            total_cross += run_cross;
        }

        total_cross
    }
}

// ── Diagnosticable ────────────────────────────────────────────────────────────

impl flui_foundation::Diagnosticable for RenderWrap {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add_enum("direction", self.direction);
        properties.add_enum("alignment", self.alignment);
        properties.add_default_double("spacing", self.spacing, 0.0, Some("px"));
        properties.add_enum("run_alignment", self.run_alignment);
        properties.add_default_double("run_spacing", self.run_spacing, 0.0, Some("px"));
        properties.add_enum("cross_axis_alignment", self.cross_axis_alignment);
    }
}

// ── RenderBox impl ────────────────────────────────────────────────────────────

impl RenderBox for RenderWrap {
    type Arity = Variable;
    type ParentData = WrapParentData;

    /// Three-phase layout matching Flutter's `RenderWrap.performLayout`.
    ///
    /// **Phases 1-2** are delegated to [`RenderWrap::compute_runs`] so that
    /// `compute_dry_layout` can reuse identical sizing logic.
    ///
    /// **Phase 3 — child positioning** (`_positionChildren`): distribute
    /// free cross-axis space among runs via `run_alignment`, then distribute
    /// free main-axis space within each run via `alignment`, then place each
    /// child with its `cross_axis_alignment` offset within the run.
    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Variable, WrapParentData>) -> Size {
        let constraints = *ctx.constraints();
        let child_count = ctx.child_count();
        self.child_count = child_count;

        let sized = self.compute_runs(constraints, child_count, |i, c| ctx.layout_child(i, c));

        // Zero-child fast path: compute_runs already returns constraints.smallest().
        if child_count == 0 {
            return sized.container;
        }

        let container = sized.container;
        let container_main = self.main_extent(container);
        let container_cross = self.cross_extent(container);
        let runs = sized.runs;
        let child_sizes = sized.child_sizes;

        // ── Phase 3: position children ────────────────────────────────────────

        let num_runs = runs.len();
        // Recompute total_cross from runs to drive free-cross distribution.
        let total_cross: f32 = runs.iter().map(|r| r.cross_axis_extent).sum::<f32>()
            + self.run_spacing * num_runs.saturating_sub(1) as f32;

        let free_cross = (container_cross - total_cross).max(0.0);
        let (mut cross_cursor, run_gap) =
            distribute_space(self.run_alignment, free_cross, self.run_spacing, num_runs);

        for run in &runs {
            let free_main = (container_main - run.main_axis_extent).max(0.0);
            let (mut main_cursor, child_gap) =
                distribute_space(self.alignment, free_main, self.spacing, run.child_count);

            let run_end = run.first_child_index + run.child_count;
            for (position_in_run, &child_size) in child_sizes[run.first_child_index..run_end]
                .iter()
                .enumerate()
            {
                let child_index = run.first_child_index + position_in_run;
                let child_main = self.main_extent(child_size);
                let child_cross = self.cross_extent(child_size);

                let child_cross_offset = cross_axis_child_offset(
                    self.cross_axis_alignment,
                    run.cross_axis_extent,
                    child_cross,
                );

                ctx.position_child(
                    child_index,
                    self.make_offset(main_cursor, cross_cursor + child_cross_offset),
                );

                main_cursor += child_main + child_gap;
            }

            cross_cursor += run.cross_axis_extent + run_gap;
        }

        container
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        // Delegates entirely to compute_runs: sizing (Phases 1-2) is
        // identical to perform_layout; Phase 3 positioning is irrelevant
        // for a dry query.
        self.compute_runs(constraints, ctx.child_count(), |i, c| {
            ctx.child_dry_layout(i, c)
        })
        .container
    }

    // ── Intrinsic dimensions ──────────────────────────────────────────────────

    fn compute_min_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        match self.direction {
            // Worst case: every child on its own row → max of child min widths.
            Axis::Horizontal => {
                let n = ctx.child_count();
                (0..n)
                    .map(|i| ctx.child_min_intrinsic_width(i, f32::INFINITY))
                    .fold(0.0_f32, f32::max)
            }
            // Vertical: simulate column wrapping at the given height.
            Axis::Vertical => self.simulate_wrap_cross(height, ctx),
        }
    }

    fn compute_max_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        match self.direction {
            // Best case: all children on one row → SUM of child max widths.
            // Flutter wrap.dart computeMaxIntrinsicWidth sums the children with
            // NO inter-child `spacing` term; adding it diverged from the oracle.
            Axis::Horizontal => (0..ctx.child_count())
                .map(|i| ctx.child_max_intrinsic_width(i, f32::INFINITY))
                .sum(),
            // Vertical: simulate column wrapping at the given height.
            Axis::Vertical => self.simulate_wrap_cross(height, ctx),
        }
    }

    fn compute_min_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        match self.direction {
            // Horizontal: simulate row wrapping at the given width.
            Axis::Horizontal => self.simulate_wrap_cross(width, ctx),
            // Worst case: every child in its own column → max of child min heights.
            Axis::Vertical => {
                let n = ctx.child_count();
                (0..n)
                    .map(|i| ctx.child_min_intrinsic_height(i, f32::INFINITY))
                    .fold(0.0_f32, f32::max)
            }
        }
    }

    fn compute_max_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        match self.direction {
            // Horizontal: simulate row wrapping at the given width.
            Axis::Horizontal => self.simulate_wrap_cross(width, ctx),
            // Best case: all children in one column → SUM of child max heights.
            // Flutter wrap.dart computeMaxIntrinsicHeight sums with NO `spacing`
            // term (matches the horizontal max-width path above).
            Axis::Vertical => (0..ctx.child_count())
                .map(|i| ctx.child_max_intrinsic_height(i, f32::INFINITY))
                .sum(),
        }
    }

    // ── Hit testing ───────────────────────────────────────────────────────────

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable, WrapParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }

        // Reverse paint order: the last child is painted on top.
        for i in (0..self.child_count).rev() {
            if ctx.hit_test_child_at_layout_offset(i) {
                return true;
            }
        }

        false
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_wrap_default_is_horizontal_start() {
        let wrap = RenderWrap::default();
        assert_eq!(wrap.direction, Axis::Horizontal);
        assert_eq!(wrap.alignment, WrapAlignment::Start);
        assert_eq!(wrap.cross_axis_alignment, WrapCrossAlignment::Start);
        assert_eq!(wrap.spacing, 0.0);
        assert_eq!(wrap.run_spacing, 0.0);
    }

    #[test]
    fn render_wrap_builders_round_trip() {
        let wrap = RenderWrap::new()
            .with_direction(Axis::Vertical)
            .with_spacing(8.0)
            .with_run_spacing(4.0)
            .with_alignment(WrapAlignment::Center)
            .with_run_alignment(WrapAlignment::SpaceBetween)
            .with_cross_axis_alignment(WrapCrossAlignment::End);

        assert_eq!(wrap.direction, Axis::Vertical);
        assert_eq!(wrap.spacing, 8.0);
        assert_eq!(wrap.run_spacing, 4.0);
        assert_eq!(wrap.alignment, WrapAlignment::Center);
        assert_eq!(wrap.run_alignment, WrapAlignment::SpaceBetween);
        assert_eq!(wrap.cross_axis_alignment, WrapCrossAlignment::End);
    }

    // ── distribute_space ──────────────────────────────────────────────────────

    #[test]
    fn distribute_space_start_zero_leading_spacing_gap() {
        let (leading, between) = distribute_space(WrapAlignment::Start, 100.0, 10.0, 3);
        assert_eq!(leading, 0.0);
        assert_eq!(between, 10.0);
    }

    #[test]
    fn distribute_space_end_full_leading_spacing_gap() {
        let (leading, between) = distribute_space(WrapAlignment::End, 100.0, 10.0, 3);
        assert_eq!(leading, 100.0);
        assert_eq!(between, 10.0);
    }

    #[test]
    fn distribute_space_center_half_leading() {
        let (leading, between) = distribute_space(WrapAlignment::Center, 100.0, 10.0, 3);
        assert!((leading - 50.0).abs() < 1e-5);
        assert_eq!(between, 10.0);
    }

    #[test]
    fn distribute_space_space_between_spreads_between_items() {
        // 3 items, 80 free, 10 spacing → between = 80/2 + 10 = 50
        let (leading, between) = distribute_space(WrapAlignment::SpaceBetween, 80.0, 10.0, 3);
        assert_eq!(leading, 0.0);
        assert!((between - 50.0).abs() < 1e-5);
    }

    #[test]
    fn distribute_space_space_between_single_item_falls_back_to_start() {
        let (leading, between) = distribute_space(WrapAlignment::SpaceBetween, 50.0, 10.0, 1);
        assert_eq!(leading, 0.0);
        assert_eq!(between, 10.0);
    }

    #[test]
    fn distribute_space_space_around_half_gap_at_edges() {
        // 3 items, 60 free, 0 spacing → per_item=20, leading=10, between=20
        let (leading, between) = distribute_space(WrapAlignment::SpaceAround, 60.0, 0.0, 3);
        assert!((leading - 10.0).abs() < 1e-5);
        assert!((between - 20.0).abs() < 1e-5);
    }

    #[test]
    fn distribute_space_space_evenly_equal_gaps_including_edges() {
        // 3 items, 80 free, 0 spacing → per_gap = 80/4 = 20
        let (leading, between) = distribute_space(WrapAlignment::SpaceEvenly, 80.0, 0.0, 3);
        assert!((leading - 20.0).abs() < 1e-5);
        assert!((between - 20.0).abs() < 1e-5);
    }

    // ── cross_axis_child_offset ───────────────────────────────────────────────

    #[test]
    fn cross_axis_child_offset_start_returns_zero() {
        assert_eq!(
            cross_axis_child_offset(WrapCrossAlignment::Start, 60.0, 20.0),
            0.0
        );
    }

    #[test]
    fn cross_axis_child_offset_end_aligns_to_run_bottom() {
        let offset = cross_axis_child_offset(WrapCrossAlignment::End, 60.0, 20.0);
        assert!((offset - 40.0).abs() < 1e-5);
    }

    #[test]
    fn cross_axis_child_offset_center_bisects_run_cross_extent() {
        let offset = cross_axis_child_offset(WrapCrossAlignment::Center, 60.0, 20.0);
        assert!((offset - 20.0).abs() < 1e-5);
    }
}
