//! The build-during-layout seam.
//!
//! # Why this exists
//!
//! A `LayoutBuilder` cannot build its child **inside** `perform_layout`,
//! mutating the element and render trees mid-walk, for two structural reasons:
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
//! `PipelineOwner::run_frame`. A builder sees
//! the real incoming constraints and its child is laid out and painted in the
//! **same frame**; the cost is only extra internal layout passes.
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
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};

use flui_foundation::{ElementId, RenderId};
use flui_objects::BuildDuringLayoutCell;
// Only the `testing` registration helper names the concrete cell; the registry
// itself is payload-blind.
#[cfg(any(test, feature = "testing"))]
use flui_objects::LayoutConstraintsCell;
use flui_rendering::pipeline::PipelineCell;

use super::{BuildOwner, build_owner::BuildScopeTarget};
use crate::tree::ElementTree;

/// Passes of one layout↔build fixpoint that made no progress before it stops
/// (ADR-0308).
///
/// Steady state is a single pass: `needs_build` is edge-triggered, so a builder
/// whose constraints did not change never re-dirties its element. Nesting
/// costs one pass per level: a freshly mounted builder publishes only when it
/// is first laid out, a rebuilt builder hands each inner builder a new view
/// that drains in the next pass, and a resize settles constraint-dependent
/// levels one at a time. Such passes are progress and are not charged (see
/// [`classify_pass`]). A charged pass rebuilt a scope that already built in
/// this fixpoint with no ancestor rebuilt in the pass before it: the builder's
/// output changes its own constraints.
const MAX_CHARGED_PASSES: usize = 10;

/// Every pass of one fixpoint, charged or not: the guard against a hang when
/// build-during-layout nesting keeps growing (a builder that mounts a fresh
/// builder below itself on every pass). Legitimate content reaches it only at
/// about this many levels of nesting.
const MAX_FIXPOINT_PASSES: usize = 256;

/// How many fixpoint passes may service lazy-sliver child requests in one
/// frame before the rest is deferred to the next frame.
///
/// A band settles in two or three passes once the extent estimate adapts
/// (request the band → lay it out → one more round when the measured sizes
/// move the band edge); the budget leaves headroom for a heterogeneous list
/// whose mean keeps shifting. A pass that services a band is never charged
/// against [`MAX_CHARGED_PASSES`]: this budget is what bounds those passes.
pub(crate) const MAX_LAZY_BAND_PASSES: usize = 6;

/// What one pass of the layout↔build fixpoint did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PassOutcome {
    /// Nothing rebuilt and no lazy band was serviced: the tree is settled.
    Settled,
    /// Another pass is needed and this one made progress (see [`classify_pass`]).
    Advanced,
    /// Another pass is needed and nothing but a scope rebuilding on its own
    /// explains it.
    Charged,
}

/// How a layout↔build fixpoint ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixpointEnd {
    /// No builder and no lazy band needs another pass.
    Settled,
    /// [`MAX_CHARGED_PASSES`] passes made no progress.
    Repeating,
    /// [`MAX_FIXPOINT_PASSES`] passes ran.
    PassCap,
}

/// What one layout-builder service did: whether anything rebuilt, and which
/// build-during-layout scopes rebuilt as their own target.
struct LayoutBuilderService {
    any_rebuilt: bool,
    rebuilt_scopes: HashSet<ElementId>,
}

impl LayoutBuilderService {
    fn of(any_rebuilt: bool) -> Self {
        Self {
            any_rebuilt,
            rebuilt_scopes: HashSet::new(),
        }
    }
}

