//! View module for flui_rendering
//!
//! Contains RenderView trait and wrappers for integrating render objects
//! with the view/element system.
//!
//! # Architecture
//!
//! ```text
//! flui-view (ViewObject, component views)
//!      ↓
//! flui_rendering/view (RenderView, RenderViewObject, wrappers)
//!      ↓
//! flui_rendering/core (RenderObject, RenderState, etc.)
//! ```
//!
//! # Key Types
//!
//! - [`RenderView`] - Trait for views that create render objects
//! - [`RenderViewObject`] - Extension trait for render-specific ViewObject methods
//! - [`RenderViewWrapper`] - Wrapper that implements ViewObject for RenderView
//! - [`RenderObjectWrapper`] - Wrapper for raw RenderObject instances
//! - [`UpdateResult`] - Result of updating a render object

mod render_object_wrapper;
mod render_view;
mod render_view_object;
mod render_view_wrapper;
mod update_result;

pub use render_object_wrapper::RenderObjectWrapper;
pub use render_view::{
    RenderObjectFor, RenderView, RenderViewConfig, RenderViewExt, RenderViewLeaf,
    RenderViewWithChild, RenderViewWithChildren, RenderViewWithOptionalChild,
};
pub use render_view_object::{is_render_protocol, RenderViewObject};
pub use render_view_wrapper::{ArityToRuntime, RenderViewWrapper};
pub use update_result::UpdateResult;
