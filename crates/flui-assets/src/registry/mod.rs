//! Asset registry for central asset management.
//!
//! The registry provides a centralized system for loading and caching assets.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::cache::AssetCache;
use crate::core::Asset;
use crate::error::{AssetError, Result};
use crate::types::AssetHandle;

#[cfg(feature = "images")]
mod bridge;
#[cfg(feature = "images")]
use bridge::BridgeRuntime;

/// Asset registry for central asset management.
///
/// The registry manages caches for different asset types and provides
/// a unified API for loading assets.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_assets::{AssetRegistryBuilder, ImageAsset};
///
/// // Create a registry
/// let registry = AssetRegistryBuilder::new().with_default_capacity().build();
///
/// // Load an image
/// let image = ImageAsset::file("logo.png");
/// let handle = registry.load(image).await?;
///
/// println!("Loaded: {}x{}", handle.width(), handle.height());
/// ```
pub struct AssetRegistry {
    /// Type-erased caches for different asset types.
    /// Key: TypeId of the Asset type
    /// Value: `Box<dyn Any>` containing `AssetCache<T>`
    caches: Arc<RwLock<HashMap<TypeId, Box<dyn Any + Send + Sync>>>>,

    /// Default cache capacity in bytes.
    pub(crate) default_capacity: usize,

    /// A host-supplied runtime handle for [`load_image_bridged`](Self::load_image_bridged)
    /// to spawn onto, set at construction via
    /// [`AssetRegistryBuilder::with_runtime_handle`]. `None` defers to an
    /// ambient runtime, then an owned one — see [`BridgeRuntime::resolve`].
    #[cfg(feature = "images")]
    injected_runtime_handle: Option<tokio::runtime::Handle>,

    /// Backs bridged loads' runtime resolution — see [`BridgeRuntime`].
    #[cfg(feature = "images")]
    bridge_runtime: BridgeRuntime,

    /// One HTTP connection pool per registry, initialized on the loading runtime.
    #[cfg(all(feature = "images", feature = "network"))]
    network_loader: Arc<tokio::sync::OnceCell<crate::NetworkLoader>>,
}

impl std::fmt::Debug for AssetRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssetRegistry")
            .field("cache_count", &self.caches.read().len())
            .field("default_capacity", &self.default_capacity)
            // The bridge-runtime fields (images feature only) are omitted:
            // a runtime handle's own Debug output is not diagnostically
            // useful here, and printing whether one has been resolved yet
            // would make this impl's output depend on load order.
            .finish_non_exhaustive()
    }
}

