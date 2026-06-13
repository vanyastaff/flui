//! Base ParentData trait for child metadata storage.

use std::fmt::Debug;

use downcast_rs::{DowncastSync, impl_downcast};

// ============================================================================
// PARENT DATA TRAIT
// ============================================================================

/// Metadata stored on children by parent render objects.
///
/// Different layout protocols require different parent data types.
/// For example, box layout uses [`BoxParentData`](super::BoxParentData) to store child offsets,
/// while sliver layout uses [`SliverParentData`](super::SliverParentData) for logical positioning.
///
/// # Downcasting
///
/// Use `downcast_ref::<T>()` and `downcast_mut::<T>()` to access
/// concrete parent data types.
///
/// # Cloning
///
/// `ParentData` requires `DynClone` from the `dyn_clone` crate. All types
/// that implement `Clone` automatically satisfy this bound. This enables
/// `dyn_clone::clone_box(&*data)` to clone `&dyn ParentData` into
/// `Box<dyn ParentData>`, used to seed `ErasedChildState` from the
/// child's persistent `RenderState.parent_data`.
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
/// #[derive(Debug, Clone, Default)]
/// struct CustomParentData {
///     custom_field: f32,
/// }
///
/// impl ParentData for CustomParentData {}
/// ```
pub trait ParentData: Debug + DowncastSync + dyn_clone::DynClone {
    /// Called when render object is removed from tree.
    ///
    /// Override to clean up resources (listeners, subscriptions, etc).
    /// Default implementation does nothing.
    fn detach(&mut self) {}
}

// Enable downcasting for ParentData trait objects
impl_downcast!(sync ParentData);

// Enable `Box<dyn ParentData>::clone()` — all concrete types that
// derive `Clone` automatically satisfy the `DynClone` bound.
dyn_clone::clone_trait_object!(ParentData);

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Default)]
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

    #[test]
    fn test_clone_box_dyn() {
        let data = TestParentData { value: 42 };
        let boxed: &dyn ParentData = &data;
        let cloned: Box<dyn ParentData> = dyn_clone::clone_box(boxed);
        let downcasted = cloned.downcast_ref::<TestParentData>();
        assert!(downcasted.is_some());
        assert_eq!(downcasted.unwrap().value, 42);
    }
}
