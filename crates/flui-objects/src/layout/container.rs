//! `RenderContainer` — margin, constraints, decoration, padding, alignment and
//! a transform folded into one render object.
//!
//! Flutter builds `Container` as a conditional widget stack
//! (`widgets/container.dart`): from the child outward, `Align` → `Padding` →
//! `ColoredBox` → `DecoratedBox` → `ConstrainedBox` → `Padding` (margin) →
//! `Transform`, each layer present only while its property is set. This object
//! reproduces that stack's observable geometry — layout, paint order, hit
//! testing, intrinsics and baselines — in a single node.
//!
//! Two things follow from collapsing the stack, and both are the point of it:
//!
//! * **The child's slot is structurally stable.** In the conditional stack an
//!   option that turns on or off inserts or removes a level between the parent
//!   and the child, so reconciliation diverges at that level and every element
//!   below it — including an unkeyed stateful child — is rebuilt from scratch
//!   (flutter/flutter#161698). Here the options are *fields*, so no element
//!   moves and no state is lost.
//! * **A `Container` costs one node, not up to seven.** The stack's own
//!   justification is that an unused layer is absent; one node makes every
//!   configuration, used or not, cheaper than the cheapest stack.
//!
//! See `crates/flui-widgets/ARCHITECTURE.md` mapping decision 15.

use flui_painting::{DecorationPaintOptions, Paint, box_decoration_hit_test, paint_box_decoration};
use flui_tree::Single;
use flui_types::geometry::px;
use flui_types::styling::BoxDecoration;
use flui_types::{Alignment, Color, EdgeInsets, Matrix4, Offset, Pixels, Point, Rect, Size};

use flui_rendering::{
    RenderUpdateImpact,
    constraints::BoxConstraints,
    context::{
        BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
        PaintCx,
    },
    parent_data::BoxParentData,
    traits::{PaintEffects, RenderBox, TextBaseline},
};

use crate::layout::align::positioned_box_size;

/// A render object that composes `Container`'s optional layers in one node.
///
/// Every option defaults to "absent", which reproduces the stack that omits
/// the corresponding layer: zero insets, no additional constraints, no
/// alignment, no background, and no transform.
#[derive(Debug, Clone, Default)]
pub struct RenderContainer {
    alignment: Option<Alignment>,
    padding: EdgeInsets,
    margin: EdgeInsets,
    color: Option<Color>,
    decoration: Option<BoxDecoration<Pixels>>,
    additional_constraints: Option<BoxConstraints>,
    transform: Option<Matrix4>,

    has_child: bool,
    /// The child's top-left within this box, committed by the last layout.
    child_offset: Offset,
    /// Extent of the decorated area — the box inside the margin — from the
    /// last layout. This is the `DecoratedBox`/`ColoredBox` level's own size
    /// in the stack this collapses.
    inner_size: Size,
    /// Child baselines captured during layout, indexed by [`TextBaseline`]
    /// (0 = alphabetic, 1 = ideographic).
    child_baselines: [Option<f32>; 2],
}

impl RenderContainer {
    /// An unconfigured container: no insets, constraints, chrome or transform.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the alignment applied to the child, if any.
    pub fn alignment(&self) -> Option<Alignment> {
        self.alignment
    }

    /// Returns the inner padding.
    pub fn padding(&self) -> EdgeInsets {
        self.padding
    }

    /// Returns the outer margin.
    pub fn margin(&self) -> EdgeInsets {
        self.margin
    }

    /// Returns the background color, if any.
    pub fn color(&self) -> Option<Color> {
        self.color
    }

    /// Returns the decoration, if any.
    pub fn decoration(&self) -> Option<&BoxDecoration<Pixels>> {
        self.decoration.as_ref()
    }

    /// Returns the additional constraints, if any.
    pub fn additional_constraints(&self) -> Option<BoxConstraints> {
        self.additional_constraints
    }

    /// Returns the paint transform, if any.
    pub fn transform(&self) -> Option<Matrix4> {
        self.transform
    }

    /// Sets the alignment applied to the child.
    pub fn set_alignment(&mut self, alignment: Option<Alignment>) -> RenderUpdateImpact {
        if self.alignment == alignment {
            return RenderUpdateImpact::NONE;
        }
        self.alignment = alignment;
        RenderUpdateImpact::LAYOUT
    }

