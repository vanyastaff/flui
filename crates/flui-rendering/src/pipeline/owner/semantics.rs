//! Semantics phase implementation: the body of `FramePass<Semantics>::run_semantics`.
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

use flui_foundation::arena::{FrameArena, FrameVec};
use flui_foundation::geometry::{Matrix4, Point, Rect, Size};
use flui_foundation::{RenderId, TextDirection};
use flui_semantics::{
    SemanticsConfiguration, SemanticsNode, SemanticsOwner, SemanticsRegions, SemanticsTree,
};
use rustc_hash::FxHashSet;

use crate::{
    error::SemanticsError,
    pipeline::scheduler::PhaseKind,
    storage::{RenderNode, RenderTree},
};

use super::{PipelineOwner, subtree_arena::ensure_stack};

mod clips;
mod collections;
mod graft;
mod regions;

use clips::{ClippedRect, SemanticsClips, child_clips_of, visits_child_for_semantics};
use collections::reconcile_collections;
#[cfg(any(test, feature = "testing"))]
use graft::check_placement_against_rebuild;
use graft::{
    apply_graft, clear_graft_anchors, placement_anchors, placement_keeps_order, plan_graft_pass,
};
pub(super) use regions::RegionIdentities;
use regions::{AssemblyCallbacks, AssemblyRegions, RegionPlacement, RegionSource, SemanticsSource};

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

