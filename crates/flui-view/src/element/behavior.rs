//! Element behaviors - view-specific build logic.
//!
//! This module defines the ElementBehavior trait and implementations for each
//! view type (Stateless, Proxy, Stateful, Render). Behaviors encapsulate the
//! view-specific logic while the unified Element handles all common operations.

use super::arity::ElementArity;
use super::generic::ElementCore;
use crate::context::ElementBuildContext;
use crate::view::{
    AnimatedView, InheritedView, ProxyView, StatefulView, StatelessView, View, ViewState,
};
use flui_foundation::{ListenerId, RenderId};
use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::protocol::Protocol;
use flui_rendering::traits::RenderObject as RenderObjectTrait;
use std::marker::PhantomData;

use crate::element::RenderSlot;
use crate::view::RenderView;
use flui_foundation::ElementId;

// ============================================================================
// ElementBehavior Trait
// ============================================================================

/// Trait for view-specific element behavior.
///
/// This trait encapsulates all the view-type-specific logic that varies between
/// Stateless, Proxy, Stateful, and Render elements. The unified Element<V,A,B>
/// delegates to this trait for view-specific operations.
pub trait ElementBehavior<V, A>: Send + Sync + 'static
where
    V: Clone + Send + Sync + 'static,
    A: ElementArity,
{
    /// Perform the build operation for this view type.
    fn perform_build(&mut self, core: &mut ElementCore<V, A>);

    /// Called after mount to perform behavior-specific setup.
    #[allow(unused_variables)]
    fn on_mount(&mut self, core: &mut ElementCore<V, A>) {}

    /// Called before unmount to perform behavior-specific cleanup.
    #[allow(unused_variables)]
    fn on_unmount(&mut self, core: &mut ElementCore<V, A>) {}

    /// Called after view update to perform behavior-specific reactions.
    #[allow(unused_variables)]
    fn on_update(&mut self, core: &ElementCore<V, A>) {}
}

// ============================================================================
// StatelessBehavior
// ============================================================================

/// Behavior for StatelessView elements.
///
/// Calls view.build() to get the child view.
#[derive(Debug, Clone, Copy)]
pub struct StatelessBehavior;

impl StatelessBehavior {
    /// Create a new StatelessBehavior.
    pub fn new() -> Self {
        Self
    }
}

impl Default for StatelessBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl<V, A> ElementBehavior<V, A> for StatelessBehavior
where
    V: StatelessView,
    A: ElementArity,
{
    fn perform_build(&mut self, core: &mut ElementCore<V, A>) {
        if !core.should_build() {
            tracing::trace!("StatelessBehavior::perform_build skipped");
            return;
        }

        tracing::debug!("StatelessBehavior::perform_build starting");

        let ctx = ElementBuildContext::new_minimal(core.depth());
        let child_view = core.view().build(&ctx);
        core.update_or_create_child(child_view);
        core.clear_dirty();

        tracing::debug!("StatelessBehavior::perform_build completed");
    }
}

// ============================================================================
// ProxyBehavior
// ============================================================================

/// Behavior for ProxyView elements.
///
/// Gets the child directly from view.child() without building.
#[derive(Debug, Clone, Copy)]
pub struct ProxyBehavior;

impl ProxyBehavior {
    /// Create a new ProxyBehavior.
    pub fn new() -> Self {
        Self
    }
}

impl Default for ProxyBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl<V, A> ElementBehavior<V, A> for ProxyBehavior
where
    V: ProxyView,
    A: ElementArity,
{
    fn perform_build(&mut self, core: &mut ElementCore<V, A>) {
        if !core.should_build() {
            tracing::trace!("ProxyBehavior::perform_build skipped");
            return;
        }

        tracing::debug!("ProxyBehavior::perform_build starting");

        let child_view = core.view().child();
        let child_view_boxed = dyn_clone::clone_box(child_view);
        core.update_or_create_child(child_view_boxed);
        core.clear_dirty();

        tracing::debug!("ProxyBehavior::perform_build completed");
    }
}

// ============================================================================
// StatefulBehavior
// ============================================================================

/// Behavior for StatefulView elements.
///
/// Manages persistent state and calls state.build().
#[derive(Debug)]
pub struct StatefulBehavior<V: StatefulView> {
    /// The persistent state for this element.
    pub state: V::State,
    initialized: bool,
}

impl<V: StatefulView> StatefulBehavior<V> {
    /// Create a new StatefulBehavior by creating the state from the view.
    pub fn new(view: &V) -> Self {
        Self {
            state: view.create_state(),
            initialized: false,
        }
    }

