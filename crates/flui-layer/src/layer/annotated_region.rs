//! AnnotatedRegionLayer - Metadata regions
//!
//! This layer annotates a region of the layer tree with metadata.
//! Used for system UI integration (status bar color, etc.) and
//! accessibility regions.

use flui_types::geometry::{Pixels, Rect};
use std::any::Any;
use std::fmt;
use std::sync::Arc;

/// Type-erased annotation value.
pub type AnnotationValue = Arc<dyn Any + Send + Sync>;

/// Layer that annotates a region with metadata.
///
/// AnnotatedRegionLayer allows attaching arbitrary metadata to regions
/// of the UI. This metadata can be queried by the system for various purposes:
///
/// # Use Cases
///
/// - **System UI**: Control status bar appearance (dark/light icons)
/// - **Accessibility**: Mark regions with semantic information
/// - **Analytics**: Track which regions are visible
/// - **Testing**: Mark testable regions
///
/// # Architecture
///
/// ```text
/// AnnotatedRegionLayer<T>
///   │
///   │ Marks region with value of type T
///   ▼
/// Child layers render normally
///   │
///   │ System can query annotations
///   ▼
/// System behavior (e.g., status bar color)
/// ```
///
/// # Example
///
/// ```rust
/// use flui_layer::{AnnotatedRegionLayer, AnnotationValue};
/// use flui_types::geometry::Rect;
/// use std::sync::Arc;
///
/// // Define a status bar style annotation
/// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// pub enum SystemUiOverlayStyle {
///     Light,
///     Dark,
/// }
///
/// // Create an annotation for status bar style
/// let style = Arc::new(SystemUiOverlayStyle::Dark);
/// let layer = AnnotatedRegionLayer::new(
///     Rect::from_xywh(0.0, 0.0, 400.0, 24.0),
///     style,
/// );
/// ```
pub struct AnnotatedRegionLayer {
    /// The annotated region bounds
    rect: Rect<Pixels>,

    /// The annotation value (type-erased)
    value: AnnotationValue,

    /// Whether the region is sized to its children
    sized_by_parent: bool,
}

impl AnnotatedRegionLayer {
    /// Creates a new annotated region layer.
    ///
    /// # Arguments
    ///
    /// * `rect` - The region bounds
    /// * `value` - The annotation value (must be Send + Sync)
    #[inline]
    pub fn new<T: Any + Send + Sync>(rect: Rect<Pixels>, value: Arc<T>) -> Self {
        Self {
            rect,
            value,
            sized_by_parent: false,
        }
    }

    /// Creates an annotated region that is sized by its parent.
    #[inline]
    pub fn sized_by_parent<T: Any + Send + Sync>(value: Arc<T>) -> Self {
        Self {
            rect: Rect::ZERO,
            value,
            sized_by_parent: true,
        }
    }

    /// Sets whether the region is sized by its parent.
    #[inline]
    pub fn with_sized_by_parent(mut self, sized: bool) -> Self {
        self.sized_by_parent = sized;
        self
    }

    /// Returns the region bounds.
    #[inline]
    pub fn rect(&self) -> Rect<Pixels> {
        self.rect
    }

    /// Returns the region bounds.
    #[inline]
    pub fn bounds(&self) -> Rect<Pixels> {
        self.rect
    }

    /// Returns the annotation value as a type-erased reference.
    #[inline]
    pub fn value(&self) -> &AnnotationValue {
        &self.value
    }

    /// Attempts to downcast the annotation value to a specific type.
    #[inline]
    pub fn downcast_value<T: Any>(&self) -> Option<&T> {
        self.value.downcast_ref()
    }

    /// Returns whether the region is sized by its parent.
    #[inline]
    pub fn is_sized_by_parent(&self) -> bool {
        self.sized_by_parent
    }

    /// Sets the region bounds.
    #[inline]
    pub fn set_rect(&mut self, rect: Rect<Pixels>) {
        self.rect = rect;
    }

    /// Sets the annotation value.
    #[inline]
    pub fn set_value<T: Any + Send + Sync>(&mut self, value: Arc<T>) {
        self.value = value;
    }
}

impl Clone for AnnotatedRegionLayer {
    fn clone(&self) -> Self {
        Self {
            rect: self.rect,
            value: Arc::clone(&self.value),
            sized_by_parent: self.sized_by_parent,
        }
    }
}

