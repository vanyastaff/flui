//! Built-in asset types for common use cases.
//!
//! This module provides ready-to-use asset types that implement the [`Asset`](crate::Asset) trait:
//!
//! - [`FontAsset`] - TrueType/OpenType font files (always available)
//! - [`ImageAsset`] - Image files (requires `images` feature)
//!
//! # Examples
//!
//! ## Loading Fonts
//!
//! ```rust,no_run
//! use flui_assets::{AssetRegistry, FontAsset};
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let registry = AssetRegistry::global();
//! let font = FontAsset::file("assets/Roboto-Regular.ttf");
//! let handle = registry.load(font).await?;
//! println!("Font: {} bytes", handle.bytes.len());
//! # Ok(())
//! # }
//! ```
//!
//! ## Loading Images
//!
//! Requires `images` feature flag:
//!
//! ```toml
//! [dependencies]
//! flui_assets = { version = "0.1", features = ["images"] }
//! ```
//!
//! ```rust,ignore
//! use flui_assets::{AssetRegistry, ImageAsset};
//!
//! let registry = AssetRegistry::global();
//! let image = ImageAsset::file("assets/logo.png");
//! let handle = registry.load(image).await?;
//! println!("Image: {}x{}", handle.width(), handle.height());
//! ```

pub mod font;
pub mod image;

pub use font::FontAsset;
pub use image::ImageAsset;