impl PipelineOwner {
    /// Updates semantics for all dirty render objects.
    ///
    /// This is phase 4 of the rendering pipeline. During semantics:
    /// - Accessibility information is gathered
    /// - Semantics tree is updated
    ///
    /// Nodes are sorted by depth (shallow first) for top-down traversal.
    /// The geometries of children depend on ancestors' transforms and clips,
    /// so parents must be processed first.
    pub(super) fn flush_semantics(&mut self) -> crate::error::RenderResult<()> {
        if !self.semantics_enabled() {
            return Ok(());
        }

        let _span = tracing::debug_span!(
            target: "flui.frame",
            "semantics",
            dirty_nodes = self.scheduler.semantics_queue_len(),
        )
        .entered();

        self.scheduler.enter_phase(PhaseKind::Semantics);
        // Assembly runs authored code (configurations, Flow delegates). Every
        // return leaves the phase itself; an unwind leaves it here, so a host
        // that catches the panic keeps an owner routing marks normally.
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: semantics assembly runs authored code; the phase is left on a panic"
        )]
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

    /// The body of [`FramePass::run_semantics`](super::FramePass::run_semantics),
    /// entered with the phase set.
    fn run_semantics_in_phase(&mut self) -> crate::error::RenderResult<()> {
        // The last pass's scratch lists and built nodes are gone with its regions; this one
        // starts empty.
        self.frame_arena.reset();
        self.built_arena.reset();
        let mut callbacks = AssemblyCallbacks::default();
        // The rebuild check's own retained handlers, retired with the pass's.
        #[cfg_attr(
            not(any(test, feature = "testing")),
            expect(unused_mut, reason = "only the testing build runs the rebuild check")
        )]
        let mut check_callbacks: Option<AssemblyCallbacks> = None;

        // Sort shallow-first. Roots dispatch before their descendants so a parent's config is
        // assembled before children fold into it.
        self.scheduler.sort_semantics_shallow_first();

        // The pass's id lists live between passes, so a frame reuses them.
        let mut marked = std::mem::take(&mut self.semantics_regions.marked);
        marked.clear();
        marked.extend(
            self.scheduler
                .nodes_needing_semantics()
                .iter()
                .map(|d| d.id)
                .filter(|&id| self.render_tree.contains(id)),
        );
        let mut placed = std::mem::take(&mut self.semantics_regions.placed);
        placed.clear();
        let mut walk = std::mem::take(&mut self.semantics_regions.placement_walk);
        let pending_count = marked.len();
        let moved_count = self.scheduler.nodes_needing_semantics_geometry().len();

        let tree_is_empty = self
            .semantics_owner
            .as_ref()
            .is_some_and(|owner| owner.tree().is_empty());
        let should_build = pending_count > 0 || moved_count > 0 || tree_is_empty;

        if should_build {
            if let Some(owner) = self.semantics_owner.as_mut() {
                // Stage every anchor and all authored key scopes before reserving
                // identities or touching the current arena. A failed later anchor
                // cannot partially commit an earlier subtree.
                let mut regions = AssemblyRegions::reusing(
                    &self.frame_arena,
                    &self.built_arena,
                    std::mem::take(&mut self.semantics_regions.visited),
                );
                if !tree_is_empty {
                    // A moved subtree re-assembles the semantics nodes inside it,
                    // each in its own slot. One the walk cannot place that way is
                    // an ordinary mark: its anchor above re-assembles it.
                    for moved in self.scheduler.nodes_needing_semantics_geometry() {
                        if !self.render_tree.contains(moved.id) {
                            continue;
                        }
                        let first = placed.len();
                        let placeable = placement_anchors(
                            &self.render_tree,
                            self.root_id,
                            owner.tree(),
                            moved.id,
                            &mut walk,
                            &mut placed,
                            &mut regions,
                        ) && placed[first..]
                            .iter()
                            .all(|&anchor| placement_keeps_order(owner.tree(), anchor));
                        if !placeable {
                            placed.truncate(first);
                            marked.push(moved.id);
                        }
                    }
                }
                let nothing_moved = !tree_is_empty && marked.is_empty() && placed.is_empty();
                let grafts = (!tree_is_empty && !nothing_moved)
                    .then(|| {
                        plan_graft_pass(
                            &self.render_tree,
                            self.root_id,
                            owner,
                            self.semantics_regions
                                .containment_known
                                .then_some(&self.semantics_regions.uncontained),
                            &marked,
                            &placed,
                            &mut regions,
                        )
                    })
                    .flatten();
                if nothing_moved {
                    // Every mark was a move whose subtree holds no semantics:
                    // the published tree is already what a rebuild would give.
                    if owner.needs_flush() {
                        let published = owner.flush();
                        self.counters.semantics_nodes_updated += published as u64;
                    }
                    // A flush records what it found wrong; report it this
                    // frame, as the assembling branch does.
                    let diagnostics = owner.take_diagnostics();
                    #[cfg(any(test, feature = "testing"))]
                    self.unchecked_semantics_diagnostics
                        .extend(diagnostics.iter().cloned());
                    self.semantics_diagnostics.extend(diagnostics);
                    callbacks = std::mem::take(&mut regions.callbacks);
                    self.semantics_regions.visited = std::mem::take(&mut regions.visited);
                } else {
                    let planned = if let Some(grafts) = grafts {
                        Ok(SemanticsPlan::Graft(grafts))
                    } else {
                        // A failed graft proposal can discard configurations, but
                        // their handlers remain owned until the whole pass verdict.
                        regions.visited.clear();
                        regions.authored.clear();
                        regions.duplicate = None;
                        regions.invalid_sort_key = None;
                        assemble_semantics_root(&self.render_tree, self.root_id, &mut regions).map(
                            |root| {
                                SemanticsPlan::Rebuild(
                                    root.map(|root| Box::new_in(root, &self.built_arena)),
                                )
                            },
                        )
                    };
                    let mut planned = match planned {
                        Ok(plan) => plan,
                        Err(error) => {
                            regions.callbacks.rejected = true;
                            let _ = self.scheduler.exit_phase(PhaseKind::Semantics);
                            return Err(crate::error::RenderError::Semantics(error));
                        }
                    };
                    if let SemanticsPlan::Rebuild(Some(root)) = &mut planned {
                        reconcile_collections(root);
                    }
                    // Exit the phase before rejected configurations retire their
                    // callback captures. Their destruction is authored code too.
                    if let Err(error) = self.semantics_regions.prepare(&regions, &self.render_tree)
                    {
                        regions.callbacks.rejected = true;
                        let _ = self.scheduler.exit_phase(PhaseKind::Semantics);
                        retire_rejected_plan(planned);
                        return Err(crate::error::RenderError::Semantics(error));
                    }
                    self.semantics_regions
                        .assign_text_runs(&mut planned, &self.render_tree);
                    let mut uncontained = std::mem::take(&mut self.semantics_regions.uncontained);
                    // Removing replaced nodes drops their handlers' captures, which is
                    // authored code: a panic there leaves the table unknown, so the
                    // next pass assembles afresh. A graft is planned only over a
                    // known table, so both plans leave it known.
                    self.semantics_regions.containment_known = false;
                    match planned {
                        SemanticsPlan::Rebuild(built) => {
                            rebuild_semantics_owner(
                                owner,
                                built.map(|node| *node),
                                &self.semantics_regions,
                                &mut uncontained,
                            );
                        }
                        SemanticsPlan::Graft(grafts) => {
                            clear_graft_anchors(owner, &grafts, &mut uncontained);
                            for (anchor, built) in grafts {
                                apply_graft(
                                    owner,
                                    anchor,
                                    built,
                                    &self.semantics_regions,
                                    &mut uncontained,
                                );
                            }
                        }
                    }
                    self.semantics_regions.uncontained = uncontained;
                    self.semantics_regions.containment_known = true;
                    let published = owner.flush();
                    let diagnostics = owner.take_diagnostics();
                    #[cfg(any(test, feature = "testing"))]
                    self.unchecked_semantics_diagnostics
                        .extend(diagnostics.iter().cloned());
                    self.semantics_diagnostics.extend(diagnostics);
                    self.counters.semantics_nodes_updated += published as u64;
                    callbacks = std::mem::take(&mut regions.callbacks);
                    self.semantics_regions.visited = std::mem::take(&mut regions.visited);
                }
                #[cfg(any(test, feature = "testing"))]
                if moved_count > 0 && self.semantics_regions.checks_placement() {
                    check_callbacks = Some(check_placement_against_rebuild(
                        &self.render_tree,
                        self.root_id,
                        owner,
                        &mut self.semantics_regions,
                    ));
                }
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
            // A full publish checks the whole tree: report what it found this
            // frame, before the owner can be replaced or disposed.
            let diagnostics = owner.take_diagnostics();
            #[cfg(any(test, feature = "testing"))]
            self.unchecked_semantics_diagnostics
                .extend(diagnostics.iter().cloned());
            self.semantics_diagnostics.extend(diagnostics);
        } else if pending_count > 0 {
            tracing::warn!(
                count = pending_count,
                "run_semantics: semantics update requested but no rebuild was required"
            );
        }

        self.semantics_regions.marked = marked;
        self.semantics_regions.placed = placed;
        self.semantics_regions.placement_walk = walk;

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
        drop(check_callbacks);

        Ok(())
    }
}

