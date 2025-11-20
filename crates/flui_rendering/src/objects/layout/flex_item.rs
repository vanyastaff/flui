//! RenderFlexItem - wrapper for flexible children in Flex layout
//!
//! This RenderObject wraps a child and provides FlexItemMetadata
//! that the parent RenderFlex uses to determine layout behavior.
//!
//! # Architecture
//!
//! Following the GAT Metadata pattern from FINAL_ARCHITECTURE_V2.md:
//! - FlexItemMetadata is stored inline (not in separate ParentData)
//! - Parent (RenderFlex) accesses metadata via GAT-based downcast
//! - Zero-cost when not using flexible children

// TODO: Migrate to Render<A>
// use flui_core::render::{RuntimeArity, LayoutContext, PaintContext, LegacyRender};

use flui_types::{layout::FlexFit, Size};

/// Metadata for flexible children in Flex layout
///
/// This metadata is read by the parent RenderFlex during layout
/// to determine how much space to allocate to this child.
///
/// # Example
///
/// ```rust,ignore
/// // Expanded widget (flex=1, fit=Tight)
/// let expanded = FlexItemMetadata {
///     flex: 1,
///     fit: FlexFit::Tight,
/// };
///
/// // Flexible widget (flex=1, fit=Loose)
/// let flexible = FlexItemMetadata {
///     flex: 1,
///     fit: FlexFit::Loose,
/// };
///
/// // Custom flex factor
/// let custom = FlexItemMetadata {
///     flex: 2,
///     fit: FlexFit::Tight,
/// };
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlexItemMetadata {
    /// The flex factor to use for this child.
    ///
    /// If non-zero, the child is flexible and will receive space according to its flex factor.
    /// Higher flex factors get proportionally more space.
    ///
    /// - `0`: Child is not flexible, uses intrinsic size
    /// - `1`: Child gets equal share of remaining space
    /// - `2+`: Child gets proportionally more space (2x gets twice as much as 1x)
    pub flex: i32,

    /// How the child should fit into the available space.
    ///
    /// - `FlexFit::Tight`: Child must fill allocated space (Expanded behavior)
    /// - `FlexFit::Loose`: Child can be smaller than allocated space (Flexible behavior)
    pub fit: FlexFit,
}

impl FlexItemMetadata {
    /// Create new flex item metadata
    pub fn new(flex: i32, fit: FlexFit) -> Self {
        Self { flex, fit }
    }

    /// Create metadata for Expanded widget (tight fit, flex=1)
    pub fn expanded() -> Self {
        Self {
            flex: 1,
            fit: FlexFit::Tight,
        }
    }

    /// Create metadata for Expanded widget with custom flex factor
    pub fn expanded_with_flex(flex: i32) -> Self {
        Self {
            flex,
            fit: FlexFit::Tight,
        }
    }

    /// Create metadata for Flexible widget (loose fit, flex=1)
    pub fn flexible() -> Self {
        Self {
            flex: 1,
            fit: FlexFit::Loose,
        }
    }

    /// Create metadata for Flexible widget with custom flex factor
    pub fn flexible_with_flex(flex: i32) -> Self {
        Self {
            flex,
            fit: FlexFit::Loose,
        }
    }

    /// Check if this child is flexible (flex > 0)
    pub fn is_flexible(&self) -> bool {
        self.flex > 0
    }

    /// Check if this child uses tight fit (must fill allocated space)
    pub fn is_tight(&self) -> bool {
        self.fit == FlexFit::Tight
    }
}

impl Default for FlexItemMetadata {
    fn default() -> Self {
        Self {
            flex: 0,
            fit: FlexFit::Tight,
        }
    }
}

/// RenderObject that wraps a child and provides flex metadata
///
/// This is a pass-through render object that simply delegates layout
/// and paint to its child, but provides FlexItemMetadata that the parent
/// RenderFlex can query.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderFlexItem, FlexItemMetadata};
/// use flui_types::layout::FlexFit;
///
/// // Create a flex item for Expanded widget
/// let flex_item = RenderFlexItem::new(FlexItemMetadata::expanded());
///
/// // Create a flex item for Flexible widget with custom flex
/// let flexible = RenderFlexItem::new(FlexItemMetadata::flexible_with_flex(2));
/// ```
#[derive(Debug)]
pub struct RenderFlexItem {
    /// The flex metadata for this child
    pub metadata: FlexItemMetadata,
}

