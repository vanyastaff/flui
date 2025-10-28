//! RenderRotatedBox - rotates child by quarter turns (90°, 180°, 270°)

use flui_types::{Size, Offset, constraints::BoxConstraints};
use flui_core::render::{RenderObject, SingleArity, LayoutCx, PaintCx, SingleChild, SingleChildPaint};
use flui_engine::{BoxedLayer, TransformLayer};

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
/// use flui_rendering::RenderRotatedBox;
///
/// // Rotate child 90° clockwise
/// let mut rotated = RenderRotatedBox::rotate_90();
/// ```
#[derive(Debug)]
pub struct RenderRotatedBox {
    /// Number of quarter turns clockwise
    pub quarter_turns: QuarterTurns,
    /// Cached size from layout phase
    size: Size,
}

// ===== Public API =====

impl RenderRotatedBox {
    /// Create new RenderRotatedBox
    pub fn new(quarter_turns: QuarterTurns) -> Self {
        Self {
            quarter_turns,
            size: Size::ZERO,
        }
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

    /// Set quarter turns
    pub fn set_quarter_turns(&mut self, quarter_turns: QuarterTurns) {
        self.quarter_turns = quarter_turns;
    }
}

// ===== RenderObject Implementation =====

impl RenderObject for RenderRotatedBox {
    type Arity = SingleArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        let child = cx.child();
        let constraints = cx.constraints();

        // For odd quarter turns (90°, 270°), swap width and height constraints
        let child_constraints = if self.quarter_turns.swaps_dimensions() {
            // Manually flip constraints - swap width and height
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
        let child_size = cx.layout_child(child, child_constraints);

        // Our size is child size with potentially swapped dimensions
        let size = if self.quarter_turns.swaps_dimensions() {
            Size::new(child_size.height, child_size.width)
        } else {
            child_size
        };

        // Store size for paint phase
        self.size = size;
        size
    }

    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        let child = cx.child();

        // Calculate offset based on rotation
        // Note: For now, this is a simplified implementation
        // TODO: Implement proper rotation transformation
        let offset = match self.quarter_turns {
            QuarterTurns::Zero => Offset::new(0.0, 0.0),
            QuarterTurns::One => {
                // 90° clockwise: child's top-left becomes our top-right
                Offset::new(self.size.width, 0.0)
            }
            QuarterTurns::Two => {
                // 180°: child's top-left becomes our bottom-right
                Offset::new(self.size.width, self.size.height)
            }
            QuarterTurns::Three => {
                // 270° clockwise: child's top-left becomes our bottom-left
                Offset::new(0.0, self.size.height)
            }
        };

        // Capture child layer and apply offset transform
        // TODO: Add actual rotation transformation when available
        let child_layer = cx.capture_child_layer(child);

        if offset != Offset::ZERO {
            Box::new(TransformLayer::translate(child_layer, offset))
        } else {
            child_layer
        }
    }
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
        let rotated = RenderRotatedBox::rotate_90();
        assert_eq!(rotated.quarter_turns, QuarterTurns::One);
    }

    #[test]
    fn test_render_rotated_box_set_quarter_turns() {
        let mut rotated = RenderRotatedBox::new(QuarterTurns::Zero);

        rotated.set_quarter_turns(QuarterTurns::Two);
        assert_eq!(rotated.quarter_turns, QuarterTurns::Two);
    }

    #[test]
    fn test_render_rotated_box_helpers() {
        let rotated_90 = RenderRotatedBox::rotate_90();
        assert_eq!(rotated_90.quarter_turns, QuarterTurns::One);

        let rotated_180 = RenderRotatedBox::rotate_180();
        assert_eq!(rotated_180.quarter_turns, QuarterTurns::Two);

        let rotated_270 = RenderRotatedBox::rotate_270();
        assert_eq!(rotated_270.quarter_turns, QuarterTurns::Three);
    }
}
