//! Asset loaders for different sources.
//!
//! This module provides concrete implementations of asset loaders for various data sources.
//! Load byte sources directly, then decode them in an [`Asset`](crate::Asset) implementation.
//!
//! # Available Loaders
//!
//! - [`BytesFileLoader`] - Optimized loader for raw bytes from files
//! - `NetworkLoader` - HTTP/HTTPS loading (requires `network` feature)
//!
//! # Examples
//!
//! ## File System Loading
//!
//! ```rust,no_run
//! use flui_assets::BytesFileLoader;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let loader = BytesFileLoader::new("assets");
//! let bytes = loader.load_bytes("logo.png").await?;
//! let text = loader.load_string("config.json").await?;
//! # Ok(())
//! # }
//! ```
//!
pub mod file;
#[cfg(feature = "network")]
pub mod network;

pub use file::BytesFileLoader;
#[cfg(feature = "network")]
pub use network::NetworkLoader;
