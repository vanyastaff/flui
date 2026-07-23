//! Integration coverage for the semantics assembly walk.
//!
//! Exercises `PipelineOwner<Semantics>::run_semantics` through the real
//! pipeline (`RenderTester::run_to_semantics`) against small test-only
//! render objects that override `describe_semantics_configuration` /
//! `excludes_semantics_subtree` directly. The
//! `merge_semantics_collapses_a_nested_boundary_descendant` test specifically
//! targets the boundary-vs-merge distinction: a naive
//! `is_semantics_boundary() || has_been_annotated()` predicate, or a boundary
//! decision that ignores `is_merging_semantics_of_descendants`, would
//! leave the nested boundary child as its own node).

use std::sync::Arc;

use flui_foundation::RenderId;
use flui_rendering::{
    constraints::BoxConstraints,
    context::BoxLayoutContext,
    parent_data::BoxParentData,
    semantics::{
        AccessibilityNodeId, AttributedString, SemanticsAction, SemanticsConfiguration,
        SemanticsFlag, SemanticsHintOverrides, SemanticsNodeSnapshot, SemanticsRole,
        SemanticsSnapshot,
    },
    testing::{FrameRun, Probe, RenderTester, box_node},
    traits::RenderBox,
};
use flui_tree::{Leaf, Variable};
use flui_types::{
    Offset, Point, Rect, Size,
    geometry::{Pixels, px},
};

/// A fixed-size leaf that reports a configurable `SemanticsConfiguration`.
///
/// Stands in for a `Semantics`/button-like leaf widget — the real
/// `RenderSemanticsAnnotations` is not built yet.
#[derive(Debug, Default)]
struct SemanticsLeaf {
    side: f32,
    configuration: Option<SemanticsConfiguration>,
    label: Option<&'static str>,
    button: bool,
    boundary: bool,
    tap_action: bool,
    accessibility_focus_action: bool,
    block_user_actions: bool,
    role: Option<SemanticsRole>,
}

impl SemanticsLeaf {
    fn new(side: f32) -> Self {
        Self {
            side,
            ..Default::default()
        }
    }

    fn with_label(mut self, label: &'static str) -> Self {
        self.label = Some(label);
        self
    }

    fn with_configuration(mut self, configuration: SemanticsConfiguration) -> Self {
        self.configuration = Some(configuration);
        self
    }

    fn with_button(mut self) -> Self {
        self.button = true;
        self
    }

    /// Declares this leaf its own semantics boundary — simulates a nested
    /// `container: true` `Semantics` widget (or another accidental
    /// boundary) appearing *inside* a `MergeSemantics` subtree.
    fn with_boundary(mut self) -> Self {
        self.boundary = true;
        self
    }

    fn with_tap_action(mut self) -> Self {
        self.tap_action = true;
        self
    }

    fn with_accessibility_focus_action(mut self) -> Self {
        self.accessibility_focus_action = true;
        self
    }

    fn with_blocked_user_actions(mut self) -> Self {
        self.block_user_actions = true;
        self
    }

    fn with_role(mut self, role: SemanticsRole) -> Self {
        self.role = Some(role);
        self
    }
}

impl flui_foundation::Diagnosticable for SemanticsLeaf {}

impl RenderBox for SemanticsLeaf {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        ctx.constraints()
            .constrain(Size::new(px(self.side), px(self.side)))
    }

    fn describe_semantics_configuration(&self, config: &mut SemanticsConfiguration) {
        if let Some(configuration) = &self.configuration {
            *config = configuration.clone();
        }
        if self.boundary {
            config.set_semantics_boundary(true);
        }
        if let Some(label) = self.label {
            config.set_label(label);
        }
        if self.button {
            config.set_button(true);
        }
        if self.tap_action {
            config.add_action(SemanticsAction::Tap, Arc::new(|_, _| {}));
        }
        if self.accessibility_focus_action {
            config.add_action(
                SemanticsAction::DidGainAccessibilityFocus,
                Arc::new(|_, _| {}),
            );
        }
        if self.block_user_actions {
            config.set_blocks_user_actions(true);
        }
        if let Some(role) = self.role {
            config.set_role(role);
        }
    }
}

/// A pass-through container (`Variable` arity) standing in for the
/// not-yet-built `RenderMergeSemantics` / `RenderExcludeSemantics`:
/// it can declare itself a semantics boundary, a
/// merge-descendants boundary, or an excluded subtree, purely to exercise
/// the assembly walk's boundary/merge decisions.
#[derive(Debug, Default)]
struct SemanticsContainer {
    configuration: Option<SemanticsConfiguration>,
    boundary: bool,
    explicit_child_nodes: bool,
    merging_descendants: bool,
    excludes_subtree: bool,
    block_user_actions: bool,
    side: Option<f32>,
    child_offset: Option<Offset>,
}

impl SemanticsContainer {
    /// `RenderMergeSemantics` parity: `isSemanticBoundary = true` and
    /// `isMergingSemanticsOfDescendants = true`.
    fn merge_semantics() -> Self {
        Self {
            boundary: true,
            merging_descendants: true,
            ..Default::default()
        }
    }

    fn with_configuration(mut self, configuration: SemanticsConfiguration) -> Self {
        self.configuration = Some(configuration);
        self
    }

    fn with_boundary(mut self) -> Self {
        self.boundary = true;
        self
    }

    fn with_explicit_child_nodes(mut self, value: bool) -> Self {
        self.explicit_child_nodes = value;
        self
    }

    fn with_side(mut self, side: f32) -> Self {
        self.side = Some(side);
        self
    }

    fn with_child_offset(mut self, dx: f32, dy: f32) -> Self {
        self.child_offset = Some(Offset::new(px(dx), px(dy)));
        self
    }

    fn with_blocked_user_actions(mut self) -> Self {
        self.block_user_actions = true;
        self
    }

    /// `RenderExcludeSemantics` parity: drops the child subtree from the
    /// walk without itself becoming a boundary.
    fn exclude_semantics() -> Self {
        Self {
            excludes_subtree: true,
            ..Default::default()
        }
    }
}

impl flui_foundation::Diagnosticable for SemanticsContainer {}

