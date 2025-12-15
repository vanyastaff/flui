//! Base RenderObject trait.
//!
//! RenderObject is the base class for all objects in the render tree.

use std::any::Any;
use std::fmt::Debug;

use crate::lifecycle::BaseRenderObject;
use crate::parent_data::ParentData;
use crate::pipeline::PipelineOwner;
use crate::semantics::{SemanticsConfiguration, SemanticsEvent, SemanticsNode};

// ============================================================================
// Diagnostic Types
// ============================================================================

/// Builder for diagnostic properties.
///
/// Used by `debug_fill_properties` to collect diagnostic information.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `DiagnosticPropertiesBuilder`.
#[derive(Debug, Default)]
pub struct DiagnosticPropertiesBuilder {
    properties: Vec<DiagnosticProperty>,
}

impl DiagnosticPropertiesBuilder {
    /// Creates a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a string property.
    pub fn add_string(&mut self, name: &str, value: impl Into<String>) {
        self.properties.push(DiagnosticProperty {
            name: name.to_string(),
            value: DiagnosticValue::String(value.into()),
        });
    }

    /// Adds a boolean property.
    pub fn add_bool(&mut self, name: &str, value: bool) {
        self.properties.push(DiagnosticProperty {
            name: name.to_string(),
            value: DiagnosticValue::Bool(value),
        });
    }

    /// Adds an integer property.
    pub fn add_int(&mut self, name: &str, value: i64) {
        self.properties.push(DiagnosticProperty {
            name: name.to_string(),
            value: DiagnosticValue::Int(value),
        });
    }

    /// Adds a float property.
    pub fn add_float(&mut self, name: &str, value: f64) {
        self.properties.push(DiagnosticProperty {
            name: name.to_string(),
            value: DiagnosticValue::Float(value),
        });
    }

    /// Adds a flag property (shown only if true).
    pub fn add_flag(&mut self, name: &str, value: bool) {
        if value {
            self.properties.push(DiagnosticProperty {
                name: name.to_string(),
                value: DiagnosticValue::Flag(value),
            });
        }
    }

    /// Returns the collected properties.
    pub fn properties(&self) -> &[DiagnosticProperty] {
        &self.properties
    }

    /// Consumes the builder and returns the properties.
    pub fn into_properties(self) -> Vec<DiagnosticProperty> {
        self.properties
    }
}

/// A single diagnostic property.
#[derive(Debug, Clone)]
pub struct DiagnosticProperty {
    /// Property name.
    pub name: String,
    /// Property value.
    pub value: DiagnosticValue,
}

/// Value of a diagnostic property.
#[derive(Debug, Clone)]
pub enum DiagnosticValue {
    /// String value.
    String(String),
    /// Boolean value.
    Bool(bool),
    /// Integer value.
    Int(i64),
    /// Float value.
    Float(f64),
    /// Flag value (only shown if true).
    Flag(bool),
}

impl std::fmt::Display for DiagnosticValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String(s) => write!(f, "{}", s),
            Self::Bool(b) => write!(f, "{}", b),
            Self::Int(i) => write!(f, "{}", i),
            Self::Float(fl) => write!(f, "{}", fl),
            Self::Flag(b) => write!(f, "{}", b),
        }
    }
}

/// A node in the diagnostics tree.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `DiagnosticsNode`.
#[derive(Debug, Clone)]
pub struct DiagnosticsNode {
    /// Node name.
    pub name: String,
    /// Node description.
    pub description: String,
    /// Child properties.
    pub properties: Vec<DiagnosticProperty>,
}

// ============================================================================
// RenderObject Trait
// ============================================================================

