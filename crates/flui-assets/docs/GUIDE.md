# User Guide

Complete guide to using `flui_assets` in your application.

## Table of Contents

1. [Quick Start](#quick-start)
2. [Basic Usage](#basic-usage)
3. [Asset Types](#asset-types)
4. [Advanced Features](#advanced-features)
5. [Best Practices](#best-practices)
6. [Troubleshooting](#troubleshooting)

## Quick Start

### Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
flui_assets = "0.1"
tokio = { version = "1.0", features = ["macros", "rt-multi-thread"] }

# Optional features
flui_assets = { version = "0.1", features = ["images", "serde"] }
```

### Your First Asset

```rust
use flui_assets::{AssetRegistry, FontAsset};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get global registry
    let registry = AssetRegistry::global();

    // Load a font
    let font = FontAsset::file("assets/Roboto-Regular.ttf");
    let handle = registry.load(font).await?;

    // Use the font
    println!("Font loaded: {} bytes", handle.bytes.len());

    Ok(())
}
```

## Basic Usage

### Loading Assets

#### Method 1: Using Global Registry (Recommended)

```rust
use flui_assets::AssetRegistry;

let registry = AssetRegistry::global();
let font = registry.load(FontAsset::file("font.ttf")).await?;
```

**Pros**:
- Simple API
- Shared cache across application
- No need to pass registry around

**Cons**:
- Global state (acceptable for most apps)

#### Method 2: Creating Custom Registry

```rust
use flui_assets::AssetRegistryBuilder;

// Create with specific capacity
let registry = AssetRegistryBuilder::new()
    .with_capacity(100 * 1024 * 1024)  // 100 MB cache
    .build();

let font = registry.load(FontAsset::file("font.ttf")).await?;
```

**Pros**:
- Control over cache size
- Multiple registries for different use cases
- Better for testing (isolated state)

**Cons**:
- Must pass registry explicitly
- Slightly more verbose

### Accessing Asset Data

```rust
// Method 1: Direct access (immutable reference)
let font = registry.load(FontAsset::file("font.ttf")).await?;
let bytes = &font.bytes;
println!("Font size: {} bytes", bytes.len());

// Method 2: Using AssetHandleExt methods
use flui_assets::AssetHandleExt;

let size = font.map(|f| f.bytes.len());
println!("Font size: {} bytes", size);

// Method 3: Deref to inner type
let font_data = &*font;  // Deref to &FontData
println!("Font: {:?}", font_data);
```

### Caching Behavior

Assets are automatically cached:

```rust
// First load: reads from disk
let font1 = registry.load(FontAsset::file("font.ttf")).await?;

// Second load: returns cached data (instant)
let font2 = registry.load(FontAsset::file("font.ttf")).await?;

// Both point to same data
assert!(Arc::ptr_eq(&font1.data, &font2.data));
```

### Cache Management

```rust
use flui_assets::AssetRegistry;

let registry = AssetRegistry::global();

// Invalidate specific asset
registry.invalidate::<FontAsset>(&AssetKey::new("old_font.ttf")).await;

// Clear all assets of type
registry.clear::<FontAsset>().await;

// Check if asset is cached
if let Some(cached) = registry.get::<FontAsset>(&key).await {
    println!("Asset in cache!");
}
```

## Asset Types

### Built-in: Fonts

Fonts are always available (no feature flag required):

```rust
use flui_assets::{AssetRegistry, FontAsset};

// Load from file
let font = FontAsset::file("assets/Roboto-Regular.ttf");
let handle = registry.load(font).await?;

// Load from bytes (e.g., embedded fonts)
let bytes = include_bytes!("../assets/font.ttf");
let font = FontAsset::from_bytes(bytes.to_vec());
let handle = registry.load(font).await?;

// Access font data
println!("Font bytes: {:?}", &handle.bytes[..10]);
```

### Optional: Images

Requires `images` feature flag:

```toml
[dependencies]
flui_assets = { version = "0.1", features = ["images"] }
```

```rust
use flui_assets::{AssetRegistry, ImageAsset};

// Load from file
let image = ImageAsset::file("assets/logo.png");
let handle = registry.load(image).await?;

// Access image data
println!("Image: {}x{}", handle.width(), handle.height());
println!("Format: {:?}", handle.format());

// Load from bytes
let bytes = std::fs::read("logo.png")?;
let image = ImageAsset::from_bytes(bytes);
let handle = registry.load(image).await?;
```

### Custom Asset Types

Create your own asset types by implementing the `Asset` trait:

```rust
use flui_assets::{Asset, AssetKey, AssetError, AssetMetadata};

// 1. Define your asset source
pub struct ConfigAsset {
    path: String,
}

impl ConfigAsset {
    pub fn file(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}

// 2. Define your asset data
#[derive(Debug, Clone)]
pub struct ConfigData {
    pub content: String,
}

// 3. Implement Asset trait
impl Asset for ConfigAsset {
    type Data = ConfigData;
    type Key = AssetKey;
    type Error = AssetError;

    fn key(&self) -> AssetKey {
        AssetKey::new(&self.path)
    }

    async fn load(&self) -> Result<ConfigData, AssetError> {
        let content = tokio::fs::read_to_string(&self.path)
            .await
            .map_err(|e| AssetError::Io(e))?;

        Ok(ConfigData { content })
    }

    fn metadata(&self) -> Option<AssetMetadata> {
        Some(AssetMetadata {
            format: Some("JSON".to_string()),
            ..Default::default()
        })
    }
}

// 4. Use it!
let config = ConfigAsset::file("config.json");
let handle = registry.load(config).await?;
println!("Config: {}", handle.content);
```

## Advanced Features

### Extension Traits

Get convenience methods automatically:

```rust
use flui_assets::{AssetHandle, AssetHandleExt};

let font = registry.load(FontAsset::file("font.ttf")).await?;

// Check if this is the only reference
if font.is_unique() {
    println!("Exclusive ownership");
}

// Count total references
println!("References: {}", font.total_ref_count());

// Transform data without cloning
let size = font.map(|f| f.bytes.len());
```

### Weak References

Avoid keeping assets alive unnecessarily:

```rust
use flui_assets::WeakAssetHandle;

// Create weak reference
let font = registry.load(FontAsset::file("font.ttf")).await?;
let weak: WeakAssetHandle<_, _> = font.downgrade();

// Drop strong reference
drop(font);

// Asset can now be evicted from cache

// Later, try to upgrade
match weak.upgrade() {
    Some(strong) => {
        // Asset still cached
        use_font(&strong);
    }
    None => {
        // Asset evicted, need to reload
        let strong = registry.load(FontAsset::file("font.ttf")).await?;
        use_font(&strong);
    }
}
```

**Use case**: Store weak references in UI elements, upgrade when rendering.

### Cache Statistics

Monitor cache performance:

```rust
use flui_assets::{AssetCache, AssetCacheExt, FontAsset};

// Get cache for specific asset type
let cache: AssetCache<FontAsset> = registry.get_cache().unwrap();

// Get statistics
println!("Hit rate: {:.1}%", cache.hit_rate() * 100.0);
println!("Miss rate: {:.1}%", cache.miss_rate() * 100.0);

// Check efficiency
if cache.is_efficient() {
    println!("Cache performing well (>70% hit rate)");
} else {
    println!("Consider increasing cache size");
}

// Get detailed stats
let stats = cache.stats();
println!("Hits: {}, Misses: {}", stats.hits, stats.misses);
println!("Entries: {}", cache.len());
```

### Loaders

Use loaders for different data sources:

#### File Loader

```rust
use flui_assets::BytesFileLoader;

let loader = BytesFileLoader::new("assets");

// Load raw bytes
let bytes = loader.load_bytes("texture.png").await?;

// Load as string
let text = loader.load_string("config.json").await?;
```

#### Memory Loader

```rust
use flui_assets::{MemoryLoader, AssetKey};

let loader = MemoryLoader::new();

// Insert data
loader.insert(AssetKey::new("data"), vec![1, 2, 3, 4]);

// Check if exists
assert!(loader.contains(&AssetKey::new("data")));

// Get length
assert_eq!(loader.len(), 1);
```

**Use case**: Testing, embedded assets, hot-reload.

### Parallel Loading

Load multiple assets concurrently:

```rust
use futures::future::join_all;

let registry = AssetRegistry::global();

// Load fonts in parallel
let handles = (0..10)
    .map(|i| {
        let registry = registry.clone();
        async move {
            registry.load(FontAsset::file(&format!("font{}.ttf", i))).await
        }
    })
    .collect::<Vec<_>>();

// Wait for all
let results = join_all(handles).await;

// Process results
for result in results {
    match result {
        Ok(handle) => println!("Loaded: {:?}", handle.key()),
        Err(e) => eprintln!("Failed: {}", e),
    }
}
```

### Error Handling

```rust
use flui_assets::AssetError;

match registry.load(font).await {
    Ok(handle) => {
        println!("Loaded successfully");
    }
    Err(AssetError::Io(e)) => {
        eprintln!("I/O error: {}", e);
    }
    Err(AssetError::InvalidFormat(msg)) => {
        eprintln!("Invalid format: {}", msg);
    }
    Err(AssetError::NotFound { path }) => {
        eprintln!("Not found: {}", path);
    }
    Err(e) => {
        eprintln!("Other error: {}", e);
    }
}
```

## Best Practices

### 1. Use Global Registry for Simple Apps

```rust
// ✅ Good: Simple and clean
let registry = AssetRegistry::global();
let font = registry.load(FontAsset::file("font.ttf")).await?;
```

### 2. Preload Critical Assets

```rust
// Preload at startup
async fn preload_assets(registry: &AssetRegistry) -> Result<()> {
    let critical = vec![
        FontAsset::file("ui_font.ttf"),
        ImageAsset::file("logo.png"),
    ];

    for asset in critical {
        registry.preload(asset).await?;
    }

    Ok(())
}
```

### 3. Use Weak References in UI

```rust
struct Button {
    font: WeakAssetHandle<FontData, AssetKey>,
}

impl Button {
    fn render(&self, registry: &AssetRegistry) {
        if let Some(font) = self.font.upgrade() {
            // Render with cached font
            draw_text(&font);
        } else {
            // Reload if evicted
            let font = registry.load(FontAsset::file("button_font.ttf")).await;
            draw_text(&font);
        }
    }
}
```

### 4. Monitor Cache Performance

```rust
// In development/testing
#[cfg(debug_assertions)]
fn check_cache_performance(registry: &AssetRegistry) {
    let cache: AssetCache<ImageAsset> = registry.get_cache().unwrap();

    if cache.hit_rate() < 0.7 {
        eprintln!("Warning: Low cache hit rate: {:.1}%", cache.hit_rate() * 100.0);
        eprintln!("Consider increasing cache size");
    }
}
```

### 5. Handle Errors Gracefully

```rust
// Provide fallback for missing assets
async fn load_font_with_fallback(
    registry: &AssetRegistry,
    path: &str,
) -> AssetHandle<FontData, AssetKey> {
    match registry.load(FontAsset::file(path)).await {
        Ok(handle) => handle,
        Err(_) => {
            // Load default font
            registry.load(FontAsset::file("default.ttf"))
                .await
                .expect("Default font must exist")
        }
    }
}
```

### 6. Use Type Aliases

```rust
// Define convenient type aliases
type FontHandle = AssetHandle<FontData, AssetKey>;
type ImageHandle = AssetHandle<ImageData, AssetKey>;

fn process_font(font: FontHandle) {
    // ...
}
```

## Troubleshooting

### Issue: Asset Not Found

**Error**: `AssetError::NotFound { path: "font.ttf" }`

**Solutions**:
1. Check file path is correct
2. Verify file exists: `ls assets/font.ttf`
3. Check working directory: `println!("{:?}", std::env::current_dir())`
4. Use absolute path for testing

### Issue: High Memory Usage

**Symptoms**: Application using too much memory

**Solutions**:
1. Reduce cache size in registry builder
2. Use weak references in long-lived structures
3. Clear unused asset types periodically
4. Profile memory usage with `cargo flamegraph`

### Issue: Low Cache Hit Rate

**Symptoms**: `cache.hit_rate() < 0.7`

**Solutions**:
1. Increase cache capacity
2. Preload frequently-used assets
3. Check if assets are being loaded with different keys
4. Profile access patterns

### Issue: Slow Startup

**Symptoms**: Long initial load time

**Solutions**:
1. Use lazy loading (load on demand)
2. Show loading screen with progress
3. Preload in background task
4. Profile with `cargo flamegraph`

### Issue: Type Mismatch

**Error**: Type mismatch when downcasting

**Solutions**:
1. Ensure using correct asset type for cache
2. Check TypeId matches
3. Verify Asset::Data type is correct

## Feature Flags

### Available Features

| Feature | Description | Dependencies |
|---------|-------------|--------------|
| `serde` | Enable serde serialization | `serde`, `serde_json` |
| `images` | Enable image loading | `image` |
| `network` | Enable HTTP/HTTPS loading | `reqwest` |
| `full` | Enable all stable features | All above |

### Enabling Features

```toml
# Single feature
flui_assets = { version = "0.1", features = ["images"] }

# Multiple features
flui_assets = { version = "0.1", features = ["images", "serde"] }

# All features
flui_assets = { version = "0.1", features = ["full"] }
```

## Examples

### Example 1: Loading Multiple Font Families

```rust
use flui_assets::{AssetRegistry, FontAsset};
use std::collections::HashMap;

async fn load_font_families(
    registry: &AssetRegistry
) -> Result<HashMap<String, AssetHandle<FontData, AssetKey>>> {
    let fonts = vec![
        ("regular", "Roboto-Regular.ttf"),
        ("bold", "Roboto-Bold.ttf"),
        ("italic", "Roboto-Italic.ttf"),
    ];

    let mut handles = HashMap::new();

    for (name, path) in fonts {
        let font = FontAsset::file(path);
        let handle = registry.load(font).await?;
        handles.insert(name.to_string(), handle);
    }

    Ok(handles)
}
```

### Example 2: Asset Loading Progress

```rust
use flui_assets::{AssetRegistry, FontAsset};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

async fn load_with_progress(
    registry: &AssetRegistry,
    paths: Vec<String>,
) -> Result<Vec<AssetHandle<FontData, AssetKey>>> {
    let total = paths.len();
    let loaded = Arc::new(AtomicUsize::new(0));

    let handles = futures::future::join_all(
        paths.into_iter().map(|path| {
            let registry = registry.clone();
            let loaded = loaded.clone();
            async move {
                let font = FontAsset::file(&path);
                let result = registry.load(font).await;

                let count = loaded.fetch_add(1, Ordering::Relaxed) + 1;
                println!("Progress: {}/{}", count, total);

                result
            }
        })
    ).await;

    handles.into_iter().collect()
}
```

### Example 3: Custom Asset with Validation

```rust
use flui_assets::{Asset, AssetKey, AssetError, AssetMetadata};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct GameConfig {
    pub title: String,
    pub version: String,
}

pub struct GameConfigAsset {
    path: String,
}

impl Asset for GameConfigAsset {
    type Data = GameConfig;
    type Key = AssetKey;
    type Error = AssetError;

    fn key(&self) -> AssetKey {
        AssetKey::new(&self.path)
    }

    async fn load(&self) -> Result<GameConfig, AssetError> {
        let content = tokio::fs::read_to_string(&self.path)
            .await
            .map_err(|e| AssetError::Io(e))?;

        let config: GameConfig = serde_json::from_str(&content)
            .map_err(|e| AssetError::InvalidFormat(e.to_string()))?;

        // Validate
        if config.title.is_empty() {
            return Err(AssetError::InvalidFormat("Title cannot be empty".into()));
        }

        Ok(config)
    }

    fn metadata(&self) -> Option<AssetMetadata> {
        Some(AssetMetadata {
            format: Some("JSON".to_string()),
            ..Default::default()
        })
    }
}
```

## Next Steps

- Read [ARCHITECTURE.md](ARCHITECTURE.md) for system internals
- Read [PATTERNS.md](PATTERNS.md) for design patterns
- Read [PERFORMANCE.md](PERFORMANCE.md) for optimization tips
- Check [API documentation](https://docs.rs/flui_assets) for complete reference
