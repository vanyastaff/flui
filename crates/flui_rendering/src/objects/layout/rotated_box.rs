//! RenderRotatedBox - rotates child by quarter turns (90°, 180°, 270°)

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Quarter turns for rotation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuarterTurns {
    /// No rotation (0°)
    Zero = 0,
    /// 90° clockwise
    One = 1,
    /// 180° rotation
    Two = 2,
    /// 270° clockwise (90° counter-clockwise)
    Three = 3,
}

impl QuarterTurns {
    /// Create from integer (modulo 4)
    pub fn from_int(turns: i32) -> Self {
        match turns.rem_euclid(4) {
            0 => QuarterTurns::Zero,
            1 => QuarterTurns::One,
            2 => QuarterTurns::Two,
            3 => QuarterTurns::Three,
            _ => unreachable!(),
        }
    }

    /// Get as integer
    pub fn as_int(self) -> i32 {
        self as i32
    }

    /// Check if this rotation swaps width and height
    pub fn swaps_dimensions(self) -> bool {
        matches!(self, QuarterTurns::One | QuarterTurns::Three)
    }
}

/// Data for RenderRotatedBox
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RotatedBoxData {
    /// Number of quarter turns clockwise
    pub quarter_turns: QuarterTurns,
}

impl RotatedBoxData {
    /// Create new rotated box data
    pub fn new(quarter_turns: QuarterTurns) -> Self {
        Self { quarter_turns }
    }

    /// Create with 90° rotation
    pub fn rotate_90() -> Self {
        Self::new(QuarterTurns::One)
    }

    /// Create with 180° rotation
    pub fn rotate_180() -> Self {
        Self::new(QuarterTurns::Two)
    }

    /// Create with 270° rotation
    pub fn rotate_270() -> Self {
        Self::new(QuarterTurns::Three)
    }
}

impl Default for RotatedBoxData {
    fn default() -> Self {
        Self::new(QuarterTurns::Zero)
    }
}

/// RenderObject that rotates its child by quarter turns
///
/// Unlike RenderTransform which can do arbitrary rotations,
/// RenderRotatedBox only supports 90° increments and properly
/// adjusts layout constraints (swapping width/height for odd turns).
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::layout::{RotatedBoxData, QuarterTurns}};
///
/// // Rotate child 90° clockwise
/// let mut rotated = SingleRenderBox::new(RotatedBoxData::rotate_90());
/// ```
pub type RenderRotatedBox = SingleRenderBox<RotatedBoxData>;

// ===== Public API =====

impl RenderRotatedBox {
    /// Get quarter turns
    pub fn quarter_turns(&self) -> QuarterTurns {
        self.data().quarter_turns
    }

