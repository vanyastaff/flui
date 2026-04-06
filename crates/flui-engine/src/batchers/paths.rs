//! Path batching for tessellated vector graphics.
//!
//! [`PathBatcher`] tessellates fill and stroke paths via lyon,
//! accumulating [`PathVertex`] and index data for batch drawing.

use lyon::path::Path;
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, FillVertexConstructor, StrokeOptions,
    StrokeTessellator, StrokeVertex, StrokeVertexConstructor, VertexBuffers,
};

use crate::vertex::PathVertex;

// ============================================================================
// Vertex constructors for lyon
// ============================================================================

/// Constructs [`PathVertex`] from lyon fill output, stamping a uniform color.
struct FillWithColor([f32; 4]);

impl FillVertexConstructor<PathVertex> for FillWithColor {
    fn new_vertex(&mut self, vertex: FillVertex<'_>) -> PathVertex {
        PathVertex::new(vertex.position().to_array(), self.0)
    }
}

/// Constructs [`PathVertex`] from lyon stroke output, stamping a uniform color.
struct StrokeWithColor([f32; 4]);

impl StrokeVertexConstructor<PathVertex> for StrokeWithColor {
    fn new_vertex(&mut self, vertex: StrokeVertex<'_, '_>) -> PathVertex {
        PathVertex::new(vertex.position().to_array(), self.0)
    }
}

// ============================================================================
// Types
// ============================================================================

/// Tracks a contiguous range of indices belonging to one path draw call.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PathDrawRange {
    /// First index in the shared index buffer.
    pub start_index: u32,
    /// Number of indices for this path.
    pub index_count: u32,
}

// ============================================================================
// PathBatcher
// ============================================================================

/// Collects tessellated path geometry into shared vertex and index buffers.
///
/// Reusable across frames — call [`clear`](Self::clear) between frames to
/// reset geometry while keeping tessellator allocations alive.
pub struct PathBatcher {
    fill_tessellator: FillTessellator,
    stroke_tessellator: StrokeTessellator,
    vertices: Vec<PathVertex>,
    indices: Vec<u32>,
    draw_ranges: Vec<PathDrawRange>,
}

impl PathBatcher {
    /// Create a new, empty path batcher with fresh tessellators.
    #[must_use]
    pub fn new() -> Self {
        Self {
            fill_tessellator: FillTessellator::new(),
            stroke_tessellator: StrokeTessellator::new(),
            vertices: Vec::new(),
            indices: Vec::new(),
            draw_ranges: Vec::new(),
        }
    }

    /// Tessellate a filled path and append the resulting geometry.
    ///
    /// If tessellation fails the path is silently skipped and a warning is logged.
    pub fn add_fill(&mut self, path: &Path, color: [f32; 4]) {
        let mut buffers: VertexBuffers<PathVertex, u32> = VertexBuffers::new();

        if let Err(e) = self.fill_tessellator.tessellate_path(
            path,
            &FillOptions::default(),
            &mut BuffersBuilder::new(&mut buffers, FillWithColor(color)),
        ) {
            tracing::warn!("Fill tessellation failed, skipping path: {e}");
            return;
        }

        self.append_buffers(&buffers);
    }

    /// Tessellate a stroked path and append the resulting geometry.
    ///
    /// If tessellation fails the path is silently skipped and a warning is logged.
    pub fn add_stroke(&mut self, path: &Path, color: [f32; 4], line_width: f32) {
        let mut buffers: VertexBuffers<PathVertex, u32> = VertexBuffers::new();

        let options = StrokeOptions::default().with_line_width(line_width);

        if let Err(e) = self.stroke_tessellator.tessellate_path(
            path,
            &options,
            &mut BuffersBuilder::new(&mut buffers, StrokeWithColor(color)),
        ) {
            tracing::warn!("Stroke tessellation failed, skipping path: {e}");
            return;
        }

        self.append_buffers(&buffers);
    }

