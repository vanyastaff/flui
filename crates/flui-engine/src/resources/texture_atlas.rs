//! Texture atlas packing for batching small textures into larger ones.
//!
//! Uses shelf-packing algorithm for efficient allocation of small textures
//! (glyphs, icons) into a single larger GPU texture.

use std::collections::HashMap;

/// Rectangle within an atlas, representing an allocated region.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AtlasRect {
    /// X offset in pixels from the left edge of the atlas.
    pub x: u32,
    /// Y offset in pixels from the top edge of the atlas.
    pub y: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl AtlasRect {
    /// Compute normalized UV coordinates for this rectangle within an atlas
    /// of the given dimensions.
    ///
    /// Returns `[u_min, v_min, u_max, v_max]`.
    pub fn uv_coords(&self, atlas_width: u32, atlas_height: u32) -> [f32; 4] {
        [
            self.x as f32 / atlas_width as f32,
            self.y as f32 / atlas_height as f32,
            (self.x + self.width) as f32 / atlas_width as f32,
            (self.y + self.height) as f32 / atlas_height as f32,
        ]
    }
}

/// Shelf-packing texture atlas for small textures (glyphs, icons).
///
/// Allocates rectangles by placing them left-to-right on shelves. When
/// a rectangle doesn't fit on the current shelf, a new shelf is started
/// below. The shelf height is determined by the tallest item on that shelf.
pub struct TextureAtlas {
    width: u32,
    height: u32,
    current_x: u32,
    current_y: u32,
    shelf_height: u32,
    entries: HashMap<u64, AtlasRect>,
}

impl TextureAtlas {
    /// Create a new empty atlas with the given pixel dimensions.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            current_x: 0,
            current_y: 0,
            shelf_height: 0,
            entries: HashMap::new(),
        }
    }

    /// Allocate a rectangle in the atlas using shelf packing.
    ///
    /// Returns `None` if the atlas has no room for the requested size.
    pub fn allocate(&mut self, id: u64, width: u32, height: u32) -> Option<AtlasRect> {
        if width > self.width || height > self.height {
            return None;
        }

        // Try to fit on the current shelf
        if self.current_x + width <= self.width {
            // Check vertical space (current shelf or new shelf height)
            let needed_height = if self.shelf_height > 0 {
                self.current_y + self.shelf_height.max(height)
            } else {
                self.current_y + height
            };
            if needed_height <= self.height {
                let rect = AtlasRect {
                    x: self.current_x,
                    y: self.current_y,
                    width,
                    height,
                };
                self.current_x += width;
                if height > self.shelf_height {
                    self.shelf_height = height;
                }
                self.entries.insert(id, rect);
                return Some(rect);
            }
        }

        // Start a new shelf
        let new_y = self.current_y + self.shelf_height;
        if new_y + height > self.height {
            return None;
        }
        if width > self.width {
            return None;
        }

        self.current_x = width;
        self.current_y = new_y;
        self.shelf_height = height;

        let rect = AtlasRect {
            x: 0,
            y: new_y,
            width,
            height,
        };
        self.entries.insert(id, rect);
        Some(rect)
    }

    /// Look up a previously allocated rectangle by ID.
    pub fn get(&self, id: u64) -> Option<&AtlasRect> {
        self.entries.get(&id)
    }

    /// Remove an entry by ID. Note: this does not reclaim atlas space.
    pub fn remove(&mut self, id: u64) {
        self.entries.remove(&id);
    }

    /// Clear all entries and reset shelf state.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.current_x = 0;
        self.current_y = 0;
        self.shelf_height = 0;
    }

    /// Return the approximate fraction of atlas area in use.
    pub fn usage_fraction(&self) -> f32 {
        let total = self.width as f64 * self.height as f64;
        if total == 0.0 {
            return 0.0;
        }
        let used: f64 = self
            .entries
            .values()
            .map(|r| r.width as f64 * r.height as f64)
            .sum();
        (used / total) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_atlas() {
        let atlas = TextureAtlas::new(256, 256);
        assert_eq!(atlas.usage_fraction(), 0.0);
    }

    #[test]
    fn allocate_single() {
        let mut atlas = TextureAtlas::new(256, 256);
        let rect = atlas.allocate(1, 32, 32).unwrap();
        assert_eq!(rect.x, 0);
        assert_eq!(rect.y, 0);
        assert_eq!(rect.width, 32);
        assert_eq!(rect.height, 32);
    }

    #[test]
    fn allocate_fills_shelf() {
        let mut atlas = TextureAtlas::new(256, 256);
        atlas.allocate(1, 100, 32).unwrap();
        let r2 = atlas.allocate(2, 100, 32).unwrap();
        assert_eq!(r2.x, 100); // same shelf
        assert_eq!(r2.y, 0);
    }

    #[test]
    fn allocate_new_shelf() {
        let mut atlas = TextureAtlas::new(256, 256);
        atlas.allocate(1, 200, 32).unwrap();
        let r2 = atlas.allocate(2, 200, 32).unwrap();
        // Doesn't fit on first shelf (200+200 > 256), starts new shelf
        assert_eq!(r2.x, 0);
        assert_eq!(r2.y, 32);
    }

    #[test]
    fn allocate_returns_none_when_full() {
        let mut atlas = TextureAtlas::new(64, 64);
        atlas.allocate(1, 64, 64).unwrap();
        assert!(atlas.allocate(2, 1, 1).is_none());
    }

    #[test]
    fn uv_coords() {
        let rect = AtlasRect {
            x: 10,
            y: 20,
            width: 30,
            height: 40,
        };
        let uv = rect.uv_coords(100, 100);
        assert_eq!(uv, [0.1, 0.2, 0.4, 0.6]);
    }

    #[test]
    fn clear_resets() {
        let mut atlas = TextureAtlas::new(256, 256);
        atlas.allocate(1, 100, 100).unwrap();
        atlas.clear();
        assert_eq!(atlas.usage_fraction(), 0.0);
        assert!(atlas.get(1).is_none());
    }

    #[test]
    fn get_returns_allocated_rect() {
        let mut atlas = TextureAtlas::new(256, 256);
        let rect = atlas.allocate(42, 16, 16).unwrap();
        assert_eq!(atlas.get(42), Some(&rect));
    }

    #[test]
    fn remove_entry() {
        let mut atlas = TextureAtlas::new(256, 256);
        atlas.allocate(1, 32, 32).unwrap();
        atlas.remove(1);
        assert!(atlas.get(1).is_none());
    }

    #[test]
    fn too_large_returns_none() {
        let mut atlas = TextureAtlas::new(64, 64);
        assert!(atlas.allocate(1, 65, 32).is_none());
        assert!(atlas.allocate(2, 32, 65).is_none());
    }

    #[test]
    fn multiple_shelves() {
        let mut atlas = TextureAtlas::new(100, 100);
        // Shelf 1: two 50x20 items
        atlas.allocate(1, 50, 20).unwrap();
        atlas.allocate(2, 50, 20).unwrap();
        // Shelf 2: need new shelf since 50+50+50 > 100
        let r3 = atlas.allocate(3, 50, 30).unwrap();
        assert_eq!(r3.x, 0);
        assert_eq!(r3.y, 20);
    }

    #[test]
    fn usage_fraction_accuracy() {
        let mut atlas = TextureAtlas::new(100, 100);
        atlas.allocate(1, 50, 50).unwrap();
        let frac = atlas.usage_fraction();
        assert!((frac - 0.25).abs() < 0.001);
    }
}
