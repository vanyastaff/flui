//! RenderFittedBox - scales and positions child_id according to BoxFit

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::BoxedLayer;
use flui_types::{Alignment, Offset, Size, constraints::BoxConstraints};

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
    /// How to fit child_id into parent
    pub fit: BoxFit,
    /// How to align child_id within parent
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
    pub fn calculate_fit(&self, child_size: Size, container_size: Size) -> (Size, Offset) {
        let scale = match self.fit {
            BoxFit::Fill => (
                container_size.width / child_size.width,
                container_size.height / child_size.height,
            ),
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

        let fitted_size = Size::new(child_size.width * scale.0, child_size.height * scale.1);

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

/// RenderObject that scales and positions its child_id according to BoxFit
///
/// FittedBox is useful for scaling children to fit within constrained spaces
/// while maintaining aspect ratio.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderFittedBox, BoxFit};
/// use flui_types::Alignment;
///
/// // Scale child_id to cover the entire box
/// let mut fitted = RenderFittedBox::with_alignment(BoxFit::Cover, Alignment::TOP_LEFT);
/// ```
#[derive(Debug)]
pub struct RenderFittedBox {
    /// Fitted box data
    pub data: FittedBoxData,
}

// ===== Public API =====

impl RenderFittedBox {
    /// Create new RenderFittedBox
    pub fn new(fit: BoxFit) -> Self {
        Self {
            data: FittedBoxData::new(fit),
        }
    }

    /// Create with alignment
    pub fn with_alignment(fit: BoxFit, alignment: Alignment) -> Self {
        Self {
            data: FittedBoxData::with_alignment(fit, alignment),
        }
    }

    /// Get fit mode
    pub fn fit(&self) -> BoxFit {
        self.data.fit
    }

    /// Set fit mode
    pub fn set_fit(&mut self, fit: BoxFit) {
        self.data.fit = fit;
    }

    /// Get alignment
    pub fn alignment(&self) -> Alignment {
        self.data.alignment
    }

    /// Set alignment
    pub fn set_alignment(&mut self, alignment: Alignment) {
        self.data.alignment = alignment;
    }
}

// ===== RenderObject Implementation =====

impl SingleRender for RenderFittedBox {
    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        // Our size is determined by constraints (we try to be as large as possible)
        let size = constraints.biggest();

        // Layout child_id with unbounded constraints to get natural size
        let child_constraints =
            flui_types::constraints::BoxConstraints::new(0.0, f32::INFINITY, 0.0, f32::INFINITY);
        tree.layout_child(child_id, child_constraints);

        size
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        // Get child_id layer and calculate fit
        // TODO: Apply transform for scaling based on self.data.calculate_fit()
        // For now, just return child_id layer as-is
        // In a real implementation, we'd wrap in a TransformLayer

        (tree.paint_child(child_id, offset)) as _
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
        let fitted = RenderFittedBox::new(BoxFit::Contain);
        assert_eq!(fitted.fit(), BoxFit::Contain);
        assert_eq!(fitted.alignment(), Alignment::CENTER);
    }

    #[test]
    fn test_render_fitted_box_with_alignment() {
        let fitted = RenderFittedBox::with_alignment(BoxFit::Cover, Alignment::TOP_LEFT);
        assert_eq!(fitted.fit(), BoxFit::Cover);
        assert_eq!(fitted.alignment(), Alignment::TOP_LEFT);
    }

    #[test]
    fn test_render_fitted_box_set_fit() {
        let mut fitted = RenderFittedBox::new(BoxFit::Contain);

        fitted.set_fit(BoxFit::Cover);
        assert_eq!(fitted.fit(), BoxFit::Cover);
    }

    #[test]
    fn test_render_fitted_box_set_alignment() {
        let mut fitted = RenderFittedBox::new(BoxFit::Contain);

        fitted.set_alignment(Alignment::TOP_LEFT);
        assert_eq!(fitted.alignment(), Alignment::TOP_LEFT);
    }
}
