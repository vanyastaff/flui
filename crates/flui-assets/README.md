# flui_assets

High-performance asset management system for FLUI framework with smart caching, type safety, and async I/O.

## Features

- 🚀 **High Performance** - Lock-free caching with TinyLFU eviction algorithm
- 🔒 **Thread-Safe** - Built on tokio, parking_lot, and moka for concurrent access
- 💾 **Smart Caching** - Automatic memory management with configurable capacity
- 🎯 **Type-Safe** - Generic `Asset<T>` trait for compile-time guarantees
- ⚡ **Async I/O** - Non-blocking loading with tokio runtime
- 🔑 **Efficient Keys** - 4-byte interned keys for fast hashing and comparison
- 📦 **Arc-Based Handles** - Cheap cloning with automatic cleanup via weak references
- 🎨 **Built-in Assets** - Images (optional), fonts, with extensible system

## Quick Start

```rust
use flui_assets::{AssetRegistry, FontAsset};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get the global registry
    let registry = AssetRegistry::global();

    // Load a font
    let font = FontAsset::file("assets/Roboto-Regular.ttf");
    let handle = registry.load(font).await?;

    println!("Font loaded: {} bytes", handle.bytes.len());

    // Subsequent loads use the cache (instant!)
    let handle2 = registry.load(FontAsset::file("assets/Roboto-Regular.ttf")).await?;

    Ok(())
}
```

## Architecture

### Three-Layer Design

```
AssetRegistry (Global)
    ↓
AssetCache<T> (Per Type) - Moka TinyLFU cache
    ↓
AssetHandle<T, K> (Arc) - Smart handles with weak references
```

### Type State Builder

The registry uses a type-state builder for compile-time validation:

```rust
use flui_assets::AssetRegistryBuilder;

// ✅ This compiles
let registry = AssetRegistryBuilder::new()
    .with_capacity(10 * 1024 * 1024)
    .build();

// ❌ This doesn't compile - cannot build without capacity
// let registry = AssetRegistryBuilder::new().build();
```

### Extension Traits

Convenience methods without bloating core API:

```rust
use flui_assets::{AssetHandle, AssetHandleExt, AssetCache, AssetCacheExt};

let handle = registry.load(font).await?;

// Handle extensions
if handle.is_unique() {
    println!("Only reference!");
}
let size = handle.map(|font| font.bytes.len());
println!("Total refs: {}", handle.total_ref_count());

// Cache extensions
let cache: AssetCache<FontAsset> = AssetCache::new(1024 * 1024);
println!("Hit rate: {:.1}%", cache.hit_rate() * 100.0);
if cache.is_efficient() {
    println!("Cache performing well (>70% hit rate)");
}
```

## Asset Types

### Fonts (Built-in)

```rust
use flui_assets::{AssetRegistry, FontAsset};

let registry = AssetRegistry::global();
let font = FontAsset::file("fonts/Roboto-Regular.ttf");
let handle = registry.load(font).await?;

// Or from bytes
let bytes = std::fs::read("font.ttf")?;
let font = FontAsset::from_bytes(bytes);
let handle = registry.load(font).await?;
```

### Images (Optional)

Requires `images` feature flag:

```toml
[dependencies]
flui_assets = { path = "../flui_assets", features = ["images"] }
```

```rust
use flui_assets::{AssetRegistry, ImageAsset};

let registry = AssetRegistry::global();
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

### Memory Loader

```rust
use flui_assets::MemoryLoader;

let loader = MemoryLoader::new();
loader.insert(AssetKey::new("data"), vec![1, 2, 3, 4, 5]);
let data = loader.load(&AssetKey::new("data")).await?;
```

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `images` | Enable image loading (PNG, JPEG, GIF, WebP) | No |
| `serde` | Enable serde serialization (bundles, manifests) | No |
| `network` | Enable HTTP/HTTPS asset loading | No |
| `full` | Enable all stable features | No |

## Performance Characteristics

### Memory Efficiency
- **AssetKey**: 4 bytes (vs 24+ for `String`)
- **AssetHandle**: 8 bytes (single `Arc` pointer)
- **Option<ElementId>**: 8 bytes (niche optimization)

### Cache Performance
- **Insert**: O(1) amortized - Lock-free with Moka
- **Get**: O(1) expected - Hash table lookup
- **Eviction**: O(1) amortized - TinyLFU admission policy

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
    Err(AssetError::InvalidFormat(msg)) => eprintln!("Invalid: {}", msg),
    Err(AssetError::NotFound(key)) => eprintln!("Not found: {}", key),
    Err(e) => eprintln!("Error: {}", e),
}
```

## Testing

```bash
# Run all tests
cargo test -p flui_assets

# Run with all features
cargo test -p flui_assets --all-features

# Check documentation
cargo doc -p flui_assets --open
```

## Examples

```bash
# Basic usage
cargo run -p flui_assets --example basic_usage

# With images (requires 'images' feature)
cargo run -p flui_assets --example basic_usage --features images
```

## API Compliance

This crate achieves **96% compliance** with Rust API Guidelines (106/110 points).

See [API_GUIDELINES_AUDIT.md](API_GUIDELINES_AUDIT.md) for detailed compliance report.

## Documentation

### Quick Links

- **[User Guide](docs/GUIDE.md)** - Complete guide to using flui_assets
- **[Architecture](docs/ARCHITECTURE.md)** - System internals and design
- **[Design Patterns](docs/PATTERNS.md)** - Patterns used and why
- **[Performance](docs/PERFORMANCE.md)** - Optimization techniques

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT License ([LICENSE-MIT](../../LICENSE-MIT))

at your option.

## Related Crates

- [`flui_types`](../flui_types) - Core types for FLUI
- [`flui_core`](../flui_core) - FLUI framework core
- [`flui_painting`](../flui_painting) - 2D graphics API
