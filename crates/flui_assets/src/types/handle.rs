//! Arc-based handles for shared asset ownership.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::sync::Arc;

use crate::types::AssetKey;

/// A strong reference to a loaded asset.
///
/// `AssetHandle` provides shared ownership of asset data using an Arc.
///
/// The handle also stores the asset key for identification and cache lookups.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_assets::{AssetHandle, AssetKey};
///
/// let handle: AssetHandle<Image, AssetKey> = registry.load(key).await?;
///
/// // Access the data
/// let image = handle.get();
/// println!("Image size: {}x{}", image.width(), image.height());
///
/// // Cheap clone (just increments ref count)
/// let handle2 = handle.clone();
/// ```
pub struct AssetHandle<T, K = AssetKey> {
    /// The asset data (shared via Arc).
    inner: Arc<T>,
    /// The asset key (for identification).
    key: K,
}

impl<T, K> AssetHandle<T, K>
where
    K: Clone,
{
    /// Creates a new asset handle.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_assets::{AssetHandle, AssetKey};
    /// use triomphe::Arc;
    ///
    /// let data = Image::new(100, 100);
    /// let handle = AssetHandle::new(Arc::new(data), AssetKey::new("test.png"));
    /// ```
    #[inline]
    pub fn new(data: Arc<T>, key: K) -> Self {
        Self { inner: data, key }
    }

    /// Gets a reference to the asset data.
    ///
    /// This is a zero-cost operation.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let image = handle.get();
    /// println!("Width: {}", image.width());
    /// ```
    #[inline]
    pub fn get(&self) -> &T {
        &self.inner
    }

    /// Returns the asset key.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let key = handle.key();
    /// println!("Asset: {}", key);
    /// ```
    #[inline]
    pub fn key(&self) -> &K {
        &self.key
    }

    /// Downgrades to a weak handle.
    ///
    /// Weak handles don't prevent the asset from being evicted from cache.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let weak = handle.downgrade();
    ///
    /// // Later, try to upgrade back to strong reference
    /// if let Some(handle) = weak.upgrade() {
    ///     // Asset is still loaded
    /// }
    /// ```
    #[inline]
    pub fn downgrade(&self) -> WeakAssetHandle<T, K> {
        WeakAssetHandle {
            inner: Arc::downgrade(&self.inner),
            key: self.key.clone(),
        }
    }
}

impl<T, K> AssetHandle<T, K> {
    /// Returns the strong reference count.
    ///
    /// This counts how many `AssetHandle` instances exist for this asset.
    #[inline]
    pub fn strong_count(&self) -> usize {
        Arc::strong_count(&self.inner)
    }

    /// Returns the weak reference count.
    ///
    /// This counts how many `WeakAssetHandle` instances exist for this asset.
    #[inline]
    pub fn weak_count(&self) -> usize {
        Arc::weak_count(&self.inner)
    }
}

impl<T, K> Clone for AssetHandle<T, K>
where
    K: Clone,
{
    #[inline]
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            key: self.key.clone(),
        }
    }
}

impl<T, K> Deref for AssetHandle<T, K> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

// Hash only by key, not by data pointer
impl<T, K> Hash for AssetHandle<T, K>
where
    K: Hash,
{
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
    }
}

impl<T, K> PartialEq for AssetHandle<T, K>
where
    K: PartialEq,
{
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<T, K> Eq for AssetHandle<T, K> where K: Eq {}

impl<T, K> fmt::Debug for AssetHandle<T, K>
where
    T: fmt::Debug,
    K: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AssetHandle")
            .field("key", &self.key)
            .field("data", &self.inner)
            .field("strong_count", &self.strong_count())
            .finish()
    }
}

/// A weak reference to a loaded asset.
///
/// `WeakAssetHandle` is a non-owning reference to an asset. It doesn't prevent
/// the asset from being evicted from cache. Use `upgrade()` to convert back to
/// a strong `AssetHandle` if the asset is still loaded.
///
/// # Examples
///
/// ```rust,ignore
/// let weak = handle.downgrade();
/// drop(handle); // Release strong reference
///
/// // Later...
/// match weak.upgrade() {
///     Some(handle) => println!("Asset still loaded"),
///     None => println!("Asset was evicted"),
/// }
/// ```
pub struct WeakAssetHandle<T, K = AssetKey> {
    inner: std::sync::Weak<T>,
    key: K,
}

