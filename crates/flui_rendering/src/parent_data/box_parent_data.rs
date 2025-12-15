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

// ============================================================================
// WrapParentData
// ============================================================================

/// Parent data for children of wrap render objects.
///
/// Extends [`ContainerBoxParentData`] with wrap-specific metadata.
#[derive(Debug, Clone, Default)]
pub struct WrapParentData {
    /// The offset at which to paint the child.
    pub offset: Offset,
}

impl WrapParentData {
    /// Creates new WrapParentData with the given offset.
    #[inline]
    pub fn new(offset: Offset) -> Self {
        Self { offset }
    }
}

crate::impl_parent_data!(WrapParentData);

// ============================================================================
// ListWheelParentData
// ============================================================================

/// Parent data for children of list wheel render objects.
///
/// Extends [`ContainerBoxParentData`] with an index for wheel positioning.
///
/// # Flutter Equivalence
///
/// ```dart
/// class ListWheelParentData extends ContainerBoxParentData<RenderBox> {
///   int? index;
/// }
/// ```
#[derive(Debug, Clone, Default)]
pub struct ListWheelParentData {
    /// The offset at which to paint the child.
    pub offset: Offset,

    /// The index of this child in the wheel.
    pub index: Option<usize>,
}

impl ListWheelParentData {
    /// Creates new ListWheelParentData with the given index.
    #[inline]
    pub fn new(index: usize) -> Self {
        Self {
            offset: Offset::ZERO,
            index: Some(index),
        }
    }
}

crate::impl_parent_data!(ListWheelParentData);

// ============================================================================
// MultiChildLayoutParentData
// ============================================================================

/// Parent data for children of custom multi-child layout render objects.
///
/// Extends [`ContainerBoxParentData`] with an identifier for custom positioning.
///
/// # Flutter Equivalence
///
/// ```dart
/// class MultiChildLayoutParentData extends ContainerBoxParentData<RenderBox> {
///   Object? id;
/// }
/// ```
#[derive(Debug, Clone, Default)]
pub struct MultiChildLayoutParentData {
    /// The offset at which to paint the child.
    pub offset: Offset,

    /// The identifier for this child in the custom layout.
    ///
    /// This is used by [`MultiChildLayoutDelegate`] to identify which child
    /// is being laid out or positioned.
    pub id: Option<String>,
}

impl MultiChildLayoutParentData {
    /// Creates new MultiChildLayoutParentData with the given id.
    #[inline]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            offset: Offset::ZERO,
            id: Some(id.into()),
        }
    }

    /// Creates new MultiChildLayoutParentData without an id.
    #[inline]
    pub fn without_id() -> Self {
        Self::default()
    }
}

crate::impl_parent_data!(MultiChildLayoutParentData);

// ============================================================================
// FlowParentData
// ============================================================================

/// Parent data for children of flow render objects.
///
/// Extends [`ContainerBoxParentData`] with a transform for flow positioning.
///
/// # Flutter Equivalence
///
/// ```dart
/// class FlowParentData extends ContainerBoxParentData<RenderBox> {
///   Matrix4? _transform;
/// }
/// ```
#[derive(Debug, Clone, Default)]
pub struct FlowParentData {
    /// The offset at which to paint the child (used for hit testing).
    pub offset: Offset,

    /// The transform applied to this child during painting.
    ///
    /// This is set by the `FlowDelegate` during `paintChildren`.
    pub transform: Option<[f32; 16]>,
}

impl FlowParentData {
    /// Creates new FlowParentData.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the transform matrix.
    #[inline]
    pub fn set_transform(&mut self, transform: [f32; 16]) {
        self.transform = Some(transform);
    }

    /// Clears the transform matrix.
    #[inline]
    pub fn clear_transform(&mut self) {
        self.transform = None;
    }
}

crate::impl_parent_data!(FlowParentData);

// ============================================================================
// TextParentData
// ============================================================================

/// Parent data for inline children within text render objects.
///
/// Used by `RenderParagraph` and `RenderEditable` for inline widgets.
///
/// # Flutter Equivalence
///
/// ```dart
/// class TextParentData extends ParentData with ContainerParentDataMixin<RenderBox> {
///   TextRange? span;
/// }
/// ```
#[derive(Debug, Clone, Default)]
pub struct TextParentData {
    /// The offset at which to paint the child.
    pub offset: Offset,

