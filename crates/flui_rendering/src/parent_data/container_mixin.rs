//! ContainerParentDataMixin - Linked list support for container render objects.

use std::fmt::Debug;

// ============================================================================
// CONTAINER PARENT DATA MIXIN
// ============================================================================

/// Mixin providing linked list pointers for container children.
///
/// Used by container render objects to maintain doubly-linked list of children.
/// This enables efficient insertion, removal, and traversal operations.
///
/// # Type Parameter
///
/// - `ChildId` - Type used to identify children (typically `RenderId`)
///
/// # Usage
///
/// Include this in your parent data type to get sibling pointers:
///
/// ```ignore
/// #[derive(Debug, Clone, PartialEq)]
/// pub struct MyContainerParentData {
///     // Your fields
///     pub my_field: f32,
///     
///     // Container mixin
///     pub container: ContainerParentDataMixin<RenderId>,
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContainerParentDataMixin<ChildId> {
    /// Previous sibling in parent's child list.
    pub previous_sibling: Option<ChildId>,
    
    /// Next sibling in parent's child list.
    pub next_sibling: Option<ChildId>,
}

impl<ChildId> ContainerParentDataMixin<ChildId> {
    /// Create empty container mixin (no siblings).
    pub const fn new() -> Self {
        Self {
            previous_sibling: None,
            next_sibling: None,
        }
    }
    
    /// Check if this child has a previous sibling.
    #[inline]
    pub const fn has_previous_sibling(&self) -> bool {
        self.previous_sibling.is_some()
    }
    
    /// Check if this child has a next sibling.
    #[inline]
    pub const fn has_next_sibling(&self) -> bool {
        self.next_sibling.is_some()
    }
    
    /// Check if this is the first child (no previous sibling).
    #[inline]
    pub const fn is_first_child(&self) -> bool {
        self.previous_sibling.is_none()
    }
    
    /// Check if this is the last child (no next sibling).
    #[inline]
    pub const fn is_last_child(&self) -> bool {
        self.next_sibling.is_none()
    }
    
    /// Reset sibling pointers (detach from list).
    pub fn detach(&mut self) {
        self.previous_sibling = None;
        self.next_sibling = None;
    }
}

impl<ChildId> Default for ContainerParentDataMixin<ChildId> {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    type TestId = u32;

    #[test]
    fn test_new() {
        let mixin = ContainerParentDataMixin::<TestId>::new();
        
        assert!(mixin.previous_sibling.is_none());
        assert!(mixin.next_sibling.is_none());
        assert!(mixin.is_first_child());
        assert!(mixin.is_last_child());
    }

    #[test]
    fn test_with_siblings() {
        let mixin = ContainerParentDataMixin {
            previous_sibling: Some(1),
            next_sibling: Some(2),
        };
        
        assert!(mixin.has_previous_sibling());
        assert!(mixin.has_next_sibling());
        assert!(!mixin.is_first_child());
        assert!(!mixin.is_last_child());
    }

    #[test]
    fn test_detach() {
        let mut mixin = ContainerParentDataMixin {
            previous_sibling: Some(1),
            next_sibling: Some(2),
        };
        
        mixin.detach();
        
        assert!(mixin.previous_sibling.is_none());
        assert!(mixin.next_sibling.is_none());
    }

    #[test]
    fn test_first_child() {
        let mixin = ContainerParentDataMixin::<TestId> {
            previous_sibling: None,
            next_sibling: Some(2),
        };
        
        assert!(mixin.is_first_child());
        assert!(!mixin.is_last_child());
    }

    #[test]
    fn test_last_child() {
        let mixin = ContainerParentDataMixin::<TestId> {
            previous_sibling: Some(1),
            next_sibling: None,
        };
        
        assert!(!mixin.is_first_child());
        assert!(mixin.is_last_child());
    }
}