impl RenderBox for SemanticsContainer {
    type Arity = Variable;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        for i in 0..ctx.child_count() {
            ctx.layout_child(i, constraints.loosen());
            ctx.position_child(i, self.child_offset.unwrap_or(Offset::ZERO));
        }
        self.side.map_or_else(
            || constraints.biggest(),
            |side| constraints.constrain(Size::new(px(side), px(side))),
        )
    }

    fn describe_semantics_configuration(&self, config: &mut SemanticsConfiguration) {
        if let Some(configuration) = &self.configuration {
            *config = configuration.clone();
        }
        if self.boundary {
            config.set_semantics_boundary(true);
        }
        if self.explicit_child_nodes {
            config.set_explicit_child_nodes(true);
        }
        if self.merging_descendants {
            config.set_merging_semantics_of_descendants(true);
        }
        if self.block_user_actions {
            config.set_blocks_user_actions(true);
        }
    }

    fn excludes_semantics_subtree(&self) -> bool {
        self.excludes_subtree
    }
}

fn constraints() -> BoxConstraints {
    BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0))
}

fn snapshot(run: &FrameRun) -> SemanticsSnapshot {
    run.owner()
        .semantics_owner()
        .expect("semantics must remain enabled")
        .snapshot()
        .expect("rendering assembly must assign every semantics boundary a stable identity")
}

fn accessibility_id(render_id: RenderId) -> AccessibilityNodeId {
    render_id.into()
}

fn semantics_rect(x: f32, y: f32, width: f32, height: f32) -> Rect<Pixels> {
    Rect::from_origin_size(Point::new(px(x), px(y)), Size::new(px(width), px(height)))
}

fn assert_snapshot_preorder(snapshot: &SemanticsSnapshot, expected: &[AccessibilityNodeId]) {
    let (&expected_root, _) = expected
        .split_first()
        .expect("a complete semantics snapshot always has a root");
    assert_eq!(
        snapshot.root(),
        expected_root,
        "wrong snapshot root identity"
    );
    assert_eq!(
        snapshot.nodes().len(),
        expected.len(),
        "snapshot must contain exactly the expected semantics nodes",
    );
    assert_eq!(
        snapshot
            .nodes()
            .iter()
            .map(SemanticsNodeSnapshot::id)
            .collect::<Vec<_>>(),
        expected,
        "snapshot nodes must remain in deterministic render preorder",
    );
}

// The test oracle names each independently observable node field explicitly.
#[allow(clippy::too_many_arguments)]
fn assert_snapshot_node(
    snapshot: &SemanticsSnapshot,
    id: AccessibilityNodeId,
    parent: Option<AccessibilityNodeId>,
    children: &[AccessibilityNodeId],
    label: Option<&str>,
    role: SemanticsRole,
    flags: u64,
    actions: u64,
    rect: Rect<Pixels>,
) {
    let node = snapshot
        .node(id)
        .expect("every expected accessibility identity must resolve");
    assert_eq!(node.parent(), parent, "wrong parent for {id}");
    assert_eq!(node.children(), children, "wrong children for {id}");
    assert_eq!(
        node.label().map(AttributedString::as_str),
        label,
        "wrong label payload for {id}",
    );
    assert_eq!(node.role(), role, "wrong role payload for {id}");
    assert_eq!(node.flags(), flags, "wrong flag payload for {id}");
    assert_eq!(node.actions(), actions, "wrong action payload for {id}");
    assert_eq!(node.rect(), rect, "wrong source geometry for {id}");
}

// ============================================================================
// Disabled semantics: no owner is created lazily.
// ============================================================================

#[test]
fn semantics_disabled_produces_no_owner() {
    let run = RenderTester::mount(box_node(SemanticsLeaf::new(40.0).with_label("Submit")))
        .with_constraints(constraints())
        .run_to_semantics();

    assert!(
        run.semantics_owner().is_none(),
        "a PipelineOwner that never enabled semantics must never lazily \
         create a SemanticsOwner",
    );
}

// ============================================================================
// A labeled leaf becomes exactly one SemanticsNode with the right label.
// ============================================================================

#[test]
fn labeled_root_leaf_becomes_one_semantics_node() {
    let run = RenderTester::mount(box_node(
        SemanticsLeaf::new(40.0).with_label("Submit").with_button(),
    ))
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_to_semantics();

    let owner = run
        .semantics_owner()
        .expect("semantics was enabled, so run_semantics must have created an owner");

    let root_id = owner
        .root()
        .expect("the walk root always forms a SemanticsNode");
    let node = owner.get(root_id).expect("root id must resolve to a node");

    assert_eq!(node.label(), Some("Submit"));
    assert!(node.config().is_button());
    assert!(
        node.children().is_empty(),
        "a single leaf produces exactly one node with no children",
    );
    assert_eq!(owner.tree().len(), 1, "exactly one SemanticsNode total");
}

// ============================================================================
// MergeSemantics-equivalent collapses its whole subtree into one node.
// ============================================================================

