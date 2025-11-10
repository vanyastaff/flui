# FLUI Assets Architecture

**Version:** 0.1.0
**Date:** 2025-11-10
**Author:** Claude (Anthropic)
**Status:** Design Proposal

---

## Executive Summary

This document defines the complete architecture for FLUI's asset management system (`flui-assets`), based on Flutter's AssetBundle, ImageProvider, and asset loading patterns. The system provides **high-performance**, **type-safe**, **extensible** asset loading with automatic caching.

**Key Design Principles:**
1. **Flutter-Compatible API**: AssetBundle, ImageProvider pattern with DefaultAssetBundle
2. **Type-Safe Assets**: Generic `Asset<T>` trait for any asset type
3. **High-Performance Caching**: Moka-based cache with TinyLFU eviction (lock-free)
4. **Resolution Awareness**: Automatic selection of 1x, 2x, 3x assets based on device pixel ratio
5. **Multiple Sources**: File, memory, network, bundle loaders
6. **Async I/O**: Non-blocking loading with tokio
7. **Manifest-Based Bundling**: Production asset bundles (like Flutter's AssetManifest.json)

**Current Implementation Status:**
- ✅ Core traits (`Asset<T>`, `AssetLoader<T>`)
- ✅ Type system (`AssetKey`, `AssetHandle`)
- ✅ Caching (`AssetCache` with Moka)
- ✅ Loaders (File, Memory, Network)
- ✅ Concrete assets (Image, Font)
- ✅ Registry (`AssetRegistry`)
- ⏳ TODO: Asset bundles (manifest-based)
- ⏳ TODO: Hot reload (file watching)
- ⏳ TODO: Resolution-aware loading (1x/2x/3x)
- ⏳ TODO: ImageProvider integration
- ⏳ TODO: Video asset support

**Total Work Estimate:** ~1,500 LOC remaining (bundles + resolution awareness + ImageProvider + video)

---

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Flutter Patterns in FLUI](#flutter-patterns-in-flui)
3. [Core Asset System](#core-asset-system)
4. [AssetBundle (Flutter Pattern)](#assetbundle-flutter-pattern)
5. [ImageProvider (Flutter Pattern)](#imageprovider-flutter-pattern)
6. [Resolution-Aware Assets](#resolution-aware-assets)
7. [Asset Manifest & Bundling](#asset-manifest--bundling)
8. [Video Assets](#video-assets)
9. [Hot Reload](#hot-reload)
10. [Implementation Plan](#implementation-plan)
11. [Usage Examples](#usage-examples)
12. [Testing Strategy](#testing-strategy)

---

## Architecture Overview

### Layered Architecture

```text
┌─────────────────────────────────────────────────────────────┐
│                       flui_widgets                          │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  Image widget, DefaultAssetBundle                    │   │
│  │  AssetImage, NetworkImage, FileImage                 │   │
│  │  VideoPlayer, AudioPlayer widgets                    │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                          ↓ uses
┌─────────────────────────────────────────────────────────────┐
│                      flui_assets                            │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  Core: Asset<T>, AssetLoader<T>, AssetBundle         │   │
│  │  Registry: AssetRegistry, DefaultAssetBundle         │   │
│  │  Cache: AssetCache (Moka-based)                      │   │
│  │  Assets: ImageAsset, FontAsset, VideoAsset           │   │
│  │  Loaders: FileLoader, NetworkLoader, BundleLoader    │   │
│  │  Providers: ImageProvider, VideoPlayerController     │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                          ↓ uses
┌─────────────────────────────────────────────────────────────┐
│                     flui_types                              │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  Image (pixel data), Size, Color                     │   │
│  │  AssetMetadata, DevicePixelRatio                     │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

### Asset Loading Flow

```text
Widget (Image.asset("logo.png"))
    ↓
DefaultAssetBundle.of(context)
    ↓
AssetBundle.load("logo.png")
    ↓
Resolution Aware ("logo.png" → "2.0x/logo.png" for 2x device)
    ↓
AssetRegistry.load(ImageAsset)
    ↓
Check Cache (Moka)
    ↓ (cache miss)
FileLoader.load_bytes("2.0x/logo.png")
    ↓
Decode Image (async)
    ↓
Store in Cache
    ↓
Return AssetHandle<Image>
```

---

## Flutter Patterns in FLUI

### 1. AssetBundle Pattern

**Flutter:**
```dart
// Global rootBundle
import 'package:flutter/services.dart' show rootBundle;

Future<String> loadAsset() async {
  return await rootBundle.loadString('assets/config.json');
}

// Preferred: Use DefaultAssetBundle
Future<String> loadAssetContext(BuildContext context) async {
  return await DefaultAssetBundle.of(context).loadString('assets/config.json');
}
```

**FLUI:**
```rust
// Global AssetRegistry
use flui_assets::AssetRegistry;

async fn load_asset() -> Result<String, AssetError> {
    let registry = AssetRegistry::global();
    registry.load_string("assets/config.json").await
}

// Preferred: Use DefaultAssetBundle from context
async fn load_asset_context(ctx: &BuildContext) -> Result<String, AssetError> {
    let bundle = DefaultAssetBundle::of(ctx);
    bundle.load_string("assets/config.json").await
}
```

### 2. ImageProvider Pattern

**Flutter:**
```dart
// AssetImage (resolution-aware)
Image(
  image: AssetImage('assets/logo.png'),
)

// NetworkImage (with caching)
Image(
  image: NetworkImage('https://example.com/logo.png'),
)

// CachedNetworkImage
CachedNetworkImage(
  imageUrl: 'https://example.com/logo.png',
  placeholder: (context, url) => CircularProgressIndicator(),
  errorWidget: (context, url, error) => Icon(Icons.error),
)
```

**FLUI:**
```rust
// AssetImage (resolution-aware)
Image::new(
    AssetImage::new("assets/logo.png"),
)

// NetworkImage (with caching)
Image::new(
    NetworkImage::new("https://example.com/logo.png"),
)

// CachedNetworkImage
CachedNetworkImage::new("https://example.com/logo.png")
    .placeholder(|| CircularProgressIndicator::new())
    .error_widget(|| Icon::new("error"))
```

### 3. Resolution-Aware Assets

**Flutter Directory Structure:**
```
assets/
  images/
    logo.png        # 1x (baseline)
    2.0x/
      logo.png      # 2x (high DPI)
    3.0x/
      logo.png      # 3x (extra high DPI)
```

**pubspec.yaml:**
```yaml
flutter:
  assets:
    - assets/images/logo.png
    # Flutter automatically includes variant directories
```

**FLUI Manifest (asset_manifest.toml):**
```toml
[[assets]]
key = "assets/images/logo.png"
variants = [
    { ratio = 1.0, path = "assets/images/logo.png" },
    { ratio = 2.0, path = "assets/images/2.0x/logo.png" },
    { ratio = 3.0, path = "assets/images/3.0x/logo.png" },
]
```

### 4. Font Loading

**Flutter (pubspec.yaml):**
```yaml
flutter:
  fonts:
    - family: Roboto
      fonts:
        - asset: fonts/Roboto-Regular.ttf
        - asset: fonts/Roboto-Bold.ttf
          weight: 700
        - asset: fonts/Roboto-Italic.ttf
          style: italic
```

**FLUI (asset_manifest.toml):**
```toml
[[fonts]]
family = "Roboto"
fonts = [
    { asset = "fonts/Roboto-Regular.ttf", weight = 400, style = "normal" },
    { asset = "fonts/Roboto-Bold.ttf", weight = 700, style = "normal" },
    { asset = "fonts/Roboto-Italic.ttf", weight = 400, style = "italic" },
]
```

---

## Core Asset System

### Current Implementation (Existing in flui-assets)

```rust
// In flui_assets/src/core.rs

/// Generic asset trait for type-safe loading
pub trait Asset: Send + Sync + 'static {
    /// The loaded data type
    type Data: Send + Sync + 'static;

    /// Key type for caching
    type Key: AsRef<str> + Send + Sync + 'static;

    /// Error type
    type Error: std::error::Error + Send + Sync + 'static;

    /// Get the cache key for this asset
    fn key(&self) -> Self::Key;

    /// Load the asset (async)
    async fn load(&self) -> Result<Self::Data, Self::Error>;

    /// Optional metadata
    fn metadata(&self) -> Option<AssetMetadata> {
        None
    }
}

/// Asset metadata
#[derive(Debug, Clone)]
pub struct AssetMetadata {
    /// Asset size in bytes (if known)
    pub size: Option<usize>,

    /// MIME type (if known)
    pub mime_type: Option<String>,

    /// Format (e.g., "PNG", "TTF", "MP4")
    pub format: Option<String>,

    /// Resolution scale (e.g., 1.0, 2.0, 3.0)
    pub scale: Option<f64>,
}
```

**Key Design:**
- ✅ Generic `Asset<T>` trait
- ✅ Type-safe data loading
- ✅ Extensible for any asset type
- ✅ Async-first with tokio

---

## AssetBundle (Flutter Pattern)

### AssetBundle Trait

```rust
// In flui_assets/src/bundle.rs

/// Flutter-compatible AssetBundle interface
///
/// AssetBundle provides access to bundled assets at runtime.
#[async_trait::async_trait]
pub trait AssetBundle: Send + Sync {
    /// Load a binary asset
    async fn load(&self, key: &str) -> Result<Vec<u8>, AssetError>;

    /// Load a string asset (UTF-8)
    async fn load_string(&self, key: &str) -> Result<String, AssetError> {
        let bytes = self.load(key).await?;
        String::from_utf8(bytes).map_err(|e| AssetError::InvalidUtf8(e))
    }

    /// Load a structured asset (JSON, TOML, etc.)
    async fn load_structured<T>(&self, key: &str) -> Result<T, AssetError>
    where
        T: serde::de::DeserializeOwned,
    {
        let string = self.load_string(key).await?;
        serde_json::from_str(&string).map_err(|e| AssetError::ParseError(e.to_string()))
    }

    /// Check if an asset exists
    async fn exists(&self, key: &str) -> bool;

    /// List all asset keys (optional)
    fn list(&self) -> Option<Vec<String>> {
        None
    }
}

/// Root asset bundle (global default)
pub struct RootAssetBundle {
    registry: Arc<AssetRegistry>,
    manifest: Arc<AssetManifest>,
}

impl RootAssetBundle {
    /// Get the global root bundle
    pub fn global() -> Arc<Self> {
        static ROOT_BUNDLE: OnceLock<Arc<RootAssetBundle>> = OnceLock::new();
        ROOT_BUNDLE
            .get_or_init(|| {
                Arc::new(Self {
                    registry: AssetRegistry::global(),
                    manifest: AssetManifest::load_default().unwrap(),
                })
            })
            .clone()
    }
}

#[async_trait::async_trait]
impl AssetBundle for RootAssetBundle {
    async fn load(&self, key: &str) -> Result<Vec<u8>, AssetError> {
        // Resolve variant based on device pixel ratio
        let resolved_key = self.manifest.resolve_variant(key, self.device_pixel_ratio());

        // Load via registry
        self.registry.load_bytes(&resolved_key).await
    }

    async fn exists(&self, key: &str) -> bool {
        self.manifest.contains_key(key)
    }

    fn list(&self) -> Option<Vec<String>> {
        Some(self.manifest.keys().cloned().collect())
    }
}

/// DefaultAssetBundle (for BuildContext)
pub struct DefaultAssetBundle {
    bundle: Arc<dyn AssetBundle>,
}

impl DefaultAssetBundle {
    /// Get the default asset bundle for a BuildContext
    pub fn of(ctx: &BuildContext) -> Arc<dyn AssetBundle> {
        // Try to get from context (allows overriding)
        if let Some(bundle) = ctx.get_inherited::<DefaultAssetBundle>() {
            return bundle.bundle.clone();
        }

        // Fall back to root bundle
        RootAssetBundle::global()
    }

    /// Create a widget that provides a custom AssetBundle
    pub fn new(bundle: Arc<dyn AssetBundle>, child: AnyElement) -> impl View {
        InheritedAssetBundle {
            bundle,
            child: Some(child),
        }
    }
}

// InheritedWidget for AssetBundle
#[derive(Debug)]
struct InheritedAssetBundle {
    bundle: Arc<dyn AssetBundle>,
    child: Option<AnyElement>,
}

impl Provider for InheritedAssetBundle {
    fn should_notify(&self, old: &Self) -> bool {
        !Arc::ptr_eq(&self.bundle, &old.bundle)
    }
}
```

**Usage:**

```rust
// In app setup
fn main() {
    // Default: Uses RootAssetBundle (reads from asset_manifest.toml)
    App::new()
        .child(MyApp::new())
        .run();
}

// In widget (preferred)
impl View for MyWidget {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let bundle = DefaultAssetBundle::of(ctx);

        // Load asset
        let future = bundle.load_string("assets/config.json");

        // Use in widget...
    }
}

// Override bundle for testing
impl View for TestApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        DefaultAssetBundle::new(
            Arc::new(MemoryAssetBundle::new()),  // Custom bundle
            Box::new(MyApp::new()),
        )
    }
}
```

---

## ImageProvider (Flutter Pattern)

### ImageProvider Trait

```rust
// In flui_assets/src/providers/image_provider.rs

/// Flutter-compatible ImageProvider
///
/// ImageProvider is responsible for obtaining and caching images.
#[async_trait::async_trait]
pub trait ImageProvider: Send + Sync + fmt::Debug {
    /// Unique key for this image (for caching)
    fn key(&self) -> ImageKey;

    /// Load the image
    async fn load(&self) -> Result<Image, ImageError>;

    /// Resolve to a concrete ImageStream
    fn resolve(&self, configuration: ImageConfiguration) -> ImageStream {
        ImageStream::new(self.key(), self.load(), configuration)
    }

    /// Evict from cache
    fn evict(&self) {
        ImageCache::global().evict(&self.key());
    }
}

/// Image configuration (device pixel ratio, size, etc.)
#[derive(Debug, Clone)]
pub struct ImageConfiguration {
    pub device_pixel_ratio: f64,
    pub size: Option<Size>,
    pub platform: TargetPlatform,
}

/// Unique key for image caching
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageKey(String);

/// ImageStream (reactive image loading)
pub struct ImageStream {
    key: ImageKey,
    future: Pin<Box<dyn Future<Output = Result<Image, ImageError>> + Send>>,
    configuration: ImageConfiguration,
}

impl ImageStream {
    pub fn new(
        key: ImageKey,
        future: impl Future<Output = Result<Image, ImageError>> + Send + 'static,
        configuration: ImageConfiguration,
    ) -> Self {
        Self {
            key,
            future: Box::pin(future),
            configuration,
        }
    }

    /// Add a listener for image load completion
    pub fn add_listener(&mut self, listener: ImageStreamListener) {
        // Implementation: Store listener, notify on completion
    }
}

pub type ImageStreamListener = Arc<dyn Fn(ImageInfo, bool) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub image: Image,
    pub scale: f64,
}

/// Global image cache
pub struct ImageCache {
    cache: Arc<Mutex<HashMap<ImageKey, Arc<Image>>>>,
    max_size: usize,
    current_size: AtomicUsize,
}

impl ImageCache {
    /// Get the global image cache
    pub fn global() -> &'static Self {
        static IMAGE_CACHE: OnceLock<ImageCache> = OnceLock::new();
        IMAGE_CACHE.get_or_init(|| ImageCache::new(100 * 1024 * 1024)) // 100 MB default
    }

    pub fn new(max_size: usize) -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            max_size,
            current_size: AtomicUsize::new(0),
        }
    }

    pub fn put(&self, key: ImageKey, image: Arc<Image>) {
        let image_size = image.byte_size();

        // Evict if necessary
        while self.current_size.load(Ordering::Relaxed) + image_size > self.max_size {
            self.evict_lru();
        }

        self.cache.lock().insert(key, image);
        self.current_size.fetch_add(image_size, Ordering::Relaxed);
    }

    pub fn get(&self, key: &ImageKey) -> Option<Arc<Image>> {
        self.cache.lock().get(key).cloned()
    }

    pub fn evict(&self, key: &ImageKey) {
        if let Some(image) = self.cache.lock().remove(key) {
            self.current_size
                .fetch_sub(image.byte_size(), Ordering::Relaxed);
        }
    }

    fn evict_lru(&self) {
        // TODO: Implement LRU eviction
    }

    pub fn clear(&self) {
        self.cache.lock().clear();
        self.current_size.store(0, Ordering::Relaxed);
    }
}
```

### Concrete ImageProvider Implementations

```rust
// In flui_assets/src/providers/asset_image.rs

/// AssetImage (resolution-aware)
#[derive(Debug, Clone)]
pub struct AssetImage {
    asset_name: String,
    bundle: Option<Arc<dyn AssetBundle>>,
    scale: Option<f64>,
}

impl AssetImage {
    pub fn new(asset_name: impl Into<String>) -> Self {
        Self {
            asset_name: asset_name.into(),
            bundle: None,
            scale: None,
        }
    }

    pub fn bundle(mut self, bundle: Arc<dyn AssetBundle>) -> Self {
        self.bundle = Some(bundle);
        self
    }

    pub fn scale(mut self, scale: f64) -> Self {
        self.scale = Some(scale);
        self
    }
}

#[async_trait::async_trait]
impl ImageProvider for AssetImage {
    fn key(&self) -> ImageKey {
        ImageKey(format!("asset://{}", self.asset_name))
    }

    async fn load(&self) -> Result<Image, ImageError> {
        // Check cache first
        let key = self.key();
        if let Some(cached) = ImageCache::global().get(&key) {
            return Ok((*cached).clone());
        }

        // Load via bundle
        let bundle = self
            .bundle
            .clone()
            .unwrap_or_else(|| RootAssetBundle::global());

        let bytes = bundle.load(&self.asset_name).await?;

        // Decode image
        let image = Image::from_bytes(&bytes)?;

        // Cache
        ImageCache::global().put(key, Arc::new(image.clone()));

        Ok(image)
    }
}

/// NetworkImage (with caching)
#[derive(Debug, Clone)]
pub struct NetworkImage {
    url: String,
    scale: f64,
    headers: Option<HashMap<String, String>>,
}

impl NetworkImage {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            scale: 1.0,
            headers: None,
        }
    }

    pub fn scale(mut self, scale: f64) -> Self {
        self.scale = scale;
        self
    }

    pub fn headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = Some(headers);
        self
    }
}

