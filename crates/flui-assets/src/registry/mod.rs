//! Asset registry for central asset management.
//!
//! The registry provides a centralized system for loading and caching assets.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use flui_foundation::memory::{MemoryPressureLevel, ReclaimMemory};
use parking_lot::RwLock;

use crate::cache::{AssetCache, AssetCacheConfig, CacheCapacity};
use crate::core::Asset;
use crate::error::{AssetError, Result};
use crate::types::AssetHandle;

#[cfg(feature = "images")]
mod bridge;
#[cfg(feature = "images")]
mod images;
#[cfg(feature = "images")]
mod ordered_slots;
#[cfg(feature = "images")]
use bridge::BridgeRuntime;
#[cfg(feature = "images")]
pub use images::{ImageLoad, ImageSource};

/// How a registry keeps and decodes bridged images.
#[cfg(feature = "images")]
#[derive(Debug, Clone, Copy)]
pub(crate) struct ImageConfig {
    /// The most decoded pixel bytes kept.
    pub(crate) budget: usize,
    /// The most decodes running at once.
    pub(crate) decode_threads: std::num::NonZeroUsize,
}

#[cfg(feature = "images")]
impl Default for ImageConfig {
    fn default() -> Self {
        Self {
            budget: images::DEFAULT_DECODED_IMAGE_BYTES,
            decode_threads: images::default_decode_threads(),
        }
    }
}

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

    /// Count capacity and expiration applied separately to each asset type.
    pub(crate) cache_config: AssetCacheConfig,

    /// A host-supplied runtime handle for [`load_image_bridged`](Self::load_image_bridged)
    /// to spawn onto, set at construction via
    /// [`AssetRegistryBuilder::with_runtime_handle`]. `None` defers to an
    /// ambient runtime, then an owned one — see [`BridgeRuntime::resolve`].
    #[cfg(feature = "images")]
    injected_runtime_handle: Option<tokio::runtime::Handle>,

    /// Backs bridged loads' runtime resolution — see [`BridgeRuntime`].
    #[cfg(feature = "images")]
    bridge_runtime: BridgeRuntime,

    /// Bridged loads' decoded images and loads in flight.
    #[cfg(feature = "images")]
    decoded_images: Arc<images::DecodedImages>,

    /// One HTTP connection pool per registry, initialized on the loading runtime.
    #[cfg(all(feature = "images", feature = "network"))]
    network_loader: Arc<tokio::sync::OnceCell<crate::NetworkLoader>>,
}

impl std::fmt::Debug for AssetRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssetRegistry")
            .field("cache_count", &self.caches.read().len())
            .field("cache_config", &self.cache_config)
            // The bridge-runtime fields (images feature only) are omitted:
            // a runtime handle's own Debug output is not diagnostically
            // useful here, and printing whether one has been resolved yet
            // would make this impl's output depend on load order.
            .finish_non_exhaustive()
    }
}

impl AssetRegistry {
    /// Creates a new empty registry with the given default capacity.
    fn new(cache_config: AssetCacheConfig) -> Self {
        #[cfg(feature = "images")]
        {
            Self::with_images(
                cache_config,
                None,
                ImageConfig::default(),
                #[cfg(feature = "network")]
                None,
            )
        }
        #[cfg(not(feature = "images"))]
        {
            Self {
                caches: Arc::new(RwLock::new(HashMap::new())),
                cache_config,
            }
        }
    }

    /// As [`new`](Self::new), additionally recording a host-supplied runtime
    /// handle for bridged image loads to spawn onto and how decoded images
    /// are kept.
    #[cfg(feature = "images")]
    fn with_images(
        cache_config: AssetCacheConfig,
        injected_runtime_handle: Option<tokio::runtime::Handle>,
        image_config: ImageConfig,
        #[cfg(feature = "network")] network_loader: Option<crate::NetworkLoader>,
    ) -> Self {
        Self {
            caches: Arc::new(RwLock::new(HashMap::new())),
            cache_config,
            injected_runtime_handle,
            bridge_runtime: BridgeRuntime::new(image_config.decode_threads),
            decoded_images: Arc::new(images::DecodedImages::new(
                image_config.budget,
                image_config.decode_threads,
            )),
            #[cfg(feature = "network")]
            network_loader: Arc::new(tokio::sync::OnceCell::new_with(network_loader)),
        }
    }

