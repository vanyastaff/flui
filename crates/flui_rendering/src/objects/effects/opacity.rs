//! RenderOpacity - applies opacity to a child using OpacityLayer

use crate::core::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use crate::{RenderObject, RenderResult};
use flui_types::Size;

/// RenderObject that applies opacity to its child
///
/// The opacity value ranges from 0.0 (fully transparent) to 1.0 (fully opaque).
/// Changing opacity only affects painting, not layout.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderOpacity;
///
/// let opacity = RenderOpacity::new(0.5);
/// ```
#[derive(Debug)]
pub struct RenderOpacity {
    /// Opacity value (0.0 = fully transparent, 1.0 = fully opaque)
    pub opacity: f32,
}

impl RenderOpacity {
    /// Create new RenderOpacity
    pub fn new(opacity: f32) -> Self {
        Self {
            opacity: opacity.clamp(0.0, 1.0),
        }
    }

    /// Set new opacity value
    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.0, 1.0);
    }
}

impl RenderObject for RenderOpacity {}

impl RenderBox<Single> for RenderOpacity {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        let child_id = *ctx.children.single();
        // Layout child with same constraints
        Ok(ctx.layout_child(child_id, ctx.constraints)?)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        let child_id = *ctx.children.single();

        // If fully transparent, don't paint anything
        if self.opacity <= 0.0 {
            return;
        }

        // TODO: Implement proper opacity layer support in Canvas API
        // For now, just paint child directly - opacity effect is visual only
        // In future: save layer with opacity, paint child, restore layer
        ctx.paint_child(child_id, ctx.offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_opacity_new() {
        let opacity = RenderOpacity::new(0.5);
        assert_eq!(opacity.opacity, 0.5);
    }

    #[test]
    fn test_render_opacity_clamping() {
        let opacity1 = RenderOpacity::new(-0.5);
        assert_eq!(opacity1.opacity, 0.0);

        let opacity2 = RenderOpacity::new(1.5);
        assert_eq!(opacity2.opacity, 1.0);
    }

    #[test]
    fn test_render_opacity_set_opacity() {
        let mut opacity = RenderOpacity::new(0.5);
        opacity.set_opacity(0.8);
        assert_eq!(opacity.opacity, 0.8);
    }
}
