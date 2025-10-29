//! ParentData - layout-specific data attached to children by their parent
//!
//! The ParentData system allows parent RenderObjects to attach metadata to their children
//! without maintaining separate data structures. This is a core concept in the rendering
//! pipeline, enabling parents to store per-child layout information efficiently.
//!
//! # Architecture
//!
//! The `ParentData` trait provides:
//! - **Type-safe downcasting** via `downcast-rs` for accessing concrete types
//! - **Debug formatting** for all implementations
//! - **Thread safety** (`Send + Sync`) for concurrent rendering
//!
//! # Common Use Cases
//!
//! - **Flex Layouts**: Store flex factor and fit mode for each child
//! - **Stack Layouts**: Store positioning (top, left, width, height) for each child
//! - **Offset Storage**: Cache calculated child positions for efficient painting/hit-testing
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_core::{ParentData, BoxParentData};
//! use flui_types::Offset;
//!
//! // Create parent data with offset
//! let data = BoxParentData::with_offset(Offset::new(10.0, 20.0));
//!
//! // Store as trait object
//! let boxed: Box<dyn ParentData> = Box::new(data);
//!
//! // Downcast to access concrete type
//! if let Some(box_data) = boxed.downcast_ref::<BoxParentData>() {
//!     println!("Offset: {:?}", box_data.offset());
//! }
//! ```

use std::fmt;

use downcast_rs::{DowncastSync, impl_downcast};
use flui_types::Offset;

/// ParentData - metadata that a parent RenderObject attaches to child elements
///
/// This trait enables parents to store layout-specific information about each child
/// without maintaining separate data structures. The trait provides type-safe
/// downcasting, allowing generic code to work with `dyn ParentData` while concrete
/// implementations access their specific data.
///
/// # Thread Safety
///
/// All ParentData implementations must be `Send + Sync` to enable concurrent
/// rendering operations across threads.
///
/// # Example Implementation
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// struct FlexParentData {
///     flex: i32,
///     fit: FlexFit,
/// }
///
/// impl ParentData for FlexParentData {}
///
/// // Use in layout code:
/// fn layout_child(parent_data: &dyn ParentData) {
///     if let Some(flex_data) = parent_data.downcast_ref::<FlexParentData>() {
///         let flex_value = flex_data.flex;
///     }
/// }
/// ```
pub trait ParentData: DowncastSync + fmt::Debug {
    /// Try to access this ParentData as ParentDataWithOffset
    ///
    /// Returns `Some` if this ParentData implements ParentDataWithOffset,
    /// `None` otherwise. This enables generic access to offset data without
    /// knowing the concrete type.
    ///
    /// # Default Implementation
    ///
    /// Returns `None`. Override in types that implement ParentDataWithOffset.
    fn as_parent_data_with_offset(&self) -> Option<&dyn ParentDataWithOffset> {
        None
    }
}

// Enable downcasting for ParentData trait objects
impl_downcast!(sync ParentData);

/// ParentData with cached offset for efficient hit testing and painting
///
/// This trait is implemented by ParentData types that cache the child's offset
/// (calculated during layout). This avoids recalculating positions during
/// painting and hit testing.
///
/// # Common Implementations
///
/// - `BoxParentData`: Simple offset storage
/// - `ContainerBoxParentData`: Offset + sibling links
/// - Custom layout-specific ParentData types
///
/// # Example
///
/// ```rust,ignore
/// fn hit_test_children(&self, result: &mut HitTestResult, position: Offset, ctx: &RenderContext) -> bool {
///     for &child_id in ctx.children().iter().rev() {
///         // Read cached offset from ParentData
///         let child_offset = if let Some(parent_data) = ctx.tree().parent_data(child_id) {
///             if let Some(data_with_offset) = parent_data.as_parent_data_with_offset() {
///                 data_with_offset.offset()
///             } else {
///                 Offset::ZERO
///             }
///         } else {
///             Offset::ZERO
///         };
///
///         let child_position = position - child_offset;
///         if ctx.hit_test_child(child_id, result, child_position) {
///             return true;
///         }
///     }
///     false
/// }
/// ```
pub trait ParentDataWithOffset: ParentData {
    /// Get the cached offset for this child
    ///
    /// This offset is calculated during layout and read during paint/hit_test.
    fn offset(&self) -> Offset;

