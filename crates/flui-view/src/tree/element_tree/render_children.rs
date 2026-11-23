//! Render-tree wiring of the element tree: render-parent order after a build, lazy sliver children, and render subtree relocation.

use flui_foundation::arena::{FrameArena, FrameVec};
use flui_foundation::{ElementId, RenderId};
use flui_rendering::{parent_data::SliverMultiBoxAdaptorParentData, pipeline::PipelineCell};

use super::{ElementNode, ElementTree, InheritedScope, compute_inherited_scope};

/// The render-parent edge the post-build reorder pass wants for one render
/// node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenderEdge {
    /// The nearest render ancestor in the element graph, or `None` for a
    /// render root.
    Parent(Option<RenderId>),
    /// The first render node of a lazy sliver child: its edge and its place
    /// in the sliver's list belong to the child manager and the sliver's
    /// parent data, so the pass leaves both and settles only its children.
    Sparse,
}

fn append_sparse_sliver_children<A: std::alloc::Allocator + Copy>(
    render_tree: &flui_rendering::storage::RenderTree,
    parent_render: RenderId,
    desired_children: &mut Vec<RenderId, A>,
) {
    // Lazy sliver children intentionally stay out of ElementNode::child_ids;
    // their render order is recovered from SliverMultiBoxAdaptorParentData.
    let Some(parent_node) = render_tree.get(parent_render) else {
        return;
    };

    // A child the walk did not reach is a lazy sliver child when its parent
    // data says so; most parents have none, so membership in the walk's own
    // list is built only once the first such child turns up.
    let mut desired_membership: Option<flui_foundation::SlotSet<RenderId>> = None;
    let mut sparse_children: Vec<(usize, RenderId), A> = Vec::new_in(*desired_children.allocator());
    for &child in parent_node.children() {
        let Some(child_node) = render_tree.get(child) else {
            continue;
        };
        if render_tree.parent(child) != Some(parent_render) {
            continue;
        }
        let Some(index) = child_node
            .parent_data()
            .and_then(|data| data.downcast_ref::<SliverMultiBoxAdaptorParentData>())
            .map(|data| data.index)
        else {
            continue;
        };
        let desired = desired_membership.get_or_insert_with(|| {
            let mut desired = flui_foundation::SlotSet::new();
            for &child in desired_children.iter() {
                desired.insert(child);
            }
            desired
        });
        if !desired.contains(child) {
            if sparse_children.capacity() == 0 {
                // Reserved before the sparse list, so in an arena the sparse list frees last.
                desired_children.reserve(parent_node.children().len());
            }
            sparse_children.push((index, child));
        }
    }

    sparse_children.sort_unstable_by_key(|&(index, child)| (index, child));
    desired_children.reserve(sparse_children.len());
    for &(_, child) in &sparse_children {
        desired_children.push(child);
    }
}

/// The lazy sliver child `from` lives inside — the direct child of the nearest
/// element that hosts sparse children.
///
/// **Not** the nearest ancestor carrying a `sliver_slot`: a component element
/// passes its slot down to its own children, so several elements in one item
/// carry the same value and the nearest is usually a descendant
/// `SparseChildren` has never heard of. Nesting resolves correctly for the same
/// reason — the *inner* list's host is found first.
///
/// Self-inclusive, because an item whose parent is the host is itself that
/// child. Ancestors are read through `element_opt`, so a node extracted by
/// `build_scope`'s take/put window is a clean miss rather than a panic — the
/// contract for every walk reachable during a build.
///
/// Shared by the keep-alive capability (which resolves it when a lease is
/// taken) and by band eviction (which re-resolves it for every live holder, so
/// a holder grafted between slivers is re-targeted rather than pinning the row
/// it left). Two copies of this rule would drift, and the drift would be a
/// silent leak.
pub(crate) fn enclosing_sparse_child(tree: &ElementTree, from: ElementId) -> Option<ElementId> {
    let mut current_id = from;
    loop {
        let parent_id = tree.get(current_id)?.parent()?;
        let hosts = tree
            .get(parent_id)?
            .element_opt()
            .is_some_and(crate::view::ElementBase::hosts_sparse_children);
        if hosts {
            return Some(current_id);
        }
        current_id = parent_id;
    }
}

