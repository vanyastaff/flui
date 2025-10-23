//! RenderAnnotatedRegion - Annotates a region for system UI
//!
//! This widget provides metadata about the region it covers that can be read by
//! ancestors or the system (e.g., system UI overlay styling).

use flui_core::DynRenderObject;
use flui_types::{Offset, Size, constraints::BoxConstraints};
use std::any::Any;

use crate::core::{RenderBoxMixin, SingleRenderBox};
use crate::delegate_to_mixin;

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
        Self {
            value,
            sized: true,
        }
    }

    /// Create with custom sized flag
    pub fn with_sized(value: T, sized: bool) -> Self {
        Self { value, sized }
    }
}

// ===== Type Alias =====

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
/// let render = RenderAnnotatedRegion::new(AnnotatedRegionData::new("dark"));
/// ```
pub type RenderAnnotatedRegion<T> = SingleRenderBox<AnnotatedRegionData<T>>;

// ===== Methods =====

impl<T: Clone + Send + Sync + std::fmt::Debug + 'static> RenderAnnotatedRegion<T> {
    /// Get the annotation value
    pub fn value(&self) -> &T {
        &self.data().value
    }

    /// Set the annotation value
    pub fn set_value(&mut self, value: T) {
        self.data_mut().value = value;
        // No repaint needed - this is just metadata
    }

    /// Check if the annotation applies to the entire sized region
    pub fn is_sized(&self) -> bool {
        self.data().sized
    }

    /// Set whether annotation applies to entire region
    pub fn set_sized(&mut self, sized: bool) {
        self.data_mut().sized = sized;
    }
}

// ===== DynRenderObject Implementation =====

impl<T: Clone + Send + Sync + std::fmt::Debug + 'static> DynRenderObject for RenderAnnotatedRegion<T> {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        *state.constraints.lock() = Some(constraints);

        let children_ids = ctx.children();
        let size =
        if let Some(&child_id) = children_ids.first() {
            // Layout child with same constraints
            ctx.layout_child_cached(child_id, constraints, None)
        } else {
            // No child - use smallest size
            constraints.smallest()
        };

        *state.size.lock() = Some(size);
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);
        size
    }

    fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        // This is a pass-through - just paint child
        // The annotation value is used by ancestors, not painted
        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        if let Some(&child_id) = children_ids.first() {
            ctx.paint_child(child_id, painter, offset);
        }
    }

    // Delegate all other methods to the mixin
    delegate_to_mixin!();
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
        let data = AnnotatedRegionData::new("dark");
        let region = SingleRenderBox::new(data);
        assert_eq!(region.value(), &"dark");
        assert!(region.is_sized());
    }

    #[test]
    fn test_render_annotated_region_set_value() {
        let data = AnnotatedRegionData::new("dark");
        let mut region = SingleRenderBox::new(data);

        region.set_value("light");
        assert_eq!(region.value(), &"light");
    }

    #[test]
    fn test_render_annotated_region_set_sized() {
        let data = AnnotatedRegionData::new(42);
        let mut region = SingleRenderBox::new(data);

        assert!(region.is_sized());
        region.set_sized(false);
        assert!(!region.is_sized());
    }

    #[test]
    fn test_render_annotated_region_layout() {
        use flui_core::testing::mock_render_context;

        let data = AnnotatedRegionData::new("test");
        let region = SingleRenderBox::new(data);

        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let (_tree, ctx) = mock_render_context();
        let size = region.layout(constraints, &ctx);

        // Without child, should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
        assert_eq!(region.size(), Size::new(0.0, 0.0));
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

        let data = AnnotatedRegionData::new(metadata.clone());
        let mut region = SingleRenderBox::new(data);

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