impl<T, K> WeakAssetHandle<T, K>
where
    K: Clone,
{
    /// Attempts to upgrade to a strong handle.
    ///
    /// Returns `None` if the asset has been evicted from cache.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if let Some(handle) = weak.upgrade() {
    ///     println!("Got asset: {}", handle.key());
    /// }
    /// ```
    #[inline]
    pub fn upgrade(&self) -> Option<AssetHandle<T, K>> {
        self.inner.upgrade().map(|inner| AssetHandle {
            inner,
            key: self.key.clone(),
        })
    }

    /// Returns the asset key.
    #[inline]
    pub fn key(&self) -> &K {
        &self.key
    }
}

impl<T, K> WeakAssetHandle<T, K> {
    /// Returns the weak reference count.
    #[inline]
    pub fn weak_count(&self) -> usize {
        std::sync::Weak::weak_count(&self.inner)
    }
}

impl<T, K> Clone for WeakAssetHandle<T, K>
where
    K: Clone,
{
    #[inline]
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            key: self.key.clone(),
        }
    }
}

impl<T, K> fmt::Debug for WeakAssetHandle<T, K>
where
    K: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WeakAssetHandle")
            .field("key", &self.key)
            .field("weak_count", &self.weak_count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct TestData {
        value: i32,
    }

    #[test]
    fn test_handle_creation() {
        let data = TestData { value: 42 };
        let handle = AssetHandle::new(Arc::new(data), AssetKey::new("test"));

        assert_eq!(handle.get().value, 42);
        assert_eq!(handle.key().as_str(), "test");
    }

    #[test]
    fn test_handle_clone() {
        let data = TestData { value: 42 };
        let handle1 = AssetHandle::new(Arc::new(data), AssetKey::new("test"));
        let handle2 = handle1.clone();

        assert_eq!(handle1, handle2);
        assert_eq!(handle1.get().value, handle2.get().value);
        assert_eq!(handle1.strong_count(), 2);
    }

    #[test]
    fn test_handle_deref() {
        let data = TestData { value: 42 };
        let handle = AssetHandle::new(Arc::new(data), AssetKey::new("test"));

        // Can use deref to access fields directly
        assert_eq!(handle.value, 42);
    }

    #[test]
    fn test_handle_downgrade_upgrade() {
        let data = TestData { value: 42 };
        let handle = AssetHandle::new(Arc::new(data), AssetKey::new("test"));

        let weak = handle.downgrade();
        assert_eq!(weak.key().as_str(), "test");

        // Should be able to upgrade
        let upgraded = weak.upgrade().unwrap();
        assert_eq!(upgraded.get().value, 42);
        assert_eq!(upgraded.key(), handle.key());
    }

    #[test]
    fn test_weak_handle_after_drop() {
        let data = TestData { value: 42 };
        let handle = AssetHandle::new(Arc::new(data), AssetKey::new("test"));
        let weak = handle.downgrade();

        drop(handle);

        // After dropping strong handle, upgrade should fail
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn test_handle_equality() {
        let data1 = TestData { value: 42 };
        let data2 = TestData { value: 99 };

        let handle1 = AssetHandle::new(Arc::new(data1), AssetKey::new("test"));
        let handle2 = AssetHandle::new(Arc::new(data2), AssetKey::new("test"));
        let handle3 = AssetHandle::new(Arc::new(TestData { value: 42 }), AssetKey::new("other"));

        // Equality based on key, not data
        assert_eq!(handle1, handle2); // Same key
        assert_ne!(handle1, handle3); // Different key
    }

    #[test]
    fn test_handle_ref_counts() {
        let data = TestData { value: 42 };
        let handle1 = AssetHandle::new(Arc::new(data), AssetKey::new("test"));

        assert_eq!(handle1.strong_count(), 1);
        assert_eq!(handle1.weak_count(), 0);

        let handle2 = handle1.clone();
        assert_eq!(handle1.strong_count(), 2);

        let weak = handle1.downgrade();
        assert_eq!(handle1.weak_count(), 1);

        drop(handle2);
        assert_eq!(handle1.strong_count(), 1);

        drop(weak);
        assert_eq!(handle1.weak_count(), 0);
    }

    #[test]
    fn test_handle_debug() {
        let data = TestData { value: 42 };
        let handle = AssetHandle::new(Arc::new(data), AssetKey::new("test"));

        let debug_str = format!("{:?}", handle);
        assert!(debug_str.contains("AssetHandle"));
        // AssetKey debug format includes the string
        assert!(debug_str.contains("key"));
    }
}
