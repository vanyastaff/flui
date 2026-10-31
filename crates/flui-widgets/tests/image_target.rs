//! Standalone `image` test target kept for CI feature-gated runs.

mod common;

#[path = "image.rs"]
mod image;

#[cfg(feature = "images")]
#[path = "image_background.rs"]
mod image_background;
