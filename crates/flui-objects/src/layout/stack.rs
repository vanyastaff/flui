//! `RenderStack` — variable-arity render object that overlays children,
//! with both auto-aligned ("non-positioned") and `Positioned`-decorated
//! ("positioned") layout flows.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderStack`](https://api.flutter.dev/flutter/rendering/RenderStack-class.html)
//! (`packages/flutter/lib/src/rendering/stack.dart`).
//!
//! # Rust-native improvements
//!
//! Flutter's `RenderStack` decides each child's layout by reading raw
//! optional `top` / `right` / `bottom` / `left` / `width` / `height`
//! fields off the child's `StackParentData` at every site that needs
//! the position decision. Those fields are all `double?`; "did the
//! caller actually opt into positioning?" lives in
//! `StackParentData.isPositioned`, a property that re-reads the same
//! optional fields.
//!
//! The Rust port lifts that bimodal decision to a **typed view** —
//! [`PositionedSpec`] — that is constructed once via
//! [`PositionedSpec::from_parent_data`] and *cannot exist for a
//! non-positioned child*. The compiler then forces every call site
//! that needs the positioning math to go through the typed methods on
//! `PositionedSpec`. The optional-field tangle exists only in
//! `StackParentData` (preserved for back-compat); the layout code
//! never sees it.
//!
//! Other Rust niceties:
//!
//! * `const fn` builders (`with_fit`, `with_alignment`,
//!   `with_clip_behavior`) compose at compile time.
//! * Setters return `bool` to signal change (mirrors Wave 1 / Wave 3a
//!   discipline for pipeline `mark_needs_layout` short-circuit).
//! * `has_visual_overflow()` is a post-layout query method; Flutter
//!   keeps the same flag as a private field that the framework reads
//!   only through `clipBehavior`-dependent paint code. Exposing the
//!   flag makes the overflow signal observable for tests and
//!   diagnostics without touching painting.

use flui_tree::Variable;
pub use flui_types::layout::StackFit;
use flui_types::{Alignment, Offset, Pixels, Point, Rect, Size, painting::Clip};

use flui_rendering::{
    constraints::BoxConstraints,
    context::{
        BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
    },
    parent_data::StackParentData,
    traits::{RenderBox, TextBaseline},
};

// =============================================================================
// PositionedSpec — typed view over a positioned child's StackParentData
// =============================================================================

/// Typed snapshot of the six optional positioning fields that make a
/// child "positioned" inside a [`RenderStack`].
///
/// Constructed only by [`PositionedSpec::from_parent_data`], which
/// returns `None` for non-positioned children — that's the discipline
/// that lets the layout code branch on `Option<PositionedSpec>`
/// instead of re-checking individual `Option<f32>` fields.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositionedSpec {
    /// Distance from parent's top edge.
    pub top: Option<Pixels>,
    /// Distance from parent's right edge.
    pub right: Option<Pixels>,
    /// Distance from parent's bottom edge.
    pub bottom: Option<Pixels>,
    /// Distance from parent's left edge.
    pub left: Option<Pixels>,
    /// Explicit width.
    pub width: Option<Pixels>,
    /// Explicit height.
    pub height: Option<Pixels>,
}

impl PositionedSpec {
    /// Lifts a `StackParentData` into a typed positioning view.
    ///
    /// Returns `None` when none of the six positioning fields is set —
    /// i.e. when the child is "non-positioned" and should be sized by
    /// the [`StackFit`] / aligned by the stack's [`Alignment`].
    pub fn from_parent_data(pd: &StackParentData) -> Option<Self> {
        if !pd.is_positioned() {
            return None;
        }
        Some(Self {
            top: pd.top.map(Pixels::new),
            right: pd.right.map(Pixels::new),
            bottom: pd.bottom.map(Pixels::new),
            left: pd.left.map(Pixels::new),
            width: pd.width.map(Pixels::new),
            height: pd.height.map(Pixels::new),
        })
    }

    /// Computes the constraints that should be passed to the positioned
    /// child, given the stack's resolved `size`.
    ///
    /// Matches Flutter's `RenderStack.layoutPositionedChild` constraint
    /// derivation: a paired-edge (left+right or top+bottom) tightens
    /// the corresponding dimension to the remaining gap; otherwise an
    /// explicit `width`/`height` tightens; otherwise the child is loose.
    pub fn child_constraints(&self, stack_size: Size) -> BoxConstraints {
        let mut cc = BoxConstraints::UNCONSTRAINED;

        let tighten_w = if let (Some(l), Some(r)) = (self.left, self.right) {
            Some((stack_size.width - l - r).max(Pixels::ZERO))
        } else {
            self.width.map(|w| w.max(Pixels::ZERO))
        };
        let tighten_h = if let (Some(t), Some(b)) = (self.top, self.bottom) {
            Some((stack_size.height - t - b).max(Pixels::ZERO))
        } else {
            self.height.map(|h| h.max(Pixels::ZERO))
        };

        if let Some(w) = tighten_w {
            cc = cc.tighten(Some(w), None);
        }
        if let Some(h) = tighten_h {
            cc = cc.tighten(None, Some(h));
        }
        cc
    }

