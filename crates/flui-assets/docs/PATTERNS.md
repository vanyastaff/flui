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
A weak handle observes that ownership without extending the data's lifetime.
Eviction removes cache ownership, not ownership held by existing handles.

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

Ordinary concurrent requests coalesce. Same-key calls inside an initializer or
its destructor cannot wait for that initializer: they reuse completed data when
available, otherwise return independently initialized uncached handles. Only the
outer elected initializer publishes its result. Ancestry is shared by cache
clones and scoped to polling and retirement, including after suspension; it does
not detect cycles through separately spawned tasks.

Moka's `entry_by_ref(...).or_try_insert_with(...)` selects one cold initializer
per typed key and shares its result with waiting callers. The public helper
returns `Arc<Asset::Error>`, so custom errors do not need a `Clone` implementation.
Only a fresh returned entry counts as a completed insertion.

```rust
use flui_assets::{Asset, AssetCache, CacheCapacity, FontAsset};

let cache = AssetCache::<FontAsset>::new(CacheCapacity::default());
let font = FontAsset::file("font.ttf");
let handle = cache.get_or_insert_with(font.key(), || font.load()).await?;
```

Registry loading first calls `validate`, then uses this same helper and maps
shared `AssetError` into its owned error contract. Validation still runs for
cache hits. Failed loads are not retained. Cancellation and an initializer panic
allow another waiter to retry; repeated failures are bounded by Moka's retry
policy. Invalidation affects completed entries, not pending initialization.

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
