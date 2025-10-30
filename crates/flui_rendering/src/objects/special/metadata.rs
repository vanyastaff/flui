//! RenderMetaData - attaches metadata to child_id for parent access

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::BoxedLayer;
use flui_types::{Offset, Size, constraints::BoxConstraints};
use std::any::Any;

/// Data for RenderMetaData
#[derive(Debug)]
pub struct MetaData {
    /// Metadata value (can be any type)
    pub metadata: Option<Box<dyn Any + Send + Sync>>,
    /// Whether hit testing should use this metadata
    pub behavior: HitTestBehavior,
}

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

impl MetaData {
    /// Create new metadata data
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
}

impl Default for MetaData {
    fn default() -> Self {
        Self::new()
    }
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
    /// Metadata data
    pub data: MetaData,
}

// ===== Public API =====

impl RenderMetaData {
    /// Create new RenderMetaData
    pub fn new() -> Self {
        Self {
            data: MetaData::new(),
        }
    }

    /// Create with metadata
    pub fn with_metadata<T: Any + Send + Sync>(metadata: T) -> Self {
        Self {
            data: MetaData::with_metadata(metadata),
        }
    }

    /// Create with behavior
    pub fn with_behavior(behavior: HitTestBehavior) -> Self {
        Self {
            data: MetaData::with_behavior(behavior),
        }
    }

    /// Get behavior
    pub fn behavior(&self) -> HitTestBehavior {
        self.data.behavior
    }

    /// Check if has metadata
    pub fn has_metadata(&self) -> bool {
        self.data.metadata.is_some()
    }

    /// Try to get metadata as specific type
    pub fn metadata<T: Any>(&self) -> Option<&T> {
        self.data
            .metadata
            .as_ref()
            .and_then(|m| m.downcast_ref::<T>())
    }

    /// Set behavior
    pub fn set_behavior(&mut self, behavior: HitTestBehavior) {
        if self.data.behavior != behavior {
            self.data.behavior = behavior;
        }
    }

    /// Set metadata
    pub fn set_metadata<T: Any + Send + Sync>(&mut self, metadata: T) {
        self.data.metadata = Some(Box::new(metadata));
    }

    /// Clear metadata
    pub fn clear_metadata(&mut self) {
        self.data.metadata = None;
    }
}

impl Default for RenderMetaData {
    fn default() -> Self {
        Self::new()
    }
}

// ===== RenderObject Implementation =====

impl SingleRender for RenderMetaData {
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
    fn test_metadata_data_new() {
        let data = MetaDataData::new();
        assert!(data.metadata.is_none());
        assert_eq!(data.behavior, HitTestBehavior::Defer);
    }

    #[test]
    fn test_metadata_data_with_metadata() {
        let test_data = TestMetadata { value: 42 };
        let data = MetaDataData::with_metadata(test_data);
        assert!(data.metadata.is_some());
        assert_eq!(data.behavior, HitTestBehavior::Defer);
    }

    #[test]
    fn test_metadata_data_with_behavior() {
        let data = MetaDataData::with_behavior(HitTestBehavior::Opaque);
        assert!(data.metadata.is_none());
        assert_eq!(data.behavior, HitTestBehavior::Opaque);
    }

    #[test]
    fn test_render_metadata_new() {
        let meta = RenderMetaData::new();
        assert!(!meta.has_metadata());
        assert_eq!(meta.behavior(), HitTestBehavior::Defer);
    }

    #[test]
    fn test_render_metadata_with_metadata() {
        let test_data = TestMetadata { value: 42 };
        let meta = RenderMetaData::with_metadata(test_data.clone());
        assert!(meta.has_metadata());

        let retrieved = meta.metadata::<TestMetadata>();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().value, 42);
    }

    #[test]
    fn test_render_metadata_with_behavior() {
        let meta = RenderMetaData::with_behavior(HitTestBehavior::Opaque);
        assert_eq!(meta.behavior(), HitTestBehavior::Opaque);
    }

    #[test]
    fn test_render_metadata_set_metadata() {
        let mut meta = RenderMetaData::new();
        let test_data = TestMetadata { value: 123 };

        meta.set_metadata(test_data.clone());
        assert!(meta.has_metadata());

        let retrieved = meta.metadata::<TestMetadata>();
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
        assert_eq!(meta.behavior(), HitTestBehavior::Translucent);
    }
}
