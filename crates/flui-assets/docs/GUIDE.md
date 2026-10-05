# User Guide

## Quick Start

Add the asset crate and a Tokio runtime to your application:

```toml
[dependencies]
flui-assets = { git = "https://github.com/vanyastaff/flui" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

```rust
use flui_assets::{AssetRegistryBuilder, FontAsset};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    let font = registry.load(FontAsset::file("assets/font.ttf")).await?;
    println!("Font loaded: {} bytes", font.bytes.len());
    Ok(())
}
```

Build one registry for a shared asset domain and pass a reference or `Arc` to
consumers. Different registries own independent typed caches and loading policy.

## Basic Usage

### Capacity and expiration

The default retains up to 10,240 completed entries per asset type after pending
maintenance settles. Capacity counts entries, not bytes:

```rust
use flui_assets::{AssetRegistryBuilder, CacheCapacity};
use std::num::NonZeroU64;

let registry = AssetRegistryBuilder::new()
    .with_capacity(CacheCapacity::Entries(NonZeroU64::new(256).expect("nonzero entry limit")))
    .build();
```

Use `CacheCapacity::Disabled` to retain no completed entries. Concurrent cold
custom requests run independently; font requests share pending work. Later
requests reload. Full configuration independently selects lifetime and idle expiration:

```rust
use flui_assets::{AssetCacheConfig, AssetRegistryBuilder, CacheCapacity, CacheExpiration};
use std::time::Duration;

let config = AssetCacheConfig {
    capacity: CacheCapacity::default(),
    time_to_live: CacheExpiration::NEVER,
    time_to_idle: CacheExpiration::after(Duration::from_mins(2))?,
};
let registry = AssetRegistryBuilder::new().with_cache_config(config).build();
```

`CacheExpiration::after` rejects unsupported durations before cache construction.
Defaults are five-minute lifetime and one-minute idle expiration. Neither capacity
nor expiration limits memory retained by consumer handles.

### Loaded data and identity

```rust
use flui_assets::{AssetRegistryBuilder, FontAsset};

let registry = AssetRegistryBuilder::new().with_default_capacity().build();
let first = registry.load(FontAsset::file("font.ttf")).await?;
let second = registry.load(FontAsset::file("font.ttf")).await?;
assert!(first.ptr_eq(&second));
println!("Loaded bytes: {}", first.bytes.len());
```

A cache hit shares the loaded allocation. Equal typed keys identify equivalent
requests; callers must use different keys when the requested data differ.
Cloned handles share data without requiring `Data: Clone`.

### Cache management

```rust
use flui_assets::{AssetKey, AssetRegistryBuilder, FontAsset};

let registry = AssetRegistryBuilder::new().with_default_capacity().build();
let key = AssetKey::new("font.ttf");
registry.invalidate::<FontAsset>(&key).await;
registry.clear::<FontAsset>().await;
registry.clear_all().await;
```

Invalidation excludes completed cached values from subsequent lookups. It does
not cancel an in-flight initializer, which may insert afterward. Previously
returned handles remain valid. `clear_all` detaches the registry's cache map
before retiring data so generic destructors can reenter the committed registry.

## Asset Types

### Fonts

Fonts are available without optional features. Embedded sources take a cache
identifier and an owned byte vector:

```rust
use flui_assets::FontAsset;

let bytes = std::fs::read("font.ttf")?;
let asset = FontAsset::from_bytes("embedded-font", bytes);
let font = registry.load(asset).await?;
```

### Images

Enable `images` to use `ImageAsset`; `full` additionally enables `network`:

```toml
flui-assets = { git = "https://github.com/vanyastaff/flui", features = ["images"] }
```

```rust
use flui_assets::ImageAsset;

