//! Sliver parent data for scrollable content positioning

use flui_types::Offset;

/// Parent data for sliver protocol render objects
///
/// SliverParentData stores the paint offset for sliver children. This offset
/// is along the main scroll axis and is used to position the child within
/// the scrollable viewport.
///
/// # Usage
///
/// ```ignore
/// let mut parent_data = SliverParentData::default();
/// parent_data.paint_offset = 100.0;  // 100px down the scroll axis
///
/// // In parent's paint method:
/// let child_offset = compute_paint_offset_for_child(child);
/// context.paint_child(child, child_offset);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliverParentData {
    /// Paint offset along the main scroll axis
    ///
    /// This is the position where the child should be painted relative to
    /// the sliver's origin along the scroll axis.
    pub paint_offset: f32,

    /// 2D offset for cross-axis positioning
    ///
    /// Used when the sliver needs to position children in 2D space
    /// (e.g., for grid layouts or cross-axis adjustments).
    pub offset: Option<Offset>,
}

impl SliverParentData {
    /// Creates new sliver parent data with zero offset
    pub const fn new() -> Self {
        Self {
            paint_offset: 0.0,
            offset: None,
        }
    }

    /// Creates sliver parent data with specified paint offset
    pub const fn with_paint_offset(paint_offset: f32) -> Self {
        Self {
            paint_offset,
            offset: None,
        }
    }

    /// Creates sliver parent data with both paint offset and 2D offset
    pub const fn with_offsets(paint_offset: f32, offset: Offset) -> Self {
        Self {
            paint_offset,
            offset: Some(offset),
        }
    }

    /// Returns the 2D offset, falling back to paint offset along main axis
    pub fn get_offset(&self, main_axis_vertical: bool) -> Offset {
        self.offset.unwrap_or_else(|| {
            if main_axis_vertical {
                Offset::new(0.0, self.paint_offset)
            } else {
                Offset::new(self.paint_offset, 0.0)
            }
        })
    }
}

impl Default for SliverParentData {
    fn default() -> Self {
        Self::new()
    }
}

// Implement ParentData trait using the helper macro
crate::impl_parent_data!(SliverParentData);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default() {
        let data = SliverParentData::default();
        assert_eq!(data.paint_offset, 0.0);
        assert_eq!(data.offset, None);
    }

    #[test]
    fn test_with_paint_offset() {
        let data = SliverParentData::with_paint_offset(100.0);
        assert_eq!(data.paint_offset, 100.0);
        assert_eq!(data.offset, None);
    }

    #[test]
    fn test_get_offset_vertical() {
        let data = SliverParentData::with_paint_offset(100.0);
        let offset = data.get_offset(true);
        assert_eq!(offset.dx, 0.0);
        assert_eq!(offset.dy, 100.0);
    }

    #[test]
    fn test_get_offset_horizontal() {
        let data = SliverParentData::with_paint_offset(100.0);
        let offset = data.get_offset(false);
        assert_eq!(offset.dx, 100.0);
        assert_eq!(offset.dy, 0.0);
    }
}
