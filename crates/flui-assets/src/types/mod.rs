//! Asset values and shared ownership handles.
//!
//! - [`AssetKey`] owns a nonempty name through `Arc<str>`. Clones share
//!   storage; independently constructed equal names compare and hash by contents.
//! - [`AssetHandle`] shares loaded data and owns its key.
//! - [`AssetHandleCore`] and [`AssetHandleExt`] provide handle operations.
//! - [`WeakAssetHandle`] weakly observes data while still owning its key.
//! - [`LoadState`] tracks async loading.
//! - [`FontData`] contains loaded font data.
//!
//! # Examples
//!
//! ```rust
//! use flui_assets::{AssetKey, AssetHandle, AssetHandleExt};
//! use std::sync::Arc;
//!
//! // Independently owned equal names identify the same request
//! let key1 = AssetKey::new("texture.png");
//! let key2 = AssetKey::new("texture.png");
//! assert_eq!(key1, key2); // Equal contents
//!
//! // Handles provide cheap cloning
//! let data = vec![1, 2, 3, 4];
//! let handle = AssetHandle::new(Arc::new(data), key1);
//! let handle2 = handle.clone(); // Shares data and clones the key
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
