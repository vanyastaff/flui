# flui_assets

High-performance asset management system for FLUI framework with smart caching, type safety, and async I/O.

## Features

- 🚀 **High Performance** - Concurrent Moka caching with TinyLFU admission
- 🔒 **Thread-Safe** - Built on tokio, parking_lot, and moka for concurrent access
- 💾 **Smart Caching** - Explicit entry capacity and configurable expiration
- 🎯 **Type-Safe** - `Asset` trait with typed `Data`, `Key` and `Error`
- ⚡ **Async I/O** - Non-blocking loading with tokio runtime
- 🔑 **Efficient Keys** - Owned nonempty names with shared string storage
- 📦 **Arc-Based Handles** - Shared loaded data; weak data references do not extend its lifetime
- 🎨 **Built-in Assets** - Images (optional), fonts, with extensible system

## Quick Start

```rust
use flui_assets::{AssetRegistryBuilder, FontAsset};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a registry
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();

    // Load a font
    let font = FontAsset::file("assets/Roboto-Regular.ttf");
    let handle = registry.load(font).await?;

    println!("Font loaded: {} bytes", handle.bytes.len());

    // Subsequent loads share cached data while it remains retained
    let handle2 = registry.load(FontAsset::file("assets/Roboto-Regular.ttf")).await?;

    Ok(())
}
```

## Architecture

### Three-Layer Design

```
AssetRegistry (per app)
    ↓
AssetCache<T> (Per Type) - Moka TinyLFU cache
    ↓
AssetHandle<T, K> (Arc) - Smart handles with weak references
```

### Type State Builder

The registry uses a type-state builder for compile-time validation:

```rust
use flui_assets::{AssetRegistryBuilder, CacheCapacity};
use std::num::NonZeroU64;

// ✅ This compiles
let registry = AssetRegistryBuilder::new()
    .with_capacity(CacheCapacity::Entries(NonZeroU64::new(256).expect("nonzero entry limit")))
    .build();

// ❌ This doesn't compile - cannot build without capacity
// let registry = AssetRegistryBuilder::new().build();
```

### Extension Traits

Convenience methods without bloating core API:

```rust
use flui_assets::{AssetHandle, AssetHandleExt, AssetCache, AssetCacheExt, CacheCapacity, FontAsset};

let handle = registry.load(font).await?;

// Handle extensions
if handle.is_unique() {
    println!("Only reference!");
}
let size = handle.map(|font| font.bytes.len());
println!("Total refs: {}", handle.total_ref_count());

// Cache extensions
let cache: AssetCache<FontAsset> = AssetCache::new(CacheCapacity::default());
println!("Hit rate: {:.1}%", cache.hit_rate() * 100.0);
if cache.is_efficient() {
    println!("Cache performing well (>70% hit rate)");
}
```

## Asset Types

### Fonts (Built-in)

```rust
use flui_assets::{AssetRegistryBuilder, FontAsset};

let registry = AssetRegistryBuilder::new().with_default_capacity().build();
let font = FontAsset::file("fonts/Roboto-Regular.ttf");
let handle = registry.load(font).await?;

// Or from bytes
let bytes = std::fs::read("font.ttf")?;
let font = FontAsset::from_bytes("embedded-font", bytes);
let handle = registry.load(font).await?;
```

### Images (Optional)

Requires `images` feature flag:

```toml
[dependencies]
flui-assets = { git = "https://github.com/vanyastaff/flui", features = ["images"] }
```

```rust
use flui_assets::{AssetRegistryBuilder, ImageAsset};

let registry = AssetRegistryBuilder::new().with_default_capacity().build();
let image = ImageAsset::file("assets/logo.png");
let handle = registry.load(image).await?;

println!("Image: {}x{}", handle.width(), handle.height());
```

### Custom Assets

Implement the `Asset` trait:

```rust
use flui_assets::{Asset, AssetKey, AssetError, AssetMetadata};

pub struct AudioAsset {
    path: String,
}

#[derive(Debug, Clone)]
pub struct AudioData {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl Asset for AudioAsset {
    type Data = AudioData;
    type Key = AssetKey;
    type Error = AssetError;

    fn key(&self) -> AssetKey {
        AssetKey::new(&self.path)
    }

    async fn load(&self) -> Result<AudioData, AssetError> {
        let bytes = tokio::fs::read(&self.path).await?;
        // Decode audio...
        Ok(AudioData { samples: vec![], sample_rate: 44100 })
    }

    fn metadata(&self) -> Option<AssetMetadata> {
        Some(AssetMetadata {
            format: Some("Audio".to_string()),
            ..Default::default()
        })
    }
}
```