    /// Loads and decodes the image asset at `path` off the caller's thread,
    /// through this registry's decoded-image cache.
    ///
    /// A completed image for `path` is returned at once and made most recently
    /// used; a request for a path already loading joins that load; otherwise
    /// one task reads the file on the bridge runtime and decodes it on that
    /// runtime's blocking pool, at most
    /// [`with_decode_threads`](AssetRegistryBuilder::with_decode_threads)
    /// decodes at a time. The decoded image is kept within
    /// [`with_decoded_image_budget`](AssetRegistryBuilder::with_decoded_image_budget);
    /// failures are not kept, so a later request retries.
    ///
    /// Dropping the last [`ImageLoad`] waiting for a load cancels it: the file
    /// is read in chunks and stops at the next one (a chunk already being read
    /// on a blocking thread finishes), and a decode that has not started never
    /// runs. Loads start reading in the order they were requested, up to four
    /// per decode thread; a decode slot goes to the earliest-requested load
    /// whose bytes are ready, never held for one still fetching.
    ///
    /// The task spawns onto [`AssetRegistryBuilder::with_runtime_handle`]'s
    /// injected handle if one was supplied at construction; otherwise an
    /// ambient multi-thread tokio runtime on the calling thread right now
    /// (`tokio::runtime::Handle::try_current`, re-checked on **every** call —
    /// never memoized, since an ambient runtime can shut down and restart
    /// between calls); otherwise a dedicated single-worker runtime, started
    /// on first need and reused (this one, unlike an ambient handle, is safe
    /// to memoize: this registry controls its lifetime completely).
    /// An ambient current-thread runtime is skipped because entering it alone
    /// does not drive spawned tasks. Explicitly injected runtimes must remain
    /// driven until the loads complete.
    ///
    /// The returned future is reactor-free: polling it never requires an
    /// ambient tokio context — only the spawn (done inside this method, once)
    /// needs a runtime handle.
    ///
    /// This method does not consult or populate [`AssetRegistry::load`]'s
    /// typed cache (`AssetCache<ImageAsset>`): the decoded-image cache is the
    /// one a UI layer probes synchronously with [`cached_image`](Self::cached_image).
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
    pub fn load_image_bridged(&self, path: impl Into<Arc<str>>) -> ImageLoad {
        let source = ImageSource::Asset(path.into());
        self.load_decoded(source, read_in_chunks)
    }

    /// Fetches and decodes an image over HTTP/HTTPS via
    /// [`NetworkLoader`](crate::loaders::NetworkLoader), through the same
    /// decoded-image cache, runtime selection and cancellation as
    /// [`load_image_bridged`](Self::load_image_bridged).
    ///
    /// Requires both the `images` (decode) and `network` (HTTP client)
    /// features.
    /// Requests share this registry's lazily initialized HTTP connection pool.
    /// Client initialization errors are returned and a later load may retry.
    /// Dropping the last waiter aborts the request, closing its connection.
    ///
    /// # Errors
    ///
    /// Returns [`AssetError::LoadFailed`]/[`AssetError::NetworkError`] on a
    /// failed request or a failed decode of the response body, or if the
    /// loading task is dropped before completing — see
    /// [`load_image_bridged`](Self::load_image_bridged)'s `# Errors` for the
    /// same contract.
    #[cfg(all(feature = "images", feature = "network"))]
    pub fn load_network_image_bridged(&self, url: impl Into<Arc<str>>) -> ImageLoad {
        let source = ImageSource::Network(url.into());
        let network_loader = Arc::clone(&self.network_loader);
        self.load_decoded(source, move |url| async move {
            let loader = network_loader
                .get_or_try_init(|| async {
                    crate::NetworkLoader::new().map_err(|error| AssetError::LoadFailed {
                        path: url.to_string(),
                        reason: format!("HTTP client initialization failed: {error}"),
                    })
                })
                .await?;
            loader.load_url(&url).await
        })
    }

