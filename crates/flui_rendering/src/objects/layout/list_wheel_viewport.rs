//! RenderListWheelViewport - 3D wheel picker viewport

use crate::core::{
    BoxProtocol, ChildrenAccess, LayoutContext, PaintContext, RenderBox, Variable,
};
use flui_types::constraints::BoxConstraints;
use flui_types::prelude::*;
use std::f32::consts::PI;

/// RenderObject for a 3D cylindrical scrolling viewport (wheel picker)
///
/// Creates a 3D effect where items appear to wrap around a cylinder,
/// with perspective and rotation. Commonly used for iOS-style pickers.
///
/// # Features
///
/// - 3D cylindrical perspective effect
/// - Configurable item extent (height of each item)
/// - Configurable diameter ratio (affects curvature)
/// - Configurable perspective (depth effect)
/// - Off-axis fraction for tilting the cylinder
/// - Squeeze factor for compressing items vertically
///
/// # Use Cases
///
/// - iOS-style date/time pickers
/// - Settings wheels
/// - Any cylindrical scrolling interface
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::objects::layout::RenderListWheelViewport;
///
/// let viewport = RenderListWheelViewport::new(50.0);
/// ```
#[derive(Debug)]
pub struct RenderListWheelViewport {
    /// The size of each child along the main axis (height for vertical)
    pub item_extent: f32,
    /// The diameter of the cylinder as a fraction of viewport size
    /// Default: 1.0 (diameter equals viewport height)
    /// Smaller values create tighter curves, larger values create gentler curves
    pub diameter_ratio: f32,
    /// The perspective effect intensity (0.0 = no perspective, higher = more depth)
    /// Default: 0.003 (subtle 3D effect)
    pub perspective: f32,
    /// Horizontal offset fraction for tilting the cylinder (-1.0 to 1.0)
    /// 0.0 = centered, -1.0 = tilted left, 1.0 = tilted right
    pub off_axis_fraction: f32,
    /// Whether to use magnification effect for center item
    pub use_magnifier: bool,
    /// Magnification factor for center item (1.0 = no magnification)
    pub magnification: f32,
    /// Squeeze factor for vertical compression (1.0 = no squeeze)
    pub squeeze: f32,
    /// Current scroll offset in pixels
    pub scroll_offset: f32,

    // Cache for paint
    child_offsets: Vec<Offset>,
    child_transforms: Vec<Option<Matrix4>>,
}

impl RenderListWheelViewport {
    /// Create new list wheel viewport
    ///
    /// # Arguments
    /// * `item_extent` - Height of each item in pixels
    pub fn new(item_extent: f32) -> Self {
        Self {
            item_extent,
            diameter_ratio: 1.0,
            perspective: 0.003,
            off_axis_fraction: 0.0,
            use_magnifier: false,
            magnification: 1.0,
            squeeze: 1.0,
            scroll_offset: 0.0,
            child_offsets: Vec::new(),
            child_transforms: Vec::new(),
        }
    }

    /// Set diameter ratio
    pub fn with_diameter_ratio(mut self, ratio: f32) -> Self {
        self.diameter_ratio = ratio;
        self
    }

    /// Set perspective intensity
    pub fn with_perspective(mut self, perspective: f32) -> Self {
        self.perspective = perspective;
        self
    }

    /// Set off-axis fraction (tilt)
    pub fn with_off_axis_fraction(mut self, fraction: f32) -> Self {
        self.off_axis_fraction = fraction.clamp(-1.0, 1.0);
        self
    }

    /// Enable magnification effect
    pub fn with_magnification(mut self, magnification: f32) -> Self {
        self.use_magnifier = true;
        self.magnification = magnification;
        self
    }

    /// Set squeeze factor
    pub fn with_squeeze(mut self, squeeze: f32) -> Self {
        self.squeeze = squeeze;
        self
    }

