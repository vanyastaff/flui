//! Semantics phase implementation for `PipelineOwner<Semantics>`.
//!
//! # Assembly is mark-scoped, with a full rebuild as the fallback
//!
//! A pass re-assembles only the subtrees that can observe the frame's
//! semantics marks, grafting each re-assembled subtree into the persistent
//! semantics arena under its **anchor** — the nearest unmarked ancestor
//! that formed a semantics node last pass. Anything the graft preconditions
//! cannot prove falls back to the classic whole-tree rebuild (flui-semantics ARCHITECTURE.md, semantics assembly),
//! so the fallback path IS the previous behavior and correctness never
//! depends on the graft being possible.
//!
//! ## Why the anchor is the re-assembly unit
//!
//! A marked render object's semantics cannot be re-assembled in isolation:
//! merge/exclude folding means its configuration may be absorbed into an
//! ancestor's formed node, and a pending fragment can flip a *sibling*
//! into forming a node of its own (`mark_configuration_conflicts`). But
//! that influence travels exactly as far as the nearest ancestor that
//! FORMS a node — a formed node absorbs every pending fragment below it,
//! and its own forming decision depends only on its config and inherited
//! context, not on its descendants. So re-assembling the anchor's whole
//! render subtree reproduces everything the mark could have influenced,
//! and nothing above the anchor can change — provided the anchor itself
//! and every ancestor above it are unmarked, which the anchor selection
//! and the containment dedupe guarantee together (a marked ancestor's own
//! anchor sits above it and covers this one).
//!
//! ## What "fresh" means under mark-scoping
//!
//! Geometry and configuration outside the grafted subtrees keep their
//! last-published values. This makes semantics freshness strictly
//! mark-driven, where a full rebuild would refresh every node's geometry
//! as a side effect of ANY mark, an accident no contract promises.

use std::num::NonZeroUsize;

use flui_foundation::RenderId;
use flui_foundation::geometry::{Matrix4, Point, Rect, Size};
use flui_semantics::{
    AccessibilityNodeId, SemanticsCollectionOrigin, SemanticsConfiguration, SemanticsIndexOrigin,
    SemanticsNode, SemanticsOwner, SemanticsRegions, SemanticsTree, SetSize, TableShape,
    TextDirection,
};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    pipeline::{
        phase::{Idle, Semantics},
        scheduler::PhaseKind,
    },
    storage::{RenderNode, RenderTree},
};

use super::{PipelineOwner, rebind_phase, subtree_arena::ensure_stack};