    /// Computes the child's top-left offset within `stack_size`, given
    /// the laid-out `child_size` and the stack's fallback `alignment`.
    ///
    /// Matches Flutter:
    /// * x = left, OR stack.width − right − child.width, OR
    ///   `alignment.along_offset(stack − child).dx`.
    /// * y = top, OR stack.height − bottom − child.height, OR
    ///   `alignment.along_offset(stack − child).dy`.
    pub fn child_offset(&self, stack_size: Size, child_size: Size, alignment: Alignment) -> Offset {
        let free_w = stack_size.width - child_size.width;
        let free_h = stack_size.height - child_size.height;

        let x = if let Some(left) = self.left {
            left
        } else if let Some(right) = self.right {
            stack_size.width - right - child_size.width
        } else {
            alignment_along_axis(alignment.x, free_w)
        };

        let y = if let Some(top) = self.top {
            top
        } else if let Some(bottom) = self.bottom {
            stack_size.height - bottom - child_size.height
        } else {
            alignment_along_axis(alignment.y, free_h)
        };

        Offset::new(x, y)
    }
}

/// Maps an alignment scalar in [-1, 1] to a position in [0, free].
#[inline]
fn alignment_along_axis(component: f32, free: Pixels) -> Pixels {
    Pixels::new(free.get() * (component + 1.0) * 0.5)
}

/// Maps a [`TextBaseline`] kind into compact per-kind storage.
#[inline]
const fn baseline_kind_index(baseline: TextBaseline) -> usize {
    match baseline {
        TextBaseline::Alphabetic => 0,
        TextBaseline::Ideographic => 1,
    }
}

// =============================================================================
// StackSizes — shared sizing result used by perform_layout and compute_dry_layout
// =============================================================================

/// Intermediate result from the stack sizing pass.
///
/// Shared between [`RenderStack::perform_layout`] (which continues to
/// positioning) and [`RenderBox::compute_dry_layout`] (which only needs the
/// container size). Mirrors the `FlexSizes` pattern in `flex.rs`.
struct StackSizes {
    /// Constrained container size.
    size: Size,
    /// Per-child sizes: the actual laid-out size for non-positioned children,
    /// [`Size::ZERO`] as a placeholder for positioned children (which are not
    /// measured in the sizing pass).
    child_sizes: Vec<Size>,
}

// =============================================================================
// RenderStack
// =============================================================================

/// Render object that overlays its children, with two layout flows
/// keyed on whether each child has positioning information in its
/// [`StackParentData`].
///
/// **Non-positioned children** are laid out under constraints derived
/// from [`StackFit`] and aligned per [`alignment`](Self::alignment)
/// inside the stack's box. They are the primary contributors to the
/// stack's *size*.
///
/// **Positioned children** are laid out under constraints derived from
/// their [`PositionedSpec`] (synthesised from the `top`/`right`/
/// `bottom`/`left`/`width`/`height` fields on `StackParentData`) and
/// positioned anywhere — possibly outside the box. They do **not**
/// contribute to the stack's size.
///
/// After layout, [`has_visual_overflow`](Self::has_visual_overflow)
/// reports whether any positioned child fell outside the stack
/// box — driving [`paint`](RenderBox::paint) to apply
/// [`clip_behavior`](Self::clip_behavior) when needed.
#[derive(Debug, Clone)]
pub struct RenderStack {
    fit: StackFit,
    alignment: Alignment,
    clip_behavior: Clip,
    /// Computed during layout — read by
    /// [`RenderStack::has_visual_overflow`].
    has_visual_overflow: bool,
    /// Child count snapshot for hit-testing.
    child_count: usize,
}

impl RenderStack {
    /// Creates a stack with `StackFit::Loose`, `Alignment::TOP_LEFT`,
    /// and `Clip::HardEdge` — matching Flutter's `Stack()` defaults.
    pub const fn new() -> Self {
        Self {
            fit: StackFit::Loose,
            alignment: Alignment::TOP_LEFT,
            clip_behavior: Clip::HardEdge,
            has_visual_overflow: false,
            child_count: 0,
        }
    }

    /// Builder: set the fit applied to non-positioned children.
    #[must_use]
    pub const fn with_fit(mut self, fit: StackFit) -> Self {
        self.fit = fit;
        self
    }

