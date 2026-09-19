//! `AnnotatedRegionLayer` — attaches a type-erased value to a region for a reader above
//! the tree; the reader does not exist yet.

use std::{any::Any, fmt, sync::Arc};

use flui_types::geometry::{Pixels, Rect};

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
/// use std::sync::Arc;
///
/// use flui_layer::{AnnotatedRegionLayer, SystemUiOverlayStyle};
/// use flui_types::geometry::{Rect, px};
///
/// let style = Arc::new(SystemUiOverlayStyle::Dark);
/// let layer = AnnotatedRegionLayer::new(Rect::from_xywh(px(0.0), px(0.0), px(400.0), px(24.0)), style);
/// ```
#[derive(Clone)]
pub struct AnnotatedRegionLayer {
    /// The annotated region bounds
    rect: Rect<Pixels>,

    /// The annotation value (type-erased)
    value: AnnotationValue,

    /// Whether the region is sized to its children
    sized_by_parent: bool,
}

impl AnnotatedRegionLayer {
    /// Attaches `value` to `rect` for a reader above the tree to find.
    #[inline]
    pub fn new<T: Any + Send + Sync>(rect: Rect<Pixels>, value: Arc<T>) -> Self {
        Self {
            rect,
            value,
            sized_by_parent: false,
        }
    }

    /// An annotation whose region is its parent's bounds.
    #[inline]
    pub fn sized_by_parent<T: Any + Send + Sync>(value: Arc<T>) -> Self {
        Self {
            rect: Rect::ZERO,
            value,
            sized_by_parent: true,
        }
    }

    /// The region bounds (`Rect::ZERO` when sized by the parent).
    #[inline]
    pub fn bounds(&self) -> Rect<Pixels> {
        self.rect
    }

    /// The annotation value, type-erased; a reader downcasts it.
    #[inline]
    pub fn value(&self) -> &AnnotationValue {
        &self.value
    }

    /// Whether the region takes its parent's bounds instead of `bounds()`.
    #[inline]
    pub fn is_sized_by_parent(&self) -> bool {
        self.sized_by_parent
    }
}

impl fmt::Debug for AnnotatedRegionLayer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnnotatedRegionLayer")
            .field("rect", &self.rect)
            .field("sized_by_parent", &self.sized_by_parent)
            .field("value_type", &self.value.as_ref().type_id())
            .finish()
    }
}

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
pub struct SemanticLabel(String);

impl SemanticLabel {
    /// A label from any string-like value.
    #[inline]
    pub fn new(label: impl Into<String>) -> Self {
        Self(label.into())
    }

    /// The label text.
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
    use flui_types::geometry::px;

    use super::*;

    #[test]
    fn debug_reports_the_annotation_payload_type() {
        let layer = AnnotatedRegionLayer::sized_by_parent(Arc::new(SystemUiOverlayStyle::Dark));
        let debug = format!("{layer:?}");
        let payload_type = std::any::TypeId::of::<SystemUiOverlayStyle>();
        assert!(
            debug.contains(&format!("value_type: {payload_type:?}")),
            "{debug}"
        );
    }

    #[test]
    fn test_annotated_region_new() {
        let rect = Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0));
        let value = Arc::new(SystemUiOverlayStyle::Dark);
        let layer = AnnotatedRegionLayer::new(rect, value);

        assert_eq!(layer.bounds(), rect);
        assert!(!layer.is_sized_by_parent());
    }

    #[test]
    fn test_annotated_region_sized_by_parent() {
        let value = Arc::new(SystemUiOverlayStyle::Light);
        let layer = AnnotatedRegionLayer::sized_by_parent(value);

        assert!(layer.is_sized_by_parent());
        assert_eq!(layer.bounds(), Rect::ZERO);
    }

    #[test]
    fn test_annotated_region_with_semantic_label() {
        let label = Arc::new(SemanticLabel::new("Submit Button"));
        let layer = AnnotatedRegionLayer::new(
            Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(44.0)),
            label,
        );

        let value = layer.value().downcast_ref::<SemanticLabel>().unwrap();
        assert_eq!(value.text(), "Submit Button");
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
}
