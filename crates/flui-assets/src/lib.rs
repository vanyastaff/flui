//! The decoded images a FLUI application shows, and the loads that fill them.
//!
//! An [`ImageCache`] keeps completed images within a byte budget, shares one
//! load among every request for the same [`ImageSource`], reads an
//! application's asset through the host's asset bundle (ADR-0384) on
//! the IO lane of the caller's lanes and decodes it on their compute lane
//! (ADR-0279), and fetches network images with the host's HTTP client
//! (ADR-0280). It starts no runtime or thread of its own: [`LoadServices`]
//! names the lanes a load runs on, and a network source the client that
//! fetches it.
//!
//! `flui-widgets` resolves `Image::asset` and `Image::network` through it: each
//! UI runtime keeps one, which a widget acquires as [`ImageCacheCapability`]
//! and the runtime trims under memory pressure (ADR-0387).
//!
//! ```
//! use std::sync::Arc;
//!
//! use flui_assets::{ImageCache, ImageCacheConfig, ImageSource, LoadServices};
//! use flui_platform::assets::{AssetBundle, AssetKey, DirectoryAssets};
//!
//! let cache = ImageCache::new(ImageCacheConfig::default().budget(64 * 1024 * 1024));
//! let bundle = AssetBundle::new(Arc::new(DirectoryAssets::new("flui_assets")));
//! let key = AssetKey::try_new("assets/logo.png").expect("a key inside the bundle");
//! // Nothing is kept before a load completes.
//! let logo = ImageSource::asset(key, &bundle);
//! assert!(cache.get(&logo).is_none());
//! // With no lanes, the load runs where the returned future is polled.
//! let load = cache.load(&logo, &LoadServices::new());
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

mod capability;
mod error;
mod image_cache;

pub use crate::capability::ImageCacheCapability;
pub use crate::error::{ImageLoadError, ImageLoadErrorKind};
pub use crate::image_cache::{ImageCache, ImageCacheConfig, ImageLoad, ImageSource, LoadServices};
