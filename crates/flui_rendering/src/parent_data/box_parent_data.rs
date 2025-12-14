//! Box parent data types.

use flui_types::Offset;

// ============================================================================
// BoxParentData
// ============================================================================

/// Parent data for children of box render objects.
///
/// Stores the offset at which to paint the child in the parent's
/// coordinate system.
///
/// # Flutter Equivalence
///
/// ```dart
/// class BoxParentData extends ParentData {
///   Offset offset = Offset.zero;
/// }
/// ```
///
/// # Example
///
/// ```ignore
/// use flui_rendering::parent_data::BoxParentData;
/// use flui_types::Offset;
///
/// let mut data = BoxParentData::default();
/// data.offset = Offset::new(10.0, 20.0);
/// ```
#[derive(Debug, Clone, Default)]
pub struct BoxParentData {
    /// The offset at which to paint the child in the parent's coordinate system.
    pub offset: Offset,
}

impl BoxParentData {
    /// Creates new BoxParentData with the given offset.
    #[inline]
    pub fn new(offset: Offset) -> Self {
        Self { offset }
    }

    /// Creates BoxParentData with zero offset.
    #[inline]
    pub fn zero() -> Self {
        Self::default()
    }
}

crate::impl_parent_data!(BoxParentData);

// ============================================================================
// ContainerBoxParentData
// ============================================================================

/// Parent data for children in a container with multiple box children.
///
/// Extends [`BoxParentData`] with sibling pointers for efficient
/// iteration through children.
///
/// # Flutter Equivalence
///
/// ```dart
/// abstract class ContainerBoxParentData<ChildType extends RenderObject>
///     extends BoxParentData with ContainerParentDataMixin<ChildType> {}
/// ```
///
/// # Note
///
/// In Rust we don't use sibling pointers. Instead, children are stored
/// in a Vec or similar collection in the parent. This type exists for
/// API compatibility and to store additional metadata.
#[derive(Debug, Clone, Default)]
pub struct ContainerBoxParentData {
    /// The offset at which to paint the child.
    pub offset: Offset,
}

impl ContainerBoxParentData {
    /// Creates new ContainerBoxParentData with the given offset.
    #[inline]
    pub fn new(offset: Offset) -> Self {
        Self { offset }
    }
}

impl From<BoxParentData> for ContainerBoxParentData {
    fn from(data: BoxParentData) -> Self {
        Self {
            offset: data.offset,
        }
    }
}

crate::impl_parent_data!(ContainerBoxParentData);

// ============================================================================
// FlexParentData
// ============================================================================

/// Parent data for children of flex (Row/Column) render objects.
///
/// Extends [`ContainerBoxParentData`] with flex factor and fit mode.
///
/// # Flutter Equivalence
///
/// ```dart
/// class FlexParentData extends ContainerBoxParentData<RenderBox> {
///   int? flex;
///   FlexFit fit = FlexFit.tight;
/// }
/// ```
#[derive(Debug, Clone)]
pub struct FlexParentData {
    /// The offset at which to paint the child.
    pub offset: Offset,

    /// The flex factor for this child.
    ///
    /// If None, the child is inflexible and determines its own size.
    /// If Some, the child's size along the main axis is determined by
    /// dividing free space according to flex factors.
    pub flex: Option<u32>,

    /// How the child should be inscribed into the space allocated by flex.
    pub fit: FlexFit,
}

impl Default for FlexParentData {
    fn default() -> Self {
        Self {
            offset: Offset::ZERO,
            flex: None,
            fit: FlexFit::Tight,
        }
    }
}

crate::impl_parent_data!(FlexParentData);

/// How a flexible child is inscribed into available space.
///
/// # Flutter Equivalence
///
/// ```dart
/// enum FlexFit { tight, loose }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlexFit {
    /// The child is forced to fill available space.
    #[default]
    Tight,

    /// The child can be at most as large as available space.
    Loose,
}

// ============================================================================
// StackParentData
// ============================================================================

/// Parent data for children of stack render objects.
///
/// Extends [`ContainerBoxParentData`] with positioning information.
///
/// # Flutter Equivalence
///
/// ```dart
/// class StackParentData extends ContainerBoxParentData<RenderBox> {
///   double? top, right, bottom, left, width, height;
///   bool get isPositioned => ...;
/// }
/// ```
#[derive(Debug, Clone, Default)]
pub struct StackParentData {
    /// The offset at which to paint the child.
    pub offset: Offset,

    /// Distance from the top edge.
    pub top: Option<f32>,

    /// Distance from the right edge.
    pub right: Option<f32>,

    /// Distance from the bottom edge.
    pub bottom: Option<f32>,

    /// Distance from the left edge.
    pub left: Option<f32>,

    /// Fixed width for the child.
    pub width: Option<f32>,

    /// Fixed height for the child.
    pub height: Option<f32>,
}

impl StackParentData {
    /// Returns whether this child is positioned (has any position constraints).
    #[inline]
    pub fn is_positioned(&self) -> bool {
        self.top.is_some()
            || self.right.is_some()
            || self.bottom.is_some()
            || self.left.is_some()
            || self.width.is_some()
            || self.height.is_some()
    }
}

crate::impl_parent_data!(StackParentData);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_parent_data_default() {
        let data = BoxParentData::default();
        assert_eq!(data.offset, Offset::ZERO);
    }

    #[test]
    fn test_flex_parent_data_default() {
        let data = FlexParentData::default();
        assert_eq!(data.offset, Offset::ZERO);
        assert_eq!(data.flex, None);
        assert_eq!(data.fit, FlexFit::Tight);
    }

    #[test]
    fn test_stack_parent_data_is_positioned() {
        let mut data = StackParentData::default();
        assert!(!data.is_positioned());

        data.top = Some(10.0);
        assert!(data.is_positioned());
    }
}
