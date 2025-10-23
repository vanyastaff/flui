//! RenderFittedBox - scales and positions child according to BoxFit

use flui_types::{Alignment, Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// How a box should be inscribed into another box
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxFit {
    /// Fill the target box by distorting aspect ratio
    Fill,
    /// As large as possible while maintaining aspect ratio (may clip)
    Cover,
    /// As large as possible while entirely contained (may leave empty space)
    Contain,
    /// Maintain original size (may overflow or leave empty space)
    None,
    /// Scale down to fit if needed, otherwise maintain size
    ScaleDown,
    /// Fill width, scale height maintaining aspect ratio
    FitWidth,
    /// Fill height, scale width maintaining aspect ratio
    FitHeight,
}

/// Data for RenderFittedBox
#[derive(Debug, Clone, Copy)]
pub struct FittedBoxData {
    /// How to fit child into parent
    pub fit: BoxFit,
    /// How to align child within parent
    pub alignment: Alignment,
    /// Clip behavior
    pub clip_behavior: ClipBehavior,
}

/// Clip behavior
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipBehavior {
    /// No clipping
    None,
    /// Clip to bounds
    HardEdge,
    /// Clip with anti-aliasing
    AntiAlias,
}

impl FittedBoxData {
    /// Create new fitted box data
    pub fn new(fit: BoxFit) -> Self {
        Self {
            fit,
            alignment: Alignment::CENTER,
            clip_behavior: ClipBehavior::None,
        }
    }

    /// Create with alignment
    pub fn with_alignment(fit: BoxFit, alignment: Alignment) -> Self {
        Self {
            fit,
            alignment,
            clip_behavior: ClipBehavior::None,
        }
    }

    /// Calculate fitted size and offset
    pub fn calculate_fit(
        &self,
        child_size: Size,
        container_size: Size,
    ) -> (Size, Offset) {
        let scale = match self.fit {
            BoxFit::Fill => {
                (container_size.width / child_size.width, container_size.height / child_size.height)
            }
            BoxFit::Cover => {
                let scale = (container_size.width / child_size.width)
                    .max(container_size.height / child_size.height);
                (scale, scale)
            }
            BoxFit::Contain => {
                let scale = (container_size.width / child_size.width)
                    .min(container_size.height / child_size.height);
                (scale, scale)
            }
            BoxFit::None => (1.0, 1.0),
            BoxFit::ScaleDown => {
                let scale = (container_size.width / child_size.width)
                    .min(container_size.height / child_size.height)
                    .min(1.0);
                (scale, scale)
            }
            BoxFit::FitWidth => {
                let scale = container_size.width / child_size.width;
                (scale, scale)
            }
            BoxFit::FitHeight => {
                let scale = container_size.height / child_size.height;
                (scale, scale)
            }
        };

        let fitted_size = Size::new(
            child_size.width * scale.0,
            child_size.height * scale.1,
        );

        // Calculate offset based on alignment
        // Alignment: -1.0 = left/top, 0.0 = center, 1.0 = right/bottom
        let dx = (container_size.width - fitted_size.width) * (self.alignment.x + 1.0) / 2.0;
        let dy = (container_size.height - fitted_size.height) * (self.alignment.y + 1.0) / 2.0;

        (fitted_size, Offset::new(dx, dy))
    }
}

impl Default for FittedBoxData {
    fn default() -> Self {
        Self::new(BoxFit::Contain)
    }
}

/// RenderObject that scales and positions its child according to BoxFit
///
/// FittedBox is useful for scaling children to fit within constrained spaces
/// while maintaining aspect ratio.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::special::{FittedBoxData, BoxFit}};
/// use flui_types::Alignment;
///
/// // Scale child to cover the entire box
/// let mut fitted = SingleRenderBox::new(
///     FittedBoxData::with_alignment(BoxFit::Cover, Alignment::TOP_LEFT)
/// );
/// ```
pub type RenderFittedBox = SingleRenderBox<FittedBoxData>;

// ===== Public API =====

impl RenderFittedBox {
    /// Get fit mode
    pub fn fit(&self) -> BoxFit {
        self.data().fit
    }

    /// Set fit mode
    pub fn set_fit(&mut self, fit: BoxFit) {
        if self.data().fit != fit {
            self.data_mut().fit = fit;
            self.mark_needs_layout();
        }
    }

    /// Get alignment
    pub fn alignment(&self) -> Alignment {
        self.data().alignment
    }

    /// Set alignment
    pub fn set_alignment(&mut self, alignment: Alignment) {
        if self.data().alignment != alignment {
            self.data_mut().alignment = alignment;
            self.mark_needs_paint(); // Only repaint needed for alignment change
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderFittedBox {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        // Store constraints
        *state.constraints.lock() = Some(constraints);

        // Our size is determined by constraints (we try to be as large as possible)
        let size = constraints.biggest();

        // Layout child with unbounded constraints to get natural size
        let children_ids = ctx.children();
        if let Some(&child_id) = children_ids.first() {
            let child_constraints = BoxConstraints::new(0.0, f32::INFINITY, 0.0, f32::INFINITY);
            let _child_size = ctx.layout_child(child_id, child_constraints);
        }

        // Store size and clear needs_layout flag
        *state.size.lock() = Some(size);
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);

        size
    }

    fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        // Get our size from state (avoid ambiguity by accessing state directly)
        if let Some(size) = *state.size.lock() {
            let children_ids = ctx.children();
            if let Some(&child_id) = children_ids.first() {
                // Get child's size
                // Get child size from tree
                let child_size = if let Some(child_elem) = ctx.tree().get(child_id) {
                    if let Some(child_ro) = child_elem.render_object() {
                        child_ro.size()
                    } else {
                        Size::ZERO
                    }
                } else {
                    Size::ZERO
                };

                // Calculate fitted size and offset
                let (_fitted_size, child_offset) = self.data().calculate_fit(child_size, size);

                // Apply transform for scaling
                // Note: In a real implementation, we'd use egui's transform system
                // For now, just paint child at calculated offset
                let final_offset = Offset::new(
                    offset.dx + child_offset.dx,
                    offset.dy + child_offset.dy,
                );

                ctx.paint_child(child_id, painter, final_offset);
            }
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
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
    fn test_fitted_box_data_new() {
        let data = FittedBoxData::new(BoxFit::Cover);
        assert_eq!(data.fit, BoxFit::Cover);
        assert_eq!(data.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_fitted_box_data_calculate_fit_contain() {
        let data = FittedBoxData::new(BoxFit::Contain);
        let child_size = Size::new(200.0, 100.0);
        let container_size = Size::new(100.0, 100.0);

        let (fitted_size, offset) = data.calculate_fit(child_size, container_size);

        // Should scale down to fit width (200 -> 100), maintaining aspect ratio
        assert_eq!(fitted_size.width, 100.0);
        assert_eq!(fitted_size.height, 50.0);

        // Centered vertically
        assert_eq!(offset.dx, 0.0);
        assert_eq!(offset.dy, 25.0);
    }

    #[test]
    fn test_fitted_box_data_calculate_fit_cover() {
        let data = FittedBoxData::new(BoxFit::Cover);
        let child_size = Size::new(100.0, 50.0);
        let container_size = Size::new(100.0, 100.0);

        let (fitted_size, _offset) = data.calculate_fit(child_size, container_size);

        // Should scale to cover height (50 -> 100), maintaining aspect ratio
        assert_eq!(fitted_size.width, 200.0);
        assert_eq!(fitted_size.height, 100.0);
    }

    #[test]
    fn test_fitted_box_data_calculate_fit_fill() {
        let data = FittedBoxData::new(BoxFit::Fill);
        let child_size = Size::new(200.0, 100.0);
        let container_size = Size::new(100.0, 50.0);

        let (fitted_size, _offset) = data.calculate_fit(child_size, container_size);

        // Should distort to fill exactly
        assert_eq!(fitted_size.width, 100.0);
        assert_eq!(fitted_size.height, 50.0);
    }

    #[test]
    fn test_fitted_box_data_calculate_fit_none() {
        let data = FittedBoxData::new(BoxFit::None);
        let child_size = Size::new(200.0, 100.0);
        let container_size = Size::new(100.0, 100.0);

        let (fitted_size, _offset) = data.calculate_fit(child_size, container_size);

        // Should keep original size
        assert_eq!(fitted_size, child_size);
    }

    #[test]
    fn test_render_fitted_box_new() {
        let mut fitted = SingleRenderBox::new(FittedBoxData::new(BoxFit::Contain));
        assert_eq!(fitted.fit(), BoxFit::Contain);
        assert_eq!(fitted.alignment(), Alignment::CENTER);
    }

    #[test]
    fn test_render_fitted_box_set_fit() {
        use flui_core::DynRenderObject;

        let mut fitted = SingleRenderBox::new(FittedBoxData::new(BoxFit::Contain));

        fitted.set_fit(BoxFit::Cover);
        assert_eq!(fitted.fit(), BoxFit::Cover);
        assert!(DynRenderObject::needs_layout(&fitted));
    }

    #[test]
    fn test_render_fitted_box_set_alignment() {
        use flui_core::testing::mock_render_context;

        use flui_core::DynRenderObject;

        let mut fitted = SingleRenderBox::new(FittedBoxData::new(BoxFit::Contain));

        // Layout first to clear initial needs_layout flag
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let (_tree, ctx) = mock_render_context();
        fitted.layout(constraints, &ctx);

        fitted.set_alignment(Alignment::TOP_LEFT);
        assert_eq!(fitted.alignment(), Alignment::TOP_LEFT);
        assert!(DynRenderObject::needs_paint(&fitted));
        assert!(!DynRenderObject::needs_layout(&fitted)); // Alignment only affects paint
    }

    #[test]
    fn test_render_fitted_box_layout() {
        use flui_core::testing::mock_render_context;

        let mut fitted = SingleRenderBox::new(FittedBoxData::new(BoxFit::Contain));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let (_tree, ctx) = mock_render_context();
        let size = fitted.layout(constraints, &ctx);

        // Should fill available space
        assert_eq!(size, Size::new(100.0, 100.0));
    }
}
