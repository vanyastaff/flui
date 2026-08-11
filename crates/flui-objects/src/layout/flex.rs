//! RenderFlex - lays out children in a row or column.

use flui_tree::Variable;
use flui_types::typography::TextDirection;
use flui_types::{Offset, Pixels, Size, geometry::px};

use flui_rendering::{
    constraints::BoxConstraints,
    context::{
        BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
    },
    parent_data::{FlexFit, FlexParentData},
    traits::{RenderBox, TextBaseline},
};

/// Direction of the flex layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlexDirection {
    /// Children are laid out horizontally (Row).
    #[default]
    Horizontal,
    /// Children are laid out vertically (Column).
    Vertical,
}

/// How children are aligned along the main axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MainAxisAlignment {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// Children are placed at the start.
    #[default]
    Start,
    /// Children are placed at the end.
    End,
    /// Children are centered.
    Center,
    /// Space is distributed evenly between children.
    SpaceBetween,
    /// Space is distributed evenly around children.
    SpaceAround,
    /// Space is distributed evenly, including edges.
    SpaceEvenly,
}

/// Re-export of the canonical [`flui_types::layout::MainAxisSize`]:
/// `Max` (Flutter default) fills the incoming max main extent when it
/// is bounded - without it, alignment is dead under loose constraints
/// (the container shrink-wraps, so there is never free space to
/// distribute).
pub use flui_types::layout::MainAxisSize;

/// How children are aligned along the cross axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrossAxisAlignment {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// Children are aligned at the start.
    #[default]
    Start,
    /// Children are aligned at the end.
    End,
    /// Children are centered.
    Center,
    /// Children are stretched to fill the cross axis.
    Stretch,
    /// Align children by their text baselines (horizontal flex only).
    Baseline,
}

/// Intermediate result of the flex sizing pass, shared between
/// `perform_layout` (which continues to positioning) and
/// `compute_dry_layout` / `compute_dry_baseline` (which only need size /
/// child baselines respectively).
struct FlexSizes {
    /// Constrained container size.
    size: Size,
    /// Per-child sized extents, indexed `0..child_count`.
    /// `None` means the slot was not yet laid out (should not occur after
    /// `compute_sizes` completes normally).
    child_sizes: Vec<Option<Size>>,
    /// Sum of every child's main-axis size plus all inter-child spacing.
    /// Needed by `perform_layout` to compute free-space distribution.
    total_main: Pixels,
    /// The `BoxConstraints` that was passed to `measure` for each child,
    /// indexed `0..child_count`.  Required by `compute_dry_baseline` to
    /// query `ctx.child_dry_baseline(i, child_constraints[i], …)` using the
    /// same constraint that was used during the sizing pass.
    child_constraints: Vec<BoxConstraints>,
    /// Each child's distance to `self.text_baseline`, as reported right after
    /// it was measured — `None` for a child with no such baseline, and
    /// all-`None` unless the flex is baseline-aligned.
    ///
    /// Sizing needs these (a baseline-aligned child contributes ascent +
    /// descent to the cross extent, not its raw cross size), and positioning
    /// needs the same values; they are collected once here so the two agree by
    /// construction and each child is queried exactly once.
    alignment_baselines: Vec<Option<f32>>,
}

/// A render object that lays out children in a flex layout (row or column).
///
/// This is a simplified Flex implementation without flex factors.
/// Children are laid out sequentially and positioned according to alignment.
///
/// # Example
///
/// ```ignore
/// // Horizontal row
/// let row = RenderFlex::row();
///
/// // Vertical column with center alignment
/// let column = RenderFlex::column()
///     .with_main_axis_alignment(MainAxisAlignment::Center)
///     .with_cross_axis_alignment(CrossAxisAlignment::Center);
/// ```
#[derive(Debug, Clone)]
pub struct RenderFlex {
    /// Direction of layout.
    direction: FlexDirection,
    /// Main axis alignment.
    main_axis_alignment: MainAxisAlignment,
    /// How much main-axis space the container claims.
    main_axis_size: MainAxisSize,
    /// Cross axis alignment.
    cross_axis_alignment: CrossAxisAlignment,
    /// Resolves which physical edge `Start`/`End` mean, and which order
    /// children are laid out in.
    ///
    /// A horizontal flex (`Row`) consults this for its **main** axis: under
    /// `Rtl` children are laid out right-to-left (`RenderFlex._flipMainAxis`,
    /// `rendering/flex.dart`). A vertical flex (`Column`) consults this for
    /// its **cross** axis instead (`_flipCrossAxis`) — its main axis is
    /// governed by `VerticalDirection`, which FLUI does not yet model, so a
    /// `Column`'s main axis never flips. Defaults to `Ltr`, matching every
    /// other FLUI render object that has no ambient `Directionality` to fall
    /// back on.
    text_direction: TextDirection,
    /// Baseline kind used when [`CrossAxisAlignment::Baseline`] is selected.
    text_baseline: TextBaseline,
    /// Spacing between children.
    spacing: f32,
    /// Number of children (tracked for hit testing).
    child_count: usize,
    /// Baseline eagerly recorded during `perform_layout` for both
    /// [`TextBaseline`] kinds, served by `compute_distance_to_actual_baseline`.
    ///
    /// Index 0 = `Alphabetic`, index 1 = `Ideographic` (see [`baseline_kind_index`]).
    ///
    /// - Horizontal flex: minimum of `child_baseline + child_offset.dy` over
    ///   all children (oracle: `box.dart:3336-3348` highest baseline).
    /// - Vertical flex: first child in list order that has a baseline
    ///   (oracle: `box.dart:3318-3330` first baseline).
    ///
    /// Mirrors the eager-record convention of `AligningShiftedBox::child_baselines`
    /// (`shifted_box.rs:138-141`).  Reset to `[None; 2]` on layout when no
    /// children are present or none report a baseline.
    reported_baselines: [Option<f32>; 2],
}

