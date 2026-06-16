//! Texture atlas for efficient image rendering
//!
//! This module provides texture atlas management for packing multiple images
//! into a single GPU texture, reducing draw calls and improving performance.

use std::collections::HashMap;

use wgpu::{
    Device, Extent3d, Queue, Texture, TextureDescriptor, TextureDimension, TextureFormat,
    TextureUsages,
};

/// Rectangle in atlas space
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AtlasRect {
    /// X coordinate (pixels)
    pub x: u32,
    /// Y coordinate (pixels)
    pub y: u32,
    /// Width (pixels)
    pub width: u32,
    /// Height (pixels)
    pub height: u32,
}

impl AtlasRect {
    /// Create a new atlas rectangle
    #[must_use]
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Get UV coordinates (0.0 - 1.0) for this rect in the atlas
    ///
    /// # Arguments
    ///
    /// * `atlas_width` - Total atlas width
    /// * `atlas_height` - Total atlas height
    ///
    /// # Returns
    ///
    /// (min_u, min_v, max_u, max_v)
    #[must_use]
    pub fn uv_coords(&self, atlas_width: u32, atlas_height: u32) -> ([f32; 2], [f32; 2]) {
        let min_u = self.x as f32 / atlas_width as f32;
        let min_v = self.y as f32 / atlas_height as f32;
        let max_u = (self.x + self.width) as f32 / atlas_width as f32;
        let max_v = (self.y + self.height) as f32 / atlas_height as f32;

        ([min_u, min_v], [max_u, max_v])
    }
}

/// Texture atlas entry
///
/// Cycle 4 wave 5 E-10: dropped the `image_id: u32` field. It was
/// set on construction but read by zero consumers in the workspace
/// -- the `HashMap<u32, AtlasEntry>` keying inside `TextureAtlas`
/// is the canonical ID source, and the field was duplicate
/// bookkeeping.
#[derive(Clone, Debug)]
pub struct AtlasEntry {
    /// Rectangle in atlas
    pub rect: AtlasRect,
}

/// Maximum dimension (width or height) for images eligible for atlas packing.
///
/// Images with both width and height at or below this threshold are routed to
/// the atlas instead of getting a standalone GPU texture. Typical icons and
/// thumbnails are well below 256x256.
pub const ATLAS_MAX_DIMENSION: u32 = 256;

/// Default atlas texture size (2048x2048).
pub const ATLAS_DEFAULT_SIZE: u32 = 2048;

/// Returns `true` when the given image dimensions fit inside the atlas.
#[must_use]
pub fn fits_in_atlas(width: u32, height: u32) -> bool {
    width <= ATLAS_MAX_DIMENSION && height <= ATLAS_MAX_DIMENSION && width > 0 && height > 0
}

/// Simple texture atlas using shelf packing
///
/// Packs images into horizontal shelves to minimize wasted space.
/// Good for images of varying sizes.
pub struct TextureAtlas {
    /// GPU texture
    texture: Texture,

    /// Atlas width
    width: u32,

    /// Atlas height
    height: u32,

    /// Current shelf Y position
    current_shelf_y: u32,

    /// Current shelf height
    current_shelf_height: u32,

    /// Current X position in shelf
    current_x: u32,

    /// Allocated entries
    entries: HashMap<u32, AtlasEntry>,

    /// Next available image ID
    next_image_id: u32,
}

