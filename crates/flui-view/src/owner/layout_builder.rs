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
//! 2. Building while the pipeline write-lock is held self-deadlocks as soon as
//!    the build mounts a render object, because elements reach the
//!    `PipelineOwner` through the same non-reentrant `Arc<RwLock<…>>`.
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

use std::{collections::HashMap, sync::Arc};

use flui_foundation::{ElementId, RenderId};
use flui_objects::LayoutConstraintsCell;
use flui_rendering::pipeline::PipelineOwner;
use parking_lot::{Mutex, RwLock};

use super::BuildOwner;
use crate::tree::ElementTree;

/// Upper bound on layout↔build passes within one frame.
///
/// Steady state is a single pass: `needs_build` is edge-triggered, so a builder
/// whose constraints did not change never re-dirties its element. A second pass
/// happens when a builder actually rebuilt; a third when a *nested* builder's
/// constraints only became known once its ancestor's fresh child was laid out.
/// Beyond that, a builder's own output is changing the constraints it receives —
/// a bug in the builder, not in the seam.
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
/// `Arc<Mutex<…>>` (not a plain `HashMap`) so [`ElementOwner`] can carry a
/// `&'a` reference to it — the same pattern as `child_manager_registry` and
/// `external_inbox`.
///
/// [`ElementOwner`]: super::ElementOwner
pub(crate) type LayoutBuilderRegistry = Arc<Mutex<HashMap<RenderId, LayoutBuilderEntry>>>;

impl BuildOwner {
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
    /// # The pipeline lock must be free
    ///
    /// This runs `build_scope`, and mounting a render element reaches the
    /// `PipelineOwner` through the `Arc<RwLock<…>>` each element carries
    /// (`set_pipeline_owner_any`). So the caller must have **restored the owner
    /// into its lock and dropped the write guard** first — exactly as
    /// [`service_child_requests`](Self::service_child_requests) requires.
    /// Holding the guard across this call self-deadlocks (`parking_lot`'s
    /// `RwLock` is not reentrant) the moment a builder actually mounts a child.
    /// The debug tripwire below turns that hang into a loud failure.
    pub fn service_layout_builders(
        &mut self,
        tree: &mut ElementTree,
        pipeline: &Arc<RwLock<PipelineOwner>>,
    ) -> bool {
        debug_assert!(
            pipeline.try_read().is_some(),
            "BUG: service_layout_builders ran while the pipeline write-lock was held — \
             build_scope would deadlock as soon as a builder mounts a child"
        );

        // Prune stale entries and collect the ones that need a build, in one
        // pass over the registry. Both the registry lock and the pipeline read
        // lock are released before `build_scope` runs.
        let mut scheduled: Vec<(RenderId, ElementId, Arc<LayoutConstraintsCell>)> = Vec::new();
        {
            let pipeline_guard = pipeline.read();
            let mut registry = self.layout_builder_registry.lock();
            registry.retain(|render_id, entry| {
                let element_alive = tree.contains(entry.element);
                let render_alive = pipeline_guard.render_tree().get(*render_id).is_some();
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
                if entry.cell.needs_build() {
                    scheduled.push((*render_id, entry.element, Arc::clone(&entry.cell)));
                }
                true
            });
        }

        if scheduled.is_empty() {
            return false;
        }

        tracing::debug!(
            count = scheduled.len(),
            "service_layout_builders: rebuilding layout builders with fresh constraints"
        );

        // 1. Schedule each dirty builder's element.
        for (_, element, _) in &scheduled {
            let depth = tree.get(*element).map_or(0, |node| node.depth);
            tree.mark_needs_build(*element);
            self.schedule_build_for(*element, depth, super::RebuildReason::LayoutChange);
        }

        // 2. Run the builders — with NO pipeline lock held, so a builder that
        //    mounts a child can insert its render objects. `build_scope` asserts
        //    `!self.building`, the standing proof we are not inside a layout walk.
        self.build_scope(tree);

        // 3. Commit each cell (published -> last_built, clearing `needs_build`)
        //    and dirty the render node so the next pass lays out the new child.
        //    Commit happens *after* the build, so a builder that observed
        //    constraints C is recorded as having built against C.
        {
            let mut pipeline_guard = pipeline.write();
            for (render_id, _, cell) in &scheduled {
                cell.commit();
                pipeline_guard.mark_needs_layout(*render_id);
            }
        }

        // 4. Unmount children the reconcile replaced.
        self.finalize_tree(tree);

        true
    }