    /// Calculate the 3D transform for an item at the given index
    ///
    /// # Arguments
    /// * `index` - Item index
    /// * `viewport_height` - Height of the viewport
    ///
    /// # Returns
    /// Transform matrix and offset for the item
    fn calculate_item_transform(
        &self,
        index: usize,
        viewport_height: f32,
    ) -> (Matrix4, Offset, f32) {
        // Calculate the diameter of the cylinder
        let diameter = viewport_height * self.diameter_ratio;
        let radius = diameter / 2.0;

        // Calculate the vertical position of this item in the scrollable content
        let item_offset_in_content = index as f32 * self.item_extent;

        // Calculate the vertical offset relative to the scroll position
        // This determines where the item appears in the viewport
        let offset_from_scroll_origin = item_offset_in_content - self.scroll_offset;

        // Calculate the angle this item makes on the cylinder
        // The angle is based on how far the item is from the center
        let theta = offset_from_scroll_origin / radius;

        // Calculate the vertical position (y) using sine
        // Items at the center (theta = 0) are at y = 0
        // Items above/below curve away based on the cylinder's radius
        let y = radius * theta.sin();

        // Calculate the z-depth using cosine
        // Items at the center (theta = 0) are at z = radius (closest)
        // Items at top/bottom curve back (smaller z)
        let z = radius * (1.0 - theta.cos());

        // Calculate the x position based on off-axis fraction
        let x = viewport_height * self.off_axis_fraction;

        // Calculate scale based on perspective and z-depth
        let scale = 1.0 / (1.0 + self.perspective * z);

        // Apply squeeze factor to vertical scale
        let scale_y = scale * self.squeeze;

        // Apply magnification if enabled and item is near center
        let final_scale = if self.use_magnifier {
            let distance_from_center = theta.abs();
            if distance_from_center < PI / 4.0 {
                // Gradually increase scale for items near center
                let magnification_factor =
                    1.0 + (self.magnification - 1.0) * (1.0 - distance_from_center / (PI / 4.0));
                scale * magnification_factor
            } else {
                scale
            }
        } else {
            scale
        };

        // Create transform matrix
        // Order: Scale -> Translate
        let transform = Matrix4::scaling(final_scale, scale_y, 1.0);

        // The offset is where to position the item in the viewport
        // Center the item vertically in the viewport and apply y offset
        let offset = Offset::new(x, viewport_height / 2.0 + y - self.item_extent / 2.0);

        (transform, offset, final_scale)
    }

    /// Check if an item is visible in the viewport
    #[allow(dead_code)]
    fn is_item_visible(&self, offset: Offset, viewport_height: f32) -> bool {
        let item_top = offset.dy;
        let item_bottom = offset.dy + self.item_extent;

        // Item is visible if it overlaps the viewport (0 to viewport_height)
        item_bottom >= 0.0 && item_top <= viewport_height
    }
}

impl Default for RenderListWheelViewport {
    fn default() -> Self {
        Self::new(50.0) // Default item height
    }
}

impl RenderBox<Variable> for RenderListWheelViewport {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Variable, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let constraints = ctx.constraints;
        let children = ctx.children;

        // Viewport takes all available space
        let size = constraints.biggest();

        if children.as_slice().is_empty() {
            self.child_offsets.clear();
            self.child_transforms.clear();
            return size;
        }

        // Clear cache
        self.child_offsets.clear();
        self.child_transforms.clear();

        // Each child has fixed extent along main axis, full extent along cross axis
        let child_constraints =
            BoxConstraints::new(0.0, size.width, self.item_extent, self.item_extent);

        // Layout all children and calculate their transforms
        for (index, child_id) in children.iter().enumerate() {
            // Layout child with fixed constraints
            let _child_size = ctx.layout_child(child_id, child_constraints);

            // Calculate 3D transform and offset
            let (transform, offset, _scale) = self.calculate_item_transform(index, size.height);

            // Store offset and transform for painting
            self.child_offsets.push(offset);
            self.child_transforms.push(Some(transform));
        }

