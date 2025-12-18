//! Base ParentData trait for child metadata storage.

use downcast_rs::{impl_downcast, DowncastSync};
use std::fmt::Debug;

// ============================================================================
// PARENT DATA TRAIT
// ============================================================================

/// Metadata stored on children by parent render objects.
///
/// Different layout protocols require different parent data types.
/// For example, box layout uses [`BoxParentData`] to store child offsets,
/// while sliver layout uses [`SliverParentData`] for logical positioning.
///
/// # Downcasting
///
/// Use `downcast_ref::<T>()` and `downcast_mut::<T>()` to access
/// concrete parent data types.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::parent_data::{ParentData, BoxParentData};
///
/// fn access_offset(data: &dyn ParentData) {
///     if let Some(box_data) = data.downcast_ref::<BoxParentData>() {
///         println!("Offset: {:?}", box_data.offset);
///     }
/// }
/// ```
///
/// # Implementation
///
/// ```ignore
/// #[derive(Debug, Default)]
/// struct CustomParentData {
///     custom_field: f32,
/// }
///
/// impl ParentData for CustomParentData {
///     fn detach(&mut self) {
///         // Optional: cleanup resources
///     }
/// }
/// ```
pub trait ParentData: Debug + DowncastSync {
    /// Called when render object is removed from tree.
    ///
    /// Override to clean up resources (listeners, subscriptions, etc).
    /// Default implementation does nothing.
    fn detach(&mut self) {}
}

// Enable downcasting for ParentData trait objects
impl_downcast!(sync ParentData);

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default)]
    struct TestParentData {
        value: i32,
    }

    impl ParentData for TestParentData {}

    #[test]
    fn test_downcast() {
        let data = TestParentData { value: 42 };
        let trait_obj: &dyn ParentData = &data;

        let downcasted = trait_obj.downcast_ref::<TestParentData>();
        assert!(downcasted.is_some());
        assert_eq!(downcasted.unwrap().value, 42);
    }

    #[test]
    fn test_detach_default() {
        let mut data = TestParentData { value: 10 };
        data.detach(); // Should not panic
    }
}