impl ElementTree {
    /// Reorder every render object's children to match element-slot order.
    ///
    /// A render child appends itself to its render parent during mount, so when
    /// a component ancestor (a `StatelessView` / `ParentDataView`) builds its
    /// render descendant in a *later* `build_scope` iteration than a render
    /// sibling that already attached, the parent's children list ends up in
    /// attach order, not slot order. This single post-build pass walks the
    /// element tree depth-first in slot order, derives each render parent's
    /// correct child sequence, and rewrites only those that drifted.
    ///
    /// No-op unless insertion or committed child reconciliation set
    /// `needs_render_reorder`.
    /// Average/worst case O(element-tree size) for the DFS, plus O(children) per
    /// drifted render parent. The element walk completes before the pipeline
    /// owner is locked; the lock guards only the render tree.
    ///
    /// # Exempt from [`adopt_child`](flui_rendering::storage::RenderTree::adopt_child)
    ///
    /// `adopt_child` is the single-edge primitive for attaching ONE freshly
    /// mounted child under its parent. This pass is a different shape: a
    /// bulk diff of a parent's WHOLE children list against the element-slot
    /// order, so it writes the parent-pointer sync loop and the
    /// children-list rewrite loop separately rather than one `adopt_child`
    /// call per child — an `adopt_child`-per-child here would re-derive the
    /// same target list one element at a time for no benefit, and would
    /// still need the separate stale-child eviction the children-list loop
    /// already does. The two loops are still required to leave both edge
    /// directions consistent; a debug-only invariant check at the end of
    /// this function proves that rather than trusting it silently (this is
    /// exactly the invariant the root-hop parent-link defect violated: a
    /// child recorded in its parent's children list while its own `parent`
    /// link disagreed).
    pub(crate) fn reorder_render_children_after_build(
        &mut self,
        registry: &crate::element::child_manager::ChildManagerRegistry,
        arena: &mut FrameArena,
    ) -> usize {
        if !self.needs_render_reorder {
            return 0;
        }
        self.needs_render_reorder = false;
        // Reset after the walk: between frames the arena holds one chunk, not the chain that grew.
        let reordered = self.synchronize_render_children(registry, arena);
        arena.reset();
        reordered
    }

    pub(in crate::tree) fn mark_render_reorder_needed(&mut self) {
        self.needs_render_reorder = true;
    }

    /// Synchronize only the destination's nearest render parent's direct
    /// children for the provisional GlobalKey order.
    ///
    /// This must run before the moved element's update and observer callbacks,
    /// but a global element-tree walk would make each retake O(tree size). The
    /// walk therefore starts at the destination render element, follows only
    /// its logical descendants, and stops at each first render boundary.
    pub(super) fn synchronize_destination_render_children_for_relocation(
        &mut self,
        parent_id: ElementId,
        provisional_children: &[ElementId],
    ) -> usize {
        let mut render_element_id = Some(parent_id);
        let (render_element_id, render_parent_id, pipeline) = loop {
            let Some(element_id) = render_element_id else {
                return 0;
            };
            let Some(node) = self.get(element_id) else {
                return 0;
            };
            if let Some(render_id) = node.element().render_id() {
                let Some(pipeline) = node.element().pipeline_owner() else {
                    return 0;
                };
                break (element_id, render_id, pipeline);
            }
            render_element_id = node.parent();
        };

        let root_children = if render_element_id == parent_id {
            provisional_children
        } else {
            self.get(render_element_id)
                .map_or(&[][..], ElementNode::child_ids)
        };
        let mut desired_children = Vec::new();
        let mut visited = 1;
        let mut stack: Vec<ElementId> = root_children.iter().rev().copied().collect();
        while let Some(element_id) = stack.pop() {
            visited += 1;
            let Some(node) = self.get(element_id) else {
                continue;
            };
            if let Some(render_id) = node.element().render_id() {
                desired_children.push(render_id);
                continue;
            }
            let children = if element_id == parent_id {
                provisional_children
            } else {
                node.child_ids()
            };
            stack.extend(children.iter().rev().copied());
        }

        pipeline.with_mut(|owner| {
            append_sparse_sliver_children(
                owner.render_tree(),
                render_parent_id,
                &mut desired_children,
            );
            let _ = owner.set_render_children(render_parent_id, &desired_children);
        });
        visited
    }