impl AssetRegistry {
    /// Creates a new empty registry with the given default capacity.
    fn new(default_capacity: usize) -> Self {
        Self {
            caches: Arc::new(RwLock::new(HashMap::new())),
            default_capacity,
            #[cfg(feature = "images")]
            injected_runtime_handle: None,
            #[cfg(feature = "images")]
            bridge_runtime: BridgeRuntime::new(),
            #[cfg(all(feature = "images", feature = "network"))]
            network_loader: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    /// As [`new`](Self::new), additionally recording a host-supplied runtime
    /// handle for bridged image loads to spawn onto.
    #[cfg(feature = "images")]
    fn with_injected_handle(
        default_capacity: usize,
        injected_runtime_handle: Option<tokio::runtime::Handle>,
        #[cfg(feature = "network")] network_loader: Option<crate::NetworkLoader>,
    ) -> Self {
        Self {
            caches: Arc::new(RwLock::new(HashMap::new())),
            default_capacity,
            injected_runtime_handle,
            bridge_runtime: BridgeRuntime::new(),
            #[cfg(feature = "network")]
            network_loader: Arc::new(tokio::sync::OnceCell::new_with(network_loader)),
        }
    }

    /// Asynchronously loads and decodes the image asset at `path`, running
    /// the file read and decode on a background tokio runtime this registry
    /// owns or was handed — never on the caller's thread.
    ///
    /// Spawns onto [`AssetRegistryBuilder::with_runtime_handle`]'s injected
    /// handle if one was supplied at construction; otherwise an ambient multi-thread
    /// tokio runtime on the calling thread right now
    /// (`tokio::runtime::Handle::try_current`, re-checked on **every** call —
    /// never memoized, since an ambient runtime can shut down and restart
    /// between calls); otherwise a dedicated single-worker runtime, started
    /// on first need and reused (this one, unlike an ambient handle, is safe
    /// to memoize: this registry controls its lifetime completely).
    /// An ambient current-thread runtime is skipped because entering it alone
    /// does not drive spawned tasks. Explicitly injected runtimes must remain
    /// driven until the loads complete.
    ///
    /// The returned future is reactor-free: it only awaits a
    /// [`tokio::sync::oneshot`] receiver, so *polling* it never requires an
    /// ambient tokio context — only the spawn (done inside this method, once)
    /// needs a runtime handle.
    ///
    /// This method does not consult or populate [`AssetRegistry::load`]'s own
    /// cache (`AssetCache<ImageAsset>`, moka-backed with a 5-minute TTL) —
    /// the decoded-image cache a UI layer probes synchronously before
    /// spawning a load, and the in-flight load coalescing that lets two
    /// concurrent subscribers share one load, are both a UI-layer concern
    /// (`flui_widgets::image`'s decode cache), not this registry's.
    ///
    /// # Errors
    ///
    /// Returns [`AssetError::LoadFailed`] if the file cannot be read or
    /// decoded, or if the loading task is dropped before completing — most
    /// often because it panicked, but also possible if the runtime it was
    /// spawned on (an injected or ambient one this registry does not own)
    /// shuts down while the task is still in flight. Returns [`AssetError::Io`]
    /// if starting the owned runtime fails; a later call retries initialization.
    #[cfg(feature = "images")]
    pub fn load_image_bridged(
        &self,
        path: impl Into<String>,
    ) -> impl std::future::Future<Output = Result<crate::Image>> + Send + 'static {
        let path = path.into();
        let handle = self
            .bridge_runtime
            .resolve(self.injected_runtime_handle.as_ref());
        let (tx, rx) = tokio::sync::oneshot::channel();

        let spawn_path = path.clone();
        match handle {
            Ok(handle) => {
                handle.spawn(async move {
                    let asset = crate::assets::image::ImageAsset::file(spawn_path);
                    let outcome = Asset::load(&asset).await;
                    // A dropped receiver means the observer abandoned the load.
                    let _ = tx.send(outcome);
                });
            }
            Err(error) => {
                let _ = tx.send(Err(error));
            }
        }

        async move {
            rx.await.map_err(|_| AssetError::LoadFailed {
                path,
                reason: "the asset-loading task was dropped before completing".to_string(),
            })?
        }
    }

    /// Asynchronously fetches and decodes an image over HTTP/HTTPS via
    /// [`NetworkLoader`](crate::loaders::NetworkLoader), running the request
    /// and decode on the same background runtime
    /// [`load_image_bridged`](Self::load_image_bridged) uses — never on the
    /// caller's thread.
    ///
    /// Requires both the `images` (decode) and `network` (HTTP client)
    /// features.
    /// Requests share this registry's lazily initialized HTTP connection pool.
    /// Client initialization errors are returned and a later load may retry.
    ///
    /// # Errors
    ///
    /// Returns [`AssetError::LoadFailed`]/[`AssetError::NetworkError`] on a
    /// failed request or a failed decode of the response body, or if the
    /// loading task is dropped before completing — see
    /// [`load_image_bridged`](Self::load_image_bridged)'s `# Errors` for the
    /// same contract.
    #[cfg(all(feature = "images", feature = "network"))]
    pub fn load_network_image_bridged(
        &self,
        url: impl Into<String>,
    ) -> impl std::future::Future<Output = Result<crate::Image>> + Send + 'static {
        let url = url.into();
        let handle = self
            .bridge_runtime
            .resolve(self.injected_runtime_handle.as_ref());
        let (tx, rx) = tokio::sync::oneshot::channel();

        let spawn_url = url.clone();
        let network_loader = Arc::clone(&self.network_loader);
        match handle {
            Ok(handle) => {
                handle.spawn(async move {
                    let outcome = async {
                        let loader = network_loader
                            .get_or_try_init(|| async {
                                crate::NetworkLoader::new().map_err(|error| {
                                    AssetError::LoadFailed {
                                        path: spawn_url.clone(),
                                        reason: format!(
                                            "HTTP client initialization failed: {error}"
                                        ),
                                    }
                                })
                            })
                            .await?;
                        let bytes = loader.load_url(&spawn_url).await?;
                        let asset =
                            crate::assets::image::ImageAsset::from_bytes(spawn_url.clone(), bytes);
                        Asset::load(&asset).await
                    }
                    .await;
                    let _ = tx.send(outcome);
                });
            }
            Err(error) => {
                let _ = tx.send(Err(error));
            }
        }

        async move {
            rx.await.map_err(|_| AssetError::LoadFailed {
                path: url,
                reason: "the network-image loading task was dropped before completing".to_string(),
            })?
        }
    }

