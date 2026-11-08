//! Decoded-image loading and retention for FLUI applications.
//!
//! An application owns an [`AssetRegistry`], usually behind an `Arc`, and
//! supplies it to image widgets or calls its loading methods directly. File
//! and network sources share completed loads and a byte-budgeted decoded-image
//! LRU. The registry starts no runtime or thread: callers provide execution
//! lanes and, for network images, an HTTP capability through [`LoadServices`].
//!
//! # Quick start
//!
//! ```
//! use flui_assets::AssetRegistryBuilder;
//!
//! let registry = AssetRegistryBuilder::new().build();
//! ```
//!
//! With the `images` feature, [`AssetRegistry::load_image`] returns an
//! [`ImageLoad`] future. [`AssetRegistry::cached_image`] is the synchronous
//! probe a frame uses before starting work. Decode allocation is bounded by
//! the image header and decoder limits; encoded files and HTTP bodies have
//! independent size bounds.
//!
//! # Feature flags
//!
//! - `images`: decoded file and network images, backed by `image` and `lru`.
//! - `full`: every stable feature; currently equivalent to `images`.

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "test code catches a panic to assert on it (ADR-0303 §5)"
    )
)]
#![deny(missing_docs)]

pub mod error;
pub mod registry;

pub use crate::error::{AssetError, Result};
pub use crate::registry::{AssetRegistry, AssetRegistryBuilder};
#[cfg(feature = "images")]
pub use crate::registry::{ImageLoad, ImageSource, LoadServices, NetworkSource};
pub use flui_painting::paint::Image;
