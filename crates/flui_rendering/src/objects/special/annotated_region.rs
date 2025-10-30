//! RenderAnnotatedRegion - Annotates a region for system UI
//!
//! This widget provides metadata about the region it covers that can be read by
//! ancestors or the system (e.g., system UI overlay styling).

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::BoxedLayer;
use flui_types::{Offset, Size, constraints::BoxConstraints};

// ===== Data Structure =====

/// Data for RenderAnnotatedRegion
///
/// Stores a value that can be retrieved by ancestors to determine how to style
/// system UI elements (e.g., status bar, navigation bar).
#[derive(Debug, Clone)]
pub struct AnnotatedRegionData<T: Clone + Send + Sync + std::fmt::Debug + 'static> {
    /// The value to annotate this region with
    pub value: T,
    /// Whether this annotation should apply to the entire region
    pub sized: bool,
}

impl<T: Clone + Send + Sync + std::fmt::Debug + 'static> AnnotatedRegionData<T> {
    /// Create new annotated region data
    pub fn new(value: T) -> Self {
        Self { value, sized: true }
    }

    /// Create with custom sized flag
    pub fn with_sized(value: T, sized: bool) -> Self {
        Self { value, sized }
    }
}

// ===== RenderObject =====

/// RenderAnnotatedRegion - Annotates a region with a value
///
/// This is a pass-through render object that provides metadata about its region.
/// The value can be retrieved by ancestors (especially important for system UI styling).
///
/// # Type Parameter
///
/// - `T`: The type of value to annotate the region with (must be Clone + Send + Sync + 'static)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderAnnotatedRegion;
///
/// // Annotate region for dark status bar
/// let render = RenderAnnotatedRegion::new("dark");
/// ```
#[derive(Debug)]
pub struct RenderAnnotatedRegion<T: Clone + Send + Sync + std::fmt::Debug + 'static> {
    /// The annotation data
    pub data: AnnotatedRegionData<T>,
}

// ===== Methods =====

impl<T: Clone + Send + Sync + std::fmt::Debug + 'static> RenderAnnotatedRegion<T> {
    /// Create new RenderAnnotatedRegion
    pub fn new(value: T) -> Self {
        Self {
            data: AnnotatedRegionData::new(value),
        }
    }

    /// Create with custom sized flag
    pub fn with_sized(value: T, sized: bool) -> Self {
        Self {
            data: AnnotatedRegionData::with_sized(value, sized),
        }
    }

    /// Get the annotation value
    pub fn value(&self) -> &T {
        &self.data.value
    }

    /// Set the annotation value
    pub fn set_value(&mut self, value: T) {
        self.data.value = value;
        // No repaint needed - this is just metadata
    }

    /// Check if the annotation applies to the entire sized region
    pub fn is_sized(&self) -> bool {
        self.data.sized
    }

    /// Set whether annotation applies to entire region
    pub fn set_sized(&mut self, sized: bool) {
        self.data.sized = sized;
    }
}

// ===== RenderObject Implementation =====

impl<T: Clone + Send + Sync + std::fmt::Debug + 'static> SingleRender for RenderAnnotatedRegion<T> {
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
        // This is a pass-through - just paint child_id
        // The annotation value is used by ancestors, not painted
                tree.paint_child(child_id, offset)
    }
}

// ===== Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    enum SystemUiStyle {
        Light,
        Dark,
    }

    #[test]
    fn test_annotated_region_data_new() {
        let data = AnnotatedRegionData::new(SystemUiStyle::Dark);
        assert_eq!(data.value, SystemUiStyle::Dark);
        assert!(data.sized);
    }

    #[test]
    fn test_annotated_region_data_with_sized() {
        let data = AnnotatedRegionData::with_sized(SystemUiStyle::Light, false);
        assert_eq!(data.value, SystemUiStyle::Light);
        assert!(!data.sized);
    }

    #[test]
    fn test_render_annotated_region_new() {
        let region = RenderAnnotatedRegion::new("dark");
        assert_eq!(region.value(), &"dark");
        assert!(region.is_sized());
    }

    #[test]
    fn test_render_annotated_region_set_value() {
        let mut region = RenderAnnotatedRegion::new("dark");

        region.set_value("light");
        assert_eq!(region.value(), &"light");
    }

    #[test]
    fn test_render_annotated_region_set_sized() {
        let mut region = RenderAnnotatedRegion::new(42);

        assert!(region.is_sized());
        region.set_sized(false);
        assert!(!region.is_sized());
    }

    #[test]
    fn test_render_annotated_region_complex_type() {
        #[derive(Debug, Clone, PartialEq)]
        struct ComplexMetadata {
            style: String,
            priority: i32,
        }

        let metadata = ComplexMetadata {
            style: "dark".to_string(),
            priority: 5,
        };

        let mut region = RenderAnnotatedRegion::new(metadata.clone());

        assert_eq!(region.value().style, "dark");
        assert_eq!(region.value().priority, 5);

        let new_metadata = ComplexMetadata {
            style: "light".to_string(),
            priority: 10,
        };
        region.set_value(new_metadata.clone());

        assert_eq!(region.value().style, "light");
        assert_eq!(region.value().priority, 10);
    }
}