    /// Loads an asset, using the cache if available.
    ///
    /// Validates each supplied asset before consulting the cache. A rejected
    /// descriptor returns its validation error even on a cache hit, without
    /// loading or changing previously cached data.
    /// If the asset is already cached, returns the cached version immediately.
    /// Otherwise, concurrent requests for the same typed key share one load.
    /// Loading errors are shared with current waiters but are not cached; a
    /// later request may retry. Cancelling the initializing request lets a
    /// remaining waiter initialize from its own accepted descriptor.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The asset cannot be loaded (`AssetError::LoadFailed`)
    /// - The asset data is invalid (`AssetError::DecodeFailed`)
    /// - The asset format is unsupported (`AssetError::UnsupportedFormat`)
    /// - Any I/O error occurs (`AssetError::Io`)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let image = ImageAsset::file("logo.png");
    /// let handle = registry.load(image).await?;
    /// ```
    pub async fn load<T>(&self, asset: T) -> Result<AssetHandle<T::Data, T::Key>>
    where
        T: Asset<Error = AssetError>,
        T::Key: std::hash::Hash + Eq + Clone,
    {
        asset.validate()?;
        let key = asset.key();
        let cache = self.get_or_create_cache::<T>();

        cache
            .get_or_insert_coalesced_with(key, || asset.load())
            .await
    }

    /// Gets an asset from cache without loading.
    ///
    /// Returns `None` if the asset is not cached.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let key = AssetKey::new("logo.png");
    /// if let Some(handle) = registry.get::<ImageAsset>(&key).await {
    ///     println!("Found in cache!");
    /// }
    /// ```
    pub async fn get<T>(&self, key: &T::Key) -> Option<AssetHandle<T::Data, T::Key>>
    where
        T: Asset,
        T::Key: std::hash::Hash + Eq + Clone,
    {
        self.get_cache::<T>()?.get(key).await
    }

    /// Preloads an asset into the cache.
    ///
    /// This is useful for warming up the cache before assets are needed.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Preload critical assets at startup
    /// registry.preload(ImageAsset::file("logo.png")).await?;
    /// registry.preload(FontAsset::file("Roboto-Regular.ttf")).await?;
    /// ```
    pub async fn preload<T>(&self, asset: T) -> Result<()>
    where
        T: Asset<Error = AssetError>,
        T::Key: std::hash::Hash + Eq + Clone,
    {
        self.load(asset).await?;
        Ok(())
    }

    /// Invalidates (removes) an asset from the cache.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let key = AssetKey::new("logo.png");
    /// registry.invalidate::<ImageAsset>(&key).await;
    /// ```
    pub async fn invalidate<T>(&self, key: &T::Key)
    where
        T: Asset,
        T::Key: std::hash::Hash + Eq + Clone,
    {
        if let Some(cache) = self.get_cache::<T>() {
            cache.invalidate(key).await;
        }
    }

    /// Clears all cached assets of a specific type.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Clear all cached images
    /// registry.clear::<ImageAsset>().await;
    /// ```
    pub async fn clear<T>(&self)
    where
        T: Asset,
        T::Key: std::hash::Hash + Eq + Clone,
    {
        if let Some(cache) = self.get_cache::<T>() {
            cache.clear().await;
        }
    }

    /// Clears all caches in the registry.
    #[expect(
        clippy::unused_async,
        reason = "public API: uniform async surface with the genuinely-async `invalidate`/`clear` siblings"
    )]
    pub async fn clear_all(&self) {
        let mut caches = self.caches.write();
        caches.clear();
    }

    /// Gets the cache for a specific asset type, if it exists.
    fn get_cache<T>(&self) -> Option<AssetCache<T>>
    where
        T: Asset,
        T::Key: std::hash::Hash + Eq + Clone,
    {
        let caches = self.caches.read();
        let type_id = TypeId::of::<T>();

        caches
            .get(&type_id)
            .and_then(|any| any.downcast_ref::<AssetCache<T>>().cloned())
    }

    /// Gets or creates the cache for a specific asset type.
    fn get_or_create_cache<T>(&self) -> AssetCache<T>
    where
        T: Asset,
        T::Key: std::hash::Hash + Eq + Clone,
    {
        let type_id = TypeId::of::<T>();

        // Fast path: cache already exists
        {
            let caches = self.caches.read();
            if let Some(any) = caches.get(&type_id)
                && let Some(cache) = any.downcast_ref::<AssetCache<T>>()
            {
                return cache.clone();
            }
        }

        // Slow path: create new cache
        let mut caches = self.caches.write();

        // Double-check in case another thread created it
        if let Some(any) = caches.get(&type_id)
            && let Some(cache) = any.downcast_ref::<AssetCache<T>>()
        {
            return cache.clone();
        }

        // Create new cache
        let cache = AssetCache::<T>::new(self.default_capacity);
        caches.insert(type_id, Box::new(cache.clone()));
        cache
    }
}

