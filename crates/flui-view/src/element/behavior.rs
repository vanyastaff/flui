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
    context::{BuildContext, BuildCtx},
    element::RenderSlot,
    view::{
        AnimatedView, InheritedView, IntoView, ProxyView, RenderView, StatefulView, StatelessView,
        View, ViewState,
    },
};

/// The `BuildContext` a behavior hands to user `build()` / lifecycle code.
///
/// During a [`BuildOwner::build_scope`](crate::BuildOwner) drain the owner
/// carries a [`BuildHandle`](crate::ElementOwner) — a borrowed live view of
/// the real tree — so the variant resolves to [`BuildCtx`] and ancestor
/// walks / `depend_on` hit real nodes (PR-K). A component build outside a
/// `build_scope` drain is a framework bug: it would otherwise resurrect the
/// inert `new_minimal` context and make `InheritedView` reads silently fail.
///
/// Holding the concrete value in a local lets the caller hand out a
/// `&dyn BuildContext` whose borrow outlives the user `build()` closure.
pub(crate) enum BuildCtxChoice<'a> {
    /// Live context over the borrowed real tree (inside `build_scope`).
    Live(BuildCtx<'a>),
}

impl BuildCtxChoice<'_> {
    /// Borrow the chosen context as the object-safe trait.
    pub(crate) fn as_ctx(&self) -> &dyn BuildContext {
        match self {
            Self::Live(ctx) => ctx,
        }
    }
}

/// Pick the build context for `core`'s `build()` from `owner`.
///
/// `BuildHandle` is `Copy`, so reading `owner.build_view` lifts the borrowed
/// tree/sink references (lifetime `'a`, tied to the data — not to this `&`
/// borrow of `owner`) out cleanly; the returned `BuildCtxChoice<'a>` does not
/// keep `owner` borrowed, so the caller can still pass `owner` on mutably.
pub(crate) fn make_build_ctx<'a, V, A>(
    core: &ElementCore<V, A>,
    owner: &crate::ElementOwner<'a>,
) -> BuildCtxChoice<'a>
where
    V: Clone + 'static,
    A: ElementArity,
{
    let handle = owner.build_view.expect(
        "component build requires BuildOwner::build_scope live BuildHandle; \
         ElementBuildContext::new_minimal is not a production fallback",
    );
    let element_id = core
        .self_id()
        .expect("component build requires ElementCore::self_id stamped by ElementTree insertion");

    // The live context carries the element's AUTHORITATIVE tree depth
    // (`parent_depth + 1`, from its node), not `ElementCore::depth` — the
    // sibling SLOT index. `BuildContext::depth` is documented as the tree
    // depth, and `depend_on` records a dependent at this depth while
    // `mark_needs_build` schedules a rebuild at it; using the slot would
    // mis-order a nested dependent / rebuild in the dirty heap (the same
    // class of bug `rekey_dirty_depths` corrects for the `setState` path).
    // Falling back to the slot only keeps release builds whole if the tree
    // node vanished despite the stamped id; the preceding `expect`s catch the
    // intended invariants.
    let tree_depth = match handle.tree.get(element_id) {
        Some(node) => node.depth(),
        None => core.depth(),
    };
    // The rebuild capability is minted from the element's own core,
    // which already holds the `ExternalBuildScheduler` installed at mount — the
    // same channel `AnimatedView` rides. `build` must not call `schedule()`;
    // port-check trigger #22 enforces that a handle is not even acquired there.
    // The pipeline owner comes off the core, not off the tree node: `build_scope`
    // has the element *extracted* from its node for the duration of the build
    // (`ElementNode::element` panics in that window), so a `BuildContext` cannot
    // look itself up. `ElementCore` holds the same `Arc` the node would have.
    BuildCtxChoice::Live(BuildCtx::new(
        element_id,
        tree_depth,
        handle.tree,
        handle.dep_sink,
        core.rebuild_handle(),
        crate::context::BuildCapabilities {
            async_driver: owner.async_driver.clone(),
            post_frame_handle: owner.post_frame_handle.clone(),
            pipeline_owner: core.pipeline_owner().map(std::sync::Arc::clone),
        },
    ))
}