#[test]
fn merge_semantics_boundary_collapses_plain_children_into_one_node() {
    let run = RenderTester::mount(
        box_node(SemanticsContainer::merge_semantics())
            .child(box_node(SemanticsLeaf::new(20.0).with_label("Alpha")))
            .child(box_node(
                SemanticsLeaf::new(20.0).with_label("Beta").with_button(),
            )),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_to_semantics();

    let owner = run.semantics_owner().expect("semantics enabled");
    let root_id = owner.root().expect("merge boundary forms the root node");
    let node = owner.get(root_id).expect("root id must resolve");

    assert_eq!(
        owner.tree().len(),
        1,
        "is_merging_semantics_of_descendants must collapse both children \
         into the boundary's single node — neither leaf gets its own \
         SemanticsNode",
    );
    assert!(node.children().is_empty());
    assert!(
        node.config().is_button(),
        "Beta's button flag absorbs up into the merged node",
    );
    let label = node.label().expect("merged label present");
    assert!(
        label.contains("Alpha") && label.contains("Beta"),
        "both descendants' labels absorb into the single merged node, got {label:?}",
    );
}

/// The boundary-vs-merge refinement, specifically: a descendant that independently
/// declares itself a semantics boundary must still collapse into the
/// `MergeSemantics`-equivalent ancestor's single node. A boundary decision
/// of the form `is_semantics_boundary() || has_been_annotated()` — or one that computes the boundary
/// decision without threading a force-merge flag down from the ancestor —
/// would let `Beta` spawn its own second `SemanticsNode` here.
#[test]
fn merge_semantics_collapses_a_nested_boundary_descendant() {
    let run = RenderTester::mount(
        box_node(SemanticsContainer::merge_semantics())
            .child(box_node(SemanticsLeaf::new(20.0).with_label("Alpha")))
            .child(box_node(
                SemanticsLeaf::new(20.0)
                    .with_label("Beta")
                    .with_button()
                    .with_boundary(),
            )),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_to_semantics();

    let owner = run.semantics_owner().expect("semantics enabled");

    assert_eq!(
        owner.tree().len(),
        1,
        "Beta's own is_semantics_boundary() must be suppressed by the \
         ancestor's is_merging_semantics_of_descendants — the whole \
         subtree still collapses into exactly one node",
    );
    let root_id = owner.root().expect("merge boundary forms the root node");
    let node = owner.get(root_id).expect("root id must resolve");
    assert!(node.children().is_empty());
    assert!(node.config().is_button());
    let label = node.label().expect("merged label present");
    assert!(label.contains("Alpha") && label.contains("Beta"));
}

// ============================================================================
// ExcludeSemantics-equivalent drops its subtree entirely from the walk.
// ============================================================================

#[test]
fn excludes_semantics_subtree_drops_descendant_content() {
    let run = RenderTester::mount(
        box_node(SemanticsContainer::exclude_semantics())
            .child(box_node(SemanticsLeaf::new(20.0).with_label("Hidden"))),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_to_semantics();

    let owner = run.semantics_owner().expect("semantics enabled");
    // The container itself sets no content and is not a boundary, but IS
    // the walk root, so it still forms the (empty) root node; its excluded
    // child never contributes.
    let root_id = owner.root().expect("root always forms a node");
    let node = owner.get(root_id).expect("root id must resolve");

    assert_eq!(
        owner.tree().len(),
        1,
        "excludes_semantics_subtree must drop the child's subtree entirely \
         — no node for the hidden leaf",
    );
    assert!(
        node.label().is_none(),
        "the excluded child's label never merges up",
    );
}

#[test]
fn root_forces_a_non_boundary_contributor_to_form_a_child_node() {
    let run = RenderTester::mount(
        box_node(SemanticsContainer::default())
            .label("root")
            .child(box_node(SemanticsLeaf::new(20.0).with_label("Child")).label("child")),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_frame();

    let root = accessibility_id(run.id("root"));
    let child = accessibility_id(run.id("child"));
    let snapshot = snapshot(&run);

    assert_snapshot_preorder(&snapshot, &[root, child]);
    assert_snapshot_node(
        &snapshot,
        root,
        None,
        &[child],
        None,
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 200.0, 200.0),
    );
    assert_snapshot_node(
        &snapshot,
        child,
        Some(root),
        &[],
        Some("Child"),
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 20.0, 20.0),
    );
}

#[test]
fn explicit_child_nodes_forms_direct_contributors() {
    let mut group_configuration = SemanticsConfiguration::new();
    group_configuration.set_label("Group");
    let run = RenderTester::mount(
        box_node(SemanticsContainer::default()).label("root").child(
            box_node(
                SemanticsContainer::default()
                    .with_configuration(group_configuration)
                    .with_boundary()
                    .with_explicit_child_nodes(true)
                    .with_side(80.0)
                    .with_child_offset(5.0, 6.0),
            )
            .label("group")
            .child(box_node(SemanticsLeaf::new(10.0).with_label("Alpha")).label("alpha"))
            .child(box_node(SemanticsLeaf::new(12.0).with_label("Beta")).label("beta")),
        ),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_frame();

    let root = accessibility_id(run.id("root"));
    let group = accessibility_id(run.id("group"));
    let alpha = accessibility_id(run.id("alpha"));
    let beta = accessibility_id(run.id("beta"));
    let snapshot = snapshot(&run);

    assert_snapshot_preorder(&snapshot, &[root, group, alpha, beta]);
    assert_snapshot_node(
        &snapshot,
        root,
        None,
        &[group],
        None,
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 200.0, 200.0),
    );
    assert_snapshot_node(
        &snapshot,
        group,
        Some(root),
        &[alpha, beta],
        Some("Group"),
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 80.0, 80.0),
    );
    assert_snapshot_node(
        &snapshot,
        alpha,
        Some(group),
        &[],
        Some("Alpha"),
        SemanticsRole::None,
        0,
        0,
        semantics_rect(5.0, 6.0, 10.0, 10.0),
    );
    assert_snapshot_node(
        &snapshot,
        beta,
        Some(group),
        &[],
        Some("Beta"),
        SemanticsRole::None,
        0,
        0,
        semantics_rect(5.0, 6.0, 12.0, 12.0),
    );
}

#[test]
fn explicit_child_nodes_passes_through_unannotated_render_objects() {
    let mut group_configuration = SemanticsConfiguration::new();
    group_configuration.set_label("Group");
    let run = RenderTester::mount(
        box_node(SemanticsContainer::default()).label("root").child(
            box_node(
                SemanticsContainer::default()
                    .with_configuration(group_configuration)
                    .with_boundary()
                    .with_explicit_child_nodes(true)
                    .with_side(80.0)
                    .with_child_offset(10.0, 20.0),
            )
            .label("group")
            .child(
                box_node(
                    SemanticsContainer::default()
                        .with_side(50.0)
                        .with_child_offset(3.0, 4.0),
                )
                .label("pass-through")
                .child(box_node(SemanticsLeaf::new(10.0).with_label("Nested")).label("nested")),
            ),
        ),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_frame();

    let root = accessibility_id(run.id("root"));
    let group = accessibility_id(run.id("group"));
    let pass_through = accessibility_id(run.id("pass-through"));
    let nested = accessibility_id(run.id("nested"));
    let snapshot = snapshot(&run);

    assert_snapshot_preorder(&snapshot, &[root, group, nested]);
    assert_snapshot_node(
        &snapshot,
        root,
        None,
        &[group],
        None,
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 200.0, 200.0),
    );
    assert_snapshot_node(
        &snapshot,
        group,
        Some(root),
        &[nested],
        Some("Group"),
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 80.0, 80.0),
    );
    assert_snapshot_node(
        &snapshot,
        nested,
        Some(group),
        &[],
        Some("Nested"),
        SemanticsRole::None,
        0,
        0,
        semantics_rect(13.0, 24.0, 10.0, 10.0),
    );
    assert!(
        snapshot.node(pass_through).is_none(),
        "an unannotated pass-through render object must not gain an accessibility identity",
    );
}