struct BuiltSemanticsNode<'f> {
    source: SemanticsSource,
    config: SemanticsConfiguration,
    rect: Rect<f64>,
    reveal_rect: Rect<f64>,
    /// A child past `rect` is off screen: see [`PendingSemanticsNode::form`].
    clips_children: bool,
    /// Everything below the node, merged content included, lies inside the
    /// rect of the node above it: see [`PendingSemanticsNode::form`].
    contained: bool,
    /// From the owner's built-node arena: the pass moves the nodes into the semantics owner and
    /// drops the list.
    children: FrameVec<'f, BuiltSemanticsNode<'f>>,
}

enum SemanticsPlan<'f> {
    Rebuild(Option<Box<BuiltSemanticsNode<'f>, &'f FrameArena>>),
    Graft(FrameVec<'f, (flui_foundation::SemanticsId, BuiltSemanticsNode<'f>)>),
}

/// Release draft metadata while the validation error retains first-failure
/// authority. Configuration callbacks are opaque ownership (ADR-0127).
fn retire_rejected_plan(plan: SemanticsPlan<'_>) {
    let mut nodes: Vec<BuiltSemanticsNode<'_>> = match plan {
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

struct PendingSemanticsNode<'f> {
    source: SemanticsSource,
    config: SemanticsConfiguration,
    rect: Rect<f64>,
    reveal_rect: Rect<f64>,
    /// Each child of the node, and so everything below it, lies under a paint
    /// clip inside its rect.
    clip_inside: bool,
    children: FrameVec<'f, BuiltSemanticsNode<'f>>,
    /// The content merged into the node, which has no node of its own, lies
    /// inside its rect.
    absorbed_inside: bool,
    /// The clips hide this node's own rect. The hidden flag belongs to the
    /// node it forms, not to its configuration: merged into an ancestor, it
    /// says nothing about the ancestor, whose own rect decides.
    hidden_by_clip: bool,
}

impl<'f> PendingSemanticsNode<'f> {
    /// The node as published. It clips its children when everything below it
    /// lies under a paint clip inside its rect and each child has an area and is
    /// contained: everything below the child, at any depth, merged content
    /// included, lies inside the rect of the node above it. That is a
    /// scrollable's content past its viewport, which Flutter marks
    /// `isHidden`, and AccessKit's filter leaves it out of navigation except
    /// the first child past each edge, which a reader moves to in order to
    /// scroll it in.
    ///
    /// The clip of the slot of the node a descendant sits under holds it, as
    /// clips only narrow on the way down. AccessKit leaves out the whole
    /// subtree of a node whose bounds miss its clipping parent's, and it
    /// judges a node against its nearest ancestor that its filter keeps,
    /// looking through generic containers. Containment makes any node it
    /// leaves out lie, with everything below it, outside the node's rect,
    /// which holds the clip, so nothing it leaves out is painted, however the
    /// filter flattens the tree. Content laid out in a row past the edge but
    /// translated back into view, or text a row merges that overflows it,
    /// keeps the flag off. So does a list that fills only part of its
    /// viewport (under a header, inside padding), whose rect is smaller than
    /// the viewport's clip
    /// ([gaps](../../../../../docs/gaps.md#a-list-that-fills-part-of-its-viewport-publishes-its-whole-cache-to-a-screen-reader)).
    ///
    /// A child with an empty or non-finite rect keeps the flag off: AccessKit
    /// counts a child with no area as outside its clipping parent and drops
    /// it unless a sibling next to it lies inside.
    fn form(mut self) -> BuiltSemanticsNode<'f> {
        if self.hidden_by_clip {
            self.config.set_hidden(true);
        }
        let contained = self.absorbed_inside
            && self.children.iter().all(|child| {
                has_area(&child.rect) && inside(&child.rect, &self.rect) && child.contained
            });
        let clips_children = self.clip_inside
            && !self.children.is_empty()
            && self
                .children
                .iter()
                .all(|child| has_area(&child.rect) && child.contained);
        BuiltSemanticsNode {
            source: self.source,
            config: self.config,
            rect: self.rect,
            reveal_rect: self.reveal_rect,
            clips_children,
            contained,
            children: self.children,
        }
    }
}

