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
4. A retained hit shares its existing `Arc<Data>`. Concurrent cold font requests
   through the registry use Moka's entry selector to share one initializer or
   failure. Images, custom assets and public cache initializers run independently.
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
file bytes for `FontAsset` and `ImageAsset`; the caller's HTTP client fetches bytes for
network images. Embedded assets own their bytes through `from_bytes`.
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

- A network image is fetched with the HTTP client its caller passes in
  `LoadServices` (the host's `HttpCapability`, ADR-0280); this crate links no
  HTTP stack and keeps no connection pool. The client drives the request, so the
  lane a load runs on needs no async runtime, and a load with no lanes fetches
  where it is polled. A load without a client answers `AssetError::HttpUnavailable`,
  a failed request `AssetError::Http` and a status other than 2xx
  `AssetError::HttpStatus`; none is kept, so a later request retries. Each request
  limits its body to 256 MiB. `network_loads_fetch_with_the_callers_client_and_recover_after_errors`
  covers recovery after invalid image bytes, no lanes, an error status, a failed
  request, no client and a load handed over to the next request's client.

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
  bytes, and network image loads fetch with the caller's HTTP client before decoding.
  `font_sources_preserve_bytes_and_recover_after_load_errors` and
  `image_asset_file_loads_a_committed_png_fixture_to_its_real_dimensions` pin
  file/embedded equivalence, invalid sources, missing sources and recovery.
  [ADR-0107](../../../docs/adr/ADR-0107-asset-byte-sources-and-decoding.md) records
  removal of the unsupported generic loader abstraction and feature-gated types.

- Registry admission validates each descriptor before cache lookup or loading,
  including cache hits. Rejection preserves previously accepted cached data.
  `validation_precedes_loading_and_cache_hits_without_poisoning_accepted_data`
  pins rejection, cache preservation and the next accepted request.
- Image loads run on the lanes their caller hands them, and the registry starts no
  runtime or thread of its own (ADR-0279).
  `image_loads_run_on_their_lanes_and_survive_the_requests_that_leave` exercises
  lanes, no lanes, refused lanes, handover and cancellation through the public API.
  [ADR-0105](../../../docs/adr/ADR-0105-asset-validation-and-bridge-progress.md)
  records the validation contract.

### Generic initialization is independent; font loads share work

The public cache helper performs get, initialize and insert without joining a
pending same-key initializer. Each successful call publishes; later completion
can replace earlier data. Existing handles retain their own allocations. Errors
are owned and never cached. Cancellation drops only that initializer, while
independent work continues. A panic reaches its caller; subsequent calls can
retry. No exactly-once side effects or generic cold-load coalescing is promised.
This covers direct reentry, destructor reentry and awaited native spawned work
without ancestry trackers, runtime-specific context or a global wait graph.

Registry loading validates each descriptor before lookup. The crate-private
AssetCache::load boundary checks the exact TypeId identity of the closed
FontAsset producer. Only fonts enter the private borrowed-key Moka initializer;
FontAsset reads bytes through standard file IO and wraps them in Arc
without custom asset or decoder callbacks. There is no custom opt-in switch, public
coalescing closure API, specialization or unsafe conversion. ImageAsset and generic Asset
implementations use the independent public helper and keep owned AssetError
results. Wrapping a font asset in a custom type does not inherit singleflight.
Image decoding keeps image crate registered format/decoder hooks, which can
reenter the registry, so images cannot enter this singleflight boundary.

Font cold contenders share an allocation or failure; failures are not cached.
Moka waiters can restart after elected cancellation or panic, subject to its
finite retry policy. Font loading must tolerate restart. Only fresh returned
entries count as completed font insertions. Generic initialization counts each
successful publication; presence and count observations can race concurrent writes.
Cancellation after backend publication can leave data without a completed count.

[ADR-0120](../../../docs/adr/ADR-0120-typed-asset-cache-retention.md)
records this boundary. The public
`cold_registry_loads_preserve_publication_and_recover` family pins independent
custom loads, late completion overwrite and retained handle ownership, direct
and two-thread spawned reentry, errors, cancellation, panic and healthy retry.
Spawned rows have five-second timeouts. Its cold font/image rows occupy the
runtime-local IO worker before polling both
requests, then assert shared font allocation and independent image allocations.
`builtin_initialization_shares_work_and_recovers` is a private failure-path
matrix because the public font loader cannot deterministically suspend at each
IO/cancellation/panic boundary. It invokes the actual private Moka helper and
pins one accepted initializer, shared data/error, waiter restart and next hit.

### Decoded images are kept, shared and cancelled by their registry

Image loads (`load_image`, `load_network_image`) bypass typed caching. Each
registry keeps their decoded images in its own LRU, bounded by pixel bytes
(`with_decoded_image_budget`; 256 MiB, 64 MiB on Android, iOS and the web); an
image larger than the budget is handed to its waiters and not kept.
`cached_image` is the synchronous probe a frame makes before it starts a load.
There is no process-wide image cache, so equal sources in two registries are two
images (ADR-0250).

A load runs on the `LoadServices` its first request hands it: lanes (an
`Arc<dyn flui_platform::executor::Executor>`, the UI runtime's for the `Image`
widget) and an HTTP client (`flui_platform::http::Http`, the host's for that
widget, ADR-0280): read or fetched on the IO lane, decoded on the compute lane.
The registry starts no runtime or thread (ADR-0279). With no lanes, or lanes
refusing it for want of room, the load runs inside the requesting `ImageLoad`,
on the thread that polls it; lanes shutting down answer
`AssetError::LanesRefused`.

A request for a source already loading joins that load. The load is the
registry's: every `ImageLoad` waiting on it is a subscriber, and only when the
last one is dropped is the pending entry removed, its ticket cancelled and its
cancel signal sent, which the load sees before its next step. An HTTP request is
dropped at once. A file is read in 256 KiB chunks with a yield between them, so
the read stops at the next chunk. A load cancelled before its bytes are ready
never asks for a decode slot, and a decode that has not started never runs. The
ticket's allocation is the load's identity: a load finishing after its entry was
abandoned or replaced compares tickets and leaves the newer entry alone.

A load that ends without an outcome, because the request it ran in left or its
lanes shut down and dropped it, closes its waiters, and each admits itself again:
the first becomes the new lead, on its own lanes. A load that panics answers its
waiters with the failure instead, from a drop guard that sees the unwind.

Decodes run at most `with_decode_threads` at once (half the cores, at most four),
never on the IO lane. A compute lane with no room hands the decode back to where
the load runs; lanes shutting down, by refusing the decode or dropping it unrun,
close the load unsettled for a waiter on other lanes to take over. At admission,
on the calling thread and under the lock that assigns its order, a load reserves
one of four load slots per decode thread. A load bound for lanes is admitted when
it is requested; a load with no lanes runs only while its request polls it, so it
is admitted at its first poll: loads a caller holds unpolled keep no slot from
the one it awaits, and open no entry that a request for the same source would
wait on with nobody driving it. A load
waits for its slot before it reads and holds it until its decode
returns, so the reads in flight and the encoded bytes waiting to decode are
bounded by count, and each by size: a response body by `MAX_BODY_BYTES`, a file
by the same 256 MiB (`an_oversized_file_is_refused_before_it_is_read`), refused
before more than that is buffered. Only a regular file is opened, so a FIFO or a
device cannot block the shared IO lane in `open`. Once its bytes are read it asks for a decode
slot. Load slots are granted in admission order. A decode slot goes to the
earliest-admitted load whose bytes are ready; a load still fetching holds none,
so a ready load admitted later decodes first rather than leaving the decoder idle
behind a slow response. Strict admission order would be head-of-line blocking:
one stalled request would stop every visible image behind it. A load abandoned
while it waited for either slot skips the decode and frees its load slot.

One mutex guards the cache and the pending map. Senders are completed, cancel
signals sent, loads spawned and evicted pixels freed after it is released: lanes
that refuse a load drop it inside `spawn_io`, and that drop settles its entry
through the same lock.

Under memory pressure the registry trims decoded images to half the budget
(moderate) or drops them all (critical); loads in flight are left alone.

`decoded_images_stay_within_their_byte_budget_and_give_back_under_pressure`
pins byte eviction, the over-budget image and both pressure levels.
`image_loads_run_on_their_lanes_and_survive_the_requests_that_leave` pins the
lanes, sharing, handover and cancellation.
`an_image_request_lives_exactly_as_long_as_someone_waits_for_it` pins coalescing
onto one HTTP connection, which stays open while one waiter remains and closes
when the last is dropped. The separate `image_decode_scheduling` target, which
registers real decoder hooks, runs
`decodes_leave_the_io_lane_free` (a decode waits for a task on the IO lane's only
worker) and
`abandoned_loads_never_decode_and_do_not_delay_a_live_one` (one decode thread:
five loads abandoned behind a running decode never decode, and the live load
queued after them decodes next) and
`decode_slots_go_to_the_earliest_admitted_ready_load` (with the decoder free,
a file load decodes while an earlier network load still fetches; with both
waiting behind a running decode, the earlier-admitted network load goes first)
and `loads_past_admission_are_bounded_by_count` (one decode thread: while a
decode holds its slot only three more requests go out, and the next starts
when it returns).

A network image's request carries `HttpRequest`'s default deadline (two minutes,
body included), so a server that drips one byte at a time cannot hold a load
slot forever, and its 256 MiB body limit; the host's built-in client also gives
up on a connection and on a silent response after 30 seconds.

Every image decode, typed or registry-kept, reads the header first and refuses an
image whose RGBA8 pixels would pass 256 MiB (128 MiB on Android, iOS and the
web), which is also the decoder's allocation limit: an image that compresses
small but decodes huge fails with a typed error instead of allocating. With at
most four decodes at once, their transient memory stays a few times that.
`decoded_pixels_are_bounded_before_decoding` decodes a header that declares
too many pixels.

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

The separate `registered_image_hook_can_await_same_key_spawned_work` consumer test
registers a real image decoder hook, starts a same-key child from it and bounds
its synchronous bridge to three seconds. It checks child publication, independent
parent/child handles, unchanged registered-hook decoding and the next cache hit.
Its dedicated process is necessary because image registrations are global and
cannot be removed. No hook registration enters the ordinary integration binary.
