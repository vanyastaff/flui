//! The root render object and the viewport abstractions.
//!
//! [`RenderView`] is the root of the render tree: it fills the constraints the
//! pipeline owner holds for the root and lays its children out tight at that
//! size.
//!
//! Viewports are render objects that are "bigger on the inside" - they
//! display a portion of their content controlled by a scroll offset.

mod render_view;
mod scroll_position;
mod viewport;
mod viewport_offset;

pub use render_view::RenderView;
pub use scroll_position::{
    DimensionChangePolicy, OffsetWriteCause, ScrollOffsetWriter, ScrollPosition,
    ScrollPositionLayout, ScrollPositionSnapshot,
};
pub use viewport::{CacheExtentStyle, RenderAbstractViewport, RevealedOffset, SliverPaintOrder};
pub use viewport_offset::{
    FixedViewportLayout, FixedViewportOffset, ScrollDirection, ViewportLayout, ViewportOffset,
};
