//! RenderMetaData - attaches metadata to child_id for parent access

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::BoxedLayer;
use flui_types::{constraints::BoxConstraints, Offset, Size};
use std::any::Any;

/// Hit test behavior for metadata
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitTestBehavior {
    /// Defer to child_id
    Defer,
    /// Always include this widget in hit tests
    Opaque,
    /// Include if pointer is inside bounds
    Translucent,
}

/// RenderObject that attaches metadata to its child_id
///
/// This is a transparent widget that stores arbitrary metadata.
/// Parent widgets can access this metadata during hit testing or layout.
///
/// Useful for passing information up the tree without affecting layout or paint.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderMetaData;
///
/// // Attach custom metadata to child_id
/// #[derive(Debug)]
/// struct MyMetadata {
///     id: i32,
///     label: String,
/// }
///
/// let metadata = MyMetadata { id: 42, label: "Item".to_string() };
/// let mut meta = RenderMetaData::with_metadata(metadata);
/// ```
#[derive(Debug)]
pub struct RenderMetaData {
    /// Metadata value (can be any type)
    pub metadata: Option<Box<dyn Any + Send + Sync>>,
    /// Whether hit testing should use this metadata
    pub behavior: HitTestBehavior,
}

// ===== Public API =====

impl RenderMetaData {
    /// Create new RenderMetaData
    pub fn new() -> Self {
        Self {
            metadata: None,
            behavior: HitTestBehavior::Defer,
        }
    }

    /// Create with metadata
    pub fn with_metadata<T: Any + Send + Sync>(metadata: T) -> Self {
        Self {
            metadata: Some(Box::new(metadata)),
            behavior: HitTestBehavior::Defer,
        }
    }

    /// Create with behavior
    pub fn with_behavior(behavior: HitTestBehavior) -> Self {
        Self {
            metadata: None,
            behavior,
        }
    }

    /// Check if has metadata
    pub fn has_metadata(&self) -> bool {
        self.metadata.is_some()
    }

    /// Try to get metadata as specific type
    pub fn get_metadata<T: Any>(&self) -> Option<&T> {
        self.metadata.as_ref().and_then(|m| m.downcast_ref::<T>())
    }

    /// Set behavior
    pub fn set_behavior(&mut self, behavior: HitTestBehavior) {
        if self.behavior != behavior {
            self.behavior = behavior;
        }
    }

    /// Set metadata
    pub fn set_metadata<T: Any + Send + Sync>(&mut self, metadata: T) {
        self.metadata = Some(Box::new(metadata));
    }

    /// Clear metadata
    pub fn clear_metadata(&mut self) {
        self.metadata = None;
    }
}

impl Default for RenderMetaData {
    fn default() -> Self {
        Self::new()
    }
}

// ===== RenderObject Implementation =====

impl SingleRender for RenderMetaData {
    /// No metadata needed
    type Metadata = ();

    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        // Layout child_id with same constraints (pass-through)
        tree.layout_child(child_id, constraints)
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        // Paint child_id directly (pass-through)
        tree.paint_child(child_id, offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct TestMetadata {
        value: i32,
    }

    #[test]
    fn test_hit_test_behavior_variants() {
        assert_ne!(HitTestBehavior::Defer, HitTestBehavior::Opaque);
        assert_ne!(HitTestBehavior::Opaque, HitTestBehavior::Translucent);
    }

    #[test]
    fn test_render_metadata_default_state() {
        let meta = RenderMetaData::new();
        assert!(meta.metadata.is_none());
        assert_eq!(meta.behavior, HitTestBehavior::Defer);
    }

    #[test]
    fn test_render_metadata_with_metadata_state() {
        let test_data = TestMetadata { value: 42 };
        let meta = RenderMetaData::with_metadata(test_data);
        assert!(meta.metadata.is_some());
        assert_eq!(meta.behavior, HitTestBehavior::Defer);
    }

    #[test]
    fn test_render_metadata_with_behavior_state() {
        let meta = RenderMetaData::with_behavior(HitTestBehavior::Opaque);
        assert!(meta.metadata.is_none());
        assert_eq!(meta.behavior, HitTestBehavior::Opaque);
    }

    #[test]
    fn test_render_metadata_new() {
        let meta = RenderMetaData::new();
        assert!(!meta.has_metadata());
        assert_eq!(meta.behavior, HitTestBehavior::Defer);
    }

    #[test]
    fn test_render_metadata_with_metadata() {
        let test_data = TestMetadata { value: 42 };
        let meta = RenderMetaData::with_metadata(test_data.clone());
        assert!(meta.has_metadata());

        let retrieved = meta.get_metadata::<TestMetadata>();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().value, 42);
    }

    #[test]
    fn test_render_metadata_with_behavior() {
        let meta = RenderMetaData::with_behavior(HitTestBehavior::Opaque);
        assert_eq!(meta.behavior, HitTestBehavior::Opaque);
    }

    #[test]
    fn test_render_metadata_set_metadata() {
        let mut meta = RenderMetaData::new();
        let test_data = TestMetadata { value: 123 };

        meta.set_metadata(test_data.clone());
        assert!(meta.has_metadata());

        let retrieved = meta.get_metadata::<TestMetadata>();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), &test_data);
    }

    #[test]
    fn test_render_metadata_clear_metadata() {
        let mut meta = RenderMetaData::with_metadata(TestMetadata { value: 42 });
        assert!(meta.has_metadata());

        meta.clear_metadata();
        assert!(!meta.has_metadata());
    }

    #[test]
    fn test_render_metadata_set_behavior() {
        let mut meta = RenderMetaData::new();

        meta.set_behavior(HitTestBehavior::Translucent);
        assert_eq!(meta.behavior, HitTestBehavior::Translucent);
    }
}