    /// Builder: set the alignment used for non-positioned children and
    /// as the fallback for positioned children with neither
    /// left/right nor top/bottom.
    #[must_use]
    pub const fn with_alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Builder: set the clip behavior used when overflow occurs.
    #[must_use]
    pub const fn with_clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = clip_behavior;
        self
    }

    /// Returns the current fit.
    #[inline]
    pub fn fit(&self) -> StackFit {
        self.fit
    }

    /// Returns the current alignment.
    #[inline]
    pub fn alignment(&self) -> Alignment {
        self.alignment
    }

    /// Returns the current clip behavior.
    #[inline]
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    /// Returns whether the last layout produced positioned children
    /// that fell outside the stack box. Reset on every layout.
    #[inline]
    pub fn has_visual_overflow(&self) -> bool {
        self.has_visual_overflow
    }

    /// Updates the fit; returns true if the value changed.
    pub fn set_fit(&mut self, fit: StackFit) -> bool {
        if self.fit == fit {
            return false;
        }
        self.fit = fit;
        true
    }

    /// Updates the alignment; returns true if the value changed.
    pub fn set_alignment(&mut self, alignment: Alignment) -> bool {
        if self.alignment == alignment {
            return false;
        }
        self.alignment = alignment;
        true
    }

    /// Updates the clip behavior; returns true if the value changed.
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) -> bool {
        if self.clip_behavior == clip_behavior {
            return false;
        }
        self.clip_behavior = clip_behavior;
        true
    }

    /// Returns the constraints to pass to non-positioned children for
    /// the given incoming stack constraints + fit.
    fn non_positioned_constraints(&self, incoming: BoxConstraints) -> BoxConstraints {
        match self.fit {
            StackFit::Loose => incoming.loosen(),
            StackFit::Expand => BoxConstraints::tight(incoming.biggest()),
            StackFit::Passthrough => incoming,
        }
    }

    /// Returns whether the offset+size lie inside `stack_size` (used
    /// to compute `has_visual_overflow`).
    fn child_overflows(stack_size: Size, offset: Offset, child_size: Size) -> bool {
        offset.dx.get() < 0.0
            || offset.dy.get() < 0.0
            || (offset.dx + child_size.width).get() > stack_size.width.get()
            || (offset.dy + child_size.height).get() > stack_size.height.get()
    }

    /// Core of the stack sizing pass, shared between `perform_layout` and
    /// `compute_dry_layout`.
    ///
    /// Mirrors Flutter's `_computeSize` (stack.dart:625-675): measures ONLY
    /// non-positioned children (those with `specs[i].is_none()`), accumulates
    /// the maximum child extents, and resolves the container size from
    /// [`StackFit`] and the incoming constraints.
    ///
    /// `measure(i, constraints)` is either `ctx.layout_child` (real layout)
    /// or `ctx.child_dry_layout` (dry layout). Positioned children are NOT
    /// measured in this pass — they are measured in Pass 2 of `perform_layout`.
    fn compute_size(
        &self,
        incoming: BoxConstraints,
        specs: &[Option<PositionedSpec>],
        mut measure: impl FnMut(usize, BoxConstraints) -> Size,
    ) -> StackSizes {
        let child_count = specs.len();

        if child_count == 0 {
            return StackSizes {
                size: if incoming.biggest().is_finite() {
                    incoming.biggest()
                } else {
                    incoming.smallest()
                },
                child_sizes: vec![],
            };
        }

        let nonpos_constraints = self.non_positioned_constraints(incoming);
        let mut has_non_positioned = false;
        let mut content_w = incoming.min_width;
        let mut content_h = incoming.min_height;
        let mut child_sizes = vec![Size::ZERO; child_count];

        for i in 0..child_count {
            if specs[i].is_none() {
                has_non_positioned = true;
                let s = measure(i, nonpos_constraints);
                child_sizes[i] = s;
                if s.width > content_w {
                    content_w = s.width;
                }
                if s.height > content_h {
                    content_h = s.height;
                }
            }
        }

        let size = if has_non_positioned {
            incoming.constrain(Size::new(content_w, content_h))
        } else if incoming.biggest().is_finite() {
            incoming.biggest()
        } else {
            incoming.smallest()
        };

        StackSizes { size, child_sizes }
    }

    /// Flutter stack.dart: each intrinsic dimension is the max of the children.
    fn max_child_intrinsic(
        ctx: &mut BoxIntrinsicsCtx<'_>,
        extent: f32,
        mut query: impl FnMut(&mut BoxIntrinsicsCtx<'_>, usize, f32) -> f32,
    ) -> f32 {
        let child_count = ctx.child_count();
        if child_count == 0 {
            return 0.0;
        }
        let mut max = 0.0f32;
        for i in 0..child_count {
            max = max.max(query(ctx, i, extent));
        }
        max
    }
}

impl Default for RenderStack {
    fn default() -> Self {
        Self::new()
    }
}

impl flui_foundation::Diagnosticable for RenderStack {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add(
            "alignment",
            format!("({}, {})", self.alignment.x, self.alignment.y),
        );
        builder.add_enum("fit", self.fit);
        builder.add_enum("clip_behavior", self.clip_behavior);
    }
}