impl Default for RenderFlex {
    fn default() -> Self {
        Self {
            direction: FlexDirection::Horizontal,
            main_axis_alignment: MainAxisAlignment::Start,
            main_axis_size: MainAxisSize::Max,
            cross_axis_alignment: CrossAxisAlignment::Start,
            text_direction: TextDirection::Ltr,
            text_baseline: TextBaseline::Alphabetic,
            spacing: 0.0,
            child_count: 0,
            reported_baselines: [None; 2],
        }
    }
}

impl RenderFlex {
    /// Creates a new flex with default settings (horizontal).
    pub fn new() -> Self {
        Self::default()
    }

    /// Updates axis and direction configuration without replacing layout
    /// caches.
    pub fn update_directions(
        &mut self,
        direction: FlexDirection,
        text_direction: TextDirection,
        text_baseline: TextBaseline,
    ) -> flui_rendering::RenderUpdateImpact {
        let changed = self.direction != direction
            || self.text_direction != text_direction
            || self.text_baseline != text_baseline;
        self.direction = direction;
        self.text_direction = text_direction;
        self.text_baseline = text_baseline;
        if changed {
            flui_rendering::RenderUpdateImpact::LAYOUT
        } else {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }

    /// Updates alignment, sizing, and spacing without replacing layout caches.
    pub fn update_layout_configuration(
        &mut self,
        main_axis_alignment: MainAxisAlignment,
        main_axis_size: MainAxisSize,
        cross_axis_alignment: CrossAxisAlignment,
        spacing: f32,
    ) -> flui_rendering::RenderUpdateImpact {
        let changed = self.main_axis_alignment != main_axis_alignment
            || self.main_axis_size != main_axis_size
            || self.cross_axis_alignment != cross_axis_alignment
            || self.spacing != spacing;
        self.main_axis_alignment = main_axis_alignment;
        self.main_axis_size = main_axis_size;
        self.cross_axis_alignment = cross_axis_alignment;
        self.spacing = spacing;
        if changed {
            flui_rendering::RenderUpdateImpact::LAYOUT
        } else {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }

    /// Creates a horizontal flex (Row).
    pub fn row() -> Self {
        Self {
            direction: FlexDirection::Horizontal,
            ..Default::default()
        }
    }

    /// Creates a vertical flex (Column).
    pub fn column() -> Self {
        Self {
            direction: FlexDirection::Vertical,
            ..Default::default()
        }
    }

    /// Sets the main axis alignment.
    pub fn with_main_axis_alignment(mut self, alignment: MainAxisAlignment) -> Self {
        self.main_axis_alignment = alignment;
        self
    }

    /// Builder: set the main-axis size policy.
    pub fn with_main_axis_size(mut self, size: MainAxisSize) -> Self {
        self.main_axis_size = size;
        self
    }

    /// Sets the cross axis alignment.
    pub fn with_cross_axis_alignment(mut self, alignment: CrossAxisAlignment) -> Self {
        self.cross_axis_alignment = alignment;
        self
    }

    /// Sets the text baseline used for [`CrossAxisAlignment::Baseline`].
    pub fn with_text_baseline(mut self, baseline: TextBaseline) -> Self {
        self.text_baseline = baseline;
        self
    }

    /// Sets the ambient text direction that resolves `Start`/`End` and child
    /// order: a horizontal flex (`Row`) flips its main axis under `Rtl`; a
    /// vertical flex (`Column`) flips its cross axis instead. Defaults to
    /// `Ltr`.
    pub fn with_text_direction(mut self, text_direction: TextDirection) -> Self {
        self.text_direction = text_direction;
        self
    }

    /// Sets the spacing between children.
    ///
    /// Debug-asserts `spacing >= 0.0` — Flutter's `RenderFlex` asserts the
    /// same (`rendering/flex.dart`, tag `3.44.0`); a NaN also fails the
    /// comparison. Negative spacing would subtract main-axis extent and
    /// overlap children.
    pub fn with_spacing(mut self, spacing: f32) -> Self {
        debug_assert!(
            spacing >= 0.0,
            "flex spacing must be non-negative and not NaN, got {spacing}"
        );
        self.spacing = spacing;
        self
    }

    /// Returns the direction.
    pub fn direction(&self) -> FlexDirection {
        self.direction
    }

    /// Returns true if this is a horizontal layout.
    pub fn is_horizontal(&self) -> bool {
        self.direction == FlexDirection::Horizontal
    }

    /// Returns true if this is a vertical layout.
    pub fn is_vertical(&self) -> bool {
        self.direction == FlexDirection::Vertical
    }

    /// Returns the ambient text direction used to resolve `Start`/`End`.
    pub fn text_direction(&self) -> TextDirection {
        self.text_direction
    }

    /// Whether the main axis is laid out and iterated in reverse.
    ///
    /// Mirrors Flutter `RenderFlex._flipMainAxis` (`rendering/flex.dart`):
    /// only a horizontal flex (`Row`) consults `text_direction` here — a
    /// vertical flex's main axis is governed by `VerticalDirection`, which
    /// FLUI does not model, so it never flips.
    fn flip_main_axis(&self) -> bool {
        self.direction == FlexDirection::Horizontal && self.text_direction.is_rtl()
    }

    /// Whether the cross axis's `Start`/`End` offsets are swapped.
    ///
    /// Mirrors Flutter `RenderFlex._flipCrossAxis`: only a vertical flex
    /// (`Column`) consults `text_direction` here — a horizontal flex's cross
    /// axis is governed by `VerticalDirection` instead, so a `Row` never
    /// flips its cross axis from `text_direction` alone.
    fn flip_cross_axis(&self) -> bool {
        self.direction == FlexDirection::Vertical && self.text_direction.is_rtl()
    }

    /// Extracts main axis extent from a size.
    fn main_size(&self, size: Size) -> Pixels {
        match self.direction {
            FlexDirection::Horizontal => size.width,
            FlexDirection::Vertical => size.height,
        }
    }

    /// Extracts cross axis extent from a size.
    fn cross_size(&self, size: Size) -> Pixels {
        match self.direction {
            FlexDirection::Horizontal => size.height,
            FlexDirection::Vertical => size.width,
        }
    }

    /// Creates an offset from main and cross values.
    fn offset(&self, main: Pixels, cross: Pixels) -> Offset {
        match self.direction {
            FlexDirection::Horizontal => Offset::new(main, cross),
            FlexDirection::Vertical => Offset::new(cross, main),
        }
    }

    /// Creates a size from main and cross values.
    fn size_from_main_cross(&self, main: Pixels, cross: Pixels) -> Size {
        match self.direction {
            FlexDirection::Horizontal => Size::new(main, cross),
            FlexDirection::Vertical => Size::new(cross, main),
        }
    }

    /// Flutter `RenderFlex._getIntrinsicSize` main-axis branch
    /// (`flex.dart:716-733`): flex children contribute via the largest
    /// per-flex-unit size; inflexible children sum directly.
    fn fold_main_axis_intrinsics(
        &self,
        ctx: &mut BoxIntrinsicsCtx<'_>,
        cross_extent: f32,
        mut child_size: impl FnMut(&mut BoxIntrinsicsCtx<'_>, usize, f32) -> f32,
    ) -> f32 {
        let child_count = ctx.child_count();
        if child_count == 0 {
            return 0.0;
        }

        let spacing_total = self.spacing * (child_count.saturating_sub(1)) as f32;
        let mut total_flex = 0i32;
        let mut inflexible_space = spacing_total;
        let mut max_flex_fraction = 0.0f32;

        for i in 0..child_count {
            let flex = ctx.child_flex(i);
            total_flex += flex;
            if flex > 0 {
                let size = child_size(ctx, i, cross_extent);
                max_flex_fraction = max_flex_fraction.max(size / flex as f32);
            } else {
                inflexible_space += child_size(ctx, i, cross_extent);
            }
        }

        max_flex_fraction * total_flex as f32 + inflexible_space
    }

    /// Folds child intrinsics along the cross axis (max of child cross sizes).
    fn intrinsic_cross(
        ctx: &mut BoxIntrinsicsCtx<'_>,
        main_extent: f32,
        mut child_cross: impl FnMut(&mut BoxIntrinsicsCtx<'_>, usize, f32) -> f32,
    ) -> f32 {
        let child_count = ctx.child_count();
        if child_count == 0 {
            return 0.0;
        }
        let mut max = 0.0f32;
        for i in 0..child_count {
            max = max.max(child_cross(ctx, i, main_extent));
        }
        max
    }

    /// Whether children are positioned by their baselines — only ever true for
    /// a horizontal flex, since a column has no shared baseline to align to.
    ///
    /// Mirrors Flutter's `RenderFlex._isBaselineAligned`, which gates both the
    /// baseline queries during sizing and the cross-axis offset formula.
    fn is_baseline_aligned(&self) -> bool {
        self.cross_axis_alignment == CrossAxisAlignment::Baseline
            && self.direction == FlexDirection::Horizontal
    }

    /// Core two-pass flex sizing algorithm shared by `perform_layout` and
    /// `compute_dry_layout`.
    ///
    /// Takes the incoming `constraints`, per-child `flex_factors` and
    /// `flex_fits` (length == child_count), and a `measure` callback that
    /// returns the size a child reports for given `BoxConstraints` **and** its
    /// distance to `self.text_baseline` at that size.  Does NOT position
    /// children — the caller is responsible for that.
    ///
    /// The callback returns the baseline alongside the size because a
    /// baseline-aligned child's contribution to the cross extent is its ascent
    /// and descent, not its raw cross size, and the reference queries the
    /// baseline immediately after laying the child out. Callers that are not
    /// baseline-aligned return `None` — mirroring the reference, which nulls
    /// out the baseline kind unless [`Self::is_baseline_aligned`].
    ///
    /// Mirrors Flutter `RenderFlex._computeSizes`.
    fn compute_sizes(
        &self,
        constraints: BoxConstraints,
        flex_factors: &[Option<i32>],
        flex_fits: &[FlexFit],
        mut measure: impl FnMut(usize, BoxConstraints) -> (Size, Option<f32>),
    ) -> FlexSizes {
        let child_count = flex_factors.len();

        // ── Zero-child fast path ──────────────────────────────────────────────
        // Flutter flex.dart: `idealMainSize = maxMainSize` when MainAxisSize::Max
        // and the main axis is bounded; otherwise collapse both axes.
        if child_count == 0 {
            let max_main = match self.direction {
                FlexDirection::Horizontal => constraints.max_width,
                FlexDirection::Vertical => constraints.max_height,
            };
            let ideal_main = if self.main_axis_size == MainAxisSize::Max && max_main.is_finite() {
                max_main
            } else {
                Pixels::ZERO
            };
            let size = match self.direction {
                FlexDirection::Horizontal => Size::new(ideal_main, Pixels::ZERO),
                FlexDirection::Vertical => Size::new(Pixels::ZERO, ideal_main),
            };
            return FlexSizes {
                size: constraints.constrain(size),
                child_sizes: Vec::new(),
                total_main: Pixels::ZERO,
                child_constraints: Vec::new(),
                alignment_baselines: Vec::new(),
            };
        }

        // ── Cross-axis policy ─────────────────────────────────────────────────
        // Flutter flex.dart:889-898: Stretch tightens the cross axis to max when
        // it is bounded; all other alignments loosen the cross.
        let stretch = self.cross_axis_alignment == CrossAxisAlignment::Stretch;
        let cross_max = match self.direction {
            FlexDirection::Horizontal => constraints.max_height,
            FlexDirection::Vertical => constraints.max_width,
        };
        let (child_cross_min, child_cross_max) = if stretch && cross_max.is_finite() {
            (cross_max, cross_max)
        } else {
            (Pixels::ZERO, cross_max)
        };

        // Non-flex children get an unbounded main axis.
        let non_flex_constraints = match self.direction {
            FlexDirection::Horizontal => BoxConstraints::new(
                Pixels::ZERO,
                Pixels::INFINITY,
                child_cross_min,
                child_cross_max,
            ),
            FlexDirection::Vertical => BoxConstraints::new(
                child_cross_min,
                child_cross_max,
                Pixels::ZERO,
                Pixels::INFINITY,
            ),
        };

        // Per-child constraint tracking: defaults to non_flex_constraints; flex
        // children under a bounded main axis get their allocated constraints below.
        // Used by `compute_dry_baseline` to query dry child baselines with the
        // exact constraints that were used during the sizing pass.
        let mut child_constraints: Vec<BoxConstraints> = vec![non_flex_constraints; child_count];

        // ── Pass 1: size inflexible children ─────────────────────────────────
        let total_flex: i32 = flex_factors.iter().filter_map(|&f| f).sum();
        let mut child_sizes: Vec<Option<Size>> = vec![None; child_count];
        let mut inflexible_main = Pixels::ZERO;
        let mut max_cross = Pixels::ZERO;
        let mut alignment_baselines: Vec<Option<f32>> = vec![None; child_count];
        // Running (max ascent, max descent) over the children that reported a
        // baseline. `None` while no child has; folded into the cross extent
        // once every child is sized.
        let mut ascent_descent: Option<(f32, f32)> = None;

        // Records a measured child: its raw cross size feeds `max_cross`, and,
        // if it reported a baseline, its ascent/descent feed the running pair.
        let accumulate_cross =
            |child_size: Size,
             baseline: Option<f32>,
             max_cross: &mut Pixels,
             ascent_descent: &mut Option<(f32, f32)>| {
                *max_cross = (*max_cross).max(self.cross_size(child_size));
                if let Some(ascent) = baseline {
                    let descent = self.cross_size(child_size).get() - ascent;
                    *ascent_descent = Some(match *ascent_descent {
                        None => (ascent, descent),
                        Some((a, d)) => (a.max(ascent), d.max(descent)),
                    });
                }
            };

        for i in 0..child_count {
            if flex_factors[i].is_none() || flex_factors[i] == Some(0) {
                let (child_size, baseline) = measure(i, non_flex_constraints);
                child_sizes[i] = Some(child_size);
                alignment_baselines[i] = baseline;
                inflexible_main += self.main_size(child_size);
                accumulate_cross(child_size, baseline, &mut max_cross, &mut ascent_descent);
            }
        }

        let total_spacing = px(self.spacing * (child_count - 1) as f32);
        inflexible_main += total_spacing;

        // Flutter flex.dart:1232 — flex factors are meaningful only when the
        // main axis is bounded. Under an unbounded main, flex children are
        // treated as inflexible (tight or zero allocation would collapse them).
        let max_main = match self.direction {
            FlexDirection::Horizontal => constraints.max_width,
            FlexDirection::Vertical => constraints.max_height,
        };
        let can_flex = max_main.is_finite();

        if !can_flex && total_flex > 0 {
            for i in 0..child_count {
                if matches!(flex_factors[i], Some(f) if f > 0) {
                    let (child_size, baseline) = measure(i, non_flex_constraints);
                    child_sizes[i] = Some(child_size);
                    alignment_baselines[i] = baseline;
                    inflexible_main += self.main_size(child_size);
                    accumulate_cross(child_size, baseline, &mut max_cross, &mut ascent_descent);
                }
            }
        }

        let remaining = if can_flex {
            (max_main - inflexible_main).max(Pixels::ZERO)
        } else {
            Pixels::ZERO
        };

        // ── Pass 2: size flex children ────────────────────────────────────────
        if can_flex && total_flex > 0 {
            for i in 0..child_count {
                if let Some(flex) = flex_factors[i]
                    && flex > 0
                {
                    let allocated = remaining * (flex as f32 / total_flex as f32);
                    let allocated_constraints = match (self.direction, flex_fits[i]) {
                        (FlexDirection::Horizontal, FlexFit::Tight) => BoxConstraints::new(
                            allocated,
                            allocated,
                            child_cross_min,
                            child_cross_max,
                        ),
                        (FlexDirection::Horizontal, FlexFit::Loose) => BoxConstraints::new(
                            Pixels::ZERO,
                            allocated,
                            child_cross_min,
                            child_cross_max,
                        ),
                        (FlexDirection::Vertical, FlexFit::Tight) => BoxConstraints::new(
                            child_cross_min,
                            child_cross_max,
                            allocated,
                            allocated,
                        ),
                        (FlexDirection::Vertical, FlexFit::Loose) => BoxConstraints::new(
                            child_cross_min,
                            child_cross_max,
                            Pixels::ZERO,
                            allocated,
                        ),
                    };
                    child_constraints[i] = allocated_constraints;
                    let (child_size, baseline) = measure(i, allocated_constraints);
                    child_sizes[i] = Some(child_size);
                    alignment_baselines[i] = baseline;
                    accumulate_cross(child_size, baseline, &mut max_cross, &mut ascent_descent);
                }
            }
        }

        // Baseline-aligned children stack ascent-above-descent rather than
        // sitting flush at the cross start, so the extent they need is the
        // tallest ascent plus the deepest descent — which can exceed every
        // individual child's own cross size. Children with no baseline (and
        // every child when the flex is not baseline-aligned) still contribute
        // their raw cross size through `max_cross`, so a tall no-baseline child
        // continues to win. Mirrors Flutter's `_AscentDescent` accumulation
        // folded into `accumulatedSize` in `RenderFlex._computeSizes`.
        if let Some((ascent, descent)) = ascent_descent {
            max_cross = max_cross.max(px(ascent + descent));
        }

        // ── Container size ────────────────────────────────────────────────────
        let mut total_main = Pixels::ZERO;
        for s in child_sizes.iter().flatten() {
            total_main += self.main_size(*s);
        }
        total_main += total_spacing;

        // Flutter flex.dart:1298 — MainAxisSize::Max claims the full bounded
        // main extent; Min shrink-wraps.
        let ideal_main = if can_flex && self.main_axis_size == MainAxisSize::Max {
            max_main
        } else {
            total_main
        };
        let main_extent = match self.direction {
            FlexDirection::Horizontal => constraints.constrain_width(ideal_main),
            FlexDirection::Vertical => constraints.constrain_height(ideal_main),
        };
        let cross_extent = match self.direction {
            FlexDirection::Horizontal => constraints.constrain_height(max_cross),
            FlexDirection::Vertical => constraints.constrain_width(max_cross),
        };

        FlexSizes {
            size: self.size_from_main_cross(main_extent, cross_extent),
            child_sizes,
            total_main,
            child_constraints,
            alignment_baselines,
        }
    }

    /// Compute each child's absolute `Offset` within the flex box.
    ///
    /// Takes `flex_sizes` from a prior sizing pass and per-child
    /// `alignment_baselines` (the `self.text_baseline` distance for each child,
    /// used only when [`CrossAxisAlignment::Baseline`] is active on a horizontal
    /// flex; pass `&[None; n]` or an all-`None` slice otherwise).
    ///
    /// Extracted from `perform_layout`'s offset loop so that `perform_layout`
    /// (live baselines) and `compute_dry_baseline` (dry baselines) share one
    /// positioning home — one fact, one place.
    ///
    /// Mirrors Flutter `RenderFlex.performLayout` offset loop (`flex.dart:1339+`).
    /// The returned `Vec` is parallel to `flex_sizes.child_sizes`.
    fn compute_child_offsets(
        &self,
        flex_sizes: &FlexSizes,
        alignment_baselines: &[Option<f32>],
    ) -> Vec<Offset> {
        let child_count = flex_sizes.child_sizes.len();
        if child_count == 0 {
            return Vec::new();
        }

        let main_extent = self.main_size(flex_sizes.size);
        let cross_extent = self.cross_size(flex_sizes.size);
        // Flutter flex.dart:1339 — clamp free_space to zero so overflowing rows
        // do not shift children by negative offsets under End/Center/Space*.
        let free_space = (main_extent - flex_sizes.total_main).max(Pixels::ZERO);

        // Flutter flex.dart: `MainAxisAlignment._distributeSpace` derives `end`'s
        // leading space from `start`'s formula with the flip inverted, which is
        // equivalent to swapping Start/End up front and reusing one formula
        // table below. Center/SpaceBetween/SpaceAround/SpaceEvenly are already
        // symmetric, so flipping never changes their case.
        let flip_main_axis = self.flip_main_axis();
        let effective_main_axis_alignment = match (self.main_axis_alignment, flip_main_axis) {
            (MainAxisAlignment::Start, true) => MainAxisAlignment::End,
            (MainAxisAlignment::End, true) => MainAxisAlignment::Start,
            (alignment, _) => alignment,
        };

        let (leading_space, between_space) = match effective_main_axis_alignment {
            MainAxisAlignment::Start => (Pixels::ZERO, Pixels::ZERO),
            MainAxisAlignment::End => (free_space, Pixels::ZERO),
            MainAxisAlignment::Center => (free_space / 2.0, Pixels::ZERO),
            MainAxisAlignment::SpaceBetween => {
                if child_count > 1 {
                    (Pixels::ZERO, free_space / (child_count - 1) as f32)
                } else {
                    (Pixels::ZERO, Pixels::ZERO)
                }
            }
            MainAxisAlignment::SpaceAround => {
                let space = free_space / child_count as f32;
                (space / 2.0, space)
            }
            MainAxisAlignment::SpaceEvenly => {
                let space = free_space / (child_count + 1) as f32;
                (space, space)
            }
        };

        // Same Start/End swap for the cross axis (`_flipCrossAxis`); unlike
        // the main axis this never reorders children, it only changes which
        // physical edge each child's own offset is measured from.
        let effective_cross_axis_alignment =
            match (self.cross_axis_alignment, self.flip_cross_axis()) {
                (CrossAxisAlignment::Start, true) => CrossAxisAlignment::End,
                (CrossAxisAlignment::End, true) => CrossAxisAlignment::Start,
                (alignment, _) => alignment,
            };

        // Flutter flex.dart: baseline cross-axis alignment applies to rows only.
        // Find the maximum alignment-baseline distance — all children shift down
        // so their baselines land on the same horizontal level.
        let max_alignment_baseline = if self.direction == FlexDirection::Horizontal
            && self.cross_axis_alignment == CrossAxisAlignment::Baseline
        {
            alignment_baselines
                .iter()
                .filter_map(|&b| b)
                .reduce(f32::max)
        } else {
            None
        };

        // Flutter flex.dart's offset loop walks from `topLeftChild` (last
        // child, iterating `childBefore`) when `flipMainAxis`, instead of the
        // usual first-child-forward order — the visual placement order
        // reverses under RTL even though each child's own offset is still
        // measured in the same local (always-increasing-rightward) coordinate
        // space. `offsets` is written by real child index so the caller sees
        // one `Offset` per child regardless of visiting order.
        // `step` counts placement positions along the main axis; `i` is the
        // real child index occupying that position — the same sequence for
        // both orders, derived rather than boxed behind `dyn Iterator` so
        // this layout path stays allocation- and dispatch-free.
        let mut offsets = vec![Offset::ZERO; child_count];

        let mut main_offset = leading_space;
        for step in 0..child_count {
            let i = if flip_main_axis {
                child_count - 1 - step
            } else {
                step
            };
            let child_size = flex_sizes.child_sizes[i].unwrap_or(Size::ZERO);

            let cross_offset = match effective_cross_axis_alignment {
                CrossAxisAlignment::Start | CrossAxisAlignment::Stretch => Pixels::ZERO,
                CrossAxisAlignment::End => cross_extent - self.cross_size(child_size),
                CrossAxisAlignment::Center => (cross_extent - self.cross_size(child_size)) / 2.0,
                CrossAxisAlignment::Baseline => {
                    max_alignment_baseline.map_or(Pixels::ZERO, |max_dist| {
                        alignment_baselines[i].map_or(Pixels::ZERO, |child_dist| {
                            Pixels::new(max_dist - child_dist)
                        })
                    })
                }
            };

            offsets[i] = self.offset(main_offset, cross_offset);
            main_offset += self.main_size(child_size) + px(self.spacing) + between_space;
        }

        offsets
    }
}

/// Maps a [`TextBaseline`] kind to an index into `[Option<f32>; 2]` arrays
/// such as [`RenderFlex::reported_baselines`].
///
/// Mirrors the convention in `AligningShiftedBox::child_baselines`
/// (`shifted_box.rs:155-158`): index 0 = Alphabetic, 1 = Ideographic.
fn baseline_kind_index(baseline: TextBaseline) -> usize {
    match baseline {
        TextBaseline::Alphabetic => 0,
        TextBaseline::Ideographic => 1,
    }
}

impl flui_foundation::Diagnosticable for RenderFlex {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add_enum("direction", self.direction);
        properties.add_enum("main_axis_alignment", self.main_axis_alignment);
        properties.add_default_enum("main_axis_size", self.main_axis_size, MainAxisSize::Max);
        properties.add_enum("cross_axis_alignment", self.cross_axis_alignment);
        if self.cross_axis_alignment == CrossAxisAlignment::Baseline {
            properties.add_enum("text_baseline", self.text_baseline);
        }
        properties.add_default_enum("text_direction", self.text_direction, TextDirection::Ltr);
        properties.add_default_double("spacing", self.spacing, 0.0, Some("px"));
    }
}
impl RenderBox for RenderFlex {
    type Arity = Variable;
    type ParentData = FlexParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Variable, FlexParentData>) -> Size {
        let constraints = *ctx.constraints();
        let child_count = ctx.child_count();
        self.child_count = child_count;

        // Collect flex factors and fits from each child's parent data.
        let mut flex_factors: Vec<Option<i32>> = Vec::with_capacity(child_count);
        let mut flex_fits: Vec<FlexFit> = Vec::with_capacity(child_count);
        for i in 0..child_count {
            let (flex, fit) = ctx
                .child_parent_data(i)
                .map_or((None, FlexFit::Loose), |pd| (pd.flex, pd.fit));
            flex_factors.push(flex);
            flex_fits.push(fit);
        }

        // Only queried when the flex is baseline-aligned; all-`None` otherwise
        // (and then ignored by `compute_child_offsets`).
        let baseline_aligned = self.is_baseline_aligned();
        let text_baseline = self.text_baseline;
        let flex_sizes = self.compute_sizes(constraints, &flex_factors, &flex_fits, |i, c| {
            let child_size = ctx.layout_child(i, c);
            let baseline = baseline_aligned
                .then(|| ctx.child_distance_to_actual_baseline(i, text_baseline))
                .flatten();
            (child_size, baseline)
        });

        // Zero-child case: no positioning loop needed.
        if child_count == 0 {
            self.reported_baselines = [None; 2];
            return flex_sizes.size;
        }

        // ── Positioning pass ─────────────────────────────────────────────────
        let alignment_baselines = &flex_sizes.alignment_baselines;
        let child_offsets = self.compute_child_offsets(&flex_sizes, alignment_baselines);

        // Reset recorded baselines; they are populated in the loop below.
        self.reported_baselines = [None; 2];

        for (i, &child_offset) in child_offsets.iter().enumerate() {
            ctx.position_child(i, child_offset);

            // Record the flex's own baseline for both kinds.
            // Horizontal → highest = minimum (oracle box.dart:3336-3348).
            // Vertical   → first child in list order (oracle box.dart:3318-3330).
            // The queried kind differs from the alignment kind in the general case,
            // so both are queried; when kind == self.text_baseline and the
            // Baseline alignment was active, reuse the pre-queried value instead
            // of issuing a redundant context call.
            let offset_dy = child_offset.dy.get();
            for kind in [TextBaseline::Alphabetic, TextBaseline::Ideographic] {
                let kind_index = baseline_kind_index(kind);
                let child_baseline = if kind == self.text_baseline
                    && self.cross_axis_alignment == CrossAxisAlignment::Baseline
                    && self.direction == FlexDirection::Horizontal
                {
                    alignment_baselines[i]
                } else {
                    ctx.child_distance_to_actual_baseline(i, kind)
                };

                if let Some(baseline_distance) = child_baseline {
                    let candidate = baseline_distance + offset_dy;
                    let slot = &mut self.reported_baselines[kind_index];
                    match self.direction {
                        FlexDirection::Horizontal => {
                            *slot = Some(slot.map_or(candidate, |current| current.min(candidate)));
                        }
                        FlexDirection::Vertical => {
                            if slot.is_none() {
                                *slot = Some(candidate);
                            }
                        }
                    }
                }
            }
        }

        flex_sizes.size
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        let child_count = ctx.child_count();

        // Read per-child flex factors/fits via the erased parent-data accessor.
        // Falls back to (None, Loose) for children without FlexParentData, which
        // is the correct non-flex default (they are treated as inflexible).
        let mut flex_factors: Vec<Option<i32>> = Vec::with_capacity(child_count);
        let mut flex_fits: Vec<FlexFit> = Vec::with_capacity(child_count);
        for i in 0..child_count {
            let (flex, fit) = ctx
                .child_parent_data_as::<FlexParentData>(i)
                .map_or((None, FlexFit::Loose), |pd| (pd.flex, pd.fit));
            flex_factors.push(flex);
            flex_fits.push(fit);
        }

        let baseline_aligned = self.is_baseline_aligned();
        let text_baseline = self.text_baseline;
        self.compute_sizes(constraints, &flex_factors, &flex_fits, |i, c| {
            let child_size = ctx.child_dry_layout(i, c);
            let baseline = baseline_aligned
                .then(|| ctx.child_dry_baseline(i, c, text_baseline))
                .flatten();
            (child_size, baseline)
        })
        .size
    }

    /// Returns the flex's own baseline recorded during `perform_layout`.
    ///
    /// - Horizontal: the **highest** baseline across children — the minimum of
    ///   `child_baseline + child_offset.dy` (oracle: `box.dart:3336-3348`,
    ///   `flex.dart:806-812`).
    /// - Vertical: the **first** child baseline in list order —
    ///   `child_baseline + child_offset.dy` (oracle: `box.dart:3318-3330`,
    ///   `flex.dart:806-812`).
    ///
    /// Both kinds are recorded eagerly so the querying parent can choose;
    /// this mirrors `AligningShiftedBox::actual_baseline` (`shifted_box.rs:154`).
    fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        self.reported_baselines[baseline_kind_index(baseline)]
    }

    /// Dry-baseline equivalent of `compute_distance_to_actual_baseline`.
    ///
    /// Uses `ctx.child_dry_layout` + `ctx.child_dry_baseline` through the shared
    /// `compute_child_offsets` helper (ADR-0012 D-B3), so the offset/positioning
    /// math is not duplicated.  Applies the same horizontal/highest vs
    /// vertical/first formulas as the live path (oracle: `flex.dart:936-1025` /
    /// `box.dart:3318-3348`).
    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
        ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> Option<f32> {
        let child_count = ctx.child_count();
        if child_count == 0 {
            return None;
        }

        let mut flex_factors: Vec<Option<i32>> = Vec::with_capacity(child_count);
        let mut flex_fits: Vec<FlexFit> = Vec::with_capacity(child_count);
        for i in 0..child_count {
            let (flex, fit) = ctx
                .child_parent_data_as::<FlexParentData>(i)
                .map_or((None, FlexFit::Loose), |pd| (pd.flex, pd.fit));
            flex_factors.push(flex);
            flex_fits.push(fit);
        }

        let baseline_aligned = self.is_baseline_aligned();
        let text_baseline = self.text_baseline;
        let flex_sizes = self.compute_sizes(constraints, &flex_factors, &flex_fits, |i, c| {
            let child_size = ctx.child_dry_layout(i, c);
            let baseline = baseline_aligned
                .then(|| ctx.child_dry_baseline(i, c, text_baseline))
                .flatten();
            (child_size, baseline)
        });

        let alignment_baselines = &flex_sizes.alignment_baselines;
        let child_offsets = self.compute_child_offsets(&flex_sizes, alignment_baselines);

        // Apply highest (horizontal) / first (vertical) formula to dry baselines.
        let mut reported = None::<f32>;

        for (i, &child_offset) in child_offsets.iter().enumerate() {
            let offset_dy = child_offset.dy.get();
            // Reuse alignment baseline for the matching kind (avoids a second call).
            let child_baseline = if baseline == self.text_baseline
                && self.cross_axis_alignment == CrossAxisAlignment::Baseline
                && self.direction == FlexDirection::Horizontal
            {
                alignment_baselines[i]
            } else {
                ctx.child_dry_baseline(i, flex_sizes.child_constraints[i], baseline)
            };

            if let Some(b) = child_baseline {
                let candidate = b + offset_dy;
                match self.direction {
                    FlexDirection::Horizontal => {
                        reported =
                            Some(reported.map_or(candidate, |current| current.min(candidate)));
                    }
                    FlexDirection::Vertical => {
                        if reported.is_none() {
                            reported = Some(candidate);
                        }
                    }
                }
            }
        }

        reported
    }

    // Closure is load-bearing: a `BoxIntrinsicsCtx::child_*` method path is rejected
    // ("implementation of `FnMut` is not general enough" -- the fn item's ctx lifetime
    // is not higher-ranked), so the closure cannot be replaced by a method reference.
    #[allow(clippy::redundant_closure_for_method_calls)]
    fn compute_min_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        match self.direction {
            FlexDirection::Horizontal => {
                self.fold_main_axis_intrinsics(ctx, height, |ctx, i, e| {
                    ctx.child_min_intrinsic_width(i, e)
                })
            }
            FlexDirection::Vertical => {
                Self::intrinsic_cross(ctx, height, |ctx, i, e| ctx.child_min_intrinsic_width(i, e))
            }
        }
    }