#[async_trait::async_trait]
impl ImageProvider for NetworkImage {
    fn key(&self) -> ImageKey {
        ImageKey(format!("network://{}", self.url))
    }

    async fn load(&self) -> Result<Image, ImageError> {
        // Check cache
        let key = self.key();
        if let Some(cached) = ImageCache::global().get(&key) {
            return Ok((*cached).clone());
        }

        // Download
        let client = reqwest::Client::new();
        let mut request = client.get(&self.url);

        if let Some(ref headers) = self.headers {
            for (k, v) in headers {
                request = request.header(k, v);
            }
        }

        let response = request.send().await?;
        let bytes = response.bytes().await?;

        // Decode
        let image = Image::from_bytes(&bytes)?;

        // Cache
        ImageCache::global().put(key, Arc::new(image.clone()));

        Ok(image)
    }
}

/// FileImage
#[derive(Debug, Clone)]
pub struct FileImage {
    path: PathBuf,
    scale: f64,
}

impl FileImage {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            scale: 1.0,
        }
    }
}

#[async_trait::async_trait]
impl ImageProvider for FileImage {
    fn key(&self) -> ImageKey {
        ImageKey(format!("file://{}", self.path.display()))
    }

    async fn load(&self) -> Result<Image, ImageError> {
        let bytes = tokio::fs::read(&self.path).await?;
        Ok(Image::from_bytes(&bytes)?)
    }
}
```

---

## Resolution-Aware Assets

### Automatic Variant Selection

```rust
// In flui_assets/src/manifest.rs

