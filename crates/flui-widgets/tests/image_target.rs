//! Standalone `image` test target kept for CI feature-gated runs.

#![allow(
    clippy::disallowed_methods,
    reason = "test code catches a panic to assert on it (ADR-0303 §5)"
)]
mod common;

#[path = "image.rs"]
mod image;

#[cfg(feature = "images")]
#[path = "image_background.rs"]
mod image_background;