    fn synchronize_render_children(
        &mut self,
        registry: &crate::element::child_manager::ChildManagerRegistry,
        arena: &FrameArena,
    ) -> usize {
        // Depth-first in slot order, tracking each node's nearest render
        // ancestor. A render node is appended to that ancestor's target order,
        // then becomes the render ancestor for its own subtree.
        //
        // The walk also descends into a lazy sliver's resident children. They
        // stay out of `child_ids`, and the sliver's own child list is ordered
        // by its parent data (`append_sparse_sliver_children`), so the first
        // render node of each such child keeps its edge to the sliver as the
        // child manager left it; only the render parents inside it are
        // synchronized. A row built in a sliver's layout-time build has the
        // same component-deferred attach order as one built anywhere else.
        //
        // Each render parent's target order is the run of `(parent, child)` edges a stable sort
        // by parent leaves together, in walk order: one list for the whole walk, not one per
        // parent. Every list comes from the build owner's frame arena (ADR-0328).
        let mut target: FrameVec<'_, (RenderId, RenderId)> = Vec::new_in(arena);
        // Every render node with its desired render parent, in depth-first
        // pre-order: a parent's own edge is settled before its children
        // move under it, so no step of the sync closes a cycle.
        let mut desired_parent: FrameVec<'_, (flui_foundation::RenderId, RenderEdge)> =
            Vec::new_in(arena);
        let mut pipeline_owner: Option<PipelineCell> = None;
        let mut visited = 0;

        // Children are pushed reversed so siblings pop in ascending slot order.
        // The third field is `RenderEdge::Sparse` from a lazy child down to its
        // first render node.
        let mut stack: FrameVec<'_, (ElementId, Option<flui_foundation::RenderId>, RenderEdge)> =
            Vec::new_in(arena);
        stack.extend(
            self.iter_nodes()
                .filter(|(_, node)| node.parent.is_none())
                .map(|(id, _)| (id, None, RenderEdge::Parent(None))),
        );
        stack.reverse();
        while let Some((element_id, render_ancestor, edge)) = stack.pop() {
            visited += 1;
            let Some(node) = self.get(element_id) else {
                continue;
            };
            let (child_ancestor, child_edge) = if let Some(render_id) = node.element().render_id() {
                match edge {
                    RenderEdge::Sparse => desired_parent.push((render_id, RenderEdge::Sparse)),
                    RenderEdge::Parent(_) => {
                        desired_parent.push((render_id, RenderEdge::Parent(render_ancestor)));
                        if let Some(parent_render) = render_ancestor {
                            target.push((parent_render, render_id));
                        }
                    }
                }
                if pipeline_owner.is_none() {
                    pipeline_owner = node.element().pipeline_owner();
                }
                (Some(render_id), RenderEdge::Parent(Some(render_id)))
            } else {
                (render_ancestor, edge)
            };
            for child in self.sparse_children(element_id, registry).into_iter().rev() {
                stack.push((child, child_ancestor, RenderEdge::Sparse));
            }
            let children = node.child_ids();
            for &child in children.iter().rev() {
                stack.push((child, child_ancestor, child_edge));
            }
        }

        if desired_parent.is_empty() {
            return visited;
        }
        let Some(pipeline_owner) = pipeline_owner else {
            return visited;
        };

        drop(stack);
        target.sort_by_key(|&(parent, _)| parent);
        let mut desired_children: FrameVec<'_, RenderId> = Vec::new_in(arena);

        // Tree borrows are dropped; the checkout guards only the render tree.
        // The owner marks each parent the batch changed once it ends.
        pipeline_owner.with_mut(|owner| {
            owner.edit_render_children(|edit| {
                for &(render_id, desired) in &desired_parent {
                    if !edit.tree().contains(render_id) {
                        continue;
                    }
                    // A render root in the element graph leaves the render
                    // parent it still holds.
                    if desired == RenderEdge::Parent(None)
                        && let Some(current) = edit.tree().parent(render_id)
                    {
                        edit.detach_child(current, render_id);
                    }

                    // Every element-managed render node's child list follows
                    // element slot order exactly. A node absent from `target`
                    // is a render leaf in the element graph, so its desired
                    // list holds only its sparse sliver children; anything
                    // else it still lists becomes a root. A child arriving
                    // from another render parent leaves that donor's list.
                    desired_children.clear();
                    let first = target.partition_point(|&(parent, _)| parent < render_id);
                    desired_children.extend(
                        target[first..]
                            .iter()
                            .take_while(|&&(parent, _)| parent == render_id)
                            .map(|&(_, child)| child),
                    );
                    append_sparse_sliver_children(edit.tree(), render_id, &mut desired_children);
                    let _ = edit.set_children(render_id, &desired_children);
                }

                #[cfg(debug_assertions)]
                for &(render_id, desired) in &desired_parent {
                    let RenderEdge::Parent(desired) = desired else {
                        continue;
                    };
                    if !edit.tree().contains(render_id) {
                        continue;
                    }
                    debug_assert_eq!(
                        edit.tree().parent(render_id),
                        desired,
                        "reorder pass: render node {render_id:?}'s parent link disagrees with the \
                         desired parent the DFS computed for it"
                    );
                }
            });
        });
        visited
    }