/// Asset manifest (loaded from asset_manifest.toml)
pub struct AssetManifest {
    assets: HashMap<String, AssetEntry>,
    fonts: Vec<FontFamily>,
}

#[derive(Debug, Clone)]
pub struct AssetEntry {
    /// Logical key (e.g., "assets/logo.png")
    pub key: String,

    /// Variants at different resolutions
    pub variants: Vec<AssetVariant>,
}

#[derive(Debug, Clone)]
pub struct AssetVariant {
    /// Device pixel ratio (1.0, 2.0, 3.0)
    pub ratio: f64,

    /// Physical path on disk
    pub path: String,
}

impl AssetManifest {
    /// Load default manifest from asset_manifest.toml
    pub fn load_default() -> Result<Self, AssetError> {
        let manifest_path = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|p| p.join("asset_manifest.toml")))
            .ok_or(AssetError::ManifestNotFound)?;

        Self::load_from_file(manifest_path)
    }

    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self, AssetError> {
        let content = std::fs::read_to_string(path)?;
        let manifest: ManifestFile = toml::from_str(&content)?;

        Ok(Self {
            assets: manifest
                .assets
                .into_iter()
                .map(|entry| (entry.key.clone(), entry))
                .collect(),
            fonts: manifest.fonts,
        })
    }

    /// Resolve a logical key to a physical path based on device pixel ratio
    pub fn resolve_variant(&self, key: &str, device_pixel_ratio: f64) -> String {
        let entry = match self.assets.get(key) {
            Some(e) => e,
            None => return key.to_string(), // Fallback to key itself
        };

        // Find closest variant
        let mut best_variant = &entry.variants[0];
        let mut best_distance = (best_variant.ratio - device_pixel_ratio).abs();

        for variant in &entry.variants {
            let distance = (variant.ratio - device_pixel_ratio).abs();
            if distance < best_distance {
                best_variant = variant;
                best_distance = distance;
            }
        }

        best_variant.path.clone()
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.assets.contains_key(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.assets.keys()
    }
}

