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
    default_capacity: usize,
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
- Better hit rates than LRU (admission policy)
- Concurrent cache access; statistics updates take a write lock
- Real-time statistics

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

**Purpose**: Efficient string-based identifiers.

**Key Features**:
- String interning with `lasso`
- Only 4 bytes per key
- O(1) comparison and hashing
- Global interner (thread-safe)

**Why String Interning?**
```rust
// Without interning:
let key1 = "textures/grass.png".to_string(); // 24+ bytes
let key2 = "textures/grass.png".to_string(); // 24+ bytes
assert_ne!(key1.as_ptr(), key2.as_ptr());    // Different allocations

// With interning:
let key1 = AssetKey::new("textures/grass.png"); // 4 bytes
let key2 = AssetKey::new("textures/grass.png"); // 4 bytes
assert_eq!(key1, key2);                          // Same Spur value
```

## Data Flow

### Loading an Asset

```
1. User calls: registry.load(FontAsset::file("font.ttf"))
                      │
2. Registry extracts TypeId of FontAsset
                      │
3. Get or create AssetCache<FontAsset>
                      │
4. Generate AssetKey from path ("font.ttf" → Spur(42))
                      │
5. Check cache with key
   ├─ Cache HIT ──→ Return existing Arc<FontData>
   │                       │
   └─ Cache MISS ──→ Call asset.load()
                           │
                     Load from filesystem
                           │
                     Create Arc<FontData>
                           │
                     Insert into cache
                           │
                     Return Arc<FontData>
```

### Cache Eviction

TinyLFU admission policy:
```
New asset arrives
      │
Is cache full?
   ├─ NO ──→ Insert immediately
   │
   └─ YES ──→ Admission policy
                    │
              Compare frequency:
              new_freq vs victim_freq
                    │
              ├─ new_freq > victim_freq ──→ Evict victim, insert new
              └─ new_freq ≤ victim_freq ──→ Reject new asset
```

## Capacity and synchronization

`AssetCache::new(capacity_bytes)` converts its byte hint to an estimated entry
count, assuming 10 KiB per entry and retaining a minimum of 100 entries.
`with_config` configures Moka by entry count. Neither constructor weighs decoded
assets or bounds their actual byte footprint. Handles held outside the cache
retain their data after eviction; the public
`non_clone_data_retains_evicted_handles_across_reload` case pins that ownership.

| Component | Synchronization |
|-----------|-----------------|
| Registry cache map | `parking_lot::RwLock`; typed handles leave the lock before loading |
| Typed cache | Moka's concurrent cache |
| Statistics | `parking_lot::RwLock`, written on hits, misses and mutations |
| Key interner | Lasso's `ThreadedRodeo` |

Cache lookup, admission, eviction and string interning use their dependencies'
implementations. IO and decoding costs depend on the source and data; no timing
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
            registry.load(FontAsset::file(&format!("font{}.ttf", i))).await
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
pub trait Asset {
    type Data: Send + Sync + 'static;
    type Key: Hash + Eq + Clone;
    type Error: Error + Send + Sync + 'static;

    fn key(&self) -> Self::Key;
    async fn load(&self) -> Result<Self::Data, Self::Error>;
    fn metadata(&self) -> Option<AssetMetadata> { None }
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
pub struct HasCapacity(usize);

// Builder with state
pub struct AssetRegistryBuilder<C = NoCapacity> {
    capacity: C,
}

// Initial state - cannot build
impl AssetRegistryBuilder<NoCapacity> {
    pub fn new() -> Self;
    pub fn with_capacity(self, capacity: usize) -> AssetRegistryBuilder<HasCapacity>;
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

## Comparison with Alternatives

### vs Manual HashMap

| Feature | flui_assets | Manual HashMap |
|---------|-------------|----------------|
| Type safety | ✅ Compile-time | ❌ Runtime casts |
| Eviction | ✅ Automatic (TinyLFU) | ❌ Manual |
| Thread safety | ✅ Built-in | ❌ Manual locking |
| Statistics | ✅ Built-in | ❌ Manual tracking |
| Memory efficiency | ✅ 4-byte keys | ❌ 24+ byte strings |

### vs bevy_asset

| Feature | flui_assets | bevy_asset |
|---------|-------------|------------|
| Dependencies | ✅ Minimal | ❌ Heavy (ECS) |
| Simplicity | ✅ Simple API | ⚠️ Complex |
| Performance | ✅ TinyLFU cache | ✅ Similar |
| Flexibility | ✅ Easy extension | ⚠️ ECS-coupled |

## Mapping decisions

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

## References

- [Moka Cache Documentation](https://docs.rs/moka)
- [TinyLFU Paper](https://arxiv.org/abs/1512.00727)
- [Lasso String Interning](https://docs.rs/lasso)
- [parking_lot Performance](https://github.com/Amanieu/parking_lot#performance)

### Concurrent registry misses share maintained cache initialization

`AssetRegistry::load` validates every descriptor before looking up its typed
cache. Concurrent cold requests for that key use Moka's `try_get_with`, sharing
one loaded allocation or initialization error. Errors are cloned back into the
existing owned `AssetError` result and are not cached. A cancelled initializing
future releases Moka's waiters; a remaining accepted descriptor can restart the
load. The generic public cache convenience helper keeps its existing error API.

Statistics count each initial cold probe as a miss and a successful initializer
as one insertion; waiting on another request is not another insertion. The
public `cold_registry_loads_share_work_and_recover` family checks shared
allocation identity, shared failure followed by retry, and waiter progress after
initializer cancellation. This coalesces ordinary registry loads; bridged image
loads retain the widget decode cache's subscriber-lifetime contract.