/// Whether a pass made progress.
///
/// A pass with work is [`PassOutcome::Advanced`] when it serviced a lazy band
/// ([`MAX_LAZY_BAND_PASSES`] bounds those), or when every scope it rebuilt is
/// explained:
///
/// - the scope's first build in this fixpoint (`built_earlier` holds the
///   scopes built by the passes before this one): a fresh mount, or the
///   cascade a rebuilt outer builder starts;
/// - a proper ancestor of the scope rebuilt in the previous pass (`previous`):
///   a resize settling nested constraint-dependent builders one level a pass.
///   The walk starts at the parent, or every self-oscillation would explain
///   itself;
/// - the scope is no longer in the tree (`alive`), so it cannot rebuild again.
///
/// Anything else, including global work alone, is [`PassOutcome::Charged`].
/// An oscillating scope is charged even when its output mounts a fresh inner
/// builder in the same pass. A run of uncharged repeats strictly deepens the
/// shallowest rebuilt scope, so it is no longer than the tree is deep; fresh
/// scopes need the tree to grow, which [`MAX_FIXPOINT_PASSES`] bounds.
fn classify_pass(
    service: &LayoutBuilderService,
    serviced_lazy: bool,
    built_earlier: &HashSet<ElementId>,
    previous: &HashSet<ElementId>,
    alive: impl Fn(ElementId) -> bool,
    parent_of: impl Fn(ElementId) -> Option<ElementId>,
) -> PassOutcome {
    if !service.any_rebuilt && !serviced_lazy {
        return PassOutcome::Settled;
    }
    if serviced_lazy {
        return PassOutcome::Advanced;
    }
    let explained = |scope: ElementId| {
        if !built_earlier.contains(&scope) || !alive(scope) {
            return true;
        }
        let mut ancestor = parent_of(scope);
        while let Some(id) = ancestor {
            if previous.contains(&id) {
                return true;
            }
            ancestor = parent_of(id);
        }
        false
    };
    if !service.rebuilt_scopes.is_empty() && service.rebuilt_scopes.iter().all(|s| explained(*s)) {
        PassOutcome::Advanced
    } else {
        PassOutcome::Charged
    }
}

/// A live build-during-layout node: the element to rebuild, and the cell its
/// render object publishes constraints into.
#[derive(Debug, Clone)]
pub(crate) struct LayoutBuilderEntry {
    /// Element rebuilt when the cell reports `needs_build`.
    pub(crate) element: ElementId,
    /// Shared with the render object registered under the same `RenderId`.
    ///
    /// Type-erased so the registry can service any build-during-layout node,
    /// not only `LayoutBuilder`. Servicing needs `needs_build`/`has_published`/
    /// `commit` and never the payload — reading that is the owning element's
    /// job, and only it knows the concrete cell type.
    pub(crate) cell: Rc<dyn BuildDuringLayoutCell>, // the registry services heterogeneous build-during-layout nodes (LayoutBuilder's constraints cell, and a sliver persistent header's shrink cell next) and needs only needs_build/has_published/commit; a generic would monomorphise the registry per payload type, so one map could not hold both
}

/// Registry of live build-during-layout nodes, keyed by render id.
///
/// The map sits in a private `RefCell` because element lifecycle paths hold a
/// shared split borrow of the registry. A mirrored length answers the
/// empty-registry frame path without borrowing the map, even while a
/// `with_entries_mut` caller holds it; every mutation updates it before its
/// borrow ends.
///
/// [`ElementOwner`]: super::ElementOwner
#[derive(Debug, Default)]
pub(crate) struct LayoutBuilderRegistry {
    entries: RefCell<HashMap<RenderId, LayoutBuilderEntry>>,
    len: Cell<usize>,
}

impl LayoutBuilderRegistry {
    pub(crate) fn insert(&self, render_id: RenderId, entry: LayoutBuilderEntry) {
        let mut entries = self.entries.borrow_mut();
        entries.insert(render_id, entry);
        self.len.set(entries.len());
    }

    pub(crate) fn remove(&self, render_id: RenderId) {
        let mut entries = self.entries.borrow_mut();
        entries.remove(&render_id);
        self.len.set(entries.len());
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len.get() == 0
    }

    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn len(&self) -> usize {
        self.len.get()
    }

    pub(crate) fn with_entries<R>(
        &self,
        f: impl FnOnce(&HashMap<RenderId, LayoutBuilderEntry>) -> R,
    ) -> R {
        f(&self.entries.borrow())
    }

