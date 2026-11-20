//! Mark-scoped graft of re-assembled subtrees into the persistent semantics arena; see the parent
//! module doc for the model and its guarantees.

use flui_foundation::RenderId;
use flui_foundation::arena::FrameVec;
use flui_foundation::geometry::{Matrix4, Size};
use flui_semantics::{AccessibilityNodeId, SemanticsOwner, SemanticsTree};
use rustc_hash::FxHashSet;

use crate::storage::{RenderNode, RenderTree};

#[cfg(any(test, feature = "testing"))]
use flui_foundation::arena::FrameArena;

use super::{
    BuiltSemanticsNode, SemanticsAssemblyContext, SemanticsFragment, assembly_decisions,
    build_semantics_fragments, child_transform,
    clips::{SemanticsClips, child_clips_of, visits_child_for_semantics},
    collections::{declares_collection_scope, has_collection_metadata},
    describe_semantics_configuration, grafts_keep_clipping, insert_built_semantics_node,
    regions::{AssemblyRegions, RegionIdentities, SemanticsSource},
    semantics_node_parts,
};
#[cfg(any(test, feature = "testing"))]
use super::{
    SemanticsPlan, assemble_semantics_root, collections::reconcile_collections,
    rebuild_semantics_owner, regions::AssemblyCallbacks,
};

/// Plans all marked subtrees without mutating the persistent arena. If any
/// precondition fails the caller discards this plan and assembles the full tree.
pub(super) fn plan_graft_pass<'f>(
    tree: &RenderTree,
    pipeline_root: Option<RenderId>,
    owner: &SemanticsOwner,
    uncontained: Option<&FxHashSet<flui_foundation::SemanticsId>>,
    marked: &[RenderId],
    placed: &[RenderId],
    regions: &mut AssemblyRegions<'f>,
) -> Option<FrameVec<'f, (flui_foundation::SemanticsId, BuiltSemanticsNode<'f>)>> {
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
    // A placement anchor formed last pass and its configuration has not
    // changed, so, unlike a marked node, it re-assembles in its own slot.
    anchors.extend(
        placed
            .iter()
            .copied()
            .filter(|anchor| !marked_set.contains(anchor)),
    );

    // Keep only top-most anchors: an anchor inside another anchor's render
    // subtree is fully covered by re-assembling the outer one. After this,
    // every survivor's proper ancestor chain is unmarked (a marked ancestor's
    // own anchor would sit above it and swallow this one), which is what
    // makes the recomputed context and the "nothing above changes" argument
    // in the module doc hold.
    let mut survivors: Vec<(Vec<usize>, RenderId)> = anchors
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
        .map(|anchor| (preorder_path(tree, anchor), anchor))
        .collect();

    // Assemble the survivors in render preorder, the order the full walk
    // visits them in. The set above iterates in hash order, and the plan
    // records refusals (last one wins) and action handlers in visit order,
    // so a hash order would make a graft name a different host than a
    // rebuild of the same tree.
    survivors.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));

    let mut grafts = Vec::with_capacity_in(survivors.len(), regions.built);
    for (_, anchor) in survivors {
        grafts.push(plan_graft_anchor(
            tree,
            pipeline_root,
            owner,
            anchor,
            regions,
        )?);
    }
    // A flag above an anchor that the grafts would leave stale needs the
    // node that holds it re-formed: the full rebuild.
    grafts_keep_clipping(owner.tree(), uncontained, &grafts).then_some(grafts)
}