/// What the flags above a formed node read of it: its area, whether it lies
/// inside its parent's rect, and whether it is contained.
fn place_in_parent(rect: &Rect<f64>, contained: bool, parent: &Rect<f64>) -> (bool, bool, bool) {
    (has_area(rect), inside(rect, parent), contained)
}

/// Whether applying `grafts` leaves every published `clips_children` flag
/// as a full assembly would set it. A graft re-forms its anchor and the
/// nodes below it, not the nodes above, and the flag and containment of
/// each node above read only the anchor's place in its parent
/// ([`place_in_parent`]), besides its own rect and clip, which a graft
/// below it does not move. `uncontained` holds the published nodes that are
/// not contained; `None` when the published tree was not assembled here,
/// which leaves the grafts nothing to compare with.
fn grafts_keep_clipping(
    arena: &SemanticsTree,
    uncontained: Option<&FxHashSet<flui_foundation::SemanticsId>>,
    grafts: &[(flui_foundation::SemanticsId, BuiltSemanticsNode<'_>)],
) -> bool {
    let Some(uncontained) = uncontained else {
        return false;
    };
    grafts.iter().all(|(anchor, built)| {
        let Some(old) = arena.get(*anchor) else {
            return false;
        };
        let Some(parent) = old.parent().and_then(|parent| arena.get(parent)) else {
            return true;
        };
        place_in_parent(&old.rect(), !uncontained.contains(anchor), &parent.rect())
            == place_in_parent(&built.rect, built.contained, &parent.rect())
    })
}

/// Whether `rect` is finite and has an area.
fn has_area(rect: &Rect<f64>) -> bool {
    let [left, top, right, bottom] = [rect.left(), rect.top(), rect.right(), rect.bottom()];
    [left, top, right, bottom]
        .iter()
        .all(|edge| edge.is_finite())
        && right > left
        && bottom > top
}

/// Whether `outer` holds all of `inner`.
fn inside(inner: &Rect<f64>, outer: &Rect<f64>) -> bool {
    outer.left() <= inner.left()
        && outer.top() <= inner.top()
        && outer.right() >= inner.right()
        && outer.bottom() >= inner.bottom()
}

enum SemanticsFragment<'f> {
    Pending(PendingSemanticsNode<'f>),
    Formed(BuiltSemanticsNode<'f>),
}

/// A subtree's contribution and its still-unconsumed sibling barrier.
/// Transparent render wrappers forward the barrier; a formed boundary consumes
/// descendant barriers while keeping its own directive for its parent.
struct SemanticsFragments<'f> {
    nodes: FrameVec<'f, SemanticsFragment<'f>>,
    blocks_previous_siblings: bool,
}

impl<'f> SemanticsFragments<'f> {
    /// No fragment and no barrier; allocates nothing.
    fn none(arena: &'f FrameArena) -> Self {
        Self {
            nodes: Vec::new_in(arena),
            blocks_previous_siblings: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct SemanticsAssemblyContext {
    is_root: bool,
    parent_requires_explicit_node: bool,
    merge_into_ancestor: bool,
    has_reveal_ancestor: bool,
    /// Whether what a configuration absorbs here names a control
    /// (ADR-0332): `SemanticsConfiguration::names_a_control` of the
    /// configuration above.
    names_a_control: bool,
}

fn assemble_semantics_root<'f>(
    tree: &RenderTree,
    root: Option<RenderId>,
    regions: &mut AssemblyRegions<'f>,
) -> Result<Option<BuiltSemanticsNode<'f>>, SemanticsError> {
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
            names_a_control: false,
        },
        false,
        regions,
    )
    .ok_or(SemanticsError::MissingRootRenderObject { root })?;

    extract_formed_root(root, fragments.nodes).map(Some)
}

fn extract_formed_root<'f>(
    root: RenderId,
    fragments: FrameVec<'f, SemanticsFragment<'f>>,
) -> Result<BuiltSemanticsNode<'f>, SemanticsError> {
    if fragments.len() != 1 {
        return Err(SemanticsError::InvalidRootFragmentCount {
            actual: fragments.len(),
        });
    }

    match fragments.into_iter().next() {
        Some(SemanticsFragment::Formed(root_node)) => Ok(root_node),
        Some(SemanticsFragment::Pending(_)) => Err(SemanticsError::PendingRoot { root }),
        None => Err(SemanticsError::InvalidRootFragmentCount { actual: 0 }),
    }
}

