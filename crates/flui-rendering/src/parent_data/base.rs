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

    /// Returns a mutable reference to `self` as a [`LogicalIndexParentData`] if
    /// this type carries a logical child index (i.e. if it implements
    /// [`LogicalIndexParentData`]).
    ///
    /// The default returns `None`.  Types that carry an `index` field override
    /// this to return `Some(self)`.  The pipeline uses this to stamp the
    /// logical index into freshly-inserted children without Any-reflection.
    ///
    /// `LogicalIndexParentData` is `pub(crate)`: external callers cannot name
    /// the return type, so this method is intentionally not part of the public
    /// contract.  The `private_interfaces` lint is suppressed here because the
    /// trait-object pattern is the only way to provide type-erasure without an
    /// intermediate `Any` downcast, and the seal is enforced by crate visibility
    /// on `LogicalIndexParentData` itself.
    #[allow(private_interfaces)]
    fn as_logical_index_mut(&mut self) -> Option<&mut dyn LogicalIndexParentData> {
        None
    }
}

/// Opt-in extension for parent-data types that carry a **logical child
/// index** — used by the deferred-insert path to stamp the correct index
/// onto a freshly-inserted child without going through `Any` reflection.
///
/// Implemented by [`super::SliverMultiBoxAdaptorParentData`],
/// [`super::SliverGridParentData`], and [`super::TreeSliverNodeParentData`].
/// All other [`ParentData`] types use the blanket default (returns `None`).
///
/// `pub(crate)`: only `apply_deferred_mutation` (pipeline-internal) calls
/// `as_logical_index_mut` / `set_logical_index`; there is no external contract
/// to expose.
pub(crate) trait LogicalIndexParentData: ParentData {
    /// Sets the logical child index to `index`.
    fn set_logical_index(&mut self, index: usize);
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
