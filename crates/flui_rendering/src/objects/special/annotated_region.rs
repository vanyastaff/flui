//! RenderAnnotatedRegion - Annotates a region for system UI
//!
//! This widget provides metadata about the region it covers that can be read by
//! ancestors or the system (e.g., system UI overlay styling).

use crate::core::{
    RenderBox, Single, {BoxLayoutCtx, BoxPaintCtx},
};
use flui_types::Size;

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
    /// The value to annotate this region with
    pub value: T,
    /// Whether this annotation should apply to the entire region
    pub sized: bool,
}

// ===== Methods =====

impl<T: Clone + Send + Sync + std::fmt::Debug + 'static> RenderAnnotatedRegion<T> {
    /// Create new RenderAnnotatedRegion
    pub fn new(value: T) -> Self {
        Self { value, sized: true }
    }

    /// Create with custom sized flag
    pub fn with_sized(value: T, sized: bool) -> Self {
        Self { value, sized }
    }

    /// Get the annotation value
    pub fn get_value(&self) -> &T {
        &self.value
    }

    /// Set the annotation value
    pub fn set_value(&mut self, value: T) {
        self.value = value;
        // No repaint needed - this is just metadata
    }

    /// Check if the annotation applies to the entire sized region
    pub fn is_sized(&self) -> bool {
        self.sized
    }

    /// Set whether annotation applies to entire region
    pub fn set_sized(&mut self, sized: bool) {
        self.sized = sized;
    }
}

// ===== RenderObject Implementation =====

impl<T: Clone + Send + Sync + std::fmt::Debug + 'static> RenderBox<Single>
    for RenderAnnotatedRegion<T>
{
    fn layout(&mut self, ctx: BoxLayoutCtx<'_, Single>) -> Size {
        let child_id = ctx.children.single();
        // Layout child with same constraints (pass-through)
        ctx.layout_child(child_id, ctx.constraints)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        let child_id = ctx.children.single();
        // This is a pass-through - just paint child
        // The annotation value is used by ancestors, not painted
        ctx.paint_child(child_id, ctx.offset);
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
    fn test_render_annotated_region_new() {
        let region = RenderAnnotatedRegion::new("dark");
        assert_eq!(region.value, "dark");
        assert!(region.sized);
    }

    #[test]
    fn test_render_annotated_region_set_sized() {
        let mut region = RenderAnnotatedRegion::new("light");
        region.set_sized(false);
        assert_eq!(region.value, "light");
        assert!(!region.sized);
    }

    #[test]
    fn test_render_annotated_region_set_value() {
        let mut region = RenderAnnotatedRegion::new("dark");

        region.set_value("light");
        assert_eq!(region.value, "light");
    }

    #[test]
    fn test_render_annotated_region_with_integer() {
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

        assert_eq!(region.value.style, "dark");
        assert_eq!(region.value.priority, 5);

        let new_metadata = ComplexMetadata {
            style: "light".to_string(),
            priority: 10,
        };
        region.set_value(new_metadata.clone());

        assert_eq!(region.value.style, "light");
        assert_eq!(region.value.priority, 10);
    }
}