#[test]
fn inherited_explicit_child_nodes_stops_at_a_contributor() {
    let mut group_configuration = SemanticsConfiguration::new();
    group_configuration.set_label("Group");
    let mut middle_configuration = SemanticsConfiguration::new();
    middle_configuration.set_label("Middle");
    let run = RenderTester::mount(
        box_node(SemanticsContainer::default()).label("root").child(
            box_node(
                SemanticsContainer::default()
                    .with_configuration(group_configuration)
                    .with_boundary()
                    .with_explicit_child_nodes(true)
                    .with_side(80.0)
                    .with_child_offset(7.0, 8.0),
            )
            .label("group")
            .child(
                box_node(
                    SemanticsContainer::default()
                        .with_configuration(middle_configuration)
                        .with_side(40.0)
                        .with_child_offset(3.0, 4.0),
                )
                .label("middle")
                .child(box_node(SemanticsLeaf::new(10.0).with_label("Inner")).label("inner")),
            ),
        ),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_frame();

    let root = accessibility_id(run.id("root"));
    let group = accessibility_id(run.id("group"));
    let middle = accessibility_id(run.id("middle"));
    let inner = accessibility_id(run.id("inner"));
    let snapshot = snapshot(&run);

    assert_snapshot_preorder(&snapshot, &[root, group, middle]);
    assert_snapshot_node(
        &snapshot,
        root,
        None,
        &[group],
        None,
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 200.0, 200.0),
    );
    assert_snapshot_node(
        &snapshot,
        group,
        Some(root),
        &[middle],
        Some("Group"),
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 80.0, 80.0),
    );
    assert_snapshot_node(
        &snapshot,
        middle,
        Some(group),
        &[],
        Some("Middle Inner"),
        SemanticsRole::None,
        0,
        0,
        semantics_rect(7.0, 8.0, 40.0, 40.0),
    );
    assert!(
        snapshot.node(inner).is_none(),
        "a contributor must stop inherited explicit-child propagation and absorb its child",
    );
}

#[test]
fn a_non_boundary_contributor_can_merge_into_its_parent_while_forcing_its_own_children() {
    let mut parent_configuration = SemanticsConfiguration::new();
    parent_configuration.set_label("Parent");
    let mut wrapper_configuration = SemanticsConfiguration::new();
    wrapper_configuration.set_label("Wrapper");
    let run = RenderTester::mount(
        box_node(SemanticsContainer::default()).label("root").child(
            box_node(
                SemanticsContainer::default()
                    .with_configuration(parent_configuration)
                    .with_boundary()
                    .with_side(90.0)
                    .with_child_offset(5.0, 6.0),
            )
            .label("parent")
            .child(
                box_node(
                    SemanticsContainer::default()
                        .with_configuration(wrapper_configuration)
                        .with_explicit_child_nodes(true)
                        .with_side(40.0)
                        .with_child_offset(2.0, 3.0),
                )
                .label("wrapper")
                .child(box_node(SemanticsLeaf::new(10.0).with_label("Child")).label("child")),
            ),
        ),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_frame();

    let root = accessibility_id(run.id("root"));
    let parent = accessibility_id(run.id("parent"));
    let wrapper = accessibility_id(run.id("wrapper"));
    let child = accessibility_id(run.id("child"));
    let snapshot = snapshot(&run);

    assert_snapshot_preorder(&snapshot, &[root, parent, child]);
    assert_snapshot_node(
        &snapshot,
        root,
        None,
        &[parent],
        None,
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 200.0, 200.0),
    );
    assert_snapshot_node(
        &snapshot,
        parent,
        Some(root),
        &[child],
        Some("Parent Wrapper"),
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 90.0, 90.0),
    );
    assert_snapshot_node(
        &snapshot,
        child,
        Some(parent),
        &[],
        Some("Child"),
        SemanticsRole::None,
        0,
        0,
        semantics_rect(7.0, 9.0, 10.0, 10.0),
    );
    assert!(
        snapshot.node(wrapper).is_none(),
        "a compatible non-boundary contributor merges up and must not get its own identity",
    );
}

#[test]
fn incompatible_siblings_form_separate_nodes() {
    let mut parent_configuration = SemanticsConfiguration::new();
    parent_configuration.set_label("Group");
    let mut selected_alpha = SemanticsConfiguration::new();
    selected_alpha.set_label("Selected alpha");
    selected_alpha.set_selected(true);
    let mut selected_beta = SemanticsConfiguration::new();
    selected_beta.set_label("Selected beta");
    selected_beta.set_selected(true);

    let run = RenderTester::mount(
        box_node(SemanticsContainer::default()).label("root").child(
            box_node(
                SemanticsContainer::default()
                    .with_configuration(parent_configuration)
                    .with_boundary()
                    .with_side(100.0)
                    .with_child_offset(4.0, 5.0),
            )
            .label("parent")
            .child(
                box_node(SemanticsLeaf::new(10.0).with_configuration(selected_alpha))
                    .label("selected-alpha"),
            )
            .child(
                box_node(SemanticsLeaf::new(11.0).with_configuration(selected_beta))
                    .label("selected-beta"),
            )
            .child(
                box_node(
                    SemanticsLeaf::new(12.0)
                        .with_label("Tap alpha")
                        .with_tap_action(),
                )
                .label("tap-alpha"),
            )
            .child(
                box_node(
                    SemanticsLeaf::new(13.0)
                        .with_label("Tap beta")
                        .with_tap_action(),
                )
                .label("tap-beta"),
            ),
        ),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_frame();

    let root = accessibility_id(run.id("root"));
    let parent = accessibility_id(run.id("parent"));
    let selected_alpha = accessibility_id(run.id("selected-alpha"));
    let selected_beta = accessibility_id(run.id("selected-beta"));
    let tap_alpha = accessibility_id(run.id("tap-alpha"));
    let tap_beta = accessibility_id(run.id("tap-beta"));
    let snapshot = snapshot(&run);
    let selected = SemanticsFlag::IsSelected.value();
    let tap = SemanticsAction::Tap.value();

    assert_snapshot_preorder(
        &snapshot,
        &[
            root,
            parent,
            selected_alpha,
            selected_beta,
            tap_alpha,
            tap_beta,
        ],
    );
    assert_snapshot_node(
        &snapshot,
        root,
        None,
        &[parent],
        None,
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 200.0, 200.0),
    );
    assert_snapshot_node(
        &snapshot,
        parent,
        Some(root),
        &[selected_alpha, selected_beta, tap_alpha, tap_beta],
        Some("Group"),
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 100.0, 100.0),
    );
    for (id, label, flags, actions, side) in [
        (selected_alpha, "Selected alpha", selected, 0, 10.0),
        (selected_beta, "Selected beta", selected, 0, 11.0),
        (tap_alpha, "Tap alpha", 0, tap, 12.0),
        (tap_beta, "Tap beta", 0, tap, 13.0),
    ] {
        assert_snapshot_node(
            &snapshot,
            id,
            Some(parent),
            &[],
            Some(label),
            SemanticsRole::None,
            flags,
            actions,
            semantics_rect(4.0, 5.0, side, side),
        );
    }
}