    /// Set the cached offset for this child
    ///
    /// Called by parent RenderObject during layout.
    fn set_offset(&mut self, offset: Offset);
}

// Implement ParentData for () (unit type) to represent "no parent data"
//
// This allows RenderObjects that don't need parent data to use simple APIs
// without requiring a dedicated NoParentData type.
impl ParentData for () {}

/// Box parent data - stores offset for positioned children
///
/// The fundamental ParentData type for box-based layouts. Stores the offset
/// at which a child should be painted relative to the parent's origin.
///
/// # Coordinate System
///
/// - Origin is at parent's top-left corner
/// - Positive x moves right, positive y moves down
/// - Offset is applied during painting, not during layout
///
/// # Example
///
/// ```rust,ignore
/// let mut data = BoxParentData::with_offset(Offset::new(10.0, 20.0));
/// data.set_offset(Offset::new(15.0, 25.0));
///
/// // In paint code:
/// painter.translate(data.offset());
/// child.paint(painter);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxParentData {
    /// Offset from parent's origin where this child should be painted
    offset: Offset,
}

impl Default for BoxParentData {
    fn default() -> Self {
        Self {
            offset: Offset::ZERO,
        }
    }
}

impl BoxParentData {
    /// Create new box parent data at the origin (0, 0)
    pub const fn new() -> Self {
        Self {
            offset: Offset::ZERO,
        }
    }

    /// Create box parent data with a specific offset
    pub const fn with_offset(offset: Offset) -> Self {
        Self { offset }
    }

    /// Create box parent data with x and y coordinates
    pub fn with_xy(x: f32, y: f32) -> Self {
        Self {
            offset: Offset::new(x, y),
        }
    }

    /// Set the offset
    pub fn set_offset(&mut self, offset: Offset) {
        self.offset = offset;
    }

    /// Set the offset using x and y coordinates
    pub fn set_xy(&mut self, x: f32, y: f32) {
        self.offset = Offset::new(x, y);
    }

    /// Move the offset by a delta
    pub fn translate(&mut self, delta: Offset) {
        self.offset = self.offset + delta;
    }

    /// Reset the offset to the origin
    pub fn reset(&mut self) {
        self.offset = Offset::ZERO;
    }

    /// Check if this child is at the origin
    pub fn is_at_origin(&self) -> bool {
        self.offset == Offset::ZERO
    }
}

impl ParentData for BoxParentData {
    fn as_parent_data_with_offset(&self) -> Option<&dyn ParentDataWithOffset> {
        Some(self)
    }
}

impl ParentDataWithOffset for BoxParentData {
    fn offset(&self) -> Offset {
        self.offset
    }

    fn set_offset(&mut self, offset: Offset) {
        self.offset = offset;
    }
}

/// Container parent data - sibling links for efficient traversal
///
/// Provides linked list functionality for maintaining sibling relationships.
/// Used by container RenderObjects that need to traverse their children
/// efficiently in both directions.
///
/// # Type Parameters
///
/// - `ChildId`: The type used to identify children (typically `ElementId`)
///
/// # Example
///
/// ```rust,ignore
/// let mut data = ContainerParentData::<ElementId>::new();
/// data.set_previous_sibling(Some(1));
/// data.set_next_sibling(Some(3));
///
/// // Traverse siblings
/// if let Some(next) = data.next_sibling {
///     // Process next sibling
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerParentData<ChildId> {
    /// Previous sibling in the parent's child list
    pub previous_sibling: Option<ChildId>,

    /// Next sibling in the parent's child list
    pub next_sibling: Option<ChildId>,
}

impl<ChildId> Default for ContainerParentData<ChildId> {
    fn default() -> Self {
        Self {
            previous_sibling: None,
            next_sibling: None,
        }
    }
}

impl<ChildId> ContainerParentData<ChildId> {
    /// Create new container parent data with no siblings
    pub const fn new() -> Self {
        Self {
            previous_sibling: None,
            next_sibling: None,
        }
    }

    /// Create container parent data with specific siblings
    pub fn with_siblings(previous: Option<ChildId>, next: Option<ChildId>) -> Self {
        Self {
            previous_sibling: previous,
            next_sibling: next,
        }
    }