impl RenderBox for RenderStack {
    type Arity = Variable;
    type ParentData = StackParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, StackParentData>,
    ) -> Size {
        let incoming = *ctx.constraints();
        let child_count = ctx.child_count();
        self.child_count = child_count;
        self.has_visual_overflow = false;

        // Build the per-child PositionedSpec snapshot so both the sizing
        // pass and Pass 2 can branch on positioned vs non-positioned without
        // re-reading parent_data. PositionedSpec is Copy so no reference to
        // ctx is retained after this loop.
        let mut specs: Vec<Option<PositionedSpec>> = Vec::with_capacity(child_count);
        for i in 0..child_count {
            specs.push(
                ctx.child_parent_data(i)
                    .and_then(PositionedSpec::from_parent_data),
            );
        }

        // -----------------------------------------------------------------
        // Sizing pass (= Flutter's _computeSize): measure NON-positioned
        // children, resolve the stack's own size. Delegates to compute_size
        // so dry layout can reuse identical logic.
        // -----------------------------------------------------------------
        let sized = self.compute_size(incoming, &specs, |i, c| ctx.layout_child(i, c));
        let size = sized.size;
        let mut child_sizes = sized.child_sizes;

        // -----------------------------------------------------------------
        // Pass 2 — position non-positioned children (align inside size)
        // and lay out + position the positioned ones.
        // -----------------------------------------------------------------
        #[allow(
            clippy::needless_range_loop,
            reason = "`ctx.layout_child(i, _)` / `ctx.position_child(i, _)` \
                      both consume the index; an .enumerate() iterator would \
                      not help readability and would still need the index."
        )]
        for i in 0..child_count {
            match specs[i] {
                None => {
                    let child_size = child_sizes[i];
                    let offset = Offset::new(
                        alignment_along_axis(self.alignment.x, size.width - child_size.width),
                        alignment_along_axis(self.alignment.y, size.height - child_size.height),
                    );
                    ctx.position_child(i, offset);
                }
                Some(spec) => {
                    let cc = spec.child_constraints(size);
                    let child_size = ctx.layout_child(i, cc);
                    child_sizes[i] = child_size;
                    let offset = spec.child_offset(size, child_size, self.alignment);
                    if Self::child_overflows(size, offset, child_size) {
                        self.has_visual_overflow = true;
                    }
                    ctx.position_child(i, offset);
                }
            }
        }

        size
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        // Build specs via the erased parent-data accessor: same gate as
        // perform_layout so positioned vs non-positioned classification
        // is identical in both paths.
        let child_count = ctx.child_count();
        let mut specs: Vec<Option<PositionedSpec>> = Vec::with_capacity(child_count);
        for i in 0..child_count {
            specs.push(
                ctx.child_parent_data_as::<StackParentData>(i)
                    .and_then(PositionedSpec::from_parent_data),
            );
        }
        // PositionedSpec is Copy — no reference to ctx survives into the closure.
        self.compute_size(constraints, &specs, |i, c| ctx.child_dry_layout(i, c))
            .size
    }

    fn compute_min_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        Self::max_child_intrinsic(ctx, height, |ctx, i, extent| {
            ctx.child_min_intrinsic_width(i, extent)
        })
    }

    fn compute_max_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        Self::max_child_intrinsic(ctx, height, |ctx, i, extent| {
            ctx.child_max_intrinsic_width(i, extent)
        })
    }

    fn compute_min_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        Self::max_child_intrinsic(ctx, width, |ctx, i, extent| {
            ctx.child_min_intrinsic_height(i, extent)
        })
    }

    fn compute_max_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        Self::max_child_intrinsic(ctx, width, |ctx, i, extent| {
            ctx.child_max_intrinsic_height(i, extent)
        })
    }

    fn paint(&self, ctx: &mut flui_rendering::context::PaintCx<'_, Variable>) {
        // Clip when overflow happens AND the user asked for clipping.
        // The clip must cover the CHILDREN, so it goes through a clip
        // layer scope (canvas clips are run-local and never extend
        // across child markers).
        if self.has_visual_overflow && self.clip_behavior != Clip::None {
            let bounds = Rect::from_origin_size(Point::ZERO, ctx.size());
            ctx.with_clip_rect(bounds, self.clip_behavior, |ctx| {
                // Paint all children in order (bottom-up = first to last).
                ctx.paint_children();
            });
        } else {
            ctx.paint_children();
        }
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable, StackParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        // Test children in reverse order — top-most first.
        for i in (0..self.child_count).rev() {
            if ctx.hit_test_child_at_layout_offset(i) {
                return true;
            }
        }
        false
    }
}

// =============================================================================
// RenderIndexedStack
// =============================================================================