/// Resolves a placement mark — a node whose subtree moved while no
/// configuration in it changed — to the formed semantics nodes it holds.
///
/// Walks `moved`'s render subtree as the assembly walk does (the same placed
/// children, the same exclusions) and stops at each node that formed a
/// semantics node last pass, appending it to `anchors`: re-assembling it in
/// place re-places everything below it, and its own place among its siblings
/// does not follow geometry (checked by the caller). Every node passed on the
/// way must contribute nothing at all, clipped or not, so nothing the walk
/// skips ever reached the node above `moved`.
///
/// `false` when that cannot be shown: a node on the way contributes (a
/// pending fragment, a boundary the clip dropped last pass), authors
/// regions, bars its earlier siblings, or lands where a paint clip above it
/// could hide it, which makes it contribute the way assembly counts a node the
/// clips hide. The caller then treats
/// `moved` as an ordinary semantics mark.
pub(super) fn placement_anchors(
    tree: &RenderTree,
    pipeline_root: Option<RenderId>,
    arena: &SemanticsTree,
    moved: RenderId,
    scratch: &mut PlacementScratch,
    anchors: &mut Vec<RenderId>,
    regions: &mut AssemblyRegions<'_>,
) -> bool {
    let Some(pipeline_root) = pipeline_root else {
        return false;
    };
    let Some((context, transform, clips, _)) =
        assembly_inputs_into(tree, pipeline_root, moved, regions, &mut scratch.chain)
    else {
        return false;
    };
    let walk = &mut scratch.walk;
    walk.clear();
    walk.push(PlacementStep {
        id: moved,
        transform,
        clips,
        context,
    });
    while let Some(step) = walk.pop() {
        let PlacementStep {
            id,
            transform,
            clips,
            context,
        } = step;
        let Some(node) = tree.get(id) else {
            continue;
        };
        if arena
            .find_by_accessibility_id(AccessibilityNodeId::from(id))
            .is_some()
        {
            anchors.push(id);
            continue;
        }
        let config = describe_semantics_configuration(id, node, regions);
        if config.is_semantics_boundary()
            || config.is_merge_boundary()
            || config.has_been_annotated()
            || config.blocks_previous_siblings()
        {
            return false;
        }
        // Assembly counts a node the clips hide as contributing, so a move
        // can turn a transparent wrapper into a node of its own. No pass's
        // state is kept for a wrapper that formed nothing, so wherever the
        // clips can hide one the walk refuses. (A wrapper that formed a node
        // last pass is an anchor, found above.)
        if clips.can_hide(context.has_reveal_ancestor) {
            return false;
        }
        let authored = match node {
            RenderNode::Box(entry) => entry
                .render_object()
                .semantics_regions(entry.state().geometry().unwrap_or(Size::ZERO)),
            RenderNode::Sliver(entry) => entry
                .render_object()
                .semantics_regions(entry.state().absolute_paint_size()),
        };
        // The regions' handlers are authored code: like the assembly walk,
        // keep them until the pass ends so their captures retire after the
        // phase, not here with the pipeline borrowed.
        for region in authored
            .before_children
            .iter()
            .chain(&authored.after_children)
        {
            regions.callbacks.retain(&region.config);
        }
        if !authored.before_children.is_empty() || !authored.after_children.is_empty() {
            return false;
        }
        if node.excludes_semantics_subtree() {
            continue;
        }
        let parent_generation = node.layout_generation();
        let child_context = assembly_decisions(&config, false, context).child_context;
        // Pushed in reverse so the walk visits children in slot order, the
        // order the anchors' grafts are later sorted into anyway.
        for (child_slot, &child_id) in node.children().iter().enumerate().rev() {
            let Some(child) = tree.get(child_id) else {
                continue;
            };
            if !child.was_placed_by(id, parent_generation)
                || !visits_child_for_semantics(node, child_slot)
            {
                continue;
            }
            // The same fold assembly applies on its way down.
            let Some(child_transform) = child_transform(node, child, child_slot, transform) else {
                return false;
            };
            let (local_paint, local_semantics) = child_clips_of(node, transform, child_slot);
            walk.push(PlacementStep {
                id: child_id,
                transform: child_transform,
                clips: clips.descend(local_paint, local_semantics),
                context: child_context,
            });
        }
    }
    true
}

/// One node of the placement walk, with the transform, clips and context
/// assembly would give it.
#[derive(Debug, Clone, Copy)]
struct PlacementStep {
    id: RenderId,
    transform: Matrix4,
    clips: SemanticsClips,
    context: SemanticsAssemblyContext,
}

/// The placement walk's buffers, kept between passes so a steady pass
/// allocates nothing for them.
#[derive(Debug, Default)]
pub(super) struct PlacementScratch {
    walk: Vec<PlacementStep>,
    chain: Vec<RenderId>,
}

