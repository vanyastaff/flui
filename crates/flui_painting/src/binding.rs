//! PaintingBinding - Binding for the painting library.
//!
//! Provides image caching, shader warm-up, and system font notifications.
//!
//! # Flutter Equivalence
//!
//! Corresponds to Flutter's `PaintingBinding` mixin from `painting/binding.dart`.
//!
//! # Features
//!
//! - [`ImageCache`] - Caches decoded images for reuse
//! - [`ShaderWarmUp`] - Pre-compiles shaders to avoid jank
//! - System font change notifications
//! - Memory pressure handling

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;

use flui_foundation::{impl_binding_singleton, BindingBase, HasInstance};
use flui_types::geometry::{px, Half, Pixels, Radius};
use flui_types::Size;

// ============================================================================
// ImageCache
// ============================================================================

/// A cache for decoded images.
///
/// The image cache stores decoded images keyed by a string identifier.
/// It has configurable limits for both the number of images and total
/// memory usage.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `ImageCache` class from `painting/image_cache.dart`.
#[derive(Debug)]
pub struct ImageCache {
    /// Cached images: key -> (image data, size in bytes)
    cache: RwLock<HashMap<String, CachedImage>>,

    /// Maximum number of images to cache.
    max_images: AtomicUsize,

    /// Maximum total size in bytes.
    max_size_bytes: AtomicUsize,

    /// Current total size in bytes.
    current_size_bytes: AtomicUsize,

    /// Live images (currently in use, not evictable).
    live_images: RwLock<HashMap<String, CachedImage>>,
}

/// A cached image entry.
#[derive(Debug, Clone)]
pub struct CachedImage {
    /// The image data (opaque handle).
    pub handle: ImageHandle,

    /// Size of the image in bytes.
    pub size_bytes: usize,

    /// Original size of the image.
    pub dimensions: Size<Pixels>,
}

/// Opaque handle to an image.
///
/// In a real implementation, this would reference GPU texture or decoded pixels.
#[derive(Debug, Clone)]
pub struct ImageHandle {
    /// Unique identifier for this image.
    pub id: u64,
}

impl Default for ImageCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageCache {
    /// Default maximum number of images.
    pub const DEFAULT_MAX_IMAGES: usize = 1000;

    /// Default maximum size in bytes (100 MB).
    pub const DEFAULT_MAX_SIZE_BYTES: usize = 100 * 1024 * 1024;