    /// Set the previous sibling
    pub fn set_previous_sibling(&mut self, sibling: Option<ChildId>) {
        self.previous_sibling = sibling;
    }

    /// Set the next sibling
    pub fn set_next_sibling(&mut self, sibling: Option<ChildId>) {
        self.next_sibling = sibling;
    }

    /// Clear both sibling links
    pub fn clear_siblings(&mut self) {
        self.previous_sibling = None;
        self.next_sibling = None;
    }

    /// Check if this is the first child (no previous sibling)
    pub fn is_first(&self) -> bool {
        self.previous_sibling.is_none()
    }

    /// Check if this is the last child (no next sibling)
    pub fn is_last(&self) -> bool {
        self.next_sibling.is_none()
    }

    /// Check if this is the only child (no siblings)
    pub fn is_only(&self) -> bool {
        self.is_first() && self.is_last()
    }
}

/// Container box parent data - combines offset and sibling links
///
/// The most commonly used ParentData type, combining both:
/// - Positioning information (from `BoxParentData`)
/// - Sibling links (from `ContainerParentData`)
///
/// Used by multi-child RenderObjects like Row, Column, Flex, Wrap, etc.
///
/// # Type Parameters
///
/// - `ChildId`: The type used to identify children (typically `ElementId`)
///
/// # Example
///
/// ```rust,ignore
/// let mut data = ContainerBoxParentData::<ElementId>::new();
///
/// // Set positioning
/// data.set_offset(Offset::new(10.0, 20.0));
///
/// // Set up sibling links
/// data.set_previous_sibling(Some(1));
/// data.set_next_sibling(Some(3));
///
/// // Access combined data
/// println!("Offset: {:?}", data.offset());
/// println!("Has siblings: {}", !data.is_only());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerBoxParentData<ChildId> {
    /// Box parent data (offset)
    box_data: BoxParentData,

    /// Container parent data (siblings)
    container_data: ContainerParentData<ChildId>,
}

impl<ChildId> Default for ContainerBoxParentData<ChildId> {
    fn default() -> Self {
        Self {
            box_data: BoxParentData::default(),
            container_data: ContainerParentData::default(),
        }
    }
}

impl<ChildId> ContainerBoxParentData<ChildId> {
    /// Create new container box parent data at origin with no siblings
    pub fn new() -> Self {
        Self::default()
    }

    /// Create container box parent data with a specific offset
    pub fn with_offset(offset: Offset) -> Self {
        Self {
            box_data: BoxParentData::with_offset(offset),
            container_data: ContainerParentData::default(),
        }
    }

    /// Create container box parent data with offset and siblings
    pub fn with_offset_and_siblings(
        offset: Offset,
        previous: Option<ChildId>,
        next: Option<ChildId>,
    ) -> Self {
        Self {
            box_data: BoxParentData::with_offset(offset),
            container_data: ContainerParentData::with_siblings(previous, next),
        }
    }

    /// Get the offset
    pub fn offset(&self) -> Offset {
        self.box_data.offset
    }

    /// Set the offset
    pub fn set_offset(&mut self, offset: Offset) {
        self.box_data.set_offset(offset);
    }

    /// Set the offset using x and y coordinates
    pub fn set_xy(&mut self, x: f32, y: f32) {
        self.box_data.set_xy(x, y);
    }

    /// Move the offset by a delta
    pub fn translate(&mut self, delta: Offset) {
        self.box_data.translate(delta);
    }

    /// Reset the offset to the origin
    pub fn reset_offset(&mut self) {
        self.box_data.reset();
    }

    /// Get the previous sibling
    pub fn previous_sibling(&self) -> Option<&ChildId> {
        self.container_data.previous_sibling.as_ref()
    }

    /// Get the next sibling
    pub fn next_sibling(&self) -> Option<&ChildId> {
        self.container_data.next_sibling.as_ref()
    }

    /// Set the previous sibling
    pub fn set_previous_sibling(&mut self, sibling: Option<ChildId>) {
        self.container_data.set_previous_sibling(sibling);
    }

    /// Set the next sibling
    pub fn set_next_sibling(&mut self, sibling: Option<ChildId>) {
        self.container_data.set_next_sibling(sibling);
    }

