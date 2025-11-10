//! # FLUI Assets
//!
//! High-performance asset management system for the FLUI framework.
//!
//! This crate provides a clean, extensible architecture for loading and caching
//! assets like images, fonts, audio, video, and more. It uses a trait-based design
//! that makes adding new asset types trivial.
//!
//! ## Features
//!
//! - **Generic Asset System**: Type-safe `Asset<T>` trait for extensibility
//! - **High-Performance Caching**: Moka-based cache with TinyLFU eviction
//! - **Interned Keys**: 4-byte asset keys for fast hashing and comparison
//! - **Arc-Based Handles**: Efficient shared ownership with `triomphe`
//! - **Async Loading**: Non-blocking I/O with tokio
//! - **Asset Bundles**: Manifest-based bundling for production (optional)
//! - **Hot Reload**: File watching for development (optional)
//! - **Multiple Loaders**: File, memory, network, bundle sources
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use flui_assets::{AssetRegistry, ImageAsset};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Load an image
//!     let image = AssetRegistry::global()
//!         .load(ImageAsset::file("logo.png"))
//!         .await?;
//!
//!     println!("Loaded image: {}x{}", image.width(), image.height());
//!     Ok(())
//! }
//! ```
//!
//! ## Architecture
//!
//! The asset system uses a layered architecture:
//!
//! 1. **Core Traits** (`core`): `Asset<T>` and `AssetLoader<T>` define the interfaces
//! 2. **Types** (`types`): `AssetKey`, `AssetHandle`, and state types
//! 3. **Cache** (`cache`): Multi-level caching with statistics
//! 4. **Loaders** (`loaders`): Concrete implementations for different sources
//! 5. **Assets** (`assets`): Concrete asset types (Image, Font, etc.)
//! 6. **Registry** (`registry`): Central orchestration and management
//!
//! ## Adding New Asset Types
//!
//! To add a new asset type, simply implement the `Asset` trait:
//!
//! ```rust,ignore
//! use flui_assets::core::Asset;
//!
//! pub struct VideoAsset {
//!     path: String,
//! }
//!
//! impl Asset for VideoAsset {
//!     type Data = VideoData;
//!     type Key = AssetKey;
//!     type Error = AssetError;
//!
//!     fn key(&self) -> AssetKey {
//!         AssetKey::new(&self.path)
//!     }
//!
//!     async fn load(&self) -> Result<VideoData, AssetError> {
//!         // Load and decode video
//!         todo!()
//!     }
//! }
//! ```
//!
//! The cache, registry, and loaders automatically work with your new asset type!

#![warn(missing_docs)]
#![warn(clippy::all)]

// Core traits and interfaces
pub mod core;

// Optimized types
pub mod types;

// Error handling
pub mod error;

// Caching system
pub mod cache;

// Asset loaders
pub mod loaders;

// Concrete asset types
pub mod assets;

// Asset registry and orchestration
pub mod registry;

// Optional: Asset bundles
// TODO: Implement bundle module
// #[cfg(feature = "bundles")]
// pub mod bundle;

// Optional: Hot reload
// TODO: Implement hot_reload module
// #[cfg(feature = "hot-reload")]
// pub mod hot_reload;

// Re-exports for convenience
pub use crate::cache::AssetCache;
pub use crate::core::{Asset, AssetLoader, AssetMetadata};
pub use crate::error::{AssetError, Result};
pub use crate::registry::{AssetRegistry, AssetRegistryBuilder};
pub use crate::types::{AssetHandle, AssetKey, FontData, LoadState};

// Re-export loaders
pub use crate::loaders::{BytesFileLoader, FileLoader, MemoryLoader, NetworkLoader};

// Re-export concrete asset types
pub use crate::assets::font::FontAsset;
pub use crate::assets::image::ImageAsset;

// Re-export Image from flui_types
pub use flui_types::painting::Image;