    /// Creates a new image cache with default limits.
    pub fn new() -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            max_images: AtomicUsize::new(Self::DEFAULT_MAX_IMAGES),
            max_size_bytes: AtomicUsize::new(Self::DEFAULT_MAX_SIZE_BYTES),
            current_size_bytes: AtomicUsize::new(0),
            live_images: RwLock::new(HashMap::new()),
        }
    }

    /// Creates an image cache with custom limits.
    pub fn with_limits(max_images: usize, max_size_bytes: usize) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            max_images: AtomicUsize::new(max_images),
            max_size_bytes: AtomicUsize::new(max_size_bytes),
            current_size_bytes: AtomicUsize::new(0),
            live_images: RwLock::new(HashMap::new()),
        }
    }

    /// Returns the maximum number of images to cache.
    pub fn max_images(&self) -> usize {
        self.max_images.load(Ordering::Relaxed)
    }

    /// Sets the maximum number of images to cache.
    pub fn set_max_images(&self, value: usize) {
        self.max_images.store(value, Ordering::Relaxed);
        self.evict_if_needed();
    }

    /// Returns the maximum size in bytes.
    pub fn max_size_bytes(&self) -> usize {
        self.max_size_bytes.load(Ordering::Relaxed)
    }

    /// Sets the maximum size in bytes.
    pub fn set_max_size_bytes(&self, value: usize) {
        self.max_size_bytes.store(value, Ordering::Relaxed);
        self.evict_if_needed();
    }

    /// Returns the current number of cached images.
    pub fn count(&self) -> usize {
        self.cache.read().len()
    }

    /// Returns the current size in bytes.
    pub fn current_size_bytes(&self) -> usize {
        self.current_size_bytes.load(Ordering::Relaxed)
    }

    /// Gets a cached image by key.
    pub fn get(&self, key: &str) -> Option<CachedImage> {
        self.cache.read().get(key).cloned()
    }

    /// Puts an image in the cache.
    ///
    /// Returns the previous image if the key was already present.
    pub fn put(&self, key: String, image: CachedImage) -> Option<CachedImage> {
        let size = image.size_bytes;
        let old = {
            let mut cache = self.cache.write();
            cache.insert(key, image)
        };

        if let Some(ref old_image) = old {
            // Subtract old size, add new size
            self.current_size_bytes
                .fetch_sub(old_image.size_bytes, Ordering::Relaxed);
        }
        self.current_size_bytes.fetch_add(size, Ordering::Relaxed);

        self.evict_if_needed();
        old
    }

    /// Removes an image from the cache.
    pub fn evict(&self, key: &str) -> Option<CachedImage> {
        let removed = self.cache.write().remove(key);
        if let Some(ref image) = removed {
            self.current_size_bytes
                .fetch_sub(image.size_bytes, Ordering::Relaxed);
        }
        removed
    }

    /// Clears all cached images.
    pub fn clear(&self) {
        self.cache.write().clear();
        self.current_size_bytes.store(0, Ordering::Relaxed);
    }

    /// Clears live images (images currently in use).
    pub fn clear_live_images(&self) {
        self.live_images.write().clear();
    }

    /// Marks an image as "live" (in use, should not be evicted).
    pub fn mark_live(&self, key: String, image: CachedImage) {
        self.live_images.write().insert(key, image);
    }

    /// Removes an image from live images.
    pub fn unmark_live(&self, key: &str) {
        self.live_images.write().remove(key);
    }

    /// Evicts images if over limits.
    fn evict_if_needed(&self) {
        let max_images = self.max_images.load(Ordering::Relaxed);
        let max_bytes = self.max_size_bytes.load(Ordering::Relaxed);

        // Simple LRU-like eviction: remove oldest entries
        // In a real implementation, we'd track access time
        loop {
            let count = self.cache.read().len();
            let size = self.current_size_bytes.load(Ordering::Relaxed);

            if count <= max_images && size <= max_bytes {
                break;
            }

            // Remove first entry (simple eviction strategy)
            let key_to_remove = {
                let cache = self.cache.read();
                cache.keys().next().cloned()
            };

            if let Some(key) = key_to_remove {
                self.evict(&key);
            } else {
                break;
            }
        }
    }
}

// ============================================================================
// ShaderWarmUp
// ============================================================================

/// Trait for shader warm-up implementations.
///
/// Shader compilation can cause jank during animations. By pre-compiling
/// shaders during startup, we can avoid this.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `ShaderWarmUp` class from `painting/shader_warm_up.dart`.
pub trait ShaderWarmUp: Send + Sync {
    /// The size of the canvas to use for warm-up.
    ///
    /// Defaults to 100x100 logical pixels.
    fn size(&self) -> Size<Pixels> {
        Size::new(px(100.0), px(100.0))
    }

    /// Paints the warm-up scene onto the canvas.
    ///
    /// Implementations should draw shapes that trigger shader compilation
    /// for commonly used effects.
    fn warm_up_on_canvas(&self, canvas: &mut dyn WarmUpCanvas);

    /// Executes the shader warm-up.
    ///
    /// This is called during binding initialization.
    fn execute(&self) {
        // Create a temporary canvas and paint the warm-up scene
        tracing::debug!("Executing shader warm-up with size {:?}", self.size());
        // In a real implementation, we'd create an offscreen canvas here
    }
}

/// Canvas interface for shader warm-up.
///
/// A minimal canvas interface used during warm-up.
pub trait WarmUpCanvas {
    /// Draws a rectangle.
    fn draw_rect(&mut self, rect: flui_types::Rect);

    /// Draws a rounded rectangle.
    fn draw_rrect(&mut self, rrect: flui_types::RRect);

    /// Draws a circle.
    fn draw_circle(&mut self, center: flui_types::Offset<Pixels>, radius: f32);

    /// Draws a path.
    fn draw_path(&mut self, path: &[flui_types::Offset<Pixels>]);
}

/// Default shader warm-up that draws common shapes.
#[derive(Debug, Default)]
pub struct DefaultShaderWarmUp;