impl Default for AssetRegistry {
    fn default() -> Self {
        Self::new(100 * 1024 * 1024) // 100 MB
    }
}

// ===== Type State Builder Pattern =====

/// Type-state marker: Capacity not yet set.
#[derive(Debug, Clone, Copy)]
pub struct NoCapacity;

/// Type-state marker: Capacity has been set.
#[derive(Debug, Clone, Copy)]
pub struct HasCapacity(pub(crate) usize);

/// Builder for constructing an asset registry with compile-time validation.
///
/// This builder uses the type-state pattern to ensure required configuration
/// is provided at compile-time. The `build()` method is only available after
/// capacity has been set.
///
/// # Type States
///
/// - `AssetRegistryBuilder<NoCapacity>` - Initial state, capacity must be set
/// - `AssetRegistryBuilder<HasCapacity>` - Ready to build
///
/// # Examples
///
/// ```rust,ignore
/// use flui_assets::AssetRegistryBuilder;
///
/// // This compiles - capacity is set
/// let registry = AssetRegistryBuilder::new()
///     .with_capacity(200 * 1024 * 1024) // 200 MB
///     .build();
///
/// // This won't compile - capacity not set
/// // let registry = AssetRegistryBuilder::new().build(); // ❌ ERROR
/// ```
///
/// # Default Capacity
///
/// If you want a registry with default capacity, use `with_default_capacity()`:
///
/// ```rust,ignore
/// let registry = AssetRegistryBuilder::new()
///     .with_default_capacity() // 100 MB
///     .build();
/// ```
#[derive(Debug)]
pub struct AssetRegistryBuilder<C = NoCapacity> {
    capacity: C,
    /// Set via [`with_runtime_handle`](Self::with_runtime_handle); carried
    /// across capacity-state transitions and consumed by
    /// [`AssetRegistryBuilder::<HasCapacity>::build`].
    #[cfg(feature = "images")]
    runtime_handle: Option<tokio::runtime::Handle>,
    #[cfg(all(feature = "images", feature = "network"))]
    network_loader: Option<crate::NetworkLoader>,
}

// ===== Initial State: NoCapacity =====

impl AssetRegistryBuilder<NoCapacity> {
    /// Creates a new registry builder.
    ///
    /// You must call `with_capacity()` or `with_default_capacity()` before building.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let builder = AssetRegistryBuilder::new();
    /// // builder.build(); // ❌ Won't compile - capacity not set
    /// ```
    pub fn new() -> Self {
        Self {
            capacity: NoCapacity,
            #[cfg(feature = "images")]
            runtime_handle: None,
            #[cfg(all(feature = "images", feature = "network"))]
            network_loader: None,
        }
    }

    /// Sets a custom cache capacity in bytes.
    ///
    /// This capacity is used for each asset type's cache.
    ///
    /// # Arguments
    ///
    /// * `capacity_bytes` - Cache capacity in bytes (must be > 0)
    ///
    /// # Panics
    ///
    /// Panics if `capacity_bytes` is 0.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let registry = AssetRegistryBuilder::new()
    ///     .with_capacity(500 * 1024 * 1024) // 500 MB
    ///     .build();
    /// ```
    pub fn with_capacity(self, capacity_bytes: usize) -> AssetRegistryBuilder<HasCapacity> {
        assert!(capacity_bytes > 0, "Capacity must be greater than 0");
        AssetRegistryBuilder {
            capacity: HasCapacity(capacity_bytes),
            #[cfg(feature = "images")]
            runtime_handle: self.runtime_handle,
            #[cfg(all(feature = "images", feature = "network"))]
            network_loader: self.network_loader,
        }
    }

    /// Sets the default cache capacity (100 MB).
    ///
    /// This is a convenience method for the common case.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let registry = AssetRegistryBuilder::new()
    ///     .with_default_capacity()
    ///     .build();
    /// ```
    pub fn with_default_capacity(self) -> AssetRegistryBuilder<HasCapacity> {
        AssetRegistryBuilder {
            capacity: HasCapacity(100 * 1024 * 1024), // 100 MB
            #[cfg(feature = "images")]
            runtime_handle: self.runtime_handle,
            #[cfg(all(feature = "images", feature = "network"))]
            network_loader: self.network_loader,
        }
    }
}

