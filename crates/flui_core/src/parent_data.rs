//! ParentData - data attached to child elements by their parent
//!
//! ParentData allows parent RenderObjects to attach layout information to their children.

use std::fmt;

use downcast_rs::{impl_downcast, DowncastSync};

/// ParentData - data that a parent RenderObject can attach to child elements
///
/// Similar to Flutter's ParentData. This allows the parent to store layout-specific
/// information about each child without having to maintain a separate data structure.
///
/// The trait provides downcasting capabilities via the `downcast-rs` crate.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug)]
/// struct FlexParentData {
///     flex: i32,
///     fit: FlexFit,
/// }
///
/// impl ParentData for FlexParentData {}
///
/// // Use downcast to access concrete type:
/// fn get_flex(parent_data: &dyn ParentData) -> Option<i32> {
///     parent_data.downcast_ref::<FlexParentData>().map(|d| d.flex)
/// }
/// ```
pub trait ParentData: DowncastSync + fmt::Debug {}

// Enable downcasting for ParentData trait objects
impl_downcast!(sync ParentData);

/// Container parent data mixin
///
/// Similar to Flutter's ContainerParentDataMixin. Adds prev/next sibling pointers
/// for linked list of children.
#[derive(Debug, Clone)]
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
    /// Create new container parent data
    pub const fn new() -> Self {
        Self {
            previous_sibling: None,
            next_sibling: None,
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
}

/// Box parent data - used by RenderBox children
///
/// Similar to Flutter's BoxParentData. Stores the offset where the child
/// should be painted relative to the parent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxParentData {
    /// Offset from parent's origin
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
    /// Create new box parent data at the origin
    pub const fn new() -> Self {
        Self {
            offset: crate::Offset::ZERO,
        }
    }

    /// Create box parent data with an offset
    pub const fn with_offset(offset: crate::Offset) -> Self {
        Self { offset }
    }

    /// Set the offset
    pub fn set_offset(&mut self, offset: crate::Offset) {
        self.offset = offset;
    }
}

impl ParentData for BoxParentData {}

/// Container box parent data - combines container and box parent data
///
/// Similar to Flutter's ContainerBoxParentData. Used by parents that need both
/// offset information and linked list of children.
#[derive(Debug, Clone)]
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
    /// Create new container box parent data
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the offset
    pub fn offset(&self) -> crate::Offset {
        self.box_data.offset
    }

    /// Set the offset
    pub fn set_offset(&mut self, offset: crate::Offset) {
        self.box_data.set_offset(offset);
    }

    /// Get the previous sibling
    pub fn previous_sibling(&self) -> Option<&ChildId> {
        self.container_data.previous_sibling.as_ref()
    }

    /// Get the next sibling
    pub fn next_sibling(&self) -> Option<&ChildId> {
        self.container_data.next_sibling.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_parent_data_new() {
        let data: ContainerParentData<u64> = ContainerParentData::new();
        assert_eq!(data.previous_sibling, None);
        assert_eq!(data.next_sibling, None);
    }

    #[test]
    fn test_container_parent_data_siblings() {
        let mut data = ContainerParentData::new();
        data.set_previous_sibling(Some(1u64));
        data.set_next_sibling(Some(2u64));

        assert_eq!(data.previous_sibling, Some(1));
        assert_eq!(data.next_sibling, Some(2));
    }

    #[test]
    fn test_box_parent_data_default() {
        let data = BoxParentData::default();
        assert_eq!(data.offset, crate::Offset::ZERO);
    }

    #[test]
    fn test_box_parent_data_with_offset() {
        let offset = crate::Offset::new(10.0, 20.0);
        let data = BoxParentData::with_offset(offset);

        assert_eq!(data.offset, offset);
    }

    #[test]
    fn test_box_parent_data_set_offset() {
        let mut data = BoxParentData::new();
        let offset = crate::Offset::new(5.0, 15.0);

        data.set_offset(offset);
        assert_eq!(data.offset, offset);
    }

    #[test]
    fn test_box_parent_data_downcast() {
        let data = BoxParentData::new();
        let boxed: Box<dyn ParentData> = Box::new(data);

        // Test is() check
        assert!(boxed.is::<BoxParentData>());

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
    }

    #[test]
    fn test_container_box_parent_data_full() {
        let mut data = ContainerBoxParentData::new();

        data.set_offset(crate::Offset::new(100.0, 200.0));
        data.container_data.set_previous_sibling(Some(10u64));
        data.container_data.set_next_sibling(Some(20u64));

        assert_eq!(data.offset(), crate::Offset::new(100.0, 200.0));
        assert_eq!(data.previous_sibling(), Some(&10));
        assert_eq!(data.next_sibling(), Some(&20));
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
}
