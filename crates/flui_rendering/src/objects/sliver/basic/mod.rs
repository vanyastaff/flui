//! Basic sliver render objects.
//!
//! Simple sliver modifications and adapters for embedding box content.
//!
//! # Objects
//!
//! - [`RenderSliverPadding`]: Adds padding around a sliver
//! - [`RenderSliverToBoxAdapter`]: Wraps a box widget in a sliver
//! - [`RenderSliverFillRemaining`]: Fills remaining viewport space
//! - [`RenderSliverFillViewport`]: Children fill entire viewport
//! - [`RenderSliverOffstage`]: Conditionally hides a sliver

mod fill_remaining;
mod fill_viewport;
mod offstage;
mod padding;
mod to_box_adapter;

pub use fill_remaining::RenderSliverFillRemaining;
pub use fill_viewport::RenderSliverFillViewport;
pub use offstage::RenderSliverOffstage;
pub use padding::RenderSliverPadding;
pub use to_box_adapter::RenderSliverToBoxAdapter;
