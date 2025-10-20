//! Rendering layer with zero-cost abstractions
//!
//! Third tree in Flui's architecture: Widget → Element → RenderObject

use crate::ParentData;

pub mod any_render_object;
pub mod parent_data;
pub mod widget;

pub use any_render_object::AnyRenderObject;

/// RenderObject with associated types for zero-cost operations
///
/// # Two-Trait Pattern
///
/// - **AnyRenderObject** - Object-safe for `Box<dyn AnyRenderObject>` collections
/// - **RenderObject** - Zero-cost with associated types for concrete usage
///
/// # Associated Types
///
/// - `ParentData` - Concrete parent data type (use `()` if none)
/// - `Child` - Child type: `()` (leaf), concrete type (single), or `Box<dyn AnyRenderObject>` (multi)
///
/// # Example
///
/// ```ignore
/// impl RenderObject for RenderBox {
///     type ParentData = BoxParentData;
///     type Child = Box<dyn AnyRenderObject>;
///
///     fn parent_data(&self) -> Option<&Self::ParentData> {
///         self.parent_data.as_ref()
///     }
///
///     fn parent_data_mut(&mut self) -> Option<&mut Self::ParentData> {
///         self.parent_data.as_mut()
///     }
/// }
/// ```
pub trait RenderObject: AnyRenderObject + Sized {
    /// Parent data type (use `()` for none)
    type ParentData: ParentData;

    /// Child type: `()`, concrete, or `Box<dyn AnyRenderObject>`
    type Child: Sized;

    /// Get parent data (zero-cost, no downcast)
    fn parent_data(&self) -> Option<&Self::ParentData>;

    /// Get mutable parent data (zero-cost, no downcast)
    fn parent_data_mut(&mut self) -> Option<&mut Self::ParentData>;
}



