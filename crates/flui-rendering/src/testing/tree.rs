//! Declarative render-tree specification shared by both protocols.
//!
//! A [`TreeNode`] is a protocol-tagged spec: it carries either a
//! [`BoxProtocol`] or a [`SliverProtocol`] render object, an optional
//! `&'static str` label, and an ordered list of child specs. [`mount`]
//! walks the spec top-down and inserts every node into a live
//! [`PipelineOwner`], dispatching to the correct insertion API by the
//! *child's* protocol:
//!
//! - Box child -> [`PipelineOwner::insert_child_render_object`] (full dirty
//!   tracking, the same path the box pipeline tests use);
//! - Sliver child -> [`crate::storage::RenderTree::insert_sliver_child`]
//!   (the path the sliver tests use; the root layout pass cascades into
//!   the sliver subtree).
//!
//! The tree root must be a Box render object — the layout pass drives the
//! root via [`BoxConstraints`](crate::constraints::BoxConstraints), and
//! slivers are laid out by a Box viewport/host parent, never as a bare
//! root.

use std::collections::HashMap;

use flui_foundation::RenderId;

use crate::{
    parent_data::{FlexParentData, StackParentData},
    pipeline::{Idle, PipelineOwner},
    protocol::{BoxProtocol, SliverProtocol},
    testing::parent_data::ParentDataSeed,
    traits::RenderObject,
};

/// The render-object payload carried by a [`TreeNode`], tagged by protocol.
enum NodePayload {
    /// A Box-protocol render object.
    Box(Box<dyn RenderObject<BoxProtocol>>),
    /// A Sliver-protocol render object.
    Sliver(Box<dyn RenderObject<SliverProtocol>>),
}

/// A single node in a declarative render-tree spec.
///
/// Build one with [`box_node`] or [`sliver_node`], attach children with
/// [`child`](TreeNode::child) / [`children`](TreeNode::children), and tag a
/// node with [`label`](TreeNode::label) so it can be looked up by name
/// after mounting (see [`crate::testing::Probe::id`]).
pub struct TreeNode {
    payload: NodePayload,
    label: Option<&'static str>,
    /// Parent metadata the child's layout parent reads during
    /// `perform_layout` (stack positioning, flex factor, …).
    parent_data_seed: Option<ParentDataSeed>,
    children: Vec<TreeNode>,
}

/// Creates a Box-protocol node from any concrete `RenderBox`-derived render
/// object.
pub fn box_node<R>(render_object: R) -> TreeNode
where
    R: RenderObject<BoxProtocol> + 'static,
{
    box_node_boxed(Box::new(render_object))
}

/// Creates a Box-protocol node from an already-boxed trait object.
///
/// Use this when the render object is only available as a
/// `Box<dyn RenderObject<BoxProtocol>>` (e.g. produced by a factory).
pub fn box_node_boxed(render_object: Box<dyn RenderObject<BoxProtocol>>) -> TreeNode {
    TreeNode {
        payload: NodePayload::Box(render_object),
        label: None,
        parent_data_seed: None,
        children: Vec::new(),
    }
}

/// Creates a Sliver-protocol node from any concrete `RenderSliver`-derived
/// render object.
pub fn sliver_node<R>(render_object: R) -> TreeNode
where
    R: RenderObject<SliverProtocol> + 'static,
{
    sliver_node_boxed(Box::new(render_object))
}

/// Creates a Sliver-protocol node from an already-boxed trait object.
pub fn sliver_node_boxed(render_object: Box<dyn RenderObject<SliverProtocol>>) -> TreeNode {
    TreeNode {
        payload: NodePayload::Sliver(render_object),
        label: None,
        parent_data_seed: None,
        children: Vec::new(),
    }
}

impl TreeNode {
    /// Tags this node with a label so it can be resolved to its `RenderId`
    /// after mounting via [`crate::testing::Probe::id`].
    #[must_use]
    pub fn label(mut self, label: &'static str) -> Self {
        self.label = Some(label);
        self
    }