impl RenderFlexItem {
    /// Create new RenderFlexItem with specified metadata
    pub fn new(metadata: FlexItemMetadata) -> Self {
        Self { metadata }
    }

    /// Create RenderFlexItem for Expanded widget
    pub fn expanded() -> Self {
        Self {
            metadata: FlexItemMetadata::expanded(),
        }
    }

    /// Create RenderFlexItem for Expanded widget with custom flex factor
    pub fn expanded_with_flex(flex: i32) -> Self {
        Self {
            metadata: FlexItemMetadata::expanded_with_flex(flex),
        }
    }

    /// Create RenderFlexItem for Flexible widget
    pub fn flexible() -> Self {
        Self {
            metadata: FlexItemMetadata::flexible(),
        }
    }

    /// Create RenderFlexItem for Flexible widget with custom flex factor
    pub fn flexible_with_flex(flex: i32) -> Self {
        Self {
            metadata: FlexItemMetadata::flexible_with_flex(flex),
        }
    }

    /// Get the flex metadata
    pub fn flex_metadata(&self) -> &FlexItemMetadata {
        &self.metadata
    }

    /// Get mutable flex metadata
    pub fn flex_metadata_mut(&mut self) -> &mut FlexItemMetadata {
        &mut self.metadata
    }
}

impl LegacyRender for RenderFlexItem {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;
        // Pass-through: just layout child with same constraints
        tree.layout_child(child_id, constraints)
    }

    fn paint(&self, ctx: &PaintContext) -> flui_painting::Canvas {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let offset = ctx.offset;
        // Pass-through: just paint child at same offset
        tree.paint_child(child_id, offset)
    }

    // Note: metadata() method removed - not part of unified Render trait
    // Parent data should be queried via RenderElement.parent_data() instead
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> RuntimeArity {
        RuntimeArity::Exact(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flex_item_metadata_default() {
        let meta = FlexItemMetadata::default();
        assert_eq!(meta.flex, 0);
        assert_eq!(meta.fit, FlexFit::Tight);
        assert!(!meta.is_flexible());
    }

    #[test]
    fn test_flex_item_metadata_expanded() {
        let meta = FlexItemMetadata::expanded();
        assert_eq!(meta.flex, 1);
        assert_eq!(meta.fit, FlexFit::Tight);
        assert!(meta.is_flexible());
        assert!(meta.is_tight());
    }

    #[test]
    fn test_flex_item_metadata_flexible() {
        let meta = FlexItemMetadata::flexible();
        assert_eq!(meta.flex, 1);
        assert_eq!(meta.fit, FlexFit::Loose);
        assert!(meta.is_flexible());
        assert!(!meta.is_tight());
    }

    #[test]
    fn test_flex_item_metadata_custom() {
        let meta = FlexItemMetadata::new(3, FlexFit::Loose);
        assert_eq!(meta.flex, 3);
        assert_eq!(meta.fit, FlexFit::Loose);
        assert!(meta.is_flexible());
    }

    #[test]
    fn test_render_flex_item_new() {
        let item = RenderFlexItem::new(FlexItemMetadata::expanded());
        assert_eq!(item.metadata.flex, 1);
        assert_eq!(item.metadata.fit, FlexFit::Tight);
    }

    #[test]
    fn test_render_flex_item_expanded() {
        let item = RenderFlexItem::expanded();
        assert_eq!(item.metadata.flex, 1);
        assert_eq!(item.metadata.fit, FlexFit::Tight);
    }

    #[test]
    fn test_render_flex_item_flexible() {
        let item = RenderFlexItem::flexible();
        assert_eq!(item.metadata.flex, 1);
        assert_eq!(item.metadata.fit, FlexFit::Loose);
    }

    #[test]
    fn test_render_flex_item_metadata_access() {
        let mut item = RenderFlexItem::expanded_with_flex(2);
        assert_eq!(item.flex_metadata().flex, 2);

        item.flex_metadata_mut().flex = 3;
        assert_eq!(item.flex_metadata().flex, 3);
    }
}
