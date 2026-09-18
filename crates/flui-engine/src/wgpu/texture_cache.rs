//! Texture Cache for GPU Texture Management
//!
//! Provides efficient texture loading, caching, and reuse across frames.
//!
//! # Performance Impact
//!
//! **Before (No Caching):**
//! ```text
//! Frame N:
//!   Load texture from disk        ← I/O overhead 1
//!   Decode PNG/JPEG               ← CPU overhead 1
//!   Upload to GPU                 ← GPU overhead 1
//!
//! Frame N+1:
//!   Load same texture again       ← I/O overhead 2 (WASTED!)
//!   Decode again                  ← CPU overhead 2 (WASTED!)
//!   Upload again                  ← GPU overhead 2 (WASTED!)
//! ```
//!
//! **After (With Caching):**
//! ```text
//! Frame N:
//!   Load texture from disk        ← I/O overhead (once)
//!   Decode PNG/JPEG               ← CPU overhead (once)
//!   Upload to GPU                 ← GPU overhead (once)
//!   Cache for reuse               ← HashMap insert
//!
//! Frame N+1:
//!   Lookup in cache               ← O(1) HashMap get
//!   Reuse GPU texture             ← Zero overhead!
//! ```
//!
//! **Result:** 100% reuse after first load, ~1000x faster for repeated
//! textures!

use std::{collections::HashMap, sync::Arc};

use wgpu::{
    Device, Extent3d, Origin3d, Queue, TexelCopyBufferLayout, TexelCopyTextureInfo,
    TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
};

/// GPU texture format used for all decoded image data (atlas, standalone, placeholder).
///
/// # Why `Rgba8Unorm` and NOT `Rgba8UnormSrgb`
///
/// The onscreen surface format is `Bgra8Unorm` / `Rgba8Unorm` (UNorm, plain
/// gamma-space storage, per Impeller parity — see `renderer.rs`
/// `select_surface_format`). When sampling an `*Srgb` texture, the GPU
/// hardware applies the sRGB→linear EOTF on every texel read, converting the
/// stored byte/255 value to a linear float. The shader then writes that linear
/// float into the UNorm surface, which stores it as-is — so a mid-tone byte
/// 0x80 in the PNG becomes ≈0x37 on screen (much too dark).
///
/// Using `Rgba8Unorm` for image textures means the GPU samples byte/255
/// verbatim (no EOTF), the shader outputs that value unchanged, and the UNorm
/// surface stores it as byte/255 — so 0x80 in → 0x80 out. This matches
/// Impeller's behavior: `solid_fill.frag` and the image blit shader both
/// operate in gamma space with UNorm textures and a UNorm surface.
pub(crate) const IMAGE_TEXTURE_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

/// Cache key for a texture: what a caller asks for, not an external texture
/// identity (that is `flui_types::painting::TextureId`).
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub(crate) enum TextureKey {
    /// Data-based texture with hash
    Data(u64),
    /// Pointer-based identity (Arc data pointer address).
    ///
    /// O(1) identity derived from `Arc::as_ptr()`. Images sharing the same
    /// `Arc<Vec<u8>>` allocation produce the same key, avoiding expensive
    /// full-data hashing on every frame.
    Pointer(usize),
}

impl TextureKey {
    /// Create from raw bytes with hash
    pub(crate) fn from_data(data: &[u8]) -> Self {
        use std::{
            collections::hash_map::DefaultHasher,
            hash::{Hash, Hasher},
        };

        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        Self::Data(hasher.finish())
    }

    /// Create from an `Arc` data pointer address (O(1) identity).
    ///
    /// Use with [`flui_types::painting::Image::data_ptr()`] so that images
    /// sharing the same underlying allocation are deduplicated without
    /// hashing the full pixel buffer.
    pub(crate) fn from_ptr(ptr: usize) -> Self {
        Self::Pointer(ptr)
    }
}

