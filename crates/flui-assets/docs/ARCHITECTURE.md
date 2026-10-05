# flui_assets Architecture

## Overview

`flui_assets` is a high-performance asset management system built on three core principles:
1. **Type Safety** - Generic traits ensure compile-time correctness
2. **Caching** - Moka admission and eviction, with shared ownership of decoded data
3. **Extensibility** - Easy to add custom asset types

## Three-Layer Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Application Layer                       │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐                 │
│  │  Fonts   │  │  Images  │  │  Custom  │                 │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘                 │
└───────┼─────────────┼─────────────┼────────────────────────┘
        │             │             │
        └─────────────┴─────────────┘
                      │
┌─────────────────────▼─────────────────────────────────────┐
│              AssetRegistry (per app)                      │
│  • Type-erased storage (TypeId → Box<dyn Any>)           │
│  • Automatic cache creation per asset type                │
│  • Thread-safe with parking_lot::RwLock                   │
└─────────────────────┬─────────────────────────────────────┘
                      │
        ┌─────────────┴─────────────┐
        │                           │
┌───────▼────────┐         ┌────────▼───────┐
│ AssetCache<T>  │         │ AssetCache<T>  │
│  (FontAsset)   │         │  (ImageAsset)  │
├────────────────┤         ├────────────────┤
│ • Moka cache   │         │ • Moka cache   │
│ • TinyLFU      │         │ • TinyLFU      │
│ • Stats        │         │ • Stats        │
└───────┬────────┘         └────────┬───────┘
        │                           │
        └─────────────┬─────────────┘
                      │
┌─────────────────────▼─────────────────────────────────────┐
│             AssetHandle<T, K> (Arc)                        │
│  • Shared data and a caller-selected key                  │
│  • Weak references for cache-friendly patterns            │
│  • Extension traits for convenience                       │
└───────────────────────────────────────────────────────────┘
```

## Core Components

### 1. AssetRegistry

**Purpose**: The entry point for asset loading and management. An application builds one
with `AssetRegistryBuilder` and owns it; there is no process-wide instance.

**Key Features**:
- Type-erased storage using `TypeId`
- Lazy cache creation (only when first asset of type is loaded)
- Thread-safe concurrent access

**Implementation**:
```rust
pub struct AssetRegistry {
    // TypeId -> Box<dyn Any> where Any is AssetCache<T>
    caches: Arc<RwLock<HashMap<TypeId, Box<dyn Any + Send + Sync>>>>,
    cache_config: AssetCacheConfig,
}
```

**Trade-offs**:
- ✅ Type erasure allows storing different cache types
- Cache lookup and downcasting check the requested type at runtime.
- A typed cache handle is cloned before asynchronous loading; the registry lock
  does not span that load.

### 2. AssetCache<T>

**Purpose**: Type-specific caching with automatic eviction.

**Key Features**:
- Built on `moka` with TinyLFU eviction algorithm
- Frequency and recency inform TinyLFU admission
- Concurrent cache access; statistics updates take a write lock
- Shared best-effort operation counters

**Implementation**:
```rust
pub struct AssetCache<T: Asset> {
    cache: Cache<T::Key, Arc<T::Data>>,
    stats: Arc<parking_lot::RwLock<CacheStats>>,
}
```

**Why TinyLFU?**
- Considers both frequency and recency
- Admission and eviction are delegated to Moka; this crate does not implement
  a replacement policy or claim a measured advantage over another policy.

### 3. AssetHandle<T, K>

**Purpose**: Smart pointer to cached asset data.

**Key Features**:
- Arc-based sharing (cheap clone)
- Weak references for cache-aware code
- Extension traits for convenience methods
- A handle owns an `Arc<T>` and its generic key; neither its byte size nor the
  cache entry layout is an ABI contract.

**Implementation**:
```rust
pub struct AssetHandle<T, K> {
    inner: Arc<T>,
    key: K,
}
```

**Design Pattern**: Handle-Body idiom
- Cloning a handle shares its decoded data and clones its key
- Body is the actual data (potentially large)
- Multiple handles can point to same data

### 4. AssetKey

**Purpose**: Nonempty names with shared ownership independent of a registry.

`AssetKey` owns an `Arc<str>`. Cloning shares its string allocation; independently
constructed names compare and hash by contents. There is no global interner,
integer namespace or retaining arena. `as_str()` borrows from the key, so callers
must keep an owner alive rather than rely on a static string. Keys are Clone,
not Copy; no fixed representation size or constant-time string hashing is promised.

```rust
use flui_assets::AssetKey;
use std::sync::Arc;