    /// Run one frame, settling every build-during-layout node before paint.
    ///
    /// This is the **single** implementation shared by `HeadlessBinding::pump_frame`
    /// and `AppBinding::draw_frame`. The two frame paths must not diverge here: a
    /// builder that settles headlessly but not on screen (or vice versa) is a
    /// silent correctness bug, so neither binding may hand-roll this loop.
    ///
    /// The loop drives `run_layout` → `service_layout_builders` until no builder
    /// needs a build, then delegates to [`PipelineOwner::run_frame`] for the full
    /// layout → compositing → paint → semantics sequence. `run_frame`'s own
    /// `run_layout` is a no-op on the settled tree (it early-exits when the
    /// scheduler has no layout work), so the frame orchestrator is not
    /// duplicated here.
    ///
    /// # Locking
    ///
    /// Each pass takes the owner out of `pipeline` under the write lock, lays
    /// out, **restores it and drops the guard**, and only then services the
    /// builders. `build_scope` mounts render objects through the very same
    /// `Arc<RwLock<…>>`, so building under the guard would self-deadlock. This is
    /// why the owner is threaded by lock rather than by value, and it is the same
    /// discipline `service_child_requests` follows.
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
        pipeline: &Arc<RwLock<PipelineOwner>>,
    ) -> flui_rendering::error::RenderResult<Option<flui_rendering::layer::LayerTree>> {
        let converged = {
            let owner = &mut *self;
            drive_fixpoint(|| {
                // Layout under the write lock…
                {
                    let mut guard = pipeline.write();
                    let mut layout = std::mem::take(&mut *guard).into_layout();
                    let result = layout.run_layout();
                    // Restore on the error path too: the owner always comes back.
                    *guard = layout.into_idle();
                    result?;
                }
                // …build with the lock free.
                Ok(owner.service_layout_builders(tree, pipeline))
            })
        };

        match converged {
            Err(e) => return Err(e),
            Ok(false) => report_non_convergence(),
            Ok(true) => {}
        }

        let mut guard = pipeline.write();
        let (owner, result) = std::mem::take(&mut *guard).run_frame();
        *guard = owner;
        result
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
/// into it until `LayoutBuilder` lands — so the only way for `flui-binding` / `flui-app` to prove
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
        self.layout_builder_registry.lock().insert(
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
        self.layout_builder_registry.lock().len()
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

    use crate::View;

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
        ) {
        }
    }

    impl View for TestView {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }
    }

    /// The shared pipeline handle, exactly as the bindings hold it.
    fn shared_pipeline() -> Arc<RwLock<PipelineOwner>> {
        Arc::new(RwLock::new(PipelineOwner::new()))
    }

    fn constraints(side: f32) -> BoxConstraints {
        BoxConstraints::tight_for(Some(px(side)), Some(px(side)))
    }

    /// Whether `render_id` is queued for the next layout pass.
    fn needs_layout(pipeline: &Arc<RwLock<PipelineOwner>>, render_id: RenderId) -> bool {
        pipeline
            .read()
            .nodes_needing_layout()
            .iter()
            .any(|node| node.id == render_id)
    }

    /// Mount an element and insert a render node, returning ids that both pass
    /// the liveness check in `service_layout_builders`.
    fn live_entry(
        owner: &mut BuildOwner,
        tree: &mut ElementTree,
        pipeline: &Arc<RwLock<PipelineOwner>>,
    ) -> (RenderId, ElementId) {
        let element = tree.mount_root(&TestView, &mut owner.element_owner_mut());
        let render_id = pipeline
            .write()
            .insert::<BoxProtocol>(Box::new(RenderSizedBox::shrink()));
        (render_id, element)
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

    // ── service ─────────────────────────────────────────────────────────────

    #[test]
    fn layout_builder_service_is_a_noop_with_an_empty_registry() {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let pipeline = shared_pipeline();

        assert!(
            !owner.service_layout_builders(&mut tree, &pipeline),
            "no builders ⇒ no further layout pass"
        );
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
        pipeline.write().clear_all_dirty_nodes();
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

    /// The tripwire that keeps the deadlock above from ever regressing silently:
    /// calling the service under a held write guard fails loudly in debug rather
    /// than hanging.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "BUG: service_layout_builders ran while the pipeline write-lock")]
    fn layout_builder_service_tripwire_fires_under_a_held_write_lock() {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let pipeline = shared_pipeline();

        let _guard = pipeline.write();
        owner.service_layout_builders(&mut tree, &pipeline);
    }

    /// Regression: `build_scope` must run with the pipeline write-lock **free**.
    ///
    /// Elements carry the `Arc<RwLock<PipelineOwner>>` (`set_pipeline_owner_any`)
    /// and lock it to mount their render objects. An earlier draft of this seam
    /// held the frame's write guard across `service_layout_builders`, which is a
    /// self-deadlock (`parking_lot` `RwLock` is not reentrant) the instant a
    /// builder mounts a child — invisible while the registry is empty, fatal on
    /// the first real `LayoutBuilder`.
    ///
    /// Driving the whole helper over a pipeline-attached tree with a dirty
    /// builder exercises exactly that path. If the guard is ever held across the
    /// build, the `debug_assert!` tripwire in `service_layout_builders` fires,
    /// turning what would be a hang into a loud failure.
    #[test]
    fn layout_builder_frame_does_not_deadlock_on_a_pipeline_attached_tree() {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let pipeline = shared_pipeline();

        let element = tree.mount_root_with_pipeline_owner(
            &TestView,
            Some(Arc::clone(&pipeline)),
            &mut owner.element_owner_mut(),
        );
        let render_id = pipeline
            .write()
            .insert::<BoxProtocol>(Box::new(RenderSizedBox::shrink()));

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
