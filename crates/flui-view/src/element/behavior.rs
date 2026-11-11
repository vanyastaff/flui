//! Element behaviors - view-specific build logic.
//!
//! This module defines the ElementBehavior trait and implementations for each
//! view type (Stateless, Proxy, Stateful, Render). Behaviors encapsulate the
//! view-specific logic while the unified Element handles all common operations.

use flui_rendering::parent_data::SliverSlot;
use std::{
    any::TypeId, collections::HashMap, marker::PhantomData, panic::AssertUnwindSafe, rc::Rc,
};

use flui_foundation::{ElementId, Listenable, RenderId};

use super::{
    arity::ElementArity,
    generic::ElementCore,
    provided::{Provided, Publication},
};
use crate::{
    context::{BuildContext, BuildCtx, LifecycleContext},
    owner::LifecycleHook,
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
    /// Borrow the chosen context as `build` sees it: no presentation
    /// capabilities.
    pub(crate) fn as_ctx(&self) -> &dyn BuildContext {
        match self {
            Self::Live(ctx) => ctx,
        }
    }

    /// Borrow the same context as a lifecycle hook sees it, with the
    /// presentation capabilities of [`LifecycleContext`].
    pub(crate) fn as_lifecycle_ctx(&self) -> &dyn LifecycleContext {
        match self {
            Self::Live(ctx) => ctx,
        }
    }
}

/// Length of the drain's dependency sink before a lifecycle hook runs (`None`
/// outside a drain, where reads are recorded directly as lifecycle reads).
fn lifecycle_sink_len(owner: &crate::ElementOwner<'_>) -> Option<usize> {
    owner.build_view.map(|handle| handle.dep_sink.lock().len())
}

/// Tag every dependency record a lifecycle hook pushed since `start` so the
/// drain keeps it across rebuilds (ADR-0074 §5.5: reset-on-build re-derives
/// only what `build` reads).
fn mark_lifecycle_records(owner: &crate::ElementOwner<'_>, start: Option<usize>) {
    let (Some(handle), Some(start)) = (owner.build_view, start) else {
        return;
    };
    let mut sink = handle.dep_sink.lock();
    for record in sink.iter_mut().skip(start) {
        record.lifecycle = true;
    }
}

/// Runs a state's hook that its element cannot go on without (`init_state`,
/// `did_update_view`, `apply_view`, `did_change_dependencies`, `activate`).
///
/// The hook is user code. Dependency records it pushes are tagged as
/// lifecycle reads. A panic is staged against the element through the armed
/// lifecycle handoff, so the seam that called in substitutes the element
/// (ADR-0287), and then resumes.
pub(super) fn armed_state_hook<V, A>(
    core: &ElementCore<V, A>,
    owner: &mut crate::ElementOwner<'_>,
    hook: LifecycleHook,
    context: &'static str,
    run: impl FnOnce(),
) where
    V: Clone + 'static,
    A: ElementArity,
{
    let sink_start = lifecycle_sink_len(owner);
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: a state hook's panic is recorded against its element, then resumed"
    )]
    let outcome = std::panic::catch_unwind(AssertUnwindSafe(run));
    mark_lifecycle_records(owner, sink_start);
    if let Err(payload) = outcome {
        owner.record_armed_lifecycle_panic(
            core.self_id(),
            core.creation_site(),
            TypeId::of::<V>(),
            hook,
            payload.as_ref(),
            context,
        );
        std::panic::resume_unwind(payload);
    }
}

/// Runs a state's hook on the way out (`deactivate`, `dispose`): a panic is
/// recorded against the one element and goes no further, so the elements
/// after it in the pass still leave.
pub(super) fn contained_state_hook<V, A>(
    core: &ElementCore<V, A>,
    owner: &mut crate::ElementOwner<'_>,
    hook: LifecycleHook,
    context: &'static str,
    run: impl FnOnce(),
) where
    V: Clone + 'static,
    A: ElementArity,
{
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: a leaving state's hook panic is recorded; later elements still leave"
    )]
    if let Err(payload) = std::panic::catch_unwind(AssertUnwindSafe(run)) {
        owner.record_hook_panic(
            core.self_id(),
            core.creation_site(),
            TypeId::of::<V>(),
            hook,
            payload.as_ref(),
            context,
        );
        flui_foundation::panic::discard_payload(payload);
    }
}

