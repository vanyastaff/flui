# Design patterns in flui_assets

## Type-state configuration

`AssetRegistryBuilder<NoCapacity>` cannot build a registry until the caller
selects capacity or a complete cache configuration. `with_default_capacity`
selects 10,240 completed entries per asset type. Explicit capacity separates
disabled retention from a positive count:

```rust
use flui_assets::{AssetRegistryBuilder, CacheCapacity};
use std::num::NonZeroU64;

let registry = AssetRegistryBuilder::new()
    .with_capacity(CacheCapacity::Entries(NonZeroU64::new(128).expect("nonzero entry limit")))
    .build();

let uncached = AssetRegistryBuilder::new()
    .with_capacity(CacheCapacity::Disabled)
    .build();
```

`with_cache_config` supplies `AssetCacheConfig`, including independently validated
lifetime and idle expiration. Capacity transitions preserve configured expiration
and any host runtime or HTTP policy. The type-state transition requires an
explicit policy; it does not claim a memory budget.

## Shared handle ownership

`AssetHandle<Data, Key>` owns an `Arc<Data>` and a caller-selected key. Cloning a
handle shares data and clones its key; data does not need to implement `Clone`.
A weak handle observes data ownership without extending the data's lifetime,
but still owns its generic key strongly. Eviction removes cache ownership, not
ownership held by existing handles.

`AssetKey` owns a nonempty `Arc<str>`. Clones share its allocation, while equality
and hashing use contents even for independently constructed names. Its borrowed
`as_str` result cannot outlive the key. Font and image descriptors keep shared
names and clone them when producing keys. Name reclamation follows those owners;
there is no process-global retaining arena or integer identity.

```rust
use flui_assets::{AssetHandle, AssetKey};
use std::sync::Arc;

let first = AssetHandle::new(Arc::new(vec![1, 2, 3]), AssetKey::new("bytes"));
let shared = first.clone();
let independent = AssetHandle::new(Arc::new(vec![1, 2, 3]), AssetKey::new("bytes"));
assert!(first.ptr_eq(&shared));
assert!(!first.ptr_eq(&independent));
```

Equal keys identify equivalent requested assets, not equal allocation ownership.
Reloading an evicted key may create another allocation while old handles remain
live. `ptr_eq` checks shared ownership directly.

## Sealed core traits and extensions

`AssetHandleCore` and `AssetCacheCore` are sealed. Their extension traits provide
convenience operations for the supported handles and caches. Presence and
capacity are core cache observations; batch insertion is an extension operation.

```rust
use flui_assets::{AssetCache, AssetCacheExt, AssetKey, CacheCapacity, FontAsset};

let cache = AssetCache::<FontAsset>::new(CacheCapacity::default());
let key = AssetKey::new("font.ttf");
assert!(!cache.contains(&key));
assert_eq!(cache.stats().total_requests(), 0);
assert_eq!(cache.utilization(), 0.0);
```

`contains` is synchronous and does not refresh idle expiration. `get` retrieves
shared data asynchronously and records a request. Sealing controls who implements
the core traits; it does not make every public API change backward compatible.

## Fallible shared initialization

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

```rust
use flui_assets::{Asset, AssetCache, CacheCapacity, FontAsset};

let cache = AssetCache::<FontAsset>::new(CacheCapacity::default());
let font = FontAsset::file("font.ttf");
let handle = cache.get_or_insert_with(font.key(), || font.load()).await?;
```

Registry validation still runs for cache hits. Invalidation affects completed
entries, not pending initialization; a successful load can publish afterward.

## Type-erased registry storage

A registry maps each asset type's `TypeId` to its typed `AssetCache`. An existing
cache is cloned under the map lock, then used after releasing that lock. Moka
entries and FLUI operation counters remain shared across these clones.

`clear_all` detaches the map under its write guard and retires the outgoing map
after releasing that guard. Generic data destructors may reenter the same
registry and observe the committed state. Last-owner teardown exposes an
absent-owner fallback through weak ownership; it cannot resurrect the registry.

## Source and presentation boundaries

An `Asset` selects bytes, validates its descriptor and decodes its typed data.
`BytesFileLoader` and `NetworkLoader` supply byte sources; they are not generic
cache-key-to-asset factories. Embedded constructors take an identifier and owned
bytes. Ordinary registry loads cache loaded results, including decoded images.

Widget bridge methods bypass registry caching and deliver decoded images to the
widget layer's synchronous LRU. This preserves synchronous build/layout/paint
while async IO runs at the loading edge. Subscriber ownership determines when
abandoned shared widget loads leave their pending map.

## References

- [Architecture](ARCHITECTURE.md)
- [User guide](GUIDE.md)
- [Cache behavior and performance](PERFORMANCE.md)
