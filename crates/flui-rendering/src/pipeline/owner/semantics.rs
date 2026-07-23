//! Semantics phase implementation for `PipelineOwner<Semantics>`.

use flui_foundation::RenderId;
use flui_semantics::{SemanticsConfiguration, SemanticsNode, SemanticsOwner};
use flui_types::{Offset, Point, Rect, Size, geometry::Pixels};

use crate::{
    pipeline::{
        phase::{Idle, Semantics},
        scheduler::PhaseKind,
    },
    storage::{RenderNode, RenderTree},
};

use super::{PipelineOwner, rebind_phase, subtree_arena::ensure_stack};

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

        let pending_count = self
            .scheduler
            .nodes_needing_semantics()
            .iter()
            .filter(|d| self.render_tree.contains(d.id))
            .count();

        let tree_is_empty = self
            .semantics_owner
            .as_ref()
            .is_some_and(|owner| owner.tree().is_empty());
        let should_build = pending_count > 0 || tree_is_empty;

        if should_build {
            let built = match assemble_semantics_root(&self.render_tree, self.root_id) {
                Ok(built) => built,
                Err(error) => {
                    let _ = self.scheduler.exit_phase(PhaseKind::Semantics);
                    return Err(crate::error::RenderError::semantics(error.to_string()));
                }
            };
            if let Some(owner) = self.semantics_owner.as_mut() {
                rebuild_semantics_owner(owner, built);
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

/// Body of [`build_semantics_fragments`].
fn build_semantics_fragments_impl(
    tree: &RenderTree,
    id: RenderId,
    origin: Offset,
    context: SemanticsAssemblyContext,
    ancestor_blocks_user_actions: bool,
) -> Option<Vec<SemanticsFragment>> {
    let node = tree.get(id)?;
    let mut config = describe_semantics_configuration(node);
    let blocks_user_actions = ancestor_blocks_user_actions || config.blocks_user_actions();
    config.set_blocks_user_actions(blocks_user_actions);
    let rect = node_semantics_rect(node, origin);

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
                SemanticsAssemblyContext {
                    is_root: false,
                    parent_requires_explicit_node: children_require_explicit_node,
                    merge_into_ancestor: children_merge_into_ancestor,
                },
                blocks_user_actions,
            )
            .unwrap_or_default();
            child_fragments.append(&mut fragments);
        }
    }

    if !contributes {
        return Some(child_fragments);
    }

    let children =
        merge_child_fragments(&mut config, child_fragments, children_merge_into_ancestor);
    let pending = PendingSemanticsNode {
        source_render_id: id,
        config,
        rect,
        children,
    };

    Some(vec![if forms_node {
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

fn insert_built_semantics_node(
    owner: &mut SemanticsOwner,
    built: BuiltSemanticsNode,
) -> flui_foundation::SemanticsId {
    let mut node = SemanticsNode::new()
        .with_source_render_id(built.source_render_id)
        .with_config(built.config);
    node.set_rect(built.rect);
    let id = owner.insert(node);
    for child in built.children {
        let child_id = insert_built_semantics_node(owner, child);
        owner.add_child(id, child_id);
    }
    id
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
