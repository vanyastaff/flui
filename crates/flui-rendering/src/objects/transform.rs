//! RenderTransform - applies a transformation matrix to a single child.

use flui_tree::Single;
use flui_types::{Alignment, Matrix4, Offset, Pixels, Point, Rect, Size};

use crate::{
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability},
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
    /// Size after layout.
    size: Size,
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
            size: Size::ZERO,
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

    /// Computes the effective origin offset based on alignment and size.
    fn compute_origin(&self) -> Offset {
        if let Some(origin) = self.origin {
            origin
        } else {
            // Compute from alignment
            let x = self.size.width * ((self.alignment.x + 1.0) / 2.0);
            let y = self.size.height * ((self.alignment.y + 1.0) / 2.0);
            Offset::new(x, y)
        }
    }

    /// Computes the effective transform matrix with origin applied.
    fn effective_transform(&self) -> Matrix4 {
        let origin = self.compute_origin();

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

impl flui_foundation::Diagnosticable for RenderTransform {}
impl RenderBox for RenderTransform {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) {
        let constraints = *ctx.constraints();

        if ctx.child_count() > 0 {
            self.has_child = true;

            // Layout child with same constraints
            let child_size = ctx.layout_child(0, constraints);
            self.size = child_size;

            ctx.complete_with_size(self.size);
        } else {
            self.has_child = false;
            self.size = constraints.smallest();
            ctx.complete_with_size(self.size);
        }
    }

    fn size(&self) -> &Size {
        &self.size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

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
        let Some(inverse) = self.effective_transform().try_inverse() else {
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

        // Record the forward transform for hit entries (R-24 stack composition).
        ctx.push_transform(self.effective_transform());
        let hit = ctx.hit_test_child(0, Offset::new(tx, ty));
        ctx.pop_transform();
        hit
    }

    fn box_paint_bounds(&self) -> Rect {
        // Transform the bounds
        let bounds = Rect::from_origin_size(Point::ZERO, self.size);
        let effective = self.effective_transform();

        // Transform all four corners and compute bounding box
        let corners = [
            Point::new(bounds.min.x, bounds.min.y),
            Point::new(bounds.max.x, bounds.min.y),
            Point::new(bounds.max.x, bounds.max.y),
            Point::new(bounds.min.x, bounds.max.y),
        ];

        let transformed: Vec<(Pixels, Pixels)> = corners
            .iter()
            .map(|p| effective.transform_point(p.x, p.y))
            .collect();

        let min_x = transformed
            .iter()
            .map(|(x, _)| *x)
            .fold(Pixels::INFINITY, Pixels::min);
        let min_y = transformed
            .iter()
            .map(|(_, y)| *y)
            .fold(Pixels::INFINITY, Pixels::min);
        let max_x = transformed
            .iter()
            .map(|(x, _)| *x)
            .fold(Pixels::NEG_INFINITY, Pixels::max);
        let max_y = transformed
            .iter()
            .map(|(_, y)| *y)
            .fold(Pixels::NEG_INFINITY, Pixels::max);

        Rect::from_ltrb(min_x, min_y, max_x, max_y)
    }
}

// Mythos Step 11: PaintEffectsCapability override -- the whole point of
// RenderTransform.
impl PaintEffectsCapability for RenderTransform {
    fn paint_transform(&self) -> Option<Matrix4> {
        // Return the effective transform so paint_node_recursive can apply it
        Some(self.effective_transform())
    }
}

impl SemanticsCapability for RenderTransform {}
impl HotReloadCapability for RenderTransform {}

#[cfg(test)]
mod tests {

    /// Transform symmetry: `paint_transform` hands the pipeline the
    /// SAME matrix hit-test inverts (`effective_transform` both ways),
    /// and the inverse maps a visual point to the child-local point.
    #[test]
    fn paint_and_hit_test_share_one_transform() {
        let mut node = RenderTransform::scale(2.0, 2.0);
        node.has_child = true;
        assert_eq!(node.paint_transform(), Some(node.effective_transform()));

        let inverse = node
            .effective_transform()
            .try_inverse()
            .expect("scale(2,2) is invertible");
        let (tx, ty) = inverse.transform_point(
            flui_types::geometry::px(80.0),
            flui_types::geometry::px(60.0),
        );
        assert!((tx.get() - 40.0).abs() < 1e-4, "tx = {tx:?}");
        assert!((ty.get() - 30.0).abs() < 1e-4, "ty = {ty:?}");
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