    /// Sets the inner padding.
    ///
    /// # Panics
    ///
    /// Debug builds panic on a negative inset, mirroring the Dart
    /// `assert(padding.isNonNegative)`: layout deflates the constraints by
    /// these insets and re-inflates the size by them, so a negative inset
    /// would report a size that does not contain the child.
    pub fn set_padding(&mut self, padding: EdgeInsets) -> RenderUpdateImpact {
        debug_assert!(
            padding.is_non_negative(),
            "RenderContainer padding must be non-negative, got {padding:?}"
        );
        if self.padding == padding {
            return RenderUpdateImpact::NONE;
        }
        self.padding = padding;
        RenderUpdateImpact::LAYOUT
    }

    /// Sets the outer margin.
    ///
    /// # Panics
    ///
    /// Debug builds panic on a negative inset, for the reason given on
    /// [`set_padding`](Self::set_padding).
    pub fn set_margin(&mut self, margin: EdgeInsets) -> RenderUpdateImpact {
        debug_assert!(
            margin.is_non_negative(),
            "RenderContainer margin must be non-negative, got {margin:?}"
        );
        if self.margin == margin {
            return RenderUpdateImpact::NONE;
        }
        self.margin = margin;
        RenderUpdateImpact::LAYOUT
    }

    /// Sets the background color painted over the decoration.
    pub fn set_color(&mut self, color: Option<Color>) -> RenderUpdateImpact {
        if self.color == color {
            return RenderUpdateImpact::NONE;
        }
        self.color = color;
        RenderUpdateImpact::PAINT
    }

    /// Sets the decoration painted behind the child.
    pub fn set_decoration(
        &mut self,
        decoration: Option<BoxDecoration<Pixels>>,
    ) -> RenderUpdateImpact {
        if self.decoration == decoration {
            return RenderUpdateImpact::NONE;
        }
        self.decoration = decoration;
        RenderUpdateImpact::PAINT
    }

    /// Sets the constraints imposed on the child in addition to the incoming
    /// ones.
    pub fn set_additional_constraints(
        &mut self,
        constraints: Option<BoxConstraints>,
    ) -> RenderUpdateImpact {
        if self.additional_constraints == constraints {
            return RenderUpdateImpact::NONE;
        }
        self.additional_constraints = constraints;
        RenderUpdateImpact::LAYOUT
    }

    /// Sets the paint transform.
    pub fn set_transform(&mut self, transform: Option<Matrix4>) -> RenderUpdateImpact {
        if self.transform == transform {
            return RenderUpdateImpact::NONE;
        }
        self.transform = transform;
        RenderUpdateImpact::PAINT | RenderUpdateImpact::SEMANTICS
    }

