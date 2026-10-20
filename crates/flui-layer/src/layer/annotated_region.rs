//! `AnnotatedRegionLayer` — attaches a type-erased value to a region for a reader of the
//! composited scene (`LayerTree::find_annotations`).

use std::{any::Any, fmt, sync::Arc};

use flui_foundation::geometry::Rect;

/// A type-erased annotation value that still prints and names its type.
///
/// Clones share one value; [`Self::ptr_eq`] tells whether two handles do.
#[derive(Clone)]
pub struct AnnotationValue {
    value: Arc<dyn Any + Send + Sync>,
    type_name: &'static str,
    debug: fn(&(dyn Any + Send + Sync), &mut fmt::Formatter<'_>) -> fmt::Result,
}

impl AnnotationValue {
    /// Erases `value`, keeping its type name and `Debug` form.
    pub fn new<T: Any + Send + Sync + fmt::Debug>(value: Arc<T>) -> Self {
        Self {
            value,
            type_name: std::any::type_name::<T>(),
            debug: |value, f| match value.downcast_ref::<T>() {
                Some(value) => fmt::Debug::fmt(value, f),
                None => {
                    unreachable!("BUG: an annotation value prints as the type it was made from")
                }
            },
        }
    }

    /// The value as `T`, or `None` when it is another type.
    #[must_use]
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.value.downcast_ref()
    }

    /// A handle to the value as `T`, or `None` when it is another type.
    #[must_use]
    pub fn downcast<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        Arc::clone(&self.value).downcast().ok()
    }

    /// Whether both handles share one value.
    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.value, &other.value)
    }

    /// The full name of the value's type.
    #[must_use]
    pub const fn type_name(&self) -> &'static str {
        self.type_name
    }

    pub(crate) fn erased(&self) -> &Arc<dyn Any + Send + Sync> {
        &self.value
    }
}

impl<T: Any + Send + Sync + fmt::Debug> From<Arc<T>> for AnnotationValue {
    fn from(value: Arc<T>) -> Self {
        Self::new(value)
    }
}

/// The value's own `Debug` form.
impl fmt::Debug for AnnotationValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (self.debug)(self.value.as_ref(), f)
    }
}

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
/// use flui_layer::AnnotatedRegionLayer;
/// use flui_foundation::geometry::Rect;
///
/// #[derive(Debug, PartialEq)]
/// struct Toolbar;
///
/// let layer = AnnotatedRegionLayer::new(Rect::from_xywh(0.0, 0.0, 400.0, 24.0), Arc::new(Toolbar));
/// ```
#[derive(Clone)]
pub struct AnnotatedRegionLayer {
    /// The annotated region bounds
    rect: Rect<f64>,

    /// The annotation value (type-erased)
    value: AnnotationValue,

    /// Whether the region is sized to its children
    sized_by_parent: bool,
}

impl AnnotatedRegionLayer {
    /// Attaches `value` to `rect` for a reader above the tree to find.
    #[inline]
    pub fn new<T: Any + Send + Sync + fmt::Debug>(rect: Rect<f64>, value: Arc<T>) -> Self {
        Self {
            rect,
            value: AnnotationValue::new(value),
            sized_by_parent: false,
        }
    }

    /// Attaches an already type-erased `value` to `rect`.
    #[inline]
    pub fn from_value(rect: Rect<f64>, value: AnnotationValue) -> Self {
        Self {
            rect,
            value,
            sized_by_parent: false,
        }
    }

    /// An annotation whose region is its parent's bounds.
    #[inline]
    pub fn sized_by_parent<T: Any + Send + Sync + fmt::Debug>(value: Arc<T>) -> Self {
        Self {
            rect: Rect::ZERO,
            value: AnnotationValue::new(value),
            sized_by_parent: true,
        }
    }

    /// The region bounds (`Rect::ZERO` when sized by the parent).
    #[inline]
    pub fn bounds(&self) -> Rect<f64> {
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
            .field("value", &self.value)
            .finish()
    }
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