// ============================================================================
// ElementBehavior Trait
// ============================================================================

/// Trait for view-specific element behavior.
///
/// This trait encapsulates all the view-type-specific logic that varies between
/// Stateless, Proxy, Stateful, and Render elements. The unified Element<V,A,B>
/// delegates to this trait for view-specific operations.
pub trait ElementBehavior<V, A>: 'static
where
    V: Clone + 'static,
    A: ElementArity,
{
    /// Run this view type's build half and return its OWNED child view(s).
    ///
    /// E3 (atomic box→arena swap): this replaces the old
    /// `perform_build(&mut core, owner)` that reconciled the box-owned
    /// children in place. Now it runs only the build half (today's
    /// `build_or_recover` / `build_proxy_style` producer) and returns the
    /// owned child views; the reconcile half is hoisted to
    /// [`BuildOwner::build_scope`](crate::BuildOwner), which feeds the
    /// returned views to the slab id-reconciler with a fresh `&mut tree`.
    /// The element holds no child storage, so this method never touches a
    /// child graph and there is no double-borrow with the slab.
    ///
    /// The split-borrow `owner` handle is threaded through so the build
    /// half can register `GlobalKey`s, schedule rebuilds, etc.
    fn build_into_views(
        &mut self,
        core: &mut ElementCore<V, A>,
        owner: &mut crate::ElementOwner<'_>,
    ) -> Vec<Box<dyn View>>;

    /// Called after mount to perform behavior-specific setup.
    ///
    /// The split-borrow `owner` handle is threaded through so behaviors can
    /// register themselves in `BuildOwner` registries. (The O(1) inherited
    /// lookup is NOT wired here — it is built structurally by
    /// [`ElementTree::insert`](crate::tree::ElementTree) into each node's
    /// `inherited` map, so `InheritedBehavior` needs no mount-time hook.)
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
    fn on_update(&mut self, core: &ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {}

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
    /// descendants.
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
    /// `BuildContext::depend_on_inherited` (Flutter
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
    /// [`BuildContext::find_ancestor_state`] to surface
    /// the typed `ViewState` to the dispatch boundary without naming
    /// `V` at the object-safe trait surface.
    ///
    /// Flutter parity: `framework.dart:5132`
    /// `findAncestorStateOfType<T>` reads `element.state` after a
    /// runtime-type check.
    fn state_as_any(&self) -> Option<&dyn std::any::Any> {
        None
    }

    /// Return the `RenderId` of this behavior's `RenderObject` if it has
    /// one — i.e. if this is `RenderBehavior<V>` and `on_mount` ran with
    /// a `PipelineOwner` in scope.
    ///
    /// Default returns `None`; only `RenderBehavior<V>` overrides this
    /// (`AnimatedBehavior` composes `StatefulBehavior`, not `RenderBehavior`,
    /// so it keeps the default). Used by
    /// [`BuildContext::find_render_object`] to surface
    /// the nearest ancestor's `RenderId` to the dispatch boundary.
    ///
    /// Flutter parity: `framework.dart:5160`
    /// `findAncestorRenderObjectOfType<T>` reads
    /// `(ancestor as RenderObjectElement).renderObject` after a
    /// runtime-type check.
    fn render_id(&self) -> Option<flui_foundation::RenderId> {
        None
    }

    /// The parent-data this element contributes to its render child's nearest
    /// render parent, if it is a `ParentDataView` (Flexible / Positioned).
    ///
    /// Default `None`; `ParentDataBehavior` overrides it. Read at the
    /// `ElementTree` insert/update seams (`apply_ancestor_parent_data`) — the
    /// port of Flutter's `RenderObjectElement.attachRenderObject` →
    /// `_findAncestorParentDataElements` → `ParentDataWidget.applyParentData`.
    #[allow(unused_variables)]
    fn parent_data_config(
        &self,
        core: &ElementCore<V, A>,
    ) -> Option<Box<dyn flui_rendering::parent_data::ParentData>> {
        None
    }

    /// Object-safe notification handler hook routed from
    /// [`ElementBase::on_notification`](crate::view::ElementBase::on_notification)
    /// during bubble dispatch.
    ///
    /// Default returns `false` — non-listener behaviors are skipped
    /// cleanly. A future production `NotificationListener<N>` widget will
    /// override this in a dedicated `NotificationListenerBehavior<N>`
    /// (out of scope for now — the integration tests in
    /// `tests/notifications.rs` exercise the protocol via a hand-rolled
    /// `ElementBase` impl so the wiring is validated end-to-end without
    /// adding production scaffolding the framework doesn't yet need).
    ///
    /// Flutter parity: `notification_listener.dart:127`
    /// (`_NotificationElement.onNotification`).
    fn on_notification(&self, type_id: std::any::TypeId, notification: &dyn std::any::Any) -> bool {
        let _ = (type_id, notification);
        false
    }

    /// Fire the typed `did_change_dependencies` lifecycle hook for any
    /// state this behavior owns.
    ///
    /// Routed from
    /// [`ElementBase::notify_dependency_change`](crate::view::ElementBase::notify_dependency_change)
    /// by `BuildOwner::build_scope` immediately before the dependent's
    /// `perform_build`, when an inherited ancestor's
    /// `update_should_notify` returned `true` since the last build —
    /// Flutter parity for `framework.dart:5977-5982`
    /// `StatefulElement.performRebuild` reading the
    /// `_didChangeDependencies` flag set at `framework.dart:6117`.
    ///
    /// Default is a no-op — Stateless, Proxy, Inherited, and Render
    /// behaviors own no `ViewState`, so the scheduled rebuild alone
    /// suffices. [`StatefulBehavior`] overrides this to forward to
    /// `ViewState::did_change_dependencies`; [`AnimatedBehavior`]
    /// delegates to the composed `StatefulBehavior`.
    ///
    /// The split-borrow `owner` handle is threaded through so the override
    /// can resolve the same live build-time context (`BuildHandle`) the
    /// rebuild uses, letting a user `did_change_dependencies` re-read the
    /// changed inherited value against the real tree (PR-K).
    #[allow(unused_variables)]
    fn did_change_dependencies(
        &mut self,
        core: &ElementCore<V, A>,
        owner: &mut crate::ElementOwner<'_>,
    ) {
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

    fn build_into_views(
        &mut self,
        core: &mut ElementCore<V, A>,
        owner: &mut crate::ElementOwner<'_>,
    ) -> Vec<Box<dyn View>> {
        if !super::behavior_commons::should_build_with_trace(core, "StatelessBehavior") {
            return Vec::new();
        }
        // Live tree-backed context inside a `build_scope` drain (PR-K). Held
        // in a local so the `&dyn BuildContext` outlives the `catch_unwind`
        // closure below; it borrows the tree/sink, not `owner`, so the
        // mutable `owner` reborrow for `build_or_recover` is still free.
        let ctx_choice = make_build_ctx(core, owner);
        let ctx = ctx_choice.as_ctx();
        // The user `build()` is wrapped in `catch_unwind`: a panicking
        // build is caught and substituted with the registered
        // `ErrorView`. The catch covers ONLY the build expression — the
        // `view` borrow is moved into the closure so nothing of `core`
        // is mutated under the catch (Flutter parity:
        // `ComponentElement.performRebuild`, `framework.dart:5810`).
        let view = core.view().clone();
        let child_view =
            super::behavior_commons::build_or_recover(core, owner, "StatelessElement", move || {
                // `view.build(ctx)` returns `impl IntoView` that may
                // capture closure-local borrows of `view`/`ctx` (Rust 2024
                // RPITIT default). We consume the opaque value through
                // `IntoView::into_view()` inside the closure body — the
                // resulting `<R as IntoView>::View` is `'static`, and
                // boxing it produces an owned `Box<dyn View>` with no
                // escaping borrows for `catch_unwind` to return.
                let opaque = view.build(ctx);
                Box::new(IntoView::into_view(opaque)) as Box<dyn View>
            });
        super::behavior_commons::single_child_views(core, child_view, "StatelessBehavior")
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

    fn build_into_views(
        &mut self,
        core: &mut ElementCore<V, A>,
        _owner: &mut crate::ElementOwner<'_>,
    ) -> Vec<Box<dyn View>> {
        super::behavior_commons::proxy_style_views(core, "ProxyBehavior", V::child)
    }
}

// ============================================================================
// ParentDataBehavior
// ============================================================================

/// Behavior for `ParentDataView` elements (Flexible / Expanded / Positioned).
///
/// A transparent single-child proxy: like [`ProxyBehavior`] it owns no render
/// object, so the unified `Element` passes the parent's render id straight
/// through to the wrapped child (whose render object attaches to the nearest
/// ancestor render object). The parent-data the view contributes is surfaced
/// via [`parent_data_config`](ElementBehavior::parent_data_config) and written
/// onto the child render node by the `ElementTree` insert/update seams — the
/// port of Flutter's `RenderObjectElement.attachRenderObject` →
/// `_updateParentData`.
#[derive(Debug, Clone, Copy)]
pub struct ParentDataBehavior;

impl<V, A> ElementBehavior<V, A> for ParentDataBehavior
where
    V: crate::view::ParentDataView,
    A: ElementArity,
{
    fn debug_kind(&self) -> &'static str {
        "ParentDataElement"
    }

    fn build_into_views(
        &mut self,
        core: &mut ElementCore<V, A>,
        _owner: &mut crate::ElementOwner<'_>,
    ) -> Vec<Box<dyn View>> {
        super::behavior_commons::proxy_style_views(core, "ParentDataBehavior", V::child)
    }

    fn parent_data_config(
        &self,
        core: &ElementCore<V, A>,
    ) -> Option<Box<dyn flui_rendering::parent_data::ParentData>> {
        Some(Box::new(core.view().create_parent_data()))
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
    /// can downcast at the dispatch boundary.
    ///
    /// `V::State: 'static` is guaranteed by the [`ViewState`] trait
    /// bound, so the resulting `&dyn Any` has a well-defined `TypeId`
    /// equal to `TypeId::of::<V::State>()`.
    fn state_as_any(&self) -> Option<&dyn std::any::Any> {
        Some(&self.state as &dyn std::any::Any)
    }

    fn build_into_views(
        &mut self,
        core: &mut ElementCore<V, A>,
        owner: &mut crate::ElementOwner<'_>,
    ) -> Vec<Box<dyn View>> {
        // Live tree-backed context inside a `build_scope` drain (PR-K). One
        // context serves both the first-build `init_state` and the `build`
        // below; it borrows the tree/sink, not `owner`/`self`, so the later
        // mutable reborrows stay free.
        let ctx_choice = make_build_ctx(core, owner);
        let ctx = ctx_choice.as_ctx();

        // Initialize state on first build — must run before the
        // `should_build` guard so a freshly-mounted `StatefulView` calls
        // `init_state` exactly once even if the element is clean.
        if !self.initialized {
            self.state.init_state(ctx);
            self.initialized = true;
        }

        if !super::behavior_commons::should_build_with_trace(core, "StatefulBehavior") {
            return Vec::new();
        }
        // The user `ViewState::build` is wrapped in `catch_unwind`: a
        // panicking build is caught and substituted with the registered
        // `ErrorView`. The catch covers ONLY the build expression — the
        // `view` borrow is moved into the closure (cloned) and `state` is
        // captured by reference, independent of `core` (Flutter parity:
        // `ComponentElement.performRebuild`, `framework.dart:5810`).
        let view = core.view().clone();
        let state = &self.state;
        let child_view =
            super::behavior_commons::build_or_recover(core, owner, "StatefulElement", move || {
                // See `StatelessBehavior::build_into_views` for the
                // RPITIT-capture rationale — consume the opaque
                // `impl IntoView` inside the closure body to box an owned
                // `Box<dyn View>` for the `catch_unwind` return.
                let opaque = state.build(&view, ctx);
                Box::new(IntoView::into_view(opaque)) as Box<dyn View>
            });
        super::behavior_commons::single_child_views(core, child_view, "StatefulBehavior")
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
        core: &ElementCore<V, A>,
        old_view: &V,
        _owner: &mut crate::ElementOwner<'_>,
    ) {
        // `core.view()` is already the freshly-swapped-in configuration here —
        // Flutter's `this.widget` at `didUpdateWidget` time.
        self.state.did_update_view(old_view, core.view());
    }

    /// Fire `ViewState::did_change_dependencies` on the owned state.
    ///
    /// Called by `BuildOwner::build_scope` right before this
    /// dependent's `perform_build` when an inherited ancestor's
    /// `update_should_notify` returned true since the last build —
    /// Flutter parity for `framework.dart:5977-5982`.
    ///
    /// `init_state` always runs before any `did_change_dependencies`
    /// dispatch because `state_as_any` reaches the typed
    /// `did_change_dependencies` only via this hook, and the hook can
    /// only fire after the element is `Active` (it's gated on
    /// `lifecycle().can_build()` in `build_scope`). Defunct or already-
    /// inactive lifecycles are skipped by the build-scope check itself,
    /// so we get the defensive-shape Flutter calls for free without a
    /// second guard here.
    fn did_change_dependencies(
        &mut self,
        core: &ElementCore<V, A>,
        owner: &mut crate::ElementOwner<'_>,
    ) {
        // Live tree-backed context (PR-K) when fired from a `build_scope`
        // drain — so a user `did_change_dependencies` that re-reads the
        // changed inherited value via `depend_on` resolves against the real
        // ancestor chain, matching Flutter (`framework.dart:5977-5982` runs
        // the hook with the element's live `BuildContext`).
        let ctx_choice = make_build_ctx(core, owner);
        self.state.did_change_dependencies(ctx_choice.as_ctx());
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

    /// Expose the behavior's `RenderId` (set by `on_mount` once a
    /// `PipelineOwner` is in scope) so [`BuildContext::find_render_object`]
    /// can surface it to the dispatch boundary.
    ///
    /// Returns `None` until `on_mount` has run with a PipelineOwner —
    /// matches Flutter's behavior where `findAncestorRenderObjectOfType`
    /// returns `null` for an unmounted `RenderObjectElement`.
    fn render_id(&self) -> Option<flui_foundation::RenderId> {
        self.render_id
    }

    fn build_into_views(
        &mut self,
        core: &mut ElementCore<V, A>,
        _owner: &mut crate::ElementOwner<'_>,
    ) -> Vec<Box<dyn View>> {
        if !core.should_build() {
            tracing::trace!(
                "RenderBehavior::build_into_views skipped render_id={:?}",
                self.render_id
            );
            return Vec::new();
        }

        tracing::info!(
            "RenderBehavior::build_into_views START render_id={:?}",
            self.render_id
        );

        // Collect the render element's OWNED child views. The
        // RenderObject-attach side-effects already ran in `on_mount`
        // (they touch the pipeline owner / render tree, not the element
        // slab); here we only surface the element-child views for the
        // slab id-reconciler to reconcile in `build_scope`.
        let mut child_views: Vec<Box<dyn View>> = Vec::new();
        if core.view().has_children() {
            core.view().visit_child_views(&mut |child_view| {
                child_views.push(dyn_clone::clone_box(child_view));
            });
        }

        core.clear_dirty();

        tracing::debug!(
            "RenderBehavior::build_into_views completed render_id={:?} children={}",
            self.render_id,
            child_views.len()
        );

        child_views
    }

    fn on_mount(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        // Create RenderObject and insert into RenderTree
        if let Some(pipeline_owner) = core.pipeline_owner() {
            tracing::info!("RenderBehavior::on_mount creating RenderObject");

            let ctx = crate::RenderObjectContext::new(owner.interaction_dispatch.as_ref());
            let render_object = core.view().create_render_object(&ctx);

            let render_id = {
                let mut pipeline_owner = pipeline_owner.write();

                // Use helper to insert (handles Protocol type)
                let render_id = insert_render_object_helper(render_object, &mut pipeline_owner);

                // Handle parent relationship
                if let Some(parent_id) = core.parent_render_id() {
                    let render_tree = pipeline_owner.render_tree_mut();
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

    fn on_unmount(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        if let Some(render_id) = self.render_id
            && let Some(pipeline_owner) = core.pipeline_owner()
        {
            let ctx = crate::RenderObjectContext::new(owner.interaction_dispatch.as_ref());
            let mut pipeline_owner = pipeline_owner.write();
            if let Some(render_object) = pipeline_owner
                .render_tree_mut()
                .get_mut(render_id)
                .and_then(|node| node.downcast_render_object_mut::<V::RenderObject>())
            {
                core.view().did_unmount_render_object(&ctx, render_object);
            }
        }
        super::behavior_commons::remove_render_object_from_tree(
            core,
            self.render_id,
            "RenderBehavior",
        );
        self.render_id = None;
    }

    fn on_update(&mut self, core: &ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        // Apply the widget's new configuration to the *existing* render object
        // before marking it dirty — Flutter's `RenderObjectElement.update` ->
        // `widget.updateRenderObject(context, renderObject)`. Without this the
        // render object keeps its `create_render_object()` configuration and a
        // `setState` that changes a render-object widget (padding, size, text,
        // colour, …) would never be reflected after the first frame.
        if let Some(render_id) = self.render_id
            && let Some(pipeline_owner) = core.pipeline_owner()
        {
            let ctx = crate::RenderObjectContext::new(owner.interaction_dispatch.as_ref());
            let mut pipeline_owner = pipeline_owner.write();
            if let Some(render_object) = pipeline_owner
                .render_tree_mut()
                .get_mut(render_id)
                .and_then(|node| node.downcast_render_object_mut::<V::RenderObject>())
            {
                core.view().update_render_object(&ctx, render_object);
            }
        }

        super::behavior_commons::mark_render_needs_layout_and_paint(
            core,
            self.render_id,
            "RenderBehavior",
        );
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
/// `Object?` value); we will gain that capability if we expand
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
    /// latest call wins). HashMap inherently dedups on key.
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

    fn build_into_views(
        &mut self,
        core: &mut ElementCore<V, A>,
        _owner: &mut crate::ElementOwner<'_>,
    ) -> Vec<Box<dyn View>> {
        // Like ProxyView, InheritedView just returns the child directly.
        super::behavior_commons::proxy_style_views(core, "InheritedBehavior", V::child)
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
                // Flutter parity (`framework.dart:6371-6374`):
                // `notifyDependent` calls `dependent.didChangeDependencies`.
                // We split this across two phases — the set-flag part
                // (`note_dependency_change`) here, and the fire part
                // (`ElementBase::notify_dependency_change`) inside
                // `BuildOwner::build_scope` right before the dependent's
                // `perform_build`. The typed
                // `ViewState::did_change_dependencies` hook fires
                // exactly once per dependency-change-then-rebuild
                // cycle, strictly before the build.
                owner.note_dependency_change(dep_id);
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
// AnimatedBehavior (composes StatefulBehavior with automatic listener)
// ============================================================================

/// Behavior for AnimatedView - automatically subscribes to Listenable changes.
///
/// AnimatedBehavior composes StatefulBehavior and adds automatic listener
/// management. When the listenable changes, the element is marked dirty
/// and rebuilt automatically.
///
/// This eliminates the boilerplate of manually subscribing/unsubscribing
/// to animations in every animated widget.
///
/// # Naming
///
/// Named `AnimatedBehavior` (not `AnimationBehavior`) to follow the
/// `<ViewKind>Behavior` convention (`StatelessBehavior`, `StatefulBehavior`,
/// `ProxyBehavior`, `InheritedBehavior`, `RenderBehavior`) and to
/// disambiguate from the `flui_animation::AnimationBehavior` enum
/// (which describes how an animation behaves when the framework reduces
/// motion — a separate concern from the element-tree behavior here).
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
pub struct AnimatedBehavior<V>
where
    V: AnimatedView,
{
    /// Composed StatefulBehavior for state management
    stateful: StatefulBehavior<V>,
    /// Listener ID for cleanup
    listener_id: Option<ListenerId>,
}

impl<V> AnimatedBehavior<V>
where
    V: AnimatedView,
{
    /// Create a new AnimatedBehavior for the given view.
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

impl<V> std::fmt::Debug for AnimatedBehavior<V>
where
    V: AnimatedView,
    V::State: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedBehavior")
            .field("state", &self.stateful.state)
            .field("has_listener", &self.listener_id.is_some())
            .finish()
    }
}

impl<V, A> ElementBehavior<V, A> for AnimatedBehavior<V>
where
    V: AnimatedView,
    A: ElementArity,
{
    fn debug_kind(&self) -> &'static str {
        "AnimatedElement"
    }

    /// Delegate to the composed `StatefulBehavior` so animated elements
    /// participate in ancestor-state lookups.
    fn state_as_any(&self) -> Option<&dyn std::any::Any> {
        <StatefulBehavior<V> as ElementBehavior<V, A>>::state_as_any(&self.stateful)
    }

    fn build_into_views(
        &mut self,
        core: &mut ElementCore<V, A>,
        owner: &mut crate::ElementOwner<'_>,
    ) -> Vec<Box<dyn View>> {
        // Delegate to StatefulBehavior
        self.stateful.build_into_views(core, owner)
    }

    fn on_mount(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        // First, let StatefulBehavior do its setup (initialize state)
        self.stateful.on_mount(core, owner);

        // Then subscribe to the listenable
        let listenable = core.view().listenable();
        let mark_dirty = core.create_mark_dirty_callback();

        self.listener_id = Some(listenable.add_listener(mark_dirty));

        tracing::debug!("AnimatedBehavior::on_mount subscribed to listenable");
    }

    fn on_unmount(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        // Unsubscribe from the listenable
        if let Some(listener_id) = self.listener_id.take() {
            let listenable = core.view().listenable();
            listenable.remove_listener(listener_id);
            tracing::debug!("AnimatedBehavior::on_unmount unsubscribed from listenable");
        }

        // Then let StatefulBehavior do its cleanup (dispose state)
        self.stateful.on_unmount(core, owner);
    }

    fn on_update(&mut self, core: &ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
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

        self.stateful.on_update(core, owner);

        tracing::debug!("AnimatedBehavior::on_update resubscribed to listenable");
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

    /// Delegate to the composed `StatefulBehavior` so animated
    /// dependents also fire the typed
    /// `ViewState::did_change_dependencies` hook before rebuilding —
    /// Flutter parity (animated widgets in Flutter inherit the
    /// `StatefulElement._didChangeDependencies` flag-and-fire path).
    fn did_change_dependencies(
        &mut self,
        core: &ElementCore<V, A>,
        owner: &mut crate::ElementOwner<'_>,
    ) {
        <StatefulBehavior<V> as ElementBehavior<V, A>>::did_change_dependencies(
            &mut self.stateful,
            core,
            owner,
        );
    }
}
