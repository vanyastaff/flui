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
use flui_foundation::geometry::{Offset, Point, Rect, Size};
use flui_foundation::{Leaf, Variable};
use flui_rendering::{
    constraints::BoxConstraints,
    context::BoxLayoutContext,
    parent_data::BoxParentData,
    semantics::{
        AccessibilityNodeId, AttributedString, SemanticsAction, SemanticsConfiguration,
        SemanticsFlag, SemanticsNodeSnapshot, SemanticsRole, SemanticsSnapshot,
    },
    testing::{FrameRun, Probe, RenderTester, box_node},
    traits::RenderBox,
};

/// A fixed-size leaf that reports a configurable `SemanticsConfiguration`.
///
/// Stands in for a `Semantics`/button-like leaf widget — the real
/// `RenderSemanticsAnnotations` is not built yet.
#[derive(Debug, Default)]
struct SemanticsLeaf {
    side: f64,
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
    fn new(side: f64) -> Self {
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
}

impl flui_foundation::Diagnosticable for SemanticsLeaf {}

impl RenderBox for SemanticsLeaf {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        ctx.constraints().constrain(Size::new(self.side, self.side))
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
    side: Option<f64>,
    child_offset: Option<Offset>,
    semantics_clip: Option<Rect<f64>>,
    paint_clip: Option<Rect<f64>>,
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

    fn with_side(mut self, side: f64) -> Self {
        self.side = Some(side);
        self
    }

    fn with_child_offset(mut self, dx: f64, dy: f64) -> Self {
        self.child_offset = Some(Offset::new(dx, dy));
        self
    }

    /// Reported from `describe_semantics_clip`, in this node's coordinates.
    fn with_semantics_clip(mut self, top: f64, bottom: f64) -> Self {
        self.semantics_clip = Some(Rect::from_ltrb(0.0, top, 200.0, bottom));
        self
    }

    /// Reported from `describe_approximate_paint_clip`.
    fn with_paint_clip(mut self, top: f64, bottom: f64) -> Self {
        self.paint_clip = Some(Rect::from_ltrb(0.0, top, 200.0, bottom));
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
            |side| constraints.constrain(Size::new(side, side)),
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

    fn describe_semantics_clip(&self, _child_slot: usize, _size: Size) -> Option<Rect<f64>> {
        self.semantics_clip
    }

    fn describe_approximate_paint_clip(
        &self,
        _child_slot: usize,
        _size: Size,
    ) -> Option<Rect<f64>> {
        self.paint_clip
    }
}

fn constraints() -> BoxConstraints {
    BoxConstraints::new(0.0, 200.0, 0.0, 200.0)
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

fn semantics_rect(x: f64, y: f64, width: f64, height: f64) -> Rect<f64> {
    Rect::from_origin_size(Point::new(x, y), Size::new(width, height))
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
#[expect(clippy::too_many_arguments)]
fn assert_snapshot_node(
    snapshot: &SemanticsSnapshot,
    id: AccessibilityNodeId,
    parent: Option<AccessibilityNodeId>,
    children: &[AccessibilityNodeId],
    label: Option<&str>,
    role: SemanticsRole,
    flags: u64,
    actions: u64,
    rect: Rect<f64>,
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

// ============================================================================
// A labeled leaf becomes exactly one SemanticsNode with the right label.
// ============================================================================

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
    run.owner_mut().mark_needs_semantics(group_render_id);
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
    run.owner_mut().mark_needs_semantics(group_render_id);
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
    run.owner_mut().mark_needs_semantics(group_render_id);
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
    run.owner_mut().mark_needs_semantics(root);
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
    run.owner_mut().mark_needs_semantics(root);
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

// ============================================================================
// Ancestor clips: what a screen reader is told about content that has been
// scrolled or clipped out of sight.
// ============================================================================

/// A child with nothing left after its ancestor's semantics clip contributes
/// no node at all — it is not on screen and not reachable, so announcing it
/// would point a screen reader at empty space.
#[test]
fn a_semantics_clip_drops_a_child_that_falls_entirely_outside_it() {
    let run = RenderTester::mount(
        box_node(
            SemanticsContainer::default()
                .with_side(200.0)
                .with_semantics_clip(0.0, 100.0)
                .with_child_offset(0.0, 150.0),
        )
        .child(box_node(
            SemanticsLeaf::new(20.0).with_label("Out of reach"),
        )),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_to_semantics();

    let owner = run.semantics_owner().expect("semantics enabled");
    assert_eq!(
        owner.tree().len(),
        1,
        "only the walk root survives; the clipped-away child contributes nothing",
    );
}

/// Outside the paint clip but inside the semantics clip is the off-screen but
/// reachable case: the node stays, so a "scroll to" action has a target, and
/// carries the hidden flag so it is not announced as if it were visible.
#[test]
fn a_paint_clip_keeps_the_child_and_flags_it_hidden() {
    let run = RenderTester::mount(
        box_node(
            SemanticsContainer::default()
                .with_side(200.0)
                .with_semantics_clip(0.0, 200.0)
                .with_paint_clip(0.0, 50.0)
                .with_child_offset(0.0, 100.0),
        )
        .child(box_node(
            SemanticsLeaf::new(40.0).with_label("Below the fold"),
        )),
    )
    .with_constraints(constraints())
    .with_semantics_enabled()
    .run_to_semantics();

    let owner = run.semantics_owner().expect("semantics enabled");
    let root_id = owner.root().expect("root forms a node");
    let child_id = *owner
        .get(root_id)
        .expect("root resolves")
        .children()
        .first()
        .expect("an unpainted but reachable child stays in the tree");
    let child = owner.get(child_id).expect("child resolves");

    assert!(
        child.config().is_hidden(),
        "a child the paint clip excludes must be announced as hidden",
    );
    assert_eq!(
        (child.rect().min.y, child.rect().max.y),
        (100.0, 140.0),
        "its rect is the semantics one, not the empty paint intersection — a \
         scroll-to action needs somewhere to aim",
    );
}