// Render nodes visited by fragment assembly since the last reset — the
// oracle for "a local change re-assembles only the affected subtree".
#[cfg(test)]
thread_local! {
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    static ASSEMBLY_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Render nodes visited by fragment assembly since the last
/// [`reset_assembly_visits`].
#[cfg(test)]
pub(crate) fn assembly_visits() -> usize {
    ASSEMBLY_VISITS.with(std::cell::Cell::get)
}

/// Resets the assembly visit counter.
#[cfg(test)]
pub(crate) fn reset_assembly_visits() {
    ASSEMBLY_VISITS.with(|counter| counter.set(0));
}

// ============================================================================
// Semantics phase: run_semantics
// ============================================================================

impl PipelineOwner<Semantics> {
    /// Completes the frame and returns to [`Idle`].
    #[must_use]
    pub fn finish(self) -> PipelineOwner<Idle> {
        rebind_phase(self)
    }

    /// Updates semantics for all dirty render objects.
    ///
    /// This is phase 4 of the rendering pipeline. During semantics:
    /// - Accessibility information is gathered
    /// - Semantics tree is updated
    ///
    /// Nodes are sorted by depth (shallow first) for top-down traversal.
    /// The geometries of children depend on ancestors' transforms and clips,
    /// so parents must be processed first.
    pub fn run_semantics(&mut self) -> crate::error::RenderResult<()> {
        if !self.semantics_enabled() {
            return Ok(());
        }

        tracing::debug!(
            "run_semantics: {} nodes",
            self.scheduler.semantics_queue_len()
        );

        self.scheduler.enter_phase(PhaseKind::Semantics);
        // Assembly runs authored code (configurations, Flow delegates). Every
        // return leaves the phase itself; an unwind leaves it here, so a host
        // that catches the panic keeps an owner routing marks normally.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.run_semantics_in_phase()
        })) {
            Ok(result) => result,
            Err(payload) => {
                let _ = self.scheduler.exit_phase(PhaseKind::Semantics);
                std::panic::resume_unwind(payload)
            }
        }
    }

    /// The body of [`Self::run_semantics`], entered with the phase set.
    fn run_semantics_in_phase(&mut self) -> crate::error::RenderResult<()> {
        let mut callbacks = AssemblyCallbacks::default();

        // Sort shallow-first. Roots dispatch before their descendants so a parent's config is
        // assembled before children fold into it.
        self.scheduler.sort_semantics_shallow_first();

        let marked: Vec<RenderId> = self
            .scheduler
            .nodes_needing_semantics()
            .iter()
            .map(|d| d.id)
            .filter(|&id| self.render_tree.contains(id))
            .collect();
        let pending_count = marked.len();

        let tree_is_empty = self
            .semantics_owner
            .as_ref()
            .is_some_and(|owner| owner.tree().is_empty());
        let should_build = pending_count > 0 || tree_is_empty;

        if should_build {
            if let Some(owner) = self.semantics_owner.as_mut() {
                // Stage every anchor and all authored key scopes before reserving
                // identities or touching the current arena. A failed later anchor
                // cannot partially commit an earlier subtree.
                let mut regions = AssemblyRegions::default();
                let grafts = (!tree_is_empty)
                    .then(|| {
                        plan_graft_pass(
                            &self.render_tree,
                            self.root_id,
                            owner,
                            &marked,
                            &mut regions,
                        )
                    })
                    .flatten();
                let planned = if let Some(grafts) = grafts {
                    Ok(SemanticsPlan::Graft(grafts))
                } else {
                    // A failed graft proposal can discard configurations, but
                    // their handlers remain owned until the whole pass verdict.
                    regions.visited.clear();
                    regions.authored.clear();
                    regions.duplicate = None;
                    regions.invalid_sort_key = None;
                    assemble_semantics_root(&self.render_tree, self.root_id, &mut regions)
                        .map(|root| SemanticsPlan::Rebuild(root.map(Box::new)))
                };
                let mut planned = match planned {
                    Ok(plan) => plan,
                    Err(error) => {
                        regions.callbacks.rejected = true;
                        let _ = self.scheduler.exit_phase(PhaseKind::Semantics);
                        return Err(crate::error::RenderError::semantics(error.to_string()));
                    }
                };
                if let SemanticsPlan::Rebuild(Some(root)) = &mut planned {
                    reconcile_collections(root);
                }
                // Exit the phase before rejected configurations retire their
                // callback captures. Their destruction is authored code too.
                if let Err(error) = self.semantics_regions.prepare(&regions, &self.render_tree) {
                    regions.callbacks.rejected = true;
                    let _ = self.scheduler.exit_phase(PhaseKind::Semantics);
                    retire_rejected_plan(planned);
                    return Err(crate::error::RenderError::semantics(error.to_string()));
                }
                self.semantics_regions
                    .assign_text_runs(&mut planned, &self.render_tree);
                match planned {
                    SemanticsPlan::Rebuild(built) => {
                        rebuild_semantics_owner(
                            owner,
                            built.map(|node| *node),
                            &self.semantics_regions,
                        );
                    }
                    SemanticsPlan::Graft(grafts) => {
                        for (anchor, built) in grafts {
                            apply_graft(owner, anchor, built, &self.semantics_regions);
                        }
                    }
                }
                let published = owner.flush();
                let diagnostics = owner.take_diagnostics();
                #[cfg(any(test, feature = "testing"))]
                self.unchecked_semantics_diagnostics
                    .extend(diagnostics.iter().cloned());
                self.semantics_diagnostics.extend(diagnostics);
                self.counters.semantics_nodes_updated += published as u64;
                callbacks = std::mem::take(&mut regions.callbacks);
            } else if pending_count > 0 {
                // With no owner installed the pass returns early. Keep the
                // warning so enabled-without-owner wiring
                // is visible during integration without panicking in a frame
                // hot path.
                tracing::warn!(
                    count = pending_count,
                    "run_semantics: SemanticsOwner is not installed; \
                     semantics config build for {pending_count} node(s) is skipped"
                );
            }
        } else if let Some(owner) = self
            .semantics_owner
            .as_mut()
            .filter(|owner| owner.needs_flush())
        {
            // Nothing to assemble, but the owner has work of its own: an
            // announcement, or a full publish a new adapter asked for.
            let published = owner.flush();
            self.counters.semantics_nodes_updated += published as u64;
        } else if pending_count > 0 {
            tracing::warn!(
                count = pending_count,
                "run_semantics: semantics update requested but no rebuild was required"
            );
        }

        // `clear()` retains the Vec's allocated capacity; next frame's
        // pushes amortise into the existing buffer.
        self.scheduler.clear_semantics_queue();

        // exit_phase clears debug_doing_semantics AND drains mid-semantics
        // marks so semantics marks made during this iteration's
        // `debug_doing_semantics = true` window aren't stranded. Drained
        // entries land on dirty.needs_semantics for the NEXT run_semantics.
        let _ = self.scheduler.exit_phase(PhaseKind::Semantics);

        // Losing/filtered handlers retire only after accepted work and phase
        // completion. An unwind during assembly uses the same RAII custody.
        drop(callbacks);

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum RegionPlacement {
    BeforeChildren,
    AfterChildren,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct RegionSource {
    host: RenderId,
    placement: RegionPlacement,
    key: flui_semantics::SemanticsRegionKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SemanticsSource {
    Render(RenderId),
    Region(RegionSource),
}

/// A text run's full identity (ADR-0204): an edit that changes any part gives
/// the run a new identity, so an old position never lands on other text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TextRunKey {
    field: SemanticsSource,
    ordinal: usize,
    start: usize,
    text: Box<str>,
}
/// The pipeline keeps the authority across accessibility disable/enable and
/// owner replacement. Only a successfully assembled scope retires missing keys.
#[derive(Debug, Default)]
pub(super) struct RegionIdentities {
    source: flui_semantics::SemanticsIdentitySource,
    retained: FxHashMap<RegionSource, flui_semantics::SemanticsRegionId>,
    /// The announcement queue and announcer node identities, minted from
    /// `source` on first request and kept across owner replacement.
    announcements: Option<flui_semantics::Announcements>,
    /// The identity space ran out before the announcer nodes were minted.
    announcements_exhausted: bool,
    /// Each field's text runs and their identities, kept while the run is
    /// the same: the same node, place, start and text.
    text_runs: FxHashMap<TextRunKey, flui_semantics::SemanticsRegionId>,
    /// The identity space ran out before a field's runs were given theirs.
    text_runs_exhausted: bool,
}

#[derive(Default)]
struct AssemblyRegions {
    visited: FxHashSet<RenderId>,
    authored: FxHashSet<RegionSource>,
    duplicate: Option<RegionSource>,
    invalid_sort_key: Option<RenderId>,
    callbacks: AssemblyCallbacks,
}

/// Pins executable ownership before folding can replace a receiver's handler
/// or discard a blocked, clipped or fallback contributor. Topology and metadata
/// remain ordinary temporary values; only the opaque captures need this custody.
#[derive(Default)]
struct AssemblyCallbacks {
    handlers: Vec<flui_semantics::SemanticsActionHandler>,
    rejected: bool,
}

impl AssemblyCallbacks {
    fn retain(&mut self, config: &SemanticsConfiguration) {
        self.handlers
            .extend(config.registered_action_handlers().cloned());
    }
}

impl Drop for AssemblyCallbacks {
    fn drop(&mut self) {
        let mut recovery = flui_foundation::panic::PanicRecovery::new();
        if self.rejected {
            recovery.inherit_failure();
        }
        for handler in std::mem::take(&mut self.handlers) {
            recovery.retire(handler);
        }
        recovery.finish();
    }
}

impl RegionIdentities {
    /// The presentation's announcement queue, minting the two announcer
    /// identities on first use. `None` once the identity space is exhausted,
    /// reported once.
    pub(super) fn announcements(&mut self) -> Option<flui_semantics::Announcements> {
        if self.announcements.is_none() && !self.announcements_exhausted {
            if let Ok(ids) = self.source.allocate(2) {
                self.announcements = Some(flui_semantics::Announcements::new(ids[0], ids[1]));
            } else {
                self.announcements_exhausted = true;
                tracing::error!(
                    "semantics identity space exhausted: this presentation cannot announce"
                );
            }
        }
        self.announcements.clone()
    }

    /// Gives the text runs of every field `plan` builds their identities:
    /// a run seen before keeps its own, a new one takes one from the source.
    /// When the source is exhausted the field keeps its runs without
    /// identities, so the owner publishes it without them and reports it.
    /// A field the plan rebuilt forgets the runs it no longer has.
    fn assign_text_runs(&mut self, plan: &mut SemanticsPlan, tree: &RenderTree) {
        let mut stack: Vec<&mut BuiltSemanticsNode> = match plan {
            SemanticsPlan::Rebuild(root) => root.as_deref_mut().into_iter().collect(),
            SemanticsPlan::Graft(grafts) => grafts.iter_mut().map(|(_, built)| built).collect(),
        };
        let mut rebuilt = FxHashSet::default();
        let mut live = FxHashSet::default();
        while let Some(node) = stack.pop() {
            rebuilt.insert(node.source);
            if let Some(content) = node.config.text_content() {
                let keys: Vec<TextRunKey> = content
                    .runs()
                    .iter()
                    .enumerate()
                    .map(|(ordinal, run)| TextRunKey {
                        field: node.source,
                        ordinal,
                        start: run.byte_range().start,
                        text: run.text().into(),
                    })
                    .collect();
                let missing: Vec<&TextRunKey> = keys
                    .iter()
                    .filter(|key| !self.text_runs.contains_key(*key))
                    .collect();
                match self.source.allocate(missing.len()) {
                    Ok(ids) => {
                        for (key, id) in missing.into_iter().zip(ids) {
                            self.text_runs.insert(key.clone(), id);
                        }
                        let ids = keys.iter().map(|key| self.text_runs[key]).collect();
                        let content = content
                            .clone()
                            .with_run_identities(ids)
                            .expect("BUG: one identity is assigned per run");
                        node.config.set_text_content(content);
                    }
                    Err(_) if !self.text_runs_exhausted => {
                        self.text_runs_exhausted = true;
                        tracing::error!(
                            "semantics identity space exhausted: text fields publish without runs"
                        );
                    }
                    Err(_) => {}
                }
                live.extend(keys);
            }
            stack.extend(node.children.iter_mut());
        }
        let retained = &self.retained;
        self.text_runs.retain(|key, _| {
            live.contains(key)
                || (!rebuilt.contains(&key.field)
                    && match key.field {
                        SemanticsSource::Render(id) => tree.contains(id),
                        SemanticsSource::Region(source) => retained.contains_key(&source),
                    })
        });
    }
    /// Spends the identity source.
    #[cfg(any(test, feature = "testing"))]
    pub(super) fn exhaust(&mut self) {
        self.source.exhaust();
    }

    /// The queue, if one was requested.
    pub(super) fn existing_announcements(&self) -> Option<&flui_semantics::Announcements> {
        self.announcements.as_ref()
    }

    fn prepare(
        &mut self,
        regions: &AssemblyRegions,
        tree: &RenderTree,
    ) -> Result<(), SemanticsAssemblyError> {
        if let Some(host) = regions.invalid_sort_key {
            return Err(SemanticsAssemblyError::InvalidSortKey { host });
        }
        if let Some(source) = regions.duplicate {
            return Err(SemanticsAssemblyError::DuplicateRegion {
                host: source.host,
                key: source.key.0,
            });
        }
        let missing: Vec<_> = regions
            .authored
            .iter()
            .copied()
            .filter(|source| !self.retained.contains_key(source))
            .collect();
        let ids = self
            .source
            .allocate(missing.len())
            .map_err(|_| SemanticsAssemblyError::RegionIdentityExhausted)?;
        self.retained.retain(|source, _| {
            tree.contains(source.host)
                && (!regions.visited.contains(&source.host) || regions.authored.contains(source))
        });
        self.retained.extend(missing.into_iter().zip(ids));
        Ok(())
    }
}

struct BuiltSemanticsNode {
    source: SemanticsSource,
    config: SemanticsConfiguration,
    rect: Rect<f64>,
    reveal_rect: Rect<f64>,
    children: Vec<BuiltSemanticsNode>,
}

enum SemanticsPlan {
    Rebuild(Option<Box<BuiltSemanticsNode>>),
    Graft(Vec<(flui_foundation::SemanticsId, BuiltSemanticsNode)>),
}

/// A set an ancestor in the built tree contains, as a member below it sees it.
#[derive(Clone, Copy)]
struct SetScope {
    /// The render node that declared it; a region declares sets of its own.
    host: Option<RenderId>,
    size: SetSize,
    origin: SemanticsCollectionOrigin,
    /// A member below would read this set's total as its own: withhold it.
    omit_total: bool,
}

/// Holds every place a node declares to the sets and tables it actually sits
/// in once the tree is built, so a node the clip dropped imposes no scope.
///
/// AccessKit pairs a position with the size of the nearest included ancestor
/// that declares one (`size_of_set_from_container`) and a cell with the
/// nearest table. So:
///
/// - a position past its own known set's size is not published ("item 8 of
///   3"), nor a cell or row outside its known table ("row 5 of 3");
/// - a member of an unknown or separately authored set must not
///   acquire an unrelated total through that walk ("item 12 of 5"): the totals
///   of the known sets above it are withheld rather than a sentinel invented.
fn reconcile_collections(root: &mut BuiltSemanticsNode) {
    fn visit(
        node: &mut BuiltSemanticsNode,
        sets: &mut Vec<SetScope>,
        tables: &mut Vec<TableShape>,
    ) {
        ensure_stack(|| {
            // A position is held to its own set's size only: one numbered for
            // another set is that set's, and the total is withheld below.
            let member_of_nearest = node.config.position_in_set().is_some()
                && member_matches(node.config.index_origin(), sets.last());
            hold_places_to_scopes(
                &mut node.config,
                sets.last().copied().filter(|_| member_of_nearest),
                tables.last().copied(),
            );
            let own = node.config.size_of_set().map(|size| SetScope {
                host: match node.source {
                    SemanticsSource::Render(id) => Some(id),
                    SemanticsSource::Region(_) => None,
                },
                size,
                origin: node
                    .config
                    .collection_origin()
                    .unwrap_or(SemanticsCollectionOrigin::Authored),
                omit_total: false,
            });
            let unmatched_member = node.config.position_in_set().is_some() && !member_of_nearest;
            // Only a member that would read an unrelated total withholds it:
            // an unknown set with no positioned member below leaves the totals
            // above it alone, and one with members reaches here through them.
            if unmatched_member {
                for scope in sets.iter_mut() {
                    scope.omit_total |= matches!(scope.size, SetSize::Known(_));
                }
            }
            // A table, a grid or a tree grid is its cells' table whether or
            // not it declares a shape: without one its extent is unbounded,
            // and an outer table's shape does not bound its cells.
            let own_table = node.config.published_table_shape().or_else(|| {
                matches!(
                    node.config.role(),
                    flui_semantics::SemanticsRole::Table
                        | flui_semantics::SemanticsRole::Grid
                        | flui_semantics::SemanticsRole::TreeGrid
                )
                .then(|| TableShape::new(SetSize::Unknown, SetSize::Unknown))
            });
            if let Some(scope) = own {
                sets.push(scope);
            }
            if let Some(table) = own_table {
                tables.push(table);
            }
            for child in &mut node.children {
                visit(child, sets, tables);
            }
            if own_table.is_some() {
                tables.pop();
            }
            if own.is_some()
                && sets
                    .pop()
                    .expect("BUG: the scope pushed for this node is on top")
                    .omit_total
            {
                node.config.omit_collection_total();
            }
        });
    }
    visit(root, &mut Vec::new(), &mut Vec::new());
}

/// Whether a member's position belongs to the nearest set above it: a lazy
/// host's stamp to that host, an authored position to an authored set.
fn member_matches(origin: Option<SemanticsIndexOrigin>, nearest: Option<&SetScope>) -> bool {
    let Some(scope) = nearest.filter(|scope| matches!(scope.size, SetSize::Known(_))) else {
        return false;
    };
    match origin {
        Some(SemanticsIndexOrigin::Collection(host)) => scope.host == Some(host),
        Some(SemanticsIndexOrigin::Authored) | None => {
            scope.origin == SemanticsCollectionOrigin::Authored
        }
    }
}

/// Clear the places `config` declares that its nearest set and table cannot
/// hold. Assembly re-runs on every mark, so a drop is logged at debug level
/// only, where a warning would repeat.
fn hold_places_to_scopes(
    config: &mut SemanticsConfiguration,
    set: Option<SetScope>,
    table: Option<TableShape>,
) {
    if let Some(index) = config.position_in_set()
        && let Some(SetSize::Known(size)) = set.map(|set| set.size)
        && index >= size
    {
        tracing::debug!(
            index,
            size,
            "a semantics position is past its set's size; not published"
        );
        config.clear_position_in_set();
    }
    if let Some(cell) = config.cell_position()
        && !cell_fits(cell, table)
    {
        tracing::debug!(
            ?cell,
            ?table,
            "a semantics cell does not lie inside its table; not published"
        );
        config.clear_cell_position();
    }
    if let Some(row) = config.row_index()
        && !row_fits(row, table)
    {
        tracing::debug!(
            row = row.get(),
            ?table,
            "a semantics row is not one of its table's; not published"
        );
        config.clear_row_index();
    }
}

/// Whether a cell, with its spans, lies inside the table it sits in. A known
/// extent bounds it; an unknown one cannot be exceeded.
fn cell_fits(cell: flui_semantics::CellPosition, table: Option<TableShape>) -> bool {
    let within = |first: NonZeroUsize, span: NonZeroUsize, extent: SetSize| match extent {
        SetSize::Known(count) => first
            .get()
            .checked_add(span.get() - 1)
            .is_some_and(|last| last <= count),
        _ => true,
    };
    table.is_none_or(|table| {
        within(cell.row(), cell.row_span(), table.rows())
            && within(cell.column(), cell.column_span(), table.columns())
    })
}

/// Whether a row is one of the rows of the table it sits in.
fn row_fits(row: NonZeroUsize, table: Option<TableShape>) -> bool {
    table.is_none_or(|table| match table.rows() {
        SetSize::Known(count) => row.get() <= count,
        _ => true,
    })
}

/// Whether `config` declares a set or a table, whose members' places
/// [`reconcile_collections`] holds to it.
fn declares_collection_scope(config: &SemanticsConfiguration) -> bool {
    config.size_of_set().is_some() || config.table_shape().is_some()
}

fn has_collection_metadata(root: &BuiltSemanticsNode) -> bool {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if declares_collection_scope(&node.config) {
            return true;
        }
        pending.extend(&node.children);
    }
    false
}

/// Release draft metadata while the validation error retains first-failure
/// authority. Configuration callbacks are opaque ownership (ADR-0127).
fn retire_rejected_plan(plan: SemanticsPlan) {
    let mut nodes: Vec<BuiltSemanticsNode> = match plan {
        SemanticsPlan::Rebuild(root) => root.into_iter().map(|node| *node).collect(),
        SemanticsPlan::Graft(grafts) => grafts.into_iter().map(|(_, node)| node).collect(),
    };
    let mut recovery = flui_foundation::panic::PanicRecovery::new();
    recovery.inherit_failure();
    while let Some(node) = nodes.pop() {
        nodes.extend(node.children);
        node.config.retire(&mut recovery.scope());
    }
    recovery.finish();
}

struct PendingSemanticsNode {
    source: SemanticsSource,
    config: SemanticsConfiguration,
    rect: Rect<f64>,
    reveal_rect: Rect<f64>,
    children: Vec<BuiltSemanticsNode>,
}

impl PendingSemanticsNode {
    fn form(self) -> BuiltSemanticsNode {
        BuiltSemanticsNode {
            source: self.source,
            config: self.config,
            rect: self.rect,
            reveal_rect: self.reveal_rect,
            children: self.children,
        }
    }
}

enum SemanticsFragment {
    Pending(PendingSemanticsNode),
    Formed(BuiltSemanticsNode),
}

/// A subtree's contribution and its still-unconsumed sibling barrier.
/// Transparent render wrappers forward the barrier; a formed boundary consumes
/// descendant barriers while keeping its own directive for its parent.
#[derive(Default)]
struct SemanticsFragments {
    nodes: Vec<SemanticsFragment>,
    blocks_previous_siblings: bool,
}

/// The clips an ancestor chain imposes on a node, in ROOT coordinates.
///
/// Publication stays clipped to paint bounds where there is an intersection.
/// Scroll caches limit ordinary publication but cannot erase a materialized
/// descendant with a reveal ancestor. Authored bounds and occlusion retain
/// their exclusion and hidden behavior independently of viewport clipping.
#[derive(Debug, Clone, Copy, Default)]
struct SemanticsClips {
    /// Outside this, a child is painted nowhere the user can see.
    paint: Option<Rect<f64>>,
    /// Outside this, a child has no accessibility presence at all.
    semantics: Option<Rect<f64>>,
    cache: Option<Rect<f64>>,
    occlusion: Option<Rect<f64>>,
}

/// What [`SemanticsClips::apply`] decided about one node's rect.
struct ClippedRect {
    /// The rect to publish, already narrowed to the surviving part.
    rect: Rect<f64>,
    /// Authored occlusion or ordinary off-screen content makes the node hidden.
    hidden: bool,
    /// Nothing of the node survives the semantics clip: publish no node for
    /// it. Its children are still walked — an overflowing child can extend
    /// back into the clip its parent fell outside of.
    dropped: bool,
}

impl SemanticsClips {
    /// Narrows `rect` to what these clips leave of it.
    fn apply(self, rect: Rect<f64>, revealable: bool) -> ClippedRect {
        let was_empty = rect.is_empty();

        // `Rect::intersect` reports a zero-area overlap as `Some(empty)`;
        // for a clip that is the same answer as no overlap at all.
        let intersect = |clip: &Rect<f64>, rect: &Rect<f64>| {
            clip.intersect(rect).filter(|kept| !kept.is_empty())
        };

        let semantics = if revealable {
            self.semantics
        } else {
            intersect_clips(self.semantics, self.cache)
        };
        let rect = match semantics {
            Some(clip) => match intersect(&clip, &rect) {
                Some(kept) => kept,
                // Nothing survives. An already-empty rect is not "clipped
                // away" — a zero-size annotated node is the caller's own
                // shape and keeps whatever treatment it had.
                None if !was_empty => {
                    return ClippedRect {
                        rect,
                        hidden: false,
                        dropped: true,
                    };
                }
                None => rect,
            },
            None => rect,
        };

        let Some(paint) = self.paint else {
            return ClippedRect {
                rect,
                hidden: false,
                dropped: false,
            };
        };

        match intersect(&paint, &rect) {
            Some(visible) => ClippedRect {
                rect: visible,
                hidden: false,
                dropped: false,
            },
            // Entirely outside what is painted: keep the semantics rect so a
            // "scroll to" action has somewhere to aim, and flag it hidden.
            None => ClippedRect {
                rect,
                hidden: !was_empty
                    && (!revealable
                        || self
                            .occlusion
                            .is_some_and(|clip| intersect(&clip, &rect).is_none())),
                dropped: false,
            },
        }
    }

    /// The clips a child of this node inherits.
    ///
    /// Paint clips intersect. A scroll cache replaces only the cache; authored
    /// bounds replace the hard clip. Other paint clips narrow inherited bounds
    /// and remain authored occlusion even inside a revealable scroll subtree.
    fn descend(
        self,
        local_paint: Option<Rect<f64>>,
        local_semantics: Option<crate::traits::SemanticsClip>,
    ) -> Self {
        use crate::traits::SemanticsClip;
        let paint = intersect_clips(self.paint, local_paint);
        let semantics = match local_semantics {
            Some(SemanticsClip::Bounds(replacement)) => Some(replacement),
            Some(SemanticsClip::ScrollCache(_)) => self.semantics,
            None => match (self.semantics, local_paint) {
                // Disjoint means nothing survives, which is an EMPTY clip —
                // `None` here would read as "no clip at all" and republish
                // the whole subtree unclipped.
                (Some(inherited), Some(narrowing)) => {
                    Some(inherited.intersect(&narrowing).unwrap_or(Rect::ZERO))
                }
                (Some(inherited), None) => Some(inherited),
                (None, _) => None,
            },
        };
        let cache = match local_semantics {
            Some(SemanticsClip::ScrollCache(bounds)) => Some(bounds),
            Some(SemanticsClip::Bounds(_)) => None,
            None => intersect_clips(self.cache, local_paint),
        };
        let occlusion = if matches!(local_semantics, Some(SemanticsClip::ScrollCache(_))) {
            self.occlusion
        } else {
            intersect_clips(self.occlusion, local_paint)
        };
        Self {
            paint,
            semantics,
            cache,
            occlusion,
        }
    }
}

/// Intersection where `None` means "no clip", not "empty".
fn intersect_clips(a: Option<Rect<f64>>, b: Option<Rect<f64>>) -> Option<Rect<f64>> {
    match (a, b) {
        (Some(a), Some(b)) => a.intersect(&b).or(Some(Rect::ZERO)),
        (Some(only), None) | (None, Some(only)) => Some(only),
        (None, None) => None,
    }
}

/// The clips `node` imposes on the child in `child_slot`, moved from the
/// node's own coordinates into the walk's root coordinates.
/// Whether `node` presents the child in `child_slot` to the semantics tree.
///
/// Dispatched the same way `child_clips_of` is, because it answers the same
/// kind of question — one the parent knows about a particular child and the
/// walk cannot infer.
fn visits_child_for_semantics(node: &RenderNode, child_slot: usize) -> bool {
    match node {
        RenderNode::Box(entry) => entry
            .render_object()
            .visits_child_for_semantics(child_slot, entry.state().geometry().unwrap_or(Size::ZERO)),
        RenderNode::Sliver(entry) => entry
            .render_object()
            .visits_child_for_semantics(child_slot, entry.state().absolute_paint_size()),
    }
}

/// The clips `node` imposes on the child in `child_slot`, moved from the
/// node's own coordinates into the walk's root coordinates.
fn child_clips_of(
    node: &RenderNode,
    transform: Matrix4,
    child_slot: usize,
) -> (Option<Rect<f64>>, Option<crate::traits::SemanticsClip>) {
    // The node's own size is passed in rather than cached by each implementor.
    // A clip is always a function of the box it clips, so every implementor
    // would otherwise have to commit its own copy of a value the walk already
    // has — and keep that copy honest across every layout path.
    let (paint, semantics) = match node {
        RenderNode::Box(entry) => {
            let size = entry.state().geometry().unwrap_or(Size::ZERO);
            (
                entry
                    .render_object()
                    .describe_approximate_paint_clip(child_slot, size),
                entry
                    .render_object()
                    .describe_semantics_clip(child_slot, size),
            )
        }
        RenderNode::Sliver(entry) => {
            let size = entry.state().absolute_paint_size();
            (
                entry
                    .render_object()
                    .describe_approximate_paint_clip(child_slot, size),
                entry
                    .render_object()
                    .describe_semantics_clip(child_slot, size),
            )
        }
    };
    (
        paint.map(|rect| transformed_rect(transform, rect).unwrap_or(Rect::ZERO)),
        semantics.map(|clip| match clip {
            crate::traits::SemanticsClip::Bounds(rect) => crate::traits::SemanticsClip::Bounds(
                transformed_rect(transform, rect).unwrap_or(Rect::ZERO),
            ),
            crate::traits::SemanticsClip::ScrollCache(rect) => {
                crate::traits::SemanticsClip::ScrollCache(
                    transformed_rect(transform, rect).unwrap_or(Rect::ZERO),
                )
            }
        }),
    )
}

#[derive(Debug, Clone, Copy)]
struct SemanticsAssemblyContext {
    is_root: bool,
    parent_requires_explicit_node: bool,
    merge_into_ancestor: bool,
    has_reveal_ancestor: bool,
}

#[derive(Debug, thiserror::Error)]
enum SemanticsAssemblyError {
    #[error("render object {host:?} authored a non-finite semantics sort order")]
    InvalidSortKey { host: RenderId },
    #[error(
        "render object {host:?} authored duplicate semantics region key {key} in one contribution list"
    )]
    DuplicateRegion { host: RenderId, key: u64 },

    #[error("synthetic semantics identity space exhausted")]
    RegionIdentityExhausted,
    #[error("semantics root render object {root:?} is missing from the render tree")]
    MissingRootRenderObject { root: RenderId },

    #[error(
        "semantics root assembly produced {actual} fragments; exactly one formed root is required"
    )]
    InvalidRootFragmentCount { actual: usize },

    #[error("semantics root {root:?} remained pending instead of forming a node")]
    PendingRoot { root: RenderId },
}