    /// Builder form of [`set_alignment`](Self::set_alignment).
    #[must_use]
    pub fn with_alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = Some(alignment);
        self
    }

    /// Builder form of [`set_padding`](Self::set_padding).
    #[must_use]
    pub fn with_padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = padding;
        self
    }

    /// Builder form of [`set_margin`](Self::set_margin).
    #[must_use]
    pub fn with_margin(mut self, margin: EdgeInsets) -> Self {
        self.margin = margin;
        self
    }

    /// Builder form of [`set_color`](Self::set_color).
    #[must_use]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Builder form of [`set_decoration`](Self::set_decoration).
    #[must_use]
    pub fn with_decoration(mut self, decoration: BoxDecoration<Pixels>) -> Self {
        self.decoration = Some(decoration);
        self
    }

    /// Builder form of
    /// [`set_additional_constraints`](Self::set_additional_constraints).
    #[must_use]
    pub fn with_additional_constraints(mut self, constraints: BoxConstraints) -> Self {
        self.additional_constraints = Some(constraints);
        self
    }

    /// Builder form of [`set_transform`](Self::set_transform).
    #[must_use]
    pub fn with_transform(mut self, transform: Matrix4) -> Self {
        self.transform = Some(transform);
        self
    }

    /// The child's laid-out offset within this box, valid after layout.
    pub fn child_offset(&self) -> Offset {
        self.child_offset
    }

    /// The decorated area's extent — the box inside the margin — valid after
    /// layout.
    pub fn inner_size(&self) -> Size {
        self.inner_size
    }

    /// The content extent of a childless container: fill a bounded axis,
    /// collapse an unbounded one.
    ///
    /// Flutter's `build` picks between three childless shapes — the
    /// placeholder `LimitedBox(0, 0, child: ConstrainedBox(expand))`, an empty
    /// `Align`, and nothing at all — on whether the additional constraints are
    /// tight and whether an alignment is set. **All three produce this same
    /// size**, so the branch survives in the widget layer only because that
    /// layer has to name *some* widget:
    ///
    /// * the placeholder gives `bounded ? max : min` directly;
    /// * an empty `Align` proposes `max.isInfinite ? 0 : infinity` and then
    ///   constrains it, which lands on the same two values;
    /// * the third shape is reachable only under tight additional
    ///   constraints, where `min == max` makes every candidate coincide.
    ///
    /// Collapsing the stack therefore collapses the branch too. The three
    /// configurations are pinned separately by
    /// `harness_container_childless_branches_all_size_the_same`.
    fn childless_content_size(constraints: &BoxConstraints) -> Size {
        let width = if constraints.has_bounded_width() {
            constraints.max_width
        } else {
            constraints.min_width
        };
        let height = if constraints.has_bounded_height() {
            constraints.max_height
        } else {
            constraints.min_height
        };
        constraints.constrain(Size::new(width, height))
    }

    /// Constraints handed to the padding level: incoming, deflated by the
    /// margin, then narrowed by the additional constraints.
    fn inner_constraints(&self, constraints: &BoxConstraints) -> BoxConstraints {
        let after_margin = constraints.deflate(self.margin);
        match self.additional_constraints {
            Some(additional) => additional.enforce(&after_margin),
            None => after_margin,
        }
    }

    /// Grows the content extent back through the padding and margin, applying
    /// each level's own `constrain` exactly where the stack would.
    fn inflate(
        &self,
        constraints: &BoxConstraints,
        inner_constraints: &BoxConstraints,
        content_size: Size,
    ) -> (Size, Size) {
        let inner_size = inner_constraints.constrain(Size::new(
            content_size.width + self.padding.horizontal_total(),
            content_size.height + self.padding.vertical_total(),
        ));
        let outer_size = constraints.constrain(Size::new(
            inner_size.width + self.margin.horizontal_total(),
            inner_size.height + self.margin.vertical_total(),
        ));
        (inner_size, outer_size)
    }

    /// The child's offset from this box's origin, given the alignment's
    /// contribution inside the content area.
    fn child_offset_for(&self, align_offset: Offset) -> Offset {
        Offset::new(
            self.margin.left + self.padding.left + align_offset.dx,
            self.margin.top + self.padding.top + align_offset.dy,
        )
    }

    /// The translation the transform contributes directly in `paint`.
    ///
    /// A matrix that only translates is applied as a plain offset with no
    /// compositing layer — the same fork `RenderTransform` takes, and the
    /// reason `Container(transform: translate)` does not pay for a layer.
    /// Anything else goes through [`paint_effects`](RenderBox::paint_effects).
    fn paint_translation(&self) -> Offset {
        self.transform
            .and_then(|matrix| matrix.as_translation())
            .map_or(Offset::ZERO, |(dx, dy)| Offset::new(px(dx), px(dy)))
    }

    /// The rect the decoration and color fill, in this box's local space.
    fn chrome_rect(&self, shift: Offset) -> Rect<Pixels> {
        Rect::from_origin_size(
            Point::new(self.margin.left + shift.dx, self.margin.top + shift.dy),
            self.inner_size,
        )
    }
}

impl flui_foundation::Diagnosticable for RenderContainer {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        if let Some(alignment) = self.alignment {
            properties.add_enum("alignment", alignment);
        }
        properties.add_enum("padding", self.padding);
        properties.add_enum("margin", self.margin);
        properties.add("hasColor", self.color.is_some());
        properties.add("hasDecoration", self.decoration.is_some());
        properties.add("hasConstraints", self.additional_constraints.is_some());
        properties.add("hasTransform", self.transform.is_some());
    }
}

impl RenderBox for RenderContainer {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        let inner_constraints = self.inner_constraints(&constraints);
        let content_constraints = inner_constraints.deflate(self.padding);

        self.has_child = ctx.child_count() > 0;

        let content_size = if self.has_child {
            // With an alignment the child is laid out loose and placed inside
            // the resulting box (the `Align` level); without one it takes the
            // content constraints directly and the box is exactly its size.
            let (align_offset, content_size) = if let Some(alignment) = self.alignment {
                let child_size = ctx.layout_child(0, content_constraints.loosen());
                let content_size =
                    positioned_box_size(&content_constraints, child_size, None, None);
                (
                    alignment.along_size(content_size - child_size),
                    content_size,
                )
            } else {
                (Offset::ZERO, ctx.layout_child(0, content_constraints))
            };

            self.child_offset = self.child_offset_for(align_offset);
            ctx.position_child(0, self.child_offset);
            self.child_baselines = [
                ctx.child_distance_to_actual_baseline(0, TextBaseline::Alphabetic),
                ctx.child_distance_to_actual_baseline(0, TextBaseline::Ideographic),
            ];
            content_size
        } else {
            self.child_offset = Offset::ZERO;
            self.child_baselines = [None; 2];
            Self::childless_content_size(&content_constraints)
        };