    /// Add pre-tessellated vertices and indices directly.
    ///
    /// If `colors` is provided it must have the same length as `vertices`;
    /// otherwise `default_color` is used for every vertex.
    pub fn add_vertices(
        &mut self,
        vertices: &[[f32; 2]],
        colors: Option<&[[f32; 4]]>,
        indices: &[u32],
        default_color: [f32; 4],
    ) {
        if vertices.is_empty() || indices.is_empty() {
            return;
        }

        let base_vertex = self.vertices.len() as u32;
        let start_index = self.indices.len() as u32;

        for (i, &pos) in vertices.iter().enumerate() {
            let color = colors
                .and_then(|c| c.get(i))
                .copied()
                .unwrap_or(default_color);
            self.vertices.push(PathVertex::new(pos, color));
        }

        self.indices
            .extend(indices.iter().map(|&idx| idx + base_vertex));

        self.draw_ranges.push(PathDrawRange {
            start_index,
            index_count: indices.len() as u32,
        });
    }

    // -- Accessors -----------------------------------------------------------

    /// Number of accumulated vertices.
    #[must_use]
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Number of accumulated indices.
    #[must_use]
    pub fn index_count(&self) -> usize {
        self.indices.len()
    }

    /// Number of recorded draw ranges.
    #[must_use]
    pub fn draw_range_count(&self) -> usize {
        self.draw_ranges.len()
    }

    /// Returns `true` when no geometry has been accumulated.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.vertices.is_empty()
    }

    /// Read-only access to the accumulated vertices.
    #[must_use]
    pub fn vertices(&self) -> &[PathVertex] {
        &self.vertices
    }

    /// Read-only access to the accumulated indices.
    #[must_use]
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// Read-only access to the draw ranges.
    #[must_use]
    pub fn draw_ranges(&self) -> &[PathDrawRange] {
        &self.draw_ranges
    }

    // -- Lifecycle -----------------------------------------------------------

    /// Clear all accumulated geometry, keeping tessellator state and
    /// allocated capacity.
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
        self.draw_ranges.clear();
    }

    /// Take ownership of the vertex buffer, leaving an empty vec in place.
    ///
    /// Useful for pool recycling — the caller can return the buffer via
    /// [`restore`](Self::restore).
    pub fn take_vertices(&mut self) -> Vec<PathVertex> {
        std::mem::take(&mut self.vertices)
    }

    /// Take ownership of the index buffer, leaving an empty vec in place.
    pub fn take_indices(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.indices)
    }

    /// Restore previously taken buffers so their capacity can be reused.
    pub fn restore(&mut self, vertices: Vec<PathVertex>, indices: Vec<u32>) {
        self.vertices = vertices;
        self.indices = indices;
        self.vertices.clear();
        self.indices.clear();
    }

    // -- Internal ------------------------------------------------------------

    /// Append pre-built lyon buffers, offsetting indices and recording a draw range.
    fn append_buffers(&mut self, buffers: &VertexBuffers<PathVertex, u32>) {
        if buffers.vertices.is_empty() || buffers.indices.is_empty() {
            return;
        }

        let base_vertex = self.vertices.len() as u32;
        let start_index = self.indices.len() as u32;

        self.vertices.extend_from_slice(&buffers.vertices);

        self.indices
            .extend(buffers.indices.iter().map(|&idx| idx + base_vertex));

        self.draw_ranges.push(PathDrawRange {
            start_index,
            index_count: buffers.indices.len() as u32,
        });
    }
}

impl Default for PathBatcher {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use lyon::math::point;

    /// Helper: build a simple closed rectangle path.
    fn rect_path() -> Path {
        let mut builder = Path::builder();
        builder.begin(point(0.0, 0.0));
        builder.line_to(point(100.0, 0.0));
        builder.line_to(point(100.0, 100.0));
        builder.line_to(point(0.0, 100.0));
        builder.close();
        builder.build()
    }

    #[test]
    fn empty_path_batcher() {
        let batcher = PathBatcher::new();
        assert!(batcher.is_empty());
        assert_eq!(batcher.vertex_count(), 0);
        assert_eq!(batcher.index_count(), 0);
        assert_eq!(batcher.draw_range_count(), 0);
    }

    #[test]
    fn add_fill_produces_vertices() {
        let mut batcher = PathBatcher::new();
        let path = rect_path();
        let color = [1.0, 0.0, 0.0, 1.0];

        batcher.add_fill(&path, color);

        assert!(!batcher.is_empty());
        assert!(batcher.vertex_count() > 0);
        assert!(batcher.index_count() > 0);
        assert_eq!(batcher.draw_range_count(), 1);

        // All vertices should have the requested color
        for v in batcher.vertices() {
            assert_eq!(v.color, color);
        }
    }

