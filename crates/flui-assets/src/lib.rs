//! High-performance asset management with smart caching, type safety, and async I/O.
//!
//! This crate provides a production-ready asset system for the FLUI framework with efficient
//! caching, type-safe APIs, and extensible architecture for custom asset types.
//!
//! # Features
//!
//! - 🚀 **High Performance** - Concurrent caching with TinyLFU admission and LRU eviction
//! - 🔒 **Thread-Safe** - Built on tokio, parking_lot, and moka for concurrent access
//! - 💾 **Smart Caching** - Explicit entry retention and expiration
//! - 🎯 **Type-Safe** - Generic `Asset<T>` trait for compile-time guarantees
//! - ⚡ **Async I/O** - Non-blocking loading with tokio runtime
//! - 🔑 **Efficient Keys** - 4-byte interned keys for fast hashing and comparison
//! - 📦 **Arc-Based Handles** - Cheap cloning with automatic cleanup via weak references
//! - 🎨 **Built-in Assets** - Images (optional), fonts, with extensible system
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use flui_assets::{AssetRegistryBuilder, FontAsset};
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a registry
//! let registry = AssetRegistryBuilder::new().with_default_capacity().build();
//!
//! // Load a font
//! let font = FontAsset::file("assets/font.ttf");
//! let handle = registry.load(font).await?;
//!
//! println!("Font loaded: {} bytes", handle.bytes.len());
//! # Ok(())
//! # }
//! ```
//!
//! # Architecture
//!
//! The system uses a three-layer architecture:
//!
//! ```text
//! AssetRegistry (per app)
//!     ↓
//! AssetCache<T> (Per Type) - Moka TinyLFU cache
//!     ↓
//! AssetHandle<T, K> (Arc) - Smart handles with weak references
//! ```
//!
//! ## Type State Builder
//!
//! The registry uses a type-state builder for compile-time validation:
//!
//! ```rust
//! use flui_assets::AssetRegistryBuilder;
//!
//! // ✅ This compiles
//! let registry = AssetRegistryBuilder::new()
//!     .with_capacity(flui_assets::CacheCapacity::Entries(std::num::NonZeroU64::new(1_024).expect("nonzero capacity")))
//!     .build();
//!
//! // ❌ This doesn't compile - cannot build without capacity
//! // let registry = AssetRegistryBuilder::new().build();
//! ```
//!
//! ## Extension Traits
//!
//! Convenience methods are provided via extension traits:
//!
//! ```rust,no_run
//! use flui_assets::{AssetHandle, AssetHandleExt, AssetCache, AssetCacheExt, FontAsset};
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let registry = flui_assets::AssetRegistryBuilder::new().with_default_capacity().build();
//! # let font = FontAsset::file("assets/font.ttf");
//! let handle = registry.load(font).await?;
//!
//! // Handle extensions
//! if handle.is_unique() {
//!     println!("Only reference!");
//! }
//! let size = handle.map(|font| font.bytes.len());
//! println!("Total refs: {}", handle.total_ref_count());
//!
//! // Cache extensions
//! let cache: AssetCache<FontAsset> = AssetCache::new(flui_assets::CacheCapacity::default());
//! println!("Hit rate: {:.1}%", cache.hit_rate() * 100.0);
//! # Ok(())
//! # }
//! ```
//!
//! # Custom Asset Types
//!
//! Implement the [`Asset`] trait for custom types:
//!
//! ```rust
//! use flui_assets::{Asset, AssetKey, AssetError, AssetMetadata};
//!
//! pub struct AudioAsset {
//!     path: String,
//! }
//!
//! #[derive(Debug, Clone)]
//! pub struct AudioData {
//!     pub samples: Vec<f32>,
//!     pub sample_rate: u32,
//! }
//!
//! impl Asset for AudioAsset {
//!     type Data = AudioData;
//!     type Key = AssetKey;
//!     type Error = AssetError;
//!
//!     fn key(&self) -> AssetKey {
//!         AssetKey::new(&self.path)
//!     }
//!
//!     async fn load(&self) -> Result<AudioData, AssetError> {
//!         let bytes = tokio::fs::read(&self.path).await?;
//!         // Decode audio...
//!         Ok(AudioData { samples: vec![], sample_rate: 44100 })
//!     }
//!
//!     fn metadata(&self) -> Option<AssetMetadata> {
//!         Some(AssetMetadata {
//!             format: Some("Audio".to_string()),
//!             ..Default::default()
//!         })
//!     }
//! }
//! ```
//!
//! # Performance
//!
//! ## Memory Efficiency
//!
//! - **AssetKey**: 4 bytes (vs 24+ for `String`)
//! - **AssetHandle**: stores a key and an `Arc` sharing the loaded data
//! - **Cache**: Moka manages entries and admission; statistics use a separate lock
//!
//! ## Thread Safety
//!
//! All public types implement `Send + Sync`:
//!
//! ```rust
//! # use flui_assets::*;
//! fn assert_send_sync<T: Send + Sync>() {}
//!
//! assert_send_sync::<AssetKey>();
//! assert_send_sync::<AssetHandle<FontData, AssetKey>>();
//! assert_send_sync::<AssetCache<FontAsset>>();
//! assert_send_sync::<AssetRegistry>();
//! ```
//!
//! # Feature Flags
//!
//! - `images` - Enable image loading (PNG, JPEG, GIF, WebP)
//! - `network` - Enable HTTP/HTTPS asset loading
//! - `full` - Enable all stable features

// Ship bar: crate is doc-complete — keep it that way.
#![deny(missing_docs)]

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
pub use crate::cache::{
    AssetCache, AssetCacheConfig, AssetCacheCore, AssetCacheExt, CacheCapacity, CacheExpiration,
    ExpirationTooLong,
};
pub use crate::core::{Asset, AssetMetadata};
pub use crate::error::{AssetError, Result};
pub use crate::registry::{AssetRegistry, AssetRegistryBuilder, HasCapacity, NoCapacity};
pub use crate::types::{
    AssetHandle, AssetHandleCore, AssetHandleExt, AssetKey, FontData, LoadState,
};

// Re-export loaders
pub use crate::loaders::BytesFileLoader;
#[cfg(feature = "network")]
pub use crate::loaders::NetworkLoader;

// Re-export concrete asset types
pub use crate::assets::font::FontAsset;
#[cfg(feature = "images")]
pub use crate::assets::image::ImageAsset;

// Re-export Image from flui_painting
pub use flui_painting::paint::Image;