        let (inner_size, outer_size) = self.inflate(&constraints, &inner_constraints, content_size);
        self.inner_size = inner_size;
        outer_size
    }

    fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        let index = match baseline {
            TextBaseline::Alphabetic => 0,
            TextBaseline::Ideographic => 1,
        };
        self.child_baselines[index].map(|raw| raw + self.child_offset.dy.get())
    }

    fn compute_min_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        let content_height =
            (height - self.margin.vertical_total().get() - self.padding.vertical_total().get())
                .max(0.0);
        let content = if ctx.child_count() == 0 {
            0.0
        } else {
            ctx.child_min_intrinsic_width(0, content_height)
        };
        self.intrinsic_width(content)
    }

    fn compute_max_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        let content_height =
            (height - self.margin.vertical_total().get() - self.padding.vertical_total().get())
                .max(0.0);
        let content = if ctx.child_count() == 0 {
            0.0
        } else {
            ctx.child_max_intrinsic_width(0, content_height)
        };
        self.intrinsic_width(content)
    }

    fn compute_min_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        let content_width =
            (width - self.margin.horizontal_total().get() - self.padding.horizontal_total().get())
                .max(0.0);
        let content = if ctx.child_count() == 0 {
            0.0
        } else {
            ctx.child_min_intrinsic_height(0, content_width)
        };
        self.intrinsic_height(content)
    }

    fn compute_max_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        let content_width =
            (width - self.margin.horizontal_total().get() - self.padding.horizontal_total().get())
                .max(0.0);
        let content = if ctx.child_count() == 0 {
            0.0
        } else {
            ctx.child_max_intrinsic_height(0, content_width)
        };
        self.intrinsic_height(content)
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        let inner_constraints = self.inner_constraints(&constraints);
        let content_constraints = inner_constraints.deflate(self.padding);

        let content_size = if ctx.child_count() > 0 {
            match self.alignment {
                Some(_) => {
                    let child_size = ctx.child_dry_layout(0, content_constraints.loosen());
                    positioned_box_size(&content_constraints, child_size, None, None)
                }
                None => ctx.child_dry_layout(0, content_constraints),
            }
        } else {
            Self::childless_content_size(&content_constraints)
        };

        self.inflate(&constraints, &inner_constraints, content_size)
            .1
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
        ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> Option<f32> {
        if ctx.child_count() == 0 {
            return None;
        }
        let inner_constraints = self.inner_constraints(&constraints);
        let content_constraints = inner_constraints.deflate(self.padding);

        let (child_constraints, align_dy) = match self.alignment {
            Some(alignment) => {
                let loose = content_constraints.loosen();
                let child_size = ctx.child_dry_layout(0, loose);
                let content_size =
                    positioned_box_size(&content_constraints, child_size, None, None);
                let dy = alignment.along_size(content_size - child_size).dy;
                (loose, dy)
            }
            None => (content_constraints, Pixels::ZERO),
        };
        let child_baseline = ctx.child_dry_baseline(0, child_constraints, baseline)?;
        Some(
            child_baseline
                + self
                    .child_offset_for(Offset::new(Pixels::ZERO, align_dy))
                    .dy
                    .get(),
        )
    }

    fn skip_paint(&self) -> bool {
        // A singular matrix compresses the subtree to a line or a point:
        // recording draw commands for it produces output that cannot occupy a
        // pixel. Flutter's `RenderTransform.paint` short-circuits the same way.
        match self.transform {
            Some(matrix) => {
                let determinant = matrix.determinant();
                determinant == 0.0 || !determinant.is_finite()
            }
            None => false,
        }
    }

    /// Paints the decoration, then the color, then the child — Flutter's order
    /// for the stack this collapses, where `DecoratedBox` encloses
    /// `ColoredBox`, which encloses the content.
    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        let shift = self.paint_translation();
        let rect = self.chrome_rect(shift);

        if let Some(decoration) = &self.decoration {
            paint_box_decoration(
                ctx.canvas(),
                rect,
                decoration,
                DecorationPaintOptions::default(),
            );
        }
        if let Some(color) = self.color {
            ctx.canvas().draw_rect(rect, &Paint::fill(color));
        }
        if self.has_child {
            ctx.paint_child_at(self.child_offset + shift);
        }
    }

    /// Reports a non-translation transform for the pipeline to wrap the whole
    /// recorded fragment — decoration included — in a `TransformLayer`.
    ///
    /// A pure translation is excluded: [`paint`](RenderBox::paint) already
    /// applies it as a plain offset, and reporting it here too would translate
    /// the content twice.
    fn paint_effects(&self, _size: Size) -> PaintEffects {
        match self.transform {
            Some(matrix) if matrix.as_translation().is_none() => {
                PaintEffects::NONE.with_transform(matrix)
            }
            _ => PaintEffects::NONE,
        }
    }

    /// Folds the transform and the child's offset into a child-to-parent
    /// mapping.
    ///
    /// Unconditional in the transform, unlike `paint_effects` above: a pure
    /// translation still moves the child, and `local_to_global` and every
    /// hero flight built on it need that term in both branches. This is the
    /// same split Flutter makes between `paint` and `applyPaintTransform`.
    fn apply_paint_transform(
        &self,
        _child: usize,
        child_offset: Offset,
        _size: Size,
        transform: &mut Matrix4,
    ) {
        if let Some(matrix) = self.transform {
            *transform *= matrix;
        }
        *transform *= Matrix4::translation(child_offset.dx.0, child_offset.dy.0, 0.0);
    }

    fn hit_test_transform(&self, _size: Size) -> Option<Matrix4> {
        self.transform
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        // Undo the transform first: everything below it — the margin's bounds
        // gate, the child, the decoration shape — lives in the untransformed
        // space. The driver pushes the inverse of `hit_test_transform` onto the
        // result stack, so the entry's coordinate mapping is already handled.
        let position = match self.transform {
            Some(matrix) => {
                let Some(inverse) = matrix.try_inverse() else {
                    // Degenerate: zero visual area, nothing to hit.
                    return false;
                };
                let local = ctx.position();
                let (dx, dy) = inverse.transform_point(local.dx, local.dy);
                Offset::new(dx, dy)
            }
            None => *ctx.position(),
        };

        let size = ctx.own_size();
        let inside = position.dx >= Pixels::ZERO
            && position.dx < size.width
            && position.dy >= Pixels::ZERO
            && position.dy < size.height;
        if !inside {
            return false;
        }

        // Child before self: the decoration's shape excludes its rounded
        // corners, and a child hittable in a cut-out must still be reachable.
        if self.has_child {
            let child_local = Offset::new(
                position.dx - self.child_offset.dx,
                position.dy - self.child_offset.dy,
            );
            let hit = if self.child_offset == Offset::ZERO {
                ctx.hit_test_child(0, child_local)
            } else {
                ctx.with_offset(self.child_offset, |ctx| ctx.hit_test_child(0, child_local))
            };
            if hit {
                return true;
            }
        }

        let rect = self.chrome_rect(Offset::ZERO);
        if let Some(decoration) = &self.decoration
            && box_decoration_hit_test(rect, decoration, position)
        {
            return true;
        }
        // A colored box is hit-opaque across its whole rect — Flutter's
        // `_RenderColoredBox` is a proxy with `HitTestBehavior.opaque`.
        self.color.is_some()
            && position.dx >= rect.min.x
            && position.dx < rect.max.x
            && position.dy >= rect.min.y
            && position.dy < rect.max.y
    }
}

impl RenderContainer {
    /// Applies the additional constraints and the margin to a content-level
    /// intrinsic width, mirroring `RenderConstrainedBox`'s own intrinsics.
    fn intrinsic_width(&self, content: f32) -> f32 {
        let padded = content + self.padding.horizontal_total().get();
        let constrained = match self.additional_constraints {
            Some(additional) if additional.has_bounded_width() && additional.has_tight_width() => {
                additional.min_width.get()
            }
            Some(additional) if !additional.has_infinite_width() => {
                additional.constrain_width(px(padded)).get()
            }
            _ => padded,
        };
        constrained + self.margin.horizontal_total().get()
    }

    /// Height counterpart of [`intrinsic_width`](Self::intrinsic_width).
    fn intrinsic_height(&self, content: f32) -> f32 {
        let padded = content + self.padding.vertical_total().get();
        let constrained = match self.additional_constraints {
            Some(additional)
                if additional.has_bounded_height() && additional.has_tight_height() =>
            {
                additional.min_height.get()
            }
            Some(additional) if !additional.has_infinite_height() => {
                additional.constrain_height(px(padded)).get()
            }
            _ => padded,
        };
        constrained + self.margin.vertical_total().get()
    }
}
