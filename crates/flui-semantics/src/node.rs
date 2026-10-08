//! SemanticsNode - Individual node in the semantics tree
//!
//! Each semantics node represents accessible content and corresponds to
//! one or more render objects. Non-boundary render objects merge their
//! semantics into the nearest boundary ancestor.

use flui_foundation::geometry::{Matrix4, Rect};
use flui_foundation::{ElementId, RenderId, SemanticsId};

// Use our optimized types from flui-semantics
use crate::{
    configuration::SemanticsConfiguration, identity::AccessibilityNodeId, update::SemanticsNodeData,
};

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
/// use flui_semantics::{SemanticsAction, SemanticsConfiguration, SemanticsNode};
///
/// let mut node = SemanticsNode::new();
/// node.config_mut().set_label("Submit");
/// node.config_mut().set_button(true);
/// node.config_mut()
///     .add_action(SemanticsAction::Tap, Arc::new(|_, _| {}));
///
/// assert!(node.has_been_annotated());
/// assert!(node.config().has_action(SemanticsAction::Tap));
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

    /// The render object that forms this semantics boundary.
    ///
    /// This generational identity survives semantics-arena rebuilds and is
    /// the sole source of the OS-facing [`AccessibilityNodeId`].
    source_render_id: Option<RenderId>,

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
            source_render_id: None,
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
        self.source_render_id = Some(render_id);
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
        self.source_render_id
    }

    /// Returns the stable OS-facing identity derived from the source render
    /// object, or `None` for a manually-created node that has not been bound to
    /// a render boundary.
    #[inline]
    pub fn accessibility_id(&self) -> Option<AccessibilityNodeId> {
        self.source_render_id.map(AccessibilityNodeId::from)
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

    // ========== Data Export ==========

    /// Converts this node's content to [`SemanticsNodeData`], keyed by its
    /// stable [`accessibility_id`](Self::accessibility_id) (`None` for a node
    /// never bound to a render boundary — such a node is not publishable).
    ///
    /// `children` is left **empty**: a node stores its children as arena
    /// [`SemanticsId`]s and cannot resolve their stable identities alone. The
    /// payload constructor that fills `children` is
    /// [`SemanticsTree::node_data`](crate::tree::SemanticsTree::node_data).
    pub fn to_node_data(&self) -> SemanticsNodeData {
        SemanticsNodeData {
            id: self.accessibility_id(),
            flags: self.config.flags().bits(),
            actions: self.config.effective_actions_as_bits(),
            label: self.config.label().map(|l| l.string.clone()),
            value: self.config.value().map(|v| v.string.clone()),
            numeric_range: self.config.numeric_range(),
            increased_value: self.config.increased_value().map(|v| v.string.clone()),
            decreased_value: self.config.decreased_value().map(|v| v.string.clone()),
            hint: self.config.hint().map(|h| h.string.clone()),
            tooltip: self.config.tooltip().map(Into::into),
            text_direction: self.config.text_direction(),
            role: self.config.role(),
            rect: self.rect,
            transform: self.transform.unwrap_or(Matrix4::IDENTITY),
            children: smallvec::SmallVec::new(),
            platform_view_id: self.config.platform_view_id(),
            max_value_length: self.config.max_value_length(),
            current_value_length: self.config.current_value_length(),
            scroll_position: self.config.scroll_position(),
            scroll_extent_max: self.config.scroll_extent_max(),
            scroll_extent_min: self.config.scroll_extent_min(),
            scroll_index: self.config.scroll_index(),
            scroll_child_count: self.config.scroll_child_count(),
            index_in_parent: self.config.index_in_parent(),
        }
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
