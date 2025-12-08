//! ParentData: per-child layout metadata with Flutter compliance.
//!
//! This module implements Flutter's ParentData system, enabling parents to attach
//! layout-specific metadata directly to their children without external side maps.
//!
//! # Common ParentData Types
//!
//! | Type | Purpose | Flutter Equivalent |
//! |------|---------|-------------------|
//! | `BoxParentData` | Simple offset | `BoxParentData` |
//! | `ContainerBoxParentData` | Offset + siblings | `ContainerBoxParentData` |
//! | `ContainerParentData` | Just siblings | (not in Flutter) |
//! | `()` (unit) | No parent data | `null` |

use std::any::Any;
use std::fmt;

use flui_types::Offset;

// ============================================================================
// SEALED HELPER TRAIT
// ============================================================================

mod sealed {
    use super::*;

    /// Internal sealed helper that supplies `as_any_parent_data()` for all
    /// `ParentData` implementors.
    pub trait AsAnyParentData: fmt::Debug + Send + Sync + 'static {
        fn as_any_parent_data(&self) -> &dyn Any;
        fn as_any_parent_data_mut(&mut self) -> &mut dyn Any;
    }

    impl<T> AsAnyParentData for T
    where
        T: fmt::Debug + Send + Sync + 'static,
    {
        fn as_any_parent_data(&self) -> &dyn Any {
            self
        }

        fn as_any_parent_data_mut(&mut self) -> &mut dyn Any {
            self
        }
    }
}

// ============================================================================
// MAIN PARENTDATA TRAIT
// ============================================================================

/// ParentData - metadata that a parent RenderObject attaches to child elements.
///
/// This trait enables parents to store layout-specific information about each
/// child without maintaining separate data structures.
pub trait ParentData: sealed::AsAnyParentData {
    /// Returns immutable type-erased access for downcasting.
    fn as_any(&self) -> &dyn Any {
        self.as_any_parent_data()
    }

    /// Returns mutable type-erased access for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self.as_any_parent_data_mut()
    }

    /// Returns offset capability if this ParentData supports it.
    fn as_parent_data_with_offset(&self) -> Option<&dyn ParentDataWithOffset> {
        None
    }

    /// Returns mutable offset capability if this ParentData supports it.
    fn as_parent_data_with_offset_mut(&mut self) -> Option<&mut dyn ParentDataWithOffset> {
        None
    }

    /// Detaches this ParentData (cleanup hook).
    fn detach(&mut self) {
        // Default: no-op
    }
}

// ============================================================================
// PARENTDATA WITH OFFSET TRAIT
// ============================================================================

/// ParentData with cached offset for efficient hit testing and painting.
///
/// This trait is implemented by ParentData types that cache the child's offset
/// (calculated during layout).
pub trait ParentDataWithOffset: ParentData {
    /// Returns the cached layout offset in parent-local coordinates.
    fn offset(&self) -> Offset;

    /// Sets the cached layout offset.
    fn set_offset(&mut self, offset: Offset);

    /// Translates the offset by a delta.
    fn translate_offset(&mut self, delta: Offset) {
        let current = self.offset();
        self.set_offset(current + delta);
    }

    /// Checks if the child is at the origin (0, 0).
    fn is_at_origin(&self) -> bool {
        self.offset() == Offset::ZERO
    }
}

// ============================================================================
// UNIT TYPE IMPLEMENTATION (NO PARENT DATA)
// ============================================================================

impl ParentData for () {}

// ============================================================================
// BOX PARENT DATA
// ============================================================================

/// Box parent data - stores offset for positioned children.
///
/// The fundamental ParentData type for box-based layouts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxParentData {
    offset: Offset,
}

impl Default for BoxParentData {
    fn default() -> Self {
        Self::new()
    }
}

impl BoxParentData {
    /// Creates a new BoxParentData at the origin (0, 0).
    pub const fn new() -> Self {
        Self {
            offset: Offset::ZERO,
        }
    }

    /// Creates BoxParentData with a specific offset.
    pub const fn with_offset(offset: Offset) -> Self {
        Self { offset }
    }

    /// Creates BoxParentData with x and y coordinates.
    pub fn with_xy(x: f32, y: f32) -> Self {
        Self {
            offset: Offset::new(x, y),
        }
    }

    /// Sets the offset using x and y coordinates.
    pub fn set_xy(&mut self, x: f32, y: f32) {
        self.offset = Offset::new(x, y);
    }

