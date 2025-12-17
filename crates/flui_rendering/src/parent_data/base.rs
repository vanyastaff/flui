//! Base ParentData trait.

use downcast_rs::{impl_downcast, DowncastSync};
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
/// - Downcasting via `downcast_rs` (use `downcast_ref::<T>()`, `downcast_mut::<T>()`)
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
/// impl ParentData for MyParentData {}
/// ```
pub trait ParentData: Debug + DowncastSync {
    /// Called when the render object is removed from the tree.
    ///
    /// Subclasses should override this to clean up any resources.
    /// The default implementation does nothing.
    fn detach(&mut self) {}
}

impl_downcast!(sync ParentData);