    /// The completed decoded image this registry keeps for `source`, made most
    /// recently used — the synchronous probe a frame makes before starting a
    /// bridged load.
    #[cfg(feature = "images")]
    #[must_use]
    pub fn cached_image(&self, source: &ImageSource) -> Option<crate::Image> {
        self.decoded_images.cached(source)
    }

    /// Serves `source` from the decoded-image cache, joins its load in flight,
    /// or spawns a task that reads it with `fetch` and decodes it.
    #[cfg(feature = "images")]
    fn load_decoded<F, Fut>(&self, source: ImageSource, fetch: F) -> ImageLoad
    where
        F: FnOnce(Arc<str>) -> Fut,
        Fut: std::future::Future<Output = Result<Vec<u8>>> + Send + 'static,
    {
        let (load, ticket) = match self.decoded_images.admit(source.clone()) {
            images::Admission::Served(load) => return load,
            images::Admission::Start(load, ticket) => (load, ticket),
        };
        let completion =
            images::Completion::new(Arc::clone(&self.decoded_images), source.clone(), ticket);
        let handle = match self
            .bridge_runtime
            .resolve(self.injected_runtime_handle.as_ref())
        {
            Ok(handle) => handle,
            Err(error) => {
                completion.settle(Some(Err(error)));
                return load;
            }
        };
        let bytes = fetch(Arc::clone(source.label()));
        let ticket = Arc::clone(completion.ticket());
        // Spawned with no registry lock held: a runtime that has shut down
        // drops the task inside `spawn`, and the completion's drop then
        // settles the entry on this thread.
        let task = handle.spawn(async move {
            // Taken before the read starts and held until the decode returns:
            // reads past admission, and the encoded bytes waiting to decode,
            // are bounded by count.
            let _load = completion.images().admit_load(completion.ticket()).await;
            let outcome = match bytes.await {
                Ok(bytes) => {
                    completion
                        .images()
                        .decode(completion.source(), completion.ticket(), bytes)
                        .await
                }
                Err(error) => Some(Err(error)),
            };
            completion.settle(outcome);
        });
        self.decoded_images
            .attach(&source, &ticket, task.abort_handle());
        load
    }

    /// Loads an asset, using the cache if available.
    ///
    /// Validates each supplied asset before consulting the cache. A rejected
    /// descriptor returns its validation error even on a cache hit, without
    /// loading or changing previously cached data.
    /// If the asset is already cached, returns the cached version immediately.
    /// Built-in [`crate::FontAsset`] loads share same-key pending work.
    /// Images and custom assets run independently on a cold miss, including reentry through
    /// spawned tasks. Each success publishes; later completion may replace earlier
    /// data without invalidating its handles. Exactly-once side effects are not
    /// guaranteed. Errors are not cached. Cancelling one custom load does not
    /// cancel another; later calls can retry. Built-in waiters may restart after
    /// elected cancellation or panic, subject to Moka's finite retry policy.
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

        cache.load(key, &asset).await
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

    /// Detaches all typed caches before retiring their ownership. Generic data
    /// destructors may reenter this registry and create fresh caches.
    /// In-flight loads can still return handles from their detached cache.
    #[expect(
        clippy::unused_async,
        reason = "public API: uniform async surface with the genuinely-async `invalidate`/`clear` siblings"
    )]
    pub async fn clear_all(&self) {
        self.retire_caches();
    }

    /// Detaches every typed cache and drops it with no lock held.
    fn retire_caches(&self) {
        let retired = {
            let mut caches = self.caches.write();
            std::mem::take(&mut *caches)
        };
        drop(retired);
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
        let cache = AssetCache::<T>::with_config(self.cache_config);
        caches.insert(type_id, Box::new(cache.clone()));
        cache
    }
}