let image = registry.load(ImageAsset::file("logo.png")).await?;
println!("Image: {}x{}", image.width(), image.height());
let bytes = std::fs::read("logo.png")?;
let embedded = registry.load(ImageAsset::from_bytes("embedded-logo", bytes)).await?;
```

Ordinary registry loading caches the decoded result. Widget image bridges bypass
this cache and deliver results to the widget layer's synchronous LRU.

### Custom assets

Implement `Asset` with typed data, key and error. `Data` is shared and need not be
Clone. `validate` runs before every registry cache lookup, including hits; direct
callers of `Asset::load` are responsible for validation themselves.

```rust
use flui_assets::{Asset, AssetError};

struct TextAsset { path: String }

impl Asset for TextAsset {
    type Data = String;
    type Key = String;
    type Error = AssetError;

    fn key(&self) -> String { self.path.clone() }

    async fn load(&self) -> Result<String, AssetError> {
        Ok(tokio::fs::read_to_string(&self.path).await?)
    }
}
```

## Advanced Features

### Typed cache operations

A separately owned `AssetCache` exposes counters and maintenance. Registry cache
lookup is private; there is no aggregated registry statistics API.

```rust
use flui_assets::{Asset, AssetCache, AssetCacheExt, CacheCapacity, FontAsset};

let cache = AssetCache::<FontAsset>::new(CacheCapacity::default());
let font = FontAsset::file("font.ttf");
let key = font.key();
let loaded = cache.get_or_insert_with(key, || font.load()).await?;
assert!(cache.contains(loaded.key()));
cache.sync().await;
println!("Estimated entries: {}, utilization: {}", cache.len(), cache.utilization());
println!("Invalidation requests: {}", cache.stats().invalidations);
```

`contains` is synchronous and does not count a request or refresh idle expiration.
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

Counters are shared across clones. Hit/miss counts for initialization describe
an initial presence probe; concurrent changes can race it. Insertions count
every generic publication and fresh returned built-in results, not guaranteed
resident entries. `invalidations` includes requests for absent keys and excludes
automatic eviction. `len` and utilization are estimates, not memory measurements.

### Weak ownership

```rust
let font = registry.load(FontAsset::file("font.ttf")).await?;
let weak = font.downgrade();
drop(font);
if let Some(still_loaded) = weak.upgrade() {
    println!("Retained bytes: {}", still_loaded.bytes.len());
}
```

Weak handles do not retain data. Their upgrade can succeed because the cache or
another consumer still owns it, including a consumer holding an evicted value.
Eviction itself does not depend on dropping consumer handles.

### Byte sources

```rust
use flui_assets::BytesFileLoader;

let loader = BytesFileLoader::new("assets");
let bytes = loader.load_bytes("logo.png").await?;
let text = loader.load_string("config.json").await?;
```

With `network`, `NetworkLoader` fetches HTTP bytes and is constructed fallibly.
Asset implementations own source selection and decoding; loaders do not infer
arbitrary decoded types from keys.

## Best Practices

Share a registry within one asset domain, preload only needed assets, and measure
real decoded sizes and access patterns before selecting entry counts. Keep IO and
decoding outside synchronous UI build/layout/paint. Handle loading failures as
results rather than assuming sources exist. Preload different asset types through
separate typed calls:

```rust
registry.preload(FontAsset::file("font.ttf")).await?;
// With the images feature:
registry.preload(flui_assets::ImageAsset::file("logo.png")).await?;
```

## Troubleshooting

For missing files, check the application's working directory and the path supplied
to the descriptor. For unexpected cache misses, inspect keys, capacity and
expiration. For memory pressure, inspect retained consumer handles and decoded
asset sizes as well as cache counts. Increasing entry capacity does not address
failures, source changes hidden behind equal keys or data retained outside caches.

## Feature Flags

| Feature | Description |
|---------|-------------|
| `images` | Image asset decoding |
| `network` | HTTP/HTTPS byte loading |
| `full` | Both stable optional features |

## Next Steps

- [Architecture](ARCHITECTURE.md)
- [Design patterns](PATTERNS.md)
- [Cache behavior and performance](PERFORMANCE.md)
- `cargo doc -p flui-assets --open`
