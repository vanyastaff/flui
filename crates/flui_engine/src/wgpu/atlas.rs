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
#[derive(Clone, Debug)]
pub struct AtlasEntry {
    /// Rectangle in atlas
    pub rect: AtlasRect,

    /// Image ID
    pub image_id: u32,
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

    /// Texture format
    format: TextureFormat,

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
            format,
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

            self.entries.insert(image_id, AtlasEntry { rect, image_id });

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

                self.entries.insert(image_id, AtlasEntry { rect, image_id });

                Some((image_id, rect))
            } else {
                None // Atlas is full
            }
        }
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
                wgpu::ImageCopyTexture {
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
                wgpu::ImageDataLayout {
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

    /// Get atlas entry for an image
    #[must_use]
    pub fn get_entry(&self, image_id: u32) -> Option<&AtlasEntry> {
        self.entries.get(&image_id)
    }

    /// Get the GPU texture
    #[must_use]
    pub fn texture(&self) -> &Texture {
        &self.texture
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

#[cfg(test)]
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
}
