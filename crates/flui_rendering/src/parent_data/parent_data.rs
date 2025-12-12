//! Parent data trait for render object metadata

use std::fmt::Debug;
use downcast_rs::{impl_downcast, Downcast};
use dyn_clone::DynClone;

/// Base trait for parent data attached to render object children
///
/// Parent data is metadata that a parent render object attaches to each of its children.
/// This metadata typically contains layout information specific to how the parent
/// positions and sizes its children.
///
/// # Examples
///
/// - **BoxParentData**: Contains offset position for box children
/// - **FlexParentData**: Contains flex factor and fit for flex children
/// - **StackParentData**: Contains offset, alignment, and fit for stack children
///
/// # Type Safety
///
/// Parent data is stored as `Box<dyn ParentData>` and can be downcasted to concrete
/// types using `downcast_ref` and `downcast_mut` methods from the `Downcast` trait.
///
/// ```ignore
/// let parent_data: &dyn ParentData = /* ... */;
/// if let Some(box_data) = parent_data.downcast_ref::<BoxParentData>() {
///     println!("Offset: {:?}", box_data.offset);
/// }
/// ```
pub trait ParentData: Debug + Send + Sync + Downcast + DynClone + 'static {
    /// Detaches this parent data from its render object
    ///
    /// Called when the child is being removed from the parent.
    /// Default implementation does nothing.
    fn detach(&mut self) {}
}

// Implement downcast methods for ParentData
impl_downcast!(ParentData);

// Implement clone for trait objects
dyn_clone::clone_trait_object!(ParentData);

/// Helper macro for implementing ParentData trait
///
/// This macro provides a simple implementation marker.
/// The actual Clone implementation is provided by `dyn_clone::clone_trait_object!`.
///
/// # Example
///
/// ```ignore
/// #[derive(Debug, Clone)]
/// pub struct MyParentData {
///     pub offset: Offset,
/// }
///
/// impl_parent_data!(MyParentData);
/// ```
#[macro_export]
macro_rules! impl_parent_data {
    ($type:ty) => {
        impl $crate::parent_data::ParentData for $type {}
    };
}
