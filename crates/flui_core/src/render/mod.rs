//! Rendering layer with zero-cost abstractions
//!
//! Third tree in Flui's architecture: Widget → Element → RenderObject
//!
//! # Module Structure
//!
//! - `dyn_render_object` - Object-safe DynRenderObject trait
//! - `parent_data` - Parent data types for layout
//! - `widget` - RenderObjectWidget implementations

use crate::ParentData;

// ============================================================================
// Module Declarations
// ============================================================================

pub mod context;
pub mod dyn_render_object;
pub mod parent_data;
pub mod render_flags;
pub mod render_state;
pub mod widget;





// ============================================================================
// Public API Re-exports
// ============================================================================

pub use context::RenderContext;
pub use dyn_render_object::DynRenderObject;
pub use render_flags::RenderFlags;
pub use render_state::RenderState;

// ============================================================================
// Core Traits
// ============================================================================

/// RenderObject with associated types for zero-cost operations
///
/// # Two-Trait Pattern
///
/// - **DynRenderObject** - Object-safe for `Box<dyn DynRenderObject>` collections
/// - **RenderObject** - Zero-cost with associated types for concrete usage
///
/// # Associated Types
///
/// - `ParentData` - Concrete parent data type (use `()` if none)
/// - `Child` - Child type: `()` (leaf), concrete type (single), or `Box<dyn DynRenderObject>` (multi)
///
/// # Example
///
/// ```ignore
/// impl RenderObject for RenderBox {
///     type ParentData = BoxParentData;
///     type Child = Box<dyn DynRenderObject>;
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
pub trait RenderObject: DynRenderObject + Sized {
    /// Parent data type (use `()` for none)
    type ParentData: ParentData;

    /// Child type: `()`, concrete, or `Box<dyn DynRenderObject>`
    type Child: Sized;

    /// Get parent data (zero-cost, no downcast)
    fn parent_data(&self) -> Option<&Self::ParentData>;

    /// Get mutable parent data (zero-cost, no downcast)
    fn parent_data_mut(&mut self) -> Option<&mut Self::ParentData>;

    // Note: Dirty tracking, lifecycle, and boundaries methods
    // are defined in DynRenderObject trait and inherited automatically.
    // RenderObject types implement them through DynRenderObject.
}





