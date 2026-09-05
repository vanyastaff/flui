//! Semantics phase implementation for `PipelineOwner<Semantics>`.
//!
//! # Assembly is mark-scoped, with a full rebuild as the fallback
//!
//! A pass re-assembles only the subtrees that can observe the frame's
//! semantics marks, grafting each re-assembled subtree into the persistent
//! semantics arena under its **anchor** — the nearest unmarked ancestor
//! that formed a semantics node last pass. Anything the graft preconditions
//! cannot prove falls back to the classic whole-tree rebuild (ADR-0014),
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
//! mark-driven — matching Flutter, whose `flushSemantics` also updates
//! only dirty boundaries — where the previous full rebuild refreshed
//! every node's geometry as a side effect of ANY mark, an accident no
//! contract promised.

use flui_foundation::RenderId;
use flui_semantics::{
    AccessibilityNodeId, SemanticsConfiguration, SemanticsNode, SemanticsOwner, SemanticsTree,
};
use flui_types::{Offset, Point, Rect, Size, geometry::Pixels};
use rustc_hash::FxHashSet;

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
    /// so parents must be processed first. This matches Flutter's
    /// `flushSemantics`.
    pub fn run_semantics(&mut self) -> crate::error::RenderResult<()> {
        if !self.semantics_enabled() {
            return Ok(());
        }

        tracing::debug!(
            "run_semantics: {} nodes",
            self.scheduler.semantics_queue_len()
        );

        self.scheduler.enter_phase(PhaseKind::Semantics);

        // Sort shallow-first matching Flutter's flushSemantics. Roots
        // dispatch before their descendants so a parent's config is
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
                // Mark-scoped graft first; anything it cannot prove falls
                // back to the classic full rebuild (see the module doc).
                // A partial graft aborted midway is safe for the same
                // reason: the fallback clears and rebuilds the arena.
                let grafted = !tree_is_empty
                    && try_graft_pass(&self.render_tree, self.root_id, owner, &marked);
                if !grafted {
                    let built = match assemble_semantics_root(&self.render_tree, self.root_id) {
                        Ok(built) => built,
                        Err(error) => {
                            let _ = self.scheduler.exit_phase(PhaseKind::Semantics);
                            return Err(crate::error::RenderError::semantics(error.to_string()));
                        }
                    };
                    rebuild_semantics_owner(owner, built);
                }
                owner.flush();
            } else if pending_count > 0 {
                // This matches Flutter's early return when `_semanticsOwner`
                // is absent. Keep the warning so enabled-without-owner wiring
                // is visible during integration without panicking in a frame
                // hot path.
                tracing::warn!(
                    count = pending_count,
                    "run_semantics: SemanticsOwner is not installed; \
                     semantics config build for {pending_count} node(s) is skipped"
                );
            }
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

        Ok(())
    }
}

struct BuiltSemanticsNode {
    source_render_id: RenderId,
    config: SemanticsConfiguration,
    rect: Rect<Pixels>,
    children: Vec<BuiltSemanticsNode>,
}

struct PendingSemanticsNode {
    source_render_id: RenderId,
    config: SemanticsConfiguration,
    rect: Rect<Pixels>,
    children: Vec<BuiltSemanticsNode>,
}

impl PendingSemanticsNode {
    fn form(self) -> BuiltSemanticsNode {
        BuiltSemanticsNode {
            source_render_id: self.source_render_id,
            config: self.config,
            rect: self.rect,
            children: self.children,
        }
    }
}

enum SemanticsFragment {
    Pending(PendingSemanticsNode),
    Formed(BuiltSemanticsNode),
}

/// The clips an ancestor chain imposes on a node, in ROOT coordinates.
///
/// Two rects, because "not painted" and "not there" are different answers for
/// a screen reader. Content scrolled just past a viewport's edge is still
/// reachable — a user can ask to scroll to it — so it stays in the tree and is
/// flagged hidden; content past the cache area is gone from the tree entirely.
/// Reporting either one as an ordinary visible node puts a focus ring on empty
/// screen, which is why this is applied to the rect and not merely to the
/// membership.
#[derive(Debug, Clone, Copy, Default)]
struct SemanticsClips {
    /// Outside this, a child is painted nowhere the user can see.
    paint: Option<Rect<Pixels>>,
    /// Outside this, a child has no accessibility presence at all.
    semantics: Option<Rect<Pixels>>,
}

