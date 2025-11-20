//! RenderFittedBox - scales and positions child according to BoxFit

use flui_core::render::{
    {BoxProtocol, LayoutContext, PaintContext},
    RenderBox,
    Single,
};
use flui_types::{layout::BoxFit, painting::ClipBehavior, Alignment, Offset, Size};

/// RenderObject that scales and positions its child_id according to BoxFit
///
/// FittedBox is useful for scaling children to fit within constrained spaces
/// while maintaining aspect ratio.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderFittedBox;
/// use flui_types::layout::BoxFit;
/// use flui_types::Alignment;
///
/// // Scale child to cover the entire box
/// let mut fitted = RenderFittedBox::with_alignment(BoxFit::Cover, Alignment::TOP_LEFT);
/// ```
#[derive(Debug)]
pub struct RenderFittedBox {
    /// How to fit child into parent
    pub fit: BoxFit,
    /// How to align child within parent
    pub alignment: Alignment,
    /// Clip behavior
    pub clip_behavior: ClipBehavior,
}

// ===== Public API =====

impl RenderFittedBox {
    /// Create new RenderFittedBox with default alignment and no clipping
    pub fn new(fit: BoxFit) -> Self {
        Self {
            fit,
            alignment: Alignment::CENTER,
            clip_behavior: ClipBehavior::None,
        }
    }

    /// Create with custom alignment
    pub fn with_alignment(fit: BoxFit, alignment: Alignment) -> Self {
        Self {
            fit,
            alignment,
            clip_behavior: ClipBehavior::None,
        }
    }

    /// Set fit mode
    pub fn set_fit(&mut self, fit: BoxFit) {
        self.fit = fit;
    }

    /// Set alignment
    pub fn set_alignment(&mut self, alignment: Alignment) {
        self.alignment = alignment;
    }

    /// Set clip behavior
    pub fn set_clip_behavior(&mut self, clip_behavior: ClipBehavior) {
        self.clip_behavior = clip_behavior;
    }

    /// Calculate fitted size and offset for given child and container sizes
    pub fn calculate_fit(&self, child_size: Size, container_size: Size) -> (Size, Offset) {
        // Epsilon for safe float comparisons (Rust 1.91.0 strict arithmetic)
        const EPSILON: f32 = 1e-6;

        let scale = match self.fit {
            BoxFit::Fill => {
                let scale_x = if child_size.width.abs() > EPSILON {
                    container_size.width / child_size.width
                } else {
                    1.0
                };
                let scale_y = if child_size.height.abs() > EPSILON {
                    container_size.height / child_size.height
                } else {
                    1.0
                };
                (scale_x, scale_y)
            }
            BoxFit::Cover => {
                let scale_x = if child_size.width.abs() > EPSILON {
                    container_size.width / child_size.width
                } else {
                    1.0
                };
                let scale_y = if child_size.height.abs() > EPSILON {
                    container_size.height / child_size.height
                } else {
                    1.0
                };
                let scale = scale_x.max(scale_y);
                (scale, scale)
            }
            BoxFit::Contain => {
                let scale_x = if child_size.width.abs() > EPSILON {
                    container_size.width / child_size.width
                } else {
                    1.0
                };
                let scale_y = if child_size.height.abs() > EPSILON {
                    container_size.height / child_size.height
                } else {
                    1.0
                };
                let scale = scale_x.min(scale_y);
                (scale, scale)
            }
            BoxFit::None => (1.0, 1.0),
            BoxFit::ScaleDown => {
                let scale_x = if child_size.width.abs() > EPSILON {
                    container_size.width / child_size.width
                } else {
                    1.0
                };
                let scale_y = if child_size.height.abs() > EPSILON {
                    container_size.height / child_size.height
                } else {
                    1.0
                };
                let scale = scale_x.min(scale_y).min(1.0);
                (scale, scale)
            }
            BoxFit::FitWidth => {
                let scale = if child_size.width.abs() > EPSILON {
                    container_size.width / child_size.width
                } else {
                    1.0
                };
                (scale, scale)
            }
            BoxFit::FitHeight => {
                let scale = if child_size.height.abs() > EPSILON {
                    container_size.height / child_size.height
                } else {
                    1.0
                };
                (scale, scale)
            }
        };

        let fitted_size = Size::new(child_size.width * scale.0, child_size.height * scale.1);

        // Calculate offset based on alignment
        // Alignment: -1.0 = left/top, 0.0 = center, 1.0 = right/bottom
        let dx = (container_size.width - fitted_size.width) * (self.alignment.x + 1.0) / 2.0;
        let dy = (container_size.height - fitted_size.height) * (self.alignment.y + 1.0) / 2.0;

        (fitted_size, Offset::new(dx, dy))
    }
}

impl Default for RenderFittedBox {
    fn default() -> Self {
        Self::new(BoxFit::Contain)
    }
}

// ===== RenderObject Implementation =====

