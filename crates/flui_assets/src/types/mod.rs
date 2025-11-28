//! Performance-optimized types for the asset system.
//!
//! This module provides highly-optimized types that minimize memory usage and
//! maximize performance:
//!
//! - [`AssetKey`] - Interned string keys (4 bytes vs 24+ for `String`)
//! - [`AssetHandle`] - Arc-based handles with weak references (8 bytes)
//! - [`AssetHandleCore`] - Core handle operations (sealed trait)
//! - [`AssetHandleExt`] - Extension trait with convenience methods
//! - [`WeakAssetHandle`] - Weak reference for cache-friendly patterns
//! - [`LoadState`] - State machine for tracking async loading
//! - [`FontData`] - Font-specific data container
//!
//! # Performance Characteristics
//!
//! - **AssetKey**: 4 bytes (string interning with lasso)
//! - **AssetHandle**: 8 bytes (single Arc pointer)
//! - **Hashing**: O(1) for interned keys
//! - **Comparison**: O(1) for interned keys
//!
//! # Examples
//!
//! ```rust
//! use flui_assets::{AssetKey, AssetHandle, AssetHandleExt};
//! use std::sync::Arc;
//!
//! // Keys are interned for efficiency
//! let key1 = AssetKey::new("texture.png");
//! let key2 = AssetKey::new("texture.png");
//! assert_eq!(key1, key2); // Fast comparison
//!
//! // Handles provide cheap cloning
//! let data = vec![1, 2, 3, 4];
//! let handle = AssetHandle::new(Arc::new(data), key1);
//! let handle2 = handle.clone(); // Just clones Arc
//!
//! // Extension traits provide convenience methods
//! assert!(!handle.is_unique()); // Two handles exist
//! assert_eq!(handle.total_ref_count(), 2);
//! ```

pub mod font_data;
pub mod handle;
pub mod key;
pub mod state;

pub use font_data::FontData;
pub use handle::{AssetHandle, AssetHandleCore, AssetHandleExt, WeakAssetHandle};
pub use key::AssetKey;
pub use state::LoadState;
