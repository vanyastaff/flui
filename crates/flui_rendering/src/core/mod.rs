//! Core infrastructure for the generic rendering architecture

pub mod render_flags;
pub mod render_state;
pub mod render_box_mixin;
pub mod leaf_render_box;
pub mod single_render_box;
pub mod container_render_box;

// Re-exports
pub use render_flags::RenderFlags;
pub use render_state::RenderState;
pub use render_box_mixin::RenderBoxMixin;
pub use leaf_render_box::LeafRenderBox;
pub use single_render_box::SingleRenderBox;
pub use container_render_box::ContainerRenderBox;

// Re-export from flui_core
pub use flui_core::DynRenderObject;