impl ShaderWarmUp for DefaultShaderWarmUp {
    fn warm_up_on_canvas(&self, canvas: &mut dyn WarmUpCanvas) {
        let size = self.size();

        // Draw various shapes to trigger shader compilation
        canvas.draw_rect(flui_types::Rect::from_ltwh(
            px(0.0),
            px(0.0),
            size.width,
            size.height,
        ));
        canvas.draw_rrect(flui_types::RRect::from_rect_and_radius(
            flui_types::Rect::from_ltwh(px(10.0), px(10.0), px(80.0), px(80.0)),
            Radius::circular(px(10.0)),
        ));
        canvas.draw_circle(
            flui_types::Offset::new(size.width.half(), size.height.half()),
            30.0,
        );
    }
}

// ============================================================================
// SystemFontsNotifier
// ============================================================================

/// Notifies listeners when system fonts change.
///
/// System fonts can change when the OS installs or removes fonts.
/// Text-related widgets should listen to this and redraw.
#[derive(Default)]
pub struct SystemFontsNotifier {
    listeners: RwLock<Vec<Arc<dyn Fn() + Send + Sync>>>,
}

impl std::fmt::Debug for SystemFontsNotifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemFontsNotifier")
            .field("listeners_count", &self.listeners.read().len())
            .finish()
    }
}

impl SystemFontsNotifier {
    /// Creates a new notifier.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a listener for font changes.
    pub fn add_listener(&self, listener: Arc<dyn Fn() + Send + Sync>) {
        self.listeners.write().push(listener);
    }

    /// Removes a listener.
    pub fn remove_listener(&self, listener: &Arc<dyn Fn() + Send + Sync>) {
        self.listeners.write().retain(|l| !Arc::ptr_eq(l, listener));
    }

    /// Notifies all listeners that fonts have changed.
    pub fn notify_listeners(&self) {
        let listeners = self.listeners.read();
        for listener in listeners.iter() {
            listener();
        }
    }
}

// ============================================================================
// PaintingBinding
// ============================================================================

/// Binding for the painting library.
///
/// Provides image caching, shader warm-up, and system font notifications.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `PaintingBinding` mixin.
pub struct PaintingBinding {
    /// The image cache singleton.
    image_cache: ImageCache,

    /// System fonts notifier.
    system_fonts: SystemFontsNotifier,

    /// Optional shader warm-up.
    shader_warm_up: Option<Box<dyn ShaderWarmUp>>,
}

impl std::fmt::Debug for PaintingBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaintingBinding")
            .field("image_cache_count", &self.image_cache.count())
            .field(
                "image_cache_size_bytes",
                &self.image_cache.current_size_bytes(),
            )
            .field("has_shader_warm_up", &self.shader_warm_up.is_some())
            .finish()
    }
}

impl Default for PaintingBinding {
    fn default() -> Self {
        Self::new()
    }
}

impl PaintingBinding {
    /// Creates a new painting binding.
    pub fn new() -> Self {
        let mut binding = Self {
            image_cache: ImageCache::new(),
            system_fonts: SystemFontsNotifier::new(),
            shader_warm_up: None,
        };
        binding.init_instances();
        binding
    }

    /// Creates a painting binding with a custom shader warm-up.
    pub fn with_shader_warm_up(warm_up: Box<dyn ShaderWarmUp>) -> Self {
        let mut binding = Self {
            image_cache: ImageCache::new(),
            system_fonts: SystemFontsNotifier::new(),
            shader_warm_up: Some(warm_up),
        };
        binding.init_instances();
        binding
    }

    /// Returns the image cache.
    pub fn image_cache(&self) -> &ImageCache {
        &self.image_cache
    }

    /// Returns a mutable reference to the image cache.
    pub fn image_cache_mut(&mut self) -> &mut ImageCache {
        &mut self.image_cache
    }

    /// Returns the system fonts notifier.
    pub fn system_fonts(&self) -> &SystemFontsNotifier {
        &self.system_fonts
    }

    /// Sets the shader warm-up.
    ///
    /// Must be called before [`init_instances`](BindingBase::init_instances).
    pub fn set_shader_warm_up(&mut self, warm_up: Box<dyn ShaderWarmUp>) {
        self.shader_warm_up = Some(warm_up);
    }

    /// Handles memory pressure by clearing the image cache.
    pub fn handle_memory_pressure(&self) {
        tracing::info!("Memory pressure: clearing image cache");
        self.image_cache.clear();
    }

