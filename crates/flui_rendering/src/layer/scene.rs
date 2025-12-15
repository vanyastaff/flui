//! Scene building and representation.

use std::sync::atomic::{AtomicU64, Ordering};

use flui_types::Offset;

use super::base::{SceneBuilder, SceneOperation};
use super::container::OffsetLayer;
use super::Layer;

// ============================================================================
// Scene
// ============================================================================

/// Unique identifier for a scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SceneId(u64);

impl SceneId {
    /// Creates a new unique scene ID.
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Returns the raw ID value.
    pub fn get(&self) -> u64 {
        self.0
    }
}

impl Default for SceneId {
    fn default() -> Self {
        Self::new()
    }
}

/// A composited scene ready for rendering.
///
/// A Scene is built from a layer tree using `SceneBuilder` and represents
/// the final composited output ready to be submitted to the compositor.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `Scene` class from `dart:ui`.
#[derive(Debug)]
pub struct Scene {
    /// Unique identifier.
    id: SceneId,

    /// The operations that make up this scene.
    operations: Vec<SceneOperation>,

    /// Whether this scene has been disposed.
    disposed: bool,
}

impl Scene {
    /// Creates a new scene from operations.
    pub fn new(operations: Vec<SceneOperation>) -> Self {
        Self {
            id: SceneId::new(),
            operations,
            disposed: false,
        }
    }

    /// Returns the scene ID.
    pub fn id(&self) -> SceneId {
        self.id
    }

    /// Returns the operations in this scene.
    pub fn operations(&self) -> &[SceneOperation] {
        &self.operations
    }

    /// Returns whether this scene has been disposed.
    pub fn is_disposed(&self) -> bool {
        self.disposed
    }

    /// Disposes the scene, releasing resources.
    pub fn dispose(&mut self) {
        self.disposed = true;
        self.operations.clear();
    }
}

// ============================================================================
// SceneBuilder Extensions
// ============================================================================

/// Extension methods for building scenes from layers.
impl SceneBuilder {
    /// Builds the scene from a layer tree.
    pub fn build_from_layer(&mut self, layer: &mut dyn Layer) {
        layer.add_to_scene(self, Offset::ZERO);
    }

    /// Builds the scene from an offset layer.
    pub fn build_from_offset_layer(&mut self, layer: &mut OffsetLayer) {
        layer.add_to_scene(self, Offset::ZERO);
    }

    /// Builds and returns the final scene.
    pub fn build(self) -> Scene {
        Scene::new(self.take_operations_owned())
    }

    /// Helper to take operations with ownership.
    fn take_operations_owned(mut self) -> Vec<SceneOperation> {
        self.take_operations()
    }
}

// ============================================================================
// Scene Statistics
// ============================================================================

/// Statistics about a scene.
#[derive(Debug, Clone, Default)]
pub struct SceneStatistics {
    /// Number of push operations.
    pub push_count: usize,
    /// Number of pop operations.
    pub pop_count: usize,
    /// Number of picture operations.
    pub picture_count: usize,
    /// Number of clip operations.
    pub clip_count: usize,
    /// Number of opacity operations.
    pub opacity_count: usize,
    /// Maximum nesting depth.
    pub max_depth: usize,
}

impl SceneStatistics {
    /// Calculates statistics for a scene.
    pub fn from_scene(scene: &Scene) -> Self {
        let mut stats = Self::default();
        let mut current_depth = 0usize;

        for op in &scene.operations {
            match op {
                SceneOperation::PushTransform { .. } | SceneOperation::PushOffset { .. } => {
                    stats.push_count += 1;
                    current_depth += 1;
                    stats.max_depth = stats.max_depth.max(current_depth);
                }
                SceneOperation::PushClipRect { .. } | SceneOperation::PushClipRRect { .. } => {
                    stats.clip_count += 1;
                    stats.push_count += 1;
                    current_depth += 1;
                    stats.max_depth = stats.max_depth.max(current_depth);
                }
                SceneOperation::PushOpacity { .. } => {
                    stats.opacity_count += 1;
                    stats.push_count += 1;
                    current_depth += 1;
                    stats.max_depth = stats.max_depth.max(current_depth);
                }
                SceneOperation::Pop => {
                    stats.pop_count += 1;
                    current_depth = current_depth.saturating_sub(1);
                }
                SceneOperation::AddPicture { .. } => {
                    stats.picture_count += 1;
                }
            }
        }

        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::Rect;

    #[test]
    fn test_scene_id_unique() {
        let id1 = SceneId::new();
        let id2 = SceneId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_scene_builder_empty() {
        let builder = SceneBuilder::new();
        assert!(builder.is_empty());
        assert_eq!(builder.operation_count(), 0);
    }

    #[test]
    fn test_scene_builder_push_offset() {
        let mut builder = SceneBuilder::new();
        builder.push_offset(10.0, 20.0);
        builder.pop();

        assert_eq!(builder.operation_count(), 2);
    }

    #[test]
    fn test_scene_builder_push_clip_rect() {
        let mut builder = SceneBuilder::new();
        let rect = Rect::from_ltwh(0.0, 0.0, 100.0, 100.0);
        builder.push_clip_rect(rect);
        builder.pop();

        let scene = builder.build();
        assert_eq!(scene.operations().len(), 2);
    }

    #[test]
    fn test_scene_builder_add_picture() {
        let mut builder = SceneBuilder::new();
        builder.add_picture(Offset::new(10.0, 20.0), 42);

        let scene = builder.build();
        assert!(!scene.is_disposed());

        let stats = SceneStatistics::from_scene(&scene);
        assert_eq!(stats.picture_count, 1);
    }

    #[test]
    fn test_scene_dispose() {
        let builder = SceneBuilder::new();
        let mut scene = builder.build();

        assert!(!scene.is_disposed());
        scene.dispose();
        assert!(scene.is_disposed());
        assert!(scene.operations().is_empty());
    }

    #[test]
    fn test_scene_statistics() {
        let mut builder = SceneBuilder::new();
        builder.push_offset(0.0, 0.0);
        builder.push_clip_rect(Rect::from_ltwh(0.0, 0.0, 100.0, 100.0));
        builder.push_opacity(128, Offset::ZERO);
        builder.add_picture(Offset::ZERO, 1);
        builder.pop();
        builder.pop();
        builder.pop();

        let scene = builder.build();
        let stats = SceneStatistics::from_scene(&scene);

        assert_eq!(stats.push_count, 3);
        assert_eq!(stats.pop_count, 3);
        assert_eq!(stats.picture_count, 1);
        assert_eq!(stats.clip_count, 1);
        assert_eq!(stats.opacity_count, 1);
        assert_eq!(stats.max_depth, 3);
    }

    #[test]
    fn test_scene_builder_clear() {
        let mut builder = SceneBuilder::new();
        builder.push_offset(10.0, 20.0);
        builder.pop();

        assert!(!builder.is_empty());
        builder.clear();
        assert!(builder.is_empty());
    }
}
