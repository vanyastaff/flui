//! RenderMetaData - attaches metadata to child for parent access

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};
use std::any::Any;

/// Data for RenderMetaData
#[derive(Debug)]
pub struct MetaDataData {
    /// Metadata value (can be any type)
    pub metadata: Option<Box<dyn Any + Send + Sync>>,
    /// Whether hit testing should use this metadata
    pub behavior: HitTestBehavior,
}

/// Hit test behavior for metadata
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitTestBehavior {
    /// Defer to child
    Defer,
    /// Always include this widget in hit tests
    Opaque,
    /// Include if pointer is inside bounds
    Translucent,
}

impl MetaDataData {
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

impl Default for MetaDataData {
    fn default() -> Self {
        Self::new()
    }
}

/// RenderObject that attaches metadata to its child
///
/// This is a transparent widget that stores arbitrary metadata.
/// Parent widgets can access this metadata during hit testing or layout.
///
/// Useful for passing information up the tree without affecting layout or paint.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::special::MetaDataData};
///
/// // Attach custom metadata to child
/// #[derive(Debug)]
/// struct MyMetadata {
///     id: i32,
///     label: String,
/// }
///
/// let metadata = MyMetadata { id: 42, label: "Item".to_string() };
/// let mut meta = SingleRenderBox::new(MetaDataData::with_metadata(metadata));
/// ```
pub type RenderMetaData = SingleRenderBox<MetaDataData>;

// ===== Public API =====

impl RenderMetaData {
    /// Get behavior
    pub fn behavior(&self) -> HitTestBehavior {
        self.data().behavior
    }

    /// Check if has metadata
    pub fn has_metadata(&self) -> bool {
        self.data().metadata.is_some()
    }

    /// Try to get metadata as specific type
    pub fn get_metadata<T: Any>(&self) -> Option<&T> {
        self.data()
            .metadata
            .as_ref()
            .and_then(|m| m.downcast_ref::<T>())
    }

    /// Set behavior
    pub fn set_behavior(&mut self, behavior: HitTestBehavior) {
        if self.data().behavior != behavior {
            self.data_mut().behavior = behavior;
        }
    }

    /// Set metadata
    pub fn set_metadata<T: Any + Send + Sync>(&mut self, metadata: T) {
        self.data_mut().metadata = Some(Box::new(metadata));
    }

    /// Clear metadata
    pub fn clear_metadata(&mut self) {
        self.data_mut().metadata = None;
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderMetaData {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        // Layout child with same constraints (pass-through)
        let size = if let Some(child) = self.child_mut() {
            child.layout(constraints)
        } else {
            constraints.smallest()
        };

        // Store size and clear needs_layout flag
        self.state_mut().size = Some(size);
        self.clear_needs_layout();

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        // Paint child directly (pass-through)
        if let Some(child) = self.child() {
            child.paint(painter, offset);
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
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
        let meta = SingleRenderBox::new(MetaDataData::new());
        assert!(!meta.has_metadata());
        assert_eq!(meta.behavior(), HitTestBehavior::Defer);
    }

    #[test]
    fn test_render_metadata_set_metadata() {
        let mut meta = SingleRenderBox::new(MetaDataData::new());
        let test_data = TestMetadata { value: 123 };

        meta.set_metadata(test_data.clone());
        assert!(meta.has_metadata());

        let retrieved = meta.get_metadata::<TestMetadata>();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), &test_data);
    }

    #[test]
    fn test_render_metadata_clear_metadata() {
        let mut meta = SingleRenderBox::new(MetaDataData::with_metadata(TestMetadata { value: 42 }));
        assert!(meta.has_metadata());

        meta.clear_metadata();
        assert!(!meta.has_metadata());
    }

    #[test]
    fn test_render_metadata_set_behavior() {
        let mut meta = SingleRenderBox::new(MetaDataData::new());

        meta.set_behavior(HitTestBehavior::Translucent);
        assert_eq!(meta.behavior(), HitTestBehavior::Translucent);
    }

    #[test]
    fn test_render_metadata_layout() {
        let mut meta = SingleRenderBox::new(MetaDataData::new());
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = meta.layout(constraints);

        // No child, should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }
}
