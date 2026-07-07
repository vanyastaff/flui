//! RenderTransform - applies a transformation matrix to a single child.

use flui_tree::Single;
use flui_types::{Alignment, Matrix4, Offset, Size};

use flui_rendering::{
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    traits::RenderBox,
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
    /// Origin for the transformation as alignment.
    alignment: Alignment,
    /// Explicit origin offset (overrides alignment if set).
    origin: Option<Offset>,
    /// Whether we have a child.
    has_child: bool,
    /// Whether to use compositing layers.
    needs_compositing: bool,
}

impl RenderTransform {
    /// Creates a new transform render object with the given matrix.
    pub fn new(transform: Matrix4) -> Self {
        Self {
            transform,
            alignment: Alignment::CENTER,
            origin: None,
            has_child: false,
            needs_compositing: true,
        }
    }

    /// Creates an identity transform (no transformation).
    pub fn identity() -> Self {
        Self::new(Matrix4::IDENTITY)
    }

    /// Creates a translation transform.
    pub fn translate(x: f32, y: f32) -> Self {
        Self::new(Matrix4::translation(x, y, 0.0))
    }

    /// Creates a scale transform.
    pub fn scale(sx: f32, sy: f32) -> Self {
        Self::new(Matrix4::scaling(sx, sy, 1.0))
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
        Self::new(Matrix4::rotation_z(radians))
    }

    /// Creates a rotation transform from degrees.
    pub fn rotation_degrees(degrees: f32) -> Self {
        Self::rotation(degrees.to_radians())
    }

    /// Returns the transformation matrix.
    pub fn transform(&self) -> &Matrix4 {
        &self.transform
    }

    /// Sets the transformation matrix.
    pub fn set_transform(&mut self, transform: Matrix4) {
        self.transform = transform;
    }

    /// Sets the alignment for the transform origin.
    pub fn with_alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self.origin = None;
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
    pub fn set_needs_compositing(&mut self, value: bool) {
        self.needs_compositing = value;
    }

    /// Returns the alignment.
    pub fn alignment(&self) -> Alignment {
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
        let align_x = size.width * f32::midpoint(self.alignment.x, 1.0);
        let align_y = size.height * f32::midpoint(self.alignment.y, 1.0);
        let origin = self.origin.unwrap_or(Offset::ZERO);
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
        properties.add_enum("alignment", self.alignment);
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

    // paint() uses default no-op - transform is applied via paint_transform()

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

        // The pipeline pushes the forward transform onto HitTestResult
        // via hit_test_transform() before calling hit_test_raw, so
        // child entries capture the correct accumulated transform.
        // No push/pop needed here.
        ctx.hit_test_child(0, Offset::new(tx, ty))
    }

    // The whole point of RenderTransform: the pipeline reads these through
    // `&dyn RenderObject<BoxProtocol>`; the blanket impl forwards here.
    fn paint_transform(&self, size: Size) -> Option<Matrix4> {
        // Return the effective transform so the paint walk can apply it.
        // `size` is the laid-out size from RenderState (origin pivot).
        Some(self.effective_transform(size))
    }

    fn hit_test_transform(&self, size: Size) -> Option<Matrix4> {
        Some(self.effective_transform(size))
    }
}

#[cfg(test)]
mod tests {

    /// Transform symmetry: `paint_transform` hands the pipeline the
    /// SAME matrix hit-test inverts (`effective_transform` both ways),
    /// and the inverse maps a visual point to the child-local point.
    #[test]
    fn paint_and_hit_test_share_one_transform() {
        let mut node = RenderTransform::scale(2.0, 2.0);
        node.has_child = true;
        // size ZERO ⇒ CENTER-alignment origin is (0,0), so the effective
        // matrix is the pure scale; both hooks return that same matrix.
        let size = Size::ZERO;
        assert_eq!(
            node.paint_transform(size),
            Some(node.effective_transform(size))
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
        assert_eq!(transform.alignment(), Alignment::TOP_LEFT);
    }

    #[test]
    fn test_transform_with_origin() {
        let origin = Offset::new(px(50.0), px(50.0));
        let transform = RenderTransform::scale(2.0, 2.0).with_origin(origin);
        assert_eq!(transform.origin(), Some(origin));
    }

    #[test]
    fn test_default() {
        let transform = RenderTransform::default();
        assert_eq!(transform.transform(), &Matrix4::IDENTITY);
    }
}
