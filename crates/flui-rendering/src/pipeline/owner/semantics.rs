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
    context: SemanticsAssemblyContext,
    ancestor_blocks_user_actions: bool,
) -> Option<Vec<SemanticsFragment>> {
    ensure_stack(|| {
        build_semantics_fragments_impl(tree, id, origin, context, ancestor_blocks_user_actions)
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
    context: SemanticsAssemblyContext,
    ancestor_blocks_user_actions: bool,
) -> Option<Vec<SemanticsFragment>> {
    #[cfg(test)]
    ASSEMBLY_VISITS.with(|counter| counter.set(counter.get() + 1));

    let node = tree.get(id)?;
    let mut config = describe_semantics_configuration(node);
    let blocks_user_actions = ancestor_blocks_user_actions || config.blocks_user_actions();
    config.set_blocks_user_actions(blocks_user_actions);
    let rect = node_semantics_rect(node, origin);

    let decisions = assembly_decisions(&config, context);

    let mut child_fragments = Vec::with_capacity(node.children().len());
    if !node_excludes_semantics_subtree(node) {
        for &child_id in node.children() {
            let Some(child) = tree.get(child_id) else {
                continue;
            };
            let child_origin = offset_add(origin, child.offset());
            let mut fragments = build_semantics_fragments(
                tree,
                child_id,
                child_origin,
                decisions.child_context,
                blocks_user_actions,
            )
            .unwrap_or_default();
            child_fragments.append(&mut fragments);
        }
    }

    if !decisions.contributes {
        return Some(child_fragments);
    }

    let children = merge_child_fragments(
        &mut config,
        child_fragments,
        decisions.children_merge_into_ancestor,
    );
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
) -> Option<(SemanticsAssemblyContext, Offset, bool)> {
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

    for (index, &id) in chain.iter().enumerate() {
        let node = tree.get(id)?;
        if index > 0 {
            origin = offset_add(origin, node.offset());
        }
        if id == target {
            if context.merge_into_ancestor {
                return None;
            }
            return Some((context, origin, blocks_user_actions));
        }

        let mut config = describe_semantics_configuration(node);
        blocks_user_actions = blocks_user_actions || config.blocks_user_actions();
        config.set_blocks_user_actions(blocks_user_actions);
        if node_excludes_semantics_subtree(node) {
            return None;
        }
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
    let Some((context, origin, blocks_user_actions)) =
        assembly_inputs_for(tree, pipeline_root, anchor)
    else {
        return false;
    };
    let Some(mut fragments) =
        build_semantics_fragments(tree, anchor, origin, context, blocks_user_actions)
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