#[derive(Debug, Deserialize)]
struct ManifestFile {
    assets: Vec<AssetEntry>,
    fonts: Vec<FontFamily>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FontFamily {
    pub family: String,
    pub fonts: Vec<FontAssetEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FontAssetEntry {
    pub asset: String,
    pub weight: u16,
    pub style: String,
}
```

**asset_manifest.toml Example:**

```toml
# Generated by flui build tool

[[assets]]
key = "assets/images/logo.png"
variants = [
    { ratio = 1.0, path = "assets/images/logo.png" },
    { ratio = 2.0, path = "assets/images/2.0x/logo.png" },
    { ratio = 3.0, path = "assets/images/3.0x/logo.png" },
]

[[assets]]
key = "assets/images/background.jpg"
variants = [
    { ratio = 1.0, path = "assets/images/background.jpg" },
    { ratio = 2.0, path = "assets/images/2.0x/background.jpg" },
]

[[fonts]]
family = "Roboto"
fonts = [
    { asset = "fonts/Roboto-Regular.ttf", weight = 400, style = "normal" },
    { asset = "fonts/Roboto-Bold.ttf", weight = 700, style = "normal" },
    { asset = "fonts/Roboto-Italic.ttf", weight = 400, style = "italic" },
]

[[fonts]]
family = "Montserrat"
fonts = [
    { asset = "fonts/Montserrat-Regular.ttf", weight = 400, style = "normal" },
]
```

---

## Asset Manifest & Bundling

### Build Tool Integration

```rust
// In flui_tools/src/asset_bundler.rs

/// Asset bundler (generates asset_manifest.toml)
pub struct AssetBundler {
    root_dir: PathBuf,
    output_dir: PathBuf,
}

impl AssetBundler {
    pub fn new(root_dir: PathBuf, output_dir: PathBuf) -> Self {
        Self {
            root_dir,
            output_dir,
        }
    }

    /// Scan assets directory and generate manifest
    pub fn bundle(&self) -> Result<(), AssetError> {
        let mut manifest = ManifestFile {
            assets: Vec::new(),
            fonts: Vec::new(),
        };

        // Scan for images with resolution variants
        self.scan_images(&self.root_dir, &mut manifest)?;

        // Scan for fonts (from flui.toml or similar)
        self.scan_fonts(&mut manifest)?;

        // Write manifest
        let manifest_path = self.output_dir.join("asset_manifest.toml");
        let toml = toml::to_string_pretty(&manifest)?;
        std::fs::write(manifest_path, toml)?;

        // Copy assets to output directory
        self.copy_assets(&manifest)?;

        Ok(())
    }

    fn scan_images(&self, dir: &Path, manifest: &mut ManifestFile) -> Result<(), AssetError> {
        for entry in WalkDir::new(dir) {
            let entry = entry?;
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            // Check if image file
            if let Some(ext) = path.extension() {
                if !matches!(ext.to_str(), Some("png" | "jpg" | "jpeg" | "gif" | "webp")) {
                    continue;
                }
            } else {
                continue;
            }

            // Detect resolution variants
            let logical_key = self.compute_logical_key(path)?;
            let ratio = self.detect_resolution_ratio(path);

            // Find or create asset entry
            let entry = manifest
                .assets
                .iter_mut()
                .find(|e| e.key == logical_key)
                .or_else(|| {
                    manifest.assets.push(AssetEntry {
                        key: logical_key.clone(),
                        variants: Vec::new(),
                    });
                    manifest.assets.last_mut()
                })
                .unwrap();

            // Add variant
            entry.variants.push(AssetVariant {
                ratio,
                path: path.to_string_lossy().to_string(),
            });
        }

        Ok(())
    }

    fn detect_resolution_ratio(&self, path: &Path) -> f64 {
        // Check parent directory for resolution marker
        if let Some(parent) = path.parent() {
            if let Some(name) = parent.file_name() {
                let name = name.to_string_lossy();
                if name == "2.0x" {
                    return 2.0;
                } else if name == "3.0x" {
                    return 3.0;
                } else if name == "4.0x" {
                    return 4.0;
                }
            }
        }

        1.0
    }

    fn compute_logical_key(&self, path: &Path) -> Result<String, AssetError> {
        // Strip resolution directory (2.0x, 3.0x, etc.)
        let mut components: Vec<_> = path.components().collect();

        for (i, comp) in components.iter().enumerate() {
            if let Component::Normal(name) = comp {
                let name = name.to_string_lossy();
                if name == "2.0x" || name == "3.0x" || name == "4.0x" {
                    components.remove(i);
                    break;
                }
            }
        }

        let logical_path: PathBuf = components.iter().collect();
        Ok(logical_path.to_string_lossy().to_string())
    }

    fn scan_fonts(&self, manifest: &mut ManifestFile) -> Result<(), AssetError> {
        // Read font configuration from flui.toml
        let config_path = self.root_dir.join("flui.toml");
        if !config_path.exists() {
            return Ok(());
        }

        let config: FluiConfig = toml::from_str(&std::fs::read_to_string(config_path)?)?;

        for font_family in config.fonts {
            manifest.fonts.push(font_family);
        }

        Ok(())
    }

    fn copy_assets(&self, manifest: &ManifestFile) -> Result<(), AssetError> {
        for asset in &manifest.assets {
            for variant in &asset.variants {
                let src = Path::new(&variant.path);
                let dest = self.output_dir.join(&variant.path);

                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }

                std::fs::copy(src, dest)?;
            }
        }

        Ok(())
    }
}
```

---

## Video Assets

### VideoPlayerController (Flutter Pattern)

```rust
// In flui_assets/src/providers/video_player.rs

/// Video player controller (persistent object, like AnimationController)
#[derive(Clone)]
pub struct VideoPlayerController {
    inner: Arc<Mutex<VideoPlayerInner>>,
}

struct VideoPlayerInner {
    source: VideoSource,
    state: VideoPlayerState,
    position: Duration,
    duration: Option<Duration>,
    is_playing: bool,
    is_looping: bool,
    volume: f64,

