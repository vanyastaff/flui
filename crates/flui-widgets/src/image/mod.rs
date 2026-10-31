//! Image widget and image-provider abstraction.
//!
//! The [`Image`] widget is the entry point for rendering bitmap images.
//! An [`ImageProvider`] describes how to obtain the pixel data — supply an
//! already-decoded handle via [`Image::from_image`], encode-from-bytes via
//! [`Image::memory`], read from disk via [`Image::file`], or resolve
//! asynchronously via `Image::asset`/`Image::network`.
//!
//! The `asset-images` feature exposes `AssetImage`, an async provider backed
//! by `flui-assets`, and `NetworkImage`, an async HTTP/HTTPS provider over
//! the host's HTTP capability. The feature is off by default so stable builds do
//! not pull in `flui-assets` unless asked for. (Not
//! doc-linked above: these items only exist when their feature is enabled,
//! and this module's own doc is built unconditionally.)

mod cache_key;
mod image;
mod provider;
mod texture;

#[cfg(feature = "asset-images")]
mod asset_image;
#[cfg(feature = "asset-images")]
mod network_image;
mod resolve;

pub use cache_key::ImageCacheKey;
pub use image::Image;
pub use image::ImageState;
pub use provider::{
    DecodeJob, DirectImageProvider, FileImage, ImageProvider, ImageProviderError, ImageServices,
    MemoryImage,
};
pub use texture::{Texture, TextureState};

#[cfg(feature = "asset-images")]
pub use asset_image::AssetImage;
#[cfg(feature = "asset-images")]
pub use network_image::NetworkImage;