    /// The text range that this inline widget replaces.
    ///
    /// If `None`, the widget is not associated with any text range.
    pub span: Option<TextRange>,

    /// The scale factor applied to this inline widget.
    ///
    /// This is used to scale inline widgets based on the text scale factor.
    pub scale: f32,
}

/// A range of text indices.
///
/// Represents a contiguous range of characters in a text string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextRange {
    /// The index of the first character in the range.
    pub start: usize,
    /// The index of the character just after the last character in the range.
    pub end: usize,
}

impl TextRange {
    /// Creates a new text range.
    #[inline]
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Creates an empty text range at the given position.
    #[inline]
    pub fn collapsed(position: usize) -> Self {
        Self {
            start: position,
            end: position,
        }
    }

    /// Returns whether this range is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Returns whether this range is collapsed (empty).
    #[inline]
    pub fn is_collapsed(&self) -> bool {
        self.is_empty()
    }

    /// Returns the length of this range.
    #[inline]
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Returns whether this range is valid (start <= end).
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.start <= self.end
    }

    /// Returns whether the given index is within this range.
    #[inline]
    pub fn contains(&self, index: usize) -> bool {
        index >= self.start && index < self.end
    }
}

impl TextParentData {
    /// Creates new TextParentData with the given span.
    #[inline]
    pub fn new(span: TextRange) -> Self {
        Self {
            offset: Offset::ZERO,
            span: Some(span),
            scale: 1.0,
        }
    }

    /// Creates new TextParentData without a span.
    #[inline]
    pub fn without_span() -> Self {
        Self {
            offset: Offset::ZERO,
            span: None,
            scale: 1.0,
        }
    }
}

crate::impl_parent_data!(TextParentData);

// ============================================================================
// TableCellParentData
// ============================================================================

/// Parent data for children of table render objects.
///
/// Stores the cell position and vertical alignment.
///
/// # Flutter Equivalence
///
/// ```dart
/// class TableCellParentData extends BoxParentData {
///   int? x;
///   int? y;
///   TableCellVerticalAlignment? verticalAlignment;
/// }
/// ```
#[derive(Debug, Clone, Default)]
pub struct TableCellParentData {
    /// The offset at which to paint the child.
    pub offset: Offset,

    /// The column index of this cell.
    pub x: Option<usize>,

    /// The row index of this cell.
    pub y: Option<usize>,

    /// The vertical alignment for this cell.
    ///
    /// If `None`, uses the table's default alignment.
    pub vertical_alignment: Option<TableCellVerticalAlignment>,
}

/// Vertical alignment for a cell in a table.
///
/// # Flutter Equivalence
///
/// ```dart
/// enum TableCellVerticalAlignment {
///   top, middle, bottom, baseline, fill,
///   intrinsicHeight,
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TableCellVerticalAlignment {
    /// Cells with this alignment are placed with their top at the top of the row.
    #[default]
    Top,

    /// Cells with this alignment are vertically centered in the row.
    Middle,

    /// Cells with this alignment are placed with their bottom at the bottom of the row.
    Bottom,

    /// Cells with this alignment are aligned to the baseline of the row.
    ///
    /// The baseline of the row is the baseline of the cell with the largest
    /// distance between its top and its baseline.
    Baseline,

    /// Cells with this alignment are forced to have the height of the row.
    Fill,

    /// Cells with this alignment are sized intrinsically.
    IntrinsicHeight,
}

impl TableCellParentData {
    /// Creates new TableCellParentData with the given position.
    #[inline]
    pub fn new(x: usize, y: usize) -> Self {
        Self {
            offset: Offset::ZERO,
            x: Some(x),
            y: Some(y),
            vertical_alignment: None,
        }
    }
}

crate::impl_parent_data!(TableCellParentData);

// ============================================================================
// ListBodyParentData
// ============================================================================

/// Parent data for children of list body render objects.
///
/// This is a simple container parent data without additional fields.
///
/// # Flutter Equivalence
///
/// ```dart
/// class ListBodyParentData extends ContainerBoxParentData<RenderBox> {}
/// ```
#[derive(Debug, Clone, Default)]
pub struct ListBodyParentData {
    /// The offset at which to paint the child.
    pub offset: Offset,
}

impl ListBodyParentData {
    /// Creates new ListBodyParentData with the given offset.
    #[inline]
    pub fn new(offset: Offset) -> Self {
        Self { offset }
    }
}

crate::impl_parent_data!(ListBodyParentData);

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
