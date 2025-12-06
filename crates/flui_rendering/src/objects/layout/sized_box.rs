//! RenderSizedBox - enforces exact size constraints

use crate::{RenderObject, RenderResult};

use crate::core::{BoxLayoutCtx, BoxPaintCtx};
use crate::core::{Optional, RenderBox};
use flui_types::constraints::BoxConstraints;
use flui_types::Size;

/// RenderObject that enforces exact size constraints
///
/// This render object forces its child to have a specific width and/or height.
/// If width or height is None, that dimension uses the constraint's max value.
///
/// # Layout Behavior
///
/// - **Both specified**: Forces exact size (tight constraints)
/// - **Width only**: Sets width, height fills constraint
/// - **Height only**: Sets height, width fills constraint
/// - **Neither specified**: Fills max constraints (same as unconstrained child)
///
/// # Without Child (Spacer)
///
/// When no child is present, RenderSizedBox acts as a spacer, returning the specified size.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSizedBox;
///
/// // Force child to be exactly 100x100
/// let sized = RenderSizedBox::exact(100.0, 100.0);
///
/// // Set width only, height flexible
/// let wide = RenderSizedBox::width(200.0);
///
/// // Set height only, width flexible
/// let tall = RenderSizedBox::height(150.0);
///
/// // Spacer (no child): 50px horizontal gap
/// let spacer = RenderSizedBox::width(50.0);
/// ```
#[derive(Debug)]
pub struct RenderSizedBox {
    /// Explicit width (None = unconstrained)
    pub width: Option<f32>,
    /// Explicit height (None = unconstrained)
    pub height: Option<f32>,
}

impl RenderSizedBox {
    /// Create new RenderSizedBox with optional width and height
    pub fn new(width: Option<f32>, height: Option<f32>) -> Self {
        Self { width, height }
    }

    /// Create with specific width and height
    pub fn exact(width: f32, height: f32) -> Self {
        Self {
            width: Some(width),
            height: Some(height),
        }
    }

    /// Create with only width specified
    pub fn width(width: f32) -> Self {
        Self {
            width: Some(width),
            height: None,
        }
    }

    /// Create with only height specified
    pub fn height(height: f32) -> Self {
        Self {
            width: None,
            height: Some(height),
        }
    }

    /// Set width
    pub fn set_width(&mut self, width: Option<f32>) {
        self.width = width;
    }

    /// Set height
    pub fn set_height(&mut self, height: Option<f32>) {
        self.height = height;
    }
}

impl Default for RenderSizedBox {
    fn default() -> Self {
        Self::new(None, None)
    }
}

impl RenderObject for RenderSizedBox {}

impl RenderBox<Optional> for RenderSizedBox {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Optional>) -> RenderResult<Size> {
        let constraints = ctx.constraints;

        // Check if we have a child
        if let Some(child_id) = ctx.children.get() {
            let child_id = *child_id;
            // Layout child first if we need its size
            let child_size = if self.width.is_none() || self.height.is_none() {
                // Need child's intrinsic size for unspecified dimensions
                // Give child loose constraints for dimensions we don't control
                let child_constraints = BoxConstraints::new(
                    0.0,
                    self.width.unwrap_or(constraints.max_width),
                    0.0,
                    self.height.unwrap_or(constraints.max_height),
                );
                ctx.layout_child(child_id, child_constraints)?
            } else {
                // Both dimensions specified, we don't need child size yet
                Size::ZERO
            };

            // Calculate final size
            let width = self.width.unwrap_or(child_size.width);
            let height = self.height.unwrap_or(child_size.height);
            let size = Size::new(width, height);

            // If we already laid out child with correct constraints, we're done
            // Otherwise, force child to match our size
            if self.width.is_some() && self.height.is_some() {
                let child_constraints = BoxConstraints::tight(size);
                ctx.layout_child(child_id, child_constraints)?;
            }

            Ok(size)
        } else {
            // No child - act as spacer with specified dimensions
            let width = self.width.unwrap_or(constraints.max_width);
            let height = self.height.unwrap_or(constraints.max_height);
            Ok(Size::new(width, height))
        }
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Optional>) {
        // If we have a child, paint it at our offset
        if let Some(child_id) = ctx.children.get() {
            let _ = ctx.paint_child(*child_id, ctx.offset);
        }
        // If no child, nothing to paint (spacer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_sized_box_new() {
        let sized = RenderSizedBox::new(Some(100.0), Some(50.0));
        assert_eq!(sized.width, Some(100.0));
        assert_eq!(sized.height, Some(50.0));
    }

    #[test]
    fn test_render_sized_box_exact() {
        let sized = RenderSizedBox::exact(100.0, 100.0);
        assert_eq!(sized.width, Some(100.0));
        assert_eq!(sized.height, Some(100.0));
    }

    #[test]
    fn test_render_sized_box_width() {
        let sized = RenderSizedBox::width(50.0);
        assert_eq!(sized.width, Some(50.0));
        assert_eq!(sized.height, None);
    }

    #[test]
    fn test_render_sized_box_height() {
        let sized = RenderSizedBox::height(75.0);
        assert_eq!(sized.width, None);
        assert_eq!(sized.height, Some(75.0));
    }

    #[test]
    fn test_render_sized_box_default() {
        let sized = RenderSizedBox::default();
        assert_eq!(sized.width, None);
        assert_eq!(sized.height, None);
    }

    #[test]
    fn test_render_sized_box_set_width() {
        let mut sized = RenderSizedBox::width(50.0);
        sized.set_width(Some(100.0));
        assert_eq!(sized.width, Some(100.0));
    }

    #[test]
    fn test_render_sized_box_set_height() {
        let mut sized = RenderSizedBox::height(50.0);
        sized.set_height(Some(100.0));
        assert_eq!(sized.height, Some(100.0));
    }
}
