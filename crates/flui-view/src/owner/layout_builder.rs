//! The build-during-layout seam.
//!
//! # Why this exists
//!
//! Flutter's `LayoutBuilder` builds its child **inside** `performLayout`, via
//! `invokeLayoutCallback`, mutating the element and render trees mid-walk under
//! nothing but a debug flag. FLUI cannot, for two structural reasons:
//!
//! 1. `PipelineOwner::layout_node_with_children` holds `&mut RenderTree` for the
//!    entire recursive walk (the `SubtreeArena`), so mid-walk structural
//!    mutation is an aliasing violation.
//! 2. Building while the pipeline is checked out panics as soon as the build
//!    mounts a render object, because elements reach the `PipelineOwner`
//!    through the same non-reentrant [`PipelineCell`].
//!
//! So the reentrancy boundary moves to the one point where neither borrow is
//! live: **between** layout passes. [`BuildOwner::run_frame_with_layout_builders`]
//! drives `run_layout` → [`BuildOwner::service_layout_builders`] → `run_layout`
//! … to a fixpoint, then hands the settled tree to the ordinary
//! `PipelineOwner::run_frame`. Observable semantics are Flutter's: a builder sees
//! the real incoming constraints and its child is laid out and painted in the
//! **same frame**. Only the internal pass count differs.
//!
//! This requires no change to `flui-rendering`: the phase transitions
//! (`into_layout` / `into_idle`) are already public, `run_layout` is re-drivable
//! in-phase, and it is a no-op once the tree is clean.
//!
//! # Not a public feature
//!
//! This is seam infrastructure. `LayoutBuilder` is public,
//! but only its element registers here; app code never touches the registry.

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use flui_foundation::{ElementId, RenderId};
use flui_objects::LayoutConstraintsCell;
use flui_rendering::pipeline::PipelineCell;
use parking_lot::Mutex;

use super::{BuildOwner, build_owner::BuildScopeTarget};
use crate::tree::ElementTree;

/// Upper bound on layout↔build passes within one frame.
///
/// Steady state is a single pass: `needs_build` is edge-triggered, so a builder
/// whose constraints did not change never re-dirties its element. Every scope
/// whose constraints were published by the same layout pass drains from its own
/// bucket in one service step, regardless of nesting depth. Another pass is
/// needed only when a newly mounted nested builder first publishes constraints.
/// Reaching the bound means builder output keeps changing incoming constraints.
///
/// Flutter asserts on the same class of bug. We bound the loop so a release
/// build degrades to a stale frame rather than hanging the UI thread.
const MAX_LAYOUT_BUILD_PASSES: usize = 10;

/// A live build-during-layout node: the element to rebuild, and the cell its
/// render object publishes constraints into.
#[derive(Debug, Clone)]
pub(crate) struct LayoutBuilderEntry {
    /// Element rebuilt when the cell reports `needs_build`.
    pub(crate) element: ElementId,
    /// Shared with the render object registered under the same `RenderId`.
    pub(crate) cell: Arc<LayoutConstraintsCell>,
}

/// Registry of live build-during-layout nodes, keyed by render id.
///
/// The map stays behind a private mutex because element lifecycle paths hold a
/// split borrow of the registry. An atomic length keeps the empty-registry frame
/// path lock-free; insertion/removal updates it while holding the map lock.
///
/// [`ElementOwner`]: super::ElementOwner
#[derive(Debug, Default)]
pub(crate) struct LayoutBuilderRegistry {
    entries: Mutex<HashMap<RenderId, LayoutBuilderEntry>>,
    len: AtomicUsize,
}

impl LayoutBuilderRegistry {
    pub(crate) fn insert(&self, render_id: RenderId, entry: LayoutBuilderEntry) {
        let mut entries = self.entries.lock();
        entries.insert(render_id, entry);
        self.len.store(entries.len(), Ordering::Release);
    }

    pub(crate) fn remove(&self, render_id: RenderId) {
        let mut entries = self.entries.lock();
        entries.remove(&render_id);
        self.len.store(entries.len(), Ordering::Release);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len.load(Ordering::Acquire) == 0
    }

    #[cfg(any(test, feature = "test-utils"))]
    pub(crate) fn len(&self) -> usize {
        self.len.load(Ordering::Acquire)
    }

    pub(crate) fn with_entries<R>(
        &self,
        f: impl FnOnce(&HashMap<RenderId, LayoutBuilderEntry>) -> R,
    ) -> R {
        f(&self.entries.lock())
    }

    pub(crate) fn with_entries_mut<R>(
        &self,
        f: impl FnOnce(&mut HashMap<RenderId, LayoutBuilderEntry>) -> R,
    ) -> R {
        let mut entries = self.entries.lock();
        let result = f(&mut entries);
        self.len.store(entries.len(), Ordering::Release);
        result
    }
}

