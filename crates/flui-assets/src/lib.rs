//! The decoded images a FLUI application shows, and the loads that fill them.
//!
//! An [`ImageCache`] keeps completed images within a byte budget, shares one
//! load among every request for the same [`ImageSource`], reads each source on
//! the IO lane of the caller's lanes and decodes it on their compute lane
//! (ADR-0279), and fetches network images with the host's HTTP client
//! (ADR-0280). It starts no runtime or thread of its own: [`LoadServices`]
//! names the lanes and the client a load runs on.
//!
//! `flui-widgets` resolves `Image::asset` and `Image::network` through it.
//!
//! ```
//! use flui_assets::{ImageCache, ImageCacheConfig, ImageSource, LoadServices};
//!
//! let cache = ImageCache::new(ImageCacheConfig::default().budget(64 * 1024 * 1024));
//! // Nothing is kept before a load completes.
//! assert!(cache.cached_image(&ImageSource::Asset("logo.png".into())).is_none());
//! // With no lanes, the load runs where the returned future is polled.
//! let load = cache.load_image("logo.png", LoadServices::new());
//! drop(load); // the last waiter left: the load is cancelled
//! ```

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "test code catches a panic to assert on it (ADR-0303 §5)"
    )
)]
#![deny(missing_docs)]

mod error;
mod image_cache;

pub use crate::error::{AssetError, Result};
pub use crate::image_cache::{
    ImageCache, ImageCacheConfig, ImageLoad, ImageSource, LoadServices, NetworkSource,
};