        size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Variable>)
    where
        T: crate::core::PaintTree,
    {
        let offset = ctx.offset;

        // Collect child IDs first to avoid borrow checker issues
        let child_ids: Vec<_> = ctx.children.iter().collect();

        if child_ids.is_empty() {
            return;
        }

        // Paint children in order (back to front for proper layering)
        // Items further back (higher z) should be painted first
        for (index, child_id) in child_ids.into_iter().enumerate() {
            if index >= self.child_offsets.len() {
                break;
            }

            let child_offset = self.child_offsets[index];

            // Paint child at calculated offset
            // TODO: Apply 3D transform when transform layer support is available
            // For now, just using the calculated offsets for cylindrical positioning
            // Visibility culling would be done based on scroll position, but for now
            // we paint all children (could be optimized later)
            ctx.paint_child(child_id, offset + child_offset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_list_wheel_viewport_new() {
        let viewport = RenderListWheelViewport::new(50.0);

        assert_eq!(viewport.item_extent, 50.0);
        assert_eq!(viewport.diameter_ratio, 1.0);
        assert_eq!(viewport.perspective, 0.003);
        assert_eq!(viewport.off_axis_fraction, 0.0);
        assert!(!viewport.use_magnifier);
        assert_eq!(viewport.magnification, 1.0);
        assert_eq!(viewport.squeeze, 1.0);
        assert_eq!(viewport.scroll_offset, 0.0);
    }

    #[test]
    fn test_render_list_wheel_viewport_default() {
        let viewport = RenderListWheelViewport::default();

        assert_eq!(viewport.item_extent, 50.0);
    }

    #[test]
    fn test_with_diameter_ratio() {
        let viewport = RenderListWheelViewport::new(50.0).with_diameter_ratio(1.5);

        assert_eq!(viewport.diameter_ratio, 1.5);
    }

    #[test]
    fn test_with_perspective() {
        let viewport = RenderListWheelViewport::new(50.0).with_perspective(0.005);

        assert_eq!(viewport.perspective, 0.005);
    }

    #[test]
    fn test_with_off_axis_fraction() {
        let viewport = RenderListWheelViewport::new(50.0).with_off_axis_fraction(0.5);

        assert_eq!(viewport.off_axis_fraction, 0.5);
    }

    #[test]
    fn test_with_off_axis_fraction_clamped() {
        let viewport = RenderListWheelViewport::new(50.0).with_off_axis_fraction(2.0);

        assert_eq!(viewport.off_axis_fraction, 1.0); // Clamped to 1.0
    }

    #[test]
    fn test_with_magnification() {
        let viewport = RenderListWheelViewport::new(50.0).with_magnification(1.2);

        assert!(viewport.use_magnifier);
        assert_eq!(viewport.magnification, 1.2);
    }

    #[test]
    fn test_with_squeeze() {
        let viewport = RenderListWheelViewport::new(50.0).with_squeeze(0.8);

        assert_eq!(viewport.squeeze, 0.8);
    }

    #[test]
    fn test_calculate_item_transform_center() {
        let viewport = RenderListWheelViewport::new(50.0);
        let viewport_height = 300.0;

        // Calculate for center item (assuming scroll offset aligns it)
        // For a centered item, theta should be close to 0
        let (transform, offset, scale) = viewport.calculate_item_transform(0, viewport_height);

        // Center item should have scale close to 1.0
        assert!(scale > 0.9 && scale <= 1.0);

        // Offset should be near viewport center
        assert!(offset.dy > viewport_height / 2.0 - viewport.item_extent);
        assert!(offset.dy < viewport_height / 2.0 + viewport.item_extent);

        // Transform should be a scaling matrix
        assert!(transform.m[0] > 0.0); // Has x-scale
        assert!(transform.m[5] > 0.0); // Has y-scale
    }

    #[test]
    fn test_is_item_visible() {
        let viewport = RenderListWheelViewport::new(50.0);
        let viewport_height = 300.0;

        // Item fully visible in middle
        assert!(viewport.is_item_visible(Offset::new(0.0, 125.0), viewport_height));

        // Item at top edge
        assert!(viewport.is_item_visible(Offset::new(0.0, 0.0), viewport_height));

        // Item at bottom edge
        assert!(viewport.is_item_visible(Offset::new(0.0, 250.0), viewport_height));

        // Item completely above viewport
        assert!(!viewport.is_item_visible(Offset::new(0.0, -100.0), viewport_height));

        // Item completely below viewport
        assert!(!viewport.is_item_visible(Offset::new(0.0, 400.0), viewport_height));
    }
}
