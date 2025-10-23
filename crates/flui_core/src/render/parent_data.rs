//! ParentData - data attached to child elements by their parent
//!
//! ParentData allows parent RenderObjects to attach layout information to their children.
//! This is a core concept in the rendering pipeline, enabling parents to store per-child
//! layout data without maintaining separate data structures.
//!
//! # Architecture
//!
//! The `ParentData` trait provides:
//! - Type-safe downcasting via the `downcast-rs` crate
//! - Debug formatting for all implementations
//! - Thread-safe trait objects (`DowncastSync`)
//!
//! # Common Implementations
//!
//! - `BoxParentData`: Stores offset for positioned children
//! - `ContainerParentData`: Maintains sibling links for linked lists
//! - `ContainerBoxParentData`: Combines both offset and sibling information
//!
//! # Example
//!
//! ```rust,ignore
//! use parent_data::{ParentData, BoxParentData};
//!
//! // Create parent data
//! let mut data = BoxParentData::with_offset(Offset::new(10.0, 20.0));
//!
//! // Store as trait object
//! let boxed: Box<dyn ParentData> = Box::new(data);
//!
//! // Downcast to access concrete type
//! if let Some(box_data) = boxed.downcast_ref::<BoxParentData>() {
//!     println!("Offset: {:?}", box_data.offset);
//! }
//! ```

use std::fmt;

use downcast_rs::{impl_downcast, DowncastSync};
use flui_types::Offset;

/// ParentData - data that a parent RenderObject can attach to child elements
///
/// This trait enables parents to store layout-specific information about each child
/// without maintaining separate data structures. The trait provides type-safe
/// downcasting capabilities, allowing generic code to work with `dyn ParentData`
/// while concrete implementations can access their specific data.
///
/// # Thread Safety
///
/// All ParentData implementations must be `Send + Sync` to enable concurrent
/// rendering operations.
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
///         // Use flex and fit values
///     }
/// }
/// ```
pub trait ParentData: DowncastSync + fmt::Debug {
    /// Try to access this ParentData as ParentDataWithOffset
    ///
    /// Returns Some if this ParentData implements ParentDataWithOffset,
    /// None otherwise. This enables generic access to offset data.
    ///
    /// # Default Implementation
    ///
    /// Returns None. Override in types that implement ParentDataWithOffset.
    fn as_parent_data_with_offset(&self) -> Option<&dyn ParentDataWithOffset> {
        None
    }
}

// Enable downcasting for ParentData trait objects
impl_downcast!(sync ParentData);

/// ParentData with cached offset for efficient hit testing and painting
///
/// This trait is implemented by ParentData types that cache the child's offset
/// (calculated during layout). This allows hit testing and painting to avoid
/// recalculating positions.
///
/// # Implementations
///
/// - `FlexParentData`: Stores offset for Row/Column children
/// - `StackParentData`: Stores offset for Stack children
///
/// # Example
///
/// ```rust,ignore
/// fn hit_test_children(&self, result: &mut HitTestResult, position: Offset, ctx: &RenderContext) -> bool {
///     for &child_id in ctx.children().iter().rev() {
///         // Read cached offset from ParentData
///         let local_offset = if let Some(parent_data) = ctx.tree().parent_data(child_id) {
///             if let Some(data_with_offset) = parent_data.downcast_ref::<dyn ParentDataWithOffset>() {
///                 data_with_offset.offset()
///             } else {
///                 Offset::ZERO
///             }
///         } else {
///             Offset::ZERO
///         };
///
///         let child_position = Offset::new(
///             position.dx - local_offset.dx,
///             position.dy - local_offset.dy,
///         );
///
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

// Enable downcasting for ParentDataWithOffset trait objects
impl_downcast!(sync ParentDataWithOffset);

// Implement ParentData for () (unit type) to represent "no parent data"
//
// This allows render objects that don't need parent data to use `type ParentData = ()`
// without requiring a dedicated NoParentData type.
impl ParentData for () {}

/// Container parent data mixin
///
/// Provides linked list functionality for maintaining sibling relationships.
/// Used by container render objects that need to traverse their children
/// efficiently in both directions.
///
/// # Type Parameters
///
/// - `ChildId`: The type used to identify children (e.g., `NodeId`, `u64`, etc.)
///
/// # Example
///
/// ```rust,ignore
/// let mut data = ContainerParentData::<u64>::new();
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
    ///
    /// # Arguments
    ///
    /// * `sibling` - The new previous sibling, or `None` if this is the first child
    pub fn set_previous_sibling(&mut self, sibling: Option<ChildId>) {
        self.previous_sibling = sibling;
    }

    /// Set the next sibling
    ///
    /// # Arguments
    ///
    /// * `sibling` - The new next sibling, or `None` if this is the last child
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

