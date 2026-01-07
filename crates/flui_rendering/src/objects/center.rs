//! RenderCenter - centers a single child within available space.

use flui_types::{Offset, Point, Rect, Size};

use crate::arity::Single;
use crate::context::{BoxHitTestContext, BoxLayoutContext};
use crate::parent_data::BoxParentData;
use crate::traits::RenderBox;

/// A render object that centers its child within the available space.
///
/// The child is given loose constraints (can be any size up to parent's max),
/// then positioned in the center of the available space.
///
/// # Example
///
/// ```ignore
/// let center = RenderCenter::new();
/// let mut wrapper = BoxWrapper::new(center);
/// // Add a child, then layout with constraints
/// ```
#[derive(Debug, Clone, Default)]
pub struct RenderCenter {
    /// Width factor (0.0-1.0) to shrink available width, None for full width.
    width_factor: Option<f32>,
    /// Height factor (0.0-1.0) to shrink available height, None for full height.
    height_factor: Option<f32>,
    /// Size after layout.
    size: Size,
    /// Whether we have a child (tracked for hit testing).
    has_child: bool,
    /// Child offset for hit testing.
    child_offset: Offset,
}

impl RenderCenter {
    /// Creates a new center render object.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a center with width factor (shrinks available width).
    pub fn with_width_factor(mut self, factor: f32) -> Self {
        self.width_factor = Some(factor.clamp(0.0, 1.0));
        self
    }

    /// Creates a center with height factor (shrinks available height).
    pub fn with_height_factor(mut self, factor: f32) -> Self {
        self.height_factor = Some(factor.clamp(0.0, 1.0));
        self
    }

    /// Returns the width factor.
    pub fn width_factor(&self) -> Option<f32> {
        self.width_factor
    }

    /// Returns the height factor.
    pub fn height_factor(&self) -> Option<f32> {
        self.height_factor
    }
}

impl flui_foundation::Diagnosticable for RenderCenter {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add("size", format!("{:?}", self.size));
        builder.add("width_factor", format!("{:?}", self.width_factor));
        builder.add("height_factor", format!("{:?}", self.height_factor));
        builder.add("has_child", self.has_child);
    }
}

impl RenderBox for RenderCenter {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) {
        let constraints = ctx.constraints().clone();

        tracing::debug!(
            "RenderCenter::perform_layout: constraints={:?}, child_count={}",
            constraints,
            ctx.child_count()
        );

        if ctx.child_count() > 0 {
            self.has_child = true;

            // Give child loose constraints
            let child_size = ctx.layout_single_child_loose();

            tracing::debug!("RenderCenter: child_size={:?}", child_size);

            // Calculate our size
            let width = if let Some(factor) = self.width_factor {
                child_size.width * factor
            } else {
                constraints.max_width
            };

            let height = if let Some(factor) = self.height_factor {
                child_size.height * factor
            } else {
                constraints.max_height
            };

            self.size = constraints.constrain(Size::new(width, height));

            // Center the child
            self.child_offset = Offset::new(
                (self.size.width - child_size.width) / 2.0,
                (self.size.height - child_size.height) / 2.0,
            );

            tracing::debug!(
                "RenderCenter: my_size={:?}, child_offset=({}, {})",
                self.size,
                self.child_offset.dx,
                self.child_offset.dy
            );

            ctx.position_child(0, self.child_offset);
        } else {
            self.has_child = false;
            // No child - expand to fill
            self.size = constraints.biggest();
            tracing::debug!("RenderCenter: no child, size={:?}", self.size);
        }

        ctx.complete_with_size(self.size);
    }

    fn size(&self) -> &Size {
        &self.size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    // paint() uses default no-op - Center just positions children

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_size(self.size.width, self.size.height) {
            return false;
        }

        if self.has_child {
            ctx.hit_test_child_at_offset(0, self.child_offset)
        } else {
            false
        }
    }

    fn box_paint_bounds(&self) -> Rect {
        Rect::from_origin_size(Point::ZERO, self.size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_center_with_factors() {
        let center = RenderCenter::new()
            .with_width_factor(0.5)
            .with_height_factor(0.5);

        assert_eq!(center.width_factor(), Some(0.5));
        assert_eq!(center.height_factor(), Some(0.5));
    }

    #[test]
    fn test_center_default_factors() {
        let center = RenderCenter::new();
        assert_eq!(center.width_factor(), None);
        assert_eq!(center.height_factor(), None);
    }
}
