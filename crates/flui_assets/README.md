# flui_assets

[![Crates.io](https://img.shields.io/crates/v/flui_assets)](https://crates.io/crates/flui_assets)
[![Documentation](https://docs.rs/flui_assets/badge.svg)](https://docs.rs/flui_assets)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](https://github.com/flui-org/flui)

**High-performance asset management system for FLUI framework - Load, cache, and manage images, fonts, and other resources efficiently.**

FLUI Assets provides a comprehensive asset management solution designed for modern UI frameworks. It features high-performance caching, async I/O, multiple loader backends, and a clean type-safe API that makes working with assets both efficient and pleasant.

## Features

- 🚀 **High-Performance Caching** - Moka-based cache with TinyLFU eviction algorithm
- 🔑 **Interned Keys** - 4-byte asset keys using lasso for fast hashing and comparison
- 📦 **Multiple Loaders** - File, memory, network, and bundle support
- ⚡ **Async I/O** - Non-blocking loading with tokio integration
- 🔒 **Thread-Safe** - Arc-based handles with weak references for memory efficiency
- 🎯 **Type-Safe API** - Generic Asset<T> trait for extensible asset types
- 🌐 **Network Loading** - HTTP/HTTPS asset loading with caching (optional)
- 📊 **Asset Bundles** - Efficient asset bundling and manifest support (optional)
- 🔄 **Hot Reload** - File watching for development workflows (optional)
- 📈 **Performance Monitoring** - Built-in metrics and cache statistics

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
flui_assets = { version = "0.1", features = ["images"] }
```

### Basic Usage

```rust
use flui_assets::{AssetRegistry, ImageAsset};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get the global registry
    let registry = AssetRegistry::global();

    // Load an image
    let image = ImageAsset::file("assets/logo.png");
    let handle = registry.load(image).await?;
    
    println!("Loaded image: {}x{}", handle.width(), handle.height());
    
    // Subsequent loads use the cache
    let image2 = ImageAsset::file("assets/logo.png");
    let handle2 = registry.load(image2).await?; // Cache hit!
    
    Ok(())
}
```

## Core Concepts

### Asset Registry

The `AssetRegistry` is the central hub for all asset loading and caching:

```rust
use flui_assets::{AssetRegistry, RegistryConfig};

// Use the global registry (recommended)
let registry = AssetRegistry::global();

// Or create a custom registry
let registry = AssetRegistry::new(
    RegistryConfig::new()
        .cache_size(200 * 1024 * 1024) // 200MB cache
        .max_concurrent_loads(16)
        .enable_metrics(true)
);
```

### Asset Keys

Assets are identified by interned keys for performance:

```rust
use flui_assets::{AssetKey, ImageAsset};

// Keys are automatically created from paths
let image = ImageAsset::file("textures/grass.png");
let key = image.key(); // AssetKey is only 4 bytes!

// Keys can be reused efficiently
let key2 = AssetKey::new("textures/grass.png");
assert_eq!(key, key2); // Same interned string
```

### Asset Handles

Assets are returned as `Arc`-based handles for efficient sharing:

```rust
use flui_assets::{AssetRegistry, ImageAsset, Handle};

let registry = AssetRegistry::global();
let image = ImageAsset::file("logo.png");
let handle: Handle<ImageData> = registry.load(image).await?;

// Handles are cheap to clone
let handle2 = handle.clone(); // Just an Arc clone

// Access the loaded data
println!("Image format: {:?}", handle.format());
println!("Image size: {}x{}", handle.width(), handle.height());

// Get weak reference to avoid keeping assets alive
let weak_handle = handle.downgrade();
```

## Asset Types

### Images

```toml
[dependencies]
flui_assets = { version = "0.1", features = ["images"] }
```

```rust
use flui_assets::{AssetRegistry, ImageAsset, ImageFormat};

let registry = AssetRegistry::global();

// Load from file
let image = ImageAsset::file("photo.jpg");
let handle = registry.load(image).await?;

// Load from memory
let bytes = std::fs::read("photo.jpg")?;
let image = ImageAsset::memory(bytes);
let handle = registry.load(image).await?;

// Load from network (requires "network" feature)
let image = ImageAsset::url("https://example.com/image.png");
let handle = registry.load(image).await?;

// Access image data
match handle.format() {
    ImageFormat::Png => println!("PNG image"),
    ImageFormat::Jpeg => println!("JPEG image"),
    ImageFormat::Webp => println!("WebP image"),
    _ => println!("Other format"),
}

let pixels = handle.rgba_pixels(); // Convert to RGBA
let texture_data = handle.as_bytes(); // Raw bytes
```

### Fonts

```rust
use flui_assets::{AssetRegistry, FontAsset};

let registry = AssetRegistry::global();

// Load font file
let font = FontAsset::file("fonts/Roboto-Regular.ttf");
let handle = registry.load(font).await?;

println!("Font family: {}", handle.family_name());
println!("Font weight: {:?}", handle.weight());
println!("Glyph count: {}", handle.glyph_count());

// Get font data for rendering
let font_data = handle.font_data();
```

### Custom Asset Types

Create your own asset types by implementing the `Asset` trait:

```rust
use flui_assets::{Asset, AssetKey, AssetError, AssetMetadata};

#[derive(Debug)]
pub struct AudioAsset {
    path: String,
}

impl AudioAsset {
    pub fn file(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}

#[derive(Debug, Clone)]
pub struct AudioData {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
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
        
        // Decode audio file (pseudo-code)
        let decoded = decode_audio_file(&bytes)?;
        
        Ok(AudioData {
            samples: decoded.samples,
            sample_rate: decoded.sample_rate,
            channels: decoded.channels,
        })
    }

    fn metadata(&self) -> Option<AssetMetadata> {
        Some(AssetMetadata {
            format: Some("Audio".to_string()),
            estimated_size: None, // Unknown until loaded
        })
    }
}

// Use your custom asset
let registry = AssetRegistry::global();
let audio = AudioAsset::file("sounds/click.wav");
let handle = registry.load(audio).await?;
println!("Audio: {} samples at {}Hz", handle.samples.len(), handle.sample_rate);
```

## Loaders

### File Loader

Load assets from the filesystem:

```rust
use flui_assets::loaders::{FileLoader, LoaderExt};

let loader = FileLoader::new("assets/");

// Load raw bytes
let bytes = loader.load_bytes("image.png").await?;

// Load string content
let text = loader.load_string("config.json").await?;

// Load with automatic decompression
let data = loader.load_compressed("data.gz").await?;
```

### Memory Loader

Load assets from in-memory storage:

```rust
use flui_assets::loaders::MemoryLoader;

let mut loader = MemoryLoader::new();

// Insert data
loader.insert(AssetKey::new("config"), br#"{"debug": true}"#.to_vec());
loader.insert(AssetKey::new("texture"), image_bytes);

// Load data
let config_bytes = loader.load_bytes(&AssetKey::new("config")).await?;
let texture_bytes = loader.load_bytes(&AssetKey::new("texture")).await?;
```

### Network Loader

Load assets over HTTP/HTTPS (requires `network` feature):

```rust
use flui_assets::loaders::{NetworkLoader, NetworkConfig};

let loader = NetworkLoader::new(
    NetworkConfig::new()
        .timeout(std::time::Duration::from_secs(30))
        .retry_attempts(3)
        .user_agent("MyApp/1.0")
);

// Load from URL
let bytes = loader.load_bytes("https://example.com/asset.png").await?;

// Load with caching headers
let cached_bytes = loader.load_cached("https://example.com/data.json").await?;
```

## Asset Bundles

Bundle multiple assets for efficient distribution (requires `bundles` feature):

```toml
[dependencies]
flui_assets = { version = "0.1", features = ["bundles"] }
```

### Creating Bundles

```rust
use flui_assets::{Bundle, BundleBuilder, Compression};

// Create a bundle
let bundle = BundleBuilder::new()
    .add_file("textures/grass.png", "assets/textures/grass.png")?
    .add_file("sounds/click.wav", "assets/sounds/click.wav")?
    .add_bytes("config.json", br#"{"version": "1.0"}"#)
    .compression(Compression::Zstd)
    .build()?;

// Save bundle
bundle.save("game_assets.bundle").await?;

// Or get as bytes
let bundle_bytes = bundle.to_bytes()?;
```

### Loading from Bundles

```rust
use flui_assets::{Bundle, BundleLoader};

// Load bundle
let bundle = Bundle::load("game_assets.bundle").await?;
let loader = BundleLoader::new(bundle);

// Load assets from bundle
let grass_texture = loader.load_bytes("textures/grass.png").await?;
let click_sound = loader.load_bytes("sounds/click.wav").await?;
let config = loader.load_string("config.json").await?;
```

## Hot Reload

Monitor files for changes during development (requires `hot-reload` feature):

```toml
[dependencies]
flui_assets = { version = "0.1", features = ["hot-reload"] }
```

```rust
use flui_assets::{AssetRegistry, HotReloadConfig};

let registry = AssetRegistry::new(
    RegistryConfig::new()
        .hot_reload(
            HotReloadConfig::new()
                .watch_directory("assets/")
                .debounce(std::time::Duration::from_millis(100))
                .on_changed(|key| {
                    println!("Asset changed: {:?}", key);
                })
        )
);

// Assets will automatically reload when files change
let image = ImageAsset::file("logo.png");
let handle = registry.load(image).await?;
// File changes will trigger automatic reloading
```

## Performance

### Cache Configuration

```rust
use flui_assets::{AssetRegistry, RegistryConfig, CacheConfig};

let registry = AssetRegistry::new(
    RegistryConfig::new()
        .cache(
            CacheConfig::new()
                .max_capacity(500 * 1024 * 1024) // 500MB
                .time_to_live(std::time::Duration::from_secs(3600)) // 1 hour
                .time_to_idle(std::time::Duration::from_secs(300))  // 5 minutes
        )
);
```

### Performance Monitoring

```rust
use flui_assets::AssetRegistry;

let registry = AssetRegistry::global();

// Get cache statistics
let stats = registry.cache_stats();
println!("Cache hit rate: {:.2}%", stats.hit_rate() * 100.0);
println!("Total loads: {}", stats.total_loads());
println!("Cache size: {} MB", stats.cache_size() / 1024 / 1024);

// Get load metrics
let metrics = registry.load_metrics();
println!("Average load time: {:?}", metrics.average_load_time());
println!("Failed loads: {}", metrics.failed_loads());
```

### Memory Management

```rust
use flui_assets::{AssetRegistry, Handle};

let registry = AssetRegistry::global();
let handle: Handle<ImageData> = registry.load(ImageAsset::file("big_image.png")).await?;

// Convert to weak reference to allow cache eviction
let weak_handle = handle.downgrade();
drop(handle); // Release strong reference

// Later, try to upgrade
if let Some(strong_handle) = weak_handle.upgrade() {
    println!("Asset still in cache");
} else {
    println!("Asset was evicted, need to reload");
}
```

## Integration Examples

### With FLUI Widgets

```rust
use flui_assets::{AssetRegistry, ImageAsset};
use flui_widgets::{Image, AsyncImage};

// Synchronous image widget (for cached assets)
let image_widget = Image::new(
    registry.get_cached(&ImageAsset::file("logo.png").key())
        .expect("Image should be preloaded")
);

// Asynchronous image widget (loads on demand)
let async_image_widget = AsyncImage::new(ImageAsset::file("photo.jpg"))
    .placeholder(Image::new(placeholder_handle))
    .loading_indicator(LoadingSpinner::new());
```

### With FLUI Painting

```rust
use flui_assets::{AssetRegistry, ImageAsset, FontAsset};
use flui_painting::{Canvas, Paint};

let registry = AssetRegistry::global();

// Load assets
let image_handle = registry.load(ImageAsset::file("texture.png")).await?;
let font_handle = registry.load(FontAsset::file("font.ttf")).await?;

// Use in painting
let mut canvas = Canvas::new();
canvas.draw_image(&image_handle, Rect::from_size(Size::new(100.0, 100.0)));
canvas.draw_text("Hello", Point::new(10.0, 50.0), &Paint::new(), &font_handle);
```

## Feature Flags

```toml
[dependencies]
flui_assets = { version = "0.1", features = [
    "images",      # Image loading (PNG, JPEG, WebP, etc.)
    "bundles",     # Asset bundling and manifest support
    "network",     # Network-based asset loading via HTTP
    "hot-reload",  # File watching for development
    "serde",       # Serialization support for bundles
] }
```

## Advanced Usage

### Custom Cache Policies

```rust
use flui_assets::{AssetRegistry, CachePolicy, EvictionPolicy};

let registry = AssetRegistry::new(
    RegistryConfig::new()
        .cache_policy(CachePolicy::Custom {
            eviction: EvictionPolicy::LFU, // Least Frequently Used
            write_policy: WritePolicy::WriteBack,
            size_estimator: Box::new(|data| data.len() * 2), // Custom size calculation
        })
);
```

### Batch Loading

```rust
use flui_assets::{AssetRegistry, BatchLoader};

let registry = AssetRegistry::global();

// Load multiple assets concurrently
let batch = BatchLoader::new()
    .add(ImageAsset::file("image1.png"))
    .add(ImageAsset::file("image2.png"))
    .add(FontAsset::file("font.ttf"))
    .max_concurrent(8);

let results = registry.load_batch(batch).await?;

for result in results {
    match result {
        Ok(handle) => println!("Loaded: {:?}", handle.key()),
        Err(e) => println!("Failed to load: {}", e),
    }
}
```

### Asset Dependencies

```rust
use flui_assets::{Asset, AssetDependency};

#[derive(Debug)]
pub struct MaterialAsset {
    path: String,
}

impl Asset for MaterialAsset {
    type Data = MaterialData;
    
    fn dependencies(&self) -> Vec<AssetDependency> {
        vec![
            AssetDependency::new(ImageAsset::file("diffuse.png")),
            AssetDependency::new(ImageAsset::file("normal.png")),
            AssetDependency::new(ImageAsset::file("roughness.png")),
        ]
    }
    
    async fn load(&self) -> Result<MaterialData, AssetError> {
        // Dependencies are automatically loaded first
        let registry = AssetRegistry::global();
        let diffuse = registry.get_dependency::<ImageData>(0)?;
        let normal = registry.get_dependency::<ImageData>(1)?;
        let roughness = registry.get_dependency::<ImageData>(2)?;
        
        Ok(MaterialData {
            diffuse, normal, roughness
        })
    }
}
```

## Testing

```rust
use flui_assets::testing::{MockRegistry, MockAsset};

#[tokio::test]
async fn test_asset_loading() {
    let mut registry = MockRegistry::new();
    
    // Mock an asset
    registry.mock_asset(
        AssetKey::new("test.png"),
        MockAsset::success(ImageData::new(100, 100, vec![255; 40000]))
    );
    
    // Test loading
    let asset = ImageAsset::file("test.png");
    let handle = registry.load(asset).await?;
    
    assert_eq!(handle.width(), 100);
    assert_eq!(handle.height(), 100);
}

#[tokio::test]
async fn test_asset_failure() {
    let mut registry = MockRegistry::new();
    
    // Mock a failure
    registry.mock_asset(
        AssetKey::new("missing.png"),
        MockAsset::error(AssetError::NotFound)
    );
    
    let asset = ImageAsset::file("missing.png");
    let result = registry.load(asset).await;
    
    assert!(result.is_err());
}
```

## Performance Characteristics

- **Memory Efficient**: 4-byte asset keys, Arc-based sharing, weak references
- **Fast Access**: O(1) cache lookups with interned keys
- **Concurrent**: Lock-free cache with async I/O
- **Smart Eviction**: TinyLFU algorithm for better hit rates than LRU
- **Predictable**: Bounded memory usage with configurable limits

## Migration from Other Asset Systems

### From Manual Loading

```rust
// Old manual approach
let bytes = std::fs::read("asset.png")?;
let image = image::load_from_memory(&bytes)?;

// New FLUI Assets approach
let handle = AssetRegistry::global()
    .load(ImageAsset::file("asset.png"))
    .await?;
let image_data = handle.as_image(); // Automatic format detection and conversion
```

### From Basic Caching

```rust
// Old HashMap-based cache
static CACHE: Lazy<Mutex<HashMap<String, Arc<ImageData>>>> = 
    Lazy::new(|| Mutex::new(HashMap::new()));

let mut cache = CACHE.lock().unwrap();
if let Some(image) = cache.get(path) {
    image.clone()
} else {
    let image = load_image(path)?;
    cache.insert(path.to_string(), image.clone());
    image
}

// New FLUI Assets (automatic caching)
let handle = AssetRegistry::global()
    .load(ImageAsset::file(path))
    .await?; // Automatic caching, no manual management needed
```

## Contributing

We welcome contributions to FLUI Assets! See [CONTRIBUTING.md](../../CONTRIBUTING.md) for guidelines.

### Development

```bash
# Run tests
cargo test -p flui_assets

# Run with all features
cargo test -p flui_assets --all-features

# Run benchmarks
cargo bench -p flui_assets

# Check documentation
cargo doc -p flui_assets --open
```

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT License ([LICENSE-MIT](../../LICENSE-MIT))

at your option.

## Related Crates

- [`flui_types`](../flui_types) - Basic geometry and color types
- [`flui_painting`](../flui_painting) - 2D graphics and canvas API
- [`flui_widgets`](../flui_widgets) - UI widgets that consume assets
- [`flui_app`](../flui_app) - Application framework with asset integration

---

**FLUI Assets** - Fast, flexible, and memory-efficient asset management for modern UI frameworks.