/// Box parent data - used by RenderBox children
///
/// Stores the offset at which a child should be painted relative to the parent's origin.
/// This is the fundamental positioning mechanism for box-based layouts.
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
/// context.translate(data.offset);
/// child.paint(context);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxParentData {
    /// Offset from parent's origin where this child should be painted
    pub offset: crate::Offset,
}

impl Default for BoxParentData {
    fn default() -> Self {
        Self {
            offset: crate::Offset::ZERO,
        }
    }
}

impl BoxParentData {
    /// Create new box parent data at the origin (0, 0)
    pub const fn new() -> Self {
        Self {
            offset: crate::Offset::ZERO,
        }
    }

    /// Create box parent data with a specific offset
    ///
    /// # Arguments
    ///
    /// * `offset` - The offset from the parent's origin
    pub const fn with_offset(offset: crate::Offset) -> Self {
        Self { offset }
    }

    /// Create box parent data with x and y coordinates
    ///
    /// # Arguments
    ///
    /// * `x` - Horizontal offset from parent's left edge
    /// * `y` - Vertical offset from parent's top edge
    pub fn with_xy(x: f32, y: f32) -> Self {
        Self {
            offset: crate::Offset::new(x, y),
        }
    }

    /// Set the offset
    ///
    /// # Arguments
    ///
    /// * `offset` - The new offset from the parent's origin
    pub fn set_offset(&mut self, offset: crate::Offset) {
        self.offset = offset;
    }

    /// Set the offset using x and y coordinates
    pub fn set_xy(&mut self, x: f32, y: f32) {
        self.offset = crate::Offset::new(x, y);
    }

    /// Move the offset by a delta
    ///
    /// # Arguments
    ///
    /// * `delta` - The offset to add to the current offset
    pub fn translate(&mut self, delta: crate::Offset) {
        self.offset = self.offset + delta;
    }

    /// Reset the offset to the origin
    pub fn reset(&mut self) {
        self.offset = crate::Offset::ZERO;
    }

    /// Check if this child is at the origin
    pub fn is_at_origin(&self) -> bool {
        self.offset == crate::Offset::ZERO
    }
}

impl ParentData for BoxParentData {}