    /// Appends a single child spec.
    #[must_use]
    pub fn child(mut self, child: TreeNode) -> Self {
        self.children.push(child);
        self
    }

    /// Appends every child spec from an iterator.
    #[must_use]
    pub fn children(mut self, children: impl IntoIterator<Item = TreeNode>) -> Self {
        self.children.extend(children);
        self
    }

    /// Attaches a [`ParentDataSeed`] cloned into the pipeline before each
    /// layout walk so parents that read child parent data (stack, flex, …)
    /// see widget-level configuration in headless tests.
    #[must_use]
    pub fn with_parent_data_seed(mut self, seed: ParentDataSeed) -> Self {
        self.parent_data_seed = Some(seed);
        self
    }

    /// Convenience wrapper for [`StackParentData`] on [`RenderStack`] children.
    #[must_use]
    pub fn with_stack_parent_data(self, data: StackParentData) -> Self {
        self.with_parent_data_seed(ParentDataSeed::Stack(data))
    }

    /// Convenience wrapper for [`FlexParentData`] on [`RenderFlex`] children.
    #[must_use]
    pub fn with_flex_parent_data(self, data: FlexParentData) -> Self {
        self.with_parent_data_seed(ParentDataSeed::Flex(data))
    }
}

/// Maps `&'static str` labels to the `RenderId`s minted while mounting a
/// [`TreeNode`] spec.
///
/// Produced by [`mount`] and carried by the run results so assertions can
/// reference nodes by name instead of threading raw ids around.
#[derive(Debug, Default, Clone)]
pub struct RenderLabelRegistry {
    by_label: HashMap<&'static str, RenderId>,
}

impl RenderLabelRegistry {
    /// Records a label -> id mapping, panicking on a duplicate label (a
    /// duplicate is a test-authoring bug, not a runtime condition).
    fn record(&mut self, label: &'static str, id: RenderId) {
        if self.by_label.insert(label, id).is_some() {
            panic!("duplicate node label in test tree: {label:?}");
        }
    }

    /// Returns the id for `label`, if one was registered.
    #[must_use]
    pub fn get(&self, label: &str) -> Option<RenderId> {
        self.by_label.get(label).copied()
    }
}

/// Inserts a [`TreeNode`] spec into `owner`, returning the root id and the
/// label registry.
///
/// The root must be a Box node; a Sliver root panics with a clear message.
pub fn mount(owner: &mut PipelineOwner<Idle>, spec: TreeNode) -> (RenderId, RenderLabelRegistry) {
    let mut registry = RenderLabelRegistry::default();
    let root_id = match spec.payload {
        NodePayload::Box(render_object) => owner.insert(render_object),
        NodePayload::Sliver(_) => panic!(
            "the root of a test tree must be a Box render object; slivers are driven \
             by a Box viewport/host root (mount a `box_node(..)` and nest the sliver under it)"
        ),
    };
    if let Some(label) = spec.label {
        registry.record(label, root_id);
    }
    if let Some(seed) = spec.parent_data_seed {
        owner.seed_parent_data(root_id, seed);
    }
    for child in spec.children {
        mount_child(owner, root_id, child, &mut registry);
    }
    (root_id, registry)
}

/// Recursively inserts `spec` as a child of `parent_id`.
fn mount_child(
    owner: &mut PipelineOwner<Idle>,
    parent_id: RenderId,
    spec: TreeNode,
    registry: &mut RenderLabelRegistry,
) {
    let id = match spec.payload {
        NodePayload::Box(render_object) => owner
            .insert_child_render_object(parent_id, render_object)
            .expect("Box child insert must succeed: the parent id was just inserted and is valid"),
        NodePayload::Sliver(render_object) => owner
            .render_tree_mut()
            .insert_sliver_child(parent_id, render_object)
            .expect(
                "Sliver child insert must succeed: the parent id was just inserted and is valid",
            ),
    };
    if let Some(label) = spec.label {
        registry.record(label, id);
    }
    if let Some(seed) = spec.parent_data_seed {
        owner.seed_parent_data(id, seed);
    }
    for child in spec.children {
        mount_child(owner, id, child, registry);
    }
}
