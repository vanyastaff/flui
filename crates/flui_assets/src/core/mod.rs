//! Core traits and abstractions for the asset system.
//!
//! This module provides the foundational types and traits that enable type-safe,
//! extensible asset management:
//!
//! - [`Asset`] - Main trait that all asset types must implement
//! - [`AssetLoader`] - Trait for loading assets from different sources
//! - [`AssetMetadata`] - Optional metadata about assets (format, size, etc.)
//!
//! # Examples
//!
//! Implementing a custom asset type:
//!
//! ```rust
//! use flui_assets::{Asset, AssetKey, AssetError, AssetMetadata};
//!
//! pub struct ConfigAsset {
//!     path: String,
//! }
//!
//! #[derive(Debug, Clone)]
//! pub struct ConfigData {
//!     pub content: String,
//! }
//!
//! impl Asset for ConfigAsset {
//!     type Data = ConfigData;
//!     type Key = AssetKey;
//!     type Error = AssetError;
//!
//!     fn key(&self) -> AssetKey {
//!         AssetKey::new(&self.path)
//!     }
//!
//!     async fn load(&self) -> Result<ConfigData, AssetError> {
//!         let content = tokio::fs::read_to_string(&self.path).await?;
//!         Ok(ConfigData { content })
//!     }
//!
//!     fn metadata(&self) -> Option<AssetMetadata> {
//!         Some(AssetMetadata {
//!             format: Some("JSON".to_string()),
//!             ..Default::default()
//!         })
//!     }
//! }
//! ```

pub mod asset;
pub mod loader;
pub mod metadata;

pub use asset::Asset;
pub use loader::AssetLoader;
pub use metadata::AssetMetadata;