/// What [`SemanticsClips::apply`] decided about one node's rect.
struct ClippedRect {
    /// The rect to publish, already narrowed to the surviving part.
    rect: Rect<Pixels>,
    /// The node is off-screen but reachable: publish it, flagged hidden.
    hidden: bool,
    /// Nothing of the node survives the semantics clip: publish no node for
    /// it. Its children are still walked — an overflowing child can extend
    /// back into the clip its parent fell outside of.
    dropped: bool,
}

impl SemanticsClips {
    /// Narrows `rect` to what these clips leave of it.
    fn apply(self, rect: Rect<Pixels>) -> ClippedRect {
        let was_empty = rect.is_empty();

        // `Rect::intersect` reports a zero-area overlap as `Some(empty)`;
        // for a clip that is the same answer as no overlap at all.
        let intersect = |clip: &Rect<Pixels>, rect: &Rect<Pixels>| {
            clip.intersect(rect).filter(|kept| !kept.is_empty())
        };

        let rect = match self.semantics {
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
                hidden: !was_empty,
                dropped: false,
            },
        }
    }

    /// The clips a child of this node inherits.
    ///
    /// Paint clips always intersect. The semantics clip follows Flutter's
    /// three-way rule:
    ///
    /// - a node that declares one REPLACES whatever it inherited, so a nested
    ///   viewport re-grants its own cache area to its own children instead of
    ///   being confined to its parent's;
    /// - a node that declares only a paint clip NARROWS the inherited
    ///   semantics clip by it — a clip that hides paint also limits how far
    ///   an ancestor's cache area reaches through it;
    /// - a node that declares neither passes the inherited one through, and a
    ///   node with no inherited semantics clip stays unclipped whatever its
    ///   paint clip says.
    fn descend(
        self,
        local_paint: Option<Rect<Pixels>>,
        local_semantics: Option<Rect<Pixels>>,
    ) -> Self {
        let paint = intersect_clips(self.paint, local_paint);
        let semantics = match local_semantics {
            Some(replacement) => Some(replacement),
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
        Self { paint, semantics }
    }
}

/// Intersection where `None` means "no clip", not "empty".
fn intersect_clips(a: Option<Rect<Pixels>>, b: Option<Rect<Pixels>>) -> Option<Rect<Pixels>> {
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
        RenderNode::Box(entry) => entry.render_object().visits_child_for_semantics(child_slot),
        RenderNode::Sliver(entry) => entry.render_object().visits_child_for_semantics(child_slot),
    }
}

/// The clips `node` imposes on the child in `child_slot`, moved from the
/// node's own coordinates into the walk's root coordinates.
fn child_clips_of(
    node: &RenderNode,
    origin: Offset,
    child_slot: usize,
) -> (Option<Rect<Pixels>>, Option<Rect<Pixels>>) {
    let offset = flui_types::Offset::new(origin.dx, origin.dy);
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
        paint.map(|r| r.translate_offset(offset)),
        semantics.map(|r| r.translate_offset(offset)),
    )
}

#[derive(Debug, Clone, Copy)]
struct SemanticsAssemblyContext {
    is_root: bool,
    parent_requires_explicit_node: bool,
    merge_into_ancestor: bool,
}

