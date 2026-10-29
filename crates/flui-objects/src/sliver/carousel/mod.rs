//! The carousel sliver: lazily built items laid out at one large extent and masked down to the
//! keylines they pass as they scroll.

mod geometry;
mod keylines;
mod layout;
mod render;

pub use geometry::{CarouselGeometry, CarouselItemPlacement};
pub use layout::{CarouselLayout, CarouselLayoutError, MAX_ITEM_EXTENT};
pub use render::{CarouselConfig, CarouselItemFrames, RenderSliverCarousel};
