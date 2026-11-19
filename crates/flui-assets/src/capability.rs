//! [`ImageCacheCapability`]: the decoded-image cache of the UI runtime a
//! widget belongs to (ADR-0387).

use std::cell::OnceCell;
use std::sync::Arc;

use flui_foundation::memory::{MemoryPressureLevel, ReclaimMemory};
use flui_platform::Unsupported;
use flui_platform::capability::{Capability, ProviderContext};

use crate::image_cache::{ImageCache, ImageCacheConfig};

/// The image cache of the acquiring widget's UI runtime:
/// `ctx.capability::<ImageCacheCapability>()` hands out an `Arc` of it.
///
/// Every host registers [`per_runtime`](Self::per_runtime), so the images of
/// one runtime share their decoded pixels and their loads, and the runtime
/// trims them under memory pressure. An application with other needs (another
/// budget, one cache across its runtimes) provides its own through
/// `Application::capability::<ImageCacheCapability>(provider)`.
#[derive(Debug)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a capability marker has no values; it is only ever named"
)]
pub enum ImageCacheCapability {}

impl Capability for ImageCacheCapability {
    type Handle = Arc<ImageCache>;
    const NAME: &'static str = "image-cache";
}

impl ImageCacheCapability {
    /// The built-in provider: one cache per UI runtime, made with
    /// [`ImageCacheConfig::default`] at the runtime's first acquisition,
    /// trimmed by the runtime under memory pressure and ended with it.
    ///
    /// # Errors
    ///
    /// Never: every runtime has a cache.
    pub fn per_runtime(ctx: &ProviderContext<'_>) -> Result<Arc<ImageCache>, Unsupported> {
        let slot = ctx.runtime_cache::<RuntimeImageCache>();
        Ok(Arc::clone(slot.0.get_or_init(|| {
            Arc::new(ImageCache::new(ImageCacheConfig::default()))
        })))
    }
}

/// One runtime's cache, made at its first acquisition. A runtime that never
/// shows an image keeps none.
#[derive(Default)]
struct RuntimeImageCache(OnceCell<Arc<ImageCache>>);

impl ReclaimMemory for RuntimeImageCache {
    fn reclaim(&self, level: MemoryPressureLevel) {
        if let Some(cache) = self.0.get() {
            cache.reclaim(level);
        }
    }
}