    // Platform-specific player
    #[cfg(target_os = "windows")]
    player: Option<WindowsMediaPlayer>,
    #[cfg(target_os = "linux")]
    player: Option<GStreamerPlayer>,
    #[cfg(target_os = "macos")]
    player: Option<AVFoundationPlayer>,

    listeners: Vec<(ListenerId, VideoPlayerListener)>,
}

#[derive(Debug, Clone)]
pub enum VideoSource {
    Asset(String),
    File(PathBuf),
    Network(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoPlayerState {
    Uninitialized,
    Initializing,
    Ready,
    Playing,
    Paused,
    Buffering,
    Ended,
    Error,
}

pub type VideoPlayerListener = Arc<dyn Fn(VideoPlayerEvent) + Send + Sync>;

#[derive(Debug, Clone)]
pub enum VideoPlayerEvent {
    Initialized { duration: Duration },
    PositionChanged { position: Duration },
    StateChanged { state: VideoPlayerState },
    Error { error: String },
}

impl VideoPlayerController {
    /// Create from asset
    pub fn asset(asset_name: impl Into<String>) -> Self {
        Self::new(VideoSource::Asset(asset_name.into()))
    }

    /// Create from file
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::new(VideoSource::File(path.into()))
    }

    /// Create from network URL
    pub fn network(url: impl Into<String>) -> Self {
        Self::new(VideoSource::Network(url.into()))
    }

    fn new(source: VideoSource) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VideoPlayerInner {
                source,
                state: VideoPlayerState::Uninitialized,
                position: Duration::ZERO,
                duration: None,
                is_playing: false,
                is_looping: false,
                volume: 1.0,
                player: None,
                listeners: Vec::new(),
            })),
        }
    }

    /// Initialize the video player (async)
    pub async fn initialize(&self) -> Result<(), VideoError> {
        let mut inner = self.inner.lock();
        inner.state = VideoPlayerState::Initializing;

        // Load video based on source
        let video_data = match &inner.source {
            VideoSource::Asset(name) => {
                let bundle = RootAssetBundle::global();
                bundle.load(name).await?
            }
            VideoSource::File(path) => tokio::fs::read(path).await?,
            VideoSource::Network(url) => {
                let response = reqwest::get(url).await?;
                response.bytes().await?.to_vec()
            }
        };

        // Create platform-specific player
        #[cfg(target_os = "windows")]
        {
            inner.player = Some(WindowsMediaPlayer::new(video_data)?);
        }

        // Get duration
        inner.duration = inner.player.as_ref().and_then(|p| p.duration());
        inner.state = VideoPlayerState::Ready;

        // Notify listeners
        self.notify_listeners(VideoPlayerEvent::Initialized {
            duration: inner.duration.unwrap_or(Duration::ZERO),
        });

        Ok(())
    }

    /// Play the video
    pub fn play(&self) -> Result<(), VideoError> {
        let mut inner = self.inner.lock();

        if inner.state != VideoPlayerState::Ready && inner.state != VideoPlayerState::Paused {
            return Err(VideoError::InvalidState);
        }

        if let Some(player) = &inner.player {
            player.play()?;
        }

        inner.is_playing = true;
        inner.state = VideoPlayerState::Playing;

        self.notify_listeners(VideoPlayerEvent::StateChanged {
            state: VideoPlayerState::Playing,
        });

        Ok(())
    }

    /// Pause the video
    pub fn pause(&self) -> Result<(), VideoError> {
        let mut inner = self.inner.lock();

        if let Some(player) = &inner.player {
            player.pause()?;
        }

        inner.is_playing = false;
        inner.state = VideoPlayerState::Paused;

        self.notify_listeners(VideoPlayerEvent::StateChanged {
            state: VideoPlayerState::Paused,
        });

        Ok(())
    }

    /// Seek to position
    pub fn seek_to(&self, position: Duration) -> Result<(), VideoError> {
        let mut inner = self.inner.lock();

        if let Some(player) = &inner.player {
            player.seek(position)?;
        }

        inner.position = position;

        self.notify_listeners(VideoPlayerEvent::PositionChanged { position });

        Ok(())
    }

    /// Set looping
    pub fn set_looping(&self, looping: bool) {
        self.inner.lock().is_looping = looping;
    }

    /// Set volume (0.0 to 1.0)
    pub fn set_volume(&self, volume: f64) {
        let mut inner = self.inner.lock();
        inner.volume = volume.clamp(0.0, 1.0);

        if let Some(player) = &inner.player {
            let _ = player.set_volume(inner.volume);
        }
    }

    /// Get current position
    pub fn position(&self) -> Duration {
        self.inner.lock().position
    }

    /// Get duration
    pub fn duration(&self) -> Option<Duration> {
        self.inner.lock().duration
    }

    /// Get state
    pub fn state(&self) -> VideoPlayerState {
        self.inner.lock().state
    }

    /// Is playing?
    pub fn is_playing(&self) -> bool {
        self.inner.lock().is_playing
    }

    /// Add listener
    pub fn add_listener(&self, listener: VideoPlayerListener) -> ListenerId {
        let mut inner = self.inner.lock();
        let id = ListenerId::new();
        inner.listeners.push((id, listener));
        id
    }

    /// Remove listener
    pub fn remove_listener(&self, id: ListenerId) {
        let mut inner = self.inner.lock();
        inner.listeners.retain(|(listener_id, _)| *listener_id != id);
    }

    fn notify_listeners(&self, event: VideoPlayerEvent) {
        let inner = self.inner.lock();
        for (_, listener) in &inner.listeners {
            listener(event.clone());
        }
    }

    /// CRITICAL: Dispose when done
    pub fn dispose(&self) {
        let mut inner = self.inner.lock();

        if let Some(player) = inner.player.take() {
            let _ = player.stop();
        }

        inner.listeners.clear();
        inner.state = VideoPlayerState::Uninitialized;
    }
}

