//! Core traits and types for the asset system.
//!
//! This module defines the fundamental abstractions that all assets must implement:
//! - `Asset`: The main trait for all asset types
//! - `AssetLoader`: Trait for loading assets from different sources
//! - `AssetMetadata`: Metadata about assets

pub mod asset;
pub mod loader;
pub mod metadata;

pub use asset::Asset;
pub use loader::AssetLoader;
pub use metadata::AssetMetadata;
