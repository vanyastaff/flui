//! RenderTransform - applies a transformation matrix to a single child.

use flui_tree::Single;
use flui_types::geometry::px;
use flui_types::{Alignment, Matrix4, Offset, Size};

use flui_rendering::{
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    traits::{PaintEffects, RenderBox},
};

/// A render object that applies a transformation matrix to its child.
///
/// The transformation is applied around an origin point, which by default
/// is the center of the render object. The origin can be specified using
/// alignment or an explicit offset.
///
/// # Performance
///
/// Transform creates a compositing layer when `needs_compositing` is true,
/// which has some performance cost but enables hardware acceleration.
///
/// # Example
///
/// ```ignore
/// // Scale to 50%
/// let transform = RenderTransform::scale(0.5, 0.5);
///
/// // Rotate 45 degrees around center
/// let transform = RenderTransform::rotation(std::f32::consts::PI / 4.0);
///
/// // Custom matrix
/// let transform = RenderTransform::new(Matrix4::IDENTITY);
/// ```
#[derive(Debug, Clone)]
pub struct RenderTransform {
    /// The transformation matrix.
    transform: Matrix4,
    /// Origin for the transformation as alignment, or `None` for no
    /// alignment contribution at all.
    ///
    /// `None` is not the same as `Alignment::TOP_LEFT`: it means the
    /// alignment term is absent from the pivot, so an `origin` set alone acts
    /// alone. Flutter's `alignment` is nullable for exactly this reason, and
    /// its bare `Transform(...)` constructor leaves it null while the
    /// `rotate`/`scale`/`flip` factories pass `Alignment.center` explicitly.
    alignment: Option<Alignment>,
    /// Explicit origin offset, added to (not overriding) `alignment`'s own
    /// contribution — see `compute_origin`'s doc comment for the additive
    /// pivot formula.
    origin: Option<Offset>,
    /// Whether we have a child.
    has_child: bool,
    /// Whether to use compositing layers.
    needs_compositing: bool,
    /// Whether hit-testing goes through the transform.
    ///
    /// `true` by default. With `false` the child is hit where it was LAID
    /// OUT rather than where it paints — what a decorative transform wants,
    /// so the moved pixels do not move the touch target. Paint and
    /// `hit_test_transform`-derived global coordinates are unaffected either
    /// way (Flutter's `transformHitTests`).
    transform_hit_tests: bool,
}

impl RenderTransform {
    /// Creates a new transform render object with the given matrix.
    pub fn new(transform: Matrix4) -> Self {
        Self {
            transform,
            // Flutter's bare constructor leaves `alignment` null; the named
            // factories below opt into CENTER the way its own do.
            alignment: None,
            origin: None,
            has_child: false,
            needs_compositing: true,
            transform_hit_tests: true,
        }
    }

    /// Whether hit-testing goes through the transform. `true` by default.
    #[must_use]
    pub const fn transform_hit_tests(&self) -> bool {
        self.transform_hit_tests
    }