/// Container box parent data - combines container and box parent data
///
/// This is the most commonly used parent data type, combining both:
/// - Positioning information (from `BoxParentData`)
/// - Sibling links (from `ContainerParentData`)
///
/// Used by multi-child render objects like Row, Column, Flex, Wrap, etc.
///
/// # Type Parameters
///
/// - `ChildId`: The type used to identify children
///
/// # Example
///
/// ```rust,ignore
/// let mut data = ContainerBoxParentData::<u64>::new();
///
/// // Set positioning
/// data.set_offset(Offset::new(10.0, 20.0));
///
/// // Set up sibling links
/// data.container_data.set_previous_sibling(Some(1));
/// data.container_data.set_next_sibling(Some(3));
///
/// // Access combined data
/// println!("Offset: {:?}", data.offset());
/// println!("Has siblings: {}", !data.container_data.is_only());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerBoxParentData<ChildId> {
    /// Box parent data (offset)
    pub box_data: BoxParentData,

    /// Container parent data (siblings)
    pub container_data: ContainerParentData<ChildId>,
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
    pub fn with_offset(offset: crate::Offset) -> Self {
        Self {
            box_data: BoxParentData::with_offset(offset),
            container_data: ContainerParentData::default(),
        }
    }

    /// Create container box parent data with offset and siblings
    pub fn with_offset_and_siblings(
        offset: crate::Offset,
        previous: Option<ChildId>,
        next: Option<ChildId>,
    ) -> Self {
        Self {
            box_data: BoxParentData::with_offset(offset),
            container_data: ContainerParentData::with_siblings(previous, next),
        }
    }

    /// Get the offset
    pub fn offset(&self) -> crate::Offset {
        self.box_data.offset
    }

    /// Set the offset
    pub fn set_offset(&mut self, offset: crate::Offset) {
        self.box_data.set_offset(offset);
    }

    /// Set the offset using x and y coordinates
    pub fn set_xy(&mut self, x: f32, y: f32) {
        self.box_data.set_xy(x, y);
    }

    /// Move the offset by a delta
    pub fn translate(&mut self, delta: crate::Offset) {
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
    ChildId: fmt::Debug + Send + Sync + 'static
{}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_parent_data_new() {
        let data: ContainerParentData<u64> = ContainerParentData::new();
        assert_eq!(data.previous_sibling, None);
        assert_eq!(data.next_sibling, None);
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
    fn test_container_parent_data_siblings() {
        let mut data = ContainerParentData::new();
        data.set_previous_sibling(Some(1u64));
        data.set_next_sibling(Some(2u64));

        assert_eq!(data.previous_sibling, Some(1));
        assert_eq!(data.next_sibling, Some(2));
        assert!(!data.is_only());
    }

    #[test]
    fn test_container_parent_data_clear_siblings() {
        let mut data = ContainerParentData::with_siblings(Some(1u64), Some(2u64));
        data.clear_siblings();

        assert_eq!(data.previous_sibling, None);
        assert_eq!(data.next_sibling, None);
        assert!(data.is_only());
    }

    #[test]
    fn test_container_parent_data_is_first() {
        let mut data = ContainerParentData::new();
        assert!(data.is_first());

        data.set_previous_sibling(Some(1u64));
        assert!(!data.is_first());
    }

    #[test]
    fn test_container_parent_data_is_last() {
        let mut data = ContainerParentData::new();
        assert!(data.is_last());

        data.set_next_sibling(Some(1u64));
        assert!(!data.is_last());
    }

    #[test]
    fn test_box_parent_data_default() {
        let data = BoxParentData::default();
        assert_eq!(data.offset, crate::Offset::ZERO);
        assert!(data.is_at_origin());
    }

    #[test]
    fn test_box_parent_data_with_offset() {
        let offset = crate::Offset::new(10.0, 20.0);
        let data = BoxParentData::with_offset(offset);

        assert_eq!(data.offset, offset);
        assert!(!data.is_at_origin());
    }

    #[test]
    fn test_box_parent_data_with_xy() {
        let data = BoxParentData::with_xy(15.0, 25.0);
        assert_eq!(data.offset, crate::Offset::new(15.0, 25.0));
    }

    #[test]
    fn test_box_parent_data_set_offset() {
        let mut data = BoxParentData::new();
        let offset = crate::Offset::new(5.0, 15.0);

        data.set_offset(offset);
        assert_eq!(data.offset, offset);
    }

    #[test]
    fn test_box_parent_data_set_xy() {
        let mut data = BoxParentData::new();
        data.set_xy(30.0, 40.0);
        assert_eq!(data.offset, crate::Offset::new(30.0, 40.0));
    }

    #[test]
    fn test_box_parent_data_translate() {
        let mut data = BoxParentData::with_xy(10.0, 20.0);
        data.translate(crate::Offset::new(5.0, 10.0));
        assert_eq!(data.offset, crate::Offset::new(15.0, 30.0));
    }

    #[test]
    fn test_box_parent_data_reset() {
        let mut data = BoxParentData::with_xy(10.0, 20.0);
        data.reset();
        assert_eq!(data.offset, crate::Offset::ZERO);
        assert!(data.is_at_origin());
    }

    #[test]
    fn test_box_parent_data_downcast() {
        let data = BoxParentData::new();
        let boxed: Box<dyn ParentData> = Box::new(data);

        // Test is() check
        assert!(boxed.is::<BoxParentData>());
        assert!(!boxed.is::<ContainerParentData<u64>>());

        // Test downcast_ref
        let downcasted = boxed.downcast_ref::<BoxParentData>().unwrap();
        assert_eq!(downcasted.offset, crate::Offset::ZERO);
    }

    #[test]
    fn test_container_box_parent_data_new() {
        let data: ContainerBoxParentData<u64> = ContainerBoxParentData::new();
        assert_eq!(data.offset(), crate::Offset::ZERO);
        assert_eq!(data.previous_sibling(), None);
        assert_eq!(data.next_sibling(), None);
        assert!(data.is_only());
        assert!(data.is_at_origin());
    }

    #[test]
    fn test_container_box_parent_data_with_offset() {
        let offset = crate::Offset::new(50.0, 100.0);
        let data: ContainerBoxParentData<u64> = ContainerBoxParentData::with_offset(offset);
        assert_eq!(data.offset(), offset);
        assert!(data.is_only());
    }

    #[test]
    fn test_container_box_parent_data_with_offset_and_siblings() {
        let offset = crate::Offset::new(10.0, 20.0);
        let data = ContainerBoxParentData::with_offset_and_siblings(offset, Some(1u64), Some(2u64));

        assert_eq!(data.offset(), offset);
        assert_eq!(data.previous_sibling(), Some(&1));
        assert_eq!(data.next_sibling(), Some(&2));
        assert!(!data.is_only());
    }

    #[test]
    fn test_container_box_parent_data_full() {
        let mut data = ContainerBoxParentData::new();

        data.set_offset(crate::Offset::new(100.0, 200.0));
        data.set_previous_sibling(Some(10u64));
        data.set_next_sibling(Some(20u64));

        assert_eq!(data.offset(), crate::Offset::new(100.0, 200.0));
        assert_eq!(data.previous_sibling(), Some(&10));
        assert_eq!(data.next_sibling(), Some(&20));
        assert!(!data.is_first());
        assert!(!data.is_last());
    }

    #[test]
    fn test_container_box_parent_data_translate() {
        let mut data = ContainerBoxParentData::with_offset(crate::Offset::new(10.0, 20.0));
        data.translate(crate::Offset::new(5.0, 10.0));
        assert_eq!(data.offset(), crate::Offset::new(15.0, 30.0));
    }

    #[test]
    fn test_container_box_parent_data_reset() {
        let mut data = ContainerBoxParentData::with_offset(crate::Offset::new(50.0, 75.0));
        data.reset_offset();
        assert_eq!(data.offset(), crate::Offset::ZERO);
        assert!(data.is_at_origin());
    }

    #[test]
    fn test_container_box_parent_data_clear_siblings() {
        let mut data = ContainerBoxParentData::with_offset_and_siblings(
            crate::Offset::ZERO,
            Some(1u64),
            Some(2u64),
        );

        data.clear_siblings();
        assert!(data.is_only());
    }

    #[test]
    fn test_parent_data_downcast_mut() {
        let data = BoxParentData::new();
        let mut boxed: Box<dyn ParentData> = Box::new(data);

        // Test downcast_mut
        let downcasted = boxed.downcast_mut::<BoxParentData>().unwrap();
        downcasted.set_offset(crate::Offset::new(50.0, 75.0));

        assert_eq!(
            boxed.downcast_ref::<BoxParentData>().unwrap().offset,
            crate::Offset::new(50.0, 75.0)
        );
    }

    #[test]
    fn test_parent_data_downcast_owned() {
        let mut data = BoxParentData::new();
        data.set_offset(crate::Offset::new(10.0, 20.0));

        let boxed: Box<dyn ParentData> = Box::new(data);

        // Consume and downcast
        let owned: Box<BoxParentData> = boxed.downcast().ok().unwrap();
        assert_eq!(owned.offset, crate::Offset::new(10.0, 20.0));
    }

    #[test]
    fn test_unit_parent_data() {
        let data = ();
        let boxed: Box<dyn ParentData> = Box::new(data);
        assert!(boxed.is::<()>());
    }

    #[test]
    fn test_container_box_parent_data_as_trait_object() {
        let data = ContainerBoxParentData::<u64>::with_offset(crate::Offset::new(10.0, 20.0));
        let boxed: Box<dyn ParentData> = Box::new(data);

        assert!(boxed.is::<ContainerBoxParentData<u64>>());
        let downcasted = boxed.downcast_ref::<ContainerBoxParentData<u64>>().unwrap();
        assert_eq!(downcasted.offset(), crate::Offset::new(10.0, 20.0));
    }

    #[test]
    fn test_container_parent_data_equality() {
        let data1 = ContainerParentData::with_siblings(Some(1u64), Some(2u64));
        let data2 = ContainerParentData::with_siblings(Some(1u64), Some(2u64));
        let data3 = ContainerParentData::with_siblings(Some(2u64), Some(3u64));

        assert_eq!(data1, data2);
        assert_ne!(data1, data3);
    }

    #[test]
    fn test_container_box_parent_data_equality() {
        let data1 = ContainerBoxParentData::with_offset_and_siblings(
            crate::Offset::new(10.0, 20.0),
            Some(1u64),
            Some(2u64),
        );
        let data2 = ContainerBoxParentData::with_offset_and_siblings(
            crate::Offset::new(10.0, 20.0),
            Some(1u64),
            Some(2u64),
        );
        let data3 = ContainerBoxParentData::with_offset_and_siblings(
            crate::Offset::new(15.0, 25.0),
            Some(1u64),
            Some(2u64),
        );

        assert_eq!(data1, data2);
        assert_ne!(data1, data3);
    }
}