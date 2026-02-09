//! In-memory asset loader.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::core::{Asset, AssetLoader, AssetMetadata};
use crate::error::AssetError;

/// Loads assets from memory.
///
/// Useful for embedded assets, testing, or hot-reloading scenarios.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_assets::loaders::MemoryLoader;
///
/// let mut loader = MemoryLoader::new();
/// loader.insert("logo.png", logo_bytes);
///
/// let data = loader.load(&AssetKey::new("logo.png")).await?;
/// ```
#[derive(Debug, Clone)]
pub struct MemoryLoader<K, D> {
    /// In-memory storage of assets.
    storage: Arc<RwLock<HashMap<K, Arc<D>>>>,

    /// Optional metadata for each asset.
    metadata: Arc<RwLock<HashMap<K, AssetMetadata>>>,
}

impl<K, D> Default for MemoryLoader<K, D> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K, D> MemoryLoader<K, D> {
    /// Creates a new empty memory loader.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let loader = MemoryLoader::<AssetKey, Vec<u8>>::new();
    /// ```
    pub fn new() -> Self {
        Self {
            storage: Arc::new(RwLock::new(HashMap::new())),
            metadata: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Creates a memory loader with initial capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            storage: Arc::new(RwLock::new(HashMap::with_capacity(capacity))),
            metadata: Arc::new(RwLock::new(HashMap::with_capacity(capacity))),
        }
    }
}

impl<K, D> MemoryLoader<K, D>
where
    K: Eq + std::hash::Hash + Clone,
    D: Clone,
{
    /// Inserts an asset into memory.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// loader.insert(AssetKey::new("test"), vec![1, 2, 3]);
    /// ```
    pub fn insert(&self, key: K, data: D) {
        self.storage.write().insert(key, Arc::new(data));
    }

    /// Inserts an asset with metadata.
    pub fn insert_with_metadata(&self, key: K, data: D, metadata: AssetMetadata) {
        {
            let mut storage = self.storage.write();
            storage.insert(key.clone(), Arc::new(data));
        }
        {
            let mut meta = self.metadata.write();
            meta.insert(key, metadata);
        }
    }

    /// Removes an asset from memory.
    pub fn remove(&self, key: &K) -> Option<Arc<D>> {
        let mut storage = self.storage.write();
        let result = storage.remove(key);

        // Also remove metadata
        let mut meta = self.metadata.write();
        meta.remove(key);

        result
    }

    /// Checks if an asset exists in memory.
    pub fn contains(&self, key: &K) -> bool {
        self.storage.read().contains_key(key)
    }

    /// Returns the number of assets in memory.
    pub fn len(&self) -> usize {
        self.storage.read().len()
    }

    /// Returns whether the loader is empty.
    pub fn is_empty(&self) -> bool {
        self.storage.read().is_empty()
    }

    /// Clears all assets from memory.
    pub fn clear(&self) {
        self.storage.write().clear();
        self.metadata.write().clear();
    }
}

impl<T> AssetLoader<T> for MemoryLoader<T::Key, T::Data>
where
    T: Asset<Error = AssetError>,
    T::Key: Eq + std::hash::Hash + Clone + std::fmt::Display,
    T::Data: Clone,
{
    async fn load(&self, key: &T::Key) -> std::result::Result<T::Data, T::Error> {
        let storage = self.storage.read();
        storage
            .get(key)
            .map(|arc| (**arc).clone())
            .ok_or_else(|| AssetError::NotFound {
                path: format!("memory://{}", key),
            })
    }

    async fn exists(&self, key: &T::Key) -> std::result::Result<bool, T::Error> {
        Ok(self.contains(key))
    }

    async fn metadata(&self, key: &T::Key) -> std::result::Result<Option<AssetMetadata>, T::Error> {
        let meta = self.metadata.read();
        Ok(meta.get(key).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AssetKey;

    #[tokio::test]
    async fn test_memory_loader_insert_and_load() {
        let loader: MemoryLoader<AssetKey, Vec<u8>> = MemoryLoader::new();
        let key = AssetKey::new("test");
        let data = vec![1, 2, 3, 4, 5];

        loader.insert(key, data.clone());

        assert!(loader.contains(&key));
        assert_eq!(loader.len(), 1);

        // Load the data using AssetLoader trait
        let loaded: Vec<u8> = AssetLoader::<DummyAsset>::load(&loader, &key)
            .await
            .unwrap();
        assert_eq!(loaded, data);
    }

    #[tokio::test]
    async fn test_memory_loader_not_found() {
        let loader: MemoryLoader<AssetKey, Vec<u8>> = MemoryLoader::new();
        let key = AssetKey::new("nonexistent");

        let result: std::result::Result<Vec<u8>, AssetError> =
            AssetLoader::<DummyAsset>::load(&loader, &key).await;
        assert!(result.is_err());

        if let Err(AssetError::NotFound { .. }) = result {
            // Expected error type
        } else {
            panic!("Expected NotFound error");
        }
    }

    #[tokio::test]
    async fn test_memory_loader_remove() {
        let loader = MemoryLoader::<AssetKey, Vec<u8>>::new();
        let key = AssetKey::new("test");
        let data = vec![1, 2, 3];

        loader.insert(key, data.clone());
        assert!(loader.contains(&key));

        let removed = loader.remove(&key);
        assert!(removed.is_some());
        assert_eq!(*removed.unwrap(), data);
        assert!(!loader.contains(&key));
    }

    #[tokio::test]
    async fn test_memory_loader_clear() {
        let loader = MemoryLoader::<AssetKey, Vec<u8>>::new();

        loader.insert(AssetKey::new("test1"), vec![1]);
        loader.insert(AssetKey::new("test2"), vec![2]);
        loader.insert(AssetKey::new("test3"), vec![3]);

        assert_eq!(loader.len(), 3);

        loader.clear();

        assert_eq!(loader.len(), 0);
        assert!(loader.is_empty());
    }

    #[tokio::test]
    async fn test_memory_loader_with_metadata() {
        let loader: MemoryLoader<AssetKey, Vec<u8>> = MemoryLoader::new();
        let key = AssetKey::new("test");
        let data = vec![1, 2, 3];

        let metadata = AssetMetadata {
            size_bytes: Some(3),
            format: Some("BIN".to_string()),
            ..Default::default()
        };

        loader.insert_with_metadata(key, data, metadata.clone());

        let loaded_meta: Option<AssetMetadata> = AssetLoader::<DummyAsset>::metadata(&loader, &key)
            .await
            .unwrap();
        assert_eq!(loaded_meta, Some(metadata));
    }

    #[tokio::test]
    async fn test_memory_loader_exists() {
        let loader: MemoryLoader<AssetKey, Vec<u8>> = MemoryLoader::new();
        let key = AssetKey::new("test");

        assert!(!AssetLoader::<DummyAsset>::exists(&loader, &key)
            .await
            .unwrap());

        loader.insert(key, vec![1, 2, 3]);

        assert!(AssetLoader::<DummyAsset>::exists(&loader, &key)
            .await
            .unwrap());
    }

    // Dummy asset for testing
    struct DummyAsset;

    impl Asset for DummyAsset {
        type Data = Vec<u8>;
        type Key = AssetKey;
        type Error = AssetError;

        fn key(&self) -> AssetKey {
            AssetKey::new("dummy")
        }

        async fn load(&self) -> std::result::Result<Vec<u8>, AssetError> {
            Ok(vec![])
        }
    }
}