impl BuildOwner {
    fn unchanged_layout_builder_registrations(
        &self,
        scheduled: &HashMap<ElementId, (RenderId, Arc<LayoutConstraintsCell>)>,
    ) -> HashSet<ElementId> {
        self.layout_builder_registry.with_entries(|registry| {
            scheduled
                .iter()
                .filter_map(|(element, (render_id, cell))| {
                    registry.get(render_id).and_then(|entry| {
                        (entry.element == *element && Arc::ptr_eq(&entry.cell, cell))
                            .then_some(*element)
                    })
                })
                .collect()
        })
    }

    /// Rebuild every registered layout builder whose constraints changed, then
    /// mark its render node for re-layout.
    ///
    /// Returns `true` iff at least one builder rebuilt — i.e. iff another
    /// `run_layout` pass is required before the frame may paint. Returning
    /// `false` is what terminates
    /// [`run_frame_with_layout_builders`](Self::run_frame_with_layout_builders).
    ///
    /// # Stale entries
    ///
    /// An element or render node can disappear between registration and service
    /// (its subtree was reconciled away, and `on_unmount` has not run or ran
    /// against an already-removed node). Servicing such an entry would
    /// `mark_needs_layout` a dead `RenderId`. Every pass therefore prunes
    /// entries whose element is no longer in the tree, or whose render node is
    /// no longer in the render tree, **before** deciding what to build. This
    /// mirrors the hazard documented at `sliver_adaptor.rs`'s `on_unmount`.
    ///
    /// # The pipeline cell must be free
    ///
    /// This runs `build_scope`, and mounting a render element reaches the
    /// `PipelineOwner` through the [`PipelineCell`] each element carries
    /// (`set_pipeline_owner`). So the caller must have **restored the owner
    /// into the cell and let any borrow end** first — exactly as
    /// [`service_child_requests`](Self::service_child_requests) requires.
    /// Holding a `with`/`with_mut` borrow across this call panics the moment a
    /// builder actually mounts a child and reaches for the same cell. The
    /// debug tripwire below turns that hang-turned-panic into a loud,
    /// earlier failure.
    pub fn service_layout_builders(
        &mut self,
        tree: &mut ElementTree,
        pipeline: &PipelineCell,
    ) -> bool {
        debug_assert!(
            pipeline.is_free(),
            "BUG: service_layout_builders ran while the pipeline was checked out — \
             build_scope would panic as soon as a builder mounts a child"
        );

        if self.layout_builder_registry.is_empty() {
            if !self.has_partitioned_builds() {
                return false;
            }

            // The last scope can disappear after the frame's global build has
            // already quarantined a dirty descendant beneath it. Reclassify
            // those surviving buckets against an empty live-scope set and
            // drain them globally now; otherwise no registry entry remains to
            // request another layout-service pass.
            let live_scopes = HashSet::new();
            self.repartition_dirty(tree, &live_scopes);
            self.activate_build_target(BuildScopeTarget::Global);
            let drain = self.drain_prepared_build_target(
                tree,
                BuildScopeTarget::Global,
                Some(&live_scopes),
            );
            if drain.any_rebuilt {
                self.finalize_tree(tree);
            }
            self.collapse_empty_build_scope_queues();
            return drain.any_rebuilt;
        }

        // Prune stale entries and collect the ones that need a build, in one
        // pass over the registry. Both the registry lock and the pipeline
        // borrow are released before `build_scope` runs.
        let mut scheduled: HashMap<ElementId, (RenderId, Arc<LayoutConstraintsCell>)> =
            HashMap::new();
        let mut live_entries: HashMap<ElementId, (RenderId, Arc<LayoutConstraintsCell>)> =
            HashMap::new();
        let mut live_scopes = HashSet::new();
        pipeline.with(|pipeline_owner| {
            self.layout_builder_registry.with_entries_mut(|registry| {
                registry.retain(|render_id, entry| {
                    let element_alive = tree.contains(entry.element);
                    let render_alive = pipeline_owner.render_tree().get(*render_id).is_some();
                    if !element_alive || !render_alive {
                        tracing::debug!(
                            ?render_id,
                            element = ?entry.element,
                            element_alive,
                            render_alive,
                            "service_layout_builders: pruning stale layout-builder entry"
                        );
                        return false;
                    }
                    live_scopes.insert(entry.element);
                    live_entries.insert(entry.element, (*render_id, Arc::clone(&entry.cell)));
                    if entry.cell.needs_build() {
                        scheduled.insert(entry.element, (*render_id, Arc::clone(&entry.cell)));
                    }
                    true
                });
            });
        });

        for element in scheduled.keys() {
            let depth = tree.get(*element).map_or(0, |node| node.depth);
            tree.mark_needs_build(*element);
            self.schedule_build_for(*element, depth, super::RebuildReason::LayoutChange);
        }

        if scheduled.is_empty()
            && !self.has_dirty_elements()
            && self.external_inbox.lock().is_empty()
        {
            // Keep the scoped queues genuinely lazy on clean frames. In
            // particular, do not allocate scratch/bucket state merely because
            // a live layout builder is registered.
            return false;
        }

        // One authoritative routing pass per service call. Work quarantined by
        // earlier global/scoped drains is reclassified against the current tree,
        // so scope removal and GlobalKey reparenting cannot strand it.
        self.repartition_dirty(tree, &live_scopes);

        // Global work is shallower than every isolated scope and must drain
        // first. Repartitioning can create this bucket when a queued element
        // was reparented out of a scope or its scope was unregistered between
        // the frame's global build and this layout-service boundary.
        let mut any_rebuilt = false;
        if self.has_pending_global_builds() {
            self.activate_build_target(BuildScopeTarget::Global);
            any_rebuilt |= self
                .drain_prepared_build_target(tree, BuildScopeTarget::Global, Some(&live_scopes))
                .any_rebuilt;
        }

        let mut ready_scopes: Vec<ElementId> = self
            .ready_layout_scopes()
            .into_iter()
            .filter(|scope| {
                live_entries
                    .get(scope)
                    .is_some_and(|(_, cell)| cell.constraints().is_some())
            })
            .collect();
        ready_scopes.extend(
            scheduled
                .keys()
                .copied()
                .filter(|scope| !self.has_pending_layout_scope(*scope)),
        );
        if ready_scopes.is_empty() {
            if any_rebuilt {
                self.finalize_tree(tree);
            }
            self.collapse_empty_build_scope_queues();
            return any_rebuilt;
        }
        ready_scopes.sort_by_key(|scope| tree.get(*scope).map_or(0, |node| node.depth));

        tracing::debug!(
            fresh_constraints = scheduled.len(),
            ready_scopes = ready_scopes.len(),
            "service_layout_builders: draining isolated layout-builder scopes"
        );

        let mut rebuilt_scopes = HashSet::new();
        for scope in &ready_scopes {
            self.activate_build_target(BuildScopeTarget::LayoutBuilder(*scope));
            let drain = self.drain_prepared_build_target(
                tree,
                BuildScopeTarget::LayoutBuilder(*scope),
                Some(&live_scopes),
            );
            any_rebuilt |= drain.any_rebuilt;
            if drain.target_rebuilt {
                rebuilt_scopes.insert(*scope);
            }
        }

        let unchanged_registrations = self.unchanged_layout_builder_registrations(&scheduled);
        pipeline.with_mut(|pipeline_owner| {
            for (element, (render_id, cell)) in &scheduled {
                if rebuilt_scopes.contains(element)
                    && unchanged_registrations.contains(element)
                    && tree.contains(*element)
                    && pipeline_owner.render_tree().get(*render_id).is_some()
                {
                    cell.commit();
                    pipeline_owner.mark_needs_layout(*render_id);
                }
            }
        });

        self.finalize_tree(tree);
        self.collapse_empty_build_scope_queues();
        any_rebuilt
    }

