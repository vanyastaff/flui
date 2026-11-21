//! Base ParentData trait for child metadata storage.

use std::fmt::Debug;

use std::any::Any;

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
/// `ParentData` requires `DynClone` from the `dyn_clone` crate, because std has no way to clone a `&dyn Trait` into a `Box`. All types
/// that implement `Clone` automatically satisfy this bound. This enables
/// `dyn_clone::clone_box(&*data)` to clone `&dyn ParentData` into
/// `Box<dyn ParentData>`, which the intrinsic and dry query walks use to read
/// a child's parent data. The layout walk lends it instead of copying it.
///
/// # Example
///
/// ```
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
/// ```
/// use flui_rendering::parent_data::ParentData;
///
/// #[derive(Debug, Clone, Default)]
/// struct CustomParentData {
///     custom_field: f64,
/// }
///
/// impl ParentData for CustomParentData {}
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be attached to a child as parent data",
    label = "missing `impl ParentData for {Self}`",
    note = "`impl ParentData for {Self} {{}}` is enough (every method has a default) provided `{Self}` is `Debug + Clone + 'static`"
)]
pub trait ParentData: Debug + Any + dyn_clone::DynClone {
    /// Called when render object is removed from tree.
    ///
    /// Override to clean up resources (listeners, subscriptions, etc).
    /// Default implementation does nothing.
    fn detach(&mut self) {}
}

impl dyn ParentData {
    /// Returns `true` if the concrete type behind this trait object is `T`.
    #[must_use]
    pub fn is<T: ParentData>(&self) -> bool {
        (self as &dyn Any).is::<T>()
    }

    /// Returns the concrete `T` behind this trait object, or `None` for another type.
    #[must_use]
    pub fn downcast_ref<T: ParentData>(&self) -> Option<&T> {
        (self as &dyn Any).downcast_ref::<T>()
    }

    /// Mutable counterpart to `downcast_ref`.
    #[must_use]
    pub fn downcast_mut<T: ParentData>(&mut self) -> Option<&mut T> {
        (self as &mut dyn Any).downcast_mut::<T>()
    }
}

// Enable `Box<dyn ParentData>::clone()` — all concrete types that
// derive `Clone` automatically satisfy the `DynClone` bound.
dyn_clone::clone_trait_object!(ParentData);