fn assemble_semantics_root(
    tree: &RenderTree,
    root: Option<RenderId>,
    regions: &mut AssemblyRegions,
) -> Result<Option<BuiltSemanticsNode>, SemanticsAssemblyError> {
    let Some(root) = root else {
        return Ok(None);
    };

    let fragments = build_semantics_fragments(
        tree,
        root,
        Matrix4::IDENTITY,
        SemanticsClips::default(),
        SemanticsAssemblyContext {
            is_root: true,
            parent_requires_explicit_node: false,
            merge_into_ancestor: false,
            has_reveal_ancestor: false,
        },
        false,
        regions,
    )
    .ok_or(SemanticsAssemblyError::MissingRootRenderObject { root })?;

    extract_formed_root(root, fragments.nodes).map(Some)
}

fn extract_formed_root(
    root: RenderId,
    fragments: Vec<SemanticsFragment>,
) -> Result<BuiltSemanticsNode, SemanticsAssemblyError> {
    if fragments.len() != 1 {
        return Err(SemanticsAssemblyError::InvalidRootFragmentCount {
            actual: fragments.len(),
        });
    }

    match fragments.into_iter().next() {
        Some(SemanticsFragment::Formed(root_node)) => Ok(root_node),
        Some(SemanticsFragment::Pending(_)) => Err(SemanticsAssemblyError::PendingRoot { root }),
        None => Err(SemanticsAssemblyError::InvalidRootFragmentCount { actual: 0 }),
    }
}