    /// Run one frame, settling every build-during-layout node before paint.
    ///
    /// This is the **single** implementation shared by `HeadlessBinding::pump_frame`
    /// and `AppBinding::draw_frame`. The two frame paths must not diverge here: a
    /// builder that settles headlessly but not on screen (or vice versa) is a
    /// silent correctness bug, so neither binding may hand-roll this loop.
    ///
    /// The loop drives `run_layout` → `service_layout_builders` until no builder
    /// needs a build, then delegates to
    /// [`PipelineOwner::run_frame`](flui_rendering::pipeline::PipelineOwner::run_frame)
    /// for the full
    /// layout → compositing → paint → semantics sequence. `run_frame`'s own
    /// `run_layout` is a no-op on the settled tree (it early-exits when the
    /// scheduler has no layout work), so the frame orchestrator is not
    /// duplicated here.
    ///
    /// # Checkout discipline
    ///
    /// Each pass checks the owner out of `pipeline` via `with_mut`, lays out,
    /// **restores it before the closure returns**, and only then services the
    /// builders. `build_scope` mounts render objects through the very same
    /// [`PipelineCell`], so building while still inside that `with_mut` closure
    /// would panic (reentrant checkout). This is why the owner is threaded by
    /// cell rather than by value, and it is the same discipline
    /// `service_child_requests` follows.
    ///
    /// # Non-convergence
    ///
    /// Bounded at `MAX_LAYOUT_BUILD_PASSES`. Exceeding it means a builder's
    /// output changes the constraints that builder receives. In debug this is a
    /// `BUG:` panic (an internal-invariant violation per `docs/PANIC-POLICY.md`);
    /// in release it logs and paints the last settled tree rather than spinning.
    pub fn run_frame_with_layout_builders(
        &mut self,
        tree: &mut ElementTree,
        pipeline: &PipelineCell,
    ) -> flui_rendering::error::RenderResult<Option<flui_rendering::layer::LayerTree>> {
        let converged = {
            let owner = &mut *self;
            drive_fixpoint(|| {
                // Layout checked out…
                pipeline.with_mut(|pipeline_owner| {
                    let mut layout = std::mem::take(pipeline_owner).into_layout();
                    let result = layout.run_layout();
                    // Restore on the error path too: the owner always comes back.
                    *pipeline_owner = layout.into_idle();
                    result
                })?;
                // …build with the checkout closed.
                Ok(owner.service_layout_builders(tree, pipeline))
            })
        };

        match converged {
            Err(e) => return Err(e),
            Ok(false) => report_non_convergence(),
            Ok(true) => {}
        }

        pipeline.with_mut(|pipeline_owner| {
            let (owner, result) = std::mem::take(pipeline_owner).run_frame();
            *pipeline_owner = owner;
            result
        })
    }
}