fn build_semantics_fragments<'f>(
    tree: &RenderTree,
    id: RenderId,
    transform: Matrix4,
    clips: SemanticsClips,
    context: SemanticsAssemblyContext,
    ancestor_blocks_user_actions: bool,
    regions: &mut AssemblyRegions<'f>,
) -> Option<SemanticsFragments<'f>> {
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
/// recomputation (`graft::assembly_inputs_for`, private to that module) folds the EXACT rules the walk
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
    hidden_by_clip: bool,
    context: SemanticsAssemblyContext,
) -> NodeAssemblyDecisions {
    let boundary = config.is_semantics_boundary() || config.is_merge_boundary();
    // A node the clips hide contributes: as its own node it carries the
    // hidden flag (see `PendingSemanticsNode::hidden_by_clip`).
    let contributes = context.is_root || boundary || config.has_been_annotated() || hidden_by_clip;
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
            names_a_control: config.names_a_control(forms_node, context.names_a_control),
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
fn build_semantics_fragments_impl<'f>(
    tree: &RenderTree,
    id: RenderId,
    transform: Matrix4,
    clips: SemanticsClips,
    context: SemanticsAssemblyContext,
    ancestor_blocks_user_actions: bool,
    regions: &mut AssemblyRegions<'f>,
) -> Option<SemanticsFragments<'f>> {
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
    let decisions = assembly_decisions(&config, clipped.hidden, context);

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

    // Most nodes of a large subtree contribute no fragment (a decoration, a
    // layout box): the list allocates on its first fragment, not per node.
    let arena = regions.arena;
    let mut child_fragments: FrameVec<'f, SemanticsFragment<'f>> = Vec::new_in(arena);
    // Whether every child, and so everything below the node, lies under a
    // paint clip inside the node's rect: the clips only narrow on the way
    // down, so the clip of the slot a descendant sits under holds it. A
    // region lies under the node's own clip.
    let inside_rect = |clip: Option<Rect<f64>>| clip.is_some_and(|clip| inside(&clip, &rect));
    let mut clip_inside = (authored.before_children.is_empty()
        && authored.after_children.is_empty())
        || inside_rect(clips.paint);
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
            let child_clips = clips.descend(local_paint, local_semantics);
            clip_inside &= inside_rect(child_clips.paint);
            let fragments = build_semantics_fragments(
                tree,
                child_id,
                child_transform,
                child_clips,
                decisions.child_context,
                blocks_user_actions,
                regions,
            )
            .unwrap_or_else(|| SemanticsFragments::none(arena));
            if fragments.blocks_previous_siblings {
                // Every handler in discarded drafts is pinned by AssemblyCallbacks
                // until phase completion, outside the pipeline borrow.
                child_fragments.clear();
                children_block_previous = true;
            }
            // Room for one fragment per child still to come, as a list sized for every child up
            // front would have; a child with several fragments grows it by doubling.
            let still_to_come = node.children().len() - child_slot - 1;
            if fragments.nodes.is_empty() {
                // A child that contributes nothing leaves the list as it is: a node whose
                // children list nothing allocates nothing.
            } else if child_fragments.capacity() == 0 {
                // The first child that contributes hands its list over whole. It is the arena's
                // last block, so it grows in place; each later child's list then sits above it,
                // is emptied into it and freed as the last block, and the arena reuses that
                // memory for the next child.
                child_fragments = fragments.nodes;
                child_fragments.reserve(still_to_come);
            } else {
                child_fragments.reserve(fragments.nodes.len() + still_to_come);
                child_fragments.extend(fragments.nodes);
            }
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

    let (children, absorbed_inside) = merge_child_fragments(
        &mut config,
        &rect,
        child_fragments,
        regions.built,
        decisions.children_merge_into_ancestor,
        decisions.child_context.names_a_control,
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
        clip_inside,
        children,
        absorbed_inside,
        hidden_by_clip: clipped.hidden,
    };

    let mut nodes = Vec::with_capacity_in(1, arena);
    nodes.push(if decisions.forms_node {
        SemanticsFragment::Formed(pending.form())
    } else {
        SemanticsFragment::Pending(pending)
    });
    Some(SemanticsFragments {
        nodes,
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
fn append_region_fragments<'f>(
    fragments: &mut FrameVec<'f, SemanticsFragment<'f>>,
    authored: Vec<flui_semantics::SemanticsRegion>,
    host: RenderId,
    placement: RegionPlacement,
    transform: Matrix4,
    clips: SemanticsClips,
    context: SemanticsAssemblyContext,
    blocks_user_actions: bool,
    regions: &mut AssemblyRegions<'f>,
) -> bool {
    let mut blocks_previous = false;
    for region in authored {
        if region
            .config
            .sort_key()
            .is_some_and(|key| !key.order().is_finite())
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
        let decisions = assembly_decisions(&config, clipped.hidden, context);
        let pending = PendingSemanticsNode {
            source: SemanticsSource::Region(source),
            config,
            rect: clipped.rect,
            reveal_rect,
            clip_inside: clips.paint.is_some_and(|clip| inside(&clip, &clipped.rect)),
            children: Vec::new_in(regions.built),
            absorbed_inside: true,
            hidden_by_clip: clipped.hidden,
        };
        fragments.push(if decisions.forms_node {
            SemanticsFragment::Formed(pending.form())
        } else {
            SemanticsFragment::Pending(pending)
        });
    }
    blocks_previous
}

fn merge_child_fragments<'f>(
    config: &mut SemanticsConfiguration,
    host: &Rect<f64>,
    mut fragments: FrameVec<'f, SemanticsFragment<'f>>,
    built: &'f FrameArena,
    suppress_conflicts: bool,
    names_a_control: bool,
) -> (FrameVec<'f, BuiltSemanticsNode<'f>>, bool) {
    // Stable sorting preserves declarative order for equal keys and unkeyed
    // siblings. Sort before folding, so merged labels follow reading order too.
    fragments.sort_by(|a, b| {
        fn key<'a>(
            fragment: &'a SemanticsFragment<'_>,
        ) -> Option<&'a flui_semantics::SemanticsSortKey> {
            match fragment {
                SemanticsFragment::Formed(node) => node.config.sort_key(),
                SemanticsFragment::Pending(node) => node.config.sort_key(),
            }
        }
        by_sort_key(key(a), key(b))
    });
    let conflicts = (!suppress_conflicts).then(|| {
        mark_configuration_conflicts(config, &fragments, names_a_control, fragments.allocator())
    });
    // A forced merge plans who supplies each action from every part it will
    // absorb before it absorbs one (ADR-0392 §2); an ordinary one needs no plan.
    let mut fold = if suppress_conflicts {
        let receiver_has_text = config.text_content().is_some();
        Fold::Forced(config.forced_merge(forced_parts(receiver_has_text, &fragments)))
    } else {
        Fold::Ordinary(config)
    };
    let scratch = *fragments.allocator();
    let total = fragments.len();
    // Allocated on the first child that needs a list of its own: an absorbed pending node's
    // list is taken over instead, so a chain of absorbed nodes over many descendants moves one
    // list up the chain rather than copying it at each level.
    let mut children: FrameVec<'f, BuiltSemanticsNode<'f>> = Vec::new_in(built);
    let mut absorbed_inside = true;

    // The iterator holds the fragments' buffer, below the conflict marks: it is dropped after
    // them, so the arena frees both in the reverse order it handed them out.
    let mut has_text = fold.receiver().text_content().is_some();
    let mut drained = fragments.into_iter();
    for (index, fragment) in drained.by_ref().enumerate() {
        let rest = total - index - 1;
        match fragment {
            SemanticsFragment::Formed(node) => {
                make_room(&mut children, 1 + rest);
                children.push(node);
            }
            // Runs are the synthetic children of the one node declaring them,
            // so a merge never drops a second text's runs for the first's:
            // that text keeps its node and its runs (ADR-0214 §2).
            SemanticsFragment::Pending(pending)
                if conflicts
                    .as_ref()
                    .is_some_and(|places| places[index].forms_node())
                    || keeps_its_own_text(&mut has_text, &pending.config) =>
            {
                make_room(&mut children, 1 + rest);
                children.push(pending.form());
            }
            SemanticsFragment::Pending(pending) => {
                fold.absorb(&pending.config);
                absorbed_inside &= pending.absorbed_inside && inside(&pending.rect, host);
                if children.capacity() == 0 {
                    // Nothing is listed yet, so `children` holds no memory: take this list over
                    // with room for the fragments still to come.
                    children = pending.children;
                    children.reserve_exact(rest);
                } else {
                    make_room(&mut children, pending.children.len() + rest);
                    children.extend(pending.children);
                }
            }
        }
    }
    let config = fold.finish();
    drop(conflicts);
    drop(drained);

    if config.makes_document_selections() {
        in_reading_order(
            &mut children,
            config.text_direction().unwrap_or_default(),
            scratch,
        );
    }
    (children, absorbed_inside)
}

