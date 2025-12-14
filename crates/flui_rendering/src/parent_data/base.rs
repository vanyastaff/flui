//! Base ParentData trait.

use std::any::Any;
use std::fmt::Debug;

// ============================================================================
// ParentData Trait
// ============================================================================

/// Base trait for parent data types.
///
/// Parent data is metadata stored on children by their parent render object.
/// Different layout algorithms require different parent data types.
///
/// # Flutter Equivalence
///
/// ```dart
/// class ParentData {
///   void detach() {}
/// }
/// ```
///
/// # Implementation
///
/// All parent data types must implement this trait. The trait provides:
/// - `detach()` for cleanup when removed from tree
/// - `as_any()` / `as_any_mut()` for downcasting to concrete types
///
/// # Example
///
/// ```ignore
/// use flui_rendering::parent_data::ParentData;
///
/// #[derive(Debug, Default)]
/// struct MyParentData {
///     custom_field: i32,
/// }
///
/// impl ParentData for MyParentData {
///     fn as_any(&self) -> &dyn Any { self }
///     fn as_any_mut(&mut self) -> &mut dyn Any { self }
/// }
/// ```
pub trait ParentData: Any + Debug + Send + Sync + 'static {
    /// Called when the render object is removed from the tree.
    ///
    /// Subclasses should override this to clean up any resources.
    /// The default implementation does nothing.
    fn detach(&mut self) {}

    /// Returns self as `Any` for downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Returns self as mutable `Any` for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// Helper macro to implement ParentData trait for a type.
#[macro_export]
macro_rules! impl_parent_data {
    ($ty:ty) => {
        impl $crate::parent_data::ParentData for $ty {
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }

            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }
        }
    };
}
