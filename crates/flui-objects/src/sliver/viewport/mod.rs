//! The Box-protocol render objects that host sliver children and scroll them
//! by a [`ViewportOffset`](flui_rendering::view::ViewportOffset).
//!
//! [`RenderViewport`] fills its incoming main axis and lays its slivers out
//! around a `center` child; [`RenderShrinkWrappingViewport`] sizes its main
//! axis to its slivers. Both are a `ViewportCore` (the offset subscription,
//! the child walk, paint, hit testing and clips) plus their own sizing rule.

mod core;
mod shrink_wrapping;
#[expect(
    clippy::module_inception,
    reason = "the module is the viewport family; `viewport` is its RenderViewport beside `shrink_wrapping`"
)]
mod viewport;

pub use shrink_wrapping::RenderShrinkWrappingViewport;
pub use viewport::RenderViewport;