fn build_semantics_fragments(
    tree: &RenderTree,
    id: RenderId,
    transform: Matrix4,
    clips: SemanticsClips,
    context: SemanticsAssemblyContext,
    ancestor_blocks_user_actions: bool,
    regions: &mut AssemblyRegions,
) -> Option<SemanticsFragments> {
    ensure_stack(|| {
        build_semantics_fragments_impl(
            tree,
            id,
            transform,
            clips,
            context,
            ancestor_blocks_user_actions,
            regions,
        )
    })
}

/// Everything the assembly walk decides about one node from its config and
/// inherited context. Factored out so the graft's ancestor-chain context
/// recomputation ([`assembly_inputs_for`]) folds the EXACT rules the walk
/// applies — a divergence between the two would graft subtrees under a
/// context the full rebuild would never have produced.
struct NodeAssemblyDecisions {
    /// Whether this node contributes semantic content at all.
    contributes: bool,
    /// Whether this node forms its own semantics node.
    forms_node: bool,
    /// Whether pending child fragments absorb without conflict marking.
    children_merge_into_ancestor: bool,
    /// The context this node's children assemble under.
    child_context: SemanticsAssemblyContext,
}

fn assembly_decisions(
    config: &SemanticsConfiguration,
    context: SemanticsAssemblyContext,
) -> NodeAssemblyDecisions {
    let boundary = config.is_semantics_boundary() || config.is_merge_boundary();
    let contributes = context.is_root || boundary || config.has_been_annotated();
    // A node that merges its own descendants is a unit a screen reader stops
    // on, even inside an outer merge: the outer one takes it as a child rather
    // than absorbing it (ADR-0206, decision 4). A merge boundary stops the
    // outer merge the same way but merges nothing itself (ADR-0214 §3).
    let nested_merger = context.merge_into_ancestor
        && (config.is_merging_semantics_of_descendants() || config.is_merge_boundary());
    let forms_node = nested_merger
        || (!context.merge_into_ancestor
            && (context.is_root
                || boundary
                || (contributes && context.parent_requires_explicit_node)));
    let children_require_explicit_node = context.is_root
        || config.explicit_child_nodes()
        || (!contributes && context.parent_requires_explicit_node);
    let children_merge_into_ancestor = (context.merge_into_ancestor && !config.is_merge_boundary())
        || config.is_merging_semantics_of_descendants();

    NodeAssemblyDecisions {
        contributes,
        forms_node,
        children_merge_into_ancestor,
        child_context: SemanticsAssemblyContext {
            is_root: false,
            parent_requires_explicit_node: children_require_explicit_node,
            merge_into_ancestor: children_merge_into_ancestor,
            has_reveal_ancestor: context.has_reveal_ancestor
                || (config.effective_actions_as_bits()
                    & flui_semantics::SemanticsAction::ScrollIntoView.value()
                    != 0
                    && config
                        .action_handler(flui_semantics::SemanticsAction::ScrollIntoView)
                        .is_some()),
        },
    }
}