/// Whether re-assembling `anchor` in place keeps it where a full rebuild
/// would put it among its siblings. A parent that orders its children by
/// where they are laid out (a selectable document) may move it.
pub(super) fn placement_keeps_order(arena: &SemanticsTree, anchor: RenderId) -> bool {
    let Some(sid) = arena.find_by_accessibility_id(AccessibilityNodeId::from(anchor)) else {
        return false;
    };
    match arena.parent(sid).and_then(|parent| arena.get(parent)) {
        Some(parent) => !parent.config().makes_document_selections(),
        None => true,
    }
}
/// `id`'s position in render preorder, as its child index at each level from
/// the root down: comparing two paths lexicographically orders the nodes as a
/// preorder walk visits them. Survivors never contain one another, so no path
/// is a prefix of another's.
fn preorder_path(tree: &RenderTree, id: RenderId) -> Vec<usize> {
    let mut path = Vec::new();
    let mut cursor = id;
    while let Some(parent) = tree.parent(cursor) {
        let index = tree
            .children(parent)
            .iter()
            .position(|&child| child == cursor)
            .expect("BUG: a node is among its parent's children");
        path.push(index);
        cursor = parent;
    }
    path.reverse();
    path
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
    regions: &mut AssemblyRegions<'_>,
) -> Option<(SemanticsAssemblyContext, Matrix4, SemanticsClips, bool)> {
    assembly_inputs_into(tree, pipeline_root, target, regions, &mut Vec::new())
}

/// [`assembly_inputs_for`] over a caller's buffer for the ancestor chain.
fn assembly_inputs_into(
    tree: &RenderTree,
    pipeline_root: RenderId,
    target: RenderId,
    regions: &mut AssemblyRegions<'_>,
    chain: &mut Vec<RenderId>,
) -> Option<(SemanticsAssemblyContext, Matrix4, SemanticsClips, bool)> {
    chain.clear();
    chain.push(target);
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
        names_a_control: false,
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
                    false,
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
        context = assembly_decisions(&config, false, context).child_context;
    }

    // The chain always contains `target`, so the loop returns before
    // exhausting it.
    None
}