impl fmt::Debug for AnnotatedRegionLayer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnnotatedRegionLayer")
            .field("rect", &self.rect)
            .field("sized_by_parent", &self.sized_by_parent)
            .field("value_type", &self.value.type_id())
            .finish()
    }
}

// Thread safety is ensured by AnnotationValue = Arc<dyn Any + Send + Sync>
unsafe impl Send for AnnotatedRegionLayer {}
unsafe impl Sync for AnnotatedRegionLayer {}

// ============================================================================
// COMMON ANNOTATION TYPES
// ============================================================================

/// System UI overlay style for status bar appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SystemUiOverlayStyle {
    /// Light icons on dark background
    #[default]
    Light,
    /// Dark icons on light background
    Dark,
}

/// Semantic label for accessibility.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SemanticLabel(pub String);

impl SemanticLabel {
    /// Creates a new semantic label.
    #[inline]
    pub fn new(label: impl Into<String>) -> Self {
        Self(label.into())
    }

    /// Returns the label text.
    #[inline]
    pub fn text(&self) -> &str {
        &self.0
    }
}

impl From<&str> for SemanticLabel {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for SemanticLabel {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_annotated_region_new() {
        let rect = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);
        let value = Arc::new(SystemUiOverlayStyle::Dark);
        let layer = AnnotatedRegionLayer::new(rect, value);

        assert_eq!(layer.rect(), rect);
        assert_eq!(layer.bounds(), rect);
        assert!(!layer.is_sized_by_parent());
    }

    #[test]
    fn test_annotated_region_sized_by_parent() {
        let value = Arc::new(SystemUiOverlayStyle::Light);
        let layer = AnnotatedRegionLayer::sized_by_parent(value);

        assert!(layer.is_sized_by_parent());
        assert_eq!(layer.rect(), Rect::ZERO);
    }

    #[test]
    fn test_annotated_region_downcast() {
        let value = Arc::new(SystemUiOverlayStyle::Dark);
        let layer = AnnotatedRegionLayer::new(Rect::ZERO, value);

        let style = layer.downcast_value::<SystemUiOverlayStyle>();
        assert!(style.is_some());
        assert_eq!(*style.unwrap(), SystemUiOverlayStyle::Dark);

        // Wrong type should return None
        let wrong = layer.downcast_value::<SemanticLabel>();
        assert!(wrong.is_none());
    }

    #[test]
    fn test_annotated_region_with_semantic_label() {
        let label = Arc::new(SemanticLabel::new("Submit Button"));
        let layer = AnnotatedRegionLayer::new(Rect::from_xywh(0.0, 0.0, 100.0, 44.0), label);

        let value = layer.downcast_value::<SemanticLabel>().unwrap();
        assert_eq!(value.text(), "Submit Button");
    }

    #[test]
    fn test_annotated_region_setters() {
        let value = Arc::new(42i32);
        let mut layer = AnnotatedRegionLayer::new(Rect::ZERO, value);

        layer.set_rect(Rect::from_xywh(5.0, 5.0, 50.0, 50.0));
        assert_eq!(layer.rect().left(), 5.0);

        layer.set_value(Arc::new(100i32));
        assert_eq!(*layer.downcast_value::<i32>().unwrap(), 100);
    }

    #[test]
    fn test_annotated_region_clone() {
        let value = Arc::new(SystemUiOverlayStyle::Dark);
        let layer = AnnotatedRegionLayer::new(Rect::from_xywh(10.0, 20.0, 100.0, 50.0), value);

        let cloned = layer.clone();
        assert_eq!(layer.rect(), cloned.rect());
        // Arc is cloned, so values point to same data
        assert!(Arc::ptr_eq(layer.value(), cloned.value()));
    }

    #[test]
    fn test_system_ui_overlay_style() {
        assert_eq!(SystemUiOverlayStyle::default(), SystemUiOverlayStyle::Light);
    }

    #[test]
    fn test_semantic_label() {
        let label = SemanticLabel::new("Test");
        assert_eq!(label.text(), "Test");

        let from_str: SemanticLabel = "From str".into();
        assert_eq!(from_str.text(), "From str");

        let from_string: SemanticLabel = String::from("From String").into();
        assert_eq!(from_string.text(), "From String");
    }

    #[test]
    fn test_annotated_region_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<AnnotatedRegionLayer>();
        assert_sync::<AnnotatedRegionLayer>();
    }
}