impl TextureAtlas {
    /// Create a new texture atlas
    ///
    /// # Arguments
    ///
    /// * `device` - GPU device
    /// * `width` - Atlas width in pixels
    /// * `height` - Atlas height in pixels
    /// * `format` - Texture format
    pub fn new(device: &Device, width: u32, height: u32, format: TextureFormat) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("Texture Atlas"),
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });

        Self {
            texture,
            width,
            height,
            current_shelf_y: 0,
            current_shelf_height: 0,
            current_x: 0,
            entries: HashMap::new(),
            next_image_id: 1,
        }
    }

    /// Allocate space for an image in the atlas
    ///
    /// # Arguments
    ///
    /// * `width` - Image width
    /// * `height` - Image height
    ///
    /// # Returns
    ///
    /// Image ID and atlas rectangle, or None if atlas is full
    pub fn allocate(&mut self, width: u32, height: u32) -> Option<(u32, AtlasRect)> {
        // Check if image fits in current shelf
        if self.current_x + width <= self.width && self.current_shelf_y + height <= self.height {
            // Update shelf height if needed
            if height > self.current_shelf_height {
                self.current_shelf_height = height;
            }

            let rect = AtlasRect::new(self.current_x, self.current_shelf_y, width, height);
            self.current_x += width;

            let image_id = self.next_image_id;
            self.next_image_id += 1;

            self.entries.insert(image_id, AtlasEntry { rect });

            Some((image_id, rect))
        } else {
            // Try next shelf
            self.current_shelf_y += self.current_shelf_height;
            self.current_shelf_height = height;
            self.current_x = 0;

            if self.current_shelf_y + height <= self.height && width <= self.width {
                let rect = AtlasRect::new(self.current_x, self.current_shelf_y, width, height);
                self.current_x += width;

                let image_id = self.next_image_id;
                self.next_image_id += 1;

                self.entries.insert(image_id, AtlasEntry { rect });

                Some((image_id, rect))
            } else {
                None // Atlas is full
            }
        }
    }

    /// Reclaim the entire atlas: drop every entry and rewind the shelf cursor.
    ///
    /// The shelf packer is append-only — once `allocate` walks the cursor to the
    /// bottom-right it can never reuse a freed slot, so a long-lived atlas
    /// eventually fills with stale entries and `allocate` returns `None`
    /// forever. `reset` is the freeing mechanism: it clears `entries` and
    /// rewinds the cursor so the GPU texture (reused as-is) can be re-packed
    /// from scratch.
    ///
    /// # Invalidates outstanding rects
    ///
    /// Every [`AtlasRect`] / `image_id` handed out by `allocate` before this
    /// call is invalidated — the regions they point at will be overwritten by
    /// future uploads. Callers that cache atlas UVs (e.g. `TextureCache`) MUST
    /// drop those cached entries in the same step. Old pixels are not cleared;
    /// they become garbage that the next `upload_image` overwrites.
    pub fn reset(&mut self) {
        self.entries.clear();
        self.current_shelf_y = 0;
        self.current_shelf_height = 0;
        self.current_x = 0;
        self.next_image_id = 1;
    }

    /// Upload image data to the atlas
    ///
    /// # Arguments
    ///
    /// * `queue` - GPU queue
    /// * `image_id` - Image ID from allocate()
    /// * `data` - Image data (RGBA8)
    pub fn upload_image(&self, queue: &Queue, image_id: u32, data: &[u8]) {
        if let Some(entry) = self.entries.get(&image_id) {
            let rect = entry.rect;

            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: rect.x,
                        y: rect.y,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(rect.width * 4), // RGBA8
                    rows_per_image: Some(rect.height),
                },
                Extent3d {
                    width: rect.width,
                    height: rect.height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    // Cycle 4 wave 5 E-10: `get_entry(image_id)` and `texture()`
    // getters deleted. Workspace grep returned zero callers for
    // either; the entries `HashMap` is queried internally via
    // `insert_image` / `pack_image` paths, and the texture is
    // consumed exclusively through `create_view()` (live).
    // `Texture` is wgpu's own type so the alternative `&self.texture`
    // accessor is trivial to reintroduce if a future consumer needs
    // direct access.

    /// Create a [`wgpu::TextureView`] for the atlas texture.
    ///
    /// The view covers the entire atlas and is suitable for binding to a
    /// render pipeline so that all atlas-backed images can be drawn in a
    /// single instanced draw call.
    #[must_use]
    pub fn create_view(&self) -> wgpu::TextureView {
        self.texture
            .create_view(&wgpu::TextureViewDescriptor::default())
    }

    /// Get atlas dimensions
    #[must_use]
    pub const fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Get number of allocated images
    #[must_use]
    pub fn image_count(&self) -> usize {
        self.entries.len()
    }

    /// Calculate atlas utilization (0.0 - 1.0)
    #[must_use]
    pub fn utilization(&self) -> f32 {
        let mut used_pixels = 0u32;

        for entry in self.entries.values() {
            used_pixels += entry.rect.width * entry.rect.height;
        }

        let total_pixels = self.width * self.height;
        used_pixels as f32 / total_pixels as f32
    }
}

/// Unit tests that do NOT require a GPU.
#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_fits_in_atlas_small_image() {
        assert!(fits_in_atlas(64, 64));
        assert!(fits_in_atlas(256, 256));
        assert!(fits_in_atlas(1, 1));
        assert!(fits_in_atlas(128, 64));
    }

    #[test]
    fn test_fits_in_atlas_too_large() {
        assert!(!fits_in_atlas(257, 64));
        assert!(!fits_in_atlas(64, 257));
        assert!(!fits_in_atlas(512, 512));
        assert!(!fits_in_atlas(1024, 1024));
    }

    #[test]
    fn test_fits_in_atlas_zero_dimension() {
        assert!(!fits_in_atlas(0, 64));
        assert!(!fits_in_atlas(64, 0));
        assert!(!fits_in_atlas(0, 0));
    }

    #[test]
    fn test_atlas_max_dimension_constant() {
        assert_eq!(ATLAS_MAX_DIMENSION, 256);
    }

    #[test]
    fn test_atlas_default_size_constant() {
        assert_eq!(ATLAS_DEFAULT_SIZE, 2048);
    }
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
#[allow(
    clippy::float_cmp,
    reason = "tests assert exact expected values produced by exact arithmetic"
)]
mod tests {
    use super::*;