/// Body of [`build_semantics_fragments`].
fn build_semantics_fragments_impl(
    tree: &RenderTree,
    id: RenderId,
    transform: Matrix4,
    clips: SemanticsClips,
    context: SemanticsAssemblyContext,
    ancestor_blocks_user_actions: bool,
    regions: &mut AssemblyRegions,
) -> Option<SemanticsFragments> {
    #[cfg(test)]
    ASSEMBLY_VISITS.with(|counter| counter.set(counter.get() + 1));

    let node = tree.get(id)?;
    let mut config = describe_semantics_configuration(id, node, regions);
    // The runs regions may hold are the authored ones, transformed or not.
    let authored_runs = config.text_content().map(|content| content.runs().len());
    // Runs are laid out in the declaring render object's space; its node is
    // published in the window's.
    if let Some(content) = config.text_content() {
        match content.transformed(&transform) {
            Some(content) => config.set_text_content(content),
            None => config.clear_text_content(),
        }
    }
    let blocks_user_actions = ancestor_blocks_user_actions || config.blocks_user_actions();
    config.set_blocks_user_actions(blocks_user_actions);
    config.set_has_reveal_ancestor(context.has_reveal_ancestor);
    let (reveal_rect, clipped) = match node_semantics_rect(node, transform) {
        Some(rect) => (rect, clips.apply(rect, context.has_reveal_ancestor)),
        // Refuse this node's unrepresentable bound but still visit its
        // children: a smaller child can have representable projected geometry.
        None => (
            Rect::ZERO,
            ClippedRect {
                rect: Rect::ZERO,
                hidden: false,
                dropped: true,
            },
        ),
    };
    let rect = clipped.rect;
    if clipped.hidden {
        config.set_hidden(true);
    }

    let decisions = assembly_decisions(&config, context);

    regions.visited.insert(id);
    let authored = match node {
        RenderNode::Box(entry) => entry
            .render_object()
            .semantics_regions(entry.state().geometry().unwrap_or(Size::ZERO)),
        RenderNode::Sliver(entry) => entry
            .render_object()
            .semantics_regions(entry.state().absolute_paint_size()),
    };
    for region in authored
        .before_children
        .iter()
        .chain(&authored.after_children)
    {
        regions.callbacks.retain(&region.config);
    }
    debug_assert_hosted_runs(authored_runs, &authored);

    let mut child_fragments = Vec::with_capacity(node.children().len());
    let mut children_block_previous = append_region_fragments(
        &mut child_fragments,
        authored.before_children,
        id,
        RegionPlacement::BeforeChildren,
        transform,
        clips,
        decisions.child_context,
        blocks_user_actions,
        regions,
    );
    if !node.excludes_semantics_subtree() {
        // The generation this node's last layout stamped onto the children it
        // laid out; anything else was not part of that pass.
        let parent_generation = node.layout_generation();
        for (child_slot, &child_id) in node.children().iter().enumerate() {
            let Some(child) = tree.get(child_id) else {
                continue;
            };
            // Skip a child this pass did not lay out, as paint and hit-test
            // do. Its rect describes a pass that no longer holds, and a screen
            // reader sent to it lands somewhere with nothing on it — worse
            // than not announcing the row at all.
            //
            // Announcing it would also make the accessibility tree disagree
            // with the two walks that already skip it.
            if !child.was_placed_by(id, parent_generation) {
                continue;
            }
            // A child the parent structurally does not present — `RenderTheater`'s
            // entries beneath the topmost opaque one — is dropped regardless of
            // layout history. The stamp above cannot do this: it excludes a
            // child a parent STOPPED laying out, so a child skipped from its
            // very first pass was never stamped and reads as placed.
            if !visits_child_for_semantics(node, child_slot) {
                continue;
            }
            let Some(child_transform) = child_transform(node, child, child_slot, transform) else {
                continue;
            };
            let (local_paint, local_semantics) = child_clips_of(node, transform, child_slot);
            let fragments = build_semantics_fragments(
                tree,
                child_id,
                child_transform,
                clips.descend(local_paint, local_semantics),
                decisions.child_context,
                blocks_user_actions,
                regions,
            )
            .unwrap_or_default();
            if fragments.blocks_previous_siblings {
                // Every handler in discarded drafts is pinned by AssemblyCallbacks
                // until phase completion, outside the pipeline borrow.
                child_fragments.clear();
                children_block_previous = true;
            }
            child_fragments.extend(fragments.nodes);
        }
    }
    children_block_previous |= append_region_fragments(
        &mut child_fragments,
        authored.after_children,
        id,
        RegionPlacement::AfterChildren,
        transform,
        clips,
        decisions.child_context,
        blocks_user_actions,
        regions,
    );

    let blocks_previous_siblings =
        config.blocks_previous_siblings() || (!decisions.forms_node && children_block_previous);

    // A node the semantics clip leaves nothing of publishes no node, but its
    // children have already been walked under their own clips above.
    if !decisions.contributes || clipped.dropped {
        // The stamped node forms no semantics node of its own — a `Padding` or
        // an `Align` at the item's root, which is what a builder most often
        // returns. Its position would be lost here, so it is handed to the
        // fragments travelling up instead: whichever of them ends up carrying
        // the row's label carries its position too.
        //
        // Skipped when the node is dropped rather than merely transparent: a
        // clipped-away row has no position to announce.
        if !clipped.dropped {
            offer_semantic_index_to_fragments(node, tree.parent(id), &mut child_fragments);
        }
        return Some(SemanticsFragments {
            nodes: child_fragments,
            blocks_previous_siblings,
        });
    }

    let children = merge_child_fragments(
        &mut config,
        child_fragments,
        decisions.children_merge_into_ancestor,
    );
    // AFTER the fold, so this is a genuine fallback. An `IndexedSemantics` on
    // the item is a NON-boundary configuration absorbed by its nearest ancestor
    // boundary — the row — which happens in the merge above. Applying the
    // stamped index before that ran would make the stamp win, since `absorb`
    // does not overwrite a value the parent already holds, and hand-indexed
    // content inside a lazy list would become impossible.
    apply_lazy_child_semantic_index(node, tree.parent(id), &mut config, &children);
    let pending = PendingSemanticsNode {
        source: SemanticsSource::Render(id),
        config,
        rect,
        reveal_rect,
        children,
    };

    Some(SemanticsFragments {
        nodes: vec![if decisions.forms_node {
            SemanticsFragment::Formed(pending.form())
        } else {
            SemanticsFragment::Pending(pending)
        }],
        blocks_previous_siblings,
    })
}