    pub(crate) fn with_entries_mut<R>(
        &self,
        f: impl FnOnce(&mut HashMap<RenderId, LayoutBuilderEntry>) -> R,
    ) -> R {
        let mut entries = self.entries.borrow_mut();
        let result = f(&mut entries);
        self.len.set(entries.len());
        result
    }
}

impl BuildOwner {
    fn unchanged_layout_builder_registrations(
        &self,
        scheduled: &HashMap<ElementId, (RenderId, Rc<dyn BuildDuringLayoutCell>)>, // the registry services heterogeneous build-during-layout nodes (LayoutBuilder's constraints cell, and a sliver persistent header's shrink cell next) and needs only needs_build/has_published/commit; a generic would monomorphise the registry per payload type, so one map could not hold both
    ) -> HashSet<ElementId> {
        self.layout_builder_registry.with_entries(|registry| {
            scheduled
                .iter()
                .filter_map(|(element, (render_id, cell))| {
                    registry.get(render_id).and_then(|entry| {
                        (entry.element == *element && Rc::ptr_eq(&entry.cell, cell))
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
        self.service_layout_builders_in_pass(tree, pipeline)
            .any_rebuilt
    }

    /// [`Self::service_layout_builders`], reporting also which scopes rebuilt
    /// as their own target, the fixpoint's progress signal.
    fn service_layout_builders_in_pass(
        &mut self,
        tree: &mut ElementTree,
        pipeline: &PipelineCell,
    ) -> LayoutBuilderService {
        // Layout-time rebuilds run their builds through
        // `drain_prepared_build_target` directly, never through `build_scope`,
        // so they would otherwise fall outside every `build` span and a
        // profiler would under-report exactly the frames that did the most
        // build work. `during_layout` distinguishes the two; the ranges are
        // disjoint (this runs between layout passes), so summing is correct.
        // `absorbed_mid_drain` must be declared here too, matching
        // `build_scope_impl`'s own "build" span: `drain_build_scope` records
        // onto whichever span is current, and a field a span never declared
        // silently drops the `record` call — without this, every fixpoint
        // pass's own mid-drain re-entries would vanish from the trace.
        let _span = tracing::debug_span!(
            target: "flui.frame",
            "build",
            during_layout = true,
            absorbed_mid_drain = tracing::field::Empty,
        )
        .entered();

        debug_assert!(
            pipeline.is_free(),
            "BUG: service_layout_builders ran while the pipeline was checked out — \
             build_scope would panic as soon as a builder mounts a child"
        );

        if self.layout_builder_registry.is_empty() {
            if !self.has_partitioned_builds() {
                return LayoutBuilderService::of(false);
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
            return LayoutBuilderService::of(drain.any_rebuilt);
        }

        // Prune stale entries and collect the ones that need a build, in one
        // pass over the registry. Both the registry lock and the pipeline
        // borrow are released before `build_scope` runs.
        let mut scheduled: HashMap<ElementId, (RenderId, Rc<dyn BuildDuringLayoutCell>)> = // the registry services heterogeneous build-during-layout nodes (LayoutBuilder's constraints cell, and a sliver persistent header's shrink cell next) and needs only needs_build/has_published/commit; a generic would monomorphise the registry per payload type, so one map could not hold both
            HashMap::new();
        let mut live_entries: HashMap<ElementId, (RenderId, Rc<dyn BuildDuringLayoutCell>)> = // the registry services heterogeneous build-during-layout nodes (LayoutBuilder's constraints cell, and a sliver persistent header's shrink cell next) and needs only needs_build/has_published/commit; a generic would monomorphise the registry per payload type, so one map could not hold both
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
                            element = entry.element.as_u64(),
                            element_alive,
                            render_alive,
                            "service_layout_builders: pruning stale layout-builder entry"
                        );
                        return false;
                    }
                    live_scopes.insert(entry.element);
                    live_entries.insert(entry.element, (*render_id, Rc::clone(&entry.cell)));
                    if entry.cell.needs_build() {
                        scheduled.insert(entry.element, (*render_id, Rc::clone(&entry.cell)));
                    }
                    true
                });
            });
        });