/// A stack that lays out all children but paints and hit-tests only one child.
///
/// Flutter parity: `RenderIndexedStack` uses the same layout algorithm as
/// [`RenderStack`] and keeps the layout cost O(N), but only the child at
/// [`index`](Self::index) participates in paint, hit testing, semantics, and
/// baseline reporting. `None` means no child is displayed.
#[derive(Debug, Clone)]
pub struct RenderIndexedStack {
    stack: RenderStack,
    index: Option<usize>,
    /// Baselines recorded during layout for the displayed child only.
    reported_baselines: [Option<f32>; 2],
}

impl RenderIndexedStack {
    /// Creates an indexed stack that displays child `0`, matching Flutter's
    /// `IndexedStack(index: 0)` default.
    pub const fn new() -> Self {
        Self {
            stack: RenderStack::new(),
            index: Some(0),
            reported_baselines: [None; 2],
        }
    }

    /// Builder: set the displayed child index. `None` displays nothing.
    #[must_use]
    pub const fn with_index(mut self, index: Option<usize>) -> Self {
        self.index = index;
        self
    }

    /// Builder: set the fit applied to non-positioned children.
    #[must_use]
    pub const fn with_fit(mut self, fit: StackFit) -> Self {
        self.stack.fit = fit;
        self
    }

    /// Builder: set the alignment used by stack positioning.
    #[must_use]
    pub const fn with_alignment(mut self, alignment: Alignment) -> Self {
        self.stack.alignment = alignment;
        self
    }

    /// Builder: set the clip behavior used when any child geometry overflows.
    #[must_use]
    pub const fn with_clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.stack.clip_behavior = clip_behavior;
        self
    }

    /// The displayed child index, or `None` if no child is displayed.
    #[inline]
    pub const fn index(&self) -> Option<usize> {
        self.index
    }

    /// Updates the displayed child index; returns true if the value changed.
    pub fn set_index(&mut self, index: Option<usize>) -> bool {
        if self.index == index {
            return false;
        }
        self.index = index;
        true
    }

    /// Returns the current fit.
    #[inline]
    pub fn fit(&self) -> StackFit {
        self.stack.fit()
    }

    /// Returns the current alignment.
    #[inline]
    pub fn alignment(&self) -> Alignment {
        self.stack.alignment()
    }

    /// Returns the current clip behavior.
    #[inline]
    pub fn clip_behavior(&self) -> Clip {
        self.stack.clip_behavior()
    }

    /// Whether the last layout produced direct-child geometry overflow.
    #[inline]
    pub fn has_visual_overflow(&self) -> bool {
        self.stack.has_visual_overflow()
    }

    /// Updates the fit; returns true if the value changed.
    pub fn set_fit(&mut self, fit: StackFit) -> bool {
        self.stack.set_fit(fit)
    }

    /// Updates the alignment; returns true if the value changed.
    pub fn set_alignment(&mut self, alignment: Alignment) -> bool {
        self.stack.set_alignment(alignment)
    }

    /// Updates the clip behavior; returns true if the value changed.
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) -> bool {
        self.stack.set_clip_behavior(clip_behavior)
    }

    fn displayed_index(&self, child_count: usize) -> Option<usize> {
        if let Some(index) = self.index {
            debug_assert!(
                index < child_count || child_count == 0,
                "RenderIndexedStack index ({index}) must reference an existing child"
            );
            (index < child_count).then_some(index)
        } else {
            None
        }
    }

    fn build_specs_from_layout_ctx(
        ctx: &BoxLayoutContext<'_, Variable, StackParentData>,
    ) -> Vec<Option<PositionedSpec>> {
        let mut specs = Vec::with_capacity(ctx.child_count());
        for i in 0..ctx.child_count() {
            specs.push(
                ctx.child_parent_data(i)
                    .and_then(PositionedSpec::from_parent_data),
            );
        }
        specs
    }

    fn build_specs_from_dry_layout_ctx(ctx: &BoxDryLayoutCtx<'_>) -> Vec<Option<PositionedSpec>> {
        let mut specs = Vec::with_capacity(ctx.child_count());
        for i in 0..ctx.child_count() {
            specs.push(
                ctx.child_parent_data_as::<StackParentData>(i)
                    .and_then(PositionedSpec::from_parent_data),
            );
        }
        specs
    }

    fn build_specs_from_dry_baseline_ctx(
        ctx: &BoxDryBaselineCtx<'_>,
    ) -> Vec<Option<PositionedSpec>> {
        let mut specs = Vec::with_capacity(ctx.child_count());
        for i in 0..ctx.child_count() {
            specs.push(
                ctx.child_parent_data_as::<StackParentData>(i)
                    .and_then(PositionedSpec::from_parent_data),
            );
        }
        specs
    }
}

impl Default for RenderIndexedStack {
    fn default() -> Self {
        Self::new()
    }
}

impl flui_foundation::Diagnosticable for RenderIndexedStack {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        flui_foundation::Diagnosticable::debug_fill_properties(&self.stack, builder);
        match self.index {
            Some(index) => {
                builder.add_int("index", index as i64, None);
            }
            None => {
                builder.add("index", "null");
            }
        }
    }
}