    /// Get a reference to the state.
    pub fn state(&self) -> &V::State {
        &self.state
    }

    /// Get a mutable reference to the state.
    pub fn state_mut(&mut self) -> &mut V::State {
        &mut self.state
    }
}

impl<V, A> ElementBehavior<V, A> for StatefulBehavior<V>
where
    V: StatefulView,
    A: ElementArity,
{
    fn perform_build(&mut self, core: &mut ElementCore<V, A>) {
        let ctx = ElementBuildContext::new_minimal(core.depth());

        // Initialize state on first build
        if !self.initialized {
            self.state.init_state(&ctx);
            self.initialized = true;
        }

        if !core.should_build() {
            tracing::trace!("StatefulBehavior::perform_build skipped");
            return;
        }

        tracing::debug!("StatefulBehavior::perform_build starting");

        let child_view = self.state.build(core.view(), &ctx);
        core.update_or_create_child(child_view);
        core.clear_dirty();

        tracing::debug!("StatefulBehavior::perform_build completed");
    }

    fn on_unmount(&mut self, _core: &mut ElementCore<V, A>) {
        self.state.dispose();
    }
}

// ============================================================================
// RenderBehavior
// ============================================================================

/// Behavior for RenderView elements.
///
/// Manages RenderObject creation, mounting, and RenderTree integration.
#[derive(Debug)]
pub struct RenderBehavior<V: RenderView> {
    /// The RenderObject ID in RenderTree.
    pub render_id: Option<RenderId>,
    /// Current slot in parent.
    pub slot: RenderSlot,
    /// Ancestor RenderObjectElement (for render tree attachment).
    pub ancestor_render_object_element: Option<ElementId>,
    /// Marker for RenderObject type.
    _phantom: PhantomData<V::RenderObject>,
}

impl<V: RenderView> RenderBehavior<V> {
    /// Create a new RenderBehavior.
    pub fn new() -> Self {
        Self {
            render_id: None,
            slot: RenderSlot::default(),
            ancestor_render_object_element: None,
            _phantom: PhantomData,
        }
    }

    /// Get the RenderObject ID if created.
    pub fn render_id(&self) -> Option<RenderId> {
        self.render_id
    }

    /// Get a reference to the RenderObject ID.
    pub fn render_id_ref(&self) -> &Option<RenderId> {
        &self.render_id
    }

    /// Get the current slot in parent.
    pub fn slot(&self) -> &RenderSlot {
        &self.slot
    }

    /// Set the slot in parent.
    pub fn set_slot(&mut self, slot: RenderSlot) {
        self.slot = slot;
    }

    /// Get the ancestor RenderObjectElement ID.
    pub fn ancestor_render_object_element(&self) -> Option<ElementId> {
        self.ancestor_render_object_element
    }

    /// Set the ancestor RenderObjectElement ID.
    pub fn set_ancestor_render_object_element(&mut self, ancestor: Option<ElementId>) {
        self.ancestor_render_object_element = ancestor;
    }
}