#[derive(Debug, thiserror::Error)]
enum SemanticsAssemblyError {
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
) -> Result<Option<BuiltSemanticsNode>, SemanticsAssemblyError> {
    let Some(root) = root else {
        return Ok(None);
    };

    let fragments = build_semantics_fragments(
        tree,
        root,
        Offset::ZERO,
        SemanticsClips::default(),
        SemanticsAssemblyContext {
            is_root: true,
            parent_requires_explicit_node: false,
            merge_into_ancestor: false,
        },
        false,
    )
    .ok_or(SemanticsAssemblyError::MissingRootRenderObject { root })?;

    extract_formed_root(root, fragments).map(Some)
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
    origin: Offset,
    clips: SemanticsClips,
    context: SemanticsAssemblyContext,
    ancestor_blocks_user_actions: bool,
) -> Option<Vec<SemanticsFragment>> {
    ensure_stack(|| {
        build_semantics_fragments_impl(
            tree,
            id,
            origin,
            clips,
            context,
            ancestor_blocks_user_actions,
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
    let contributes =
        context.is_root || config.is_semantics_boundary() || config.has_been_annotated();
    let forms_node = !context.merge_into_ancestor
        && (context.is_root
            || config.is_semantics_boundary()
            || (contributes && context.parent_requires_explicit_node));
    let children_require_explicit_node = context.is_root
        || config.explicit_child_nodes()
        || (!contributes && context.parent_requires_explicit_node);
    let children_merge_into_ancestor =
        context.merge_into_ancestor || config.is_merging_semantics_of_descendants();

    NodeAssemblyDecisions {
        contributes,
        forms_node,
        children_merge_into_ancestor,
        child_context: SemanticsAssemblyContext {
            is_root: false,
            parent_requires_explicit_node: children_require_explicit_node,
            merge_into_ancestor: children_merge_into_ancestor,
        },
    }
}

/// Body of [`build_semantics_fragments`].
fn build_semantics_fragments_impl(
    tree: &RenderTree,
    id: RenderId,
    origin: Offset,
    clips: SemanticsClips,
    context: SemanticsAssemblyContext,
    ancestor_blocks_user_actions: bool,
) -> Option<Vec<SemanticsFragment>> {
    #[cfg(test)]
    ASSEMBLY_VISITS.with(|counter| counter.set(counter.get() + 1));

    let node = tree.get(id)?;
    let mut config = describe_semantics_configuration(node);
    let blocks_user_actions = ancestor_blocks_user_actions || config.blocks_user_actions();
    config.set_blocks_user_actions(blocks_user_actions);
    let clipped = clips.apply(node_semantics_rect(node, origin));
    let rect = clipped.rect;
    if clipped.hidden {
        config.set_hidden(true);
    }

    let decisions = assembly_decisions(&config, context);

    let mut child_fragments = Vec::with_capacity(node.children().len());
    if !node_excludes_semantics_subtree(node) {
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
            // than not announcing the row at all, which is what the reference
            // does: Flutter removes an off-screen or kept-alive child from the
            // render child list, so it publishes no semantics whatsoever.
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
            let child_origin = offset_add(origin, child.offset());
            let (local_paint, local_semantics) = child_clips_of(node, origin, child_slot);
            let mut fragments = build_semantics_fragments(
                tree,
                child_id,
                child_origin,
                clips.descend(local_paint, local_semantics),
                decisions.child_context,
                blocks_user_actions,
            )
            .unwrap_or_default();
            child_fragments.append(&mut fragments);
        }
    }

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
            offer_semantic_index_to_fragments(node, &mut child_fragments);
        }
        return Some(child_fragments);
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
    apply_lazy_child_semantic_index(node, &mut config);
    let pending = PendingSemanticsNode {
        source_render_id: id,
        config,
        rect,
        children,
    };

    Some(vec![if decisions.forms_node {
        SemanticsFragment::Formed(pending.form())
    } else {
        SemanticsFragment::Pending(pending)
    }])
}

fn merge_child_fragments(
    config: &mut SemanticsConfiguration,
    fragments: Vec<SemanticsFragment>,
    suppress_conflicts: bool,
) -> Vec<BuiltSemanticsNode> {
    let conflicts = (!suppress_conflicts).then(|| mark_configuration_conflicts(config, &fragments));
    let mut children = Vec::with_capacity(fragments.len());

    for (index, fragment) in fragments.into_iter().enumerate() {
        match fragment {
            SemanticsFragment::Formed(node) => children.push(node),
            SemanticsFragment::Pending(pending)
                if conflicts.as_ref().is_some_and(|conflicts| conflicts[index]) =>
            {
                children.push(pending.form());
            }
            SemanticsFragment::Pending(pending) => {
                config.absorb(&pending.config);
                children.extend(pending.children);
            }
        }
    }

    children
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

fn rebuild_semantics_owner(owner: &mut SemanticsOwner, root: Option<BuiltSemanticsNode>) {
    owner.clear();

    let Some(root) = root else {
        return;
    };

    let root_id = insert_built_semantics_node(owner, root);
    owner.set_root(Some(root_id));
}

/// Splits an assembled node into its arena representation and its children.
fn semantics_node_parts(built: BuiltSemanticsNode) -> (SemanticsNode, Vec<BuiltSemanticsNode>) {
    let mut node = SemanticsNode::new()
        .with_source_render_id(built.source_render_id)
        .with_config(built.config);
    node.set_rect(built.rect);
    (node, built.children)
}

fn insert_built_semantics_node(
    owner: &mut SemanticsOwner,
    built: BuiltSemanticsNode,
) -> flui_foundation::SemanticsId {
    let (node, children) = semantics_node_parts(built);
    let id = owner.insert(node);
    for child in children {
        let child_id = insert_built_semantics_node(owner, child);
        owner.add_child(id, child_id);
    }
    id
}

// ============================================================================
// Mark-scoped graft (see the module doc for the model and its guarantees)
// ============================================================================

/// Attempts the mark-scoped pass: re-assemble only the anchored subtrees the
/// marks can influence, grafting each into the persistent arena. Returns
/// `false` when any precondition cannot be proven — the caller then runs the
/// classic full rebuild, which also makes a partially-applied graft safe
/// (the fallback clears the arena wholesale).
fn try_graft_pass(
    tree: &RenderTree,
    pipeline_root: Option<RenderId>,
    owner: &mut SemanticsOwner,
    marked: &[RenderId],
) -> bool {
    let Some(pipeline_root) = pipeline_root else {
        return false;
    };
    let marked_set: FxHashSet<RenderId> = marked.iter().copied().collect();

    // Resolve every mark to its anchor. A mark whose ancestor chain holds
    // no anchored formed node (the root itself is marked, or the region
    // never published) forces the full rebuild.
    let mut anchors: FxHashSet<RenderId> = FxHashSet::default();
    for &mark in &marked_set {
        let Some(anchor) = anchor_for(tree, owner.tree(), &marked_set, mark) else {
            return false;
        };
        anchors.insert(anchor);
    }

    // Keep only top-most anchors: an anchor inside another anchor's render
    // subtree is fully covered by re-assembling the outer one. After this,
    // every survivor's proper ancestor chain is unmarked (a marked ancestor's
    // own anchor would sit above it and swallow this one), which is what
    // makes the recomputed context and the "nothing above changes" argument
    // in the module doc hold.
    let survivors: Vec<RenderId> = anchors
        .iter()
        .copied()
        .filter(|&anchor| {
            let mut cursor = tree.parent(anchor);
            while let Some(ancestor) = cursor {
                if anchors.contains(&ancestor) {
                    return false;
                }
                cursor = tree.parent(ancestor);
            }
            true
        })
        .collect();

    for anchor in survivors {
        if !graft_anchor(tree, pipeline_root, owner, anchor) {
            return false;
        }
    }
    true
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

/// Recomputes the assembly inputs (context, accumulated origin, inherited
/// action-blocking) the full walk would hand `target`, by folding the
/// ancestor chain root→target through [`assembly_decisions`] — the same
/// rules the walk itself applies, so the graft assembles under exactly the
/// context a full rebuild would have used. Sound because every proper
/// ancestor of a surviving anchor is unmarked: their configs are the ones
/// the last pass already used.
///
/// `None` when the chain is not rooted at the pipeline root (a detached
/// subtree), an ancestor excludes its semantics subtree, or the fold says
/// `target` assembles merged into an ancestor — each contradicts the
/// anchor's arena presence, so the caller falls back to the full rebuild.
fn assembly_inputs_for(
    tree: &RenderTree,
    pipeline_root: RenderId,
    target: RenderId,
) -> Option<(SemanticsAssemblyContext, Offset, SemanticsClips, bool)> {
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
    };
    let mut blocks_user_actions = false;
    let mut origin = Offset::ZERO;
    let mut clips = SemanticsClips::default();

    for (index, &id) in chain.iter().enumerate() {
        let node = tree.get(id)?;
        if index > 0 {
            origin = offset_add(origin, node.offset());
        }
        if id == target {
            if context.merge_into_ancestor {
                return None;
            }
            return Some((context, origin, clips, blocks_user_actions));
        }

        let mut config = describe_semantics_configuration(node);
        blocks_user_actions = blocks_user_actions || config.blocks_user_actions();
        config.set_blocks_user_actions(blocks_user_actions);
        if node_excludes_semantics_subtree(node) {
            return None;
        }
        // The clip this node imposes on the chain's next link. The child's
        // slot is its position in this node's children — the same index the
        // full walk enumerates, so the two folds agree by construction.
        let child_slot = node
            .children()
            .iter()
            .position(|&child| child == chain[index + 1])?;
        let (local_paint, local_semantics) = child_clips_of(node, origin, child_slot);
        clips = clips.descend(local_paint, local_semantics);
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
fn graft_anchor(
    tree: &RenderTree,
    pipeline_root: RenderId,
    owner: &mut SemanticsOwner,
    anchor: RenderId,
) -> bool {
    let Some(anchor_sid) = owner
        .tree()
        .find_by_accessibility_id(AccessibilityNodeId::from(anchor))
    else {
        return false;
    };
    let Some((context, origin, clips, blocks_user_actions)) =
        assembly_inputs_for(tree, pipeline_root, anchor)
    else {
        return false;
    };
    let Some(mut fragments) =
        build_semantics_fragments(tree, anchor, origin, clips, context, blocks_user_actions)
    else {
        return false;
    };
    // The anchor formed a node last pass under this exact context and
    // config, so it must form exactly one again; anything else means an
    // assumption broke and the full rebuild is the honest answer.
    let built = match (fragments.pop(), fragments.is_empty()) {
        (Some(SemanticsFragment::Formed(built)), true) if built.source_render_id == anchor => built,
        _ => return false,
    };

    let old_children: Vec<flui_foundation::SemanticsId> = owner
        .tree()
        .children(anchor_sid)
        .map(<[flui_foundation::SemanticsId]>::to_vec)
        .unwrap_or_default();
    {
        use flui_tree::TreeWrite;
        for child in old_children {
            let _ = owner.tree_mut().remove(child);
        }
    }

    let (node, children) = semantics_node_parts(built);
    if !owner.tree_mut().replace_node(anchor_sid, node) {
        return false;
    }
    for child in children {
        let child_sid = insert_built_semantics_node(owner, child);
        owner.add_child(anchor_sid, child_sid);
    }
    true
}

fn describe_semantics_configuration(node: &RenderNode) -> SemanticsConfiguration {
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
    config
}

/// Publish a lazy sliver child's position in the set, from the index its host
/// stamped rather than from a wrapper widget.
///
/// A screen reader's "item 12 of 100" needs the 12. Flutter's lazy delegates
/// supply it by wrapping every materialised item in an `IndexedSemantics`
/// (`addSemanticIndexes`, on by default) — a render node per item, carrying an
/// index captured when the item was built. The sliver already stamps each
/// child's slot into its parent data and keeps it in step with the row's real
/// position as the band moves, so reading it here costs no node and cannot go
/// stale against the row it describes.
///
/// Applied AFTER the render object's own description on purpose: an explicit
/// [`IndexedSemantics`] on the item wins, which is what makes hand-indexed
/// content inside a lazy list possible at all.
///
/// A `semantic_index` of `None` publishes nothing — the child occupies a
/// logical index without being a member of the set, which is what a separator
/// is. A missing position degrades to "item ? of 100"; a wrong one misleads.
///
/// [`IndexedSemantics`]: https://api.flutter.dev/flutter/widgets/IndexedSemantics-class.html
/// Hand a transparent stamped node's position down to the fragments it
/// forwards, so the node that does form for this item carries it.
///
/// Only offered where nothing already declared one, at every level — an
/// explicit `IndexedSemantics` anywhere in the item wins, the same rule the
/// contributing path follows.
fn offer_semantic_index_to_fragments(node: &RenderNode, fragments: &mut [SemanticsFragment]) {
    let Some((index, set_size)) = stamped_sliver_parent_data(node)
        .and_then(|pd| pd.semantic_index.map(|index| (index, pd.semantic_set_size)))
    else {
        return;
    };
    for fragment in fragments {
        let config = match fragment {
            SemanticsFragment::Pending(pending) => &mut pending.config,
            SemanticsFragment::Formed(formed) => &mut formed.config,
        };
        // Same rule as the contributing path: an explicit `IndexedSemantics`
        // keeps its position AND suppresses the stamped size, because explicit
        // numbering describes a different set from the one the delegate
        // materialises.
        if config.index_in_parent().is_none() {
            config.set_index_in_parent(index);
            if let Some(size) = set_size {
                config.set_scroll_child_count(size);
            }
        }
    }
}

/// The lazy-sliver slot a host stamped into this node's parent data.
fn stamped_sliver_parent_data(
    node: &RenderNode,
) -> Option<&crate::parent_data::SliverMultiBoxAdaptorParentData> {
    node.parent_data()
        .and_then(|pd| pd.downcast_ref::<crate::parent_data::SliverMultiBoxAdaptorParentData>())
}

fn apply_lazy_child_semantic_index(node: &RenderNode, config: &mut SemanticsConfiguration) {
    let Some(pd) = stamped_sliver_parent_data(node) else {
        return;
    };
    // An explicit `IndexedSemantics` owns the POSITION, and the stamped size
    // does NOT pair with it. Explicit numbering exists to describe a DIFFERENT
    // set from the one the delegate materialises — six cards numbered as three
    // rows, or an offset numbering — so pairing "row 2" with the delegate's
    // count of six announces "row 2 of 6", and an offset can put the position
    // past the size entirely. A caller wanting both supplies both.
    //
    // The honest degradation is "row 2 of ?", the same trade this entry already
    // makes for an unresolved `ItemCount::Unknown`.
    if config.index_in_parent().is_some() {
        return;
    }
    if let Some(index) = pd.semantic_index {
        config.set_index_in_parent(index);
        // The total rides with the position, never without it: AccessKit's two
        // properties describe one node, and a size on a node with no position
        // announces "of 100" attached to nothing.
        //
        // `scroll_child_count` is the framework-side name for what the platform
        // publishes as `size_of_set` (`accesskit_translation`'s
        // `data.scroll_child_count -> set_size_of_set`). On an item node it
        // means the size of the set the item belongs to, NOT that the item
        // scrolls — the name comes from Flutter, where the property sits on the
        // scrollable and each platform bridge recombines it with the item's
        // index. AccessKit wants both on one node, so here it travels with the
        // item.
        if let Some(size) = pd.semantic_set_size {
            config.set_scroll_child_count(size);
        }
    }
}

fn node_excludes_semantics_subtree(node: &RenderNode) -> bool {
    match node {
        RenderNode::Box(entry) => entry.render_object().excludes_semantics_subtree(),
        RenderNode::Sliver(entry) => entry.render_object().excludes_semantics_subtree(),
    }
}

fn node_semantics_rect(node: &RenderNode, origin: Offset) -> Rect<Pixels> {
    let size = match node {
        RenderNode::Box(entry) => entry.state().geometry().unwrap_or(Size::ZERO),
        RenderNode::Sliver(entry) => entry.state().absolute_paint_size(),
    };
    Rect::from_origin_size(Point::new(origin.dx, origin.dy), size)
}

fn offset_add(a: Offset, b: Offset) -> Offset {
    Offset::new(a.dx + b.dx, a.dy + b.dy)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box that clips whatever it hosts, so a fold over an ancestor chain
    /// has something to accumulate.
    #[derive(Debug)]
    struct ClippingBox {
        semantics_clip: Option<Rect<Pixels>>,
        paint_clip: Option<Rect<Pixels>>,
    }

    impl ClippingBox {
        fn plain() -> Self {
            Self {
                semantics_clip: None,
                paint_clip: None,
            }
        }

        fn clipping(semantics: Rect<Pixels>, paint: Rect<Pixels>) -> Self {
            Self {
                semantics_clip: Some(semantics),
                paint_clip: Some(paint),
            }
        }
    }

    impl flui_foundation::Diagnosticable for ClippingBox {}

    impl crate::traits::RenderBox for ClippingBox {
        type Arity = flui_tree::Variable;
        type ParentData = crate::parent_data::BoxParentData;

        fn perform_layout(
            &mut self,
            ctx: &mut crate::context::BoxLayoutContext<
                '_,
                flui_tree::Variable,
                crate::parent_data::BoxParentData,
            >,
        ) -> Size {
            ctx.constraints().smallest()
        }

        fn describe_semantics_clip(&self, _child_slot: usize, _size: Size) -> Option<Rect<Pixels>> {
            self.semantics_clip
        }

        fn describe_approximate_paint_clip(
            &self,
            _child_slot: usize,
            _size: Size,
        ) -> Option<Rect<Pixels>> {
            self.paint_clip
        }
    }

    fn rect(top: f32, bottom: f32) -> Rect<Pixels> {
        Rect::from_ltrb(
            flui_types::geometry::px(0.0),
            flui_types::geometry::px(top),
            flui_types::geometry::px(100.0),
            flui_types::geometry::px(bottom),
        )
    }

    /// The graft re-derives a node's inherited clips by folding the ancestor
    /// chain, instead of re-walking the tree. If that fold ever stops seeing
    /// what the walk sees, the incremental path publishes rects the full
    /// rebuild would have clipped — and nothing else would notice, because
    /// both paths are "green" on their own.
    #[test]
    fn the_graft_fold_accumulates_the_same_clips_the_walk_applies() {
        let mut owner = PipelineOwner::new();
        let root = owner.set_root_render_object(Box::new(ClippingBox::clipping(
            rect(0.0, 60.0),
            rect(0.0, 20.0),
        )));
        let middle = owner
            .insert_child_render_object(root, Box::new(ClippingBox::plain()))
            .expect("middle inserted");
        let leaf = owner
            .insert_child_render_object(middle, Box::new(ClippingBox::plain()))
            .expect("leaf inserted");

        let (_, _, clips, _) = assembly_inputs_for(&owner.render_tree, root, leaf)
            .expect("the chain root..leaf is intact, so the fold resolves");

        assert_eq!(
            clips.semantics,
            Some(rect(0.0, 60.0)),
            "the root's semantics clip must reach a grandchild through the fold",
        );
        assert_eq!(
            clips.paint,
            Some(rect(0.0, 20.0)),
            "and so must its paint clip",
        );
    }

    /// A node the fold cannot place among its parent's children has no slot to
    /// ask the clip hooks about, so the graft declines and the caller falls
    /// back to the full rebuild rather than guessing.
    #[test]
    fn the_graft_fold_declines_for_a_node_outside_the_root_chain() {
        let mut owner = PipelineOwner::new();
        let root = owner.set_root_render_object(Box::new(ClippingBox::plain()));
        let orphan = RenderId::new(9999);

        assert!(
            assembly_inputs_for(&owner.render_tree, root, orphan).is_none(),
            "a target that is not under the root cannot be grafted",
        );
    }

    fn pending_fragment(source_render_id: RenderId) -> SemanticsFragment {
        SemanticsFragment::Pending(PendingSemanticsNode {
            source_render_id,
            config: SemanticsConfiguration::new(),
            rect: Rect::ZERO,
            children: Vec::new(),
        })
    }

    #[test]
    fn root_extraction_accepts_exactly_one_formed_fragment() {
        let root = RenderId::new(1);
        let SemanticsFragment::Pending(pending) = pending_fragment(root) else {
            panic!("test fixture must create a pending fragment");
        };

        let extracted = extract_formed_root(root, vec![SemanticsFragment::Formed(pending.form())])
            .expect("one formed fragment is a valid root");

        assert_eq!(extracted.source_render_id, root);
    }

    #[test]
    fn root_extraction_rejects_pending_or_multiple_fragments() {
        let root = RenderId::new(1);
        assert!(matches!(
            extract_formed_root(root, vec![pending_fragment(root)]),
            Err(SemanticsAssemblyError::PendingRoot { root: pending_root })
                if pending_root == root
        ));
        assert!(matches!(
            extract_formed_root(
                root,
                vec![pending_fragment(root), pending_fragment(RenderId::new(2))],
            ),
            Err(SemanticsAssemblyError::InvalidRootFragmentCount { actual: 2 })
        ));
    }
}