    #[test]
    fn add_stroke_produces_vertices() {
        let mut batcher = PathBatcher::new();
        let path = rect_path();
        let color = [0.0, 1.0, 0.0, 1.0];

        batcher.add_stroke(&path, color, 2.0);

        assert!(!batcher.is_empty());
        assert!(batcher.vertex_count() > 0);
        assert!(batcher.index_count() > 0);
        assert_eq!(batcher.draw_range_count(), 1);

        for v in batcher.vertices() {
            assert_eq!(v.color, color);
        }
    }

    #[test]
    fn add_vertices_direct() {
        let mut batcher = PathBatcher::new();

        let verts: &[[f32; 2]] = &[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];
        let idxs: &[u32] = &[0, 1, 2];
        let color = [0.0, 0.0, 1.0, 1.0];

        batcher.add_vertices(verts, None, idxs, color);

        assert_eq!(batcher.vertex_count(), 3);
        assert_eq!(batcher.index_count(), 3);
        assert_eq!(batcher.draw_range_count(), 1);

        for v in batcher.vertices() {
            assert_eq!(v.color, color);
        }
    }

    #[test]
    fn add_vertices_with_per_vertex_colors() {
        let mut batcher = PathBatcher::new();

        let verts: &[[f32; 2]] = &[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];
        let colors: &[[f32; 4]] = &[
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0, 1.0],
        ];
        let idxs: &[u32] = &[0, 1, 2];

        batcher.add_vertices(verts, Some(colors), idxs, [0.0; 4]);

        assert_eq!(batcher.vertex_count(), 3);
        assert_eq!(batcher.vertices()[0].color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(batcher.vertices()[1].color, [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(batcher.vertices()[2].color, [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn clear_resets() {
        let mut batcher = PathBatcher::new();
        batcher.add_fill(&rect_path(), [1.0; 4]);
        assert!(!batcher.is_empty());

        batcher.clear();
        assert!(batcher.is_empty());
        assert_eq!(batcher.vertex_count(), 0);
        assert_eq!(batcher.index_count(), 0);
        assert_eq!(batcher.draw_range_count(), 0);
    }

    #[test]
    fn multiple_paths_accumulate() {
        let mut batcher = PathBatcher::new();
        let path = rect_path();

        batcher.add_fill(&path, [1.0, 0.0, 0.0, 1.0]);
        let verts_after_first = batcher.vertex_count();
        let idxs_after_first = batcher.index_count();

        batcher.add_fill(&path, [0.0, 1.0, 0.0, 1.0]);

        assert_eq!(batcher.draw_range_count(), 2);
        assert!(batcher.vertex_count() > verts_after_first);
        assert!(batcher.index_count() > idxs_after_first);

        // Second draw range should start where the first ended
        let ranges = batcher.draw_ranges();
        assert_eq!(ranges[0].start_index, 0);
        assert_eq!(
            ranges[1].start_index,
            ranges[0].start_index + ranges[0].index_count
        );
    }

    #[test]
    fn take_and_restore() {
        let mut batcher = PathBatcher::new();
        batcher.add_fill(&rect_path(), [1.0; 4]);

        let verts = batcher.take_vertices();
        let idxs = batcher.take_indices();
        assert!(batcher.is_empty());
        assert!(!verts.is_empty());
        assert!(!idxs.is_empty());

        // Restore empties the buffers but keeps capacity
        let cap_v = verts.capacity();
        let cap_i = idxs.capacity();
        batcher.restore(verts, idxs);
        assert!(batcher.is_empty());
        assert!(batcher.vertices.capacity() >= cap_v);
        assert!(batcher.indices.capacity() >= cap_i);
    }

    #[test]
    fn indices_are_correctly_offset() {
        let mut batcher = PathBatcher::new();

        // Add direct triangle
        let verts: &[[f32; 2]] = &[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];
        batcher.add_vertices(verts, None, &[0, 1, 2], [1.0; 4]);

        // Now add a fill — its indices should be offset by 3
        batcher.add_fill(&rect_path(), [1.0; 4]);

        // All indices from the second batch should be >= 3
        let range = &batcher.draw_ranges()[1];
        let start = range.start_index as usize;
        let end = start + range.index_count as usize;
        for &idx in &batcher.indices()[start..end] {
            assert!(idx >= 3, "index {idx} should be offset by at least 3");
        }
    }
}