#[test]
fn parent_child_role_conflict_forms_a_child_node() {
    let mut parent_configuration = SemanticsConfiguration::new();
    parent_configuration.set_label("Parent");
    parent_configuration.set_role(SemanticsRole::Dialog);
    let run = RenderTester::mount(
        box_node(SemanticsContainer::default()).label("root").child(
            box_node(
                SemanticsContainer::default()
                    .with_configuration(parent_configuration)
                    .with_boundary()
                    .with_side(80.0)
                    .with_child_offset(9.0, 10.0),
            )
            .label("parent")
            .child(
                box_node(
                    SemanticsLeaf::new(14.0)
                        .with_label("Child")
                        .with_role(SemanticsRole::ListItem),
                )
                .label("child"),
            ),
        ),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_frame();

    let root = accessibility_id(run.id("root"));
    let parent = accessibility_id(run.id("parent"));
    let child = accessibility_id(run.id("child"));
    let snapshot = snapshot(&run);

    assert_snapshot_preorder(&snapshot, &[root, parent, child]);
    assert_snapshot_node(
        &snapshot,
        root,
        None,
        &[parent],
        None,
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 200.0, 200.0),
    );
    assert_snapshot_node(
        &snapshot,
        parent,
        Some(root),
        &[child],
        Some("Parent"),
        SemanticsRole::Dialog,
        0,
        0,
        semantics_rect(0.0, 0.0, 80.0, 80.0),
    );
    assert_snapshot_node(
        &snapshot,
        child,
        Some(parent),
        &[],
        Some("Child"),
        SemanticsRole::ListItem,
        0,
        0,
        semantics_rect(9.0, 10.0, 14.0, 14.0),
    );
}

#[test]
fn compatible_siblings_still_merge_in_preorder() {
    let mut parent_configuration = SemanticsConfiguration::new();
    parent_configuration.set_label("Parent");
    let run = RenderTester::mount(
        box_node(SemanticsContainer::default()).label("root").child(
            box_node(
                SemanticsContainer::default()
                    .with_configuration(parent_configuration)
                    .with_boundary()
                    .with_side(80.0)
                    .with_child_offset(6.0, 7.0),
            )
            .label("parent")
            .child(box_node(SemanticsLeaf::new(10.0).with_label("Alpha")).label("alpha"))
            .child(box_node(SemanticsLeaf::new(12.0).with_label("Beta")).label("beta")),
        ),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_frame();

    let root = accessibility_id(run.id("root"));
    let parent = accessibility_id(run.id("parent"));
    let alpha = accessibility_id(run.id("alpha"));
    let beta = accessibility_id(run.id("beta"));
    let snapshot = snapshot(&run);

    assert_snapshot_preorder(&snapshot, &[root, parent]);
    assert_snapshot_node(
        &snapshot,
        root,
        None,
        &[parent],
        None,
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 200.0, 200.0),
    );
    assert_snapshot_node(
        &snapshot,
        parent,
        Some(root),
        &[],
        Some("Parent Alpha Beta"),
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 80.0, 80.0),
    );
    assert!(snapshot.node(alpha).is_none());
    assert!(snapshot.node(beta).is_none());
}

#[test]
fn merge_descendants_overrides_explicit_and_conflict_splitting() {
    let mut merge_configuration = SemanticsConfiguration::new();
    merge_configuration.set_label("Merge");
    let mut selected_alpha = SemanticsConfiguration::new();
    selected_alpha.set_label("Alpha");
    selected_alpha.set_selected(true);
    let mut selected_beta = SemanticsConfiguration::new();
    selected_beta.set_label("Beta");
    selected_beta.set_selected(true);

    let run = RenderTester::mount(
        box_node(SemanticsContainer::default()).label("root").child(
            box_node(
                SemanticsContainer::merge_semantics()
                    .with_configuration(merge_configuration)
                    .with_explicit_child_nodes(true)
                    .with_side(30.0)
                    .with_child_offset(50.0, 60.0),
            )
            .label("merge")
            .child(
                box_node(SemanticsLeaf::new(10.0).with_configuration(selected_alpha))
                    .label("alpha"),
            )
            .child(
                box_node(
                    SemanticsLeaf::new(12.0)
                        .with_configuration(selected_beta)
                        .with_boundary(),
                )
                .label("beta"),
            ),
        ),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_frame();

    let root = accessibility_id(run.id("root"));
    let merge = accessibility_id(run.id("merge"));
    let alpha = accessibility_id(run.id("alpha"));
    let beta = accessibility_id(run.id("beta"));
    let snapshot = snapshot(&run);

    assert_snapshot_preorder(&snapshot, &[root, merge]);
    assert_snapshot_node(
        &snapshot,
        root,
        None,
        &[merge],
        None,
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 200.0, 200.0),
    );
    assert_snapshot_node(
        &snapshot,
        merge,
        Some(root),
        &[],
        Some("Merge Alpha Beta"),
        SemanticsRole::None,
        SemanticsFlag::IsSelected.value(),
        0,
        semantics_rect(0.0, 0.0, 30.0, 30.0),
    );
    assert!(snapshot.node(alpha).is_none());
    assert!(snapshot.node(beta).is_none());
}

#[test]
fn exclude_semantics_drops_descendants_but_keeps_own_annotation() {
    let mut exclude_configuration = SemanticsConfiguration::new();
    exclude_configuration.set_label("Visible");
    let run = RenderTester::mount(
        box_node(SemanticsContainer::default()).label("root").child(
            box_node(
                SemanticsContainer::exclude_semantics()
                    .with_configuration(exclude_configuration)
                    .with_boundary()
                    .with_side(40.0)
                    .with_child_offset(10.0, 20.0),
            )
            .label("exclude")
            .child(box_node(SemanticsLeaf::new(10.0).with_label("Hidden")).label("hidden")),
        ),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_frame();

    let root = accessibility_id(run.id("root"));
    let exclude = accessibility_id(run.id("exclude"));
    let hidden = accessibility_id(run.id("hidden"));
    let snapshot = snapshot(&run);

    assert_snapshot_preorder(&snapshot, &[root, exclude]);
    assert_snapshot_node(
        &snapshot,
        root,
        None,
        &[exclude],
        None,
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 200.0, 200.0),
    );
    assert_snapshot_node(
        &snapshot,
        exclude,
        Some(root),
        &[],
        Some("Visible"),
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 40.0, 40.0),
    );
    assert!(
        snapshot.node(hidden).is_none(),
        "an excluded descendant must have neither payload nor external identity",
    );
}