let name: Arc<str> = Arc::from("textures/grass.png");
let first = AssetKey::from(Arc::clone(&name));
let shared = first.clone();
let independent = AssetKey::new("textures/grass.png");
assert_eq!(first, shared);
assert_eq!(first, independent);
```

`FontAsset` and `ImageAsset` constructors accept `Into<Arc<str>>`. They retain the
name, and `key()` clones its existing storage. Pass `path.as_str()` for a borrowed
`String`, or pass owned `String`, `&str` or `Arc<str>` directly. Empty `AssetKey`
construction remains rejected. A weak data handle still owns its generic key,
so its name remains live until that handle is released too.

## Data Flow

### Loading an Asset

1. The caller supplies a typed descriptor to `registry.load`.
2. The registry validates it before any cache lookup or loading.
3. Its key and asset `TypeId` select a typed cache handle, cloned outside the
   registry map guard before asynchronous operations begin.
4. A retained hit shares its existing `Arc<Data>`. Concurrent cold requests use
   Moka's entry selector to share one initializer or failure.
5. Successful loading supplies an `Arc<Data>` to Moka and the consumer handle.
   The capacity policy may subsequently evict cache ownership; consumer
   ownership remains independent. Errors are not retained.

### Cache Eviction

Admission and capacity eviction are delegated to Moka's TinyLFU policy. Its
frequency and recency bookkeeping are not a strict LRU contract, and weighted
asset sizes are not configured. Pending maintenance applies the entry bound;
returned consumer handles retain ownership independently of admission decisions.

## Capacity and synchronization

`AssetCache::new` accepts `CacheCapacity::Entries(NonZeroU64)` or
`CacheCapacity::Disabled`. `AssetCacheConfig` independently configures lifetime
and idle expiration through validated `CacheExpiration` values. Defaults are
10,240 completed entries per asset type, a five-minute lifetime and a one-minute
idle interval. Neither entry count nor expiration bounds decoded bytes or memory
retained by consumer handles.

`len()` reports Moka's estimated `u64` entry count. `sync().await` improves
accuracy after quiescence without becoming a concurrent snapshot or physical
retirement barrier. Utilization uses the configured count capacity and clamps
maintenance lag to one; disabled retention reports zero. The public
`asset_cache_retention_and_observation_contracts` family checks count eviction,
disabled retention, held ownership, finite diagnostics and supported expiration.

| Component | Synchronization |
|-----------|-----------------|
| Registry cache map | `parking_lot::RwLock`; typed handles leave the lock before loading |
| Typed cache | Moka's concurrent cache |
| Statistics | `parking_lot::RwLock`, written on hits, misses and mutations |
| Asset names | Standard-library `Arc<str>` shared ownership |

Cache lookup, admission and eviction use Moka's implementation; asset names use
standard-library shared string ownership. IO and decoding costs depend on the source and data; no timing
or hit-rate measurements are recorded here. `AssetCache::stats` reports its
typed cache's counters. The registry exposes no aggregated statistics API; its
former method returned an empty vector without inspecting caches.

### Concurrent Access Patterns

**Read-heavy workload** (typical):
```rust
// Multiple threads can load simultaneously. `registry` is an `Arc<AssetRegistry>`:
// each spawned task needs an owned handle to the one registry.
let handles: Vec<_> = (0..10)
    .map(|i| {
        let registry = registry.clone();
        tokio::spawn(async move {
            registry.load(FontAsset::file(format!("font{}.ttf", i))).await
        })
    })
    .collect();
```

Registry-map writes create typed caches. Existing-cache lookups still acquire
the registry read lock, and cache operations update statistics under their own
write lock.

## Extension Mechanisms

### Custom Asset Types

Implement `Asset` trait:
```rust
use std::{error::Error, future::Future, hash::Hash};
use flui_assets::AssetMetadata;

