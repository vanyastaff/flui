//! Core infrastructure for the generic rendering architecture

pub mod container_render_box;
pub mod leaf_render_box;
pub mod render_box;
pub mod render_box_mixin;
pub mod render_flags;
pub mod render_state;
pub mod single_render_box;


// Re-exports
pub use render_flags::RenderFlags;
pub use render_state::RenderState;
pub use render_box_mixin::RenderBoxMixin;
pub use render_box::RenderBox; // NEW: Unified RenderBox
pub use leaf_render_box::LeafRenderBox; // OLD: Will be removed after migration
pub use single_render_box::SingleRenderBox; // OLD: Will be removed after migration
pub use container_render_box::ContainerRenderBox; // OLD: Will be removed after migration

// Re-export from flui_core
pub use flui_core::DynRenderObject;