impl<C> AssetRegistryBuilder<C> {
    /// Builds a fresh configured HTTP client for bridged network image requests.
    ///
    /// Configure deadlines, proxies, headers and redirects through the client
    /// builder. Accepting a builder rather than an already-used client prevents
    /// importing connections driven by another runtime. A fresh client establishes
    /// connections on the bridge's selected loading runtime; configuration does
    /// not change runtime selection.
    ///
    /// Without configuration, a default loader initializes lazily on the loading
    /// runtime.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid client configuration or failed TLS/resolver
    /// initialization.
    #[cfg(all(feature = "images", feature = "network"))]
    pub fn with_network_client(mut self, client: reqwest::ClientBuilder) -> reqwest::Result<Self> {
        self.network_loader = Some(crate::NetworkLoader::with_client(client.build()?));
        Ok(self)
    }

    /// Injects a tokio runtime handle for
    /// [`AssetRegistry::load_image_bridged`] to spawn onto, instead of
    /// reusing an ambient runtime or starting an owned background one.
    ///
    /// Use this when the host application already runs a tokio runtime whose
    /// lifecycle it wants bridged asset loads to share — e.g. a
    /// `#[tokio::main]` binary that wants every background task on one
    /// runtime.
    ///
    /// The host must keep this runtime alive and driving tasks until bridged
    /// loads finish. A current-thread runtime must be driven by `Runtime::block_on`;
    /// entering its handle alone does not run tasks. Injection is an explicit
    /// ownership choice and is honored even for current-thread runtimes.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[tokio::main]
    /// # async fn main() {
    /// use flui_assets::AssetRegistryBuilder;
    ///
    /// let registry = AssetRegistryBuilder::new()
    ///     .with_capacity(50 * 1024 * 1024)
    ///     .with_runtime_handle(tokio::runtime::Handle::current())
    ///     .build();
    /// # let _ = registry;
    /// # }
    /// ```
    #[cfg(feature = "images")]
    #[must_use]
    pub fn with_runtime_handle(mut self, handle: tokio::runtime::Handle) -> Self {
        self.runtime_handle = Some(handle);
        self
    }
}

// ===== Final State: HasCapacity =====

impl AssetRegistryBuilder<HasCapacity> {
    /// Builds the asset registry.
    ///
    /// This method is only available after capacity has been set.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let registry = AssetRegistryBuilder::new()
    ///     .with_capacity(200 * 1024 * 1024)
    ///     .build();
    /// ```
    pub fn build(self) -> AssetRegistry {
        #[cfg(feature = "images")]
        {
            AssetRegistry::with_injected_handle(
                self.capacity.0,
                self.runtime_handle,
                #[cfg(feature = "network")]
                self.network_loader,
            )
        }
        #[cfg(not(feature = "images"))]
        {
            AssetRegistry::new(self.capacity.0)
        }
    }

    /// Updates the capacity after it has been set.
    ///
    /// This allows changing the capacity even after calling `with_capacity()`.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let registry = AssetRegistryBuilder::new()
    ///     .with_capacity(100 * 1024 * 1024)
    ///     .with_capacity(200 * 1024 * 1024) // Override previous value
    ///     .build();
    /// ```
    pub fn with_capacity(self, capacity_bytes: usize) -> AssetRegistryBuilder<HasCapacity> {
        assert!(capacity_bytes > 0, "Capacity must be greater than 0");
        AssetRegistryBuilder {
            capacity: HasCapacity(capacity_bytes),
            #[cfg(feature = "images")]
            runtime_handle: self.runtime_handle,
            #[cfg(all(feature = "images", feature = "network"))]
            network_loader: self.network_loader,
        }
    }
}

// ===== Convenience: Default Implementation =====

impl Default for AssetRegistryBuilder<NoCapacity> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::FontAsset;
    use crate::types::AssetKey;

    #[tokio::test]
    async fn test_registry_invalidate() {
        let registry = AssetRegistry::default();

        let ttf_bytes = vec![0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let font = FontAsset::from_bytes("test.ttf", ttf_bytes);

        let _handle = registry.load(font).await.unwrap();

        let key = AssetKey::new("test.ttf");
        assert!(registry.get::<FontAsset>(&key).await.is_some());

        // Invalidate
        registry.invalidate::<FontAsset>(&key).await;
        assert!(registry.get::<FontAsset>(&key).await.is_none());
    }
}