pub trait Asset: Send + Sync + 'static {
    type Data: Send + Sync;
    type Key: Hash + Eq + Clone + Send + Sync;
    type Error: Error + Send + Sync + 'static;

    fn key(&self) -> Self::Key;
    fn load(&self) -> impl Future<Output = Result<Self::Data, Self::Error>> + Send;
    fn metadata(&self) -> Option<AssetMetadata> { None }
    fn validate(&self) -> Result<(), Self::Error> { Ok(()) }
}
```

**Requirements**:
- `Data` must be `Send + Sync` (thread-safe)
- `Key` must be hashable and comparable
- `load()` is async for non-blocking I/O

### Byte sources and decoding

`Asset::load` owns source selection and decoding. `BytesFileLoader` reads real
file bytes for `FontAsset` and `ImageAsset`; `NetworkLoader` fetches bytes for the
network image bridge. Embedded assets own their bytes through `from_bytes`.
There is no separate generic loader trait: a key alone cannot specify how to
construct an arbitrary asset's decoded data.

Image types are available with `images`, and network operations with `network`.
Unavailable operations are rejected by compilation, rather than reading data
before returning a feature-disabled error. See
[ADR-0107](../../../docs/adr/ADR-0107-asset-byte-sources-and-decoding.md).

## Design Patterns

### 1. Extension Trait Pattern

**Problem**: Don't want to bloat core APIs with convenience methods.

**Solution**: Sealed core trait + blanket extension trait.

```rust
// Core trait (sealed)
pub trait AssetHandleCore<T, K>: sealed::Sealed {
    fn get(&self) -> &T;
    fn key(&self) -> &K;
}

// Extension trait (convenience)
pub trait AssetHandleExt<T, K>: AssetHandleCore<T, K> {
    fn is_unique(&self) -> bool { self.strong_count() == 1 }
    fn map<U, F>(&self, f: F) -> U where F: FnOnce(&T) -> U { f(self.get()) }
}

// Blanket implementation
impl<H, T, K> AssetHandleExt<T, K> for H where H: AssetHandleCore<T, K> {}
```

**Benefits**:
- Core API stays minimal
- Users get convenience methods automatically
- Easy to add new methods without breaking changes

### 2. Type State Builder Pattern

**Problem**: Want compile-time validation that capacity is set.

**Solution**: Type states with conditional methods.

```rust
// Type states
pub struct NoCapacity;
pub struct HasCapacity(AssetCacheConfig);

// Builder with state
pub struct AssetRegistryBuilder<C = NoCapacity> {
    capacity: C,
}

// Initial state - cannot build
impl AssetRegistryBuilder<NoCapacity> {
    pub fn new() -> Self;
    pub fn with_capacity(self, capacity: CacheCapacity) -> AssetRegistryBuilder<HasCapacity>;
    pub fn with_cache_config(self, config: AssetCacheConfig) -> AssetRegistryBuilder<HasCapacity>;
}

