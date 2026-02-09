//! Asset loaders for different sources.
//!
//! This module provides concrete implementations of asset loaders for various data sources.
//! All loaders implement the [`AssetLoader`](crate::AssetLoader) trait for type-safe, async loading.
//!
//! # Available Loaders
//!
//! - [`FileLoader`] - Generic file system loader with path resolution
//! - [`BytesFileLoader`] - Optimized loader for raw bytes from files
//! - [`MemoryLoader`] - In-memory storage for testing and embedded assets
//! - [`NetworkLoader`] - HTTP/HTTPS loading (requires `network` feature)
//!
//! # Examples
//!
//! ## File System Loading
//!
//! ```rust,no_run
//! use flui_assets::BytesFileLoader;
//! use flui_assets::core::AssetLoader;
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
//! ## Memory Loading (Testing)
//!
//! ```rust
//! use flui_assets::MemoryLoader;
//!
//! let loader = MemoryLoader::new();
//! loader.insert("test", vec![1, 2, 3, 4]);
//!
//! assert!(loader.contains(&"test"));
//! assert_eq!(loader.len(), 1);
//! ```

pub mod file;
pub mod memory;
pub mod network;

pub use file::{BytesFileLoader, FileLoader};
pub use memory::MemoryLoader;
pub use network::NetworkLoader;
