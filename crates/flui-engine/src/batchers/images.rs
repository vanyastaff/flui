//! Image and texture quad batching.
//!
//! Accumulates [`ImageQuadInstance`] entries grouped by texture ID for
//! efficient batched draw calls. Each unique texture produces one draw call,
//! and insertion order is preserved for correct draw ordering.

use std::collections::HashMap;

use crate::vertex::ImageQuadInstance;

/// Collects image quad instances grouped by texture for batched GPU submission.
///
/// Images sharing the same `texture_id` are batched into a single draw call.
/// The insertion order of unique texture IDs is preserved so that draw calls
/// respect the original submission order (important for correct layering).
pub struct ImageBatcher {
    /// Instances grouped by texture ID.
    groups: HashMap<u64, Vec<ImageQuadInstance>>,
    /// Insertion order of texture IDs (first-seen order).
    order: Vec<u64>,
}

impl ImageBatcher {
    /// Create an empty image batcher.
    #[must_use]
    pub fn new() -> Self {
        Self {
            groups: HashMap::new(),
            order: Vec::new(),
        }
    }

    /// Add an image quad instance for the given texture.
    ///
    /// If this is the first instance for `texture_id`, a new group is created
    /// and the texture ID is appended to the insertion-order list.
    pub fn add_image(
        &mut self,
        texture_id: u64,
        dst_bounds: [f32; 4],
        src_uv: [f32; 4],
        color: [f32; 4],
        transform: [f32; 4],
    ) {
        let instance = ImageQuadInstance {
            dst_bounds,
            src_uv,
            color,
            transform,
        };

        let group = self.groups.entry(texture_id).or_insert_with(|| {
            self.order.push(texture_id);
            Vec::new()
        });
        group.push(instance);
    }

    /// Number of unique textures (draw call groups).
    #[must_use]
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    /// Total number of image quad instances across all groups.
    #[must_use]
    pub fn total_instance_count(&self) -> usize {
        self.groups.values().map(Vec::len).sum()
    }

    /// Returns `true` if no images have been added.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// Clear all groups and insertion order, keeping allocated memory.
    pub fn clear(&mut self) {
        self.groups.clear();
        self.order.clear();
    }

    /// Iterate over groups in insertion order.
    ///
    /// Each item is `(texture_id, instances)` where instances is the slice
    /// of [`ImageQuadInstance`] for that texture.
    pub fn groups_in_order(&self) -> impl Iterator<Item = (u64, &[ImageQuadInstance])> {
        self.order.iter().filter_map(|&id| {
            self.groups
                .get(&id)
                .map(|instances| (id, instances.as_slice()))
        })
    }
}

impl Default for ImageBatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    const FULL_UV: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
    const IDENTITY: [f32; 4] = [1.0, 0.0, 0.0, 0.0];

    #[test]
    fn empty_image_batcher() {
        let batcher = ImageBatcher::new();
        assert!(batcher.is_empty());
        assert_eq!(batcher.group_count(), 0);
        assert_eq!(batcher.total_instance_count(), 0);
    }

    #[test]
    fn add_image_accumulates() {
        let mut batcher = ImageBatcher::new();
        batcher.add_image(1, [0.0, 0.0, 100.0, 100.0], FULL_UV, WHITE, IDENTITY);
        batcher.add_image(1, [100.0, 0.0, 100.0, 100.0], FULL_UV, WHITE, IDENTITY);
        assert_eq!(batcher.group_count(), 1);
        assert_eq!(batcher.total_instance_count(), 2);
        assert!(!batcher.is_empty());
    }

    #[test]
    fn different_textures_separate_groups() {
        let mut batcher = ImageBatcher::new();
        batcher.add_image(1, [0.0; 4], FULL_UV, WHITE, IDENTITY);
        batcher.add_image(2, [0.0; 4], FULL_UV, WHITE, IDENTITY);
        assert_eq!(batcher.group_count(), 2);
        assert_eq!(batcher.total_instance_count(), 2);
    }

    #[test]
    fn clear_resets() {
        let mut batcher = ImageBatcher::new();
        batcher.add_image(1, [0.0; 4], FULL_UV, WHITE, IDENTITY);
        batcher.add_image(2, [0.0; 4], FULL_UV, WHITE, IDENTITY);
        batcher.clear();
        assert!(batcher.is_empty());
        assert_eq!(batcher.group_count(), 0);
        assert_eq!(batcher.total_instance_count(), 0);
    }

    #[test]
    fn groups_in_order_preserves_insertion() {
        let mut batcher = ImageBatcher::new();
        batcher.add_image(3, [0.0; 4], FULL_UV, WHITE, IDENTITY);
        batcher.add_image(1, [0.0; 4], FULL_UV, WHITE, IDENTITY);

        let ids: Vec<u64> = batcher.groups_in_order().map(|(id, _)| id).collect();
        assert_eq!(ids, vec![3, 1]);
    }
}
