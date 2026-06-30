//! Image widget and image-provider abstraction.
//!
//! The [`Image`] widget is the entry point for rendering bitmap images.
//! An [`ImageProvider`] describes how to obtain the pixel data — supply an
//! already-decoded handle via [`Image::from_image`], encode-from-bytes via
//! [`Image::memory`], or read from disk via [`Image::file`].
//!
//! The `network-images` feature exposes a placeholder HTTP/HTTPS provider while
//! async image loading is being wired. It is off by default so stable builds do
//! not expose an always-failing network constructor.

mod image;
mod provider;

pub use image::Image;
#[cfg(feature = "network-images")]
pub use provider::NetworkImage;
pub use provider::{
    DirectImageProvider, FileImage, ImageProvider, ImageProviderError, MemoryImage,
};
