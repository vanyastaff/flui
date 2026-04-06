//! Shape batching (rectangles, circles, arcs, lines).
//!
//! Accumulates instanced shape primitives for efficient GPU submission.

use crate::vertex::{ArcInstance, CircleInstance, LineInstance, RectInstance};

/// Collects shape primitives into batched instance buffers.
///
/// Each shape type is stored in a separate `Vec` with pre-allocated capacity
/// tuned for typical frame workloads. Use `take_*` / `restore` for pool
/// recycling across frames.
pub struct ShapeBatcher {
    rects: Vec<RectInstance>,
    circles: Vec<CircleInstance>,
    arcs: Vec<ArcInstance>,
    lines: Vec<LineInstance>,
}

impl ShapeBatcher {
    /// Pre-allocated capacities per shape type.
    const RECT_CAPACITY: usize = 256;
    const CIRCLE_CAPACITY: usize = 64;
    const ARC_CAPACITY: usize = 16;
    const LINE_CAPACITY: usize = 64;

    /// Create a new batcher with pre-allocated capacity.
    #[must_use]
    pub fn new() -> Self {
        Self {
            rects: Vec::with_capacity(Self::RECT_CAPACITY),
            circles: Vec::with_capacity(Self::CIRCLE_CAPACITY),
            arcs: Vec::with_capacity(Self::ARC_CAPACITY),
            lines: Vec::with_capacity(Self::LINE_CAPACITY),
        }
    }

    /// Add a rectangle with per-corner radii and a 2D transform.
    pub fn add_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: [f32; 4],
        corner_radii: [f32; 4],
        transform: [f32; 4],
    ) {
        let instance = RectInstance {
            bounds: [x, y, w, h],
            color,
            corner_radii,
            transform: [1.0, 1.0, 0.0, 0.0],
        }
        .with_transform(transform[0], transform[1], transform[2], transform[3]);
        self.rects.push(instance);
    }

    /// Add a circle (equal radii).
    pub fn add_circle(
        &mut self,
        cx: f32,
        cy: f32,
        r: f32,
        color: [f32; 4],
        transform: [f32; 4],
    ) {
        let mut instance = CircleInstance::circle([cx, cy], r, color);
        instance.transform = transform;
        self.circles.push(instance);
    }

    /// Add an oval (ellipse) defined by its bounding box.
    pub fn add_oval(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: [f32; 4],
        transform: [f32; 4],
    ) {
        let cx = x + w / 2.0;
        let cy = y + h / 2.0;
        let rx = w / 2.0;
        let ry = h / 2.0;
        let mut instance = CircleInstance::oval([cx, cy], rx, ry, color);
        instance.transform = transform;
        self.circles.push(instance);
    }

    /// Add an arc (partial circle).
    pub fn add_arc(
        &mut self,
        cx: f32,
        cy: f32,
        radius: f32,
        start_angle: f32,
        sweep_angle: f32,
        color: [f32; 4],
    ) {
        self.arcs.push(ArcInstance::new(
            [cx, cy],
            radius,
            start_angle,
            sweep_angle,
            color,
        ));
    }

    /// Add a line segment.
    pub fn add_line(
        &mut self,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        color: [f32; 4],
        width: f32,
    ) {
        self.lines
            .push(LineInstance::new([x1, y1], [x2, y2], color, width));
    }

    /// Number of accumulated rectangles.
    #[must_use]
    pub fn rect_count(&self) -> usize {
        self.rects.len()
    }

    /// Number of accumulated circles / ovals.
    #[must_use]
    pub fn circle_count(&self) -> usize {
        self.circles.len()
    }

    /// Number of accumulated arcs.
    #[must_use]
    pub fn arc_count(&self) -> usize {
        self.arcs.len()
    }

    /// Number of accumulated lines.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Returns `true` if no shapes have been added.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
            && self.circles.is_empty()
            && self.arcs.is_empty()
            && self.lines.is_empty()
    }

    /// Clear all accumulated shapes, keeping allocated memory.
    pub fn clear(&mut self) {
        self.rects.clear();
        self.circles.clear();
        self.arcs.clear();
        self.lines.clear();
    }

    /// Take the rectangle buffer for pool recycling.
    pub fn take_rects(&mut self) -> Vec<RectInstance> {
        std::mem::take(&mut self.rects)
    }

    /// Take the circle buffer for pool recycling.
    pub fn take_circles(&mut self) -> Vec<CircleInstance> {
        std::mem::take(&mut self.circles)
    }

    /// Take the arc buffer for pool recycling.
    pub fn take_arcs(&mut self) -> Vec<ArcInstance> {
        std::mem::take(&mut self.arcs)
    }

    /// Take the line buffer for pool recycling.
    pub fn take_lines(&mut self) -> Vec<LineInstance> {
        std::mem::take(&mut self.lines)
    }

    /// Restore pre-allocated buffers (e.g. returned from a pool).
    pub fn restore(
        &mut self,
        rects: Vec<RectInstance>,
        circles: Vec<CircleInstance>,
        arcs: Vec<ArcInstance>,
        lines: Vec<LineInstance>,
    ) {
        self.rects = rects;
        self.circles = circles;
        self.arcs = arcs;
        self.lines = lines;
    }
}

impl Default for ShapeBatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
    const IDENTITY: [f32; 4] = [1.0, 1.0, 0.0, 0.0];

    #[test]
    fn empty_batcher_has_no_draws() {
        let batcher = ShapeBatcher::new();
        assert!(batcher.is_empty());
        assert_eq!(batcher.rect_count(), 0);
        assert_eq!(batcher.circle_count(), 0);
        assert_eq!(batcher.arc_count(), 0);
        assert_eq!(batcher.line_count(), 0);
    }

    #[test]
    fn add_rects_accumulates() {
        let mut batcher = ShapeBatcher::new();
        batcher.add_rect(0.0, 0.0, 100.0, 50.0, WHITE, [0.0; 4], IDENTITY);
        batcher.add_rect(10.0, 20.0, 80.0, 40.0, RED, [5.0; 4], IDENTITY);
        assert_eq!(batcher.rect_count(), 2);
        assert!(!batcher.is_empty());
    }

    #[test]
    fn add_circle_accumulates() {
        let mut batcher = ShapeBatcher::new();
        batcher.add_circle(50.0, 50.0, 25.0, WHITE, IDENTITY);
        assert_eq!(batcher.circle_count(), 1);
    }

    #[test]
    fn add_oval_uses_circle_instance() {
        let mut batcher = ShapeBatcher::new();
        batcher.add_oval(10.0, 20.0, 100.0, 60.0, RED, IDENTITY);
        assert_eq!(batcher.circle_count(), 1);
    }

    #[test]
    fn clear_resets_all() {
        let mut batcher = ShapeBatcher::new();
        batcher.add_rect(0.0, 0.0, 10.0, 10.0, WHITE, [0.0; 4], IDENTITY);
        batcher.add_circle(5.0, 5.0, 3.0, RED, IDENTITY);
        batcher.clear();
        assert!(batcher.is_empty());
        assert_eq!(batcher.rect_count(), 0);
        assert_eq!(batcher.circle_count(), 0);
        assert_eq!(batcher.arc_count(), 0);
        assert_eq!(batcher.line_count(), 0);
    }
}
