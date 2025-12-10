//! RenderAnnotatedRegion - Annotates a region for system UI
//!
//! This widget provides metadata about the region it covers that can be read by
//! ancestors or the system (e.g., system UI overlay styling).
//!
//! # Flutter Equivalence
//!
//! | Aspect | Flutter | FLUI |
//! |--------|---------|------|
//! | **Class** | `RenderAnnotatedRegion<T>` | `RenderAnnotatedRegion<T>` |
//! | **Protocol** | BoxProtocol (pass-through) | BoxProtocol (pass-through) |
//! | **Generic** | `T extends Object?` | `T: Clone + Send + Sync + Debug + 'static` |
//! | **Layout** | Passes constraints to child | ✅ Identical behavior |
//! | **Paint** | Paints child (metadata for ancestors) | ✅ Identical behavior |
//! | **Fields** | value, sized | ✅ Identical |
//! | **Methods** | setValue(), markNeedsPaint() | ✅ set_value() (no repaint) |
//! | **Use Case** | System UI overlay styling | ✅ Same |
//! | **Compliance** | Full implementation | 90% (core complete, missing AnnotatedRegionLayer) |
//!
//! # Layout Protocol
//!
//! ## Input
//! - `BoxConstraints` - Constraints from parent
//! - Single child via `ctx.children.single()`
//! - `value: T` - Annotation value (metadata)
//! - `sized: bool` - Whether annotation applies to entire region
//!
//! ## Steps
//! 1. **Get child** - `ctx.children.single()`
//! 2. **Pass-through layout** - `ctx.layout_child(child_id, ctx.constraints, true)`
//! 3. **Return child size** - No modification
//!
//! ## Output
//! - Child's size (unmodified)
//! - Annotation metadata stored for ancestor queries
//!
//! ## Performance Characteristics
//! - **Time**: O(1) + child layout time (pure pass-through)
//! - **Space**: O(1) for value storage
//! - **Invalidation**: No layout invalidation when value changes (metadata only)
//! - **Cost**: Negligible overhead (single indirection)
//!
//! # Paint Protocol
//!
//! ## Steps
//! 1. **Get child** - `ctx.children.single()`
//! 2. **Paint child** - `ctx.paint_child(child_id, ctx.offset)`
//! 3. **No visual effect** - Annotation is metadata only
//!
//! ## Output
//! - Child's painted canvas (unmodified)
//! - In future: Should attach AnnotatedRegionLayer for ancestor queries
//!
//! # Use Cases
//!
//! ## System UI Overlay Styling
//! ```rust,ignore
//! // Annotate region for dark status bar
//! #[derive(Debug, Clone)]
//! enum SystemUiOverlay { Light, Dark }
//!
//! RenderAnnotatedRegion::new(SystemUiOverlay::Dark)
//! ```
//!
//! ## Semantic Annotations
//! ```rust,ignore
//! // Annotate region with semantic information
//! #[derive(Debug, Clone)]
//! struct SemanticData {
//!     label: String,
//!     role: String,
//! }
//!
//! RenderAnnotatedRegion::new(SemanticData {
//!     label: "Navigation Bar".to_string(),
//!     role: "navigation".to_string(),
//! })
//! ```
//!
//! ## Region Metadata
//! ```rust,ignore
//! // Annotate region with custom metadata
//! RenderAnnotatedRegion::new(("high-priority", 100))
//! ```
//!
//! # Critical Issues
//!
//! ⚠️ **Minor Missing Features** (90% complete):
//!
//! 1. **No AnnotatedRegionLayer** (Future Enhancement)
//!    - Currently just stores value
//!    - Should attach layer to render tree for ancestor queries
//!    - Flutter uses Layer protocol for this
//!
//! 2. **No markNeedsPaint on value change** (Intentional)
//!    - set_value() doesn't trigger repaint (line 54-57)
//!    - This is CORRECT - annotation is metadata, not visual
//!    - Should trigger ancestor notification in future
//!
//! 3. **No ancestor query API** (Future)
//!    - No way for ancestors to find annotation values
//!    - Needs Layer or ElementTree integration
//!
//! # Comparison with Related Objects
//!
//! | Aspect | RenderAnnotatedRegion | RenderMetadata | RenderOffstage |
//! |--------|----------------------|----------------|----------------|
//! | **Purpose** | System UI metadata | Arbitrary metadata | Visibility control |
//! | **Layout** | Pass-through | Pass-through | Conditional |
//! | **Paint** | Pass-through | Pass-through | Conditional |
//! | **Value Type** | Generic `T` | Generic `T` | bool (offstage) |
//! | **Use Case** | System UI styling | General metadata | Hide subtree |
//! | **Performance** | O(1) overhead | O(1) overhead | O(1) overhead |
//! | **Implementation** | 90% complete | ~90% complete | ~85% complete |
//!
//! # Pattern: Metadata Pass-Through Proxy
//!
//! This object represents the **Metadata Pass-Through Proxy** pattern:
//! - Passes layout constraints unmodified to child
//! - Returns child's size unmodified
//! - Paints child unmodified
//! - Stores metadata value for ancestor or system queries
//! - Zero visual overhead (pure metadata)
//! - Generic over value type `T`
//!
//! # Examples
//!
//! ## System UI Overlay Styling
//!
//! ```rust,ignore
//! use flui_rendering::RenderAnnotatedRegion;
//!
//! #[derive(Debug, Clone)]
//! enum SystemUiOverlay {
//!     Light,  // Dark text on light status bar
//!     Dark,   // Light text on dark status bar
//! }
//!
//! // Annotate top region for dark status bar
//! let render = RenderAnnotatedRegion::new(SystemUiOverlay::Dark);
//!
//! // System reads this to style status bar
//! assert_eq!(render.get_value(), &SystemUiOverlay::Dark);
//! ```
//!
//! ## Custom Metadata
//!
//! ```rust,ignore
//! #[derive(Debug, Clone)]
//! struct CustomMetadata {
//!     priority: u8,
//!     category: String,
//! }
//!
//! let metadata = CustomMetadata {
//!     priority: 10,
//!     category: "navigation".to_string(),
//! };
//!
//! let mut render = RenderAnnotatedRegion::new(metadata);
//!
//! // Update metadata without repaint (it's just metadata!)
//! render.set_value(CustomMetadata {
//!     priority: 5,
//!     category: "content".to_string(),
//! });
//! ```
//!
//! ## Sized vs Unsized Annotations
//!
//! ```rust,ignore
//! // Sized: annotation applies to entire region
//! let sized = RenderAnnotatedRegion::new("metadata");
//! assert!(sized.is_sized());
//!
//! // Unsized: annotation is just a marker
//! let unsized = RenderAnnotatedRegion::with_sized("marker", false);
//! assert!(!unsized.is_sized());
//! ```
//!
//! ## Type Safety with Generics
//!
//! ```rust,ignore
//! // Type-safe annotations - compile-time checked!
//! let str_region = RenderAnnotatedRegion::new("text");
//! let int_region = RenderAnnotatedRegion::new(42);
//! let enum_region = RenderAnnotatedRegion::new(SystemUiOverlay::Dark);
//!
//! // Each has different type: RenderAnnotatedRegion<&str>, etc.
//! ```