#[test]
fn merged_descendants_do_not_expand_the_source_node_rect() {
    let mut parent_configuration = SemanticsConfiguration::new();
    parent_configuration.set_label("Parent");
    let run = RenderTester::mount(
        box_node(SemanticsContainer::default().with_child_offset(3.0, 4.0))
            .label("root")
            .child(
                box_node(
                    SemanticsContainer::default()
                        .with_configuration(parent_configuration)
                        .with_boundary()
                        .with_side(20.0)
                        .with_child_offset(80.0, 90.0),
                )
                .label("parent")
                .child(box_node(SemanticsLeaf::new(10.0).with_label("Child")).label("child")),
            ),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_frame();

    let root = accessibility_id(run.id("root"));
    let parent = accessibility_id(run.id("parent"));
    let child = accessibility_id(run.id("child"));
    let snapshot = snapshot(&run);

    assert_snapshot_preorder(&snapshot, &[root, parent]);
    assert_snapshot_node(
        &snapshot,
        root,
        None,
        &[parent],
        None,
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 200.0, 200.0),
    );
    assert_snapshot_node(
        &snapshot,
        parent,
        Some(root),
        &[],
        Some("Parent Child"),
        SemanticsRole::None,
        0,
        0,
        semantics_rect(3.0, 4.0, 20.0, 20.0),
    );
    assert!(
        snapshot.node(child).is_none(),
        "a compatible merged contribution must not gain an accessibility identity",
    );
}

#[test]
fn toggling_explicit_child_nodes_reuses_the_contributors_accessibility_id() {
    let mut group_configuration = SemanticsConfiguration::new();
    group_configuration.set_label("Group");
    let mut run = RenderTester::mount(
        box_node(SemanticsContainer::default()).label("root").child(
            box_node(
                SemanticsContainer::default()
                    .with_configuration(group_configuration)
                    .with_boundary()
                    .with_side(60.0)
                    .with_child_offset(8.0, 9.0),
            )
            .label("group")
            .child(box_node(SemanticsLeaf::new(11.0).with_label("Child")).label("child")),
        ),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_frame();

    let root_render_id = run.id("root");
    let group_render_id = run.id("group");
    let child_render_id = run.id("child");
    let root = accessibility_id(root_render_id);
    let group = accessibility_id(group_render_id);
    let child = accessibility_id(child_render_id);

    let merged = snapshot(&run);
    assert_snapshot_preorder(&merged, &[root, group]);
    assert_snapshot_node(
        &merged,
        root,
        None,
        &[group],
        None,
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 200.0, 200.0),
    );
    assert_snapshot_node(
        &merged,
        group,
        Some(root),
        &[],
        Some("Group Child"),
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 60.0, 60.0),
    );
    assert!(merged.node(child).is_none());

    run.update::<SemanticsContainer>(group_render_id, |container| {
        container.explicit_child_nodes = true;
    });
    run.owner_mut()
        .add_node_needing_semantics(group_render_id, 1);
    run.pump();

    let explicit = snapshot(&run);
    assert_snapshot_preorder(&explicit, &[root, group, child]);
    assert_snapshot_node(
        &explicit,
        root,
        None,
        &[group],
        None,
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 200.0, 200.0),
    );
    assert_snapshot_node(
        &explicit,
        group,
        Some(root),
        &[child],
        Some("Group"),
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 60.0, 60.0),
    );
    assert_snapshot_node(
        &explicit,
        child,
        Some(group),
        &[],
        Some("Child"),
        SemanticsRole::None,
        0,
        0,
        semantics_rect(8.0, 9.0, 11.0, 11.0),
    );

    run.update::<SemanticsContainer>(group_render_id, |container| {
        container.explicit_child_nodes = false;
    });
    run.owner_mut()
        .add_node_needing_semantics(group_render_id, 1);
    run.pump();

    let merged_again = snapshot(&run);
    assert_snapshot_preorder(&merged_again, &[root, group]);
    assert_snapshot_node(
        &merged_again,
        root,
        None,
        &[group],
        None,
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 200.0, 200.0),
    );
    assert_snapshot_node(
        &merged_again,
        group,
        Some(root),
        &[],
        Some("Group Child"),
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 60.0, 60.0),
    );
    assert!(merged_again.node(child).is_none());

    run.update::<SemanticsContainer>(group_render_id, |container| {
        container.explicit_child_nodes = true;
    });
    run.owner_mut()
        .add_node_needing_semantics(group_render_id, 1);
    run.pump();

    let explicit_again = snapshot(&run);
    assert_snapshot_preorder(&explicit_again, &[root, group, child]);
    assert_snapshot_node(
        &explicit_again,
        root,
        None,
        &[group],
        None,
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 200.0, 200.0),
    );
    assert_snapshot_node(
        &explicit_again,
        group,
        Some(root),
        &[child],
        Some("Group"),
        SemanticsRole::None,
        0,
        0,
        semantics_rect(0.0, 0.0, 60.0, 60.0),
    );
    assert_snapshot_node(
        &explicit_again,
        child,
        Some(group),
        &[],
        Some("Child"),
        SemanticsRole::None,
        0,
        0,
        semantics_rect(8.0, 9.0, 11.0, 11.0),
    );
}

