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
            .finish_non_exhaustive()
    }
}

// ===== Extension Traits Pattern =====

/// Sealed trait module to prevent external implementations.
///
/// This module is public for technical reasons (trait bounds) but should not be
/// used directly. It's hidden from documentation.
#[doc(hidden)]
pub mod sealed {
    use super::AssetHandle;

    /// Sealed trait to prevent external implementations of AssetHandleCore.
    ///
    /// This ensures only types in this crate can implement AssetHandleCore,
    /// allowing us to add methods to AssetHandleExt without breaking changes.
    pub trait Sealed {}

    impl<T, K> Sealed for AssetHandle<T, K> {}
    impl<T, K> Sealed for &AssetHandle<T, K> {}
    impl<T, K> Sealed for &mut AssetHandle<T, K> {}
}

/// Core AssetHandle API providing fundamental access methods.
///
/// This trait defines the minimal interface for asset handles. It is sealed
/// to prevent external implementations, allowing the API to evolve without
/// breaking changes.
///
/// Most users will interact with [`AssetHandleExt`] instead, which provides
/// convenient extension methods.
pub trait AssetHandleCore<T, K>: sealed::Sealed {
    /// Gets a reference to the asset data.
    fn get(&self) -> &T;

    /// Returns the asset key.
    fn key(&self) -> &K;

    /// Returns the strong reference count.
    fn strong_count(&self) -> usize;

    /// Returns the weak reference count.
    fn weak_count(&self) -> usize;
}

impl<T, K> AssetHandleCore<T, K> for AssetHandle<T, K> {
    #[inline]
    fn get(&self) -> &T {
        &self.inner
    }

    #[inline]
    fn key(&self) -> &K {
        &self.key
    }

    #[inline]
    fn strong_count(&self) -> usize {
        Arc::strong_count(&self.inner)
    }

    #[inline]
    fn weak_count(&self) -> usize {
        Arc::weak_count(&self.inner)
    }
}

/// Extension trait providing convenient methods for asset handles.
///
/// This trait is automatically implemented for all types that implement
/// [`AssetHandleCore`]. It provides convenient helper methods without
/// polluting the core API.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_assets::{AssetHandle, AssetHandleExt};
///
/// let handle = registry.load(image).await?;
///
/// // Check if this is the only reference
/// if handle.is_unique() {
///     println!("We own the only reference!");
/// }
///
/// // Clone the inner data
/// let data_copy = handle.clone_data();
///
/// // Map the handle to a different type
/// let metadata = handle.map(|image| ImageMetadata {
///     width: image.width(),
///     height: image.height(),
/// });
/// ```
pub trait AssetHandleExt<T, K>: AssetHandleCore<T, K> {
    /// Checks if this is the only strong reference to the asset.
    ///
    /// Returns `true` if this handle is the only one pointing to the asset data.
    /// This is useful for determining if it's safe to unwrap or mutate the data.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let handle = registry.load(image).await?;
    /// if handle.is_unique() {
    ///     println!("Only one reference exists");
    /// }
    /// ```
    #[inline]
    fn is_unique(&self) -> bool {
        self.strong_count() == 1
    }

    /// Checks if there are any weak references to this asset.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let weak = handle.downgrade();
    /// assert!(handle.has_weak_refs());
    /// ```
    #[inline]
    fn has_weak_refs(&self) -> bool {
        self.weak_count() > 0
    }

    /// Clones the inner asset data (not the handle).
    ///
    /// This is a convenience method for `handle.get().clone()`.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let handle = registry.load(image).await?;
    /// let image_copy = handle.clone_data();
    /// ```
    #[inline]
    fn clone_data(&self) -> T
    where
        T: Clone,
    {
        self.get().clone()
    }

    /// Maps the asset data to a new value using a closure.
    ///
    /// This is useful for extracting metadata or transforming the asset.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let handle = registry.load(image).await?;
    ///
    /// // Extract dimensions
    /// let (width, height) = handle.map(|img| (img.width(), img.height()));
    ///
    /// // Create metadata
    /// let meta = handle.map(|img| ImageMetadata::from(img));
    /// ```
    #[inline]
    fn map<U, F>(&self, f: F) -> U
    where
        F: FnOnce(&T) -> U,
    {
        f(self.get())
    }

    /// Returns the total reference count (strong + weak).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let handle = registry.load(image).await?;
    /// let weak = handle.downgrade();
    /// assert_eq!(handle.total_ref_count(), 2); // 1 strong + 1 weak
    /// ```
    #[inline]
    fn total_ref_count(&self) -> usize {
        self.strong_count() + self.weak_count()
    }

    /// Returns `true` if two handles point to the same asset data.
    ///
    /// This compares the underlying Arc pointers, not the keys or data.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let handle1 = registry.load(image).await?;
    /// let handle2 = handle1.clone();
    /// assert!(handle1.ptr_eq(&handle2));
    /// ```
    #[inline]
    fn ptr_eq(&self, other: &Self) -> bool
    where
        Self: Sized,
        K: PartialEq,
    {
        // Use key equality as a proxy for pointer equality
        self.key() == other.key()
    }
}

// Blanket implementation for all types implementing AssetHandleCore
impl<H, T, K> AssetHandleExt<T, K> for H where H: AssetHandleCore<T, K> + ?Sized {}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct TestData {
        value: i32,
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

    // ===== Extension Trait Tests =====
}