/// Cached texture entry
#[derive(Debug)]
pub(crate) struct CachedTexture {
    /// Texture view for rendering. The underlying `wgpu::Texture` is kept
    /// alive by the view itself (`wgpu::TextureView` holds a clone of it),
    /// so a separate `texture` field would be a second strong reference no
    /// reader ever takes.
    pub view: TextureView,
    /// Number of times this texture has been used
    pub use_count: usize,
    /// Size in bytes (for memory tracking)
    pub size_bytes: usize,
    /// UV rectangle for atlas entries.
    ///
    /// `None` means this texture occupies its own standalone GPU texture
    /// (UVs are implicitly `[0, 0, 1, 1]`).
    ///
    /// `Some([u_min, v_min, u_max, v_max])` means this entry lives inside
    /// the shared `TextureAtlas` (`super::atlas::TextureAtlas`) and should use these UVs
    /// when constructing a `TextureInstance` (`super::instancing::TextureInstance`).
    pub uv_rect: Option<[f32; 4]>,
}

impl CachedTexture {
    /// Create new cached texture entry (standalone, not in atlas)
    fn new(view: TextureView, width: u32, height: u32) -> Self {
        // Widen to usize BEFORE multiplying: `width * height * 4` in u32 panics
        // (debug overflow-checks) for large dimensions whose product exceeds
        // u32::MAX. RGBA8 = 4 bytes per pixel.
        let size_bytes = width as usize * height as usize * 4;
        Self {
            view,
            use_count: 0,
            size_bytes,
            uv_rect: None,
        }
    }

    /// Create a cached texture entry backed by the shared atlas.
    fn new_atlas(atlas_view: TextureView, width: u32, height: u32, uv_rect: [f32; 4]) -> Self {
        // Size accounting: the pixels live inside the atlas, but we still
        // track per-entry byte usage for memory budgeting. Widen to usize before
        // multiplying so large dimensions cannot overflow u32.
        let size_bytes = width as usize * height as usize * 4;
        Self {
            view: atlas_view,
            use_count: 0,
            size_bytes,
            uv_rect: Some(uv_rect),
        }
    }

    /// Returns `true` when this entry is stored inside the shared atlas.
    #[must_use]
    pub(crate) fn is_atlas_entry(&self) -> bool {
        self.uv_rect.is_some()
    }

    /// Increment use counter
    fn record_use(&mut self) {
        self.use_count += 1;
    }
}

/// Outcome of one frame-boundary [`TextureCache::end_frame_maintenance`] pass.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FrameMaintenance {
    /// Number of standalone textures evicted to stay within the memory budget.
    pub evicted: usize,
    /// `true` when the shared atlas was reclaimed this frame.
    pub atlas_reset: bool,
}

/// GPU Texture Cache
///
/// Manages texture loading, caching, and reuse for optimal performance.
///
/// Small images (both dimensions <= `ATLAS_MAX_DIMENSION` private constant)
/// are automatically packed into a shared `TextureAtlas`,
/// reducing draw calls for icon-heavy UIs. Larger images get standalone
/// GPU textures as before.
///
/// The record path reaches it through `load_from_rgba` (bytes the caller
/// already decoded) and `get`; frame-boundary reclamation runs through
/// `end_frame_maintenance`.
// `missing_debug_implementations` is a crate-level `#[expect]`: these types
// hold `wgpu` handles, whose lack of `Debug` is the whole reason it exists.
pub(crate) struct TextureCache {
    /// Cached textures by ID
    textures: HashMap<TextureKey, CachedTexture>,
    /// Statistics
    cache_hits: usize,
    cache_misses: usize,
    /// Device reference (for creating textures) - Arc for safe shared ownership
    device: Arc<Device>,
    /// Queue reference (for uploading data) - Arc for safe shared ownership
    queue: Arc<Queue>,
    /// Maximum memory budget in bytes (default 100 MB)
    max_memory_bytes: usize,
    /// Shared texture atlas for small images (icons, thumbnails).
    ///
    /// Images with both dimensions <= `ATLAS_MAX_DIMENSION` are packed here.
    /// When the atlas is full, allocation falls back to standalone textures.
    atlas: super::atlas::TextureAtlas,
    /// Set when an atlas allocation fails (the shelf packer is full). Read at
    /// the frame boundary by [`Self::end_frame_maintenance`] to decide whether
    /// to reclaim the atlas; cleared once the atlas is reset.
    atlas_full: bool,
}