    #[test]
    fn test_atlas_rect_creation() {
        let rect = AtlasRect::new(10, 20, 100, 50);
        assert_eq!(rect.x, 10);
        assert_eq!(rect.y, 20);
        assert_eq!(rect.width, 100);
        assert_eq!(rect.height, 50);
    }

    #[test]
    fn test_atlas_rect_uv_coords() {
        let rect = AtlasRect::new(0, 0, 512, 512);
        let (min_uv, max_uv) = rect.uv_coords(1024, 1024);

        assert_eq!(min_uv, [0.0, 0.0]);
        assert_eq!(max_uv, [0.5, 0.5]);
    }

    #[test]
    fn test_atlas_rect_uv_coords_offset() {
        let rect = AtlasRect::new(256, 256, 256, 256);
        let (min_uv, max_uv) = rect.uv_coords(1024, 1024);

        assert_eq!(min_uv, [0.25, 0.25]);
        assert_eq!(max_uv, [0.5, 0.5]);
    }

    #[test]
    fn test_texture_atlas_exists() {
        // Compile-time check
        let _ = std::marker::PhantomData::<TextureAtlas>;
    }

    /// Headless GPU device for atlas allocation/reset tests.
    fn test_device() -> Device {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .expect("a GPU adapter for atlas tests");
        let (device, _queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("Atlas Test Device"),
                ..Default::default()
            }))
            .expect("a GPU device for atlas tests");
        device
    }

    #[test]
    fn reset_reclaims_a_full_atlas() {
        let device = test_device();
        // A 64x64 atlas fits exactly one 64x64 image, then is full.
        let mut atlas = TextureAtlas::new(&device, 64, 64, TextureFormat::Rgba8UnormSrgb);

        assert!(atlas.allocate(64, 64).is_some(), "first 64x64 must fit");
        assert!(
            atlas.allocate(1, 1).is_none(),
            "atlas must report full — the shelf packer cannot reuse freed space"
        );
        assert_eq!(atlas.image_count(), 1);

        atlas.reset();

        assert_eq!(atlas.image_count(), 0, "reset drops all entries");
        assert!(
            atlas.allocate(64, 64).is_some(),
            "reset must rewind the shelf cursor so the atlas is allocatable again"
        );
    }
}