impl<V: RenderView> Default for RenderBehavior<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V, A> ElementBehavior<V, A> for RenderBehavior<V>
where
    V: RenderView,
    A: ElementArity,
    flui_rendering::storage::RenderNode:
        From<Box<dyn flui_rendering::traits::RenderObject<V::Protocol>>>,
{
    fn perform_build(&mut self, core: &mut ElementCore<V, A>) {
        if !core.should_build() {
            tracing::trace!(
                "RenderBehavior::perform_build skipped render_id={:?}",
                self.render_id
            );
            return;
        }

        tracing::info!(
            "RenderBehavior::perform_build START render_id={:?}",
            self.render_id
        );

        let has_children = core.view().has_children();

        if has_children {
            let mut child_views: Vec<Box<dyn View>> = Vec::new();
            core.view().visit_child_views(&mut |child_view| {
                child_views.push(dyn_clone::clone_box(child_view));
            });

            core.update_or_create_children(child_views);
        }

        core.clear_dirty();

        tracing::debug!(
            "RenderBehavior::perform_build completed render_id={:?}",
            self.render_id
        );
    }

    fn on_mount(&mut self, core: &mut ElementCore<V, A>) {
        // Create RenderObject and insert into RenderTree
        if let Some(ref pipeline_owner) = core.pipeline_owner() {
            tracing::info!("RenderBehavior::on_mount creating RenderObject");

            let render_object = core.view().create_render_object();

            let render_id = {
                let mut owner = pipeline_owner.write();

                // Use helper to insert (handles Protocol type)
                let render_id = insert_render_object_helper(render_object, &mut owner);

                // Handle parent relationship
                if let Some(parent_id) = core.parent_render_id() {
                    let render_tree = owner.render_tree_mut();
                    if let Some(node) = render_tree.get_mut(render_id) {
                        node.set_parent(Some(parent_id));
                    }
                    if let Some(parent_node) = render_tree.get_mut(parent_id) {
                        parent_node.add_child(render_id);
                    }
                }

                render_id
            };

            self.render_id = Some(render_id);

            tracing::debug!("RenderBehavior::on_mount created render_id={:?}", render_id);
        } else {
            tracing::warn!("RenderBehavior::on_mount called without PipelineOwner");
        }
    }

    fn on_unmount(&mut self, core: &mut ElementCore<V, A>) {
        // Remove from RenderTree
        if let Some(render_id) = self.render_id {
            if let Some(ref pipeline_owner) = core.pipeline_owner() {
                let mut owner = pipeline_owner.write();
                owner.render_tree_mut().remove(render_id);
                tracing::debug!(
                    "RenderBehavior::on_unmount removed render_id={:?}",
                    render_id
                );
            }
        }

        self.render_id = None;
    }

    fn on_update(&mut self, core: &ElementCore<V, A>) {
        // Mark RenderObject for layout/paint
        if let Some(render_id) = self.render_id {
            if let Some(ref pipeline_owner) = core.pipeline_owner() {
                let mut owner = pipeline_owner.write();
                let tree_depth = owner.render_tree().depth(render_id).unwrap_or(0);

                owner.add_node_needing_layout(render_id.get(), tree_depth as usize);
                owner.add_node_needing_paint(render_id.get(), tree_depth as usize);

                tracing::debug!(
                    "RenderBehavior::on_update marked render_id={:?} dirty",
                    render_id
                );
            }
        }
    }
}

// Helper function for RenderObject insertion
fn insert_render_object_helper<R, P>(render_object: R, owner: &mut PipelineOwner) -> RenderId
where
    R: RenderObjectTrait<P>,
    P: Protocol,
    flui_rendering::storage::RenderNode: From<Box<dyn RenderObjectTrait<P>>>,
{
    owner.insert(Box::new(render_object))
}

// ============================================================================
// InheritedBehavior
// ============================================================================

/// Behavior for InheritedView elements.
///
/// Manages dependents tracking and data caching. Similar to ProxyView but with
/// update_should_notify logic to optimize dependent rebuilds.
#[derive(Debug)]
pub struct InheritedBehavior<V: InheritedView> {
    /// Cached data for dependents.
    pub data: V::Data,
    /// Elements that depend on this InheritedElement.
    pub dependents: Vec<ElementId>,
    /// Marker for view type.
    _phantom: PhantomData<V>,
}

impl<V: InheritedView> InheritedBehavior<V> {
    /// Create a new InheritedBehavior by extracting data from the view.
    pub fn new(view: &V) -> Self {
        Self {
            data: view.data().clone(),
            dependents: Vec::new(),
            _phantom: PhantomData,
        }
    }

    /// Get the provided data.
    pub fn data(&self) -> &V::Data {
        &self.data
    }

    /// Register a dependent element.
    pub fn add_dependent(&mut self, element: ElementId) {
        if !self.dependents.contains(&element) {
            self.dependents.push(element);
        }
    }

    /// Remove a dependent element.
    pub fn remove_dependent(&mut self, element: ElementId) {
        self.dependents.retain(|&id| id != element);
    }

    /// Get all dependent elements.
    pub fn dependents(&self) -> &[ElementId] {
        &self.dependents
    }
}

impl<V, A> ElementBehavior<V, A> for InheritedBehavior<V>
where
    V: InheritedView,
    A: ElementArity,
{
    fn perform_build(&mut self, core: &mut ElementCore<V, A>) {
        if !core.should_build() {
            tracing::trace!("InheritedBehavior::perform_build skipped");
            return;
        }

        tracing::debug!("InheritedBehavior::perform_build starting");

        // Like ProxyView, InheritedView just returns the child directly
        let child_view = core.view().child();
        let child_view_boxed = dyn_clone::clone_box(child_view);
        core.update_or_create_child(child_view_boxed);
        core.clear_dirty();

        tracing::debug!("InheritedBehavior::perform_build completed");
    }

    fn on_update(&mut self, core: &ElementCore<V, A>) {
        // Check if dependents should be notified
        // In a full implementation, we would get the old view and compare
        // For now, we update the cached data
        self.data = core.view().data().clone();

        // TODO: Mark all dependents as needing rebuild if update_should_notify returns true
        // This is handled by BuildOwner in a full implementation
        tracing::debug!("InheritedBehavior::on_update data cached");
    }

    fn on_unmount(&mut self, _core: &mut ElementCore<V, A>) {
        // Clear dependents on unmount
        let count = self.dependents.len();
        self.dependents.clear();
        tracing::debug!("InheritedBehavior::on_unmount cleared {} dependents", count);
    }
}