#[test]
fn full_snapshot_uses_render_identity_and_keeps_it_across_configuration_updates() {
    let mut run = RenderTester::mount(
        box_node(SemanticsContainer::default()).label("root").child(
            box_node(
                SemanticsLeaf::new(20.0)
                    .with_label("Before")
                    .with_button()
                    .with_tap_action()
                    .with_role(SemanticsRole::ListItem)
                    .with_boundary(),
            )
            .label("child"),
        ),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_frame();

    let root_render_id = run.id("root");
    let child_render_id = run.id("child");
    let root_accessibility_id = accessibility_id(root_render_id);
    let child_accessibility_id = accessibility_id(child_render_id);

    let initial = snapshot(&run);
    assert_eq!(initial.root(), root_accessibility_id);
    assert_eq!(
        initial
            .nodes()
            .iter()
            .map(SemanticsNodeSnapshot::id)
            .collect::<Vec<_>>(),
        vec![root_accessibility_id, child_accessibility_id],
        "full snapshots must be deterministic preorder",
    );
    let initial_child = initial
        .node(child_accessibility_id)
        .expect("child identity must resolve");
    assert_eq!(initial_child.parent(), Some(root_accessibility_id));
    assert_eq!(
        initial_child.label().map(AttributedString::as_str),
        Some("Before"),
    );
    assert_eq!(initial_child.role(), SemanticsRole::ListItem);
    assert_ne!(initial_child.flags() & SemanticsFlag::IsButton.value(), 0,);
    assert_ne!(initial_child.actions() & SemanticsAction::Tap.value(), 0,);
    assert_eq!(initial_child.rect().width(), px(20.0));
    assert_eq!(initial_child.rect().height(), px(20.0));

    run.update::<SemanticsLeaf>(child_render_id, |leaf| leaf.label = Some("After"));
    run.owner_mut()
        .add_node_needing_semantics(child_render_id, 1);
    run.pump();

    let updated = snapshot(&run);
    assert_eq!(updated.root(), root_accessibility_id);
    assert_eq!(
        updated
            .node(child_accessibility_id)
            .and_then(|node| node.label())
            .map(AttributedString::as_str),
        Some("After"),
        "changing semantics configuration must update content without changing identity",
    );
}

#[test]
fn sibling_insert_and_reorder_preserve_existing_accessibility_ids() {
    let mut run = RenderTester::mount(
        box_node(SemanticsContainer::default())
            .label("root")
            .child(
                box_node(SemanticsLeaf::new(20.0).with_label("Alpha").with_boundary())
                    .label("alpha"),
            )
            .child(
                box_node(SemanticsLeaf::new(20.0).with_label("Beta").with_boundary()).label("beta"),
            ),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_frame();

    let root = run.id("root");
    let alpha = run.id("alpha");
    let beta = run.id("beta");
    let root_accessibility_id = accessibility_id(root);
    let alpha_accessibility_id = accessibility_id(alpha);
    let beta_accessibility_id = accessibility_id(beta);

    let gamma = run
        .owner_mut()
        .insert_child_render_object(
            root,
            Box::new(SemanticsLeaf::new(20.0).with_label("Gamma").with_boundary()),
        )
        .expect("root must accept a third child");
    run.owner_mut().add_node_needing_semantics(root, 0);
    run.pump();

    let inserted = snapshot(&run);
    assert!(inserted.node(alpha_accessibility_id).is_some());
    assert!(inserted.node(beta_accessibility_id).is_some());

    {
        let tree = run.owner_mut().render_tree_mut();
        tree.drop_child(root, alpha);
        tree.adopt_child(root, alpha);
    }
    run.owner_mut().mark_needs_layout(root);
    run.owner_mut().add_node_needing_semantics(root, 0);
    run.pump();

    let reordered = snapshot(&run);
    let root_node = reordered
        .node(root_accessibility_id)
        .expect("root identity must still resolve");
    assert_eq!(
        root_node.children(),
        &[
            beta_accessibility_id,
            accessibility_id(gamma),
            alpha_accessibility_id,
        ],
        "snapshot child order must follow current render preorder without reminting IDs",
    );
    assert_eq!(
        reordered
            .nodes()
            .iter()
            .map(SemanticsNodeSnapshot::id)
            .collect::<Vec<_>>(),
        vec![
            root_accessibility_id,
            beta_accessibility_id,
            accessibility_id(gamma),
            alpha_accessibility_id,
        ],
    );
}

#[test]
fn removal_updates_parent_children_and_recycled_render_slot_gets_a_new_id() {
    let mut run = RenderTester::mount(
        box_node(SemanticsContainer::default())
            .label("root")
            .child(
                box_node(SemanticsContainer {
                    boundary: true,
                    ..Default::default()
                })
                .label("removed-branch")
                .child(
                    box_node(
                        SemanticsLeaf::new(10.0)
                            .with_label("Removed leaf")
                            .with_boundary(),
                    )
                    .label("removed-leaf"),
                ),
            )
            .child(
                box_node(SemanticsLeaf::new(20.0).with_label("Kept").with_boundary()).label("kept"),
            ),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_frame();

    let root = run.id("root");
    let removed_branch = run.id("removed-branch");
    let removed_leaf = run.id("removed-leaf");
    let kept = run.id("kept");
    let removed_branch_accessibility_id = accessibility_id(removed_branch);
    let removed_leaf_accessibility_id = accessibility_id(removed_leaf);

    assert_eq!(run.owner_mut().remove_render_object(removed_branch), 2);
    run.owner_mut().mark_needs_layout(root);
    run.owner_mut().add_node_needing_semantics(root, 0);
    run.pump();

    let removed = snapshot(&run);
    assert!(removed.node(removed_branch_accessibility_id).is_none());
    assert!(removed.node(removed_leaf_accessibility_id).is_none());
    assert_eq!(
        removed
            .node(accessibility_id(root))
            .expect("root must remain")
            .children(),
        &[accessibility_id(kept)],
    );

    let replacement = run
        .owner_mut()
        .insert_child_render_object(
            root,
            Box::new(
                SemanticsLeaf::new(20.0)
                    .with_label("Replacement")
                    .with_boundary(),
            ),
        )
        .expect("root must accept a replacement child");
    assert_eq!(
        replacement.index(),
        removed_branch.index(),
        "the test must exercise actual slab-slot reuse",
    );
    assert_ne!(replacement.generation(), removed_branch.generation());
    run.owner_mut().add_node_needing_semantics(root, 0);
    run.pump();

    let reused = snapshot(&run);
    let replacement_accessibility_id = accessibility_id(replacement);
    assert_ne!(
        replacement_accessibility_id, removed_branch_accessibility_id,
        "generation must participate in OS-facing identity",
    );
    assert!(reused.node(removed_branch_accessibility_id).is_none());
    assert!(reused.node(replacement_accessibility_id).is_some());
}

#[test]
fn merged_content_and_actions_use_the_enclosing_boundary_render_id() {
    let run = RenderTester::mount(
        box_node(SemanticsContainer::merge_semantics())
            .label("boundary")
            .child(
                box_node(
                    SemanticsLeaf::new(20.0)
                        .with_label("Activate")
                        .with_tap_action(),
                )
                .label("merged-child"),
            ),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_frame();

    let boundary_id = accessibility_id(run.id("boundary"));
    let merged_child_id = accessibility_id(run.id("merged-child"));
    let snapshot = snapshot(&run);

    assert_eq!(snapshot.root(), boundary_id);
    assert_eq!(snapshot.nodes().len(), 1);
    assert!(snapshot.node(merged_child_id).is_none());
    let boundary = snapshot.node(boundary_id).expect("boundary must resolve");
    assert_eq!(
        boundary.label().map(AttributedString::as_str),
        Some("Activate"),
    );
    assert_ne!(boundary.actions() & SemanticsAction::Tap.value(), 0);
}

#[test]
fn nested_real_boundary_keeps_its_own_render_identity() {
    let run = RenderTester::mount(
        box_node(SemanticsContainer::default()).label("root").child(
            box_node(
                SemanticsLeaf::new(20.0)
                    .with_label("Nested")
                    .with_boundary(),
            )
            .label("nested"),
        ),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_frame();

    let root_id = accessibility_id(run.id("root"));
    let nested_id = accessibility_id(run.id("nested"));
    let snapshot = snapshot(&run);

    assert_eq!(snapshot.root(), root_id);
    assert_eq!(
        snapshot
            .node(root_id)
            .expect("root must resolve")
            .children(),
        &[nested_id],
    );
    assert_eq!(
        snapshot
            .node(nested_id)
            .and_then(|node| node.label())
            .map(AttributedString::as_str),
        Some("Nested"),
    );
}

#[test]
fn blocked_boundary_snapshot_exposes_only_accessibility_focus_actions() {
    let run = RenderTester::mount(
        box_node(
            SemanticsLeaf::new(20.0)
                .with_label("Blocked")
                .with_boundary()
                .with_tap_action()
                .with_accessibility_focus_action()
                .with_blocked_user_actions(),
        )
        .label("blocked"),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_frame();

    let blocked_id = accessibility_id(run.id("blocked"));
    assert_eq!(
        snapshot(&run)
            .node(blocked_id)
            .expect("blocked boundary must resolve")
            .actions(),
        SemanticsAction::DidGainAccessibilityFocus.value(),
        "pointer actions must be removed while accessibility focus remains available",
    );
}

#[test]
fn role_only_descendant_contributes_to_the_assembled_boundary() {
    let run = RenderTester::mount(
        box_node(SemanticsContainer::default()).label("root").child(
            box_node(SemanticsContainer::default().with_boundary())
                .label("boundary")
                .child(box_node(
                    SemanticsLeaf::new(20.0).with_role(SemanticsRole::ListItem),
                )),
        ),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_frame();

    let snapshot = snapshot(&run);
    let root = accessibility_id(run.id("root"));
    let boundary = accessibility_id(run.id("boundary"));
    assert_snapshot_preorder(&snapshot, &[root, boundary]);
    assert_eq!(
        snapshot
            .node(boundary)
            .expect("nested boundary must resolve")
            .role(),
        SemanticsRole::ListItem,
        "a role is semantic annotation even without label, flags, or actions",
    );
}

#[test]
fn assembled_boundary_adopts_every_modeled_first_wins_field_from_a_descendant() {
    let mut child_configuration = SemanticsConfiguration::new();
    child_configuration.set_label("Payload contributor");
    child_configuration.set_hint_overrides(
        SemanticsHintOverrides::new()
            .with_tap_hint("Activate")
            .with_long_press_hint("Open menu"),
    );
    child_configuration.set_scroll_position(1.0);
    child_configuration.set_scroll_extent_max(2.0);
    child_configuration.set_scroll_extent_min(-3.0);
    child_configuration.set_scroll_index(4);
    child_configuration.set_scroll_child_count(5);
    child_configuration.set_index_in_parent(6);
    child_configuration.set_platform_view_id(7);
    child_configuration.set_max_value_length(8);
    child_configuration.set_current_value_length(9);

    let run = RenderTester::mount(
        box_node(SemanticsContainer::default()).label("root").child(
            box_node(SemanticsContainer::default().with_boundary())
                .label("boundary")
                .child(box_node(
                    SemanticsLeaf::new(20.0).with_configuration(child_configuration),
                )),
        ),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_frame();

    let snapshot = snapshot(&run);
    let boundary = snapshot
        .node(accessibility_id(run.id("boundary")))
        .expect("nested boundary must resolve");
    let overrides = boundary
        .hint_overrides()
        .expect("descendant hint overrides must be absorbed");
    assert_eq!(overrides.on_tap_hint.as_deref(), Some("Activate"));
    assert_eq!(overrides.on_long_press_hint.as_deref(), Some("Open menu"));
    assert_eq!(boundary.scroll_position(), Some(1.0));
    assert_eq!(boundary.scroll_extent_max(), Some(2.0));
    assert_eq!(boundary.scroll_extent_min(), Some(-3.0));
    assert_eq!(boundary.scroll_index(), Some(4));
    assert_eq!(boundary.scroll_child_count(), Some(5));
    assert_eq!(boundary.index_in_parent(), Some(6));
    assert_eq!(boundary.platform_view_id(), Some(7));
    assert_eq!(boundary.max_value_length(), Some(8));
    assert_eq!(boundary.current_value_length(), Some(9));
}

#[test]
fn blocked_ancestor_masks_actions_on_a_nested_boundary() {
    let run = RenderTester::mount(
        box_node(SemanticsContainer::default().with_blocked_user_actions())
            .label("root")
            .child(
                box_node(
                    SemanticsLeaf::new(20.0)
                        .with_boundary()
                        .with_tap_action()
                        .with_accessibility_focus_action(),
                )
                .label("nested"),
            ),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_frame();

    assert_eq!(
        snapshot(&run)
            .node(accessibility_id(run.id("nested")))
            .expect("nested boundary must resolve")
            .actions(),
        SemanticsAction::DidGainAccessibilityFocus.value(),
        "blocking policy must propagate across explicit semantics boundaries",
    );
}

#[test]
fn blocked_child_subtree_does_not_mask_unblocked_parent_actions() {
    let mut root_configuration = SemanticsConfiguration::new();
    root_configuration.add_action(SemanticsAction::Cut, Arc::new(|_, _| {}));

    let run = RenderTester::mount(
        box_node(SemanticsContainer::default().with_configuration(root_configuration))
            .label("root")
            .child(
                box_node(SemanticsContainer::default().with_blocked_user_actions()).child(
                    box_node(
                        SemanticsLeaf::new(20.0)
                            .with_boundary()
                            .with_tap_action()
                            .with_accessibility_focus_action(),
                    )
                    .label("blocked-descendant"),
                ),
            ),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_frame();

    let snapshot = snapshot(&run);
    assert_eq!(
        snapshot
            .node(accessibility_id(run.id("root")))
            .expect("root boundary must resolve")
            .actions(),
        SemanticsAction::Cut.value(),
        "a descendant block must not flow upward into parent-owned actions",
    );
    assert_eq!(
        snapshot
            .node(accessibility_id(run.id("blocked-descendant")))
            .expect("blocked descendant boundary must resolve")
            .actions(),
        SemanticsAction::DidGainAccessibilityFocus.value(),
        "a non-contributing blocker must still propagate policy down its subtree",
    );
}