/// How `merge_child_fragments` absorbs its pending fragments.
enum Fold<'c> {
    /// Conflicting fragments form nodes, so each action has one supplier.
    Ordinary(&'c mut SemanticsConfiguration),
    /// `MergeSemantics`: every fragment is absorbed, by a plan.
    Forced(flui_semantics::ForcedMerge<'c>),
}

impl<'c> Fold<'c> {
    fn receiver(&self) -> &SemanticsConfiguration {
        match self {
            Self::Ordinary(config) => config,
            Self::Forced(merge) => merge.receiver(),
        }
    }

    fn absorb(&mut self, part: &SemanticsConfiguration) {
        match self {
            Self::Ordinary(config) => config.absorb(part),
            Self::Forced(merge) => merge.absorb(part),
        }
    }

    /// Ends the fold and hands the receiver back.
    fn finish(self) -> &'c mut SemanticsConfiguration {
        match self {
            Self::Ordinary(config) => config,
            Self::Forced(merge) => merge.finish(),
        }
    }
}

/// The configurations a forced fold absorbs, in fold order: every pending
/// fragment but one [`keeps_its_own_text`] sends to a node of its own.
fn forced_parts<'a>(
    receiver_has_text: bool,
    fragments: &'a [SemanticsFragment<'_>],
) -> impl Iterator<Item = &'a SemanticsConfiguration> + Clone {
    fragments
        .iter()
        .scan(receiver_has_text, |has_text, fragment| {
            Some(match fragment {
                SemanticsFragment::Formed(_) => None,
                SemanticsFragment::Pending(pending) => {
                    (!keeps_its_own_text(has_text, &pending.config)).then_some(&pending.config)
                }
            })
        })
        .flatten()
}

