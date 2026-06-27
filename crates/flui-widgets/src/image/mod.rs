//! Image widget and image-provider abstraction.
//!
//! The [`Image`] widget is the entry point for rendering bitmap images.
//! An [`ImageProvider`] describes how to obtain the pixel data — supply an
//! already-decoded handle via [`Image::from_image`], encode-from-bytes via
//! [`Image::memory`], read from disk via [`Image::file`], or reference a
//! network URL (stub) via [`Image::network`].

mod image;
mod provider;

pub use image::Image;
pub use provider::{
    DirectImageProvider, FileImage, ImageProvider, ImageProviderError, MemoryImage, NetworkImage,
};
