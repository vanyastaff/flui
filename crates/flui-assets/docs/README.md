# flui_assets Documentation

## Guides

- [User guide](GUIDE.md): registry ownership, typed assets and cache operations.
- [Architecture](ARCHITECTURE.md): locking, data flow and behavior decisions.
- [Design patterns](PATTERNS.md): type-state configuration and shared handles.
- [Cache behavior and performance](PERFORMANCE.md): count capacity, expiration,
  maintenance and diagnostics.

## Quick reference

```rust
use flui_assets::{AssetRegistryBuilder, FontAsset};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    let font = registry.load(FontAsset::file("font.ttf")).await?;
    println!("Loaded bytes: {}", font.bytes.len());
    Ok(())
}
```

Default caches retain up to 10,240 completed entries per asset type after pending
maintenance settles, with five-minute lifetime and one-minute idle expiration.
`CacheCapacity::Entries(NonZeroU64)` selects a positive count;
`CacheCapacity::Disabled` retains no completed entries. `AssetCacheConfig`
configures capacity and validated `CacheExpiration` policies. These are entry
bounds, not decoded-byte budgets.

Ordinary registry loading validates descriptors and shares concurrent cold
initialization through Moka. Successful results include decoded images and fonts.
Errors are not cached. Strong handles retain their data after eviction; weak
handles do not retain data. Both strong and weak asset handles own their keys.
`AssetKey` owns a nonempty shared string: clones share storage, independently
constructed equal names compare by contents, and `as_str` borrows from the key.
Its storage is released when the last owner disappears rather than retained by
a global interner. Widget bridge methods bypass registry caching and
feed a synchronous widget image LRU.

A standalone typed `AssetCache` exposes shared operation counters, estimated
entry count and utilization. `contains` observes presence synchronously without
refreshing idle expiration. `insert_many` inserts sequentially. Registry typed
cache lookup is private and no public aggregated registry statistics API exists.

## Optional features

| Feature | Description |
|---------|-------------|
| `images` | Image asset decoding |
| `network` | HTTP/HTTPS byte loading |
| `full` | Both stable optional features |

## Commands

```bash
cargo test -p flui-assets --all-features
cargo doc -p flui-assets --all-features --open
cargo run -p flui-assets --example assets_basic_usage
cargo run -p flui-assets --features network --example network_loader
```

## Project information

- [Crate README](../README.md)
- [FLUI README](../../../README.md)
- [Contributing](../../../CONTRIBUTING.md)
- [Apache license](../../../LICENSE-APACHE)
- [MIT license](../../../LICENSE)
