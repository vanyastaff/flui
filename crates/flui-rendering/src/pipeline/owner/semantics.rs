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
            let built = self.root_id.and_then(|root| {
                build_semantics_fragment(&self.render_tree, root, Offset::ZERO, true, false)
            });
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
    config: SemanticsConfiguration,
    rect: Rect<Pixels>,
    children: Vec<BuiltSemanticsNode>,
}

struct SemanticsFragment {
    merge_up: Option<(SemanticsConfiguration, Rect<Pixels>)>,
    nodes: Vec<BuiltSemanticsNode>,
}

impl SemanticsFragment {
    fn empty() -> Self {
        Self {
            merge_up: None,
            nodes: Vec::new(),
        }
    }
}

fn build_semantics_fragment(
    tree: &RenderTree,
    id: RenderId,
    origin: Offset,
    is_root: bool,
    force_merge: bool,
) -> Option<SemanticsFragment> {
    ensure_stack(|| build_semantics_fragment_impl(tree, id, origin, is_root, force_merge))
}

/// Body of [`build_semantics_fragment`].
///
/// `force_merge` is `true` while this call is inside an ancestor's
/// `is_merging_semantics_of_descendants` scope (ADR-0014 D3/Slice B):
/// once set, it suppresses this node's own boundary decision for the rest
/// of the subtree — even a nested node that independently declares
/// `is_semantics_boundary` folds into the merge-collapsing ancestor's
/// single node instead of spawning its own. This is the fix for the
/// conflated `is_semantics_boundary() || has_content()` predicate flagged
/// in the ADR: boundary-forming and "has content to merge up" are
/// decided independently here.
fn build_semantics_fragment_impl(
    tree: &RenderTree,
    id: RenderId,
    origin: Offset,
    is_root: bool,
    force_merge: bool,
) -> Option<SemanticsFragment> {
    let node = tree.get(id)?;
    let mut config = describe_semantics_configuration(node);
    let rect = node_semantics_rect(node, origin);

    let forms_boundary = !force_merge && (is_root || config.is_semantics_boundary());
    // Once a `MergeSemantics`-equivalent boundary starts a merge scope,
    // every descendant — however deep — inherits `force_merge` and can
    // never spawn its own node (Slice B: `is_merging_semantics_of_descendants`).
    let child_force_merge =
        force_merge || (forms_boundary && config.is_merging_semantics_of_descendants());

    let mut child_nodes = Vec::new();
    let mut merge_rect = rect;
    // D5: `excludes_semantics_subtree` (RenderExcludeSemantics parity) skips
    // this node's children entirely — the node's own config above is still
    // built and boundary/merge-decided normally.
    if !node_excludes_semantics_subtree(node) {
        for &child_id in node.children() {
            let Some(child) = tree.get(child_id) else {
                continue;
            };
            let child_origin = offset_add(origin, child.offset());
            let fragment =
                build_semantics_fragment(tree, child_id, child_origin, false, child_force_merge)
                    .unwrap_or_else(SemanticsFragment::empty);
            if let Some((child_config, child_rect)) = fragment.merge_up {
                config.absorb(&child_config);
                merge_rect = union_non_zero(merge_rect, child_rect);
            }
            child_nodes.extend(fragment.nodes);
        }
    }

    if forms_boundary {
        Some(SemanticsFragment {
            merge_up: None,
            nodes: vec![BuiltSemanticsNode {
                config,
                rect: merge_rect,
                children: child_nodes,
            }],
        })
    } else if force_merge || config.has_content() {
        // Under `force_merge` this node's (possibly empty) config and rect
        // always bubble up explicitly, even with no content of its own —
        // otherwise a content-less structural node inside a collapsed
        // subtree would silently drop its geometry from the merged rect.
        Some(SemanticsFragment {
            merge_up: Some((config, merge_rect)),
            nodes: child_nodes,
        })
    } else {
        Some(SemanticsFragment {
            merge_up: None,
            nodes: child_nodes,
        })
    }
}

fn rebuild_semantics_owner(owner: &mut SemanticsOwner, fragment: Option<SemanticsFragment>) {
    owner.clear();

    let Some(fragment) = fragment else {
        return;
    };

    let mut root = None;
    for node in fragment.nodes {
        let id = insert_built_semantics_node(owner, node);
        if root.is_none() {
            root = Some(id);
        }
    }
    owner.set_root(root);
}

fn insert_built_semantics_node(
    owner: &mut SemanticsOwner,
    built: BuiltSemanticsNode,
) -> flui_foundation::SemanticsId {
    let mut node = SemanticsNode::new().with_config(built.config);
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

fn union_non_zero(a: Rect<Pixels>, b: Rect<Pixels>) -> Rect<Pixels> {
    if a == Rect::ZERO {
        b
    } else if b == Rect::ZERO {
        a
    } else {
        a.union(&b)
    }
}