/// Base trait for all render objects.
///
/// RenderObject is the core abstraction of the rendering system. It provides:
/// - Tree structure (parent, children, depth)
/// - Lifecycle management (attach, detach, dispose)
/// - Dirty marking (layout, paint, compositing, semantics)
/// - Parent data storage
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `RenderObject` abstract class in
/// `rendering/object.dart`.
///
/// # Implementation Notes
///
/// Most render objects don't implement this trait directly. Instead:
/// - For 2D box layout: implement [`RenderBox`](super::RenderBox)
/// - For scrollable content: implement [`RenderSliver`](super::RenderSliver)
///
/// # Thread Safety
///
/// All render objects must be `Send + Sync` to support parallel layout
/// and rendering operations.
pub trait RenderObject: Debug + Send + Sync + 'static {
    // ========================================================================
    // Base State Access
    // ========================================================================

    /// Returns the base render object state.
    ///
    /// This provides access to the unified lifecycle state management.
    /// Implementations should return a reference to their embedded
    /// [`BaseRenderObject`] instance.
    ///
    /// # Example Implementation
    ///
    /// ```ignore
    /// struct MyRenderBox {
    ///     base: BaseRenderObject,
    ///     // ... other fields
    /// }
    ///
    /// impl RenderObject for MyRenderBox {
    ///     fn base(&self) -> &BaseRenderObject {
    ///         &self.base
    ///     }
    ///
    ///     fn base_mut(&mut self) -> &mut BaseRenderObject {
    ///         &mut self.base
    ///     }
    ///     // ...
    /// }
    /// ```
    fn base(&self) -> &BaseRenderObject;

    /// Returns mutable access to the base render object state.
    fn base_mut(&mut self) -> &mut BaseRenderObject;

    // ========================================================================
    // Tree Structure
    // ========================================================================

    /// Returns the parent render object, if any.
    ///
    /// Default implementation returns `None`. Override if you track parent.
    fn parent(&self) -> Option<&dyn RenderObject> {
        None
    }

    /// Returns the depth of this node in the render tree.
    ///
    /// The depth of nodes in a tree monotonically increases as you traverse down
    /// the tree: a node always has a depth greater than its ancestors.
    /// There's no guarantee regarding depth between siblings.
    ///
    /// The root has depth 0, its children have depth 1, etc.
    ///
    /// Default implementation delegates to `base().depth()`.
    fn depth(&self) -> usize {
        self.base().depth()
    }

    /// Sets the depth of this node.
    ///
    /// This is called internally by `adopt_child` and `redepth_child`.
    /// Users should not call this directly.
    ///
    /// Default implementation delegates to `base_mut().set_depth()`.
    fn set_depth(&mut self, depth: usize) {
        self.base_mut().set_depth(depth);
    }

    /// Returns the pipeline owner that manages this render object.
    ///
    /// Implementations should return a reference to their pipeline owner.
    /// Note: The default implementations of dirty marking methods use
    /// `base()` which stores `Arc<RwLock<PipelineOwner>>` internally.
    fn owner(&self) -> Option<&PipelineOwner>;

    /// Sets the parent of this render object.
    ///
    /// This is called internally by `adopt_child` and `drop_child`.
    /// Users should not call this directly.
    ///
    /// Pass `None` to clear the parent reference.
    ///
    /// Default implementation delegates to `base_mut().state_mut().set_parent_ptr()`.
    fn set_parent(&mut self, parent: Option<*const dyn RenderObject>) {
        self.base_mut().state_mut().set_parent_ptr(parent);
    }

    // ========================================================================
    // Lifecycle
    // ========================================================================

    /// Called when this render object is attached to a pipeline owner.
    ///
    /// This is called when the render object is inserted into the tree
    /// or when the tree is attached to a pipeline owner.
    fn attach(&mut self, owner: &PipelineOwner);

    /// Called when this render object is detached from its pipeline owner.
    ///
    /// This is called when the render object is removed from the tree
    /// or when the tree is detached from its pipeline owner.
    fn detach(&mut self);

    /// Releases any resources held by this render object.
    ///
    /// Called when the render object will never be used again.
    /// After calling dispose, the object is no longer usable.
    fn dispose(&mut self) {}

    /// Returns whether this render object is attached to a pipeline owner.
    fn attached(&self) -> bool {
        self.owner().is_some()
    }

    // ========================================================================
    // Child Management
    // ========================================================================

    /// Called by subclasses when they decide a render object is a child.
    ///
    /// This method:
    /// 1. Sets up parent data for the child
    /// 2. Marks this object as needing layout, compositing bits update, and semantics update
    /// 3. Sets the child's parent to this object
    /// 4. If attached, attaches the child to the same owner
    /// 5. Adjusts the child's depth
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.adoptChild` method.
    ///
    /// # Panics
    ///
    /// Panics if the child already has a parent.
    fn adopt_child(&mut self, child: &mut dyn RenderObject);

    /// Called by subclasses when they decide a render object is no longer a child.
    ///
    /// This method:
    /// 1. Detaches the child's parent data
    /// 2. Clears the child's parent reference
    /// 3. If attached, detaches the child
    /// 4. Marks this object as needing layout, compositing bits update, and semantics update
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.dropChild` method.
    ///
    /// # Panics
    ///
    /// Panics if the child's parent is not this object.
    fn drop_child(&mut self, child: &mut dyn RenderObject);

    /// Adjusts the depth of the given child to be greater than this node's own depth.
    ///
    /// Only call this method from overrides of `redepth_children`.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.redepthChild` method.
    fn redepth_child(&mut self, child: &mut dyn RenderObject);

    /// Adjusts the depth of this node's children, if any.
    ///
    /// Override this method in subclasses with child nodes to call `redepth_child`
    /// for each child. Do not call this method directly.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.redepthChildren` method.
    fn redepth_children(&mut self) {
        // Default: no children to redepth
    }

    // ========================================================================
    // Hot Reload Support
    // ========================================================================

    /// Cause the entire subtree rooted at this RenderObject to be marked
    /// dirty for layout, paint, etc.
    ///
    /// This is called in response to hot reload, to cause the widget tree
    /// to pick up any changed implementations.
    ///
    /// This is expensive and should not be called except during development.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.reassemble` method.
    fn reassemble(&mut self) {
        self.mark_needs_layout();
        self.mark_needs_compositing_bits_update();
        self.mark_needs_paint();
        self.mark_needs_semantics_update();
        self.visit_children_mut(&mut |child| {
            child.reassemble();
        });
    }

    // ========================================================================
    // Dirty State
    // ========================================================================

    /// Returns whether this render object's layout information is dirty.
    ///
    /// This returns true if `mark_needs_layout()` has been called since the
    /// last time the render object was laid out.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.debugNeedsLayout` getter.
    ///
    /// Default implementation delegates to `base().needs_layout()`.
    fn needs_layout(&self) -> bool {
        self.base().needs_layout()
    }

    /// Alias for `needs_layout()` to match Flutter's debug naming.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.debugNeedsLayout` getter.
    fn debug_needs_layout(&self) -> bool {
        self.needs_layout()
    }

    /// Returns whether this render object needs to be repainted.
    ///
    /// This returns true if `mark_needs_paint()` has been called since the
    /// last time the render object was painted.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.debugNeedsPaint` getter.
    ///
    /// Default implementation delegates to `base().needs_paint()`.
    fn needs_paint(&self) -> bool {
        self.base().needs_paint()
    }

    /// Alias for `needs_paint()` to match Flutter's debug naming.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.debugNeedsPaint` getter.
    fn debug_needs_paint(&self) -> bool {
        self.needs_paint()
    }

    /// Returns whether this render object needs compositing bits update.
    ///
    /// This returns true if `mark_needs_compositing_bits_update()` has been
    /// called since the last time the compositing bits were updated.
    ///
    /// Default implementation delegates to `base().needs_compositing_bits_update()`.
    fn needs_compositing_bits_update(&self) -> bool {
        self.base().needs_compositing_bits_update()
    }

    /// Returns whether this render object is a relayout boundary.
    ///
    /// A relayout boundary is a render object whose parent does not rely on
    /// the child's size in its own layout algorithm. When a relayout boundary
    /// is marked as needing layout, its parent does not have to be marked dirty.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject._isRelayoutBoundary` field.
    ///
    /// Default implementation delegates to `base().is_relayout_boundary()`.
    fn is_relayout_boundary(&self) -> bool {
        self.base().is_relayout_boundary()
    }

    // ========================================================================
    // Debug State Getters
    // ========================================================================

    /// Returns whether this render object is currently performing resize.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.debugDoingThisResize` getter.
    fn debug_doing_this_resize(&self) -> bool {
        false
    }

    /// Returns whether this render object is currently performing layout.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.debugDoingThisLayout` getter.
    fn debug_doing_this_layout(&self) -> bool {
        false
    }

    /// Returns whether the parent can use this render object's size.
    ///
    /// This is set during layout to indicate whether the parent was given
    /// permission to use this object's size in its own layout.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.debugCanParentUseSize` getter.
    fn debug_can_parent_use_size(&self) -> bool {
        false
    }

    /// Returns whether this render object is currently being painted.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.debugDoingThisPaint` getter.
    fn debug_doing_this_paint(&self) -> bool {
        false
    }

    /// Returns whether this render object needs a composited layer update.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.debugNeedsCompositedLayerUpdate` getter.
    fn debug_needs_composited_layer_update(&self) -> bool {
        false
    }

    /// Returns whether this render object needs a semantics update.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.debugNeedsSemanticsUpdate` getter.
    fn debug_needs_semantics_update(&self) -> bool {
        false
    }

    /// Returns the semantics node associated with this render object, if any.
    ///
    /// This is only available in debug mode.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.debugSemantics` getter.
    fn debug_semantics(&self) -> Option<&SemanticsNode> {
        None
    }

    /// Returns the creator of this render object for debugging purposes.
    ///
    /// This is typically set to the widget that created the render object.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.debugCreator` field.
    ///
    /// Default implementation delegates to `base().debug_creator()`.
    fn debug_creator(&self) -> Option<&str> {
        self.base().debug_creator()
    }

    /// Sets the creator of this render object for debugging purposes.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.debugCreator` setter.
    ///
    /// Default implementation delegates to `base_mut().set_debug_creator()`.
    fn set_debug_creator(&mut self, creator: Option<String>) {
        self.base_mut().set_debug_creator(creator);
    }

    // ========================================================================
    // Dirty Marking
    // ========================================================================

    /// Marks this render object as needing layout.
    ///
    /// Call this when something changes that affects the layout of this
    /// object or its descendants.
    ///
    /// Default implementation delegates to `base_mut().mark_needs_layout()`.
    fn mark_needs_layout(&mut self) {
        self.base_mut().mark_needs_layout();
    }

    /// Marks this render object as needing paint.
    ///
    /// Call this when something changes that affects the visual appearance
    /// of this object but not its layout.
    ///
    /// Default implementation delegates to `base_mut().mark_needs_paint()`.
    fn mark_needs_paint(&mut self) {
        self.base_mut().mark_needs_paint();
    }

    /// Marks this render object as needing compositing bits update.
    ///
    /// Call this when something changes that affects whether this object
    /// or its descendants need compositing.
    ///
    /// Default implementation delegates to `base_mut().mark_needs_compositing_bits_update()`.
    fn mark_needs_compositing_bits_update(&mut self) {
        self.base_mut().mark_needs_compositing_bits_update();
    }

    /// Marks this render object as needing semantics update.
    ///
    /// Call this when something changes that affects the semantics
    /// (accessibility) of this object.
    ///
    /// Default implementation delegates to `base_mut().mark_needs_semantics_update()`.
    fn mark_needs_semantics_update(&mut self) {
        self.base_mut().mark_needs_semantics_update();
    }

    /// Clears the needs_layout flag after layout is complete.
    ///
    /// This is called by the layout system after a render object has been laid out.
    /// Implementations should set their internal `needs_layout` flag to false.
    ///
    /// Default implementation delegates to `base_mut().clear_needs_layout()`.
    fn clear_needs_layout(&mut self) {
        self.base_mut().clear_needs_layout();
    }

    /// Clears the needs_paint flag after painting is complete.
    ///
    /// This is called by the paint system after a render object has been painted.
    /// Implementations should set their internal `needs_paint` flag to false.
    ///
    /// Default implementation delegates to `base_mut().clear_needs_paint()`.
    fn clear_needs_paint(&mut self) {
        self.base_mut().clear_needs_paint();
    }

    /// Clears the needs_compositing_bits_update flag.
    ///
    /// This is called after compositing bits have been updated.
    ///
    /// Default implementation delegates to `base_mut().clear_needs_compositing_bits_update()`.
    fn clear_needs_compositing_bits_update(&mut self) {
        self.base_mut().clear_needs_compositing_bits_update();
    }

    // ========================================================================
    // Layout
    // ========================================================================

    /// Compute the layout for this render object.
    ///
    /// This method is the main entry point for layout. It calls `perform_resize`
    /// if `sized_by_parent` is true, then calls the subclass-specific layout method.
    ///
    /// The `parent_uses_size` parameter indicates whether the parent render object
    /// will use this object's size in its own layout. If false, this render object
    /// can be a relayout boundary.
    ///
    /// **Note:** This is a base implementation. `RenderBox` and `RenderSliver`
    /// have their own `perform_layout` methods that take constraints as parameters.
    /// The actual layout orchestration is handled by the `PipelineOwner`.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.layout` method.
    fn layout(&mut self, parent_uses_size: bool) {
        // In Flutter, this method receives constraints and orchestrates layout.
        // In our Rust implementation, the constraints are passed to perform_layout
        // in RenderBox/RenderSliver directly. This method handles the common logic.

        // If sized_by_parent, call perform_resize first
        if self.sized_by_parent() {
            self.perform_resize();
        }

        // Clear the needs_layout flag
        self.clear_needs_layout();

        // Mark as relayout boundary if parent doesn't use our size
        // (This would typically be stored in a field, but we keep it simple here)
        let _ = parent_uses_size;
    }

    /// Computes the size of this render object when `sized_by_parent` is true.
    ///
    /// This is called when `sized_by_parent` returns true, before layout.
    /// It should set the size of this render object based only on the constraints.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.performResize` method.
    fn perform_resize(&mut self) {
        // Default: do nothing
        // Subclasses that use sized_by_parent should override this
    }

    /// Override this method to paint debugging information.
    ///
    /// This is only called in debug mode and should not affect the layout.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.debugPaint` method.
    fn debug_paint(
        &self,
        _context: &mut crate::pipeline::PaintingContext,
        _offset: flui_types::Offset,
    ) {
        // Default: no debug painting
    }

    // ========================================================================
    // Hit Testing & Events
    // ========================================================================

    /// Called when a pointer event is received by this render object.
    ///
    /// Override this to handle pointer events like taps, drags, etc.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.handleEvent` method.
    fn handle_event(
        &self,
        _event: &crate::hit_testing::PointerEvent,
        _entry: &crate::hit_testing::HitTestEntry,
    ) {
        // Default: do nothing
    }

    /// Attempt to make this render object (or a descendant) visible.
    ///
    /// This method is called when the framework needs to make a render object
    /// visible, for example when it gains focus.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.showOnScreen` method.
    fn show_on_screen(&self) {
        // Default: do nothing
        // Subclasses like viewports should scroll to make the target visible
    }

    // ========================================================================
    // Semantics
    // ========================================================================

    /// Bootstrap the semantics system by scheduling the initial semantics update.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.scheduleInitialSemantics` method.
    fn schedule_initial_semantics(&mut self) {
        // Default: mark needs semantics update
        self.mark_needs_semantics_update();
    }

    /// Describes the semantic annotations for this render object.
    ///
    /// Override this to provide semantic information for accessibility.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.describeSemanticsConfiguration` method.
    fn describe_semantics_configuration(
        &self,
        _config: &mut crate::semantics::SemanticsConfiguration,
    ) {
        // Default: no semantic configuration
    }

    /// Visits children for the purposes of building the semantics tree.
    ///
    /// By default, this visits all children. Override to exclude children
    /// that should not contribute to semantics.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.visitChildrenForSemantics` method.
    fn visit_children_for_semantics(&self, visitor: &mut dyn FnMut(&dyn RenderObject)) {
        self.visit_children(visitor);
    }

    /// Clears any cached semantics information.
    ///
    /// Called when the semantics configuration for this render object changes.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.clearSemantics` method.
    fn clear_semantics(&mut self) {
        // Default: do nothing
        // Subclasses with cached semantics should clear them here
    }

    /// Sends a semantics event associated with this render object's semantics node.
    ///
    /// This is used to notify assistive technologies of events that occur
    /// without necessarily changing the semantics tree structure.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.sendSemanticsEvent` method.
    fn send_semantics_event(&self, _event: SemanticsEvent) {
        // Default: do nothing
        // The owner should dispatch this to the platform's accessibility system
        // In a real implementation, this would be forwarded to the SemanticsOwner
    }

    /// Assemble the semantics node for this render object.
    ///
    /// This is called during semantics tree building to populate the
    /// semantics node with information from this render object.
    ///
    /// The `config` parameter contains the configuration that was built
    /// by calling `describe_semantics_configuration`.
    ///
    /// The `children` parameter contains semantics nodes from child render
    /// objects that should be added to this node.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.assembleSemanticsNode` method.
    fn assemble_semantics_node(
        &self,
        node: &mut SemanticsNode,
        config: &SemanticsConfiguration,
        children: Vec<SemanticsNode>,
    ) {
        // Default implementation: copy config to node and add children
        node.set_config(config.clone());
        for child in children {
            node.add_child(child.id());
        }
    }

    // ========================================================================
    // Dirty Marking (continued)
    // ========================================================================

    /// Marks the parent render object as needing layout.
    ///
    /// This function should only be called from `mark_needs_layout` or
    /// `mark_needs_layout_for_sized_by_parent_change` implementations of
    /// subclasses that introduce more reasons for deferring the handling
    /// of dirty layout to the parent.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.markParentNeedsLayout` method.
    fn mark_parent_needs_layout(&mut self);

    /// Marks this render object's layout information as dirty, and additionally
    /// handles the case where `sized_by_parent` has changed value.
    ///
    /// This should be called whenever `sized_by_parent` might have changed.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.markNeedsLayoutForSizedByParentChange` method.
    fn mark_needs_layout_for_sized_by_parent_change(&mut self) {
        self.mark_needs_layout();
        self.mark_parent_needs_layout();
    }

    /// Bootstrap the rendering pipeline by scheduling the very first layout.
    ///
    /// Requires this render object to be attached and that this render object
    /// is the root of the render tree.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.scheduleInitialLayout` method.
    fn schedule_initial_layout(&mut self);

    /// Bootstrap the rendering pipeline by scheduling the very first paint.
    ///
    /// Requires that this render object is attached, is the root of the render
    /// tree, and has a composited layer.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.scheduleInitialPaint` method.
    fn schedule_initial_paint(&mut self);

    // ========================================================================
    // Compositing
    // ========================================================================

    /// Returns whether this render object or one of its descendants has a
    /// compositing layer.
    ///
    /// If this node needs compositing, then all ancestor nodes will also
    /// need compositing.
    ///
    /// Only legal to call after layout and compositing bits update phases.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.needsCompositing` getter.
    ///
    /// Default implementation delegates to `base().needs_compositing()`.
    fn needs_compositing(&self) -> bool {
        self.base().needs_compositing()
    }

    /// Sets whether this render object needs compositing.
    ///
    /// This is called internally during `update_compositing_bits`.
    ///
    /// Default implementation delegates to `base_mut().set_needs_compositing()`.
    fn set_needs_compositing(&mut self, value: bool) {
        self.base_mut().set_needs_compositing(value);
    }

    /// Updates the compositing bits for this render object.
    ///
    /// This method recomputes whether this render object and its descendants
    /// need compositing. It's called during the compositing bits update phase.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject._updateCompositingBits` method.
    fn update_compositing_bits(&mut self) {
        if !self.needs_compositing_bits_update() {
            return;
        }

        let mut child_needs_compositing = false;
        self.visit_children_mut(&mut |child| {
            child.update_compositing_bits();
            if child.needs_compositing() {
                child_needs_compositing = true;
            }
        });

        let needs_compositing = child_needs_compositing
            || self.is_repaint_boundary()
            || self.always_needs_compositing();
        self.set_needs_compositing(needs_compositing);
        self.clear_needs_compositing_bits_update();

        // If compositing state changed, we need to repaint
        if self.needs_compositing() != child_needs_compositing {
            self.mark_needs_paint();
        }
    }

    /// Mark this render object as having changed a property on its composited layer.
    ///
    /// This method is used when a render object's layer properties need to be
    /// updated without repainting all children.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.markNeedsCompositedLayerUpdate` method.
    fn mark_needs_composited_layer_update(&mut self) {
        // Default implementation just marks needs paint
        // Subclasses with composited layers can optimize this
        self.mark_needs_paint();
    }

    /// Returns whether this render object has a layer.
    ///
    /// Only repaint boundaries should have layers.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to checking `RenderObject.layer != null` in Flutter.
    fn has_layer(&self) -> bool {
        false
    }

    /// Returns the compositing layer for this render object, if any.
    ///
    /// Repaint boundaries typically have their own layer. This getter
    /// provides access to that layer for compositing operations.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.layer` getter.
    fn layer(&self) -> Option<&dyn crate::layer::Layer> {
        None
    }

    /// Returns the compositing layer for debug purposes.
    ///
    /// This is the same as `layer()` but available for debug assertions.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.debugLayer` getter.
    fn debug_layer(&self) -> Option<&dyn crate::layer::Layer> {
        self.layer()
    }

    /// Replaces the root layer for this render object.
    ///
    /// This should only be called on a render object that:
    /// - Is the root of the render tree
    /// - Is a repaint boundary
    /// - Already has a layer (from `schedule_initial_paint`)
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.replaceRootLayer` method.
    fn replace_root_layer(&mut self) {
        // Default implementation does nothing
        // Only root render objects with layers should implement this
    }

    /// Updates the composited layer for this render object.
    ///
    /// This is called during painting when the render object is a repaint
    /// boundary and needs to update its layer properties.
    ///
    /// Returns the updated layer, or creates a new one if needed.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.updateCompositedLayer` method.
    fn update_composited_layer(&mut self) {
        // Default implementation does nothing
        // Only repaint boundaries should implement this
    }

    // ========================================================================
    // Layout Configuration
    // ========================================================================

    /// Whether this render object's size is determined entirely by its parent.
    ///
    /// If true, the parent can skip calling layout on this object when
    /// only the parent's constraints change but the child's intrinsic
    /// dimensions haven't changed.
    ///
    /// Default is `false`.
    fn sized_by_parent(&self) -> bool {
        false
    }

    /// Whether this render object creates a new paint layer.
    ///
    /// If true, this render object will be painted into its own layer,
    /// which can improve performance when parts of the UI change frequently.
    ///
    /// Default implementation delegates to `base().is_repaint_boundary()`.
    fn is_repaint_boundary(&self) -> bool {
        self.base().is_repaint_boundary()
    }

    /// Whether this render object always needs compositing.
    ///
    /// If true, this render object requires a compositing layer even
    /// if it has no children that require compositing.
    ///
    /// Default is `false`.
    fn always_needs_compositing(&self) -> bool {
        false
    }

    // ========================================================================
    // Layout Callbacks
    // ========================================================================

    /// Whether this render object is currently invoking a layout callback.
    ///
    /// This is used by the layout system to detect reentrancy.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.debugDoingThisLayoutWithCallback` getter.
    fn doing_this_layout_with_callback(&self) -> bool {
        false
    }

    /// Allows mutations to this render object's descendants during layout.
    ///
    /// This method is used by render objects like `RenderLayoutBuilder` that
    /// need to mutate descendants during the layout phase.
    ///
    /// The callback is invoked synchronously and any mutations made during
    /// the callback will be processed after the callback returns.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.invokeLayoutCallback` method.
    fn invoke_layout_callback(&mut self, _callback: Box<dyn FnOnce() + Send>) {
        // Default implementation does nothing
        // Subclasses that support layout callbacks should implement this
    }

    // ========================================================================
    // Parent Data
    // ========================================================================

    /// Sets up the parent data for a child.
    ///
    /// Called when a child is added to this render object. Override this
    /// to set up custom parent data types.
    fn setup_parent_data(&self, child: &mut dyn RenderObject) {
        let _ = child;
    }

    /// Returns the parent data for this render object.
    ///
    /// Default implementation delegates to `base().parent_data()`.
    fn parent_data(&self) -> Option<&dyn ParentData> {
        self.base().parent_data()
    }

    /// Returns mutable parent data for this render object.
    ///
    /// Default implementation delegates to `base_mut().parent_data_mut()`.
    fn parent_data_mut(&mut self) -> Option<&mut dyn ParentData> {
        self.base_mut().parent_data_mut()
    }

    /// Sets the parent data for this render object.
    ///
    /// Default implementation delegates to `base_mut().set_parent_data()`.
    fn set_parent_data(&mut self, data: Box<dyn ParentData>) {
        self.base_mut().set_parent_data(data);
    }

    // ========================================================================
    // Children
    // ========================================================================

    /// Visits each child render object.
    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn RenderObject));

    /// Visits each child render object mutably.
    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn RenderObject));

    // ========================================================================
    // Painting
    // ========================================================================

    /// Returns an estimate of the bounds within which this render object will paint.
    ///
    /// The bounds are used to determine the size of the composited layer,
    /// if any, and to determine if the render object is visible.
    ///
    /// This method must return a rectangle that is at least as large as
    /// the area that the render object will paint.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.paintBounds` getter.
    fn paint_bounds(&self) -> flui_types::Rect;

    /// Applies the transform that would be applied when painting to the
    /// given matrix.
    ///
    /// This is used by coordinate conversion functions to translate coordinates
    /// local to this render object to coordinates in the coordinate space
    /// of another render object.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.applyPaintTransform` method.
    fn apply_paint_transform(&self, child: &dyn RenderObject, transform: &mut [f32; 16]) {
        let _ = child;
        let _ = transform;
        // Default: identity transform (no modification)
    }

    /// Returns the semantic clip rectangle.
    ///
    /// This is used by the semantics system to determine what parts of
    /// the render object are visible for accessibility purposes.
    ///
    /// Returns `None` if the entire paint bounds should be used.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.describeApproximatePaintClip` method.
    fn describe_approximate_paint_clip(
        &self,
        _child: &dyn RenderObject,
    ) -> Option<flui_types::Rect> {
        None
    }

    /// Returns the semantics clip for this render object.
    ///
    /// This is similar to `describe_approximate_paint_clip` but for semantics.
    /// Returns the clip rect that should be used for semantic purposes.
    ///
    /// Returns `None` if there is no clip.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.describeSemanticsClip` method.
    fn describe_semantics_clip(&self, _child: &dyn RenderObject) -> Option<flui_types::Rect> {
        None
    }

    /// Returns the semantic bounds of this render object.
    ///
    /// This is used by the semantics system to determine the area that
    /// represents this render object for accessibility purposes.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.semanticBounds` getter.
    fn semantic_bounds(&self) -> flui_types::Rect {
        self.paint_bounds()
    }

    /// Returns whether the given child would be painted if paint were called.
    ///
    /// Some render objects skip painting their children if they are configured
    /// to not produce any visible effects. For example, a RenderOffstage with
    /// offstage set to true, or a RenderOpacity with opacity set to zero.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.paintsChild` method.
    fn paints_child(&self, _child: &dyn RenderObject) -> bool {
        true
    }

    /// Applies the paint transform from this render object up to `target`.
    ///
    /// Returns a matrix that maps the local paint coordinate system to the
    /// coordinate system of `target`, or an identity matrix if the paint
    /// transform cannot be computed.
    ///
    /// If `target` is `None`, this method returns a matrix that maps from the
    /// local paint coordinate system to the coordinate system of the root.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.getTransformTo` method.
    fn get_transform_to(&self, target: Option<&dyn RenderObject>) -> [f32; 16] {
        // Identity matrix by default
        // Subclasses should implement proper transform accumulation
        let _ = target;
        [
            1.0, 0.0, 0.0, 0.0, // column 0
            0.0, 1.0, 0.0, 0.0, // column 1
            0.0, 0.0, 1.0, 0.0, // column 2
            0.0, 0.0, 0.0, 1.0, // column 3
        ]
    }

    // ========================================================================
    // Debug Information
    // ========================================================================

    /// Returns a human-readable description of this render object.
    ///
    /// Used for debugging and diagnostics.
    fn describe(&self) -> String {
        format!("{:?}", self)
    }

    /// Returns detailed diagnostic information about this render object.
    ///
    /// Used for debugging and developer tools.
    fn to_debug_string(&self) -> String {
        self.describe()
    }

    /// Add additional properties to the given property builder.
    ///
    /// Override this method to add diagnostic information about this object.
    /// This is called by the debug tools to gather diagnostic data.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.debugFillProperties` method.
    fn debug_fill_properties(&self, properties: &mut DiagnosticPropertiesBuilder) {
        // Default: add basic information
        properties.add_string("description", self.describe());
        properties.add_bool("needsLayout", self.needs_layout());
        properties.add_bool("needsPaint", self.needs_paint());
        properties.add_bool("needsCompositing", self.needs_compositing());
    }

    /// Returns a list of diagnostics describing this node's children.
    ///
    /// Override this method to provide information about children for debug tools.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderObject.debugDescribeChildren` method.
    fn debug_describe_children(&self) -> Vec<DiagnosticsNode> {
        let mut children = Vec::new();
        let mut index = 0;
        self.visit_children(&mut |child| {
            children.push(DiagnosticsNode {
                name: format!("child {}", index),
                description: child.describe(),
                properties: Vec::new(),
            });
            index += 1;
        });
        children
    }

    // ========================================================================
    // Type Inspection
    // ========================================================================

    /// Returns self as `Any` for downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Returns self as mutable `Any` for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// ============================================================================
// Helper Extensions
// ============================================================================

/// Extension trait for downcasting render objects.
pub trait RenderObjectExt: RenderObject {
    /// Attempts to downcast to a concrete type.
    fn downcast_ref<T: RenderObject>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }

    /// Attempts to downcast to a concrete type mutably.
    fn downcast_mut<T: RenderObject>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
    }
}

impl<T: RenderObject + ?Sized> RenderObjectExt for T {}
