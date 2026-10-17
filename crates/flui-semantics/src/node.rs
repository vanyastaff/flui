//! SemanticsNode - Individual node in the semantics tree
//!
//! Each semantics node represents accessible content and corresponds to
//! one or more render objects. Non-boundary render objects merge their
//! semantics into the nearest boundary ancestor.

use flui_foundation::geometry::{Matrix4, Rect};
use flui_foundation::{ElementId, RenderId, SemanticsId};

// Use our optimized types from flui-semantics
use crate::{configuration::SemanticsConfiguration, identity::AccessibilityNodeId};

#[derive(Debug, Clone, Copy)]
enum NodeOrigin {
    Render(RenderId),
    Region(crate::SemanticsRegionId),
}

// ============================================================================
// SEMANTICS NODE
// ============================================================================

/// A node in the semantics tree.
///
/// Each semantics node corresponds to one or more render objects in the
/// render tree. Non-boundary render objects are merged into their parent
/// semantics boundary.
///
/// # Contents
///
/// - Properties for screen readers (label, hint, value)
/// - Supported actions (tap, scroll, increase/decrease)
/// - Geometry for spatial navigation
/// - Tree structure (parent, children)
///
/// # Example
///
/// ```rust
/// use std::sync::Arc;
///
/// use flui_semantics::{SemanticsAction, SemanticsConfiguration, SemanticsNode, SemanticsRole};
///
/// let mut node = SemanticsNode::new();
/// node.config_mut().set_label("Submit");
/// node.config_mut().set_role(SemanticsRole::Button);
/// node.config_mut()
///     .add_action(SemanticsAction::Click, Arc::new(|_| {}));
///
/// assert!(node.has_been_annotated());
/// assert!(node.config().has_action(SemanticsAction::Click));
/// ```
#[derive(Debug, Clone, Default)]
pub struct SemanticsNode {
    // ========== Tree Structure ==========
    /// Parent node ID (None for root).
    parent: Option<SemanticsId>,

    /// Child node IDs.
    children: Vec<SemanticsId>,

    // ========== Cross-tree Reference ==========
    /// The render element that owns this semantics node.
    element_id: Option<ElementId>,

    /// Render boundary or independently allocated authored content. Both
    /// origins survive semantics-arena rebuilds under their identity authority.
    origin: Option<NodeOrigin>,

    // ========== Semantic Configuration ==========
    /// Full semantic configuration (label, flags, actions, etc.).
    config: SemanticsConfiguration,

    // ========== Geometry ==========
    /// Bounding rectangle in global coordinates.
    rect: Rect<f64>,
    /// Unclipped geometry used only by ancestor reveal delivery.
    reveal_rect: Option<Rect<f64>>,

    /// Transform matrix.
    ///
    /// Stored as the workspace-canonical [`Matrix4`] from `flui_foundation::geometry`
    /// — the same representation `to_node_data()` exports. Previously
    /// stored as `Option<[f32; 16]>` and round-tripped through
    /// `Matrix4::from` at export time; unifying the representation
    /// across the framework drops that round-trip.
    transform: Option<Matrix4>,

    // ========== State ==========
    /// Whether this node is marked dirty and needs update.
    dirty: bool,
}