/// Run an element's `provide` hook and publish what it wrote (ADR-0322 §5).
///
/// The hook is user code. A panic in it, or in a value's
/// `update_should_notify`, leaves the published values as they were, is
/// recorded against the element and resumes, like a state's `apply_view`.
///
/// The hook is the first part of the element's build: it runs with the
/// reactive build phase armed, so a signal it reads rebuilds the element and
/// a signal it writes is refused, as in `build` (ADR-0074). The phase stays
/// armed for the `build_or_recover_begun` that follows, and is disarmed here
/// when the hook fails.
pub(super) fn publish_provided<V, A>(
    publication: &mut Publication,
    core: &ElementCore<V, A>,
    owner: &mut crate::ElementOwner<'_>,
    provide: impl FnOnce(&mut Provided<'_>),
) where
    V: Clone + 'static,
    A: ElementArity,
{
    super::behavior_commons::begin_reactive_build(core, owner);
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: a provide hook's panic is recorded and the element recovers"
    )]
    let published = std::panic::catch_unwind(AssertUnwindSafe(|| {
        publication.publish(owner, provide);
    }));
    if let Err(payload) = published {
        if let Some(id) = core.self_id() {
            owner.reactive.end_element_build(id, false);
        }
        owner.record_armed_lifecycle_panic(
            core.self_id(),
            core.creation_site(),
            TypeId::of::<V>(),
            LifecycleHook::Provide,
            payload.as_ref(),
            "publishing the element's values",
        );
        std::panic::resume_unwind(payload);
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

    // The live context carries the element's tree depth read from its node,
    // the authority the tree stamps onto `ElementCore::depth` too.
    // `depend_on` records a dependent at this depth and `mark_needs_build`
    // schedules a rebuild at it. Falling back to the element's own copy only
    // keeps release builds whole if the tree node vanished despite the
    // stamped id; the preceding `expect`s catch the intended invariants.
    let tree_depth = match handle.tree.get(element_id) {
        Some(node) => node.depth(),
        None => core.depth(),
    };
    // The rebuild capability is minted from the element's own core,
    // which already holds the `ExternalBuildScheduler` installed at mount — the
    // same channel `AnimatedView` rides. `build` must not call `schedule()`;
    // it receives a `BuildContext`, which has no capability methods, so a
    // handle cannot even be acquired there.
    // The pipeline owner comes off the core, not off the tree node: `build_scope`
    // has the element *extracted* from its node for the duration of the build
    // (`ElementNode::element` panics in that window), so a `BuildContext` cannot
    // look itself up. `ElementCore` holds the same `PipelineCell` the node would
    // have.
    BuildCtxChoice::Live(BuildCtx::new(
        element_id,
        tree_depth,
        handle.tree,
        handle.dep_sink,
        core.rebuild_handle(),
        crate::context::BuildCapabilities {
            focus_manager: std::rc::Rc::clone(owner.focus_manager),
            lifecycle_handle: owner.lifecycle_handle.clone(),
            window_handle: owner.window_handle.clone(),
            async_driver: owner.async_driver.clone(),
            post_frame_handle: owner.post_frame_handle.clone(),
            presentation: owner.capabilities.clone(),
            flush_registry: owner.flush_registry.clone(),
            hit_test_handle: owner.hit_test_handle.clone(),
            pipeline_owner: core.pipeline_owner().cloned(),
            keep_alive: owner.keep_alive.clone(),
            unmount_hooks: owner.unmount_hooks.clone(),
            reads: crate::reactive::ElementReads::new(owner.reactive.clone(), element_id),
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
    #[expect(unused_variables)]
    fn on_mount(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {}

    /// Called before unmount to perform behavior-specific cleanup.
    ///
    /// `owner` is threaded through so behaviors can unregister themselves
    /// from `BuildOwner` registries (mirror of `on_mount`).
    #[expect(unused_variables)]
    fn on_unmount(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {}

    /// Called after view update to perform behavior-specific reactions.
    #[expect(unused_variables)]
    fn on_update(&mut self, core: &ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {}

    /// Called after the element is re-activated (re-inserted into the tree).
    ///
    /// Default is a no-op. Behaviors that own user-visible state (e.g.
    /// `StatefulBehavior`) override this to forward to `ViewState::activate`.
    /// `owner` mirrors `on_deactivate`'s shape (both trace back to the
    /// `ElementBase::{activate, deactivate}` signature they're driven from).
    /// `StatefulBehavior` uses it to stage an owned diagnostic only while a
    /// bounded retake handoff is armed. The immediate retake catch takes that
    /// token, and the recovery path publishes it only after the substitute
    /// commits.
    #[expect(unused_variables)]
    fn on_activate(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {}

    /// Called before the element is deactivated (temporarily removed from the tree).
    ///
    /// Default is a no-op. Behaviors that own user-visible state (e.g.
    /// `StatefulBehavior`) override this to forward to `ViewState::deactivate`.
    /// `owner` lets that override report a contained `deactivate` panic as a
    /// `RecoveredPanic` — the only place in the removal path where the
    /// lifecycle flip (`ElementCore::deactivate`) must stay unconditional
    /// after a user panic, so the catch has to sit here rather than at the
    /// tree level.
    #[expect(unused_variables)]
    fn on_deactivate(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
    }

    /// Called after the view configuration is replaced, with access to the
    /// previous view value.
    ///
    /// `on_update` already fires for generic post-update reactions; this hook
    /// exists for behaviors that need the prior view (e.g. `StatefulBehavior`
    /// forwarding to `ViewState::did_update_view`, or `InheritedBehavior`
    /// scheduling rebuilds for its dependents when
    /// `update_should_notify(old)` is true).
    ///
    /// The split-borrow `owner` handle is threaded through so behaviors
    /// can call `ElementOwner::schedule_build_for` for affected
    /// descendants.
    #[expect(unused_variables)]
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
    /// `BuildContext::depend_on_inherited` can record dependents and read the
    /// view without naming `V` at the call site.
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

    /// Whether the set of types this behavior publishes changed since the
    /// last call. Only a behavior that runs a `provide` hook answers `true`.
    fn take_provided_scope_stale(&mut self) -> bool {
        false
    }

    /// Borrow this behavior's persistent `ViewState` as `&dyn Any` if
    /// this is `StatefulBehavior<V>` (the only behavior that owns user-
    /// visible state).
    ///
    /// Default returns `None`; only `StatefulBehavior<V>` overrides this
    /// to hand out `&self.state as &dyn Any`. Used by
    /// [`BuildContext::find_ancestor_state`] to surface
    /// the typed `ViewState` to the dispatch boundary without naming
    /// `V` at the object-safe trait surface. The caller reads the state
    /// after a runtime-type check.
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
    fn render_id(&self) -> Option<flui_foundation::RenderId> {
        None
    }
    /// Whether this element hosts lazy (sparse) sliver children whose
    /// insert slot is their logical data-source index.
    ///
    /// Only the lazy-sliver adaptor behaviors answer `true`. The slab reads
    /// it through [`ElementBase::child_sliver_slot`] to seed the inherited
    /// slot that [`RenderBehavior::on_mount`] stamps into the child's
    /// `SliverMultiBoxAdaptorParentData` — the moment the child adopts itself.
    ///
    /// [`ElementBase::child_sliver_slot`]: crate::view::ElementBase::child_sliver_slot
    fn hosts_sparse_children(&self) -> bool {
        false
    }

    /// The slot a sparse host hands its child at `logical` — both the logical
    /// index and the child's position in the semantic set.
    ///
    /// Only consulted when [`Self::hosts_sparse_children`] answers `true`. The
    /// default is the 1:1 mapping, which is every delegate FLUI ships: one set
    /// member per logical index. A delegate that materialises non-members —
    /// separators, the shape `ListView.separated` takes — overrides this and
    /// answers [`SliverSlot::unindexed`] for them, so a screen reader counts
    /// the members and nothing else.
    ///
    /// The two halves are minted together here, and travel together from here
    /// on, because the failure they exist to prevent is precisely a semantic
    /// position that drifts from the row it describes.
    fn sliver_slot_for_child(&self, logical: usize) -> SliverSlot {
        SliverSlot::identity(logical)
    }

    /// The parent-data this element contributes to its render child's nearest
    /// render parent, if it is a `ParentDataView` (Flexible / Positioned).
    ///
    /// Default `None`; `ParentDataBehavior` overrides it. Read at the
    /// `ElementTree` insert/update seams (`apply_ancestor_parent_data`), which
    /// find the ancestor parent-data elements and apply their data.
    #[expect(unused_variables)]
    fn parent_data_config(
        &self,
        core: &ElementCore<V, A>,
    ) -> Option<Box<dyn flui_rendering::parent_data::ParentData>> {
        None
    }

    /// Framework dispatch for applying typed parent-data configuration.
    ///
    /// This exists because the unified element is generic over `B` before
    /// `ElementKind::Proxy` erases ordinary proxy and parent-data elements
    /// behind the same trait object. Framework behaviors other than
    /// `ParentDataBehavior` must keep the conservative `NONE` default. The
    /// implementation mutates only configuration-owned fields; the caller
    /// ends its render-node borrow before applying the returned impact to the
    /// render parent. A typed mismatch in `ParentDataBehavior` is a `BUG:`
    /// invariant panic.
    ///
    /// Custom non-parent-data behaviors normally omit this method. The
    /// built-in stateless behavior therefore reports `NONE`:
    ///
    /// ```
    /// use flui_rendering::{RenderUpdateImpact, parent_data::BoxParentData};
    /// use flui_foundation::Single;
    /// use flui_view::{BuildContext, ErrorView, IntoView, StatelessView};
    /// use flui_view::element::{ElementBehavior, ElementCore, StatelessBehavior};
    ///
    /// #[derive(Clone)]
    /// struct OrdinaryView;
    ///
    /// impl StatelessView for OrdinaryView {
    ///     fn build(&self, _context: &dyn BuildContext) -> impl IntoView {
    ///         ErrorView::new("ordinary child")
    ///     }
    /// }
    ///
    /// let behavior = StatelessBehavior::new();
    /// let core = ElementCore::<OrdinaryView, Single>::new(OrdinaryView);
    /// let mut parent_data = BoxParentData::default();
    /// let impact = behavior.apply_parent_data_config(&core, &mut parent_data);
    /// assert_eq!(impact, RenderUpdateImpact::NONE);
    /// ```
    fn apply_parent_data_config(
        &self,
        _core: &ElementCore<V, A>,
        _parent_data: &mut dyn flui_rendering::parent_data::ParentData,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }

    /// [`TypeId`] of the parent-data storage this element
    /// contributes, if it is a [`ParentDataView`](crate::view::ParentDataView).
    ///
    /// Default `None`. Used by `ElementTree::apply_ancestor_parent_data` to
    /// validate compatibility with the nearest render parent before writing.
    #[expect(unused_variables)]
    fn parent_data_type_id(&self, core: &ElementCore<V, A>) -> Option<std::any::TypeId> {
        None
    }

    /// Short diagnostic name of the contributing `ParentDataView`.
    #[expect(unused_variables)]
    fn parent_data_debug_type_name(&self, core: &ElementCore<V, A>) -> Option<&'static str> {
        None
    }

    /// Description of legal ancestors for the contributing `ParentDataView`.
    #[expect(unused_variables)]
    fn parent_data_typical_ancestor_description(
        &self,
        core: &ElementCore<V, A>,
    ) -> Option<&'static str> {
        None
    }

    /// Fully-qualified name of the contributed parent-data storage type.
    #[expect(unused_variables)]
    fn parent_data_storage_type_name(&self, core: &ElementCore<V, A>) -> Option<&'static str> {
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
    /// `update_should_notify` returned `true` since the last build.
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
    #[expect(unused_variables)]
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
#[derive(Debug, Default)]
pub struct StatelessBehavior {
    /// The values the view publishes to its subtree.
    publication: Publication,
}

impl StatelessBehavior {
    /// Create a new StatelessBehavior.
    pub fn new() -> Self {
        Self::default()
    }
}

/// The hooks a behavior that publishes values shares: the access face, the
/// stale-scope flag and the release at unmount.
macro_rules! publication_hooks {
    () => {
        fn as_inherited_access(&self) -> Option<&dyn crate::element::InheritedElementAccess> {
            self.publication.access()
        }

        fn as_inherited_access_mut(
            &mut self,
        ) -> Option<&mut dyn crate::element::InheritedElementAccess> {
            self.publication.access_mut()
        }

        fn take_provided_scope_stale(&mut self) -> bool {
            self.publication.take_scope_stale()
        }
    };
}

impl<V, A> ElementBehavior<V, A> for StatelessBehavior
where
    V: StatelessView,
    A: ElementArity,
{
    fn debug_kind(&self) -> &'static str {
        "StatelessElement"
    }

    publication_hooks!();

    fn on_unmount(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        self.publication.release(core.self_id(), owner);
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
        // One clone of the view serves `provide` and `build`, taken before the
        // reactive phase is armed: between the two hooks nothing the author
        // wrote runs (no `Clone`, no `Drop`), so the phase `publish_provided`
        // arms is always handed to `build_or_recover_begun`. The children
        // this build returns mount under what is published here.
        let view = core.view().clone();
        publish_provided(&mut self.publication, core, owner, |values| {
            view.provide(ctx, values);
        });
        // The user `build()` is wrapped in `catch_unwind`: a panicking
        // build is caught and substituted with the registered
        // `ErrorView`. The catch covers ONLY the build expression — the
        // `view` borrow is moved into the closure so nothing of `core`
        // is mutated under the catch.
        let child_view = super::behavior_commons::build_or_recover_begun(
            core,
            owner,
            "StatelessElement",
            move || {
                // `view.build(ctx)` returns `impl IntoView` that may
                // capture closure-local borrows of `view`/`ctx` (Rust 2024
                // RPITIT default). We consume the opaque value through
                // `IntoView::into_view()` inside the closure body — the
                // resulting `<R as IntoView>::View` is `'static`, and
                // boxing it produces an owned `Box<dyn View>` with no
                // escaping borrows for `catch_unwind` to return.
                let opaque = view.build(ctx);
                Box::new(IntoView::into_view(opaque)) as Box<dyn View>
            },
        );
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
/// onto the child render node by the `ElementTree` insert/update seams.
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

    fn apply_parent_data_config(
        &self,
        core: &ElementCore<V, A>,
        parent_data: &mut dyn flui_rendering::parent_data::ParentData,
    ) -> flui_rendering::RenderUpdateImpact {
        let typed = parent_data.downcast_mut::<V::ParentData>().unwrap_or_else(|| {
            panic!(
                "Incorrect use of ParentDataView `{}`: existing child parent-data type does not \
                 match `{}`. `{}` must be placed under {}.",
                core.view().debug_type_name(),
                core.view().parent_data_type_name(),
                core.view().debug_type_name(),
                core.view().typical_ancestor_description(),
            )
        });
        core.view().apply_parent_data(typed)
    }

    fn parent_data_type_id(&self, _core: &ElementCore<V, A>) -> Option<std::any::TypeId> {
        Some(std::any::TypeId::of::<V::ParentData>())
    }

    fn parent_data_debug_type_name(&self, core: &ElementCore<V, A>) -> Option<&'static str> {
        Some(core.view().debug_type_name())
    }

    fn parent_data_typical_ancestor_description(
        &self,
        core: &ElementCore<V, A>,
    ) -> Option<&'static str> {
        Some(core.view().typical_ancestor_description())
    }

    fn parent_data_storage_type_name(&self, core: &ElementCore<V, A>) -> Option<&'static str> {
        Some(core.view().parent_data_type_name())
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
    /// The view `create_state` saw, held while a parent updates this element
    /// before its first build. `did_update_view` then runs right after
    /// `init_state`, so a state never observes an update before its
    /// initialization and still learns of every configuration change.
    pending_update: Option<V>,
    /// The view or the dependencies changed since `ViewState::apply_view`
    /// last ran; the next build calls it first.
    apply_pending: bool,
    /// The values the state publishes to its subtree.
    publication: Publication,
}

impl<V: StatefulView> StatefulBehavior<V> {
    /// Create a new StatefulBehavior by creating the state from the view.
    pub fn new(view: &V) -> Self {
        Self {
            state: view.create_state(),
            initialized: false,
            pending_update: None,
            apply_pending: true,
            publication: Publication::default(),
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

    publication_hooks!();

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
            let state = &mut self.state;
            armed_state_hook(
                core,
                owner,
                LifecycleHook::InitState,
                "initializing StatefulElement",
                || state.init_state(ctx_choice.as_lifecycle_ctx()),
            );
            self.initialized = true;
            if let Some(old_view) = self.pending_update.take() {
                let state = &mut self.state;
                armed_state_hook(
                    core,
                    owner,
                    LifecycleHook::Update,
                    "updating StatefulElement after its init_state",
                    || state.did_update_view(&old_view, core.view()),
                );
            }
        }

        if !super::behavior_commons::should_build_with_trace(core, "StatefulBehavior") {
            return Vec::new();
        }
        if self.apply_pending {
            let state = &mut self.state;
            armed_state_hook(
                core,
                owner,
                LifecycleHook::ApplyView,
                "applying the view to StatefulElement",
                || state.apply_view(core.view(), ctx_choice.as_ctx()),
            );
            self.apply_pending = false;
        }
        // One clone of the view serves `provide` and `build`, taken before the
        // reactive phase is armed: between the two hooks nothing the author
        // wrote runs (no `Clone`, no `Drop`), so the phase `publish_provided`
        // arms is always handed to `build_or_recover_begun`. The children
        // this build returns mount under what is published here.
        let view = core.view().clone();
        let state = &self.state;
        publish_provided(&mut self.publication, core, owner, |values| {
            state.provide(&view, ctx, values);
        });
        // The user `ViewState::build` is wrapped in `catch_unwind`: a
        // panicking build is caught and substituted with the registered
        // `ErrorView`. The catch covers ONLY the build expression — the
        // `view` borrow is moved into the closure (cloned) and `state` is
        // captured by reference, independent of `core`.
        let child_view = super::behavior_commons::build_or_recover_begun(
            core,
            owner,
            "StatefulElement",
            move || {
                // See `StatelessBehavior::build_into_views` for the
                // RPITIT-capture rationale — consume the opaque
                // `impl IntoView` inside the closure body to box an owned
                // `Box<dyn View>` for the `catch_unwind` return.
                let opaque = state.build(&view, ctx);
                Box::new(IntoView::into_view(opaque)) as Box<dyn View>
            },
        );
        super::behavior_commons::single_child_views(core, child_view, "StatefulBehavior")
    }

    /// Run `ViewState::dispose` under a containment catch so a user panic
    /// there cannot escape `finalize_tree` and abort the rest of the
    /// per-presentation frame.
    ///
    /// # The `initialized` gate
    ///
    /// FLUI's split mount/build means `init_state` does not run synchronously
    /// at mount, so a state whose `init_state` never ran — it panicked, or the
    /// element was removed before its first `build_scope` drain ever reached
    /// it — has never acquired whatever `dispose` would release, so `dispose`
    /// must not run for it either; `create_state` must not itself acquire
    /// lifecycle resources, so nothing leaks by skipping it here.
    /// `self.initialized` is set `true` only after `init_state` *returns*
    /// (see `build_into_views` above), so a panicking `init_state` leaves
    /// it `false` and this guard holds.
    ///
    /// # Containment
    ///
    /// Only `state.dispose()` runs inside the catch — nothing else about
    /// unmount moves. `Element::unmount` (`unified.rs`) always calls
    /// `core.unmount(owner)` right after this returns, so a contained
    /// panic here still ends with the tree-side teardown running and the
    /// caller (`ElementTree::remove_finalized_inner`) still freeing the
    /// slab slot and bumping its generation — a `dispose` that panicked
    /// once is never invoked again for the same element. A caught panic
    /// is recorded through `ElementOwner::record_hook_panic` instead of
    /// only reaching `tracing`, mirroring `build_or_recover`'s producer
    /// side.
    ///
    /// The per-element catch contains the damage to the one element whose
    /// `dispose` panicked, rather than leaving every later element in the
    /// finalize pass un-unmounted.
    fn on_unmount(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        self.publication.release(core.self_id(), owner);
        if !self.initialized {
            return;
        }
        let state = &mut self.state;
        contained_state_hook(
            core,
            owner,
            LifecycleHook::Dispose,
            "disposing StatefulElement",
            || state.dispose(),
        );
    }

    /// Run `ViewState::activate` under the same containment shape as
    /// [`Self::on_unmount`]/[`Self::on_deactivate`].
    ///
    /// # The `initialized` gate
    ///
    /// Symmetric with `dispose`/`deactivate`: a state whose `init_state`
    /// never ran has nothing to resume either.
    ///
    /// # Why this needs its own catch, not just the retake window's
    ///
    /// A `GlobalKey` retake wraps `activate_subtree`, but that window walks
    /// the WHOLE reactivated subtree. A panic caught only there can name the
    /// retake candidate, never the actual descendant whose `activate`
    /// failed. While that bounded retake has armed its transient handoff,
    /// catching here stages an owned diagnostic for the exact element before
    /// re-raising the unwind. The retake's immediate catch takes and carries
    /// that token while recovery removes the failed retake and mounts its
    /// substitute; the public recovery record is published only after that
    /// mount commits, with no second, coarser publication. A direct,
    /// unbounded `ElementTree::activate` call
    /// does not arm the handoff, so its panic propagates without being
    /// reported as recovered.
    fn on_activate(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        if !self.initialized {
            return;
        }
        let state = &mut self.state;
        armed_state_hook(
            core,
            owner,
            LifecycleHook::Activate,
            "activating StatefulElement",
            || state.activate(),
        );
    }

    /// Runs `ViewState::deactivate` under a panic boundary with a narrow
    /// blast radius: only the one element whose `deactivate` panicked is
    /// affected, it stays parked `Inactive` (not a whole-subtree failed
    /// state), and it is still disposed normally at `finalize_tree`. A panic
    /// is caught and reported instead of unwinding out of the reconcile that
    /// dropped this element, mirroring `on_unmount`'s containment shape one
    /// hook over — see
    /// `crates/flui-view/tests/lifecycle_panic_containment.rs`.
    ///
    /// The catch sits in the behavior, not around `ElementBase::deactivate`
    /// at the tree level: `unified.rs`'s `deactivate` still runs
    /// `self.core.deactivate()` (the lifecycle flip to `Inactive`) right
    /// after this call returns. A tree-level catch around the whole call
    /// would skip that flip on a panic and hand a same-frame `GlobalKey`
    /// retake an element still reporting `Active`.
    fn on_deactivate(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        if !self.initialized {
            return;
        }
        let state = &mut self.state;
        contained_state_hook(
            core,
            owner,
            LifecycleHook::Deactivate,
            "deactivating StatefulElement",
            || state.deactivate(),
        );
    }

    fn on_view_updated(
        &mut self,
        core: &ElementCore<V, A>,
        old_view: &V,
        _owner: &mut crate::ElementOwner<'_>,
    ) {
        // A parent can rebuild this element between its mount and its first
        // build (`init_state` runs in the build drain, not at mount). Keep the
        // view the state was created from; the first build delivers the update.
        self.apply_pending = true;
        if !self.initialized {
            if self.pending_update.is_none() {
                self.pending_update = Some(old_view.clone());
            }
            return;
        }
        // `core.view()` is already the freshly-swapped-in configuration here.
        self.state.did_update_view(old_view, core.view());
    }

    /// Fire `ViewState::did_change_dependencies` on the owned state.
    ///
    /// Called by `BuildOwner::build_scope` right before this
    /// dependent's `perform_build` when an inherited ancestor's
    /// `update_should_notify` returned true since the last build.
    ///
    /// `init_state` always runs before any `did_change_dependencies`
    /// dispatch because `state_as_any` reaches the typed
    /// `did_change_dependencies` only via this hook, and the hook can
    /// only fire after the element is `Active` (it's gated on
    /// `lifecycle().can_build()` in `build_scope`). Defunct or already-
    /// inactive lifecycles are skipped by the build-scope check itself,
    /// so no second guard is needed here.
    fn did_change_dependencies(
        &mut self,
        core: &ElementCore<V, A>,
        owner: &mut crate::ElementOwner<'_>,
    ) {
        // Live tree-backed context when fired from a `build_scope`
        // drain — so a user `did_change_dependencies` that re-reads the
        // changed inherited value via `depend_on` resolves against the real
        // ancestor chain.
        self.apply_pending = true;
        let ctx_choice = make_build_ctx(core, owner);
        let state = &mut self.state;
        armed_state_hook(
            core,
            owner,
            LifecycleHook::DidChangeDependencies,
            "notifying StatefulElement dependency change",
            || state.did_change_dependencies(ctx_choice.as_lifecycle_ctx()),
        );
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
    /// Marker for RenderObject type.
    _phantom: PhantomData<V::RenderObject>,
}

impl<V: RenderView> RenderBehavior<V> {
    /// Create a new RenderBehavior.
    pub fn new() -> Self {
        Self {
            render_id: None,
            _phantom: PhantomData,
        }
    }

    /// Get the RenderObject ID if created.
    pub fn render_id(&self) -> Option<RenderId> {
        self.render_id
    }
}

impl<V: RenderView> Default for RenderBehavior<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: RenderView> RenderBehavior<V> {
    /// Push the current view's configuration onto the live render object
    /// through `RenderView::update_render_object` (`render_custody::push`).
    ///
    /// It runs from `build_into_views`: a render element that took a new view
    /// (the dispatch marks it dirty, and every update caller schedules a dirty
    /// child) and one an inherited provider notified both reach it, so the hook
    /// runs once per rebuild. The two behaviors that build their child
    /// themselves (`LayoutBuilder`, the persistent headers) call it from
    /// `on_update` instead, with a tree-less context.
    pub(crate) fn push_config<A>(
        &mut self,
        core: &ElementCore<V, A>,
        owner: &mut crate::ElementOwner<'_>,
    ) where
        A: ElementArity,
    {
        super::render_custody::push(
            self.render_id,
            core,
            owner,
            |ctx, render_object: &mut V::RenderObject| {
                core.view().update_render_object(ctx, render_object)
            },
        );
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
    /// Returns `None` until `on_mount` has run with a PipelineOwner, so an
    /// unmounted render element is never found as an ancestor.
    fn render_id(&self) -> Option<flui_foundation::RenderId> {
        self.render_id
    }

    fn build_into_views(
        &mut self,
        core: &mut ElementCore<V, A>,
        owner: &mut crate::ElementOwner<'_>,
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

        // Every rebuild pushes the configuration: a swapped view (`on_update`
        // pushes nothing) and an inherited-data change both arrive here, with
        // the tree borrowed so `update_render_object`'s `depend_on` resolves.
        self.push_config(core, owner);

        core.clear_dirty();

        tracing::debug!(
            "RenderBehavior::build_into_views completed render_id={:?} children={}",
            self.render_id,
            child_views.len()
        );

        child_views
    }

    fn on_mount(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        super::render_custody::mount(&mut self.render_id, core, owner, |ctx| {
            core.view().create_render_object(ctx)
        });
    }

    /// Runs `RenderView::did_unmount_render_object` contained, then removes
    /// the render object: a panicking hook never skips the removal
    /// (`render_custody::unmount`). ADR-0048 (the frame transaction boundary)
    /// is where the accounting for the removal-path seams lives.
    fn on_unmount(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        super::render_custody::unmount(
            &mut self.render_id,
            core,
            owner,
            "RenderBehavior",
            |ctx, render_object: &mut V::RenderObject| {
                core.view().did_unmount_render_object(ctx, render_object);
            },
        );
    }

    /// A view swap pushes nothing here. The dispatch that swapped the view
    /// marked the element dirty, and every caller of an element update
    /// schedules a dirty child (the dense and sparse reconcilers, and the
    /// `GlobalKey` retakes through the reconcile result), so `build_into_views`
    /// runs in the same drain and pushes the configuration once, with the tree
    /// borrowed so that `RenderObjectContext::depend_on` resolves. Pushing a
    /// tree-less configuration here as well would publish a stand-in value
    /// (`Ltr` for a widget under `Rtl`) and invalidate twice. A view equal to
    /// the held one (`View::should_skip_rebuild`) leaves the element clean: its
    /// configuration did not change, and an inherited change reaches it as a
    /// dependency rebuild.
    fn on_update(&mut self, _core: &ElementCore<V, A>, _owner: &mut crate::ElementOwner<'_>) {}
}

// ============================================================================
// InheritedBehavior
// ============================================================================

pub use super::provided::DependentEntry;
use super::provided::Dependents;

/// Behavior for InheritedView elements.
///
/// Publishes the view itself to the subtree and keeps the ledger of the
/// elements that read it. Similar to ProxyView but with `update_should_notify`
/// logic to optimize dependent rebuilds.
///
/// # Dependents
///
/// Each dependent is recorded with its depth in the element tree and the
/// provider fields it read ([`FieldSet`](crate::view::FieldSet), #1090). The
/// depth is captured at `depend_on_inherited` time and used during
/// `on_view_updated` to call `ElementOwner::schedule_build_for` with a typed
/// rebuild reason, without an extra tree traversal (the tree is not in scope
/// at `on_view_updated` time because we only see `ElementCore<V, A>`).
#[derive(Debug)]
pub struct InheritedBehavior<V: InheritedView> {
    /// Cached data for dependents.
    pub data: V::Data,
    /// Cached clone of the inherited view itself.
    ///
    /// `BuildContext::depend_on_inherited` must hand a `&V` to the
    /// caller's typed callback (the view itself, not just its data). The
    /// unified element's view lives in `ElementCore`, but the
    /// [`InheritedElementAccess`](crate::element::InheritedElementAccess)
    /// trait surface only carries `&mut InheritedBehavior<V>` through
    /// the behavior-trait routing — so we cache a clone of the view
    /// here, refreshed on every `on_view_updated`.
    pub view_cache: V,
    /// Elements that depend on this InheritedElement.
    dependents: Dependents,
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
            dependents: Dependents::default(),
            _phantom: PhantomData,
        }
    }

    /// Get the provided data.
    pub fn data(&self) -> &V::Data {
        &self.data
    }

    /// Get all dependent elements (id -> depth + fields read).
    pub fn dependents(&self) -> &HashMap<ElementId, DependentEntry> {
        self.dependents.map()
    }
}

impl<V> crate::element::InheritedElementAccess for InheritedBehavior<V>
where
    V: InheritedView,
{
    fn provided_types(&self, visit: &mut dyn FnMut(TypeId)) {
        visit(TypeId::of::<V>());
    }

    fn provided(&self, type_id: TypeId) -> Option<crate::element::ProvidedRef<'_>> {
        // The cached view-clone is the value: the caller's typed downcast
        // (`.downcast_ref::<V>()`) succeeds inside `BuildContextExt::depend_on`.
        (type_id == TypeId::of::<V>()).then_some(crate::element::ProvidedRef {
            value: &self.view_cache as &dyn std::any::Any,
            field: None,
        })
    }

    fn dependent_count(&self) -> usize {
        self.dependents.len()
    }

    fn record_dependent(
        &mut self,
        _token: crate::context::CrateToken,
        dependent: ElementId,
        depth: usize,
        mask: crate::view::FieldSet,
    ) {
        self.dependents.add(dependent, depth, mask);
    }

    fn record_lifecycle_dependent(
        &mut self,
        _token: crate::context::CrateToken,
        dependent: ElementId,
        depth: usize,
        mask: crate::view::FieldSet,
    ) {
        self.dependents.add_lifecycle(dependent, depth, mask);
    }

    fn reset_dependent_mask(&mut self, _token: crate::context::CrateToken, dependent: ElementId) {
        self.dependents.reset_mask(dependent);
    }

    fn prune_unread_dependent(
        &mut self,
        _token: crate::context::CrateToken,
        dependent: ElementId,
    ) -> bool {
        self.dependents.prune_unread(dependent)
    }

    fn remove_dependent(&mut self, _token: crate::context::CrateToken, dependent: ElementId) {
        self.dependents.remove(dependent);
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

        // Field-granular (#1090): the provider reports WHICH fields changed
        // (`ALL` for a provider that never opted in — the
        // `update_should_notify` default), and only dependents whose recorded
        // mask intersects are scheduled. One path for both granularities.
        let changed = core.view().changed_fields(old_view).erase();
        if changed.is_empty() {
            tracing::trace!("InheritedBehavior::on_view_updated no notify (no field changed)");
        } else {
            tracing::debug!(
                changed = changed.bits(),
                "InheritedBehavior::on_view_updated notifying dependents of {} candidates",
                self.dependents.len()
            );
            self.dependents.notify(changed, owner);
        }
    }

    fn on_unmount(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        // Release the reverse half before dropping the provider's forward
        // notification map. Normal subtree teardown already removed every
        // dependent during its own lifecycle transition; this also covers a
        // provider being torn down independently and keeps the two indexes
        // symmetric.
        let count = self.dependents.len();
        self.dependents.release(core.self_id(), owner);
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
pub struct AnimatedBehavior<V>
where
    V: AnimatedView,
{
    /// Composed StatefulBehavior for state management
    stateful: StatefulBehavior<V>,
    /// The listenable this element is currently subscribed to, cached
    /// together with the owned `Subscription` the listen handed back — set at
    /// subscribe time (`on_mount`, and again on a swap in
    /// `on_view_updated`).
    ///
    /// # Why cache instead of re-reading `listenable()`
    ///
    /// `on_unmount` detaches the subscription without calling
    /// `core.view().listenable()` again. `listenable()` is the
    /// user's only handle to the thing being cleaned up, so a
    /// `catch_unwind` around a *second* call to it at unmount cannot make
    /// removal safe: if that call panicked, there is nothing left to
    /// remove the listener FROM — "the listener is removed regardless"
    /// would be unreachable. Caching closes the seam instead of catching
    /// it: the hook is no longer on the removal path at all, so a
    /// `listenable()` that panics on every call after mount cannot make
    /// unmount panic too.
    subscribed: Option<(std::rc::Rc<dyn Listenable>, flui_foundation::Subscription)>,
}

impl<V> AnimatedBehavior<V>
where
    V: AnimatedView,
{
    /// Create a new AnimatedBehavior for the given view.
    pub fn new(view: &V) -> Self {
        Self {
            stateful: StatefulBehavior::new(view),
            subscribed: None,
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
            .field("has_listener", &self.subscribed.is_some())
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

    fn as_inherited_access(&self) -> Option<&dyn crate::element::InheritedElementAccess> {
        <StatefulBehavior<V> as ElementBehavior<V, A>>::as_inherited_access(&self.stateful)
    }

    fn as_inherited_access_mut(
        &mut self,
    ) -> Option<&mut dyn crate::element::InheritedElementAccess> {
        <StatefulBehavior<V> as ElementBehavior<V, A>>::as_inherited_access_mut(&mut self.stateful)
    }

    fn take_provided_scope_stale(&mut self) -> bool {
        <StatefulBehavior<V> as ElementBehavior<V, A>>::take_provided_scope_stale(
            &mut self.stateful,
        )
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

        // Then subscribe to the listenable, caching the `Arc` alongside the
        // owned subscription — `on_unmount` detaches through this cache and
        // never calls `listenable()` again (see the field doc on `subscribed`).
        let listenable = core.view().listenable();
        let mark_dirty = core.create_mark_dirty_callback(crate::RebuildReason::AnimationTick);
        let subscription = listenable.listen(mark_dirty);
        self.subscribed = Some((listenable, subscription));

        tracing::debug!("AnimatedBehavior::on_mount subscribed to listenable");
    }

    fn on_unmount(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        // Unsubscribe through the CACHED `Arc` — never `core.view()
        // .listenable()` again. This is the one removal-path hook a catch
        // cannot make safe (see `subscribed`'s field doc); closing it by
        // caching means a `listenable()` that panics on every call after
        // mount cannot reach this method at all.
        if let Some(subscription) = self.subscribed.take().map(|(_, subscription)| subscription) {
            drop(subscription);
            tracing::debug!("AnimatedBehavior::on_unmount unsubscribed from listenable");
        }

        // Then let StatefulBehavior do its cleanup (dispose state)
        self.stateful.on_unmount(core, owner);
    }

    fn on_update(&mut self, core: &ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        // Listener resubscription happens in `on_view_updated` below, not
        // here: `Element::update` (unified.rs) already swapped `core`'s
        // view by the time `on_update` fires, so `core.view()` at this
        // point IS the new view — there is no way to reach the old
        // listenable to unsubscribe from it. `on_view_updated` is handed
        // the pre-swap `old_view` explicitly and is the correct place to
        // diff the two listenable instances.
        self.stateful.on_update(core, owner);
    }

    fn on_activate(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        self.stateful.on_activate(core, owner);
    }

    fn on_deactivate(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        self.stateful.on_deactivate(core, owner);
    }

    fn on_view_updated(
        &mut self,
        core: &ElementCore<V, A>,
        old_view: &V,
        owner: &mut crate::ElementOwner<'_>,
    ) {
        // Swap the subscription only when the listenable *instance* actually
        // changed — `Arc::ptr_eq`, a reference-identity comparison (the same
        // guard `ValueListenableBuilderState::did_update_view` uses). A rebuild that passes the same `Arc<dyn Listenable>` through
        // unchanged (by far the common case — the parent rebuilt but the
        // animation itself didn't) must not unsubscribe/resubscribe.
        //
        // One `listenable()` call here, not two: the CACHED `Arc` in
        // `subscribed` IS the old instance by construction
        // — `on_mount` and this method's own swap below are the only
        // writers — so comparing the new view's `listenable()` against the
        // cache is equivalent to also reading `old_view.listenable()`,
        // without the second user call.
        let new_listenable = core.view().listenable();
        let unchanged = self
            .subscribed
            .as_ref()
            .is_some_and(|(cached, _)| Rc::ptr_eq(cached, &new_listenable));

        if !unchanged {
            // Safe to detach unconditionally, even if the old listenable's
            // backing `ChangeNotifier` were already disposed: an empty
            // channel detaches silently (the subscription's entry is simply
            // absent), precisely so teardown code can detach from an
            // already-disposed listenable.
            drop(self.subscribed.take().map(|(_, subscription)| subscription));

            let mark_dirty = core.create_mark_dirty_callback(crate::RebuildReason::AnimationTick);
            let subscription = new_listenable.listen(mark_dirty);
            self.subscribed = Some((new_listenable, subscription));

            tracing::debug!(
                "AnimatedBehavior::on_view_updated resubscribed: listenable instance changed"
            );
        }

        self.stateful.on_view_updated(core, old_view, owner);
    }

    /// Delegate to the composed `StatefulBehavior` so animated
    /// dependents also fire the typed
    /// `ViewState::did_change_dependencies` hook before rebuilding.
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