impl Drop for VideoPlayerController {
    fn drop(&mut self) {
        // Auto-dispose if not already disposed
        if Arc::strong_count(&self.inner) == 1 {
            self.dispose();
        }
    }
}
```

**VideoPlayer Widget:**

```rust
// In flui_widgets/src/video/video_player.rs

/// VideoPlayer widget
#[derive(Debug)]
pub struct VideoPlayer {
    controller: Arc<VideoPlayerController>,
}

impl VideoPlayer {
    pub fn new(controller: Arc<VideoPlayerController>) -> Self {
        Self { controller }
    }
}

impl View for VideoPlayer {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Subscribe to controller changes
        let controller = self.controller.clone();
        ctx.subscribe_listenable(controller.clone());

        // Create RenderVideoPlayer
        (RenderVideoPlayer::new(controller), ())
    }
}

// In flui_rendering/src/objects/render_video_player.rs

pub struct RenderVideoPlayer {
    controller: Arc<VideoPlayerController>,
}

impl RenderVideoPlayer {
    pub fn new(controller: Arc<VideoPlayerController>) -> Self {
        Self { controller }
    }
}

impl LeafRender for RenderVideoPlayer {
    type Metadata = ();

    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Video aspect ratio
        constraints.constrain(Size::new(640.0, 480.0))
    }

    fn paint(&self, offset: Offset) -> BoxedLayer {
        // Get current video frame from controller
        let frame = self.controller.current_frame();

        // Create texture layer
        let mut picture = PictureLayer::new();
        picture.draw_image(frame, offset);

        Box::new(picture)
    }
}
```

---

## Hot Reload

### File Watching for Development

```rust
// In flui_assets/src/hot_reload.rs (feature = "hot-reload")

