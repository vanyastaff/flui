//! Optimized types for the asset system.
//!
//! This module contains performance-optimized types:
//! - `AssetKey`: Interned strings for efficient keys (4 bytes)
//! - `AssetHandle`: Arc-based handles for shared ownership
//! - `LoadState`: State machine for async loading

pub mod font_data;
pub mod handle;
pub mod key;
pub mod state;

pub use font_data::FontData;
pub use handle::{AssetHandle, WeakAssetHandle};
pub use key::AssetKey;
pub use state::LoadState;