    /// Evicts a specific asset from the cache.
    pub fn evict(&self, asset: &str) {
        self.image_cache.evict(asset);
        self.image_cache.clear_live_images();
    }

    /// Handles a system message (e.g., font change notification).
    pub fn handle_system_message(&self, message_type: &str) {
        match message_type {
            "fontsChange" => {
                tracing::debug!("System fonts changed");
                self.system_fonts.notify_listeners();
            }
            _ => {}
        }
    }
}

impl BindingBase for PaintingBinding {
    fn init_instances(&mut self) {
        // Execute shader warm-up if configured
        if let Some(warm_up) = &self.shader_warm_up {
            warm_up.execute();
            tracing::debug!("Shader warm-up completed");
        }

        tracing::info!("PaintingBinding initialized");
    }
}

// Singleton pattern
impl_binding_singleton!(PaintingBinding);

// ============================================================================
// Convenience function
// ============================================================================

/// Returns the global image cache.
///
/// This is a convenience function equivalent to `PaintingBinding.instance.imageCache`.
pub fn image_cache() -> &'static ImageCache {
    PaintingBinding::instance().image_cache()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_cache_new() {
        let cache = ImageCache::new();
        assert_eq!(cache.count(), 0);
        assert_eq!(cache.current_size_bytes(), 0);
        assert_eq!(cache.max_images(), ImageCache::DEFAULT_MAX_IMAGES);
    }

    #[test]
    fn test_image_cache_put_get() {
        let cache = ImageCache::new();

        let image = CachedImage {
            handle: ImageHandle { id: 1 },
            size_bytes: 1024,
            dimensions: Size::new(px(100.0), px(100.0)),
        };

        cache.put("test".to_string(), image.clone());
        assert_eq!(cache.count(), 1);
        assert_eq!(cache.current_size_bytes(), 1024);

        let retrieved = cache.get("test").unwrap();
        assert_eq!(retrieved.handle.id, 1);
    }

    #[test]
    fn test_image_cache_evict() {
        let cache = ImageCache::new();

        let image = CachedImage {
            handle: ImageHandle { id: 1 },
            size_bytes: 1024,
            dimensions: Size::new(px(100.0), px(100.0)),
        };

        cache.put("test".to_string(), image);
        assert_eq!(cache.count(), 1);

        cache.evict("test");
        assert_eq!(cache.count(), 0);
        assert_eq!(cache.current_size_bytes(), 0);
    }

    #[test]
    fn test_image_cache_clear() {
        let cache = ImageCache::new();

        for i in 0..10 {
            let image = CachedImage {
                handle: ImageHandle { id: i },
                size_bytes: 1024,
                dimensions: Size::new(px(100.0), px(100.0)),
            };
            cache.put(format!("test_{}", i), image);
        }

        assert_eq!(cache.count(), 10);
        cache.clear();
        assert_eq!(cache.count(), 0);
    }

    #[test]
    fn test_image_cache_eviction() {
        let cache = ImageCache::with_limits(3, 10000);

        for i in 0..5 {
            let image = CachedImage {
                handle: ImageHandle { id: i },
                size_bytes: 1024,
                dimensions: Size::new(px(100.0), px(100.0)),
            };
            cache.put(format!("test_{}", i), image);
        }

        // Should have evicted to stay under max_images
        assert!(cache.count() <= 3);
    }

    #[test]
    fn test_system_fonts_notifier() {
        use std::sync::atomic::AtomicUsize;

        let notifier = SystemFontsNotifier::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let listener: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            counter_clone.fetch_add(1, Ordering::Relaxed);
        });

        notifier.add_listener(listener.clone());
        notifier.notify_listeners();
        assert_eq!(counter.load(Ordering::Relaxed), 1);

        notifier.notify_listeners();
        assert_eq!(counter.load(Ordering::Relaxed), 2);

        notifier.remove_listener(&listener);
        notifier.notify_listeners();
        assert_eq!(counter.load(Ordering::Relaxed), 2); // No change
    }

    #[test]
    fn test_painting_binding_singleton() {
        let binding1 = PaintingBinding::instance();
        let binding2 = PaintingBinding::instance();
        assert!(std::ptr::eq(binding1, binding2));
    }
}