// Final state - can build
impl AssetRegistryBuilder<HasCapacity> {
    pub fn build(self) -> AssetRegistry;
}
```

**Benefits**:
- Compile-time enforcement
- Clear API progression
- Zero runtime overhead

## Future Optimizations

### 1. Memory-Mapped Fonts
```toml
[features]
mmap-fonts = ["memmap2"]
```

**Benefit**: Reduce memory usage by sharing font data across processes.

### 2. Parallel Decoding
```toml
[features]
parallel-decode = ["rayon"]
```

**Benefit**: Decode multiple images/videos simultaneously using thread pool.

### 3. Hot Reload
```toml
[features]
hot-reload = ["notify"]
```

**Benefit**: Automatically reload assets when files change (development mode).

## Mapping decisions

- Asset keys own nonempty shared strings rather than borrowing from a process-wide
  interner. Equality and hashing use contents across independently constructed
  keys; cloning shares string storage. Built-in font and image descriptors retain
  the same name storage that their keys and consumer handles use. Dropping a
  registry does not invalidate a live consumer's name, and dropping the final
  descriptor, key and handle releases that name independently of unrelated names.
  Weak asset handles still own their keys strongly while holding only weak data
  references. `asset_key_names_follow_consumer_ownership` exercises independent
  equal-key lookup, sharing, registry release, retained names and final reclamation
  through public font/image descriptors and handles. The compile-fail doctest on
  `AssetKey::as_str` rejects borrowing a name beyond the owning key's lifetime.
  ADR-0121 records this ownership contract.

- Default network image loads share a lazily initialized HTTP client per registry,
  constructed on the loading runtime. Initialization returns a typed error and
  remains retryable. `NetworkLoader::new` is fallible rather than hiding external
  initialization failures in a panic; `Default` is not a construction contract.
  Hosts supply a client builder that creates a fresh configured pool, preserving
  its HTTP policy through capacity transitions without changing runtime selection.
  `network_bridge_reuses_connections_and_recovers_after_decode_errors` checks
  real connection reuse after invalid image bytes, host injection, independent
  registry pool ownership and replacement of an ambient runtime. Fresh configured
  pools cannot import another runtime's existing connection drivers (ADR-0118).
- Network bytes are transferred into their owned vector through `Bytes`' consuming
  conversion, allowing buffer reuse where its ownership permits. Text remains
  strict UTF-8, preserving a BOM and ignoring charset replacement decoding. HTTP
  admission remains limited to 2xx; `error_for_status` would also admit 3xx.
  `network_loader_preserves_transport_and_text_contracts_after_failures` exercises
  HTTP status errors, redirects, malformed text, truncated bodies, a configured
  body deadline and recovery through the public loader.

- Decoded data need not implement `Clone`. Registry caches and cloned strong/weak
  handles share `Arc` ownership, while the opt-in `clone_data` operation requires
  `Clone`. `non_clone_data_retains_evicted_handles_across_reload`
  loads a non-Clone value, shares cache hits and handles, evicts it while live
  handles retain it, reloads the evicted key, and observes destruction after
  consumer handles and the owning registry are released. Cache invalidation
  excludes future lookups; Moka's deferred retirement is not an immediate physical
  deallocation guarantee.
  `AssetHandle::ptr_eq` is an inherent operation using `Arc::ptr_eq`; the same
  lifecycle test keeps an evicted value alive while reloading its equal key and
  verifies that the old clone shares ownership while the reloaded value does not.
  The extension trait no longer substitutes key equality for allocation identity.

- Byte-source selection and decoding belong to `Asset::load`. File-backed image
  and font assets use `BytesFileLoader`; embedded constructors own their source
  bytes, and the network bridge uses `NetworkLoader` before image decoding.
  `font_sources_preserve_bytes_and_recover_after_load_errors` and
  `image_asset_file_loads_a_committed_png_fixture_to_its_real_dimensions` pin
  file/embedded equivalence, invalid sources, missing sources and recovery.
  [ADR-0107](../../../docs/adr/ADR-0107-asset-byte-sources-and-decoding.md) records
  removal of the unsupported generic loader abstraction and feature-gated types.

- Registry admission validates each descriptor before cache lookup or loading,
  including cache hits. Rejection preserves previously accepted cached data.
  `validation_precedes_loading_and_cache_hits_without_poisoning_accepted_data`
  pins rejection, cache preservation and the next accepted request.
- The image bridge selects ambient multi-thread runtimes, skips ambient
  current-thread runtimes, and honors explicit host injection. Owned runtime
  creation returns typed errors and can retry after failure; ownership remains
  registry-local. `load_image_bridged_completes_both_the_success_and_the_failure_path`
  exercises real decoding and recovery while an ambient current-thread runtime
  is entered but undriven. `the_bridge_resolves_a_live_runtime_and_survives_its_own_teardown`
  includes the private runtime-construction failure seam, because OS resource
  exhaustion cannot be induced reliably through the public API.
  [ADR-0105](../../../docs/adr/ADR-0105-asset-validation-and-bridge-progress.md)
  records the host and asset contracts.

### Public cold initialization shares Moka entries and failures

Initializer ancestry belongs to each typed cache and is shared by its clones.
The scope is entered while polling or retiring user work and is retired on
pending, completion or unwind. Same-key reentry through a clone first uses a
completed entry if available; otherwise it runs the nested initializer directly
and returns an uncached handle. The outer initializer owns publication. This
prevents waiting on the entry that the caller itself must complete, including
after suspension. Other keys and cache instances remain independent, and
ordinary concurrent requests still coalesce. The ancestry tracker's lock never
spans key comparison, future polling or retirement. Separately spawned dependency
cycles are outside this ancestry contract. The reentry row in
`cold_registry_loads_share_work_and_recover` pins progress, outer publication,
independent cache/key behavior, nested failure and the next successful request.
The retirement row checks completion, cancellation of elected work and disposal
of an unselected waiter's captures. The ancestry key is owned outside Moka's
initializer future so its destructor runs after the backend waiter retires.

[ADR-0120](../../../docs/adr/ADR-0120-typed-asset-cache-retention.md)
records the cache configuration and shared-initialization contract.

`AssetRegistry::load` validates every descriptor before looking up its typed
cache. Both registry loading and the public `AssetCache::get_or_insert_with`
helper use Moka's borrowed-key entry selector with `or_try_insert_with`, sharing
one loaded allocation or `Arc<Error>`. Custom errors need not implement `Clone`.
Registry loads clone the shared `AssetError` into their owned error result.
Errors are not cached. A cancelled initializer releases Moka's waiters; a
remaining accepted descriptor can restart the load. An initializer panic reaches
its caller while a waiter can retry. Repeated cancellation and panic retries
remain subject to Moka's finite retry limit.

Hit/miss statistics record each initial presence observation, which can race
concurrent writes. Only a fresh returned entry counts as a completed insertion;
a cold waiter is not another insertion. Cancellation after backend publication
can leave an entry without a completed insertion count. The public
`cold_registry_loads_share_work_and_recover` family checks registry allocation
sharing, shared errors and cancellation recovery.
`public_cache_waiters_share_data_and_non_clone_errors` and
`public_cache_waiters_recover_after_cancellation_or_panic` check those contracts
through the public cache helper.

This coalesces ordinary registry loads, including decoded image results. Bridged
image loads bypass the typed cache and retain the widget LRU's synchronous
frame-path probe and subscriber-lifetime contract.

### Observation and invalidation have distinct effects

`contains` delegates to Moka's synchronous presence observation without cloning
data, counting requests, updating popularity or refreshing idle expiration.
Retrieving operations still record reads. `insert_many` inserts sequentially and
returns handles in input order.

`invalidations` counts explicit invalidation requests, including absent keys;
it does not count automatic capacity or expiration removals. Counters saturate,
and rate arithmetic remains finite at their boundary. Clearing a typed cache
invalidates completed entries and resets counters after maintenance. Neither
invalidation nor clearing cancels in-flight initialization, which may publish
afterward; these operations do not define a generation barrier.

### Registry retirement commits ownership before user destruction

`clear_all` detaches the cache map while guarded, then retires the outgoing map
after releasing the registry guard. Generic loaded data can reenter the same
registry during destruction and observe the committed empty map. Releasing the
last registry owner gives weak references an absent-owner fallback. The public
`registry_cache_retirement_commits_ownership_before_data_drop` family covers
clear-time reentry, subsequent healthy loading and last-owner release. This
ownership policy does not promise containment of arbitrary aggregate destructor
panics.

### Cache clones observe one cache and its counters

`AssetCache::clone` shares Moka entries and the statistics allocation. Registry
loads clone the typed cache before leaving the registry lock; these temporary
handles must not reset counters or allocate a new statistics lock per lookup.
`cache_clones_report_shared_operations_and_reset` in `tests/load_contract.rs`
checks real insertion, lookup, invalidation and clearing through different
handles, alongside their shared operation counts and statistics reset.

## References

- [Moka Cache Documentation](https://docs.rs/moka)
- [TinyLFU Paper](https://arxiv.org/abs/1512.00727)
- [Standard-library shared ownership](https://doc.rust-lang.org/std/sync/struct.Arc.html)
- [parking_lot Performance](https://github.com/Amanieu/parking_lot#performance)