    /// Moves the offset by a delta.
    pub fn translate(&mut self, delta: Offset) {
        self.offset = self.offset + delta;
    }

    /// Resets the offset to the origin (0, 0).
    pub fn reset(&mut self) {
        self.offset = Offset::ZERO;
    }

    /// Checks if this child is at the origin (0, 0).
    pub fn is_at_origin(&self) -> bool {
        self.offset == Offset::ZERO
    }
}

impl ParentData for BoxParentData {
    fn as_parent_data_with_offset(&self) -> Option<&dyn ParentDataWithOffset> {
        Some(self)
    }

    fn as_parent_data_with_offset_mut(&mut self) -> Option<&mut dyn ParentDataWithOffset> {
        Some(self)
    }
}

impl ParentDataWithOffset for BoxParentData {
    #[inline]
    fn offset(&self) -> Offset {
        self.offset
    }

    #[inline]
    fn set_offset(&mut self, offset: Offset) {
        self.offset = offset;
    }
}

// ============================================================================
// CONTAINER PARENT DATA
// ============================================================================

/// Container parent data - sibling links for efficient traversal.
///
/// Provides doubly-linked list functionality for maintaining sibling relationships.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerParentData<ChildId> {
    /// Previous sibling in the parent's child list.
    pub previous_sibling: Option<ChildId>,
    /// Next sibling in the parent's child list.
    pub next_sibling: Option<ChildId>,
}

impl<ChildId> Default for ContainerParentData<ChildId> {
    fn default() -> Self {
        Self::new()
    }
}

impl<ChildId> ContainerParentData<ChildId> {
    /// Creates new container parent data with no siblings.
    pub const fn new() -> Self {
        Self {
            previous_sibling: None,
            next_sibling: None,
        }
    }

    /// Creates container parent data with specific siblings.
    pub fn with_siblings(previous: Option<ChildId>, next: Option<ChildId>) -> Self {
        Self {
            previous_sibling: previous,
            next_sibling: next,
        }
    }

    /// Sets the previous sibling.
    pub fn set_previous_sibling(&mut self, sibling: Option<ChildId>) {
        self.previous_sibling = sibling;
    }

    /// Sets the next sibling.
    pub fn set_next_sibling(&mut self, sibling: Option<ChildId>) {
        self.next_sibling = sibling;
    }

    /// Clears both sibling links.
    pub fn clear_siblings(&mut self) {
        self.previous_sibling = None;
        self.next_sibling = None;
    }

    /// Checks if this is the first child (no previous sibling).
    #[inline]
    pub fn is_first(&self) -> bool {
        self.previous_sibling.is_none()
    }

    /// Checks if this is the last child (no next sibling).
    #[inline]
    pub fn is_last(&self) -> bool {
        self.next_sibling.is_none()
    }

    /// Checks if this is the only child (no siblings).
    #[inline]
    pub fn is_only(&self) -> bool {
        self.is_first() && self.is_last()
    }

    /// Returns true if this child has any siblings.
    #[inline]
    pub fn has_siblings(&self) -> bool {
        !self.is_only()
    }
}

// ============================================================================
// CONTAINER BOX PARENT DATA
// ============================================================================

/// Container box parent data - combines offset and sibling links.
///
/// The most commonly used ParentData type, combining both:
/// - Positioning information (from `BoxParentData`)
/// - Sibling links (from `ContainerParentData`)
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerBoxParentData<ChildId> {
    box_data: BoxParentData,
    container_data: ContainerParentData<ChildId>,
}

impl<ChildId> Default for ContainerBoxParentData<ChildId> {
    fn default() -> Self {
        Self::new()
    }
}

impl<ChildId> ContainerBoxParentData<ChildId> {
    /// Creates a new ContainerBoxParentData at origin with no siblings.
    pub fn new() -> Self {
        Self {
            box_data: BoxParentData::new(),
            container_data: ContainerParentData::new(),
        }
    }

    /// Creates container box parent data with a specific offset.
    pub fn with_offset(offset: Offset) -> Self {
        Self {
            box_data: BoxParentData::with_offset(offset),
            container_data: ContainerParentData::new(),
        }
    }

    /// Creates container box parent data with offset and siblings.
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

    // === Offset Methods ===

    /// Gets the offset.
    #[inline]
    pub fn offset(&self) -> Offset {
        self.box_data.offset
    }

    /// Sets the offset.
    #[inline]
    pub fn set_offset(&mut self, offset: Offset) {
        self.box_data.set_offset(offset);
    }

