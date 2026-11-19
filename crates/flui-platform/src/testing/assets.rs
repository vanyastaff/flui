//! [`MemoryAssets`]: an [`AssetSource`] over a map of keys to bytes.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::assets::{AssetFuture, AssetKey, AssetLoadError, AssetSource};

/// A test's assets, held in memory (ADR-0384 §5): a key the test did not
/// give is [`AssetLoadError::NotFound`], as an asset missing from a package
/// is, and the limit holds as a platform reader holds it.
#[derive(Clone, Debug, Default)]
pub struct MemoryAssets {
    assets: BTreeMap<AssetKey, Arc<[u8]>>,
}

impl MemoryAssets {
    /// No assets.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// These assets and `bytes` under `key`, replacing what `key` held.
    #[must_use = "the builder returns the updated assets"]
    pub fn asset(mut self, key: AssetKey, bytes: impl Into<Arc<[u8]>>) -> Self {
        self.assets.insert(key, bytes.into());
        self
    }
}

impl AssetSource for MemoryAssets {
    fn load(&self, key: &AssetKey, limit: u64) -> AssetFuture<Vec<u8>> {
        let answer = match self.assets.get(key) {
            None => Err(AssetLoadError::NotFound { key: key.clone() }),
            Some(bytes) if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit => {
                Err(AssetLoadError::TooLarge {
                    key: key.clone(),
                    limit,
                })
            }
            Some(bytes) => Ok(bytes.to_vec()),
        };
        Box::pin(std::future::ready(answer))
    }
}
