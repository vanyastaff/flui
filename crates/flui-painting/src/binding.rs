//! PaintingBinding - Binding for the painting library.
//!
//! Provides image caching and system font notifications.
//!
//! # Flutter Equivalence
//!
//! Corresponds to Flutter's `PaintingBinding` mixin from
//! `painting/binding.dart`. Flutter's shader-warm-up subsystem was
//! deleted in Mythos chain step 2 (decorative; `execute()` was a stub
//! and no production caller relied on it). Real offscreen-canvas-backed
//! warm-up is tracked in `crates/flui-painting/ARCHITECTURE.md`
//! `## Outstanding refactors`.
//!
//! # Features
//!
//! - [`ImageCache`] - Caches decoded images for reuse
//! - System font change notifications
//! - Memory pressure handling

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use flui_foundation::{BindingBase, HasInstance, impl_binding_singleton};
use flui_types::{Size, geometry::Pixels};
use parking_lot::RwLock;

use crate::error::{PaintingError, Result};
use crate::text_layout::SharedFontSystem;

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
/// Corresponds to Flutter's `ImageCache` class from
/// `painting/image_cache.dart`.
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
/// In a real implementation, this would reference GPU texture or decoded
/// pixels.
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
        const MAX_EVICTION_ITERATIONS: usize = 1000;

        let max_images = self.max_images.load(Ordering::Relaxed);
        let max_bytes = self.max_size_bytes.load(Ordering::Relaxed);

        // Simple LRU-like eviction: remove oldest entries
        // In a real implementation, we'd track access time.
        //
        // The exit predicate is racy against concurrent `put` calls on
        // other threads: if a parallel inserter keeps pace with our
        // eviction, the loop will never observe count/size under the
        // limit. Cap the iteration count to guarantee forward progress
        // and warn so the racy state is observable in logs.
        for _ in 0..MAX_EVICTION_ITERATIONS {
            let count = self.cache.read().len();
            let size = self.current_size_bytes.load(Ordering::Relaxed);

            if count <= max_images && size <= max_bytes {
                return;
            }

            // Remove first entry (simple eviction strategy)
            let key_to_remove = {
                let cache = self.cache.read();
                cache.keys().next().cloned()
            };

            if let Some(key) = key_to_remove {
                self.evict(&key);
            } else {
                return;
            }
        }

        tracing::warn!(
            cap = MAX_EVICTION_ITERATIONS,
            "ImageCache::evict_if_needed hit iteration cap; concurrent inserts may be outpacing eviction"
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
///
/// # Visibility
///
/// Crate-internal until a platform-side trigger exists. Today the only
/// caller of [`Self::notify_listeners`] is
/// [`PaintingBinding::handle_system_message`], and the only caller of
/// `handle_system_message("fontsChange")` is the crate-internal test
/// suite -- no `flui-platform` plumbing surfaces an OS font-change event.
/// Exposing the listener-registration surface publicly today would invite
/// consumers we cannot serve. Promote to `pub` again when the platform
/// trigger lands.
///
/// Audit reference: docs/research/2026-05-22-flui-painting-view-audit.md
/// P-10.
#[derive(Default)]
pub(crate) struct SystemFontsNotifier {
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
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Adds a listener for font changes.
    ///
    /// Crate-internal until the platform-side font-change trigger lands;
    /// currently exercised only by the in-crate test suite. See the
    /// struct doc-comment (audit P-10) for the visibility rationale.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn add_listener(&self, listener: Arc<dyn Fn() + Send + Sync>) {
        self.listeners.write().push(listener);
    }

    /// Removes a listener.
    ///
    /// See [`Self::add_listener`] for the dead-code allowance rationale.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn remove_listener(&self, listener: &Arc<dyn Fn() + Send + Sync>) {
        self.listeners.write().retain(|l| !Arc::ptr_eq(l, listener));
    }

    /// Notifies all listeners that fonts have changed.
    ///
    /// Listeners are snapshotted into a local `Vec` (cloning the
    /// `Arc<dyn Fn>`s) before the read lock is released and the
    /// callbacks are invoked. Without this, a listener that calls
    /// [`Self::add_listener`] or [`Self::remove_listener`] reentrantly
    /// from inside its own body would deadlock against the read guard
    /// we still held here.
    pub(crate) fn notify_listeners(&self) {
        let snapshot: Vec<Arc<dyn Fn() + Send + Sync>> = {
            let listeners = self.listeners.read();
            listeners.iter().cloned().collect()
        };
        for listener in &snapshot {
            listener();
        }
    }
}

// ============================================================================
// PaintingBinding
// ============================================================================

/// Binding for the painting library.
///
/// Provides image caching and system font notifications.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `PaintingBinding` mixin.
///
/// # Trimmed surface (Mythos chain U2/U3)
///
/// Flutter's shader-warm-up subsystem was deleted -- the trait had one
/// stub impl whose `execute()` body documented "in a real implementation,
/// we'd create an offscreen canvas here"; no production caller relied
/// on warm-up to bootstrap shader compilation. The optional warm-up
/// field on this binding, the `with_*` constructor variant, and the
/// `set_*` setter all went with it. Real offscreen-canvas-backed
/// warm-up is tracked in `crates/flui-painting/ARCHITECTURE.md`
/// `## Outstanding refactors`.
pub struct PaintingBinding {
    /// The image cache singleton.
    image_cache: ImageCache,

