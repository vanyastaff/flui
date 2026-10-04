//! RenderTransform - applies a transformation matrix to a single child.

use flui_foundation::Single;
use flui_foundation::geometry::{Matrix4, Offset, Size};
use flui_painting::Alignment;

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
/// let transform = RenderTransform::rotation(std::f64::consts::PI / 4.0);
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
    /// alone. The bare constructor leaves it `None` while the
    /// `rotate`/`scale`/`flip` factories pass `Alignment::CENTER` explicitly.
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
    /// way.
    transform_hit_tests: bool,
}

impl RenderTransform {
    /// Creates a new transform render object with the given matrix.
    pub fn new(transform: Matrix4) -> Self {
        Self {
            transform,
            // The bare constructor leaves `alignment` unset; the named
            // factories below opt into CENTER.
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
    pub fn translate(x: f64, y: f64) -> Self {
        Self::new(Matrix4::translation(x, y, 0.0))
    }

    /// Creates a scale transform about the box's centre.
    ///
    /// Centre, not the bare constructor's absent alignment — `Transform.scale`
    /// passes `Alignment.center` explicitly, and a scale about the top-left
    /// corner is almost never what a caller means.
    pub fn scale(sx: f64, sy: f64) -> Self {
        Self::new(Matrix4::scaling(sx, sy, 1.0)).with_alignment(Alignment::CENTER)
    }

    /// Creates a uniform scale transform.
    pub fn uniform_scale(scale: f64) -> Self {
        Self::scale(scale, scale)
    }

    /// Creates a rotation transform around the Z axis.
    ///
    /// # Arguments
    ///
    /// * `radians` - Rotation angle in radians.
    pub fn rotation(radians: f64) -> Self {
        // `Transform.rotate` passes `Alignment.center` explicitly, for the
        // same reason `scale` does.
        Self::new(Matrix4::rotation_z(radians)).with_alignment(Alignment::CENTER)
    }

    /// Creates a rotation transform from degrees.
    pub fn rotation_degrees(degrees: f64) -> Self {
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
    /// `Matrix4::translation(f64::INFINITY, 0.0, 0.0)` is `Some` before
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
    /// whole pivot.
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
    /// The origin AND the alignment apply **additively** —
    /// `T(origin)·T(alignment.along_size)·transform·…` — so the pivot is
    /// `alignment.along_size(size) + origin`, not one or the other. When
    /// `alignment` is present it always contributes; `origin` is optional.
    fn compute_origin(&self, size: Size) -> Offset {
        let origin = self.origin.unwrap_or(Offset::ZERO);
        let Some(alignment) = self.alignment else {
            // No alignment term: an `origin` set alone acts alone.
            return origin;
        };
        let align_x = size.width * f64::midpoint(alignment.x, 1.0);
        let align_y = size.height * f64::midpoint(alignment.y, 1.0);
        Offset::new(align_x + origin.dx, align_y + origin.dy)
    }

    /// Computes the effective transform matrix with origin applied, using
    /// the laid-out `size` for an alignment-relative origin.
    fn effective_transform(&self, size: Size) -> Matrix4 {
        let origin = self.compute_origin(size);

        // Translate to origin, apply transform, translate back
        let to_origin = Matrix4::translation(-origin.dx, -origin.dy, 0.0);
        let from_origin = Matrix4::translation(origin.dx, origin.dy, 0.0);

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
        // is ill-defined, so only the child decides. A scale(2) child visually
        // covering 80×80 must be hittable across that whole area even
        // though this node's laid-out size is 40×40.
        if !self.has_child {
            return false;
        }
        if !self.transform_hit_tests {
            // The child is hit where it was LAID OUT, not where it paints
            // (`transform_hit_tests: false`). A degenerate matrix does
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
        // If the matrix is singular the children would be compressed to a
        // line or single point, so paint nothing (`det == 0 || !det.is_finite()`).
        // Painting such a subtree records draw commands and composites layers for output that cannot occupy
        // a single pixel.
        //
        // `self.transform`, not `effective_transform(size)`: the effective
        // matrix only wraps this one in origin translations, and a
        // translation's determinant is 1, so the two determinants are equal.
        // That is what lets the check live on `skip_paint`, which is handed
        // no laid-out size.
        //
        // Painting rejects a collapsed or non-finite determinant. Hit testing
        // separately requires the computed finite inverse (ADR-0113), so a
        // finite determinant alone does not promise a usable inverse.
        let determinant = self.transform.determinant();
        determinant == 0.0 || !determinant.is_finite()
    }

    /// Paints the child through the effective transform — or, when that
    /// transform is a pure translation, at a plain offset with no layer at all.
    ///
    /// A matrix that only translates is applied as a plain child offset, with
    /// no layer. A compositing layer per
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
    /// below regardless of which branch painted.
    fn paint(&self, ctx: &mut flui_rendering::context::PaintCx<'_, Single>) {
        if !self.has_child {
            return;
        }

        let transform = self.effective_transform(ctx.size());
        if let Some((dx, dy)) = transform.as_translation() {
            ctx.paint_child_at(Offset::new(dx, dy));
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
    /// and every hero flight built on them. Hence unconditional here.
    fn apply_paint_transform(
        &self,
        _child: usize,
        child_offset: Offset,
        size: Size,
        transform: &mut Matrix4,
    ) {
        *transform *= self.effective_transform(size);
        *transform *= Matrix4::translation(child_offset.dx, child_offset.dy, 0.0);
    }

    fn hit_test_transform(&self, size: Size) -> Option<Matrix4> {
        // `None` when hit-testing is untransformed, and the two halves have to
        // agree or the flag is worse than useless.
        //
        // This hook feeds the hit ENTRY's transform stack — the global-to-local
        // mapping a delivered event is localized through
        // (`PipelineOwner::hit_test_subtree` pushes its inverse), and is unset
        // by `transform_hit_tests: false` — NOT like `apply_paint_transform`,
        // which stays unconditional because it answers a different question
        // (where the child paints, for local-to-global).
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