use notify::{Watcher, RecursiveMode, Event};

/// Hot reload manager for development
pub struct HotReloadManager {
    watcher: RecommendedWatcher,
    registry: Arc<AssetRegistry>,
    watched_paths: Arc<Mutex<HashSet<PathBuf>>>,
}

impl HotReloadManager {
    pub fn new(registry: Arc<AssetRegistry>) -> Result<Self, AssetError> {
        let watched_paths = Arc::new(Mutex::new(HashSet::new()));

        let registry_clone = registry.clone();
        let watched_paths_clone = watched_paths.clone();

        let watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        // Handle file change
                        for path in event.paths {
                            // Invalidate cache for this asset
                            if let Some(key) = Self::path_to_key(&path) {
                                registry_clone.evict(&key);
                                tracing::info!("Hot reloaded: {}", key);
                            }
                        }
                    }
                    Err(e) => tracing::error!("Watch error: {}", e),
                }
            },
            notify::Config::default(),
        )?;

        Ok(Self {
            watcher,
            registry,
            watched_paths,
        })
    }

    /// Watch a directory for changes
    pub fn watch(&mut self, path: impl AsRef<Path>) -> Result<(), AssetError> {
        let path = path.as_ref();
        self.watcher.watch(path, RecursiveMode::Recursive)?;
        self.watched_paths.lock().insert(path.to_path_buf());
        Ok(())
    }

    fn path_to_key(path: &Path) -> Option<String> {
        // Convert file path to asset key
        path.to_str().map(|s| s.to_string())
    }
}
```

---

## Implementation Plan

### Phase 1: AssetBundle & DefaultAssetBundle (~300 LOC)

1. **bundle.rs** (~150 LOC)
   - `AssetBundle` trait
   - `RootAssetBundle` implementation
   - `DefaultAssetBundle` context provider

2. **inherited_asset_bundle.rs** (~50 LOC)
   - `InheritedAssetBundle` Provider widget

3. **manifest.rs** (~100 LOC)
   - `AssetManifest` loader
   - Resolution variant resolution

**Total Phase 1:** ~300 LOC

### Phase 2: ImageProvider System (~400 LOC)

4. **providers/image_provider.rs** (~150 LOC)
   - `ImageProvider` trait
   - `ImageConfiguration`
   - `ImageStream`
   - `ImageCache`

5. **providers/asset_image.rs** (~100 LOC)
   - `AssetImage` implementation

6. **providers/network_image.rs** (~100 LOC)
   - `NetworkImage` implementation
   - HTTP caching

7. **providers/file_image.rs** (~50 LOC)
   - `FileImage` implementation

**Total Phase 2:** ~400 LOC

### Phase 3: Resolution-Aware Loading (~200 LOC)

8. **manifest.rs** (extend ~100 LOC)
   - Variant scanning
   - Resolution detection

9. **build_tool/asset_bundler.rs** (~100 LOC)
   - Asset scanning
   - Manifest generation

**Total Phase 3:** ~200 LOC

### Phase 4: Video Assets (~400 LOC)

10. **providers/video_player.rs** (~300 LOC)
    - `VideoPlayerController`
    - Platform-specific players

11. **widgets/video_player.rs** (~100 LOC)
    - `VideoPlayer` widget
    - `RenderVideoPlayer` render object

**Total Phase 4:** ~400 LOC

### Phase 5: Hot Reload (~200 LOC)

12. **hot_reload.rs** (~200 LOC)
    - `HotReloadManager`
    - File watching with `notify`

**Total Phase 5:** ~200 LOC

---

## Usage Examples

### Example 1: Load Image from Asset

```rust
use flui_assets::*;
use flui_widgets::*;

#[derive(Debug)]
struct ImageDemo;

impl View for ImageDemo {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Resolution-aware: automatically loads 2.0x/logo.png on 2x device
        Image::new(AssetImage::new("assets/logo.png"))
    }
}
```

### Example 2: Network Image with Cache

```rust
use flui_assets::*;
use flui_widgets::*;

#[derive(Debug)]
struct NetworkImageDemo;

impl View for NetworkImageDemo {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        CachedNetworkImage::new("https://example.com/image.png")
            .placeholder(|| CircularProgressIndicator::new())
            .error_widget(|| Icon::new("broken_image"))
    }
}
```

### Example 3: Video Player

```rust
use flui_assets::*;
use flui_widgets::*;

#[derive(Debug)]
struct VideoDemo;

impl View for VideoDemo {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Create controller (persists via hook)
        let controller = use_memo(ctx, |_| {
            Arc::new(VideoPlayerController::asset("assets/video.mp4"))
        });

        // Initialize on mount
        use_effect(ctx, {
            let controller = controller.clone();
            move || {
                tokio::spawn(async move {
                    controller.initialize().await.unwrap();
                    controller.play().unwrap();
                });

                Some(Box::new(move || {
                    controller.dispose();
                }))
            }
        });