        for element in scheduled.keys() {
            let depth = tree
                .get(*element)
                .map_or(0, crate::tree::ElementNode::depth);
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
            return LayoutBuilderService::of(false);
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
                    .is_some_and(|(_, cell)| cell.has_published())
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
            return LayoutBuilderService::of(any_rebuilt);
        }
        ready_scopes
            .sort_by_key(|scope| tree.get(*scope).map_or(0, crate::tree::ElementNode::depth));

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
        LayoutBuilderService {
            any_rebuilt,
            rebuilt_scopes,
        }
    }

    /// Run one frame, settling every build-during-layout node before paint.
    ///
    /// This is the **single** implementation shared by `HeadlessBinding::pump_frame`
    /// and `UiRuntime::draw_frame`. The two frame paths must not diverge here: a
    /// builder that settles headlessly but not on screen (or vice versa) is a
    /// silent correctness bug, so neither binding may hand-roll this loop.
    ///
    /// The loop drives `run_layout` → `service_layout_builders` +
    /// `service_child_requests` until no builder needs a build and no lazy
    /// sliver requested or evicted a child, then delegates to
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
    /// Nesting costs a pass per level and is not charged; a pass is charged
    /// only when a scope rebuilds again with nothing above it rebuilt (see
    /// `classify_pass`, ADR-0308). After `MAX_CHARGED_PASSES` charged passes,
    /// or `MAX_FIXPOINT_PASSES` passes in all, the frame logs one error and
    /// paints the last laid-out tree, in every profile: a builder whose output
    /// changes its own constraints is application content, not a FLUI
    /// invariant (`docs/PANIC-POLICY.md`). A lazy band has its own budget,
    /// `MAX_LAZY_BAND_PASSES`, and defers the rest to the next frame.
    pub fn run_frame_with_layout_builders(
        &mut self,
        tree: &mut ElementTree,
        pipeline: &PipelineCell,
    ) -> Result<
        Option<flui_rendering::layer::LayerTree>,
        flui_foundation::TreeError<flui_rendering::error::RenderError>,
    > {
        self.settle_layout_with_builders(tree, pipeline)?;
        pipeline.run_frame()
    }

    /// The build-during-layout fixpoint of
    /// [`Self::run_frame_with_layout_builders`] alone: lay out and service
    /// layout builders and lazy slivers until the tree settles, without
    /// compositing, painting or assembling semantics.
    ///
    /// A frame that must act on the settled tree before paint — move focus off
    /// a node the build removed — runs this, does its work, and then
    /// [`PipelineCell::run_frame`].
    ///
    /// # Errors
    ///
    /// The first layout error of a pass.
    pub fn settle_layout_with_builders(
        &mut self,
        tree: &mut ElementTree,
        pipeline: &PipelineCell,
    ) -> Result<(), flui_foundation::TreeError<flui_rendering::error::RenderError>> {
        let mut lazy_passes: usize = 0;
        let mut lazy_budget_exhausted = false;
        // The scopes built by this fixpoint's passes so far, and by the pass
        // before the current one: the progress signal of `classify_pass`.
        // Per settle call, so the focus re-settle of a frame has its own.
        let mut built_earlier: HashSet<ElementId> = HashSet::new();
        let mut previous: HashSet<ElementId> = HashSet::new();
        let converged: Result<
            FixpointEnd,
            flui_foundation::TreeError<flui_rendering::error::RenderError>,
        > = {
            let owner = &mut *self;
            drive_fixpoint(|| {
                // Layout checked out…
                //
                // `run_layout` drains the channel-delivered dirty marks (a
                // scroll position's change notification, a background
                // producer) BEFORE this pass, exactly as `run_frame` drains
                // before its first phase. Without that, a mark sitting in the
                // channel is laid out only by the final `run_frame` — after
                // servicing has finished — so anything the layout publishes (a
                // header's shrink change) is serviced one frame late, breaking
                // the seam's same-frame promise. The owner stays in the cell
                // on every exit, a panic included.
                pipeline.run_layout()?;
                // …build with the checkout closed. Both deferred-build seams
                // run every pass, neither short-circuits the other: a layout
                // builder inside a lazy item and a lazy list inside a layout
                // builder each need the other's output laid out before the
                // frame can settle. `|`, not `||`, on purpose.
                let service = owner.service_layout_builders_in_pass(tree, pipeline);
                // The lazy seam has its own budget. A lazy band that needs
                // more passes than the budget is content — an estimate so far
                // from the measured extents that the band keeps growing.
                // Past the budget the loop stops servicing, converges on the
                // layout builders alone, and the frame's post-`run_frame`
                // call defers the rest to the next frame: the old multi-frame
                // settle as the fallback.
                let serviced_lazy_children = if lazy_passes < owner.lazy_band_pass_budget {
                    let did_work = owner.service_child_requests_between_passes(tree, pipeline);
                    if did_work {
                        lazy_passes += 1;
                    }
                    did_work
                } else {
                    lazy_budget_exhausted = true;
                    false
                };
                let outcome = classify_pass(
                    &service,
                    serviced_lazy_children,
                    &built_earlier,
                    &previous,
                    |scope| tree.contains(scope),
                    |scope| tree.get(scope).and_then(crate::tree::ElementNode::parent),
                );
                built_earlier.extend(service.rebuilt_scopes.iter().copied());
                previous = service.rebuilt_scopes;
                Ok(outcome)
            })
        };
        if lazy_budget_exhausted {
            tracing::warn!(
                passes = self.lazy_band_pass_budget,
                "lazy sliver band did not settle within the frame's pass budget; \
                 the remaining requests are deferred to the next frame"
            );
            // Evict before paint. The last pass's bands are applied with
            // no builds, and the slivers they touched are marked for the
            // final `run_frame`'s layout — so a resident the band dropped
            // is gone before anything paints, hit-tests, or assembles
            // semantics over it. The requests stay queued for the
            // post-frame service, as the deferral intends.
            self.service_child_requests_evict_only(tree, pipeline);
        }
        // Lazy eviction runs first, so the error below reports a tree that is
        // ready to paint, and no checkout or lock is held when a subscriber
        // (user code) sees it.
        match converged? {
            FixpointEnd::Settled => {
                tracing::debug!(lazy_passes, "layout<->build fixpoint settled");
            }
            end => report_unsettled(end),
        }
        Ok(())
    }
}

