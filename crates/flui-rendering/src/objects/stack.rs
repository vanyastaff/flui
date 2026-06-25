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

use crate::{
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext},
    parent_data::StackParentData,
    traits::RenderBox,
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

        // No-child fast path — Flutter parity: take the biggest finite
        // size, otherwise the smallest.
        if child_count == 0 {
            return if incoming.biggest().is_finite() {
                incoming.biggest()
            } else {
                incoming.smallest()
            };
        }

        // -----------------------------------------------------------------
        // Pass 1 — lay out NON-positioned children and accumulate the
        // stack's intrinsic content extent.
        // -----------------------------------------------------------------
        let nonpos_constraints = self.non_positioned_constraints(incoming);
        let mut has_non_positioned = false;
        let mut content_w = incoming.min_width;
        let mut content_h = incoming.min_height;
        // Cache the per-child PositionedSpec snapshot so Pass 2 doesn't
        // have to re-read parent_data.
        let mut specs: Vec<Option<PositionedSpec>> = Vec::with_capacity(child_count);
        let mut sizes: Vec<Size> = vec![Size::ZERO; child_count];

        #[allow(
            clippy::needless_range_loop,
            reason = "`ctx.child_parent_data(i)` / `ctx.layout_child(i, _)` \
                      both consume the index; .enumerate() would not help."
        )]
        for i in 0..child_count {
            let spec = ctx
                .child_parent_data(i)
                .and_then(PositionedSpec::from_parent_data);
            specs.push(spec);

            if spec.is_none() {
                has_non_positioned = true;
                let s = ctx.layout_child(i, nonpos_constraints);
                sizes[i] = s;
                if s.width > content_w {
                    content_w = s.width;
                }
                if s.height > content_h {
                    content_h = s.height;
                }
            }
        }

        // -----------------------------------------------------------------
        // Resolve the stack's own size.
        // -----------------------------------------------------------------
        let size = if has_non_positioned {
            incoming.constrain(Size::new(content_w, content_h))
        } else if incoming.biggest().is_finite() {
            incoming.biggest()
        } else {
            incoming.smallest()
        };

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
                    let child_size = sizes[i];
                    let offset = Offset::new(
                        alignment_along_axis(self.alignment.x, size.width - child_size.width),
                        alignment_along_axis(self.alignment.y, size.height - child_size.height),
                    );
                    ctx.position_child(i, offset);
                }
                Some(spec) => {
                    let cc = spec.child_constraints(size);
                    let child_size = ctx.layout_child(i, cc);
                    sizes[i] = child_size;
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

    fn paint(&self, ctx: &mut crate::context::PaintCx<'_, Variable>) {
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
        // Per Flutter: `width` alone doesn't make a child positioned
        // (only top/right/bottom/left do). `width` is meaningful only
        // alongside one of the four edge anchors.
        let pd = StackParentData::new().with_left(0.0).with_width(80.0);
        let spec = PositionedSpec::from_parent_data(&pd).unwrap();
        let cc = spec.child_constraints(Size::new(px(200.0), px(100.0)));
        assert_eq!(cc.min_width, px(80.0));
        assert_eq!(cc.max_width, px(80.0));
    }

    #[test]
    fn positioned_spec_width_only_is_not_positioned() {
        // Flutter parity: width without an anchor doesn't trigger the
        // positioned flow at all — the child remains a non-positioned
        // stack child sized via StackFit/Alignment.
        let pd = StackParentData::new().with_width(80.0);
        assert!(PositionedSpec::from_parent_data(&pd).is_none());
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