/// Whether `part` keeps a node of its own because the fold already holds a
/// text: runs are the synthetic children of the one node declaring them, so
/// a merge never drops a second text's runs for the first's (ADR-0214 §2).
/// Otherwise the fold absorbs it, and `has_text` notes a text it brings.
fn keeps_its_own_text(has_text: &mut bool, part: &SemanticsConfiguration) -> bool {
    let text = part.text_content().is_some();
    if text && *has_text {
        return true;
    }
    *has_text |= text;
    false
}

/// Makes room in `list` for `additional` more: exactly that much for its first allocation, which
/// is sized for every fragment still to come, and by doubling after that.
fn make_room<T>(list: &mut FrameVec<'_, T>, additional: usize) {
    if list.capacity() == 0 {
        list.reserve_exact(additional);
    } else {
        list.reserve(additional);
    }
}

/// Keyed siblings first, by name and then order; unkeyed ones after, as
/// they are.
fn by_sort_key(
    a: Option<&flui_semantics::SemanticsSortKey>,
    b: Option<&flui_semantics::SemanticsSortKey>,
) -> std::cmp::Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.name().cmp(&b.name()).then_with(|| {
            if a.order() == b.order() {
                std::cmp::Ordering::Equal
            } else {
                a.order().total_cmp(&b.order())
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
///
/// The children stay in their own list; the reordering lists into `scratch`, freed in the
/// reverse order it allocates.
fn in_reading_order(
    children: &mut FrameVec<'_, BuiltSemanticsNode<'_>>,
    direction: TextDirection,
    scratch: &FrameArena,
) {
    children.sort_by(|a, b| by_sort_key(a.config.sort_key(), b.config.sort_key()));
    let keyed = children
        .iter()
        .take_while(|child| child.config.sort_key().is_some())
        .count();
    let unkeyed = children.len() - keyed;
    let mut boxes: FrameVec<'_, Rect<f64>> = Vec::with_capacity_in(unkeyed, scratch);
    boxes.extend(children[keyed..].iter().map(|child| child.reveal_rect));
    let order = flui_semantics::reading_order(&boxes, direction);
    let mut slots: FrameVec<'_, Option<BuiltSemanticsNode<'_>>> =
        Vec::with_capacity_in(unkeyed, scratch);
    while children.len() > keyed {
        slots.push(children.pop());
    }
    slots.reverse();
    // Popping kept the list's capacity, so this refills it in place.
    children.extend(order.into_iter().filter_map(|index| slots[index].take()));
}

/// What the conflict marking decided for one fragment of a parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FragmentPlace {
    /// The parent absorbs it (or it is a node already and was never asked).
    Absorbed,
    /// The parent cannot take it: it is a node of its own.
    Refused,
    /// The parent could take it, but not together with a sibling.
    Displaced,
}

impl FragmentPlace {
    /// Whether a pending fragment in this place forms its own node.
    fn forms_node(self) -> bool {
        self != Self::Absorbed
    }
}

/// Which pending fragments form nodes of their own instead of being
/// absorbed by `parent`: one `parent` cannot take, and two that cannot be in
/// its node together.
///
/// A fragment `parent` refuses is a node already, so a sibling is asked
/// whether it can stay beside that node, not whether the two can share one:
/// where the fragments name a control (ADR-0332), a check box beside an
/// icon does not keep the icon from naming the button around both.
///
/// A byte per fragment, from the frame arena: this runs for every node of every assembly.
fn mark_configuration_conflicts<'f>(
    parent: &SemanticsConfiguration,
    fragments: &[SemanticsFragment<'_>],
    names_a_control: bool,
    arena: &'f FrameArena,
) -> FrameVec<'f, FragmentPlace> {
    let pending = |index: usize| match &fragments[index] {
        SemanticsFragment::Pending(pending) => Some(&pending.config),
        SemanticsFragment::Formed(_) => None,
    };
    let mut places: FrameVec<'f, FragmentPlace> = Vec::with_capacity_in(fragments.len(), arena);
    places.extend((0..fragments.len()).map(|index| match pending(index) {
        Some(config) if !parent.can_absorb(config, names_a_control) => FragmentPlace::Refused,
        _ => FragmentPlace::Absorbed,
    }));

    for index in 0..fragments.len() {
        let Some(config) = pending(index) else {
            continue;
        };
        for sibling_index in 0..index {
            let Some(sibling) = pending(sibling_index) else {
                continue;
            };
            let refused = |index: usize| places[index] == FragmentPlace::Refused;
            let compatible = match (refused(index), refused(sibling_index)) {
                (true, true) => continue,
                (true, false) => sibling.can_stay_beside(config, names_a_control),
                (false, true) => config.can_stay_beside(sibling, names_a_control),
                (false, false) => config.can_join(sibling, names_a_control),
            };
            if !compatible {
                for displaced in [index, sibling_index] {
                    if places[displaced] == FragmentPlace::Absorbed {
                        places[displaced] = FragmentPlace::Displaced;
                    }
                }
            }
        }
    }

    places
}

/// Replaces the published tree with `root`, and `uncontained` with its
/// nodes that are not contained.
fn rebuild_semantics_owner(
    owner: &mut SemanticsOwner,
    root: Option<BuiltSemanticsNode<'_>>,
    regions: &RegionIdentities,
    uncontained: &mut FxHashSet<flui_foundation::SemanticsId>,
) {
    owner.clear();
    uncontained.clear();

    let Some(root) = root else {
        return;
    };

    let root_id = insert_built_semantics_node(owner, root, regions, uncontained);
    owner.set_root(Some(root_id));
}

/// Splits an assembled node into its arena representation and its children.
fn semantics_node_parts<'f>(
    built: BuiltSemanticsNode<'f>,
    regions: &RegionIdentities,
) -> (SemanticsNode, FrameVec<'f, BuiltSemanticsNode<'f>>) {
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
    node.set_clips_children(built.clips_children);
    (node, built.children)
}