    /// Sets the offset using x and y coordinates.
    pub fn set_xy(&mut self, x: f32, y: f32) {
        self.box_data.set_xy(x, y);
    }

    /// Moves the offset by a delta.
    pub fn translate(&mut self, delta: Offset) {
        self.box_data.translate(delta);
    }

    /// Resets the offset to the origin.
    pub fn reset_offset(&mut self) {
        self.box_data.reset();
    }

    /// Checks if this child is at the origin.
    #[inline]
    pub fn is_at_origin(&self) -> bool {
        self.box_data.is_at_origin()
    }

    // === Sibling Methods ===

    /// Gets the previous sibling.
    #[inline]
    pub fn previous_sibling(&self) -> Option<&ChildId> {
        self.container_data.previous_sibling.as_ref()
    }

    /// Gets the next sibling.
    #[inline]
    pub fn next_sibling(&self) -> Option<&ChildId> {
        self.container_data.next_sibling.as_ref()
    }

    /// Sets the previous sibling.
    pub fn set_previous_sibling(&mut self, sibling: Option<ChildId>) {
        self.container_data.set_previous_sibling(sibling);
    }

    /// Sets the next sibling.
    pub fn set_next_sibling(&mut self, sibling: Option<ChildId>) {
        self.container_data.set_next_sibling(sibling);
    }

    /// Clears both sibling links.
    pub fn clear_siblings(&mut self) {
        self.container_data.clear_siblings();
    }

    /// Checks if this is the first child.
    #[inline]
    pub fn is_first(&self) -> bool {
        self.container_data.is_first()
    }

    /// Checks if this is the last child.
    #[inline]
    pub fn is_last(&self) -> bool {
        self.container_data.is_last()
    }

    /// Checks if this is the only child.
    #[inline]
    pub fn is_only(&self) -> bool {
        self.container_data.is_only()
    }

    /// Checks if this child has any siblings.
    #[inline]
    pub fn has_siblings(&self) -> bool {
        self.container_data.has_siblings()
    }
}

impl<ChildId> ParentData for ContainerBoxParentData<ChildId>
where
    ChildId: fmt::Debug + Send + Sync + 'static,
{
    fn as_parent_data_with_offset(&self) -> Option<&dyn ParentDataWithOffset> {
        Some(self)
    }

    fn as_parent_data_with_offset_mut(&mut self) -> Option<&mut dyn ParentDataWithOffset> {
        Some(self)
    }
}

impl<ChildId> ParentDataWithOffset for ContainerBoxParentData<ChildId>
where
    ChildId: fmt::Debug + Send + Sync + 'static,
{
    #[inline]
    fn offset(&self) -> Offset {
        self.box_data.offset
    }

    #[inline]
    fn set_offset(&mut self, offset: Offset) {
        self.box_data.offset = offset;
    }
}

// ============================================================================
// TESTS
// ============================================================================

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
    fn test_box_parent_data_translate() {
        let mut data = BoxParentData::with_xy(10.0, 20.0);
        data.translate(Offset::new(5.0, 10.0));
        assert_eq!(data.offset(), Offset::new(15.0, 30.0));
    }

    #[test]
    fn test_box_parent_data_downcast() {
        let data = BoxParentData::new();
        let boxed: Box<dyn ParentData> = Box::new(data);

        assert!(boxed.as_any().is::<BoxParentData>());
        let downcast = boxed.as_any().downcast_ref::<BoxParentData>().unwrap();
        assert_eq!(downcast.offset(), Offset::ZERO);
    }

    #[test]
    fn test_container_parent_data_new() {
        let data: ContainerParentData<u64> = ContainerParentData::new();
        assert!(data.is_only());
        assert!(data.is_first());
        assert!(data.is_last());
        assert!(!data.has_siblings());
    }

    #[test]
    fn test_container_parent_data_with_siblings() {
        let data = ContainerParentData::with_siblings(Some(1u64), Some(2u64));
        assert_eq!(data.previous_sibling, Some(1));
        assert_eq!(data.next_sibling, Some(2));
        assert!(!data.is_first());
        assert!(!data.is_last());
        assert!(!data.is_only());
        assert!(data.has_siblings());
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
        assert!(!data.is_at_origin());
    }

    #[test]
    fn test_unit_parent_data() {
        let data = ();
        let boxed: Box<dyn ParentData> = Box::new(data);
        assert!(boxed.as_any().is::<()>());
    }

    #[test]
    fn test_size() {
        use std::mem::size_of;
        assert_eq!(size_of::<BoxParentData>(), 8);
    }
}