    // Closure is load-bearing: a `BoxIntrinsicsCtx::child_*` method path is rejected
    // ("implementation of `FnMut` is not general enough" -- the fn item's ctx lifetime
    // is not higher-ranked), so the closure cannot be replaced by a method reference.
    #[allow(clippy::redundant_closure_for_method_calls)]
    fn compute_max_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        match self.direction {
            FlexDirection::Horizontal => {
                self.fold_main_axis_intrinsics(ctx, height, |ctx, i, e| {
                    ctx.child_max_intrinsic_width(i, e)
                })
            }
            FlexDirection::Vertical => {
                Self::intrinsic_cross(ctx, height, |ctx, i, e| ctx.child_max_intrinsic_width(i, e))
            }
        }
    }

    // Closure is load-bearing: a `BoxIntrinsicsCtx::child_*` method path is rejected
    // ("implementation of `FnMut` is not general enough" -- the fn item's ctx lifetime
    // is not higher-ranked), so the closure cannot be replaced by a method reference.
    #[allow(clippy::redundant_closure_for_method_calls)]
    fn compute_min_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        match self.direction {
            FlexDirection::Vertical => self.fold_main_axis_intrinsics(ctx, width, |ctx, i, e| {
                ctx.child_min_intrinsic_height(i, e)
            }),
            FlexDirection::Horizontal => {
                Self::intrinsic_cross(ctx, width, |ctx, i, e| ctx.child_min_intrinsic_height(i, e))
            }
        }
    }

    // Closure is load-bearing: a `BoxIntrinsicsCtx::child_*` method path is rejected
    // ("implementation of `FnMut` is not general enough" -- the fn item's ctx lifetime
    // is not higher-ranked), so the closure cannot be replaced by a method reference.
    #[allow(clippy::redundant_closure_for_method_calls)]
    fn compute_max_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        match self.direction {
            FlexDirection::Vertical => self.fold_main_axis_intrinsics(ctx, width, |ctx, i, e| {
                ctx.child_max_intrinsic_height(i, e)
            }),
            FlexDirection::Horizontal => {
                Self::intrinsic_cross(ctx, width, |ctx, i, e| ctx.child_max_intrinsic_height(i, e))
            }
        }
    }

    // paint() uses default no-op - Flex just positions children

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable, FlexParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }

        // Test children in reverse order (top-most first)
        for i in (0..self.child_count).rev() {
            if ctx.hit_test_child_at_layout_offset(i) {
                return true;
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flex_row_creation() {
        let row = RenderFlex::row();
        assert!(row.is_horizontal());
        assert!(!row.is_vertical());
    }

    #[test]
    fn test_flex_column_creation() {
        let column = RenderFlex::column();
        assert!(column.is_vertical());
        assert!(!column.is_horizontal());
    }

    #[test]
    fn test_flex_builder() {
        let flex = RenderFlex::column()
            .with_main_axis_alignment(MainAxisAlignment::Center)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(8.0);

        assert_eq!(flex.direction(), FlexDirection::Vertical);
        assert_eq!(flex.main_axis_alignment, MainAxisAlignment::Center);
        assert_eq!(flex.cross_axis_alignment, CrossAxisAlignment::Stretch);
        assert_eq!(flex.spacing, 8.0);
    }

    #[test]
    fn test_flex_default_values() {
        let flex = RenderFlex::row();
        assert_eq!(flex.main_axis_alignment, MainAxisAlignment::Start);
        assert_eq!(flex.cross_axis_alignment, CrossAxisAlignment::Start);
        assert_eq!(flex.spacing, 0.0);
    }
}