/// Checks that the regions holding a text's runs (ADR-0214 §2 as amended)
/// hold each of its runs once, in run order: their ranges, in region order,
/// are consecutive and cover every run. Regions hold runs only of the text
/// they are drawn by.
fn debug_assert_hosted_runs(authored_runs: Option<usize>, authored: &SemanticsRegions) {
    if !cfg!(debug_assertions) {
        return;
    }
    let hosted: Vec<std::ops::Range<usize>> = authored
        .before_children
        .iter()
        .chain(&authored.after_children)
        .filter_map(|region| region.config.hosted_runs())
        .collect();
    if hosted.is_empty() {
        return;
    }
    let runs = authored_runs.expect("BUG: regions hold runs of a text that declares none");
    let mut next = 0;
    for range in &hosted {
        debug_assert_eq!(
            range.start, next,
            "BUG: hosted run ranges overlap, skip runs or leave run order: {hosted:?}"
        );
        next = range.end;
    }
    debug_assert_eq!(
        next, runs,
        "BUG: hosted run ranges do not cover the text's {runs} runs: {hosted:?}"
    );
}

#[expect(
    clippy::too_many_arguments,
    reason = "uses the same assembly context as real child fragments"
)]
fn append_region_fragments(
    fragments: &mut Vec<SemanticsFragment>,
    authored: Vec<flui_semantics::SemanticsRegion>,
    host: RenderId,
    placement: RegionPlacement,
    transform: Matrix4,
    clips: SemanticsClips,
    context: SemanticsAssemblyContext,
    blocks_user_actions: bool,
    regions: &mut AssemblyRegions,
) -> bool {
    let mut blocks_previous = false;
    for region in authored {
        if region
            .config
            .sort_key()
            .is_some_and(|key| !key.order.is_finite())
        {
            regions.invalid_sort_key = Some(host);
        }
        let source = RegionSource {
            host,
            placement,
            key: region.key,
        };
        if !regions.authored.insert(source) {
            regions.duplicate = Some(source);
        }
        let Some(reveal_rect) = transformed_rect(transform * region.transform, region.rect) else {
            continue;
        };
        let clipped = clips.apply(reveal_rect, context.has_reveal_ancestor);
        if clipped.dropped {
            continue;
        }
        let mut config = region.config;
        if config.blocks_previous_siblings() {
            fragments.clear();
            blocks_previous = true;
        }
        config.set_semantics_boundary(true);
        config.set_blocks_user_actions(blocks_user_actions || config.blocks_user_actions());
        config.set_has_reveal_ancestor(context.has_reveal_ancestor);
        if clipped.hidden {
            config.set_hidden(true);
        }
        let decisions = assembly_decisions(&config, context);
        let pending = PendingSemanticsNode {
            source: SemanticsSource::Region(source),
            config,
            rect: clipped.rect,
            reveal_rect,
            children: Vec::new(),
        };
        fragments.push(if decisions.forms_node {
            SemanticsFragment::Formed(pending.form())
        } else {
            SemanticsFragment::Pending(pending)
        });
    }
    blocks_previous
}

fn merge_child_fragments(
    config: &mut SemanticsConfiguration,
    mut fragments: Vec<SemanticsFragment>,
    suppress_conflicts: bool,
) -> Vec<BuiltSemanticsNode> {
    // Stable sorting preserves declarative order for equal keys and unkeyed
    // siblings. Sort before folding, so merged labels follow reading order too.
    fragments.sort_by(|a, b| {
        fn key(fragment: &SemanticsFragment) -> Option<&flui_semantics::SemanticsSortKey> {
            match fragment {
                SemanticsFragment::Formed(node) => node.config.sort_key(),
                SemanticsFragment::Pending(node) => node.config.sort_key(),
            }
        }
        by_sort_key(key(a), key(b))
    });
    let conflicts = (!suppress_conflicts).then(|| mark_configuration_conflicts(config, &fragments));
    let mut children = Vec::with_capacity(fragments.len());

    for (index, fragment) in fragments.into_iter().enumerate() {
        match fragment {
            SemanticsFragment::Formed(node) => children.push(node),
            // Runs are the synthetic children of the one node declaring them,
            // so a merge never drops a second text's runs for the first's:
            // that text keeps its node and its runs (ADR-0214 §2).
            SemanticsFragment::Pending(pending)
                if conflicts.as_ref().is_some_and(|conflicts| conflicts[index])
                    || (pending.config.text_content().is_some()
                        && config.text_content().is_some()) =>
            {
                children.push(pending.form());
            }
            SemanticsFragment::Pending(pending) => {
                config.absorb(&pending.config);
                children.extend(pending.children);
            }
        }
    }

    if config.makes_document_selections() {
        in_reading_order(&mut children, config.text_direction().unwrap_or_default());
    }
    children
}

/// Keyed siblings first, by name and then order; unkeyed ones after, as
/// they are.
fn by_sort_key(
    a: Option<&flui_semantics::SemanticsSortKey>,
    b: Option<&flui_semantics::SemanticsSortKey>,
) -> std::cmp::Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.name.cmp(&b.name).then_with(|| {
            if a.order == b.order {
                std::cmp::Ordering::Equal
            } else {
                a.order.total_cmp(&b.order)
            }
        }),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

/// Orders a selectable document's children as a reader takes them: the keyed
/// ones first by their keys, then the rest by where they are laid out
/// ([`flui_semantics::reading_order`]), so the text a selection reports is
/// the text the pointer selects. The children of folded-in nodes are ordered
/// with their new siblings.
fn in_reading_order(children: &mut Vec<BuiltSemanticsNode>, direction: TextDirection) {
    children.sort_by(|a, b| by_sort_key(a.config.sort_key(), b.config.sort_key()));
    let keyed = children
        .iter()
        .take_while(|child| child.config.sort_key().is_some())
        .count();
    let unkeyed = children.split_off(keyed);
    let boxes: Vec<Rect<f64>> = unkeyed.iter().map(|child| child.reveal_rect).collect();
    let mut slots: Vec<Option<BuiltSemanticsNode>> = unkeyed.into_iter().map(Some).collect();
    children.extend(
        flui_semantics::reading_order(&boxes, direction)
            .into_iter()
            .filter_map(|index| slots[index].take()),
    );
}

fn mark_configuration_conflicts(
    parent: &SemanticsConfiguration,
    fragments: &[SemanticsFragment],
) -> Vec<bool> {
    let mut conflicts = vec![false; fragments.len()];

    for (index, fragment) in fragments.iter().enumerate() {
        let SemanticsFragment::Pending(pending) = fragment else {
            continue;
        };

        if !parent.is_compatible_with(&pending.config) {
            conflicts[index] = true;
        }

        for sibling_index in 0..index {
            let SemanticsFragment::Pending(sibling) = &fragments[sibling_index] else {
                continue;
            };
            if !pending.config.is_compatible_with(&sibling.config) {
                conflicts[index] = true;
                conflicts[sibling_index] = true;
            }
        }
    }

    conflicts
}