## Loaders

### File Loader

```rust
use flui_assets::BytesFileLoader;

let loader = BytesFileLoader::new("assets");
let bytes = loader.load_bytes("logo.png").await?;
let text = loader.load_string("config.json").await?;
```

### Embedded bytes

Construct `FontAsset::from_bytes` or, with `images`, `ImageAsset::from_bytes`.
The asset owns the source bytes and decodes through the same `Asset::load` contract
as a file-backed asset; the registry caches its decoded result.

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `images` | Enable image loading (PNG, JPEG, GIF, WebP) | No |
| `network` | Enable HTTP/HTTPS asset loading | No |
| `full` | Enable all stable features | No |

## Performance Characteristics

### Memory Efficiency
- **AssetKey**: owns an `Arc<str>`; clones share its string allocation
- **AssetHandle**: stores a key and an `Arc` sharing the loaded data

Asset keys compare and hash by string contents, so independently constructed
keys with equal names identify the same asset. Keys are Clone rather than Copy;
`as_str()` borrows from the owning key instead of returning a static string.
`FontAsset` and `ImageAsset` keep shared names, and their `key()` methods clone
that storage. Names are reclaimed after their final owner disappears, without
a process-wide interner. A weak asset handle still owns its key strongly even
though it does not retain the loaded data.

### Cache Behavior

Moka caches typed loaded results, including decoded images and fonts. Capacity
counts entries separately for each type: the default is 10,240 entries, not a
byte budget. `CacheCapacity::Disabled` retains no completed entries while still
sharing concurrent cold initialization. Default expiration is a five-minute
lifetime and one-minute idle interval; `AssetCacheConfig` can configure either.
Consumer handles retain data after cache eviction.

`get_or_insert_with` shares data or an `Arc<Error>` between cold callers. Errors
are not cached. Registry loads validate every descriptor, including cache hits,
and preserve their owned `AssetError` contract. Widget bridge methods bypass this
typed cache and deliver decoded images to the widget layer's synchronous LRU.

`contains` is synchronous and does not count reads or refresh idle expiration.
`insert_many` inserts sequentially. Counters share a short lock across clones;
`invalidations` counts explicit invalidation requests, not automatic removals.
`len` and utilization are estimated entry metrics, not memory measurements.
`AssetCache::stats()` reports that typed cache's counters; the registry does not
expose typed-cache lookup or aggregate statistics across asset types.

### Thread Safety

All types implement `Send + Sync`:

```rust
fn assert_send_sync<T: Send + Sync>() {}

assert_send_sync::<AssetKey>();
assert_send_sync::<AssetHandle<FontData, AssetKey>>();
assert_send_sync::<AssetCache<FontAsset>>();
assert_send_sync::<AssetRegistry>();
```

## Error Handling

```rust
use flui_assets::AssetError;

match registry.load(asset).await {
    Ok(handle) => println!("Loaded!"),
    Err(AssetError::Io(e)) => eprintln!("IO error: {}", e),
    Err(AssetError::InvalidData { path, reason }) => eprintln!("Invalid {}: {}", path, reason),
    Err(AssetError::NotFound { path }) => eprintln!("Not found: {}", path),
    Err(e) => eprintln!("Error: {}", e),
}
```

## Testing

```bash
# Run all tests
cargo test -p flui-assets

# Run with all features
cargo test -p flui-assets --all-features

# Check documentation
cargo doc -p flui-assets --open
```

## Examples

```bash
# Basic usage
cargo run -p flui-assets --example assets_basic_usage

# With images (requires 'images' feature)
cargo run -p flui-assets --example assets_basic_usage --features images
```

## Documentation

### Quick Links

- **[User Guide](docs/GUIDE.md)** - Complete guide to using flui_assets
- **[Architecture](docs/ARCHITECTURE.md)** - System internals and design
- **[Design Patterns](docs/PATTERNS.md)** - Patterns used and why
- **[Performance](docs/PERFORMANCE.md)** - Optimization techniques

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT License ([LICENSE](../../LICENSE))

at your option.

## Related Crates

- [`flui-foundation`](../flui-foundation) - Core types and geometry values for FLUI
- [`flui-painting`](../flui-painting) - 2D graphics API