    /// Recompute ancestry-derived metadata for the subtree rooted at
    /// `root_id`, top-down against each node's current parent.
    ///
    /// Needed after a GlobalKey reparent ([`try_retake_global_key`](super::global_key_retake::try_retake_global_key)): the moved
    /// subtree carries both inherited scopes and depths derived from its OLD
    /// ancestor chain. Leaving either stale can resolve the wrong provider or
    /// let the dirty heap rebuild a descendant before its moved parent.
    ///
    /// A node is processed only after its parent, so each child observes the
    /// parent's already-updated scope and depth. Depth and inherited scope are
    /// both refreshed while reactivating a subtree.
    /// Average/worst case O(subtree size), paid only for a reparent.
    /// Move a lazy sliver's resident child to a new logical index without
    /// remounting it: the node's slot becomes `new_slot`, and the inherited
    /// `sliver_slot` is re-derived down its composite chain (the same walk a
    /// GlobalKey relocation runs). The caller re-stamps the render
    /// descendants afterwards — `stamp_sliver_logical_index` reads the new
    /// index, the walk cannot. Ordering is load-bearing: the ancestry walk
    /// derives each child's `sliver_slot` from its parent's slot, so the
    /// slot must be written first.
    pub(crate) fn relocate_sparse_child(
        &mut self,
        id: ElementId,
        new_slot: usize,
        owner: &crate::ElementOwner<'_>,
    ) {
        if let Some(node) = self.get_mut(id) {
            node.slot = new_slot;
        }
        self.recompute_subtree_ancestry(id, owner.child_manager_registry);
    }

    pub(super) fn recompute_subtree_ancestry(
        &mut self,
        root_id: ElementId,
        registry: &crate::element::child_manager::ChildManagerRegistry,
    ) {
        // A `visited` set bounds the walk to each node once. The element tree
        // is acyclic by construction (`child_ids` come from the reconciler),
        // so this never trips in practice — but it converts a malformed
        // `child_ids` cycle from an unbounded hang into clean termination.
        let mut visited = flui_foundation::SlotSet::new();
        let mut stack = vec![root_id];
        while let Some(id) = stack.pop() {
            if !visited.insert(id) {
                continue;
            }
            let Some(node) = self.get(id) else {
                continue;
            };
            let node_slot = node.slot;
            let (parent_map, depth, parent_render_id, sliver_slot) = match node.parent {
                Some(parent_id) => self.get(parent_id).map_or_else(
                    || (InheritedScope::EMPTY, 0, None, None),
                    |parent| {
                        (
                            parent.inherited.clone(),
                            parent.depth() + 1,
                            parent.element().child_render_id(),
                            parent.element().child_sliver_slot(node_slot),
                        )
                    },
                ),
                None => (InheritedScope::EMPTY, 0, None, None),
            };
            let scope = {
                let node = self.get(id).expect("id resolved at loop top");
                compute_inherited_scope(&parent_map, node.element(), id)
            };
            let node = self.get_mut(id).expect("id resolved at loop top");
            node.inherited = scope;
            node.set_depth(depth);
            node.element_mut().set_parent_render_id(parent_render_id);
            node.element_mut().set_sliver_slot(sliver_slot);
            self.extend_owned_children(id, registry, &mut stack);
        }
    }
}