fn rebuild_semantics_owner(
    owner: &mut SemanticsOwner,
    root: Option<BuiltSemanticsNode>,
    regions: &RegionIdentities,
) {
    owner.clear();

    let Some(root) = root else {
        return;
    };

    let root_id = insert_built_semantics_node(owner, root, regions);
    owner.set_root(Some(root_id));
}

/// Splits an assembled node into its arena representation and its children.
fn semantics_node_parts(
    built: BuiltSemanticsNode,
    regions: &RegionIdentities,
) -> (SemanticsNode, Vec<BuiltSemanticsNode>) {
    let mut node = match built.source {
        SemanticsSource::Render(id) => SemanticsNode::new().with_source_render_id(id),
        SemanticsSource::Region(source) => SemanticsNode::new().with_region_id(
            *regions
                .retained
                .get(&source)
                .expect("BUG: region identities are reserved before tree mutation"),
        ),
    }
    .with_config(built.config);
    node.set_rect(built.rect);
    node.set_reveal_rect(built.reveal_rect);
    (node, built.children)
}

fn insert_built_semantics_node(
    owner: &mut SemanticsOwner,
    built: BuiltSemanticsNode,
    regions: &RegionIdentities,
) -> flui_foundation::SemanticsId {
    let (node, children) = semantics_node_parts(built, regions);
    let id = owner.insert(node);
    for child in children {
        let child_id = insert_built_semantics_node(owner, child, regions);
        owner.add_child(id, child_id);
    }
    id
}

// ============================================================================
// Mark-scoped graft (see the module doc for the model and its guarantees)
// ============================================================================

/// Plans all marked subtrees without mutating the persistent arena. If any
/// precondition fails the caller discards this plan and assembles the full tree.
fn plan_graft_pass(
    tree: &RenderTree,
    pipeline_root: Option<RenderId>,
    owner: &SemanticsOwner,
    marked: &[RenderId],
    regions: &mut AssemblyRegions,
) -> Option<Vec<(flui_foundation::SemanticsId, BuiltSemanticsNode)>> {
    let pipeline_root = pipeline_root?;
    let marked_set: FxHashSet<RenderId> = marked.iter().copied().collect();

    // Resolve every mark to its anchor. A mark whose ancestor chain holds
    // no anchored formed node (the root itself is marked, or the region
    // never published) forces the full rebuild.
    let mut anchors: FxHashSet<RenderId> = FxHashSet::default();
    for &mark in &marked_set {
        let anchor = anchor_for(tree, owner.tree(), &marked_set, mark)?;
        anchors.insert(anchor);
    }

    // Keep only top-most anchors: an anchor inside another anchor's render
    // subtree is fully covered by re-assembling the outer one. After this,
    // every survivor's proper ancestor chain is unmarked (a marked ancestor's
    // own anchor would sit above it and swallow this one), which is what
    // makes the recomputed context and the "nothing above changes" argument
    // in the module doc hold.
    let survivors = anchors.iter().copied().filter(|&anchor| {
        let mut cursor = tree.parent(anchor);
        while let Some(ancestor) = cursor {
            if anchors.contains(&ancestor) {
                return false;
            }
            cursor = tree.parent(ancestor);
        }
        true
    });

    survivors
        .map(|anchor| plan_graft_anchor(tree, pipeline_root, owner, anchor, regions))
        .collect()
}

/// The nearest ancestor of `mark` (strictly above it) that is unmarked and
/// currently anchors a formed semantics node in the arena.
///
/// Strictly above, because a marked node's own config may change what it
/// forms; unmarked, for the same reason one level up. Arena presence is the
/// proof the ancestor formed a node last pass — and since its config and
/// inherited context are unchanged (ancestors unmarked, see the survivor
/// filter), it will form again with the same stable identity.
fn anchor_for(
    tree: &RenderTree,
    arena: &SemanticsTree,
    marked: &FxHashSet<RenderId>,
    mark: RenderId,
) -> Option<RenderId> {
    let mut cursor = tree.parent(mark)?;
    loop {
        if !marked.contains(&cursor)
            && arena
                .find_by_accessibility_id(AccessibilityNodeId::from(cursor))
                .is_some()
        {
            return Some(cursor);
        }
        cursor = tree.parent(cursor)?;
    }
}

/// Recomputes the assembly inputs (context, accumulated transform, inherited
/// action-blocking) the full walk would hand `target`, by folding the
/// ancestor chain root→target through [`assembly_decisions`] — the same
/// rules the walk itself applies, so the graft assembles under exactly the
/// context a full rebuild would have used. Sound because every proper
/// ancestor of a surviving anchor is unmarked: their configs are the ones
/// the last pass already used.
///
/// `None` when the chain is not rooted at the pipeline root (a detached
/// subtree), an ancestor excludes its semantics subtree, or the fold says
/// `target` assembles merged into an ancestor without being a nested merger
/// (which forms its own node there) — each contradicts the
/// anchor's arena presence, so the caller falls back to the full rebuild.
fn assembly_inputs_for(
    tree: &RenderTree,
    pipeline_root: RenderId,
    target: RenderId,
    regions: &mut AssemblyRegions,
) -> Option<(SemanticsAssemblyContext, Matrix4, SemanticsClips, bool)> {
    let mut chain = vec![target];
    let mut cursor = target;
    while let Some(parent) = tree.parent(cursor) {
        chain.push(parent);
        cursor = parent;
    }
    if chain.last() != Some(&pipeline_root) {
        return None;
    }
    chain.reverse();

    let mut context = SemanticsAssemblyContext {
        is_root: true,
        parent_requires_explicit_node: false,
        merge_into_ancestor: false,
        has_reveal_ancestor: false,
    };
    let mut blocks_user_actions = false;
    let mut transform = Matrix4::IDENTITY;
    let mut clips = SemanticsClips::default();

    for (index, &id) in chain.iter().enumerate() {
        let node = tree.get(id)?;
        if id == target {
            // Inside a merge only a nested merger forms its own node; any
            // other target there contradicts its arena presence.
            if context.merge_into_ancestor
                && !assembly_decisions(
                    &describe_semantics_configuration(id, node, regions),
                    context,
                )
                .forms_node
            {
                return None;
            }
            return Some((context, transform, clips, blocks_user_actions));
        }

        let mut config = describe_semantics_configuration(id, node, regions);
        blocks_user_actions = blocks_user_actions || config.blocks_user_actions();
        config.set_blocks_user_actions(blocks_user_actions);
        if node.excludes_semantics_subtree() {
            return None;
        }
        // The clip this node imposes on the chain's next link. The child's
        // slot is its position in this node's children — the same index the
        // full walk enumerates, so the two folds agree by construction.
        let child_slot = node
            .children()
            .iter()
            .position(|&child| child == chain[index + 1])?;
        let (local_paint, local_semantics) = child_clips_of(node, transform, child_slot);
        clips = clips.descend(local_paint, local_semantics);
        transform = child_transform(node, tree.get(chain[index + 1])?, child_slot, transform)?;
        context = assembly_decisions(&config, context).child_context;
    }

    // The chain always contains `target`, so the loop returns before
    // exhausting it.
    None
}

/// Re-assembles `anchor`'s render subtree and grafts the result into the
/// arena in place of the anchor's previous subtree: the anchor's arena slot
/// (and thus its parent's children list and its published identity) is
/// preserved; everything below is replaced.
fn plan_graft_anchor(
    tree: &RenderTree,
    pipeline_root: RenderId,
    owner: &SemanticsOwner,
    anchor: RenderId,
    regions: &mut AssemblyRegions,
) -> Option<(flui_foundation::SemanticsId, BuiltSemanticsNode)> {
    let anchor_sid = owner
        .tree()
        .find_by_accessibility_id(AccessibilityNodeId::from(anchor))?;
    if collection_affects_graft(owner.tree(), anchor_sid) {
        return None;
    }
    let (context, transform, clips, blocks_user_actions) =
        assembly_inputs_for(tree, pipeline_root, anchor, regions)?;
    let fragments = build_semantics_fragments(
        tree,
        anchor,
        transform,
        clips,
        context,
        blocks_user_actions,
        regions,
    )?;
    if fragments.blocks_previous_siblings {
        return None;
    }
    let mut fragments = fragments.nodes;
    // The anchor formed a node last pass under this exact context and
    // config, so it must form exactly one again; anything else means an
    // assumption broke and the full rebuild is the honest answer.
    let built = match (fragments.pop(), fragments.is_empty()) {
        (Some(SemanticsFragment::Formed(built)), true)
            if built.source == SemanticsSource::Render(anchor) =>
        {
            built
        }
        _ => return None,
    };

    if has_collection_metadata(&built) {
        return None;
    }
    // A changed key affects siblings outside this graft. Rebuild from the
    // declarative tree to restore their source order before a stable re-sort.
    if owner.tree().get(anchor_sid)?.config().sort_key() != built.config.sort_key() {
        return None;
    }
    Some((anchor_sid, built))
}

