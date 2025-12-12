//! Box parent data for basic offset positioning

use flui_types::Offset;

/// Parent data for box protocol render objects
///
/// BoxParentData is the base parent data type for box children. It stores the
/// offset at which the child should be painted relative to the parent's origin.
///
/// # Usage
///
/// ```ignore
/// let mut parent_data = BoxParentData::default();
/// parent_data.offset = Offset::new(10.0, 20.0);
///
/// // In parent's paint method:
/// context.paint_child(child, parent_offset + child_parent_data.offset);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxParentData {
    /// Offset of this child relative to parent's origin
    ///
    /// This is set by the parent during layout and used during painting
    /// and hit testing.
    pub offset: Offset,
}

impl BoxParentData {
    /// Creates new box parent data with zero offset
    pub const fn new() -> Self {
        Self {
            offset: Offset::ZERO,
        }
    }

    /// Creates box parent data with specified offset
    pub const fn with_offset(offset: Offset) -> Self {
        Self { offset }
    }
}

impl Default for BoxParentData {
    fn default() -> Self {
        Self::new()
    }
}

// Implement ParentData trait using the helper macro
crate::impl_parent_data!(BoxParentData);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default() {
        let data = BoxParentData::default();
        assert_eq!(data.offset, Offset::ZERO);
    }

    #[test]
    fn test_with_offset() {
        let data = BoxParentData::with_offset(Offset::new(10.0, 20.0));
        assert_eq!(data.offset.dx, 10.0);
        assert_eq!(data.offset.dy, 20.0);
    }
}