impl TextureCache {
    /// Create a new texture cache
    ///
    /// # Arguments
    /// * `device` - WGPU device for creating textures (Arc for safe sharing)
    /// * `queue` - WGPU queue for uploading texture data (Arc for safe sharing)
    pub(crate) fn new(device: Arc<Device>, queue: Arc<Queue>) -> Self {
        let atlas = super::atlas::TextureAtlas::new(
            &device,
            super::atlas::ATLAS_DEFAULT_SIZE,
            super::atlas::ATLAS_DEFAULT_SIZE,
            IMAGE_TEXTURE_FORMAT,
        );

        tracing::debug!(
            size = super::atlas::ATLAS_DEFAULT_SIZE,
            threshold = super::atlas::ATLAS_MAX_DIMENSION,
            "Texture atlas initialized"
        );

        Self {
            textures: HashMap::new(),
            cache_hits: 0,
            cache_misses: 0,
            device,
            queue,
            max_memory_bytes: 100 * 1024 * 1024, // 100 MB default
            atlas,
            atlas_full: false,
        }
    }

    /// Load texture from RGBA bytes
    ///
    /// # Arguments
    /// * `id` - Texture identifier
    /// * `width` - Texture width
    /// * `height` - Texture height
    /// * `data` - RGBA8 pixel data (width × height × 4 bytes)
    pub(crate) fn load_from_rgba(
        &mut self,
        id: TextureKey,
        width: u32,
        height: u32,
        data: &[u8],
    ) -> Result<&CachedTexture, String> {
        use std::collections::hash_map::Entry;

        // Validate data size. Widen to usize BEFORE multiplying so an oversized
        // (width, height) returns a clean Err here instead of panicking in the
        // u32 multiply under debug overflow-checks.
        let expected_size = width as usize * height as usize * 4;
        if data.len() != expected_size {
            return Err(format!(
                "Invalid RGBA data size: expected {}, got {}",
                expected_size,
                data.len()
            ));
        }

        // Use entry API to avoid double lookup
        match self.textures.entry(id) {
            Entry::Occupied(mut entry) => {
                // Cache hit
                self.cache_hits += 1;
                entry.get_mut().record_use();
                Ok(entry.into_mut())
            }
            Entry::Vacant(entry) => {
                // Cache miss - create texture
                self.cache_misses += 1;

                let queue = &self.queue;

                // Try atlas for small images (icons, thumbnails)
                if super::atlas::fits_in_atlas(width, height) {
                    if let Some((image_id, rect)) = self.atlas.allocate(width, height) {
                        // Upload to atlas sub-region
                        self.atlas.upload_image(queue, image_id, data);

                        let (atlas_w, atlas_h) = self.atlas.dimensions();
                        let (min_uv, max_uv) = rect.uv_coords(atlas_w, atlas_h);
                        let uv_rect = [min_uv[0], min_uv[1], max_uv[0], max_uv[1]];

                        // Re-use the atlas GPU texture and view for the cache entry
                        // NOTE: wgpu::Texture is not Clone, so we create a fresh
                        // view each time.  All atlas entries share the same
                        // underlying GPU allocation — the view is lightweight.
                        let atlas_view = self.atlas.create_view();

                        let cached_texture =
                            CachedTexture::new_atlas(atlas_view, width, height, uv_rect);

                        tracing::trace!(
                            width,
                            height,
                            image_id,
                            ?uv_rect,
                            "Image packed into atlas"
                        );

                        return Ok(entry.insert(cached_texture));
                    }
                    // Atlas full — fall through to standalone texture, and flag
                    // the atlas for frame-boundary reclamation (see
                    // `end_frame_maintenance`).
                    self.atlas_full = true;
                    tracing::debug!(
                        width,
                        height,
                        atlas_utilization = %format!("{:.1}%", self.atlas.utilization() * 100.0),
                        "Atlas full, falling back to standalone texture"
                    );
                }

                // Standalone texture (large image or atlas full)
                let device = &self.device;

                let texture = device.create_texture(&TextureDescriptor {
                    label: Some("Cached Texture"),
                    size: Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: IMAGE_TEXTURE_FORMAT,
                    usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                    view_formats: &[],
                });

                // Upload data
                queue.write_texture(
                    TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: 0,
                        origin: Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    data,
                    TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(4 * width),
                        rows_per_image: Some(height),
                    },
                    Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                );

                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                let cached_texture = CachedTexture::new(view, width, height);

                Ok(entry.insert(cached_texture))
            }
        }
    }

    /// Check if texture is cached. Test-only: the record path decides
    /// hit/miss through `load_from_rgba`'s own entry lookup, and no
    /// production caller asks this question separately.
    #[cfg(all(test, feature = "enable-wgpu-tests"))]
    pub(crate) fn contains(&self, id: &TextureKey) -> bool {
        self.textures.contains_key(id)
    }

    /// Get cached texture (without loading)
    pub(crate) fn get(&mut self, id: &TextureKey) -> Option<&CachedTexture> {
        if let Some(cached) = self.textures.get_mut(id) {
            cached.record_use();
            self.cache_hits += 1;
            Some(cached)
        } else {
            None
        }
    }

    /// Total memory used by all cached textures in bytes
    pub(crate) fn memory_bytes(&self) -> usize {
        self.textures.values().map(|t| t.size_bytes).sum()
    }

    /// Evict textures when total memory exceeds the budget
    ///
    /// Removes textures with `use_count == 0` until memory is within budget.
    /// Returns the number of evicted textures.
    pub(crate) fn evict_over_budget(&mut self) -> usize {
        let current = self.memory_bytes();
        if current <= self.max_memory_bytes {
            return 0;
        }

        // Collect unused texture keys sorted by size (largest first for fastest reclaim)
        let mut unused: Vec<(TextureKey, usize)> = self
            .textures
            .iter()
            .filter(|(_, t)| t.use_count == 0)
            .map(|(id, t)| (id.clone(), t.size_bytes))
            .collect();
        unused.sort_by_key(|entry| std::cmp::Reverse(entry.1));

        let mut freed = 0usize;
        let mut evicted = 0usize;
        let overshoot = current - self.max_memory_bytes;

        for (id, size) in unused {
            self.textures.remove(&id);
            freed += size;
            evicted += 1;
            if freed >= overshoot {
                break;
            }
        }

        evicted
    }

    /// Reset use counters.
    ///
    /// Sets all `use_count` to 0 so the next frame can detect unused textures.
    /// Run at the END of frame maintenance, after eviction has read this
    /// frame's counts — see [`Self::end_frame_maintenance`].
    pub(crate) fn reset_use_counters(&mut self) {
        for texture in self.textures.values_mut() {
            texture.use_count = 0;
        }
    }

    /// Reclaim the shared atlas if it filled up and holds stale entries.
    ///
    /// The shelf packer never frees individual slots, so once it fills, every
    /// subsequent small image falls back to a standalone texture and loses
    /// atlas batching for the rest of the session. When that has happened AND
    /// at least one atlas-backed entry went unused this frame (a stale slot to
    /// reclaim), drop ALL atlas entries and reset the packer; the still-live
    /// ones re-pack from their source image on the next frame (a one-frame
    /// standalone blip, then back in the atlas).
    ///
    /// Skips the reset when every atlas entry was used this frame — the working
    /// set genuinely exceeds the atlas, so a reset would immediately refill and
    /// thrash. Returns `true` when the atlas was reset.
    ///
    /// Must run before [`Self::reset_use_counters`] so per-entry `use_count`
    /// still reflects this frame.
    fn maybe_reset_atlas(&mut self) -> bool {
        if !self.atlas_full {
            return false;
        }
        let has_stale = self
            .textures
            .values()
            .any(|t| t.is_atlas_entry() && t.use_count == 0);
        if !has_stale {
            // Working set fills the atlas; resetting now would only thrash.
            return false;
        }
        self.textures.retain(|_, t| !t.is_atlas_entry());
        self.atlas.reset();
        self.atlas_full = false;
        true
    }

    /// Frame-boundary cache maintenance — call once per frame AFTER rendering.
    ///
    /// Order is load-bearing: stale-atlas detection and budget eviction read
    /// this frame's `use_count`s, THEN the counters reset for the next frame. A
    /// previous call site reset the counters FIRST and then removed every
    /// `use_count == 0` entry, which wiped the entire cache every frame and
    /// defeated cross-frame reuse. Encapsulating the sequence here keeps callers
    /// from reintroducing that ordering bug.
    pub(crate) fn end_frame_maintenance(&mut self) -> FrameMaintenance {
        let atlas_reset = self.maybe_reset_atlas();
        let evicted = self.evict_over_budget();
        self.reset_use_counters();
        FrameMaintenance {
            evicted,
            atlas_reset,
        }
    }

    // ===== Asset Integration =====

    // ===== Atlas Access =====
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod tests {
    use super::*;

    /// BUG 4 regression: an absurdly large `(width, height)` must return a clean
    /// `Err` (size mismatch), NOT panic in the size multiply.
    ///
    /// `40000 * 40000 * 4 = 6.4e9` exceeds `u32::MAX` (4.29e9). The old code
    /// computed `(width * height * 4) as usize` — the multiply ran in u32 and
    /// panicked under debug overflow-checks BEFORE the `data.len()` guard. Widen
    /// to usize first so validation rejects the input gracefully.
    #[test]
    fn load_from_rgba_oversized_dimensions_errors_without_panic() {
        let (device, queue) =
            crate::wgpu::test_support::test_device_and_queue("TextureCache Test Device");
        let mut cache = TextureCache::new(device, queue);

        // Empty data, gigantic dimensions: the size check must fire first.
        let result = cache.load_from_rgba(TextureKey::from_data(b"big"), 40000, 40000, &[]);
        assert!(
            result.is_err(),
            "oversized dimensions must return Err (size mismatch), not panic in \
             the u32 size multiply"
        );
    }

    /// Regression: frame maintenance must RETAIN textures used this frame.
    ///
    /// The previous call site reset the use-counters and THEN removed every
    /// zero-count entry, wiping the entire cache every frame. `end_frame_main`
    /// now evicts (budget-gated) before resetting, so an under-budget cache
    /// keeps its entries for cross-frame reuse.
    #[test]
    fn end_frame_maintenance_retains_used_texture() {
        let (device, queue) =
            crate::wgpu::test_support::test_device_and_queue("TextureCache Test Device");
        let mut cache = TextureCache::new(device, queue);
        let id = TextureKey::from_data(b"retained");
        // 4x4 RGBA — far under the default 100 MB budget.
        // 4x4 <= ATLAS_MAX_DIMENSION -> atlas-backed path.
        cache
            .load_from_rgba(id.clone(), 4, 4, &[0u8; 4 * 4 * 4])
            .expect("rgba upload");
        // 300x300 > ATLAS_MAX_DIMENSION -> standalone path. The per-frame wipe
        // removed atlas AND standalone entries alike, so cover both here.
        let big = TextureKey::from_data(b"retained_standalone");
        cache
            .load_from_rgba(big.clone(), 300, 300, &vec![0u8; 300 * 300 * 4])
            .expect("rgba upload");
        // A frame draws both (record_use -> use_count > 0).
        assert!(cache.get(&id).is_some());
        assert!(cache.get(&big).is_some());

        cache.end_frame_maintenance();
        assert!(
            cache.contains(&id),
            "an atlas-backed texture used this frame must survive frame maintenance"
        );
        assert!(
            cache.contains(&big),
            "a standalone texture used this frame must survive frame maintenance"
        );

        // A second, idle frame under budget also retains them for reuse.
        cache.end_frame_maintenance();
        assert!(
            cache.contains(&id) && cache.contains(&big),
            "under budget, cached textures persist across idle frames"
        );
    }
}