/// A collection change can affect a counted ancestor outside the graft. Inspect
/// only this anchor's ancestors and replaced subtree, not unrelated branches.
fn collection_affects_graft(tree: &SemanticsTree, anchor: flui_foundation::SemanticsId) -> bool {
    let mut seen = FxHashSet::default();
    let mut cursor = Some(anchor);
    while let Some(id) = cursor {
        if !seen.insert(id) {
            return true;
        }
        let Some(node) = tree.get(id) else {
            return true;
        };
        if declares_collection_scope(node.config()) {
            return true;
        }
        cursor = node.parent();
    }
    seen.clear();
    let mut pending = vec![anchor];
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            return true;
        }
        let Some(node) = tree.get(id) else {
            return true;
        };
        if declares_collection_scope(node.config()) {
            return true;
        }
        pending.extend(node.children());
    }
    false
}

fn apply_graft(
    owner: &mut SemanticsOwner,
    anchor_sid: flui_foundation::SemanticsId,
    built: BuiltSemanticsNode,
    regions: &RegionIdentities,
) {
    let old_children: Vec<flui_foundation::SemanticsId> = owner
        .tree()
        .children(anchor_sid)
        .map_or_default(<[flui_foundation::SemanticsId]>::to_vec);
    for child in old_children {
        let _ = owner.tree_mut().remove(child);
    }

    let (node, children) = semantics_node_parts(built, regions);
    assert!(
        owner.tree_mut().replace_node(anchor_sid, node),
        "BUG: a staged graft retains its anchor slot"
    );
    for child in children {
        let child_sid = insert_built_semantics_node(owner, child, regions);
        owner.add_child(anchor_sid, child_sid);
    }
}

fn describe_semantics_configuration(
    id: RenderId,
    node: &RenderNode,
    regions: &mut AssemblyRegions,
) -> SemanticsConfiguration {
    let mut config = SemanticsConfiguration::new();
    match node {
        RenderNode::Box(entry) => {
            entry
                .render_object()
                .describe_semantics_configuration(&mut config);
        }
        RenderNode::Sliver(entry) => {
            entry
                .render_object()
                .describe_semantics_configuration(&mut config);
        }
    }
    regions.callbacks.retain(&config);
    if config.sort_key().is_some_and(|key| !key.order.is_finite()) {
        regions.invalid_sort_key = Some(id);
    }
    config
}

/// The configuration a fragment carries, formed or not.
fn fragment_config(fragment: &mut SemanticsFragment) -> &mut SemanticsConfiguration {
    match fragment {
        SemanticsFragment::Pending(pending) => &mut pending.config,
        SemanticsFragment::Formed(formed) => &mut formed.config,
    }
}

/// Whether one logical item already declares its place among the members of
/// its set, at any depth: an explicit `IndexedSemantics` absorbed into a
/// descendant boundary is that item's member as much as one folded into its
/// root. A nested set's members belong to that set, so the search stops at a
/// node that declares a set of its own.
fn item_declares_position(
    config: &SemanticsConfiguration,
    children: &[BuiltSemanticsNode],
) -> bool {
    fn within(node: &BuiltSemanticsNode) -> bool {
        node.config.position_in_set().is_some()
            || (node.config.size_of_set().is_none() && node.children.iter().any(within))
    }
    config.position_in_set().is_some()
        || (config.size_of_set().is_none() && children.iter().any(within))
}

/// Record the position `node`'s host stamped, as that host's member.
fn stamp_position(host: Option<RenderId>, config: &mut SemanticsConfiguration, index: usize) {
    match host {
        Some(host) => config.set_collection_index(host, index),
        None => config.set_position_in_set(index),
    }
}

/// Hand a transparent stamped node's position down to the fragments it
/// forwards, so the node that does form for this item carries it.
///
/// The stamped node forms no semantics node of its own — a `Padding` or an
/// `Align` at the item's root, which is what a builder most often returns —
/// so its position would be lost where it is. One logical item is one member:
/// a position already declared anywhere in it is that member's, and otherwise
/// only the first fragment carries it, so a row that forms two controls does
/// not announce both as "item 1".
fn offer_semantic_index_to_fragments(
    node: &RenderNode,
    host: Option<RenderId>,
    fragments: &mut [SemanticsFragment],
) {
    let Some(index) = stamped_sliver_parent_data(node).and_then(|pd| pd.semantic_index) else {
        return;
    };
    let declared = fragments.iter().any(|fragment| match fragment {
        SemanticsFragment::Pending(pending) => {
            item_declares_position(&pending.config, &pending.children)
        }
        SemanticsFragment::Formed(formed) => {
            item_declares_position(&formed.config, &formed.children)
        }
    });
    if declared {
        return;
    }
    if let Some(first) = fragments.first_mut() {
        stamp_position(host, fragment_config(first), index);
    }
}

/// The lazy-sliver slot a host stamped into this node's parent data.
fn stamped_sliver_parent_data(
    node: &RenderNode,
) -> Option<&crate::parent_data::SliverMultiBoxAdaptorParentData> {
    node.parent_data()
        .and_then(|pd| pd.downcast_ref::<crate::parent_data::SliverMultiBoxAdaptorParentData>())
}

/// Publish a lazy sliver child's position in the set, from the index its host
/// stamped rather than from a wrapper widget.
///
/// A screen reader's "item 12 of 100" needs the 12. Wrapping every
/// materialised item in an indexing render node would cost a node per item,
/// carrying an index captured when the item was built. The sliver instead
/// already stamps each child's slot into its parent data and keeps it in step
/// with the row's real position as the band moves, so reading it here costs
/// no node and cannot go stale against the row it describes.
///
/// Applied AFTER the fold, so this is a genuine fallback: an explicit
/// `IndexedSemantics` anywhere in the item, folded into the row or formed as a
/// descendant boundary, keeps its own position and the stamp is not applied.
/// Explicit numbering belongs to an authored set, not to the host's, so
/// [`reconcile_collections`] withholds the host's total rather than pairing
/// the two.
///
/// A `semantic_index` of `None` publishes nothing — the child occupies a
/// logical index without being a member of the set, which is what a separator
/// is.
fn apply_lazy_child_semantic_index(
    node: &RenderNode,
    host: Option<RenderId>,
    config: &mut SemanticsConfiguration,
    children: &[BuiltSemanticsNode],
) {
    let Some(index) = stamped_sliver_parent_data(node).and_then(|pd| pd.semantic_index) else {
        return;
    };
    if !item_declares_position(config, children) {
        stamp_position(host, config, index);
    }
}

fn node_semantics_rect(node: &RenderNode, transform: Matrix4) -> Option<Rect<f64>> {
    let size = match node {
        RenderNode::Box(entry) => entry.state().geometry().unwrap_or(Size::ZERO),
        RenderNode::Sliver(entry) => entry.state().absolute_paint_size(),
    };
    transformed_rect(transform, Rect::from_origin_size(Point::ZERO, size))
}

/// Compose the same committed mapping used by paint and coordinate queries.
/// Full assembly and graft reconstruction both use this edge operation.
fn child_transform(
    parent: &RenderNode,
    child: &RenderNode,
    slot: usize,
    mut transform: Matrix4,
) -> Option<Matrix4> {
    parent.apply_paint_transform(slot, child.offset(), &mut transform)?;
    transform
        .to_col_major_array()
        .iter()
        .all(|value| value.is_finite())
        .then_some(transform)
}

fn transformed_rect(transform: Matrix4, rect: Rect<f64>) -> Option<Rect<f64>> {
    // Validate each corner before min/max: floating-point min/max can conceal
    // a NaN corner, and finite matrix entries can overflow during projection.
    for point in [
        rect.min,
        Point::new(rect.max.x, rect.min.y),
        rect.max,
        Point::new(rect.min.x, rect.max.y),
    ] {
        let (x, y) = transform.transform_point(point.x, point.y);
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
    }
    let bounds = transform.transform_rect(&rect);
    (bounds.width().is_finite() && bounds.height().is_finite()).then_some(bounds)
}