/// Bytes one blocking read takes of an image file: an abandoned load stops
/// reading at the next chunk instead of holding a blocking thread for the
/// whole file.
#[cfg(feature = "images")]
const FILE_READ_CHUNK: usize = 256 * 1024;

/// The largest encoded image file a bridged load reads: 256 MiB, the same
/// bound `NetworkLoader::load_url` puts on a response body. A larger file
/// fails before it is buffered.
#[cfg(feature = "images")]
const MAX_ENCODED_FILE_BYTES: u64 = 256 * 1024 * 1024;

/// Reads the file at `path` one [`FILE_READ_CHUNK`] per blocking read, so the
/// future can be dropped between chunks, and never more than
/// [`MAX_ENCODED_FILE_BYTES`].
///
/// `tokio::fs::read` runs the whole `std::fs::read` as one blocking call,
/// which an abort cannot stop.
#[cfg(feature = "images")]
async fn read_in_chunks(path: Arc<str>) -> Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;

    let failed = |error: std::io::Error| AssetError::LoadFailed {
        path: path.to_string(),
        reason: error.to_string(),
    };
    let too_large = || AssetError::LoadFailed {
        path: path.to_string(),
        reason: format!("the file is larger than {MAX_ENCODED_FILE_BYTES} bytes"),
    };
    let mut file = tokio::fs::File::open(path.as_ref()).await.map_err(failed)?;
    let length = file.metadata().await.map_err(failed)?.len();
    if length > MAX_ENCODED_FILE_BYTES {
        return Err(too_large());
    }
    file.set_max_buf_size(FILE_READ_CHUNK);
    // The metadata length is a hint, not a bound: the file can grow while it
    // is read, so the read itself stops one byte past the cap.
    let mut bytes = Vec::with_capacity(usize::try_from(length).unwrap_or(0));
    file.take(MAX_ENCODED_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(failed)?;
    if bytes.len() as u64 > MAX_ENCODED_FILE_BYTES {
        return Err(too_large());
    }
    Ok(bytes)
}

/// Under memory pressure the registry gives back every completed typed-cache
/// entry, and decoded images by level.
///
/// A typed asset reloads from its source on the next request, and there is no
/// cheaper half-way to give back, so each level drops the same entries.
/// Decoded images (`images` feature) are kept by recency: moderate pressure
/// trims them to half the decoded-image budget
/// (`AssetRegistryBuilder::with_decoded_image_budget`), critical pressure drops
/// them all. Handles and images already given out keep their data, and loads
/// in flight finish into the caches they started with.
impl ReclaimMemory for AssetRegistry {
    fn reclaim(&self, level: MemoryPressureLevel) {
        self.retire_caches();
        #[cfg(feature = "images")]
        self.decoded_images.reclaim(level);
        #[cfg(not(feature = "images"))]
        let _ = level;
    }
}

impl Default for AssetRegistry {
    fn default() -> Self {
        Self::new(AssetCacheConfig::default())
    }
}

// ===== Type State Builder Pattern =====

/// Type-state marker: Capacity not yet set.
#[derive(Debug, Clone, Copy)]
pub struct NoCapacity;