/// Drive `pass` until it reports the tree settled, or a bound trips.
///
/// Charged passes count cumulatively, not consecutively, so an oscillation
/// interleaved with progress is still caught. Errors short-circuit.
///
/// Factored out of [`BuildOwner::run_frame_with_layout_builders`] so the bound —
/// the thing standing between a buggy builder and a hung UI thread — is unit
/// testable without a pipeline.
fn drive_fixpoint<E>(mut pass: impl FnMut() -> Result<PassOutcome, E>) -> Result<FixpointEnd, E> {
    let mut charged = 0_usize;
    for _ in 0..MAX_FIXPOINT_PASSES {
        match pass()? {
            PassOutcome::Settled => return Ok(FixpointEnd::Settled),
            PassOutcome::Advanced => {}
            PassOutcome::Charged => {
                charged += 1;
                if charged == MAX_CHARGED_PASSES {
                    return Ok(FixpointEnd::Repeating);
                }
            }
        }
    }
    Ok(FixpointEnd::PassCap)
}

/// Test-only access to the otherwise crate-private registry.
///
/// Exists because the seam ships **inert** — no widget registers
/// into it until `LayoutBuilder` lands — so the only way for `flui-testing` / `flui-app` to prove
/// their frame paths actually run the seam is to plant an entry by hand.
#[cfg(any(test, feature = "testing"))]
impl BuildOwner {
    /// Override the per-frame lazy-band pass budget (default six passes).
    /// Test-only: a budget of one lets a harness
    /// drive the deferral path with ordinary content instead of hunting for
    /// content that defeats the adaptive estimate.
    pub fn set_lazy_band_pass_budget_for_test(&mut self, passes: usize) {
        self.lazy_band_pass_budget = passes;
    }