/// Re-assembles `anchor`'s render subtree and grafts the result into the
/// arena in place of the anchor's previous subtree: the anchor's arena slot
/// (and thus its parent's children list and its published identity) is
/// preserved; everything below is replaced.
fn plan_graft_anchor<'f>(
    tree: &RenderTree,
    pipeline_root: RenderId,
    owner: &SemanticsOwner,
    anchor: RenderId,
    regions: &mut AssemblyRegions<'f>,
) -> Option<(flui_foundation::SemanticsId, BuiltSemanticsNode<'f>)> {
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

/// Removes the nodes each graft of a pass replaces, and their entries in the
/// containment table. The arena reuses a removed node's id, so the table
/// drops them before any graft inserts a node; it is read once for the pass,
/// not once per graft.
pub(super) fn clear_graft_anchors(
    owner: &mut SemanticsOwner,
    grafts: &[(flui_foundation::SemanticsId, BuiltSemanticsNode<'_>)],
    uncontained: &mut FxHashSet<flui_foundation::SemanticsId>,
) {
    for &(anchor_sid, _) in grafts {
        let old_children: Vec<flui_foundation::SemanticsId> = owner
            .tree()
            .children(anchor_sid)
            .map_or_default(<[flui_foundation::SemanticsId]>::to_vec);
        for child in old_children {
            let _ = owner.tree_mut().remove(child);
        }
        uncontained.remove(&anchor_sid);
    }
    if !uncontained.is_empty() {
        uncontained.retain(|id| owner.tree().contains(*id));
    }
}

/// Places one graft into its anchor's slot, which
/// [`clear_graft_anchors`] emptied.
pub(super) fn apply_graft(
    owner: &mut SemanticsOwner,
    anchor_sid: flui_foundation::SemanticsId,
    built: BuiltSemanticsNode<'_>,
    regions: &RegionIdentities,
    uncontained: &mut FxHashSet<flui_foundation::SemanticsId>,
) {
    if !built.contained {
        uncontained.insert(anchor_sid);
    }
    let (node, children) = semantics_node_parts(built, regions);
    assert!(
        owner.tree_mut().replace_node(anchor_sid, node),
        "BUG: a staged graft retains its anchor slot"
    );
    for child in children {
        let child_sid = insert_built_semantics_node(owner, child, regions, uncontained);
        owner.add_child(anchor_sid, child_sid);
    }
}

/// Checks a pass that re-placed moved subtrees in their own slots against
/// what re-assembling the whole tree publishes: the two must export the same
/// AccessKit nodes. Run by default in debug builds with the `testing` feature,
/// so every test that moves
/// semantics checks the placement rules; a panic names the first node that
/// differs. The handlers the check's assembly retained are returned for the
/// caller to retire after the phase, as the pass's own are.
///
/// The owner's announcer nodes are not part of assembly, so the comparison
/// leaves them out of the placed tree. Skipped when the full assembly itself
/// refuses the tree, which the next ordinary mark reports.
#[cfg(any(test, feature = "testing"))]
pub(super) fn check_placement_against_rebuild(
    tree: &RenderTree,
    root: Option<RenderId>,
    owner: &SemanticsOwner,
    identities: &mut RegionIdentities,
) -> AssemblyCallbacks {
    let arena = FrameArena::new();
    let built_arena = FrameArena::new();
    let mut regions = AssemblyRegions::reusing(&arena, &built_arena, FxHashSet::default());
    let announcers: Vec<accesskit::NodeId> =
        identities
            .existing_announcements()
            .map_or_default(|announcements| {
                [
                    flui_semantics::Urgency::Polite,
                    flui_semantics::Urgency::Assertive,
                ]
                .map(|urgency| accesskit::NodeId(announcements.node_id(urgency).as_u64()))
                .to_vec()
            });
    let Ok(built) = assemble_semantics_root(tree, root, &mut regions) else {
        return std::mem::take(&mut regions.callbacks);
    };
    let mut plan = SemanticsPlan::Rebuild(built.map(|root| Box::new_in(root, &built_arena)));
    if let SemanticsPlan::Rebuild(Some(root)) = &mut plan {
        reconcile_collections(root);
    }
    if identities.prepare(&regions, tree).is_err() {
        return std::mem::take(&mut regions.callbacks);
    }
    identities.assign_text_runs(&mut plan, tree);
    let SemanticsPlan::Rebuild(built) = plan else {
        unreachable!("BUG: the check plans a rebuild");
    };
    let mut rebuilt = SemanticsOwner::new_without_callback();
    rebuilt.set_device_pixel_ratio(owner.device_pixel_ratio());
    let mut rebuilt_uncontained = FxHashSet::default();
    rebuild_semantics_owner(
        &mut rebuilt,
        built.map(|node| *node),
        identities,
        &mut rebuilt_uncontained,
    );

    let placed = owner.to_accesskit_tree_update(None);
    let expected = rebuilt.to_accesskit_tree_update(None);
    let nodes = |update: Option<accesskit::TreeUpdate>| {
        let mut nodes = update.map_or_default(|update| update.nodes);
        nodes.retain(|(id, _)| !announcers.contains(id));
        for (_, node) in &mut nodes {
            if node
                .children()
                .iter()
                .any(|child| announcers.contains(child))
            {
                let kept: Vec<accesskit::NodeId> = node
                    .children()
                    .iter()
                    .copied()
                    .filter(|child| !announcers.contains(child))
                    .collect();
                node.set_children(kept);
            }
        }
        nodes.sort_by_key(|(id, _)| id.0);
        nodes
    };
    let (placed, expected) = (nodes(placed), nodes(expected));
    if placed != expected {
        let first = placed
            .iter()
            .zip(&expected)
            .find(|(a, b)| a != b)
            .map_or_else(
                || format!("{} nodes placed, {} rebuilt", placed.len(), expected.len()),
                |(a, b)| format!("placed {a:?}\nrebuilt {b:?}"),
            );
        panic!(
            "BUG: re-placing moved semantics published another tree than a rebuild would\n{first}"
        );
    }
    // The two trees publish the same nodes under the same stable ids, so the
    // containment table the grafts kept must name the nodes a rebuild
    // records: a stale entry lets a later graft keep a stale
    // `clips_children` above its anchor.
    if identities.containment_known {
        let published = |tree: &flui_semantics::SemanticsTree,
                         ids: &FxHashSet<flui_foundation::SemanticsId>| {
            let mut stable: Vec<Option<u64>> = ids
                .iter()
                .map(|&id| {
                    tree.get(id)
                        .and_then(flui_semantics::SemanticsNode::accessibility_id)
                        .map(flui_semantics::AccessibilityNodeId::as_u64)
                })
                .collect();
            stable.sort_unstable();
            stable
        };
        let kept = published(owner.tree(), &identities.uncontained);
        let recorded = published(rebuilt.tree(), &rebuilt_uncontained);
        assert!(
            kept == recorded,
            "BUG: re-placing moved semantics kept another containment table than a rebuild \
             records\nkept {kept:?}\nrebuilt {recorded:?}"
        );
    }
    std::mem::take(&mut regions.callbacks)
}