// ============================================================================
// AnimationBehavior (composes StatefulBehavior with automatic listener)
// ============================================================================

/// Behavior for AnimatedView - automatically subscribes to Listenable changes.
///
/// AnimationBehavior composes StatefulBehavior and adds automatic listener
/// management. When the listenable changes, the element is marked dirty
/// and rebuilt automatically.
///
/// This eliminates the boilerplate of manually subscribing/unsubscribing
/// to animations in every animated widget.
///
/// # Flutter Equivalent
///
/// Corresponds to the `_AnimatedState` implementation that Flutter generates
/// for AnimatedWidget:
///
/// ```dart
/// class _AnimatedState extends State<AnimatedWidget> {
///   @override
///   void initState() {
///     super.initState();
///     widget.listenable.addListener(_handleChange);
///   }
///
///   void _handleChange() {
///     setState(() {});
///   }
///
///   @override
///   void dispose() {
///     widget.listenable.removeListener(_handleChange);
///     super.dispose();
///   }
/// }
/// ```
pub struct AnimationBehavior<V>
where
    V: AnimatedView,
{
    /// Composed StatefulBehavior for state management
    stateful: StatefulBehavior<V>,
    /// Listener ID for cleanup
    listener_id: Option<ListenerId>,
}

impl<V> AnimationBehavior<V>
where
    V: AnimatedView,
{
    /// Create a new AnimationBehavior for the given view.
    pub fn new(view: &V) -> Self {
        Self {
            stateful: StatefulBehavior::new(view),
            listener_id: None,
        }
    }

    /// Get a reference to the state.
    pub fn state(&self) -> &V::State {
        &self.stateful.state
    }

    /// Get a mutable reference to the state.
    pub fn state_mut(&mut self) -> &mut V::State {
        &mut self.stateful.state
    }
}

impl<V> std::fmt::Debug for AnimationBehavior<V>
where
    V: AnimatedView,
    V::State: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimationBehavior")
            .field("state", &self.stateful.state)
            .field("has_listener", &self.listener_id.is_some())
            .finish()
    }
}

impl<V, A> ElementBehavior<V, A> for AnimationBehavior<V>
where
    V: AnimatedView,
    A: ElementArity,
{
    fn perform_build(&mut self, core: &mut ElementCore<V, A>) {
        // Delegate to StatefulBehavior
        self.stateful.perform_build(core);
    }

    fn on_mount(&mut self, core: &mut ElementCore<V, A>) {
        // First, let StatefulBehavior do its setup (initialize state)
        self.stateful.on_mount(core);

        // Then subscribe to the listenable
        let listenable = core.view().listenable();
        let mark_dirty = core.create_mark_dirty_callback();

        self.listener_id = Some(listenable.add_listener(mark_dirty));

        tracing::debug!("AnimationBehavior::on_mount subscribed to listenable");
    }

    fn on_unmount(&mut self, core: &mut ElementCore<V, A>) {
        // Unsubscribe from the listenable
        if let Some(listener_id) = self.listener_id.take() {
            let listenable = core.view().listenable();
            listenable.remove_listener(listener_id);
            tracing::debug!("AnimationBehavior::on_unmount unsubscribed from listenable");
        }

        // Then let StatefulBehavior do its cleanup (dispose state)
        self.stateful.on_unmount(core);
    }

    fn on_update(&mut self, core: &ElementCore<V, A>) {
        // On update, we need to:
        // 1. Unsubscribe from old listenable
        // 2. Subscribe to new listenable
        // 3. Let StatefulBehavior handle state update

        if let Some(listener_id) = self.listener_id.take() {
            let listenable = core.view().listenable();
            listenable.remove_listener(listener_id);
        }

        let listenable = core.view().listenable();
        let mark_dirty = core.create_mark_dirty_callback();
        self.listener_id = Some(listenable.add_listener(mark_dirty));

        self.stateful.on_update(core);

        tracing::debug!("AnimationBehavior::on_update resubscribed to listenable");
    }
}