impl SemanticsNode {
    /// Creates a new empty semantics node.
    pub fn new() -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            element_id: None,
            origin: None,
            config: SemanticsConfiguration::new(),
            rect: Rect::ZERO,
            reveal_rect: None,
            transform: None,
            dirty: true,
        }
    }

    /// Creates a node with an associated element ID.
    pub fn with_element_id(mut self, element_id: ElementId) -> Self {
        self.element_id = Some(element_id);
        self
    }

    /// Associates this semantics boundary with its source render object.
    #[must_use]
    pub fn with_source_render_id(mut self, render_id: RenderId) -> Self {
        self.origin = Some(NodeOrigin::Render(render_id));
        self
    }

    /// Associates independently addressable content with an allocated identity.
    /// This replaces any render-boundary identity on the node.
    #[must_use]
    pub fn with_region_id(mut self, id: crate::SemanticsRegionId) -> Self {
        self.origin = Some(NodeOrigin::Region(id));
        self
    }

    /// Creates a node with a configuration.
    pub fn with_config(mut self, config: SemanticsConfiguration) -> Self {
        self.config = config;
        self
    }

    // ========== Tree Structure ==========

    /// Returns the parent node ID.
    #[inline]
    pub fn parent(&self) -> Option<SemanticsId> {
        self.parent
    }

    /// Sets the parent node ID.
    pub fn set_parent(&mut self, parent: Option<SemanticsId>) {
        self.parent = parent;
    }

    /// Returns the child node IDs.
    #[inline]
    pub fn children(&self) -> &[SemanticsId] {
        &self.children
    }

    /// Adds a child node ID.
    pub fn add_child(&mut self, child: SemanticsId) {
        if !self.children.contains(&child) {
            self.children.push(child);
        }
    }

    /// Removes a child node ID.
    pub fn remove_child(&mut self, child: SemanticsId) {
        self.children.retain(|&id| id != child);
    }

    /// Clears all children.
    pub fn clear_children(&mut self) {
        self.children.clear();
    }

    // ========== Cross-tree Reference ==========

    /// Returns the associated element ID.
    #[inline]
    pub fn element_id(&self) -> Option<ElementId> {
        self.element_id
    }

    /// Sets the associated element ID.
    pub fn set_element_id(&mut self, element_id: Option<ElementId>) {
        self.element_id = element_id;
    }

    /// Returns the render object that forms this semantics boundary.
    #[inline]
    pub fn source_render_id(&self) -> Option<RenderId> {
        match self.origin {
            Some(NodeOrigin::Render(id)) => Some(id),
            _ => None,
        }
    }

    /// Returns the stable OS-facing identity of a render boundary or authored
    /// region, or `None` for an unbound manually-created node.
    #[inline]
    pub fn accessibility_id(&self) -> Option<AccessibilityNodeId> {
        self.origin.map(|origin| match origin {
            NodeOrigin::Render(id) => AccessibilityNodeId::from(id),
            NodeOrigin::Region(id) => id.accessibility_id(),
        })
    }

    // ========== Semantic Configuration ==========

    /// Returns the semantic configuration.
    #[inline]
    pub fn config(&self) -> &SemanticsConfiguration {
        &self.config
    }

    /// Returns mutable reference to semantic configuration.
    #[inline]
    pub fn config_mut(&mut self) -> &mut SemanticsConfiguration {
        self.dirty = true;
        &mut self.config
    }

    /// Sets the semantic configuration.
    pub fn set_config(&mut self, config: SemanticsConfiguration) {
        self.config = config;
        self.dirty = true;
    }

    /// Returns the label text.
    #[inline]
    pub fn label(&self) -> Option<&str> {
        self.config.label().map(|l| l.string.as_str())
    }

    /// Returns the value text.
    #[inline]
    pub fn value(&self) -> Option<&str> {
        self.config.value().map(|v| v.string.as_str())
    }

    /// Returns the hint text.
    #[inline]
    pub fn hint(&self) -> Option<&str> {
        self.config.hint().map(|h| h.string.as_str())
    }

    // ========== Geometry ==========

    /// Returns the bounding rectangle.
    #[inline]
    pub fn rect(&self) -> Rect<f64> {
        self.rect
    }

    /// Sets the bounding rectangle.
    pub fn set_rect(&mut self, rect: Rect<f64>) {
        self.rect = rect;
        self.dirty = true;
    }

    /// Retain the render source's unclipped root-space logical geometry.
    pub fn set_reveal_rect(&mut self, rect: Rect<f64>) {
        self.reveal_rect = Some(rect);
    }

    pub(crate) fn reveal_rect(&self) -> Rect<f64> {
        self.reveal_rect.unwrap_or(self.rect)
    }

    /// Returns the transform matrix, if any.
    #[inline]
    pub fn transform(&self) -> Option<&Matrix4> {
        self.transform.as_ref()
    }

    /// Sets the transform matrix.
    pub fn set_transform(&mut self, transform: Option<Matrix4>) {
        self.transform = transform;
        self.dirty = true;
    }

    // ========== State ==========

    /// Returns true if this node is dirty.
    #[inline]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Marks this node as clean.
    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    /// Marks this node as dirty.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    // ========== Annotation Checking ==========

    /// Returns whether semantic payload was assigned to this node.
    pub fn has_been_annotated(&self) -> bool {
        self.config.has_been_annotated()
    }

    // ========== Merging ==========

    /// Absorbs another node's configuration into this one.
    ///
    /// Used when a non-boundary render object's semantics should be merged
    /// into its parent boundary. Delegates to
    /// [`SemanticsConfiguration::absorb`] while preserving this node's source
    /// render-object geometry. A merged descendant contributes payload, not a
    /// larger accessibility hit region.
    ///
    /// **Naming**: named `absorb` for consistency with
    /// [`SemanticsConfiguration::absorb`], which does the actual merging;
    /// this is a convenience that keeps the node's own geometry.
    pub fn absorb(&mut self, other: &SemanticsNode) {
        self.config.absorb(&other.config);
        self.dirty = true;
    }
}
