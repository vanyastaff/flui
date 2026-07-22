//! Image widget and image-provider abstraction.
//!
//! The [`Image`] widget is the entry point for rendering bitmap images.
//! An [`ImageProvider`] describes how to obtain the pixel data — supply an
//! already-decoded handle via [`Image::from_image`], encode-from-bytes via
//! [`Image::memory`], read from disk via [`Image::file`], or resolve
//! asynchronously via `Image::asset`/`Image::network`.
//!
//! The `asset-images` feature exposes `AssetImage`, an async provider backed
//! by `flui-assets`; `network-images` additionally exposes `NetworkImage`,
//! an async HTTP/HTTPS provider. Both are off by default so stable builds do
//! not pull in `flui-assets`/`futures-util`/`lru` unless asked for. (Not
//! doc-linked above: these items only exist when their feature is enabled,
//! and this module's own doc is built unconditionally.)

mod cache_key;
mod image;
mod provider;

#[cfg(feature = "asset-images")]
mod asset_image;
#[cfg(feature = "asset-images")]
mod decode_cache;
#[cfg(feature = "network-images")]
mod network_image;

pub use cache_key::ImageCacheKey;
pub use image::Image;
pub use provider::{
    DirectImageProvider, FileImage, ImageProvider, ImageProviderError, MemoryImage,
};

#[cfg(feature = "asset-images")]
pub use asset_image::AssetImage;
#[cfg(feature = "network-images")]
pub use network_image::NetworkImage;