/// Type-state marker: Capacity has been set.
#[derive(Debug, Clone, Copy)]
pub struct HasCapacity(pub(crate) AssetCacheConfig);

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
///     .with_capacity(flui_assets::CacheCapacity::Entries(std::num::NonZeroU64::new(1_024).expect("nonzero capacity"))) // entry count
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
///     .with_default_capacity() // entry count
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
    /// Set via [`with_decoded_image_budget`](Self::with_decoded_image_budget)
    /// and [`with_decode_threads`](Self::with_decode_threads).
    #[cfg(feature = "images")]
    images: ImageConfig,
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
            #[cfg(feature = "images")]
            images: ImageConfig::default(),
            #[cfg(all(feature = "images", feature = "network"))]
            network_loader: None,
        }
    }

    /// Sets completed-entry retention separately for each asset type.
    ///
    /// This capacity is used for each asset type's cache.
    ///
    /// # Arguments
    ///
    /// * `capacity` - An explicit count bound or disabled retention.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let registry = AssetRegistryBuilder::new()
    ///     .with_capacity(flui_assets::CacheCapacity::Entries(std::num::NonZeroU64::new(1_024).expect("nonzero capacity"))) // entry count
    ///     .build();
    /// ```
    pub fn with_capacity(self, capacity: CacheCapacity) -> AssetRegistryBuilder<HasCapacity> {
        self.with_cache_config(AssetCacheConfig {
            capacity,
            ..AssetCacheConfig::default()
        })
    }

    /// Configures count capacity and expiration for every typed cache.
    pub fn with_cache_config(self, config: AssetCacheConfig) -> AssetRegistryBuilder<HasCapacity> {
        AssetRegistryBuilder {
            capacity: HasCapacity(config),
            #[cfg(feature = "images")]
            runtime_handle: self.runtime_handle,
            #[cfg(feature = "images")]
            images: self.images,
            #[cfg(all(feature = "images", feature = "network"))]
            network_loader: self.network_loader,
        }
    }

    /// Sets the default capacity (10,240 entries per asset type).
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
        self.with_cache_config(AssetCacheConfig::default())
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
    ///     .with_capacity(flui_assets::CacheCapacity::Entries(std::num::NonZeroU64::new(1_024).expect("nonzero capacity")))
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

    /// Bounds the decoded images bridged loads keep, in pixel bytes
    /// (`width * height * 4` each).
    ///
    /// The least recently used images go first once the total passes `bytes`;
    /// an image larger than `bytes` is handed to its waiters and not kept, and
    /// `0` keeps none. Loads in flight are still shared. The default is 256 MiB,
    /// and 64 MiB on Android, iOS and the web.
    #[cfg(feature = "images")]
    #[must_use]
    pub fn with_decoded_image_budget(mut self, bytes: usize) -> Self {
        self.images.budget = bytes;
        self
    }

    /// Bounds how many bridged images decode at once.
    ///
    /// Decodes run on the loading runtime's blocking pool, never on its async
    /// workers. The default is half the available cores, at most four.
    #[cfg(feature = "images")]
    #[must_use]
    pub fn with_decode_threads(mut self, threads: std::num::NonZeroUsize) -> Self {
        self.images.decode_threads = threads;
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
    ///     .with_capacity(flui_assets::CacheCapacity::Entries(std::num::NonZeroU64::new(1_024).expect("nonzero capacity")))
    ///     .build();
    /// ```
    pub fn build(self) -> AssetRegistry {
        #[cfg(feature = "images")]
        {
            AssetRegistry::with_images(
                self.capacity.0,
                self.runtime_handle,
                self.images,
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
    ///     .with_capacity(flui_assets::CacheCapacity::Entries(std::num::NonZeroU64::new(1_024).expect("nonzero capacity")))
    ///     .with_capacity(flui_assets::CacheCapacity::Entries(std::num::NonZeroU64::new(1_024).expect("nonzero capacity"))) // Override previous value
    ///     .build();
    /// ```
    pub fn with_capacity(self, capacity: CacheCapacity) -> AssetRegistryBuilder<HasCapacity> {
        let mut config = self.capacity.0;
        config.capacity = capacity;
        self.with_cache_config(config)
    }

    /// Replaces the typed-cache capacity and expiration configuration.
    pub fn with_cache_config(self, config: AssetCacheConfig) -> AssetRegistryBuilder<HasCapacity> {
        AssetRegistryBuilder {
            capacity: HasCapacity(config),
            #[cfg(feature = "images")]
            runtime_handle: self.runtime_handle,
            #[cfg(feature = "images")]
            images: self.images,
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
