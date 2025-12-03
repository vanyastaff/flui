//! View module for flui_rendering
//!
//! Contains RenderView trait for integrating render objects with the view/element system.
//!
//! # Architecture
//!
//! ```text
//! flui-view (ViewObject, component views)
//!      ↓
//! flui_rendering/view (RenderView, UpdateResult)
//!      ↓
//! flui_rendering/core (RenderObject, RenderState, etc.)
//! ```
//!
//! # Key Types
//!
//! - [`RenderView`] - Trait for views that create render objects
//! - [`UpdateResult`] - Result of updating a render object

mod render_view;
mod update_result;

pub use render_view::{
    RenderObjectFor, RenderView, RenderViewConfig, RenderViewExt, RenderViewLeaf,
    RenderViewWithChild, RenderViewWithChildren, RenderViewWithOptionalChild,
};
pub use update_result::UpdateResult;