    /// Plant a layout-builder entry and hand back the cell its (future) render
    /// object would publish into.
    ///
    /// Returns the `Rc` so a caller in another crate need not depend on
    /// `flui-objects` to name the cell type.
    pub fn register_layout_builder_for_test(
        &mut self,
        render_id: RenderId,
        element: ElementId,
    ) -> Rc<LayoutConstraintsCell> {
        let cell = Rc::new(LayoutConstraintsCell::new());
        self.layout_builder_registry.insert(
            render_id,
            LayoutBuilderEntry {
                element,
                cell: Rc::clone(&cell) as Rc<dyn BuildDuringLayoutCell>, // coercion into the registry's erased cell handle, justified at its declaration
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

/// One error for a fixpoint that stopped before it settled; the caller paints
/// the last laid-out tree. Not a panic in any profile: only application
/// content (a builder whose output feeds back into its own constraints, or
/// nesting that keeps growing) reaches it (`docs/PANIC-POLICY.md`, ADR-0308).
#[cold]
fn report_unsettled(end: FixpointEnd) {
    match end {
        FixpointEnd::Settled => {}
        FixpointEnd::Repeating => tracing::error!(
            max_charged_passes = MAX_CHARGED_PASSES,
            "layout<->build fixpoint did not settle: a layout builder keeps rebuilding with \
             nothing above it rebuilt, so its output changes its own incoming constraints; \
             painting the last laid-out tree"
        ),
        FixpointEnd::PassCap => tracing::error!(
            max_passes = MAX_FIXPOINT_PASSES,
            "layout<->build fixpoint did not settle: build-during-layout nesting kept growing \
             for every pass; painting the last laid-out tree"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_objects::RenderSizedBox;
    use flui_rendering::constraints::BoxConstraints;
    use flui_rendering::pipeline::PipelineOwner;
    use flui_rendering::protocol::BoxProtocol;

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
        PipelineCell::new(PipelineOwner::new(
            flui_rendering::TextContextHandle::standalone(),
        ))
    }

    fn constraints(side: f64) -> BoxConstraints {
        BoxConstraints::tight_for(Some(side), Some(side))
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

    // ── the bound ───────────────────────────────────────────────────────────

    /// Drives `outcomes` (repeated from the start when exhausted) through the
    /// fixpoint and returns how it ended and after how many passes.
    fn drive(outcomes: &[PassOutcome]) -> (FixpointEnd, usize) {
        let mut passes = 0;
        let end = drive_fixpoint(|| {
            let outcome = outcomes[passes % outcomes.len()];
            passes += 1;
            Ok::<PassOutcome, ()>(outcome)
        });
        (end.expect("no pass fails"), passes)
    }

    /// The anti-hang guarantees: a builder that keeps rebuilding stops at the
    /// charged bound, nesting that keeps growing stops at the pass cap, and
    /// progress alone is never charged.
    fn layout_builder_fixpoint_is_bounded_and_never_spins() {
        use PassOutcome::{Advanced, Charged, Settled};
        assert_eq!(
            drive(&[Charged]),
            (FixpointEnd::Repeating, MAX_CHARGED_PASSES),
            "a scope that keeps rebuilding on its own"
        );
        let mut deep = vec![Advanced; 3 * MAX_CHARGED_PASSES];
        deep.push(Settled);
        assert_eq!(
            drive(&deep),
            (FixpointEnd::Settled, 3 * MAX_CHARGED_PASSES + 1),
            "nesting three times deeper than the charged bound settles"
        );
        assert_eq!(
            drive(&[Advanced]),
            (FixpointEnd::PassCap, MAX_FIXPOINT_PASSES),
            "nesting that keeps growing"
        );
        assert_eq!(
            drive(&[Charged, Advanced]),
            (FixpointEnd::Repeating, 2 * MAX_CHARGED_PASSES - 1),
            "charged passes count cumulatively through progress"
        );
    }

    /// A pass is progress when every scope it rebuilt is new to the fixpoint,
    /// gone, or under an ancestor the previous pass rebuilt; anything else is
    /// charged, an oscillating scope included when a fresh scope rebuilt
    /// beside it.
    fn layout_builder_pass_progress_is_explained_by_a_fresh_or_ancestor_build() {
        use PassOutcome::{Advanced, Charged, Settled};
        let id = ElementId::new;
        // 1 ← 2 ← 3 ← 4: element 4's parent is 3, and so on; 9 is gone.
        let parents: HashMap<ElementId, ElementId> =
            [(id(2), id(1)), (id(3), id(2)), (id(4), id(3))].into();
        let classify = |rebuilt: &[usize], earlier: &[usize], previous: &[usize], lazy: bool| {
            let set = |ids: &[usize]| ids.iter().map(|n| id(*n)).collect::<HashSet<ElementId>>();
            let service = LayoutBuilderService {
                any_rebuilt: !rebuilt.is_empty(),
                rebuilt_scopes: set(rebuilt),
            };
            classify_pass(
                &service,
                lazy,
                &set(earlier),
                &set(previous),
                |scope| scope != id(9),
                |scope| parents.get(&scope).copied(),
            )
        };
        let rows: [(&str, PassOutcome, PassOutcome); 9] = [
            ("nothing rebuilt", classify(&[], &[], &[], false), Settled),
            (
                "fresh scopes",
                classify(&[3, 4], &[1, 2], &[2], false),
                Advanced,
            ),
            (
                "a repeat under a grandparent the previous pass rebuilt",
                classify(&[4], &[1, 4], &[2], false),
                Advanced,
            ),
            (
                "a repeat with no ancestor rebuilt",
                classify(&[2], &[2], &[3], false),
                Charged,
            ),
            (
                "an oscillating scope beside a fresh one",
                classify(&[2, 3], &[2], &[], false),
                Charged,
            ),
            (
                "a scope is not its own ancestor",
                classify(&[2], &[2], &[2], false),
                Charged,
            ),
            (
                "a scope gone from the tree",
                classify(&[9], &[9], &[], false),
                Advanced,
            ),
            ("a lazy band", classify(&[2], &[2], &[], true), Advanced),
            (
                "global work alone",
                classify_pass(
                    &LayoutBuilderService::of(true),
                    false,
                    &HashSet::new(),
                    &HashSet::new(),
                    |_| true,
                    |_| None,
                ),
                Charged,
            ),
        ];
        let failures: Vec<String> = rows
            .iter()
            .filter(|(_, got, expected)| got != expected)
            .map(|(row, got, expected)| format!("{row}: {got:?}, expected {expected:?}"))
            .collect();
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    // ── registry ────────────────────────────────────────────────────────────

    // ── service ─────────────────────────────────────────────────────────────

    /// The core of the seam: a live, dirty builder is scheduled, built, its cell
    /// committed, its render node re-dirtied, and another pass requested.
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
    fn owner_layout_builder_matrix() {
        crate::table_test::run_table(
            "owner_layout_builder_matrix",
            &[
                (
                    "layout_builder_fixpoint_is_bounded_and_never_spins",
                    layout_builder_fixpoint_is_bounded_and_never_spins as fn(),
                ),
                (
                    "layout_builder_pass_progress_is_explained_by_a_fresh_or_ancestor_build",
                    layout_builder_pass_progress_is_explained_by_a_fresh_or_ancestor_build
                        as fn(),
                ),
                (
                    "layout_builder_service_schedules_build_commits_cell_and_marks_layout",
                    layout_builder_service_schedules_build_commits_cell_and_marks_layout as fn(),
                ),
            ],
        );
    }
}