    /// Sets whether hit-testing goes through the transform.
    pub fn set_transform_hit_tests(&mut self, value: bool) -> flui_rendering::RenderUpdateImpact {
        if self.transform_hit_tests == value {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.transform_hit_tests = value;
        // Hit-testing reads this live from the render object; nothing painted
        // or laid out changes.
        flui_rendering::RenderUpdateImpact::NONE
    }

    /// Sets whether hit-testing goes through the transform.
    #[must_use]
    pub const fn with_transform_hit_tests(mut self, value: bool) -> Self {
        self.transform_hit_tests = value;
        self
    }

    /// Creates an identity transform (no transformation).
    pub fn identity() -> Self {
        Self::new(Matrix4::IDENTITY)
    }

    /// Creates a translation transform.
    ///
    /// No alignment, matching `Transform.translate`, which takes none: a
    /// translation is pivot-invariant, so an alignment term would cancel out
    /// anyway.
    pub fn translate(x: f32, y: f32) -> Self {
        Self::new(Matrix4::translation(x, y, 0.0))
    }

    /// Creates a scale transform about the box's centre.
    ///
    /// Centre, not the bare constructor's absent alignment — `Transform.scale`
    /// passes `Alignment.center` explicitly, and a scale about the top-left
    /// corner is almost never what a caller means.
    pub fn scale(sx: f32, sy: f32) -> Self {
        Self::new(Matrix4::scaling(sx, sy, 1.0)).with_alignment(Alignment::CENTER)
    }

    /// Creates a uniform scale transform.
    pub fn uniform_scale(scale: f32) -> Self {
        Self::scale(scale, scale)
    }

    /// Creates a rotation transform around the Z axis.
    ///
    /// # Arguments
    ///
    /// * `radians` - Rotation angle in radians.
    pub fn rotation(radians: f32) -> Self {
        // `Transform.rotate` passes `Alignment.center` explicitly, for the
        // same reason `scale` does.
        Self::new(Matrix4::rotation_z(radians)).with_alignment(Alignment::CENTER)
    }

    /// Creates a rotation transform from degrees.
    pub fn rotation_degrees(degrees: f32) -> Self {
        Self::rotation(degrees.to_radians())
    }

    /// Returns the transformation matrix.
    pub fn transform(&self) -> &Matrix4 {
        &self.transform
    }

    /// Whether this node currently emits an updatable transform effect layer.
    ///
    /// Reading `self.transform` rather than `effective_transform(size)` is
    /// deliberate: a setter has no `size` to hand. The two agree, but the
    /// reason is narrower than it looks and the clause order below is an
    /// invariant, not an accident.
    ///
    /// The only thing conjugation `T(o)·M·T(-o)` preserves unconditionally is
    /// the **perspective row** (`m[3]`, `m[7]`, `m[11]`). It changes the linear
    /// 3×3 basis — with `m[3] = 0.25` and a pivot of 8, `m[0] = 1` conjugates
    /// to `3` — and it changes `m[15]`, by the dot product of that perspective
    /// row with `-o` (same example: `1` becomes `-1`).
    ///
    /// [`Matrix4::as_translation`] agrees across the conjugation anyway, and
    /// the argument runs through the perspective row rather than around it.
    /// That function answers `Some` only when the row is zero. When it is
    /// zero, both changes above vanish — the dot product is zero and the basis
    /// is untouched — so every bit it tests is preserved. When it is non-zero
    /// it answers `None` on both sides, whatever happened to the other
    /// entries. Neither branch can disagree.
    ///
    /// The agreement also fails outright on non-finite matrices:
    /// `Matrix4::translation(f32::INFINITY, 0.0, 0.0)` is `Some` before
    /// conjugation and `None` after, because the products contaminate every
    /// entry. Such a matrix has a non-finite determinant, so the
    /// `!skip_paint(self)` clause rejects it first — that clause is
    /// **load-bearing for this predicate**, not merely an independent gate,
    /// and reordering the three below would reintroduce the disagreement.
    ///
    /// With those two clauses in place the predicate is independent of `size`
    /// and of the pivot, which is also what lets `set_alignment`/`set_origin`
    /// know it cannot flip under them.
    fn owns_effect_layer(&self) -> bool {
        self.has_child
            && !<Self as RenderBox>::skip_paint(self)
            && self.transform.as_translation().is_none()
    }

    /// Sets the transformation matrix.
    pub fn set_transform(&mut self, transform: Matrix4) -> flui_rendering::RenderUpdateImpact {
        if self.transform == transform {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        let owned_before = self.owns_effect_layer();
        self.transform = transform;
        let owned_after = self.owns_effect_layer();
        // A matrix that stays within the layered range (owns a TransformLayer
        // both before and after) lands only on that layer, so the frame can
        // rebuild it and replay the enclosing repaint boundary's retained
        // output instead of repainting the subtree — the same
        // `markNeedsCompositedLayerUpdate` shape `RenderOpacity::set_opacity`
        // already uses. Crossing INTO or OUT OF the range (translation ↔
        // non-translation, or singular ↔ non-singular) is structural: the
        // layer itself appears or disappears, which a patch cannot express,
        // so only a repaint can serve it.
        let paint_or_layer = if owned_before && owned_after {
            flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
        } else {
            flui_rendering::RenderUpdateImpact::PAINT
        };
        paint_or_layer | flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    /// Updates the alignment-relative pivot without replacing layout state.
    pub fn set_alignment(
        &mut self,
        alignment: Option<Alignment>,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.alignment == alignment {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.alignment = alignment;
        // Pivot-only: `owns_effect_layer`'s predicate cannot flip on an
        // alignment change (see its doc), so reading it once after the
        // mutation — rather than before-and-after like `set_transform` — is
        // sufficient.
        let paint_or_layer = if self.owns_effect_layer() {
            flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
        } else {
            flui_rendering::RenderUpdateImpact::PAINT
        };
        paint_or_layer | flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    /// Updates the explicit pivot without replacing layout state.
    pub fn set_origin(&mut self, origin: Option<Offset>) -> flui_rendering::RenderUpdateImpact {
        if self.origin == origin {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.origin = origin;
        // Pivot-only, same reasoning as `set_alignment` above.
        let paint_or_layer = if self.owns_effect_layer() {
            flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
        } else {
            flui_rendering::RenderUpdateImpact::PAINT
        };
        paint_or_layer | flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    /// Sets the alignment for the transform origin.
    ///
    /// Also resets `origin` to `None` — callers that want both an alignment
    /// AND an explicit origin must call [`with_origin`](Self::with_origin)
    /// AFTER this, not before (see `compute_origin`'s additive formula:
    /// `origin` still combines with whatever `alignment` is set here, it's
    /// only a prior `with_origin` call that this clears).
    pub fn with_alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = Some(alignment);
        self.origin = None;
        self
    }

    /// Removes the alignment contribution, leaving `origin` (if any) as the
    /// whole pivot — Flutter's null `alignment`.
    #[must_use]
    pub const fn without_alignment(mut self) -> Self {
        self.alignment = None;
        self
    }

    /// Sets an explicit origin for the transformation.
    pub fn with_origin(mut self, origin: Offset) -> Self {
        self.origin = Some(origin);
        self
    }

    /// Sets whether this transform needs compositing.
    ///
    /// When true, a transform layer is created for hardware acceleration.
    /// When false, the transform is applied directly to the canvas.
    pub fn set_needs_compositing(&mut self, value: bool) -> flui_rendering::RenderUpdateImpact {
        if self.needs_compositing == value {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.needs_compositing = value;
        flui_rendering::RenderUpdateImpact::COMPOSITING_BITS
    }

    /// Returns the alignment, or `None` when the pivot has no alignment term.
    pub fn alignment(&self) -> Option<Alignment> {
        self.alignment
    }

    /// Returns the explicit origin if set.
    pub fn origin(&self) -> Option<Offset> {
        self.origin
    }

    /// Computes the effective pivot offset for the transform from the laid-out
    /// `size` (supplied by the driver from `RenderState`).
    ///
    /// Flutter's `RenderTransform._effectiveTransform` applies the origin AND the
    /// alignment **additively** — `T(origin)·T(alignment.alongSize)·transform·…`
    /// — so the pivot is `alignment.alongSize(size) + origin`, not one or the
    /// other (proxy_box.dart). `alignment` is always present (default `CENTER`);
    /// `origin` is optional. The prior code returned `origin` alone whenever it
    /// was set, silently dropping the alignment contribution.
    fn compute_origin(&self, size: Size) -> Offset {
        let origin = self.origin.unwrap_or(Offset::ZERO);
        let Some(alignment) = self.alignment else {
            // No alignment term: an `origin` set alone acts alone
            // (`_effectiveTransform`'s `resolvedAlignment == null` branch).
            return origin;
        };
        let align_x = size.width * f32::midpoint(alignment.x, 1.0);
        let align_y = size.height * f32::midpoint(alignment.y, 1.0);
        Offset::new(align_x + origin.dx, align_y + origin.dy)
    }

    /// Computes the effective transform matrix with origin applied, using
    /// the laid-out `size` for an alignment-relative origin.
    fn effective_transform(&self, size: Size) -> Matrix4 {
        let origin = self.compute_origin(size);

        // Translate to origin, apply transform, translate back
        let to_origin = Matrix4::translation((-origin.dx).into(), (-origin.dy).into(), 0.0);
        let from_origin = Matrix4::translation(origin.dx.into(), origin.dy.into(), 0.0);

        from_origin * self.transform * to_origin
    }
}

impl Default for RenderTransform {
    fn default() -> Self {
        Self::identity()
    }
}

impl flui_foundation::Diagnosticable for RenderTransform {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add("transform", format!("{:?}", self.transform));
        properties.add_optional("alignment", self.alignment.map(|a| format!("{a:?}")));
        properties.add_optional("origin", self.origin.map(|o| format!("{o:?}")));
    }
}
impl RenderBox for RenderTransform {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();

        // A transform takes its child's size (or the smallest size when
        // childless). The committed size lands on `RenderState`; the
        // transform-origin hooks read it back via their `size` argument.
        if ctx.child_count() > 0 {
            self.has_child = true;
            // Layout child with same constraints
            ctx.layout_child(0, constraints)
        } else {
            self.has_child = false;
            constraints.smallest()
        }
    }

    flui_rendering::forward_single_child_box_queries!();

    // `paint()` below and `paint_effects()`'s `transform` field split the
    // matrix by translation-ness: a pure translation is applied directly in
    // `paint()` as a plain child offset, with no layer at all; anything else
    // is reported through `paint_effects().transform`, which the pipeline
    // wraps in a `TransformLayer` BEFORE replaying `paint()`'s fragment — so
    // `paint()` must not (and does not) push that matrix a second time
    // itself.

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        // A transform does NOT test its own (untransformed) size — how the
        // untransformed size and the child's transformed position interact
        // is ill-defined, so only the child decides (Flutter parity:
        // `RenderTransform.hitTest`, box.dart). A scale(2) child visually
        // covering 80×80 must be hittable across that whole area even
        // though this node's laid-out size is 40×40.
        if !self.has_child {
            return false;
        }
        if !self.transform_hit_tests {
            // The child is hit where it was LAID OUT, not where it paints —
            // Flutter's `transformHitTests: false`, which passes a null
            // transform to `addWithPaintTransform`. A degenerate matrix does
            // not disqualify the child here either: nothing is being
            // inverted, so there is nothing to be singular.
            return ctx.hit_test_child(0, *ctx.position());
        }
        let Some(inverse) = self.effective_transform(ctx.own_size()).try_inverse() else {
            // A degenerate (non-invertible) matrix collapses the subtree
            // to zero visual area: nothing is visible, nothing is hit.
            return false;
        };
        let local_pos = ctx.position();
        let (tx, ty) = inverse.transform_point(local_pos.dx, local_pos.dy);
        // Transform symmetry: the child sees the SAME point the paint
        // matrix mapped — forward the inverse-transformed position, not
        // the original one. (The pre-fix shape passed the untransformed
        // position; the inverse was used only for a bounds gate, so any
        // scaled/rotated child hit-tested at the wrong local point.)

        // The pipeline pushes the INVERSE of hit_test_transform() onto
        // HitTestResult before calling hit_test_raw (HitTestResult composes
        // the global-to-local mapping from each level's own inverse), so
        // child entries capture the correct accumulated transform.
        // No push/pop needed here.
        ctx.hit_test_child(0, Offset::new(tx, ty))
    }

    fn skip_paint(&self) -> bool {
        // Flutter `RenderTransform.paint`: "if the matrix is singular the
        // children would be compressed to a line or single point, instead
        // short-circuit and paint nothing" — it clears its layer and returns
        // on `det == 0 || !det.isFinite`. Painting such a subtree records
        // draw commands and composites layers for output that cannot occupy
        // a single pixel.
        //
        // `self.transform`, not `effective_transform(size)`: the effective
        // matrix only wraps this one in origin translations, and a
        // translation's determinant is 1, so the two determinants are equal.
        // That is what lets the check live on `skip_paint`, which is handed
        // no laid-out size.
        //
        // Spelled out rather than `!self.transform.is_invertible()`: that
        // helper gates on `det.abs() >= f32::EPSILON`, which answers *true*
        // for an infinite determinant and would paint exactly the non-finite
        // cases this must suppress.
        let determinant = self.transform.determinant();
        determinant == 0.0 || !determinant.is_finite()
    }

    /// Paints the child through the effective transform — or, when that
    /// transform is a pure translation, at a plain offset with no layer at all.
    ///
    /// Flutter's `RenderTransform.paint` takes the same fork on
    /// `MatrixUtils.getAsTranslation` (`rendering/proxy_box.dart`): a matrix
    /// that only translates is applied as `super.paint(context, offset +
    /// childOffset)`, and the node's layer is cleared. A compositing layer per
    /// `Transform` is not free, and translation is the common case —
    /// `Transform.translate`, and every `SlideTransition` built on it, paid for
    /// one on every frame.
    ///
    /// A non-translation matrix is reported through [`paint_effects`
    /// below](Self::paint_effects)'s `transform` field instead of pushed
    /// here: the pipeline emits a node's `paint_effects` transform layer
    /// *before* replaying the fragments its `paint` recorded, wrapping the
    /// whole fragment — including the bare child splice below — in it.
    /// Pushing the matrix again here would wrap the child in it twice.
    /// Coordinate mapping keeps the matrix through `apply_paint_transform`
    /// below regardless of which branch painted — the same split Flutter
    /// makes between `paint` and `applyPaintTransform`.
    fn paint(&self, ctx: &mut flui_rendering::context::PaintCx<'_, Single>) {
        if !self.has_child {
            return;
        }

        let transform = self.effective_transform(ctx.size());
        if let Some((dx, dy)) = transform.as_translation() {
            ctx.paint_child_at(Offset::new(px(dx), px(dy)));
        } else {
            // The pipeline already pushed this matrix's TransformLayer (see
            // `paint_effects` below) before replaying this fragment, so a
            // bare splice is correct — no `with_transform` here.
            ctx.paint_child();
        }
    }

    /// Reports the effective transform for the pipeline to wrap in a
    /// `TransformLayer` — for every case except a pure translation, which
    /// `paint` above applies directly as a plain child offset with no layer.
    ///
    /// This is what makes a matrix change a composited-layer update: the
    /// layer this reports lands in the enclosing repaint boundary's retained
    /// capture (`RetainedSubtree::effect_slots`), addressable for
    /// `set_transform` to patch in place instead of repainting the subtree.
    /// See `owns_effect_layer`, which mirrors this same fork for the setters.
    fn paint_effects(&self, size: Size) -> PaintEffects {
        if !self.has_child {
            return PaintEffects::NONE;
        }
        let transform = self.effective_transform(size);
        if transform.as_translation().is_some() {
            // Painted as a plain offset in `paint` above, with no layer at
            // all — reporting a transform here too would make the pipeline
            // push a redundant TransformLayer around content `paint` already
            // offsets directly.
            return PaintEffects::NONE;
        }
        PaintEffects::NONE.with_transform(transform)
    }

    /// Folds the transform into a child-to-parent coordinate mapping.
    ///
    /// Still overridden, and the reason changed with `paint_effects`: the
    /// default derives the mapping FROM `paint_effects`'s `transform` field,
    /// which is `None` for a pure translation — `paint` applies that one as a
    /// plain child offset instead of a layer. Coordinate mapping needs the
    /// matrix in **both** branches, so deriving it from the value would
    /// silently drop the translation from `transform_to` / local-to-global
    /// and every hero flight built on them. Unconditional here, as Flutter's
    /// `applyPaintTransform` is.
    fn apply_paint_transform(
        &self,
        _child: usize,
        child_offset: Offset,
        size: Size,
        transform: &mut Matrix4,
    ) {
        *transform *= self.effective_transform(size);
        *transform *= Matrix4::translation(child_offset.dx.0, child_offset.dy.0, 0.0);
    }

    fn hit_test_transform(&self, size: Size) -> Option<Matrix4> {
        // `None` when hit-testing is untransformed, and the two halves have to
        // agree or the flag is worse than useless.
        //
        // This hook feeds the hit ENTRY's transform stack — the global-to-local
        // mapping a delivered event is localized through
        // (`PipelineOwner::hit_test_subtree` pushes its inverse). It is the
        // analogue of the `transform:` argument Flutter passes to
        // `addWithPaintTransform`, which is exactly what
        // `transformHitTests: false` sets to null — NOT of
        // `applyPaintTransform`, which stays unconditional because it answers
        // a different question (where the child paints, for `localToGlobal`).
        //
        // Reporting the matrix here while hit-testing untransformed would hand
        // the child a `local_position` mapped through a transform its hit did
        // not use, and a singular matrix would make delivery drop the entry
        // outright — so the target the flag promises would be hit and then
        // receive nothing.
        if !self.transform_hit_tests {
            return None;
        }
        Some(self.effective_transform(size))
    }
}

#[cfg(test)]
mod tests {

    /// `skip_paint` suppresses exactly the matrices whose children could not
    /// occupy a single pixel — Flutter's `det == 0 || !det.isFinite`
    /// short-circuit in `RenderTransform.paint`.
    ///
    /// The non-finite half is the reason this cannot delegate to
    /// [`Matrix4::is_invertible`]: that gates on `det.abs() >= f32::EPSILON`,
    /// which an infinite determinant satisfies. The `INFINITY` assertions
    /// below fail against that shortcut and pass against the explicit test.
    #[test]
    fn skip_paint_true_for_singular_and_non_finite_matrices() {
        assert!(RenderTransform::scale(0.0, 0.0).skip_paint());
        assert!(RenderTransform::scale(0.0, 1.0).skip_paint());
        assert!(RenderTransform::scale(1.0, 0.0).skip_paint());

        assert!(RenderTransform::scale(f32::NAN, 1.0).skip_paint());
        assert!(RenderTransform::scale(f32::INFINITY, 1.0).skip_paint());
        assert!(RenderTransform::scale(f32::NEG_INFINITY, 1.0).skip_paint());

        // Small but visible, and the identity: both must still paint. Without
        // these an unconditional `true` would satisfy every case above.
        assert!(!RenderTransform::scale(0.01, 0.01).skip_paint());
        assert!(!RenderTransform::identity().skip_paint());
    }

    /// Exactly one of `paint_effects`'s `transform` field and `paint`'s
    /// `paint_child_at` offset carries the matrix — selected by
    /// translation-ness — which is what prevents double application now that
    /// the field can be `Some`.
    ///
    /// Before the composited-layer-update wiring, the field stayed at its
    /// `None` default unconditionally, because `paint` pushed the matrix
    /// itself for every case; a `Some` here would have wrapped the child in it
    /// twice. Now the pipeline pushes `paint_effects`'s layer BEFORE
    /// replaying `paint`'s fragment, so a non-translation matrix must come
    /// from `paint_effects` (and `paint` must only splice the child, not
    /// push it again — see `paint`'s own doc), while a translation must keep
    /// coming from `paint`'s plain offset (and the `transform` field must
    /// stay `None` for it, or the pipeline would push a redundant no-op
    /// layer). Transform symmetry — paint, coordinate mapping, and hit-test
    /// all reading the SAME `effective_transform` — is asserted through
    /// `apply_paint_transform`, which stays unconditional across both
    /// branches (see its own doc).
    ///
    /// The "exactly one" half is asserted against `paint`'s ACTUAL recorded
    /// fragment, not inferred: re-adding a `with_transform` scope to `paint`
    /// would leave every other assertion in this module green, since none of
    /// them runs `paint` at all. The only other oracle for it lives a crate
    /// away, in `pipeline_scenarios`' layer-structure list.
    #[test]
    fn paint_and_hit_test_share_one_transform() {
        let mut node = RenderTransform::scale(2.0, 2.0);
        node.has_child = true;
        // size ZERO ⇒ CENTER-alignment origin is (0,0), so the effective
        // matrix is the pure scale — non-translation.
        let size = Size::ZERO;

        assert_eq!(
            RenderBox::paint_effects(&node, size).transform,
            Some(node.effective_transform(size)),
            "a non-translation matrix must come from paint_effects, which the \
             pipeline wraps in a layer before replaying paint's fragment",
        );

        let mut mapped = Matrix4::IDENTITY;
        node.apply_paint_transform(0, Offset::ZERO, size, &mut mapped);
        assert_eq!(
            mapped,
            node.effective_transform(size),
            "coordinate mapping must fold in the SAME matrix hit-test inverts",
        );

        let inverse = node
            .effective_transform(size)
            .try_inverse()
            .expect("scale(2,2) is invertible");
        let (tx, ty) = inverse.transform_point(
            flui_types::geometry::px(80.0),
            flui_types::geometry::px(60.0),
        );
        assert!((tx.get() - 40.0).abs() < 1e-4, "tx = {tx:?}");
        assert!((ty.get() - 30.0).abs() < 1e-4, "ty = {ty:?}");

        // The other half of "exactly one", read off `paint`'s real fragment:
        // a non-translation node must splice its child and push NOTHING, since
        // the pipeline already pushed `paint_effects`'s layer around this
        // fragment. A `with_transform` here would record `PushTransform`/`Pop`
        // around the child and apply the matrix twice.
        let ops = capture_paint_ops(&node, size);
        assert_eq!(
            ops,
            vec!["child"],
            "a non-translation paint must only splice the child; got {ops:?}",
        );

        // And the translation branch is the mirror: no layer from the hook, the
        // matrix carried by the child's offset instead.
        let mut translating = RenderTransform::translate(7.0, 9.0);
        translating.has_child = true;
        assert_eq!(
            RenderBox::paint_effects(&translating, size).transform,
            None,
            "a pure translation must NOT report a layer — that is the fast path",
        );
        assert_eq!(
            capture_paint_ops(&translating, size),
            vec!["child"],
            "and it still records exactly one op: the child, at an offset",
        );
    }

    /// The op kinds `paint` records, in order — enough to tell a bare child
    /// splice from one wrapped in a transform scope.
    fn capture_paint_ops(node: &RenderTransform, size: Size) -> Vec<&'static str> {
        use flui_rendering::context::{FragmentOp, FragmentRecorder, PaintCx};

        let mut rec = FragmentRecorder::new(Offset::ZERO, 1.0);
        {
            let mut cx = PaintCx::<Single>::new(&mut rec, 1, size);
            RenderBox::paint(node, &mut cx);
        }
        rec.finish()
            .ops()
            .iter()
            .map(|op| match op {
                FragmentOp::Run(_) => "run",
                FragmentOp::Push(_) => "push",
                FragmentOp::PushTransform(_) => "push_transform",
                FragmentOp::Pop => "pop",
                FragmentOp::Child { .. } => "child",
            })
            .collect()
    }

    /// `paint_effects`'s `transform` field per class: no child, translation,
    /// non-translation, singular.
    ///
    /// This is what actually feeds the paint driver's decision to push a
    /// `TransformLayer` (and, via `own_effect_layers`, what an update-only
    /// patch rebuilds) — pinning it directly guards against a regression the
    /// narrower scale-only check in `paint_and_hit_test_share_one_transform`
    /// cannot see.
    ///
    /// The singular case reports `Some`, not `None`: `paint_effects` does
    /// not itself gate on `skip_paint` — the paint driver does, returning
    /// before it ever calls `paint_effects` this frame (see
    /// `paint_subtree_impl`'s `skip_paint` early return) — so keeping the
    /// value's own contract uniform ("non-translation ⇒ `Some`") is honest
    /// rather than a redundant special case, and the singular exclusion lives
    /// in exactly the one place (`skip_paint`) that already owns it.
    #[test]
    fn paint_effects_reports_a_transform_only_for_a_non_translation_matrix_with_a_child() {
        let size = Size::new(px(40.0), px(40.0));

        let mut no_child = RenderTransform::scale(2.0, 2.0);
        no_child.has_child = false;
        assert_eq!(
            RenderBox::paint_effects(&no_child, size).transform,
            None,
            "no child, no paint, no layer",
        );

        let mut translation = RenderTransform::translate(5.0, 7.0);
        translation.has_child = true;
        assert_eq!(
            RenderBox::paint_effects(&translation, size).transform,
            None,
            "a pure translation is applied as a plain offset in paint",
        );

        let mut non_translation = RenderTransform::scale(2.0, 3.0);
        non_translation.has_child = true;
        assert_eq!(
            RenderBox::paint_effects(&non_translation, size).transform,
            Some(non_translation.effective_transform(size)),
        );

        let mut singular = RenderTransform::scale(0.0, 0.0);
        singular.has_child = true;
        assert!(
            singular.skip_paint(),
            "precondition: scale(0,0) is singular"
        );
        assert_eq!(
            RenderBox::paint_effects(&singular, size).transform,
            Some(singular.effective_transform(size)),
            "paint_effects answers uniformly; the driver never asks it this \
             question this frame because skip_paint short-circuits first",
        );
    }

    /// `set_transform`'s impact algebra: `COMPOSITED_LAYER_UPDATE` only when
    /// the node owns a `TransformLayer` both BEFORE and AFTER the change; a
    /// crossing — translation ↔ non-translation, or singular ↔ non-singular —
    /// always reports `PAINT` instead, because the set of layers a patch
    /// could address changed shape.
    #[test]
    fn set_transform_reports_layer_update_only_within_the_layered_range() {
        let mut node = RenderTransform::scale(2.0, 2.0);
        node.has_child = true;

        // Non-translation -> non-translation: stays within the layered range.
        assert_eq!(
            node.set_transform(Matrix4::scaling(3.0, 3.0, 1.0)),
            flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
        );

        // Non-translation -> translation: the layer disappears (structural).
        assert_eq!(
            node.set_transform(Matrix4::translation(5.0, 5.0, 0.0)),
            flui_rendering::RenderUpdateImpact::PAINT
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
        );

        // Translation -> non-translation: the layer appears (structural).
        assert_eq!(
            node.set_transform(Matrix4::scaling(4.0, 4.0, 1.0)),
            flui_rendering::RenderUpdateImpact::PAINT
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
        );

        // Non-translation -> singular: the layer disappears via skip_paint
        // (structural).
        assert_eq!(
            node.set_transform(Matrix4::scaling(0.0, 0.0, 1.0)),
            flui_rendering::RenderUpdateImpact::PAINT
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
        );

        // Singular -> non-singular non-translation: the layer appears
        // (structural).
        assert_eq!(
            node.set_transform(Matrix4::scaling(5.0, 5.0, 1.0)),
            flui_rendering::RenderUpdateImpact::PAINT
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
        );

        // Identical value: no-op.
        assert_eq!(
            node.set_transform(Matrix4::scaling(5.0, 5.0, 1.0)),
            flui_rendering::RenderUpdateImpact::NONE,
        );

        // Childless: never owns a layer on either side, so always PAINT —
        // matches the pre-existing (has_child-blind) behaviour.
        let mut childless = RenderTransform::scale(2.0, 2.0);
        assert!(!childless.has_child);
        assert_eq!(
            childless.set_transform(Matrix4::scaling(3.0, 3.0, 1.0)),
            flui_rendering::RenderUpdateImpact::PAINT
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
        );
    }

    /// `set_alignment`/`set_origin` are pivot-only: `owns_effect_layer`'s
    /// predicate cannot flip on either, so both always land on the SAME side
    /// (`COMPOSITED_LAYER_UPDATE` or `PAINT`) the node was already on.
    #[test]
    fn set_alignment_and_set_origin_track_whether_the_node_owns_a_layer() {
        // Owns a layer throughout: a scale with a child.
        let mut layered = RenderTransform::scale(2.0, 2.0);
        layered.has_child = true;
        assert!(layered.owns_effect_layer(), "precondition");
        assert_eq!(
            layered.set_alignment(Some(Alignment::BOTTOM_RIGHT)),
            flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
        );
        assert_eq!(
            layered.set_origin(Some(Offset::new(px(3.0), px(4.0)))),
            flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
        );

        // Owns no layer throughout: a translation never does, whatever the
        // pivot — the predicate cannot flip TO true either.
        let mut translating = RenderTransform::translate(10.0, 10.0);
        translating.has_child = true;
        assert!(!translating.owns_effect_layer(), "precondition");
        assert_eq!(
            translating.set_alignment(Some(Alignment::CENTER)),
            flui_rendering::RenderUpdateImpact::PAINT
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
        );
        assert_eq!(
            translating.set_origin(Some(Offset::new(px(1.0), px(1.0)))),
            flui_rendering::RenderUpdateImpact::PAINT
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
        );
    }

    #[test]
    fn compute_origin_combines_alignment_and_origin() {
        // Flutter applies BOTH origin and alignment additively: the pivot is
        // `alignment.alongSize(size) + origin`. 100×100 with CENTER alignment
        // (alongSize = (50,50)) plus origin (10,0) → (60,50). Before the fix the
        // explicit origin won outright and the alignment contribution was
        // dropped → (10,0).
        let node = RenderTransform::scale(2.0, 2.0)
            .with_alignment(Alignment::CENTER)
            .with_origin(Offset::new(px(10.0), px(0.0)));
        assert_eq!(
            node.compute_origin(Size::new(px(100.0), px(100.0))),
            Offset::new(px(60.0), px(50.0)),
        );
    }

    use std::f32::consts::PI;

    use flui_types::geometry::px;

    use super::*;

    #[test]
    fn test_transform_identity() {
        let transform = RenderTransform::identity();
        assert_eq!(transform.transform(), &Matrix4::IDENTITY);
    }

    #[test]
    fn test_transform_translate() {
        let transform = RenderTransform::translate(10.0, 20.0);
        let expected = Matrix4::translation(10.0, 20.0, 0.0);
        assert_eq!(transform.transform(), &expected);
    }

    #[test]
    fn test_transform_scale() {
        let transform = RenderTransform::scale(2.0, 3.0);
        let expected = Matrix4::scaling(2.0, 3.0, 1.0);
        assert_eq!(transform.transform(), &expected);
    }

    #[test]
    fn test_transform_uniform_scale() {
        let transform = RenderTransform::uniform_scale(0.5);
        let expected = Matrix4::scaling(0.5, 0.5, 1.0);
        assert_eq!(transform.transform(), &expected);
    }

    #[test]
    fn test_transform_rotation() {
        let transform = RenderTransform::rotation(PI / 2.0);
        // Should be 90 degree rotation - m[0] is m11 in column-major order
        assert!((transform.transform().m[0] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_transform_rotation_degrees() {
        let transform = RenderTransform::rotation_degrees(90.0);
        // Should be same as PI/2 radians - m[0] is m11 in column-major order
        assert!((transform.transform().m[0] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_transform_with_alignment() {
        let transform = RenderTransform::scale(2.0, 2.0).with_alignment(Alignment::TOP_LEFT);
        assert_eq!(transform.alignment(), Some(Alignment::TOP_LEFT));
    }

    #[test]
    fn test_transform_with_origin() {
        let origin = Offset::new(px(50.0), px(50.0));
        let transform = RenderTransform::scale(2.0, 2.0).with_origin(origin);
        assert_eq!(transform.origin(), Some(origin));
    }

    /// Chaining all three setters the way `Transform::update_render_object`
    /// does yields a THREE-bit union, not two.
    ///
    /// `identity()` is a translation, so `set_transform` crosses INTO the
    /// layered range (PAINT: the layer newly appears) — but by the time
    /// `set_alignment` and `set_origin` run, `self` already carries the new
    /// scaling matrix from the line before, so each of them observes a node
    /// that already owns a layer and reports `COMPOSITED_LAYER_UPDATE`. The
    /// union is accepted and documented rather than engineered away: this
    /// matches the shipped `RenderOpacity::set_opacity`, which likewise unions
    /// a structural bit with the layer-update bit and lets paint win in
    /// `apply_render_update_impact`. Behaviour is identical either way; only
    /// the bit pattern differs, and `PAINT` already implies the repaint that
    /// makes the extra bit harmless.
    ///
    /// The exact-value assertion below therefore pins the widget's CALL ORDER
    /// as well as the impacts. That is deliberate but easy to trip over, so the
    /// second half reorders the setters and asserts the property that actually
    /// matters — a repaint is requested either way. Swapping the order in
    /// `Transform::update_render_object` is behaviourally free; if you do it,
    /// the first assertion is the one to update, and it is not reporting a bug.
    #[test]
    fn targeted_setters_preserve_child_layout_state() {
        let mut transform = RenderTransform::identity();
        transform.has_child = true;
        let impact = transform.set_transform(Matrix4::scaling(2.0, 2.0, 1.0))
            | transform.set_alignment(Some(Alignment::BOTTOM_RIGHT))
            | transform.set_origin(Some(Offset::new(px(2.0), px(3.0))));
        assert_eq!(
            impact,
            flui_rendering::RenderUpdateImpact::PAINT
                | flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
                | flui_rendering::RenderUpdateImpact::SEMANTICS
        );
        assert!(transform.has_child);

        // Pivot first, matrix second — the same three changes, and the union
        // still demands a repaint. Only the bit pattern moves.
        let mut reordered = RenderTransform::identity();
        reordered.has_child = true;
        let swapped = reordered.set_alignment(Some(Alignment::BOTTOM_RIGHT))
            | reordered.set_origin(Some(Offset::new(px(2.0), px(3.0))))
            | reordered.set_transform(Matrix4::scaling(2.0, 2.0, 1.0));
        assert!(
            swapped.needs_paint() && swapped.needs_semantics_update(),
            "the order the widget happens to use must not change whether a \
             repaint is requested; got {swapped:?}",
        );
    }

    #[test]
    fn test_default() {
        let transform = RenderTransform::default();
        assert_eq!(transform.transform(), &Matrix4::IDENTITY);
    }
}