    /// Clear both sibling links
    pub fn clear_siblings(&mut self) {
        self.container_data.clear_siblings();
    }

    /// Check if this is the first child
    pub fn is_first(&self) -> bool {
        self.container_data.is_first()
    }

    /// Check if this is the last child
    pub fn is_last(&self) -> bool {
        self.container_data.is_last()
    }

    /// Check if this is the only child
    pub fn is_only(&self) -> bool {
        self.container_data.is_only()
    }

    /// Check if this child is at the origin
    pub fn is_at_origin(&self) -> bool {
        self.box_data.is_at_origin()
    }
}

impl<ChildId> ParentData for ContainerBoxParentData<ChildId>
where
    ChildId: fmt::Debug + Send + Sync + 'static,
{
    fn as_parent_data_with_offset(&self) -> Option<&dyn ParentDataWithOffset> {
        Some(self)
    }
}

impl<ChildId> ParentDataWithOffset for ContainerBoxParentData<ChildId>
where
    ChildId: fmt::Debug + Send + Sync + 'static,
{
    fn offset(&self) -> Offset {
        self.box_data.offset
    }

    fn set_offset(&mut self, offset: Offset) {
        self.box_data.offset = offset;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_parent_data_new() {
        let data = BoxParentData::new();
        assert_eq!(data.offset(), Offset::ZERO);
        assert!(data.is_at_origin());
    }

    #[test]
    fn test_box_parent_data_with_offset() {
        let offset = Offset::new(10.0, 20.0);
        let data = BoxParentData::with_offset(offset);
        assert_eq!(data.offset(), offset);
        assert!(!data.is_at_origin());
    }

    #[test]
    fn test_box_parent_data_with_xy() {
        let data = BoxParentData::with_xy(15.0, 25.0);
        assert_eq!(data.offset(), Offset::new(15.0, 25.0));
    }

    #[test]
    fn test_box_parent_data_set_offset() {
        let mut data = BoxParentData::new();
        let offset = Offset::new(5.0, 15.0);
        data.set_offset(offset);
        assert_eq!(data.offset(), offset);
    }

    #[test]
    fn test_box_parent_data_translate() {
        let mut data = BoxParentData::with_xy(10.0, 20.0);
        data.translate(Offset::new(5.0, 10.0));
        assert_eq!(data.offset(), Offset::new(15.0, 30.0));
    }

    #[test]
    fn test_box_parent_data_downcast() {
        let data = BoxParentData::new();
        let boxed: Box<dyn ParentData> = Box::new(data);

        assert!(boxed.is::<BoxParentData>());
        let downcasted = boxed.downcast_ref::<BoxParentData>().unwrap();
        assert_eq!(downcasted.offset(), Offset::ZERO);
    }

    #[test]
    fn test_container_parent_data_new() {
        let data: ContainerParentData<u64> = ContainerParentData::new();
        assert!(data.is_only());
    }

    #[test]
    fn test_container_parent_data_with_siblings() {
        let data = ContainerParentData::with_siblings(Some(1u64), Some(2u64));
        assert_eq!(data.previous_sibling, Some(1));
        assert_eq!(data.next_sibling, Some(2));
        assert!(!data.is_first());
        assert!(!data.is_last());
    }

    #[test]
    fn test_container_box_parent_data_new() {
        let data: ContainerBoxParentData<u64> = ContainerBoxParentData::new();
        assert_eq!(data.offset(), Offset::ZERO);
        assert!(data.is_only());
        assert!(data.is_at_origin());
    }

    #[test]
    fn test_container_box_parent_data_full() {
        let mut data = ContainerBoxParentData::new();
        data.set_offset(Offset::new(100.0, 200.0));
        data.set_previous_sibling(Some(10u64));
        data.set_next_sibling(Some(20u64));

        assert_eq!(data.offset(), Offset::new(100.0, 200.0));
        assert_eq!(data.previous_sibling(), Some(&10));
        assert_eq!(data.next_sibling(), Some(&20));
        assert!(!data.is_first());
        assert!(!data.is_last());
    }

    #[test]
    fn test_unit_parent_data() {
        let data = ();
        let boxed: Box<dyn ParentData> = Box::new(data);
        assert!(boxed.is::<()>());
    }
}
