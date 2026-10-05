# Cache behavior and performance

Moka manages concurrent admission and eviction of typed loaded results. FLUI
stores each result in an `Arc`, so a cache hit shares data instead of copying a
decoded image or font. No project benchmark establishes a latency, throughput,
memory-layout or hit-rate advantage over another cache.

## Name ownership

`AssetKey` owns a nonempty `Arc<str>` instead of an integer in a global interner.
Cloned keys share storage, and built-in descriptors share their name allocation
with their returned keys. Independently constructed equal names remain equal by
contents; comparison and hashing are string operations, not a promised
constant-time integer operation. This change trades global deduplication and
Copy keys for reclamation of names when their final owner disappears. It makes
no claim of a measured speed or fixed-size advantage.

Consumer handles preserve their names after registry or cache ownership ends.
Weak data handles also own their keys strongly; account for them when tracking
name retention. `as_str()` borrows from a key and supplies no static-lifetime
storage guarantee. Shared `Arc<str>` input can be transferred into a key without
copying its string contents.

## Capacity

Capacity counts completed entries separately for each asset type:

```rust
use flui_assets::{AssetRegistryBuilder, CacheCapacity};
use std::num::NonZeroU64;

let registry = AssetRegistryBuilder::new()
    .with_capacity(CacheCapacity::Entries(NonZeroU64::new(256).expect("nonzero entry limit")))
    .build();
```

The default is 10,240 entries per type. `CacheCapacity::Disabled` loads data
without retaining completed entries. Concurrent cold callers still share an
initializer; a subsequent request loads again.

This policy does not bound decoded bytes or process memory. Entry sizes can vary
widely, and consumer handles retain their data after cache eviction. Select a
count from measured asset sizes and access patterns rather than interpreting it
as a byte budget. Reducing the count can reduce cache ownership, but cannot free
data that a consumer still owns.

Moka applies its capacity policy during maintenance. `len()` is an estimated
`u64` entry count; `sync().await` improves accuracy after operations settle.
Concurrent operations and expiration can still change the result.

## Expiration

`AssetCacheConfig` independently configures lifetime and idle expiration. Defaults
are five minutes since insertion and one minute since a retrieving cache read.
`CacheExpiration::NEVER` disables either policy. `CacheExpiration::after` returns
an error for intervals above Moka's supported 1,000-year maximum; zero expires
immediately.

A synchronous `contains(&key)` is an observation: it does not clone data, record
requests, update popularity or refresh idle expiration. `get(&key).await` retrieves
shared data and records a read. Expiration and invalidation do not revoke handles
already returned to consumers or cancel in-flight initializers.

## Coalescing

Generic `get_or_insert_with` gets completed data or runs its own initializer,
then inserts each successful result. Cold calls, including direct or awaited
spawned reentry, do not wait on pending same-key work. Late completion may replace
an earlier entry; existing handles keep their data. Errors remain owned and are
not cached. Cancellation affects only that call; independent work survives and
later requests can retry. No exactly-once side effects are guaranteed.

Registry loading validates every descriptor. Only closed built-in FontAsset producers share pending work through Moka.
ImageAsset keeps registered image decoder hooks and uses the independent helper,
as custom assets do. Font failures are shared but not cached; cancellation or
panic allows a waiter to restart subject to Moka's finite retry policy. No public
custom coalescing opt-in or mandatory spawn API is introduced.

## Observing a typed cache

```rust
use flui_assets::{AssetCache, AssetCacheExt, CacheCapacity, FontAsset};

let cache = AssetCache::<FontAsset>::new(CacheCapacity::default());
cache.sync().await;
println!("Estimated entries: {}", cache.len());
println!("Entry utilization: {:.1}%", cache.utilization() * 100.0);
let stats = cache.stats();
println!("Hits: {}, misses: {}, invalidation requests: {}",
    stats.hits, stats.misses, stats.invalidations);
```

Counters are shared across cache clones and saturate rather than wrap. For
initialization, hits and misses describe the initial presence observation,
which can race concurrent changes. A fresh returned entry counts as a completed
insertion; cancellation after backend publication can leave an entry without
that completed count. `invalidations` counts explicit invalidation requests,
including absent keys; it does not count capacity eviction or expiration.

`utilization()` divides the estimated count by configured entry capacity and
clamps maintenance lag to one. It returns zero for disabled retention and does
not measure bytes. `clear().await` invalidates completed entries and resets
counters; `reset_stats()` resets counters without invalidation. Neither operation
is a snapshot of concurrent requests. The registry exposes no public typed-cache
lookup or aggregated statistics API.

`insert_many` performs sequential inserts and returns handles in input order.
Use independent async loads when concurrency is needed, keeping expensive IO and
decoding outside the synchronous build/layout/paint path.

## Widget image loading

Ordinary `registry.load(ImageAsset)` caches decoded images in Moka. Widget image
providers use the registry's bridge methods, which bypass that typed cache. Their
small non-expiring LRU supports a synchronous frame-path probe and shares
in-flight work by subscriber lifetime. This routing avoids a second typed-cache
lookup for bridged loads; it is not a claim that all applications should maintain
two image caches.

## Measuring an application

Measure cold IO/decoding separately from warm cache retrieval, and record feature
flags, input assets and runtime configuration. Inspect retained handles alongside
cache counts when investigating memory pressure. A high miss rate can result
from expiration, insufficient entry capacity, disabled retention or distinct
keys; increasing capacity does not address every cause.

Use real workload measurements before changing admission policy, hashers or
expiration defaults. Moka already supplies concurrency and TinyLFU admission;
wrapping its whole cache in an application mutex would serialize those operations.
FLUI's statistics have a separate short lock.

## References

- [Moka future cache](https://docs.rs/moka/0.12.16/moka/future/struct.Cache.html)
- [Moka entry selector](https://docs.rs/moka/0.12.16/moka/future/struct.RefKeyEntrySelector.html)
- [Architecture and behavior tests](ARCHITECTURE.md)
