//! [`Carousel`]: a horizontal list on the keyline model, with a [`CarouselController`],
//! [`KeylineSnap`] settling and opt-in [`CarouselAutoplay`].

mod autoplay;
mod carousel;
mod controller;
mod snap;

pub use autoplay::{CarouselAutoplay, CarouselAutoplayError, MIN_AUTOPLAY_PERIOD};
pub use carousel::{Carousel, CarouselItemScope, CarouselState};
pub use controller::CarouselController;
pub use flui_objects::{CarouselLayout, CarouselLayoutError, MAX_ITEM_EXTENT};
pub use snap::KeylineSnap;