        Column::new()
            .children(vec![
                Box::new(VideoPlayer::new(controller.clone())),
                Box::new(
                    Row::new()
                        .children(vec![
                            Box::new(IconButton::new("play_arrow")
                                .on_pressed({
                                    let controller = controller.clone();
                                    move || { let _ = controller.play(); }
                                })),
                            Box::new(IconButton::new("pause")
                                .on_pressed({
                                    let controller = controller.clone();
                                    move || { let _ = controller.pause(); }
                                })),
                        ])
                ),
            ])
    }
}
```

### Example 4: Custom AssetBundle (Testing)

```rust
use flui_assets::*;

#[derive(Debug)]
struct TestApp;

impl View for TestApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Override default bundle with memory bundle for testing
        let test_bundle = Arc::new(MemoryAssetBundle::new());
        test_bundle.insert("assets/test.png", include_bytes!("test.png"));

        DefaultAssetBundle::new(
            test_bundle,
            Box::new(MyApp::new()),
        )
    }
}
```

### Example 5: Load JSON Config

```rust
use flui_assets::*;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct AppConfig {
    api_url: String,
    timeout: u64,
}

async fn load_config() -> Result<AppConfig, AssetError> {
    let bundle = RootAssetBundle::global();
    bundle.load_structured("assets/config.json").await
}
```

---

## Testing Strategy

### Unit Tests

1. **AssetManifest:**
   - Test variant resolution (1x, 2x, 3x)
   - Test missing variants (fallback to 1x)
   - Test manifest parsing

2. **ImageCache:**
   - Test cache hit/miss
   - Test LRU eviction
   - Test size limits
   - Test concurrent access

3. **ImageProvider:**
   - Test AssetImage loading
   - Test NetworkImage caching
   - Test FileImage loading

4. **VideoPlayerController:**
   - Test play/pause
   - Test seek
   - Test looping
   - Test volume control

### Integration Tests

1. **Resolution Awareness:**
   - Test loading 2x asset on 2x device
   - Test fallback to 1x if 2x missing

2. **AssetBundle Override:**
   - Test DefaultAssetBundle.of()
   - Test custom bundle in tests

3. **Performance:**
   - Benchmark cache lookup
   - Test 1000+ concurrent loads
   - Measure memory usage

---

## Crate Dependencies

```toml
# crates/flui_assets/Cargo.toml

[package]
name = "flui_assets"
version = "0.1.0"
edition = "2021"

[dependencies]
flui_types = { path = "../flui_types" }
flui_core = { path = "../flui_core" }

# Async runtime
tokio = { version = "1.43", features = ["fs", "io-util", "sync"] }
async-trait = "0.1"

# Caching
moka = { version = "0.12", features = ["future"] }

# Key interning
lasso = "0.7"

# Serialization
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"
serde_json = "1.0"

# HTTP (optional)
reqwest = { version = "0.12", optional = true, features = ["stream"] }

# Image decoding (optional)
image = { version = "0.25", optional = true }

# Hot reload (optional)
notify = { version = "6.0", optional = true }

# Video (optional - platform-specific)
[target.'cfg(windows)'.dependencies]
windows = { version = "0.52", optional = true, features = ["Media_Playback"] }

[features]
default = []
images = ["image"]
network = ["reqwest"]
hot-reload = ["notify"]
video = ["windows"]  # Platform-specific

[dev-dependencies]
tokio = { version = "1.43", features = ["full", "test-util"] }
tempfile = "3.0"
```

---

## Open Questions

1. **Video Codec Support:**
   - Which codecs should we support out of the box?
   - Should we use platform decoders or cross-platform library (like ffmpeg)?

2. **Asset Compression:**
   - Should we support compressed asset bundles (gzip, brotli)?
   - When to decompress (build time vs runtime)?

3. **Asset Encryption:**
   - Should we support encrypted assets for sensitive content?
   - How to manage encryption keys?

4. **CDN Integration:**
   - Should we support CDN URLs for network assets?
   - How to handle CDN failures (fallback)?

5. **Offline Assets:**
   - Should we support offline-first asset loading?
   - How to sync assets when back online?

---

## Version History

| Version | Date       | Author | Changes                      |
|---------|------------|--------|------------------------------|
| 0.1.0   | 2025-11-10 | Claude | Initial assets architecture  |

---

## References

- [Flutter Assets and Images](https://docs.flutter.dev/ui/assets/assets-and-images)
- [Flutter AssetBundle API](https://api.flutter.dev/flutter/services/AssetBundle-class.html)
- [Flutter ImageProvider API](https://api.flutter.dev/flutter/painting/ImageProvider-class.html)
- [Flutter CachedNetworkImage](https://pub.dev/packages/cached_network_image)
- [Flutter video_player](https://pub.dev/packages/video_player)

---

## Conclusion

This architecture provides a **complete, Flutter-compatible asset management system** for FLUI:

✅ **AssetBundle pattern** for flexible asset loading
✅ **DefaultAssetBundle** for context-based bundle override
✅ **ImageProvider** trait for image caching and loading
✅ **Resolution awareness** (1x/2x/3x automatic selection)
✅ **Asset manifest** (TOML-based, like Flutter's AssetManifest.json)
✅ **Multiple sources** (file, network, memory, bundle)
✅ **Video support** (VideoPlayerController pattern)
✅ **Hot reload** for development
✅ **High performance** (Moka cache, async I/O, key interning)

**Current Status:**
- ✅ Core system (Asset trait, registry, cache) - **implemented**
- ⏳ AssetBundle & DefaultAssetBundle - **TODO (~300 LOC)**
- ⏳ ImageProvider system - **TODO (~400 LOC)**
- ⏳ Resolution-aware loading - **TODO (~200 LOC)**
- ⏳ Video assets - **TODO (~400 LOC)**
- ⏳ Hot reload - **TODO (~200 LOC)**

**Remaining Work:** ~1,500 LOC

This provides a solid foundation for production-ready asset management in FLUI! 🎨📦