fn insert_built_semantics_node(
    owner: &mut SemanticsOwner,
    built: BuiltSemanticsNode<'_>,
    regions: &RegionIdentities,
    uncontained: &mut FxHashSet<flui_foundation::SemanticsId>,
) -> flui_foundation::SemanticsId {
    let contained = built.contained;
    let (node, children) = semantics_node_parts(built, regions);
    let id = owner.insert(node);
    if !contained {
        uncontained.insert(id);
    }
    for child in children {
        let child_id = insert_built_semantics_node(owner, child, regions, uncontained);
        owner.add_child(id, child_id);
    }
    id
}

fn describe_semantics_configuration(
    id: RenderId,
    node: &RenderNode,
    regions: &mut AssemblyRegions<'_>,
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
    if config
        .sort_key()
        .is_some_and(|key| !key.order().is_finite())
    {
        regions.invalid_sort_key = Some(id);
    }
    config
}

/// The configuration a fragment carries, formed or not.
fn fragment_config<'a>(fragment: &'a mut SemanticsFragment<'_>) -> &'a mut SemanticsConfiguration {
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
    children: &[BuiltSemanticsNode<'_>],
) -> bool {
    fn within(node: &BuiltSemanticsNode<'_>) -> bool {
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
    fragments: &mut [SemanticsFragment<'_>],
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
    children: &[BuiltSemanticsNode<'_>],
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