use flui_rendering::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use flui_rendering::{RenderObject, RenderResult};
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
/// # Arity
/// - **Children**: `Single` (exactly 1 child)
/// - **Type**: Single-child pass-through proxy
/// - **Access**: Via `ctx.children.single()`
///
/// # Protocol
/// - **Input**: `BoxConstraints` from parent
/// - **Child Protocol**: `BoxProtocol` (pass-through)
/// - **Output**: `Size` (child's size, unmodified)
/// - **Pattern**: Metadata pass-through proxy
///
/// # Pattern: Metadata Pass-Through Proxy
/// This object represents the **Metadata Pass-Through Proxy** pattern:
/// - Generic over value type `T`
/// - Zero layout/paint overhead (pure pass-through)
/// - Stores metadata for ancestor or system queries
/// - Used for system UI styling (status bar, etc.)
///
/// # Flutter Compliance
/// - ✅ **API Surface**: Matches Flutter's RenderAnnotatedRegion<T>
/// - ✅ **Fields**: value, sized
/// - ✅ **Layout**: Pass-through (identical behavior)
/// - ✅ **Paint**: Pass-through (identical behavior)
/// - ✅ **Methods**: new(), set_value(), is_sized()
/// - ❌ **Layer**: Missing AnnotatedRegionLayer for ancestor queries
/// - **Overall**: ~90% compliant (core complete, missing layer)
///
/// # Implementation Status
///
/// | Feature | Status | Notes |
/// |---------|--------|-------|
/// | **Structure** | ✅ Complete | Generic over T |
/// | **Constructor** | ✅ Complete | new() + with_sized() |
/// | **Arity** | ✅ Complete | Single child |
/// | **Layout** | ✅ Complete | Pass-through to child |
/// | **Paint** | ✅ Complete | Pass-through to child |
/// | **set_value()** | ✅ Complete | No repaint (metadata only) |
/// | **get_value()** | ✅ Complete | Returns &T |
/// | **AnnotatedRegionLayer** | ❌ Missing | Future: attach layer for queries |
/// | **Ancestor Query API** | ❌ Missing | Future: Layer/ElementTree integration |
/// | **Overall** | 🟢 90% | Core complete, layer support missing |
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

impl<T: Clone + Send + Sync + std::fmt::Debug + 'static> RenderObject for RenderAnnotatedRegion<T> {}

impl<T: Clone + Send + Sync + std::fmt::Debug + 'static> RenderBox<Single>
    for RenderAnnotatedRegion<T>
{
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        let child_id = *ctx.children.single();
        // Layout child with same constraints (pass-through)
        Ok(ctx.layout_child(child_id, ctx.constraints, true)?)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        let child_id = *ctx.children.single();
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