    /// System fonts notifier.
    system_fonts: SystemFontsNotifier,
}

impl std::fmt::Debug for PaintingBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaintingBinding")
            .field("image_cache_count", &self.image_cache.count())
            .field(
                "image_cache_size_bytes",
                &self.image_cache.current_size_bytes(),
            )
            .finish_non_exhaustive()
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
    ///
    /// Crate-internal -- see [`SystemFontsNotifier`] doc-comment (audit
    /// P-10) for the visibility rationale. Kept on the surface so a
    /// future platform-side trigger only needs to flip the visibility
    /// back, not re-introduce a getter.
    #[allow(dead_code, reason = "P-10: kept ready for the platform trigger")]
    pub(crate) fn system_fonts(&self) -> &SystemFontsNotifier {
        &self.system_fonts
    }

    /// Returns the shared font system handle.
    ///
    /// This is the single [`FontSystem`](crate::FontSystem) the whole
    /// framework shapes and measures text with; the engine's glyph pipeline holds a clone
    /// of the same handle (ADR-0016), so a face registered via
    /// [`Self::register_font`] is visible to both measurement and rendering.
    #[must_use]
    pub fn font_system(&self) -> SharedFontSystem {
        crate::text_layout::shared_font_system()
    }

    /// Registers a font from its raw bytes (TTF/OTF), making every face it
    /// contains available to text measurement and glyph rendering.
    ///
    /// On success, font listeners are notified so already-laid-out text can
    /// re-shape against the newly available faces.
    ///
    /// # Errors
    ///
    /// Returns [`PaintingError::RegisterFontFailed`] if `font_bytes` parses
    /// to zero loadable faces (empty, truncated, or not a font at all).
    #[tracing::instrument(skip(self, font_bytes), fields(bytes = font_bytes.len()))]
    pub fn register_font(&self, font_bytes: &[u8]) -> Result<()> {
        let faces_added = self.font_system().with_mut(|font_system| {
            let faces_before = font_system.db().len();
            font_system.db_mut().load_font_data(font_bytes.to_vec());
            font_system.db().len() - faces_before
        });
        if faces_added == 0 {
            return Err(PaintingError::register_font_failed(
                "font data contained no loadable faces",
            ));
        }
        tracing::debug!(faces_added, "registered font");
        self.system_fonts.notify_listeners();
        Ok(())
    }

    /// Handles memory pressure by clearing the image cache.
    #[tracing::instrument(skip(self))]
    pub fn handle_memory_pressure(&self) {
        tracing::info!("Memory pressure: clearing image cache");
        self.image_cache.clear();
    }

    /// Evicts a specific asset from the cache.
    #[tracing::instrument(skip(self))]
    pub fn evict(&self, asset: &str) {
        self.image_cache.evict(asset);
        self.image_cache.clear_live_images();
    }

    /// Handles a system message (e.g., font change notification).
    #[tracing::instrument(skip(self))]
    pub fn handle_system_message(&self, message_type: &str) {
        if message_type == "fontsChange" {
            tracing::debug!("System fonts changed");
            self.system_fonts.notify_listeners();
        }
    }
}

impl BindingBase for PaintingBinding {
    fn init_instances(&mut self) {
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
/// This is a convenience function equivalent to
/// `PaintingBinding.instance.imageCache`.
pub fn image_cache() -> &'static ImageCache {
    PaintingBinding::instance().image_cache()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code: unwrap/expect-panic IS the assertion path"
)]
mod tests {
    use flui_types::geometry::px;

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

    #[test]
    fn register_font_rejects_data_with_no_loadable_faces() {
        let binding = PaintingBinding::instance();
        let err = binding
            .register_font(b"this is plainly not a font file")
            .unwrap_err();
        assert!(
            matches!(err, PaintingError::RegisterFontFailed { .. }),
            "garbage bytes must fail with RegisterFontFailed, got {err:?}"
        );
    }

    #[test]
    fn a_registered_font_is_visible_through_a_fresh_handle() {
        // Shared workspace font fixture (dev-only `include_bytes!`; a build
        // input, not an API/layering coupling — flui-engine still depends on
        // flui-painting, never the reverse).
        const ROBOTO: &[u8] = include_bytes!("../../flui-engine/assets/fonts/Roboto-Regular.ttf");
        let binding = PaintingBinding::instance();

        binding
            .register_font(ROBOTO)
            .expect("Roboto-Regular.ttf is a valid TTF with at least one face");

        // A *separately obtained* handle sees the face — proving the ADR-0016
        // contract that `font_system()` shares one instance rather than
        // handing out isolated copies.
        let visible = binding.font_system().with_mut(|font_system| {
            font_system.db().faces().any(|face| {
                face.families
                    .iter()
                    .any(|(name, _)| name.contains("Roboto"))
            })
        });
        assert!(
            visible,
            "a registered face must be visible through any font_system() handle"
        );
    }
}