impl RenderBox<Single> for RenderFittedBox {
    fn layout(&mut self, ctx: LayoutContext<'_, Single, BoxProtocol>) -> Size {
        let child_id = ctx.children.single();

        // Our size is determined by constraints (we try to be as large as possible)
        let size = ctx.constraints.biggest();

        // Layout child with unbounded constraints to get natural size
        let child_constraints =
            flui_types::constraints::BoxConstraints::new(0.0, f32::INFINITY, 0.0, f32::INFINITY);
        ctx.layout_child(child_id, child_constraints);

        size
    }

    fn paint(&self, ctx: &mut PaintContext<'_, Single>) {
        let child_id = ctx.children.single();

        // TODO: Apply transform for scaling based on self.calculate_fit()
        // For now, just paint child as-is
        // In a real implementation, we'd wrap in a TransformLayer

        ctx.paint_child(child_id, ctx.offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_fit_variants() {
        assert_ne!(BoxFit::Fill, BoxFit::Cover);
        assert_ne!(BoxFit::Cover, BoxFit::Contain);
        assert_ne!(BoxFit::Contain, BoxFit::None);
    }

    #[test]
    fn test_render_fitted_box_new() {
        let fitted = RenderFittedBox::new(BoxFit::Cover);
        assert_eq!(fitted.fit, BoxFit::Cover);
        assert_eq!(fitted.alignment, Alignment::CENTER);
        assert_eq!(fitted.clip_behavior, ClipBehavior::None);
    }

    #[test]
    fn test_calculate_fit_contain() {
        let fitted = RenderFittedBox::new(BoxFit::Contain);
        let child_size = Size::new(200.0, 100.0);
        let container_size = Size::new(100.0, 100.0);

        let (fitted_size, offset) = fitted.calculate_fit(child_size, container_size);

        // Should scale down to fit width (200 -> 100), maintaining aspect ratio
        assert_eq!(fitted_size.width, 100.0);
        assert_eq!(fitted_size.height, 50.0);

        // Centered vertically
        assert_eq!(offset.dx, 0.0);
        assert_eq!(offset.dy, 25.0);
    }

    #[test]
    fn test_calculate_fit_cover() {
        let fitted = RenderFittedBox::new(BoxFit::Cover);
        let child_size = Size::new(100.0, 50.0);
        let container_size = Size::new(100.0, 100.0);

        let (fitted_size, _offset) = fitted.calculate_fit(child_size, container_size);

        // Should scale to cover height (50 -> 100), maintaining aspect ratio
        assert_eq!(fitted_size.width, 200.0);
        assert_eq!(fitted_size.height, 100.0);
    }

    #[test]
    fn test_calculate_fit_fill() {
        let fitted = RenderFittedBox::new(BoxFit::Fill);
        let child_size = Size::new(200.0, 100.0);
        let container_size = Size::new(100.0, 50.0);

        let (fitted_size, _offset) = fitted.calculate_fit(child_size, container_size);

        // Should distort to fill exactly
        assert_eq!(fitted_size.width, 100.0);
        assert_eq!(fitted_size.height, 50.0);
    }

    #[test]
    fn test_calculate_fit_none() {
        let fitted = RenderFittedBox::new(BoxFit::None);
        let child_size = Size::new(200.0, 100.0);
        let container_size = Size::new(100.0, 100.0);

        let (fitted_size, _offset) = fitted.calculate_fit(child_size, container_size);

        // Should keep original size
        assert_eq!(fitted_size, child_size);
    }

    #[test]
    fn test_render_fitted_box_with_alignment() {
        let fitted = RenderFittedBox::with_alignment(BoxFit::Cover, Alignment::TOP_LEFT);
        assert_eq!(fitted.fit, BoxFit::Cover);
        assert_eq!(fitted.alignment, Alignment::TOP_LEFT);
    }

    #[test]
    fn test_render_fitted_box_set_fit() {
        let mut fitted = RenderFittedBox::new(BoxFit::Contain);
        fitted.set_fit(BoxFit::Cover);
        assert_eq!(fitted.fit, BoxFit::Cover);
    }

    #[test]
    fn test_render_fitted_box_set_alignment() {
        let mut fitted = RenderFittedBox::new(BoxFit::Contain);
        fitted.set_alignment(Alignment::TOP_LEFT);
        assert_eq!(fitted.alignment, Alignment::TOP_LEFT);
    }

    #[test]
    fn test_render_fitted_box_set_clip_behavior() {
        let mut fitted = RenderFittedBox::new(BoxFit::Contain);
        fitted.set_clip_behavior(ClipBehavior::AntiAlias);
        assert_eq!(fitted.clip_behavior, ClipBehavior::AntiAlias);
    }

    #[test]
    fn test_render_fitted_box_default() {
        let fitted = RenderFittedBox::default();
        assert_eq!(fitted.fit, BoxFit::Contain);
        assert_eq!(fitted.alignment, Alignment::CENTER);
    }
}