    /// Set quarter turns
    pub fn set_quarter_turns(&mut self, quarter_turns: QuarterTurns) {
        if self.data().quarter_turns != quarter_turns {
            self.data_mut().quarter_turns = quarter_turns;
            RenderBoxMixin::mark_needs_layout(self);
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderRotatedBox {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        let quarter_turns = self.data().quarter_turns;

        // For odd quarter turns (90°, 270°), swap width and height constraints
        let child_constraints = if quarter_turns.swaps_dimensions() {
            BoxConstraints::new(
                constraints.min_height,
                constraints.max_height,
                constraints.min_width,
                constraints.max_width,
            )
        } else {
            constraints
        };

        // Layout child
        let child_size = if let Some(child) = self.child_mut() {
            child.layout(child_constraints)
        } else {
            child_constraints.smallest()
        };

        // Our size is child size with potentially swapped dimensions
        let size = if quarter_turns.swaps_dimensions() {
            Size::new(child_size.height, child_size.width)
        } else {
            child_size
        };

        // Store size and clear needs_layout flag
        self.state_mut().size = Some(size);
        self.clear_needs_layout();

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        if let Some(child) = self.child() {
            let quarter_turns = self.data().quarter_turns;
            let size = self.state().size.unwrap_or(Size::ZERO);
            let _child_size = child.size();

            // Calculate paint offset based on rotation
            let paint_offset = match quarter_turns {
                QuarterTurns::Zero => {
                    // No rotation
                    offset
                }
                QuarterTurns::One => {
                    // 90° clockwise: child's top-left becomes our top-right
                    Offset::new(offset.dx + size.width, offset.dy)
                }
                QuarterTurns::Two => {
                    // 180°: child's top-left becomes our bottom-right
                    Offset::new(offset.dx + size.width, offset.dy + size.height)
                }
                QuarterTurns::Three => {
                    // 270° clockwise: child's top-left becomes our bottom-left
                    Offset::new(offset.dx, offset.dy + size.height)
                }
            };

            // Paint with rotation
            // Note: egui doesn't directly support rotation in Painter,
            // so we would need to use a custom implementation or Transform widget
            // For now, we'll paint at the calculated offset
            // TODO: Implement proper rotation when egui supports it or use manual transformation

            painter.ctx().debug_painter().text(
                egui::pos2(paint_offset.dx, paint_offset.dy),
                egui::Align2::LEFT_TOP,
                format!("Rotated {}°", quarter_turns.as_int() * 90),
                egui::FontId::default(),
                egui::Color32::RED,
            );

            // For now, paint child without actual rotation
            // In real implementation, we would apply rotation matrix
            child.paint(painter, paint_offset);
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quarter_turns_from_int() {
        assert_eq!(QuarterTurns::from_int(0), QuarterTurns::Zero);
        assert_eq!(QuarterTurns::from_int(1), QuarterTurns::One);
        assert_eq!(QuarterTurns::from_int(2), QuarterTurns::Two);
        assert_eq!(QuarterTurns::from_int(3), QuarterTurns::Three);
        assert_eq!(QuarterTurns::from_int(4), QuarterTurns::Zero);
        assert_eq!(QuarterTurns::from_int(5), QuarterTurns::One);
        assert_eq!(QuarterTurns::from_int(-1), QuarterTurns::Three);
    }

    #[test]
    fn test_quarter_turns_swaps_dimensions() {
        assert!(!QuarterTurns::Zero.swaps_dimensions());
        assert!(QuarterTurns::One.swaps_dimensions());
        assert!(!QuarterTurns::Two.swaps_dimensions());
        assert!(QuarterTurns::Three.swaps_dimensions());
    }

    #[test]
    fn test_rotated_box_data_new() {
        let data = RotatedBoxData::new(QuarterTurns::One);
        assert_eq!(data.quarter_turns, QuarterTurns::One);
    }

    #[test]
    fn test_rotated_box_data_helpers() {
        assert_eq!(RotatedBoxData::rotate_90().quarter_turns, QuarterTurns::One);
        assert_eq!(RotatedBoxData::rotate_180().quarter_turns, QuarterTurns::Two);
        assert_eq!(RotatedBoxData::rotate_270().quarter_turns, QuarterTurns::Three);
    }

    #[test]
    fn test_render_rotated_box_new() {
        let rotated = SingleRenderBox::new(RotatedBoxData::rotate_90());
        assert_eq!(rotated.quarter_turns(), QuarterTurns::One);
    }

    #[test]
    fn test_render_rotated_box_set_quarter_turns() {
        let mut rotated = SingleRenderBox::new(RotatedBoxData::default());

        rotated.set_quarter_turns(QuarterTurns::Two);
        assert_eq!(rotated.quarter_turns(), QuarterTurns::Two);
        assert!(RenderBoxMixin::needs_layout(&rotated));
    }

    #[test]
    fn test_render_rotated_box_layout_no_rotation() {
        let mut rotated = SingleRenderBox::new(RotatedBoxData::default());
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 200.0);

        let size = rotated.layout(constraints);

        // No child, should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }

    #[test]
    fn test_render_rotated_box_layout_90_degrees() {
        let mut rotated = SingleRenderBox::new(RotatedBoxData::rotate_90());
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 200.0);

        let size = rotated.layout(constraints);

        // 90° rotation swaps dimensions: child gets swapped constraints
        // No child, so size is smallest of swapped constraints
        assert_eq!(size, Size::new(0.0, 0.0));
    }

    #[test]
    fn test_render_rotated_box_layout_180_degrees() {
        let mut rotated = SingleRenderBox::new(RotatedBoxData::rotate_180());
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 200.0);

        let size = rotated.layout(constraints);

        // 180° doesn't swap dimensions
        assert_eq!(size, Size::new(0.0, 0.0));
    }
}