/// Drive `pass` until it reports no further pass is needed, or the bound trips.
///
/// `pass` returns `true` when **another** iteration is required. Returns
/// `Ok(true)` on convergence, `Ok(false)` when [`MAX_LAYOUT_BUILD_PASSES`] was
/// exhausted without converging. Errors short-circuit.
///
/// Factored out of [`BuildOwner::run_frame_with_layout_builders`] so the bound —
/// the thing standing between a buggy builder and a hung UI thread — is unit
/// testable without a pipeline.
fn drive_fixpoint<E>(mut pass: impl FnMut() -> Result<bool, E>) -> Result<bool, E> {
    for _ in 0..MAX_LAYOUT_BUILD_PASSES {
        if !pass()? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Test-only access to the otherwise crate-private registry.
///
/// Exists because the seam ships **inert** — no widget registers
/// into it until `LayoutBuilder` lands — so the only way for `flui-testing` / `flui-app` to prove
/// their frame paths actually run the seam is to plant an entry by hand.
#[cfg(any(test, feature = "test-utils"))]
impl BuildOwner {
    /// Plant a layout-builder entry and hand back the cell its (future) render
    /// object would publish into.
    ///
    /// Returns the `Arc` so a caller in another crate need not depend on
    /// `flui-objects` to name the cell type.
    pub fn register_layout_builder_for_test(
        &mut self,
        render_id: RenderId,
        element: ElementId,
    ) -> Arc<LayoutConstraintsCell> {
        let cell = Arc::new(LayoutConstraintsCell::new());
        self.layout_builder_registry.insert(
            render_id,
            LayoutBuilderEntry {
                element,
                cell: Arc::clone(&cell),
            },
        );
        cell
    }

    /// Number of live entries in the layout-builder registry.
    #[must_use]
    pub fn layout_builder_count(&self) -> usize {
        self.layout_builder_registry.len()
    }
}

/// Debug: panic. Release: log and let the caller paint the last settled tree.
///
/// Split out so the release path is `#[cold]` and the debug path is a single
/// unconditional panic rather than a `cfg!` branch clippy has to reason about.
#[cold]
fn report_non_convergence() {
    #[cfg(debug_assertions)]
    panic!(
        "BUG: layout<->build fixpoint failed to converge after {MAX_LAYOUT_BUILD_PASSES} passes \
         — a layout builder's output is changing its own incoming constraints"
    );

    #[cfg(not(debug_assertions))]
    tracing::error!(
        max_passes = MAX_LAYOUT_BUILD_PASSES,
        "layout<->build fixpoint failed to converge — a layout builder's output is changing \
         its own incoming constraints; painting the last settled tree"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_objects::RenderSizedBox;
    use flui_rendering::constraints::BoxConstraints;
    use flui_rendering::pipeline::PipelineOwner;
    use flui_rendering::protocol::BoxProtocol;
    use flui_types::geometry::px;

    use crate::{RebuildReason, View};

    /// A render-family leaf view, mirroring `build_owner.rs`'s `TestView`.
    ///
    /// Stands in for the `LayoutBuilder` view this seam will eventually add:
    /// this test only needs *some* element that owns *some* render node, so
    /// the liveness and scheduling paths can be exercised without a real builder.
    #[derive(Clone)]
    struct TestView;

    impl crate::RenderView for TestView {
        type Protocol = BoxProtocol;
        type RenderObject = RenderSizedBox;

        fn create_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            RenderSizedBox::shrink()
        }

        fn update_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
            _render_object: &mut Self::RenderObject,
        ) -> flui_rendering::RenderUpdateImpact {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }

    impl View for TestView {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }
    }

    /// The shared pipeline handle, exactly as the bindings hold it.
    fn shared_pipeline() -> PipelineCell {
        PipelineCell::new(PipelineOwner::new())
    }

    fn constraints(side: f32) -> BoxConstraints {
        BoxConstraints::tight_for(Some(px(side)), Some(px(side)))
    }

    /// Whether `render_id` is queued for the next layout pass.
    fn needs_layout(pipeline: &PipelineCell, render_id: RenderId) -> bool {
        pipeline.with(|pipeline_owner| {
            pipeline_owner
                .nodes_needing_layout()
                .iter()
                .any(|node| node.id == render_id)
        })
    }

    /// Mount an element and insert a render node, returning ids that both pass
    /// the liveness check in `service_layout_builders`.
    fn live_entry(
        owner: &mut BuildOwner,
        tree: &mut ElementTree,
        pipeline: &PipelineCell,
    ) -> (RenderId, ElementId) {
        let element = tree.mount_root(&TestView, &mut owner.element_owner_mut());
        let render_id = pipeline.with_mut(|pipeline_owner| {
            pipeline_owner.insert::<BoxProtocol>(Box::new(RenderSizedBox::shrink()))
        });
        (render_id, element)
    }

    fn insert_child(
        owner: &mut BuildOwner,
        tree: &mut ElementTree,
        parent: ElementId,
        slot: usize,
    ) -> ElementId {
        tree.insert(&TestView, parent, slot, &mut owner.element_owner_mut())
    }

    fn insert_render_node(pipeline: &PipelineCell) -> RenderId {
        pipeline.with_mut(|pipeline_owner| {
            pipeline_owner.insert::<BoxProtocol>(Box::new(RenderSizedBox::shrink()))
        })
    }

    // ── the bound ───────────────────────────────────────────────────────────

    #[test]
    fn layout_builder_fixpoint_stops_when_nothing_needs_build() {
        let mut passes = 0;
        let converged = drive_fixpoint(|| {
            passes += 1;
            Ok::<bool, ()>(false)
        });
        assert_eq!(converged, Ok(true));
        assert_eq!(passes, 1, "a settled tree costs exactly one pass");
    }

    #[test]
    fn layout_builder_fixpoint_reruns_while_builders_rebuild() {
        let mut passes = 0;
        let converged = drive_fixpoint(|| {
            passes += 1;
            Ok::<bool, ()>(passes < 3)
        });
        assert_eq!(converged, Ok(true));
        assert_eq!(passes, 3);
    }

    /// The anti-hang guarantee: a builder that never settles terminates the loop
    /// at the bound instead of spinning forever.
    #[test]
    fn layout_builder_fixpoint_is_bounded_and_never_spins() {
        let mut passes = 0;
        let converged = drive_fixpoint(|| {
            passes += 1;
            Ok::<bool, ()>(true)
        });
        assert_eq!(converged, Ok(false), "must report non-convergence");
        assert_eq!(passes, MAX_LAYOUT_BUILD_PASSES);
    }

    #[test]
    fn layout_builder_fixpoint_short_circuits_on_error() {
        let mut passes = 0;
        let result = drive_fixpoint(|| {
            passes += 1;
            Err::<bool, &str>("layout failed")
        });
        assert_eq!(result, Err("layout failed"));
        assert_eq!(passes, 1);
    }

    /// Non-convergence is a `BUG:` panic in debug, per `docs/PANIC-POLICY.md`.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "BUG: layout<->build fixpoint failed to converge")]
    fn layout_builder_non_convergence_guard_panics_in_debug() {
        report_non_convergence();
    }

    // ── registry ────────────────────────────────────────────────────────────

    #[test]
    fn layout_builder_registry_registers_and_unregisters() {
        let mut owner = BuildOwner::new();
        let render_id = RenderId::new(1);
        let element = ElementId::new(1);

        assert_eq!(owner.layout_builder_count(), 0);
        let _cell = owner.register_layout_builder_for_test(render_id, element);
        assert_eq!(owner.layout_builder_count(), 1);

        owner
            .element_owner_mut()
            .unregister_layout_builder(render_id);
        assert_eq!(owner.layout_builder_count(), 0);
    }

    /// `register_layout_builder` is the production path (called from a future
    /// element's `on_mount`); prove it and the test hook agree.
    #[test]
    fn layout_builder_registry_register_via_element_owner() {
        let mut owner = BuildOwner::new();
        let render_id = RenderId::new(7);
        let cell = Arc::new(LayoutConstraintsCell::new());

        owner.element_owner_mut().register_layout_builder(
            render_id,
            ElementId::new(3),
            Arc::clone(&cell),
        );
        assert_eq!(owner.layout_builder_count(), 1);
    }

    #[test]
    fn scheduled_cell_is_not_authoritative_after_registration_replacement() {
        let mut owner = BuildOwner::new();
        let render_id = RenderId::new(7_001);
        let element = ElementId::new(7_001);
        let original = owner.register_layout_builder_for_test(render_id, element);
        let scheduled = HashMap::from([(element, (render_id, Arc::clone(&original)))]);
        assert!(
            owner
                .unchanged_layout_builder_registrations(&scheduled)
                .contains(&element)
        );

        owner
            .element_owner_mut()
            .unregister_layout_builder(render_id);
        let replacement = owner.register_layout_builder_for_test(render_id, element);
        assert!(!Arc::ptr_eq(&original, &replacement));
        assert!(
            owner
                .unchanged_layout_builder_registrations(&scheduled)
                .is_empty(),
            "a replaced registration must not commit the stale scheduled cell"
        );
    }

    // ── service ─────────────────────────────────────────────────────────────

    #[test]
    fn scoped_queue_state_is_lazy_and_collapses_after_last_scope_removal() {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let pipeline = shared_pipeline();
        assert!(owner.build_scope_queues.is_none());

        let root = tree.mount_root(&TestView, &mut owner.element_owner_mut());
        owner.build_scope(&mut tree);
        assert!(
            owner.build_scope_queues.is_none(),
            "the no-scope build path must not allocate queue state"
        );
        tree.mark_needs_build(root);
        owner.schedule_build_for(root, 0, RebuildReason::StateChange);
        BuildOwner::reset_layout_scope_classifications();
        owner.build_scope(&mut tree);
        assert_eq!(BuildOwner::layout_scope_classifications(), 0);
        assert!(owner.build_scope_queues.is_none());

        let scope = insert_child(&mut owner, &mut tree, root, 0);
        owner.build_scope(&mut tree);
        let descendant = insert_child(&mut owner, &mut tree, scope, 0);
        owner.build_scope(&mut tree);
        let render_id = insert_render_node(&pipeline);
        let cell = owner.register_layout_builder_for_test(render_id, scope);
        cell.publish(constraints(100.0));
        cell.commit();
        owner.build_scope(&mut tree);
        assert!(owner.build_scope_queues.is_none());
        assert!(!owner.service_layout_builders(&mut tree, &pipeline));
        assert!(
            owner.build_scope_queues.is_none(),
            "a clean live scope must not allocate queue state"
        );
        tree.mark_needs_build(descendant);
        owner.schedule_build_for(descendant, 2, RebuildReason::StateChange);
        owner.build_scope(&mut tree);
        assert!(owner.has_dirty_elements());
        assert_eq!(owner.dirty_count(), 1, "bucketed work remains observable");
        assert_eq!(
            owner.element_owner_mut().dirty_count(),
            1,
            "the lifecycle split-borrow must report bucketed work too"
        );
        assert!(
            format!("{owner:?}").contains("dirty_count: 1"),
            "BuildOwner diagnostics must report the authoritative total"
        );

        owner
            .element_owner_mut()
            .unregister_layout_builder(render_id);
        owner.build_scope(&mut tree);
        assert!(!owner.has_dirty_elements());
        assert!(
            owner.build_scope_queues.is_none(),
            "the last unregister drains to global and releases lazy state"
        );
    }

    #[test]
    fn scope_without_published_constraints_remains_quarantined() {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let pipeline = shared_pipeline();
        let root = tree.mount_root(&TestView, &mut owner.element_owner_mut());
        owner.build_scope(&mut tree);
        let scope = insert_child(&mut owner, &mut tree, root, 0);
        owner.build_scope(&mut tree);
        let descendant = insert_child(&mut owner, &mut tree, scope, 0);
        owner.build_scope(&mut tree);
        let render_id = insert_render_node(&pipeline);
        let cell = owner.register_layout_builder_for_test(render_id, scope);

        tree.mark_needs_build(descendant);
        owner.schedule_build_for(descendant, 2, RebuildReason::StateChange);
        owner.build_scope(&mut tree);
        assert!(!owner.service_layout_builders(&mut tree, &pipeline));
        assert!(owner.pending_rebuild_reasons(descendant).is_some());

        cell.publish(constraints(100.0));
        cell.commit();
        assert!(owner.service_layout_builders(&mut tree, &pipeline));
        assert!(owner.pending_rebuild_reasons(descendant).is_none());
    }

    fn sibling_scope_classifications(scope_count: usize) -> usize {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let pipeline = shared_pipeline();
        let root = tree.mount_root(&TestView, &mut owner.element_owner_mut());
        owner.build_scope(&mut tree);

        for slot in 0..scope_count {
            let scope = insert_child(&mut owner, &mut tree, root, slot);
            owner.build_scope(&mut tree);
            let descendant = insert_child(&mut owner, &mut tree, scope, 0);
            owner.build_scope(&mut tree);
            let render_id = insert_render_node(&pipeline);
            let cell = owner.register_layout_builder_for_test(render_id, scope);
            cell.publish(constraints(100.0));
            cell.commit();
            tree.mark_needs_build(descendant);
            owner.schedule_build_for(descendant, 2, RebuildReason::StateChange);
        }

        owner.build_scope(&mut tree);
        BuildOwner::reset_layout_scope_classifications();
        assert!(owner.service_layout_builders(&mut tree, &pipeline));
        BuildOwner::layout_scope_classifications()
    }

    #[test]
    fn many_sibling_scopes_classify_each_dirty_element_a_constant_number_of_times() {
        for scope_count in [32, 256] {
            let classifications = sibling_scope_classifications(scope_count);
            assert_eq!(
                classifications,
                scope_count * 2,
                "each dirty element is classified once at the service boundary \
                 and once when popped for live-move revalidation"
            );
        }
    }

    #[test]
    fn more_than_ten_nested_stable_scopes_are_serviced_in_one_fixpoint_step() {
        const SCOPE_COUNT: usize = MAX_LAYOUT_BUILD_PASSES + 3;

        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let pipeline = shared_pipeline();
        let root = tree.mount_root(&TestView, &mut owner.element_owner_mut());
        owner.build_scope(&mut tree);
        let mut parent = root;
        let mut descendants = Vec::with_capacity(SCOPE_COUNT);

        for _ in 0..SCOPE_COUNT {
            let scope = insert_child(&mut owner, &mut tree, parent, 0);
            owner.build_scope(&mut tree);
            let descendant = insert_child(&mut owner, &mut tree, scope, 1);
            owner.build_scope(&mut tree);
            let render_id = insert_render_node(&pipeline);
            let cell = owner.register_layout_builder_for_test(render_id, scope);
            cell.publish(constraints(100.0));
            cell.commit();
            descendants.push(descendant);
            parent = scope;
        }

        for descendant in &descendants {
            tree.mark_needs_build(*descendant);
            let depth = tree.get(*descendant).expect("live descendant").depth;
            owner.schedule_build_for(*descendant, depth, RebuildReason::StateChange);
        }
        owner.build_scope(&mut tree);

        assert!(owner.service_layout_builders(&mut tree, &pipeline));
        for descendant in descendants {
            assert!(
                owner.pending_rebuild_reasons(descendant).is_none(),
                "all initially pending nested buckets must drain in one service step"
            );
        }
    }

    #[test]
    fn layout_builder_service_is_a_noop_with_an_empty_registry() {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let pipeline = shared_pipeline();

        assert!(
            !owner.service_layout_builders(&mut tree, &pipeline),
            "no builders ⇒ no further layout pass"
        );
        assert!(owner.build_scope_queues.is_none());
    }

    /// A registered builder whose element and render node never existed is
    /// pruned on the first service, and never scheduled. This is the stale-entry
    /// contract: servicing it would `mark_needs_layout` a dead `RenderId`.
    #[test]
    fn layout_builder_service_prunes_stale_entries() {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let pipeline = shared_pipeline();

        let cell = owner.register_layout_builder_for_test(RenderId::new(42), ElementId::new(42));
        cell.publish(constraints(10.0));
        assert!(cell.needs_build(), "the entry is dirty…");
        assert_eq!(owner.layout_builder_count(), 1);

        // …and yet it must not be built: neither its element nor its render node
        // is alive.
        assert!(
            !owner.service_layout_builders(&mut tree, &pipeline),
            "a stale entry must not request another layout pass"
        );
        assert_eq!(
            owner.layout_builder_count(),
            0,
            "stale entry must be pruned from the registry"
        );
        assert!(!owner.has_dirty_elements(), "nothing may be scheduled");
    }

    /// Pruning keys off **liveness**, not dirtiness: a clean stale entry goes
    /// too. Guards against a predicate that only prunes what it was about to
    /// build.
    #[test]
    fn layout_builder_service_prunes_stale_entries_even_when_clean() {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let pipeline = shared_pipeline();

        let cell = owner.register_layout_builder_for_test(RenderId::new(9), ElementId::new(9));
        assert!(!cell.needs_build());

        assert!(!owner.service_layout_builders(&mut tree, &pipeline));
        assert_eq!(owner.layout_builder_count(), 0);
    }

    /// A **live** entry (element in the tree, render node in the render tree) is
    /// retained across a service that has nothing to build.
    #[test]
    fn layout_builder_service_keeps_live_clean_entries() {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let pipeline = shared_pipeline();

        let (render_id, element) = live_entry(&mut owner, &mut tree, &pipeline);
        let _cell = owner.register_layout_builder_for_test(render_id, element);

        assert!(
            !owner.service_layout_builders(&mut tree, &pipeline),
            "a clean builder requests no further pass"
        );
        assert_eq!(
            owner.layout_builder_count(),
            1,
            "a live entry must survive the prune"
        );
        assert!(!owner.has_dirty_elements());
    }

    /// The core of the seam: a live, dirty builder is scheduled, built, its cell
    /// committed, its render node re-dirtied, and another pass requested.
    #[test]
    fn layout_builder_service_schedules_build_commits_cell_and_marks_layout() {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let pipeline = shared_pipeline();

        let (render_id, element) = live_entry(&mut owner, &mut tree, &pipeline);
        let cell = owner.register_layout_builder_for_test(render_id, element);
        cell.publish(constraints(10.0));
        assert!(cell.needs_build());

        // Establish the precondition: nothing is queued for layout. (Insertion
        // dirties the node for paint too, so assert on the layout queue only.)
        pipeline.with_mut(PipelineOwner::clear_all_dirty_nodes);
        assert!(
            !needs_layout(&pipeline, render_id),
            "precondition: the node must be clean before service, or the \
             post-service assertion below proves nothing"
        );

        let needs_another_pass = owner.service_layout_builders(&mut tree, &pipeline);

        assert!(
            needs_another_pass,
            "a rebuilt builder must request another layout pass"
        );
        assert!(
            !cell.needs_build(),
            "the cell must be committed against the constraints just built"
        );
        assert_eq!(cell.constraints(), Some(constraints(10.0)));
        assert!(
            needs_layout(&pipeline, render_id),
            "the builder's render node must be marked needs-layout so the new \
             child is laid out before paint"
        );
        assert_eq!(owner.layout_builder_count(), 1, "still live");

        // Second service: nothing changed, so the fixpoint terminates.
        assert!(
            !owner.service_layout_builders(&mut tree, &pipeline),
            "an unchanged builder must not re-dirty itself — this is what makes \
             the fixpoint converge"
        );
    }

    #[test]
    fn scheduled_cell_is_not_committed_when_scope_root_cannot_rebuild() {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let pipeline = shared_pipeline();
        let (render_id, element) = live_entry(&mut owner, &mut tree, &pipeline);
        owner.build_scope(&mut tree);
        let cell = owner.register_layout_builder_for_test(render_id, element);
        tree.get_mut(element)
            .expect("live element")
            .element_mut()
            .deactivate();
        cell.publish(constraints(44.0));

        assert!(!owner.service_layout_builders(&mut tree, &pipeline));
        assert!(
            cell.needs_build(),
            "commit requires a completed rebuild of the exact scope root"
        );
    }

    /// The tripwire that keeps the deadlock-turned-panic above from ever
    /// regressing silently: calling the service under a held checkout fails
    /// loudly in debug rather than hanging (or, before the `PipelineCell`
    /// port, deadlocking).
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(
        expected = "BUG: service_layout_builders ran while the pipeline was checked out"
    )]
    fn layout_builder_service_tripwire_fires_under_a_held_checkout() {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let pipeline = shared_pipeline();

        pipeline.with(|_pipeline_owner| {
            owner.service_layout_builders(&mut tree, &pipeline);
        });
    }

    /// Regression: `build_scope` must run with the pipeline checkout **free**.
    ///
    /// Elements carry a [`PipelineCell`] (`set_pipeline_owner`) and check it out
    /// via `with_mut` to mount their render objects. An earlier draft of this
    /// seam held the frame's write guard across `service_layout_builders`,
    /// which self-deadlocked (`parking_lot`'s `RwLock` was not reentrant) the
    /// instant a builder mounted a child — invisible while the registry is
    /// empty, fatal on the first real `LayoutBuilder`. Under `PipelineCell` the
    /// same mistake panics instead of hanging.
    ///
    /// Driving the whole helper over a pipeline-attached tree with a dirty
    /// builder exercises exactly that path. If the checkout is ever held
    /// across the build, the `debug_assert!` tripwire in
    /// `service_layout_builders` fires, turning what would be a hang (or now a
    /// panic either way) into an earlier, more specific failure.
    #[test]
    fn layout_builder_frame_does_not_deadlock_on_a_pipeline_attached_tree() {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let pipeline = shared_pipeline();

        let element = tree.mount_root_with_pipeline_owner(
            &TestView,
            Some(pipeline.clone()),
            &mut owner.element_owner_mut(),
        );
        let render_id = pipeline
            .with_mut(|owner| owner.insert::<BoxProtocol>(Box::new(RenderSizedBox::shrink())));

        let cell = owner.register_layout_builder_for_test(render_id, element);
        cell.publish(constraints(32.0));

        let result = owner.run_frame_with_layout_builders(&mut tree, &pipeline);

        assert!(result.is_ok(), "frame must succeed: {result:?}");
        assert!(!cell.needs_build(), "the builder must have been serviced");
        assert_eq!(cell.constraints(), Some(constraints(32.0)));
    }

    /// The seam is inert until a widget registers into it:
    /// a full frame over an empty registry converges in one pass and paints.
    #[test]
    fn layout_builder_frame_over_empty_registry_converges() {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();

        let pipeline = shared_pipeline();
        let result = owner.run_frame_with_layout_builders(&mut tree, &pipeline);
        assert!(result.is_ok(), "an empty frame must succeed: {result:?}");
    }
}