impl RenderBox for RenderIndexedStack {
    type Arity = Variable;
    type ParentData = StackParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, StackParentData>,
    ) -> Size {
        let incoming = *ctx.constraints();
        let child_count = ctx.child_count();
        self.stack.child_count = child_count;
        self.stack.has_visual_overflow = false;
        self.reported_baselines = [None; 2];
        let displayed_index = self.displayed_index(child_count);

        let specs = Self::build_specs_from_layout_ctx(ctx);
        let sized = self
            .stack
            .compute_size(incoming, &specs, |i, c| ctx.layout_child(i, c));
        let size = sized.size;
        let mut child_sizes = sized.child_sizes;

        for i in 0..child_count {
            let offset = match specs[i] {
                None => {
                    let child_size = child_sizes[i];
                    Offset::new(
                        alignment_along_axis(self.stack.alignment.x, size.width - child_size.width),
                        alignment_along_axis(
                            self.stack.alignment.y,
                            size.height - child_size.height,
                        ),
                    )
                }
                Some(spec) => {
                    let cc = spec.child_constraints(size);
                    let child_size = ctx.layout_child(i, cc);
                    child_sizes[i] = child_size;
                    let offset = spec.child_offset(size, child_size, self.stack.alignment);
                    if RenderStack::child_overflows(size, offset, child_size) {
                        self.stack.has_visual_overflow = true;
                    }
                    offset
                }
            };

            ctx.position_child(i, offset);

            if displayed_index == Some(i) {
                for kind in [TextBaseline::Alphabetic, TextBaseline::Ideographic] {
                    let slot = baseline_kind_index(kind);
                    self.reported_baselines[slot] = ctx
                        .child_distance_to_actual_baseline(i, kind)
                        .map(|baseline| baseline + offset.dy.get());
                }
            }
        }

        size
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        let specs = Self::build_specs_from_dry_layout_ctx(ctx);
        self.stack
            .compute_size(constraints, &specs, |i, c| ctx.child_dry_layout(i, c))
            .size
    }

    fn compute_min_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        RenderStack::max_child_intrinsic(ctx, height, |ctx, i, extent| {
            ctx.child_min_intrinsic_width(i, extent)
        })
    }

    fn compute_max_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        RenderStack::max_child_intrinsic(ctx, height, |ctx, i, extent| {
            ctx.child_max_intrinsic_width(i, extent)
        })
    }

    fn compute_min_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        RenderStack::max_child_intrinsic(ctx, width, |ctx, i, extent| {
            ctx.child_min_intrinsic_height(i, extent)
        })
    }

    fn compute_max_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        RenderStack::max_child_intrinsic(ctx, width, |ctx, i, extent| {
            ctx.child_max_intrinsic_height(i, extent)
        })
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
        ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> Option<f32> {
        let child_count = ctx.child_count();
        let displayed_index = self.displayed_index(child_count)?;
        let specs = Self::build_specs_from_dry_baseline_ctx(ctx);
        let size = self
            .stack
            .compute_size(constraints, &specs, |i, c| ctx.child_dry_layout(i, c))
            .size;
        let child_constraints = match specs[displayed_index] {
            Some(spec) => spec.child_constraints(size),
            None => self.stack.non_positioned_constraints(constraints),
        };
        let child_baseline =
            ctx.child_dry_baseline(displayed_index, child_constraints, baseline)?;
        let child_size = ctx.child_dry_layout(displayed_index, child_constraints);
        let offset = match specs[displayed_index] {
            Some(spec) => spec.child_offset(size, child_size, self.stack.alignment),
            None => Offset::new(
                alignment_along_axis(self.stack.alignment.x, size.width - child_size.width),
                alignment_along_axis(self.stack.alignment.y, size.height - child_size.height),
            ),
        };
        Some(child_baseline + offset.dy.get())
    }

    fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        self.reported_baselines[baseline_kind_index(baseline)]
    }

    fn paint(&self, ctx: &mut flui_rendering::context::PaintCx<'_, Variable>) {
        let Some(index) = self.displayed_index(self.stack.child_count) else {
            return;
        };

        let paint_displayed_child = |ctx: &mut flui_rendering::context::PaintCx<'_, Variable>| {
            ctx.paint_child(index);
        };

        if self.stack.has_visual_overflow && self.stack.clip_behavior != Clip::None {
            let bounds = Rect::from_origin_size(Point::ZERO, ctx.size());
            ctx.with_clip_rect(bounds, self.stack.clip_behavior, paint_displayed_child);
        } else {
            paint_displayed_child(ctx);
        }
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable, StackParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        let Some(index) = self.displayed_index(self.stack.child_count) else {
            return false;
        };
        ctx.hit_test_child_at_layout_offset(index)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    fn bc(min_w: f32, max_w: f32, min_h: f32, max_h: f32) -> BoxConstraints {
        BoxConstraints::new(px(min_w), px(max_w), px(min_h), px(max_h))
    }

    // ---------- PositionedSpec view ---------------------------------------

    #[test]
    fn positioned_spec_returns_none_for_non_positioned() {
        let pd = StackParentData::new();
        assert!(PositionedSpec::from_parent_data(&pd).is_none());
    }

    #[test]
    fn positioned_spec_returns_some_when_any_field_set() {
        let pd = StackParentData::new().with_top(10.0);
        let spec = PositionedSpec::from_parent_data(&pd).expect("positioned");
        assert_eq!(spec.top, Some(px(10.0)));
        assert!(spec.left.is_none());
    }

    #[test]
    fn positioned_spec_child_constraints_paired_edges_tighten() {
        // left + right pair → width = stack.width - left - right
        let pd = StackParentData::new().with_left(20.0).with_right(30.0);
        let spec = PositionedSpec::from_parent_data(&pd).unwrap();
        let cc = spec.child_constraints(Size::new(px(200.0), px(100.0)));
        assert_eq!(cc.min_width, px(150.0));
        assert_eq!(cc.max_width, px(150.0));
        // Height untouched — fully loose.
        assert_eq!(cc.min_height, px(0.0));
        assert_eq!(cc.max_height, Pixels::INFINITY);
    }

    #[test]
    fn positioned_spec_child_constraints_explicit_width_with_anchor() {
        // `width` with a `left` anchor: the explicit width tightens the child
        // width to 80 (Flutter layoutPositionedChild). `width` alone likewise
        // makes a child positioned — see `positioned_spec_width_only_is_positioned`.
        let pd = StackParentData::new().with_left(0.0).with_width(80.0);
        let spec = PositionedSpec::from_parent_data(&pd).unwrap();
        let cc = spec.child_constraints(Size::new(px(200.0), px(100.0)));
        assert_eq!(cc.min_width, px(80.0));
        assert_eq!(cc.max_width, px(80.0));
    }

    #[test]
    fn positioned_spec_width_only_is_positioned() {
        // Flutter parity (stack.dart:242-249): an explicit `width` with no edge
        // anchor DOES make the child positioned. It is excluded from stack
        // sizing and sized to the width, with unanchored axes falling back to
        // the stack's alignment (RenderStack.layoutPositionedChild). The prior
        // assertion (width-only -> non-positioned) mis-cited Flutter.
        let pd = StackParentData::new().with_width(80.0);
        let spec = PositionedSpec::from_parent_data(&pd).expect("a width-only child is positioned");

        // Width tightens to 80; height stays loose (no top+bottom, no height).
        let cc = spec.child_constraints(Size::new(px(200.0), px(100.0)));
        assert_eq!(cc.min_width, px(80.0));
        assert_eq!(cc.max_width, px(80.0));
        assert_eq!(cc.min_height, px(0.0));
        assert_eq!(cc.max_height, Pixels::INFINITY);

        // No horizontal anchor -> x from alignment (CENTER of the free width).
        let off = spec.child_offset(
            Size::new(px(200.0), px(100.0)),
            Size::new(px(80.0), px(40.0)),
            Alignment::CENTER,
        );
        assert_eq!(off.dx, px(60.0)); // free_w = 200 - 80 = 120; CENTER -> 60
        assert_eq!(off.dy, px(30.0)); // free_h = 100 - 40 = 60; CENTER -> 30
    }

    #[test]
    fn positioned_spec_child_offset_left_top_wins() {
        let pd = StackParentData::new().with_left(20.0).with_top(15.0);
        let spec = PositionedSpec::from_parent_data(&pd).unwrap();
        let off = spec.child_offset(
            Size::new(px(200.0), px(100.0)),
            Size::new(px(50.0), px(40.0)),
            Alignment::CENTER,
        );
        assert_eq!(off, Offset::new(px(20.0), px(15.0)));
    }

    #[test]
    fn positioned_spec_child_offset_right_bottom_compute_from_far_edge() {
        let pd = StackParentData::new().with_right(20.0).with_bottom(10.0);
        let spec = PositionedSpec::from_parent_data(&pd).unwrap();
        let off = spec.child_offset(
            Size::new(px(200.0), px(100.0)),
            Size::new(px(50.0), px(40.0)),
            Alignment::CENTER,
        );
        // x = 200 - 20 - 50 = 130; y = 100 - 10 - 40 = 50
        assert_eq!(off, Offset::new(px(130.0), px(50.0)));
    }

    #[test]
    fn positioned_spec_child_offset_falls_back_to_alignment_per_axis() {
        // Only `top` set → x uses alignment, y uses top.
        let pd = StackParentData::new().with_top(10.0);
        let spec = PositionedSpec::from_parent_data(&pd).unwrap();
        let off = spec.child_offset(
            Size::new(px(200.0), px(100.0)),
            Size::new(px(50.0), px(40.0)),
            Alignment::CENTER,
        );
        // x: free_w = 150, center alignment → 75; y = 10
        assert_eq!(off, Offset::new(px(75.0), px(10.0)));
    }

    // ---------- RenderStack defaults and builders -------------------------

    #[test]
    fn defaults_match_flutter() {
        let stack = RenderStack::new();
        assert_eq!(stack.fit(), StackFit::Loose);
        assert_eq!(stack.alignment(), Alignment::TOP_LEFT);
        assert_eq!(stack.clip_behavior(), Clip::HardEdge);
        assert!(!stack.has_visual_overflow());
    }

    #[test]
    fn builder_chain_assembles_stack() {
        let stack = RenderStack::new()
            .with_fit(StackFit::Expand)
            .with_alignment(Alignment::CENTER)
            .with_clip_behavior(Clip::AntiAlias);
        assert_eq!(stack.fit(), StackFit::Expand);
        assert_eq!(stack.alignment(), Alignment::CENTER);
        assert_eq!(stack.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn setters_return_change_flag() {
        let mut stack = RenderStack::new();
        assert!(stack.set_fit(StackFit::Expand));
        assert!(!stack.set_fit(StackFit::Expand));
        assert!(stack.set_alignment(Alignment::CENTER));
        assert!(stack.set_clip_behavior(Clip::AntiAlias));
    }

    // ---------- non_positioned_constraints --------------------------------

    #[test]
    fn fit_loose_loosens_constraints() {
        let stack = RenderStack::new().with_fit(StackFit::Loose);
        let cc = stack.non_positioned_constraints(bc(50.0, 200.0, 30.0, 100.0));
        assert_eq!(cc.min_width, px(0.0));
        assert_eq!(cc.min_height, px(0.0));
        assert_eq!(cc.max_width, px(200.0));
        assert_eq!(cc.max_height, px(100.0));
    }

    #[test]
    fn fit_expand_tightens_to_biggest() {
        let stack = RenderStack::new().with_fit(StackFit::Expand);
        let cc = stack.non_positioned_constraints(bc(0.0, 200.0, 0.0, 100.0));
        assert_eq!(cc.min_width, px(200.0));
        assert_eq!(cc.max_width, px(200.0));
        assert_eq!(cc.min_height, px(100.0));
        assert_eq!(cc.max_height, px(100.0));
    }

    #[test]
    fn fit_passthrough_preserves_constraints() {
        let stack = RenderStack::new().with_fit(StackFit::Passthrough);
        let cc = stack.non_positioned_constraints(bc(50.0, 200.0, 30.0, 100.0));
        assert_eq!(cc.min_width, px(50.0));
        assert_eq!(cc.max_width, px(200.0));
    }

    // ---------- overflow detection ----------------------------------------

    #[test]
    fn overflow_detection_inside_bounds_is_false() {
        assert!(!RenderStack::child_overflows(
            Size::new(px(100.0), px(100.0)),
            Offset::new(px(10.0), px(10.0)),
            Size::new(px(50.0), px(50.0)),
        ));
    }

    #[test]
    fn overflow_detection_offscreen_x_is_true() {
        assert!(RenderStack::child_overflows(
            Size::new(px(100.0), px(100.0)),
            Offset::new(px(60.0), px(0.0)),
            Size::new(px(50.0), px(50.0)),
        ));
    }

    #[test]
    fn overflow_detection_negative_offset_is_true() {
        assert!(RenderStack::child_overflows(
            Size::new(px(100.0), px(100.0)),
            Offset::new(px(-1.0), px(0.0)),
            Size::new(px(50.0), px(50.0)),
        ));
    }

    // ---------- alignment_along_axis helper -------------------------------

    #[test]
    fn alignment_along_axis_maps_minus_one_to_zero() {
        assert_eq!(alignment_along_axis(-1.0, px(100.0)), px(0.0));
    }

    #[test]
    fn alignment_along_axis_maps_zero_to_half() {
        assert_eq!(alignment_along_axis(0.0, px(100.0)), px(50.0));
    }

    #[test]
    fn alignment_along_axis_maps_plus_one_to_full() {
        assert_eq!(alignment_along_axis(1.0, px(100.0)), px(100.0));
    }

    // ---------- Diagnostics -----------------------------------------------

    #[test]
    fn debug_fill_properties_lists_stack_state() {
        use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
        let stack = RenderStack::new();
        let mut builder = DiagnosticsBuilder::new();
        stack.debug_fill_properties(&mut builder);
        let names: Vec<String> = builder
            .build()
            .iter()
            .map(|p| p.name().to_string())
            .collect();
        for required in ["alignment", "fit", "clip_behavior"] {
            assert!(
                names.iter().any(|n| n == required),
                "missing diagnostic field: {required}"
            );
        }
    }
}
