//! Element behaviors - view-specific build logic.
//!
//! This module defines the ElementBehavior trait and implementations for each
//! view type (Stateless, Proxy, Stateful, Render). Behaviors encapsulate the
//! view-specific logic while the unified Element handles all common operations.

use std::{collections::HashMap, marker::PhantomData};

use flui_foundation::{ElementId, ListenerId, RenderId};
use flui_rendering::{
    pipeline::PipelineOwner, protocol::Protocol, traits::RenderObject as RenderObjectTrait,
};

use super::{arity::ElementArity, generic::ElementCore};
use crate::{
    context::ElementBuildContext,
    element::RenderSlot,
    view::{
        AnimatedView, InheritedView, ProxyView, RenderView, StatefulView, StatelessView, View,
        ViewState,
    },
};

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
    ///
    /// The split-borrow `owner` handle is threaded through so child
    /// elements created during this build can register `GlobalKey`s,
    /// schedule rebuilds, etc. (plan §U8).
    fn perform_build(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>);

    /// Called after mount to perform behavior-specific setup.
    ///
    /// The split-borrow `owner` handle is threaded through so behaviors
    /// can register themselves in `BuildOwner` registries
    /// (`InheritedBehavior::on_mount` uses it to register its `TypeId`
    /// for O(1) `depend_on_inherited` lookups). Plan §U9.
    #[allow(unused_variables)]
    fn on_mount(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {}

    /// Called before unmount to perform behavior-specific cleanup.
    ///
    /// `owner` is threaded through so behaviors can unregister themselves
    /// from `BuildOwner` registries (mirror of `on_mount`).
    #[allow(unused_variables)]
    fn on_unmount(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {}

    /// Called after view update to perform behavior-specific reactions.
    #[allow(unused_variables)]
    fn on_update(&mut self, core: &ElementCore<V, A>) {}

    /// Called after the element is re-activated (re-inserted into the tree).
    ///
    /// Default is a no-op. Behaviors that own user-visible state (e.g.
    /// `StatefulBehavior`) override this to forward to `ViewState::activate`.
    #[allow(unused_variables)]
    fn on_activate(&mut self, core: &mut ElementCore<V, A>) {}

    /// Called before the element is deactivated (temporarily removed from the tree).
    ///
    /// Default is a no-op. Behaviors that own user-visible state (e.g.
    /// `StatefulBehavior`) override this to forward to `ViewState::deactivate`.
    #[allow(unused_variables)]
    fn on_deactivate(&mut self, core: &mut ElementCore<V, A>) {}

    /// Called after the view configuration is replaced, with access to the
    /// previous view value.
    ///
    /// `on_update` already fires for generic post-update reactions; this hook
    /// exists for behaviors that need the prior view (e.g. `StatefulBehavior`
    /// forwarding to `ViewState::did_update_view`, or `InheritedBehavior`
    /// scheduling rebuilds for its dependents when
    /// `update_should_notify(old)` is true — see Flutter
    /// `framework.dart:6414` `InheritedElement.notifyClients`).
    ///
    /// The split-borrow `owner` handle is threaded through so behaviors
    /// can call `ElementOwner::schedule_build_for` for affected
    /// descendants. Plan §U9.
    #[allow(unused_variables)]
    fn on_view_updated(
        &mut self,
        core: &ElementCore<V, A>,
        old_view: &V,
        owner: &mut crate::ElementOwner<'_>,
    ) {
    }

    /// The kind name used when formatting the parent `Element` with `Debug`.
    ///
    /// Defaults to `"Element"`. Behaviors override this so that type aliases
    /// like `StatelessElement` and `StatefulElement` render with a familiar
    /// name in logs and snapshot tests.
    fn debug_kind(&self) -> &'static str {
        "Element"
    }

    /// Cast this behavior as an `InheritedElementAccess` object-safe view.
    ///
    /// Returns `None` for every behavior except
    /// [`InheritedBehavior`], whose override returns `Some(self)`. The
    /// unified `Element::as_inherited` delegates to this so
    /// `BuildContext::depend_on_inherited` (plan §U9, Flutter
    /// `framework.dart:5081`) can record dependents and read the view
    /// without naming `V` at the call site.
    fn as_inherited_access(&self) -> Option<&dyn crate::element::InheritedElementAccess> {
        None
    }

    /// Mutable variant of [`as_inherited_access`].
    ///
    /// [`as_inherited_access`]: ElementBehavior::as_inherited_access
    fn as_inherited_access_mut(
        &mut self,
    ) -> Option<&mut dyn crate::element::InheritedElementAccess> {
        None
    }

    /// Borrow this behavior's persistent `ViewState` as `&dyn Any` if
    /// this is `StatefulBehavior<V>` (the only behavior that owns user-
    /// visible state).
    ///
    /// Default returns `None`; only `StatefulBehavior<V>` overrides this
    /// to hand out `&self.state as &dyn Any`. Used by
    /// [`BuildContext::find_ancestor_state`] (plan §U11, R7) to surface
    /// the typed `ViewState` to the dispatch boundary without naming
    /// `V` at the object-safe trait surface.
    ///
    /// Flutter parity: `framework.dart:5132`
    /// `findAncestorStateOfType<T>` reads `element.state` after a
    /// runtime-type check.
    fn state_as_any(&self) -> Option<&dyn std::any::Any> {
        None
    }
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
    fn debug_kind(&self) -> &'static str {
        "StatelessElement"
    }

    fn perform_build(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        if !core.should_build() {
            tracing::trace!("StatelessBehavior::perform_build skipped");
            return;
        }

        tracing::debug!("StatelessBehavior::perform_build starting");

        let ctx = ElementBuildContext::new_minimal(core.depth());
        let child_view = core.view().build(&ctx);
        core.update_or_create_child(child_view, owner);
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
    fn debug_kind(&self) -> &'static str {
        "ProxyElement"
    }

    fn perform_build(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        if !core.should_build() {
            tracing::trace!("ProxyBehavior::perform_build skipped");
            return;
        }

        tracing::debug!("ProxyBehavior::perform_build starting");

        let child_view = core.view().child();
        let child_view_boxed = dyn_clone::clone_box(child_view);
        core.update_or_create_child(child_view_boxed, owner);
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
    fn debug_kind(&self) -> &'static str {
        "StatefulElement"
    }

    /// Expose the owned `ViewState` (`V::State`) as `&dyn Any` so
    /// [`BuildContext::find_ancestor_state`] / `find_root_ancestor_state`
    /// (plan §U11, R7/R8) can downcast at the dispatch boundary.
    ///
    /// `V::State: 'static` is guaranteed by the [`ViewState`] trait
    /// bound, so the resulting `&dyn Any` has a well-defined `TypeId`
    /// equal to `TypeId::of::<V::State>()`.
    fn state_as_any(&self) -> Option<&dyn std::any::Any> {
        Some(&self.state as &dyn std::any::Any)
    }

    fn perform_build(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
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
        core.update_or_create_child(child_view, owner);
        core.clear_dirty();

        tracing::debug!("StatefulBehavior::perform_build completed");
    }

    fn on_unmount(&mut self, _core: &mut ElementCore<V, A>, _owner: &mut crate::ElementOwner<'_>) {
        self.state.dispose();
    }

    fn on_activate(&mut self, _core: &mut ElementCore<V, A>) {
        self.state.activate();
    }

    fn on_deactivate(&mut self, _core: &mut ElementCore<V, A>) {
        self.state.deactivate();
    }

    fn on_view_updated(
        &mut self,
        _core: &ElementCore<V, A>,
        old_view: &V,
        _owner: &mut crate::ElementOwner<'_>,
    ) {
        self.state.did_update_view(old_view);
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
    fn debug_kind(&self) -> &'static str {
        "RenderObjectElement"
    }

    fn perform_build(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
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

            core.update_or_create_children(child_views, owner);
        }

        core.clear_dirty();

        tracing::debug!(
            "RenderBehavior::perform_build completed render_id={:?}",
            self.render_id
        );
    }

    fn on_mount(&mut self, core: &mut ElementCore<V, A>, _owner: &mut crate::ElementOwner<'_>) {
        // Create RenderObject and insert into RenderTree
        if let Some(pipeline_owner) = core.pipeline_owner() {
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

    fn on_unmount(&mut self, core: &mut ElementCore<V, A>, _owner: &mut crate::ElementOwner<'_>) {
        // Remove from RenderTree
        if let Some(render_id) = self.render_id
            && let Some(pipeline_owner) = core.pipeline_owner()
        {
            let mut owner = pipeline_owner.write();
            owner.render_tree_mut().remove(render_id);
            tracing::debug!(
                "RenderBehavior::on_unmount removed render_id={:?}",
                render_id
            );
        }

        self.render_id = None;
    }

    fn on_update(&mut self, core: &ElementCore<V, A>) {
        // Mark RenderObject for layout/paint
        if let Some(render_id) = self.render_id
            && let Some(pipeline_owner) = core.pipeline_owner()
        {
            let mut owner = pipeline_owner.write();
            let tree_depth = owner.render_tree().depth(render_id).unwrap_or(0);

            owner.add_node_needing_layout(render_id, tree_depth as usize);
            owner.add_node_needing_paint(render_id, tree_depth as usize);

            tracing::debug!(
                "RenderBehavior::on_update marked render_id={:?} dirty",
                render_id
            );
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
/// `update_should_notify` logic to optimize dependent rebuilds.
///
/// # Dependents map
///
/// Stored as `HashMap<ElementId, usize>` — dependent id mapped to its
/// depth in the element tree. The depth is captured at
/// `depend_on_inherited` time and used during `on_view_updated` to call
/// `ElementOwner::schedule_build_for(dep_id, dep_depth)` without an
/// extra tree traversal (the tree is not in scope at `on_view_updated`
/// time because we only see `ElementCore<V, A>`).
///
/// Flutter parity: `framework.dart:6252` `_dependents:
/// HashMap<Element, Object?>` in `InheritedElement`. Flutter uses a
/// HashMap because dependents may attach dependency aspects (the
/// `Object?` value); we will gain that capability in U10 if we expand
/// `aspect` support — for now the value slot holds the depth.
#[derive(Debug)]
pub struct InheritedBehavior<V: InheritedView> {
    /// Cached data for dependents.
    pub data: V::Data,
    /// Cached clone of the inherited view itself.
    ///
    /// `BuildContext::depend_on_inherited` must hand a `&V` to the
    /// caller's typed callback (Flutter's `dependOnInheritedWidgetOfExactType`
    /// returns the widget, not the data). The unified element's view
    /// lives in `ElementCore`, but the
    /// [`InheritedElementAccess`](crate::element::InheritedElementAccess)
    /// trait surface only carries `&mut InheritedBehavior<V>` through
    /// the behavior-trait routing — so we cache a clone of the view
    /// here, refreshed on every `on_view_updated`.
    pub view_cache: V,
    /// Elements that depend on this InheritedElement.
    ///
    /// Maps each dependent's `ElementId` -> its tree depth (captured at
    /// the time `depend_on_inherited` was called). The depth is needed
    /// for `BuildOwner::schedule_build_for(id, depth)` so the rebuild
    /// heap orders dependents correctly without a separate tree walk.
    pub dependents: HashMap<ElementId, usize>,
    /// Marker for view type.
    _phantom: PhantomData<V>,
}

impl<V: InheritedView> InheritedBehavior<V> {
    /// Create a new InheritedBehavior by extracting data + view clone
    /// from the source view.
    pub fn new(view: &V) -> Self {
        Self {
            data: view.data().clone(),
            view_cache: view.clone(),
            dependents: HashMap::new(),
            _phantom: PhantomData,
        }
    }

    /// Get the provided data.
    pub fn data(&self) -> &V::Data {
        &self.data
    }

    /// Register a dependent element with its tree depth.
    ///
    /// Idempotent: re-registering the same `element` overwrites its
    /// stored depth (depths can change across reconciliation, so the
    /// latest call wins). HashMap inherently dedups on key. Plan §U9.
    pub fn add_dependent(&mut self, element: ElementId, depth: usize) {
        self.dependents.insert(element, depth);
    }

    /// Remove a dependent element.
    pub fn remove_dependent(&mut self, element: ElementId) {
        self.dependents.remove(&element);
    }

    /// Get all dependent elements (id -> depth map).
    pub fn dependents(&self) -> &HashMap<ElementId, usize> {
        &self.dependents
    }
}

impl<V> crate::element::InheritedElementAccess for InheritedBehavior<V>
where
    V: InheritedView,
{
    fn view_as_any(&self) -> &dyn std::any::Any {
        // Expose the cached view-clone as `&dyn Any` so the caller's
        // typed downcast (`.downcast_ref::<V>()`) can succeed inside
        // `BuildContextExt::depend_on`.
        &self.view_cache as &dyn std::any::Any
    }

    fn record_dependent(&mut self, dependent: ElementId, depth: usize) {
        self.add_dependent(dependent, depth);
    }
}

impl<V, A> ElementBehavior<V, A> for InheritedBehavior<V>
where
    V: InheritedView,
    A: ElementArity,
{
    fn debug_kind(&self) -> &'static str {
        "InheritedElement"
    }

    fn as_inherited_access(&self) -> Option<&dyn crate::element::InheritedElementAccess> {
        Some(self)
    }

    fn as_inherited_access_mut(
        &mut self,
    ) -> Option<&mut dyn crate::element::InheritedElementAccess> {
        Some(self)
    }

    fn perform_build(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        if !core.should_build() {
            tracing::trace!("InheritedBehavior::perform_build skipped");
            return;
        }

        tracing::debug!("InheritedBehavior::perform_build starting");

        // Like ProxyView, InheritedView just returns the child directly
        let child_view = core.view().child();
        let child_view_boxed = dyn_clone::clone_box(child_view);
        core.update_or_create_child(child_view_boxed, owner);
        core.clear_dirty();

        tracing::debug!("InheritedBehavior::perform_build completed");
    }

    fn on_view_updated(
        &mut self,
        core: &ElementCore<V, A>,
        old_view: &V,
        owner: &mut crate::ElementOwner<'_>,
    ) {
        // Refresh cached data + view-clone each update.
        self.data = core.view().data().clone();
        self.view_cache = core.view().clone();

        // Compare old vs new view; if `update_should_notify` returns
        // true, schedule rebuild for every dependent.
        //
        // Flutter parity: `framework.dart:6414`
        // `InheritedElement.notifyClients(InheritedWidget old)` calls
        // `widget.updateShouldNotify(old)` and on true iterates
        // `_dependents.keys` to enqueue each dependent for build.
        if core.view().update_should_notify(old_view) {
            tracing::debug!(
                "InheritedBehavior::on_view_updated notifying {} dependents",
                self.dependents.len()
            );
            for (&dep_id, &dep_depth) in &self.dependents {
                owner.schedule_build_for(dep_id, dep_depth);
            }
        } else {
            tracing::trace!(
                "InheritedBehavior::on_view_updated no notify (update_should_notify=false)"
            );
        }
    }

    fn on_unmount(&mut self, _core: &mut ElementCore<V, A>, _owner: &mut crate::ElementOwner<'_>) {
        // Clear dependents on unmount; stale ids would otherwise be
        // pushed onto `BuildOwner::dirty_elements` heap on a subsequent
        // view-update.
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
    fn debug_kind(&self) -> &'static str {
        "AnimatedElement"
    }

    /// Delegate to the composed `StatefulBehavior` so animated elements
    /// participate in ancestor-state lookups (plan §U11, R7/R8).
    fn state_as_any(&self) -> Option<&dyn std::any::Any> {
        <StatefulBehavior<V> as ElementBehavior<V, A>>::state_as_any(&self.stateful)
    }

    fn perform_build(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        // Delegate to StatefulBehavior
        self.stateful.perform_build(core, owner);
    }

    fn on_mount(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        // First, let StatefulBehavior do its setup (initialize state)
        self.stateful.on_mount(core, owner);

        // Then subscribe to the listenable
        let listenable = core.view().listenable();
        let mark_dirty = core.create_mark_dirty_callback();

        self.listener_id = Some(listenable.add_listener(mark_dirty));

        tracing::debug!("AnimationBehavior::on_mount subscribed to listenable");
    }

    fn on_unmount(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        // Unsubscribe from the listenable
        if let Some(listener_id) = self.listener_id.take() {
            let listenable = core.view().listenable();
            listenable.remove_listener(listener_id);
            tracing::debug!("AnimationBehavior::on_unmount unsubscribed from listenable");
        }

        // Then let StatefulBehavior do its cleanup (dispose state)
        self.stateful.on_unmount(core, owner);
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

    fn on_activate(&mut self, core: &mut ElementCore<V, A>) {
        self.stateful.on_activate(core);
    }

    fn on_deactivate(&mut self, core: &mut ElementCore<V, A>) {
        self.stateful.on_deactivate(core);
    }

    fn on_view_updated(
        &mut self,
        core: &ElementCore<V, A>,
        old_view: &V,
        owner: &mut crate::ElementOwner<'_>,
    ) {
        self.stateful.on_view_updated(core, old_view, owner);
    }
}