#[derive(Debug)]
pub(super) struct RenderRelocation {
    pub(super) pipeline: Option<PipelineCell>,
    pub(super) frontier: Vec<(ElementId, RenderId)>,
    pub(super) detached_render_subtrees: Option<flui_rendering::pipeline::DetachedRenderSubtrees>,
}

pub(super) fn collect_render_frontier(
    tree: &ElementTree,
    candidate_id: ElementId,
) -> Option<Vec<(ElementId, RenderId)>> {
    let mut frontier = Vec::new();
    let mut stack = vec![candidate_id];
    while let Some(element_id) = stack.pop() {
        let node = tree.get(element_id)?;
        if let Some(render_id) = node.element().render_id() {
            frontier.push((element_id, render_id));
            continue;
        }
        stack.extend(node.child_ids().iter().rev().copied());
    }
    Some(frontier)
}

pub(super) fn preflight_render_relocation(
    tree: &ElementTree,
    candidate_id: ElementId,
    new_parent: ElementId,
) -> Option<RenderRelocation> {
    // The destination cannot be inside the candidate's own element subtree.
    let mut cursor = Some(new_parent);
    while let Some(element_id) = cursor {
        if element_id == candidate_id {
            return None;
        }
        cursor = tree.get(element_id).and_then(ElementNode::parent);
    }

    let candidate = tree.get(candidate_id)?;
    let destination = tree.get(new_parent)?;
    let candidate_pipeline = candidate.element().pipeline_owner();
    let destination_pipeline = destination.element().pipeline_owner();
    match (&candidate_pipeline, &destination_pipeline) {
        (Some(candidate), Some(destination)) if !candidate.ptr_eq(destination) => return None,
        (Some(_), None) | (None, Some(_)) => return None,
        _ => {}
    }

    let frontier = collect_render_frontier(tree, candidate_id)?;

    if let Some(pipeline) = &candidate_pipeline {
        let destination_render_parent = destination.element().child_render_id();
        let is_valid = pipeline.with(|owner| {
            let render_tree = owner.render_tree();
            let mut moved_render_nodes = flui_foundation::SlotSet::new();
            let mut stack = Vec::new();
            for &(_, render_root) in &frontier {
                if !render_tree.contains(render_root) {
                    return false;
                }
                stack.push(render_root);
                while let Some(render_id) = stack.pop() {
                    if !moved_render_nodes.insert(render_id)
                        || destination_render_parent == Some(render_id)
                    {
                        return false;
                    }
                    let Some(node) = render_tree.get(render_id) else {
                        return false;
                    };
                    stack.extend(node.children().iter().rev().copied());
                }
            }
            true
        });
        if !is_valid {
            return None;
        }
    } else if !frontier.is_empty() {
        return None;
    }

    Some(RenderRelocation {
        pipeline: candidate_pipeline,
        frontier,
        detached_render_subtrees: None,
    })
}

pub(super) fn detach_render_relocation(relocation: &mut RenderRelocation) {
    if relocation.frontier.is_empty() {
        return;
    }
    let Some(pipeline) = &relocation.pipeline else {
        return;
    };
    let token = pipeline.with_mut(|owner| {
        let roots: Vec<_> = relocation.frontier.iter().map(|&(_, root)| root).collect();
        owner
            .detach_render_subtrees(&roots)
            .expect("BUG: render relocation preflight must guarantee a detachable disjoint batch")
    });
    relocation.detached_render_subtrees = Some(token);
}

pub(super) fn attach_render_relocation(relocation: &mut RenderRelocation) {
    if relocation.frontier.is_empty() {
        return;
    }
    let Some(pipeline) = &relocation.pipeline else {
        return;
    };
    let token = relocation
        .detached_render_subtrees
        .take()
        .expect("BUG: a render relocation with a pipeline must own its detached-subtree token");
    pipeline.with_mut(|owner| {
        owner.attach_render_subtrees(token).expect(
            "BUG: destination edge synchronization must preserve the detached render batch",
        );
    });
}
