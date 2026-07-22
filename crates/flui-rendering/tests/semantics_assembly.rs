//! ADR-0014 milestone harness — the semantics assembly walk.
//!
//! Exercises `PipelineOwner<Semantics>::run_semantics` through the real
//! pipeline (`RenderTester::run_to_semantics`) against small test-only
//! render objects that override `describe_semantics_configuration` /
//! `excludes_semantics_subtree` directly. The three real consumers
//! (`RenderSemanticsAnnotations` / `RenderMergeSemantics` /
//! `RenderExcludeSemantics`) are a separate ADR-0014 follow-up
//! task, and do not exist yet.
//!
//! This is the DoD harness proof required by AGENTS.md's
//! Definition of Done: a test that fails without the assembly walk body
//! and passes with it. Before the walk landed, `run_semantics` was a
//! `tracing::warn!` no-op stub that never created a `SemanticsNode` tree —
//! every assertion below that reads `run.semantics_owner()` would have
//! panicked on `None`/an empty tree. The `merge_semantics_collapses_a_nested_boundary_descendant`
//! test specifically targets the boundary-vs-merge refinement (a naive
//! `is_semantics_boundary() || has_content()` predicate, or a boundary
//! decision that ignores `is_merging_semantics_of_descendants`, would
//! leave the nested boundary child as its own node).

use flui_rendering::{
    constraints::BoxConstraints,
    context::BoxLayoutContext,
    parent_data::BoxParentData,
    semantics::SemanticsConfiguration,
    testing::{RenderTester, box_node},
    traits::RenderBox,
};
use flui_tree::{Leaf, Variable};
use flui_types::{Size, geometry::px};

/// A fixed-size leaf that reports a configurable `SemanticsConfiguration`.
///
/// Stands in for a `Semantics`/button-like leaf widget — the real
/// `RenderSemanticsAnnotations` is not built yet.
#[derive(Debug, Default)]
struct SemanticsLeaf {
    side: f32,
    label: Option<&'static str>,
    button: bool,
    boundary: bool,
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
        if self.boundary {
            config.set_semantics_boundary(true);
        }
        if let Some(label) = self.label {
            config.set_label(label);
        }
        if self.button {
            config.set_button(true);
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
    boundary: bool,
    merging_descendants: bool,
    excludes_subtree: bool,
}

impl SemanticsContainer {
    /// `RenderMergeSemantics` parity: `isSemanticBoundary = true,
    /// isMergingSemanticsOfDescendants = true` (ADR-0014 context, citing
    /// `proxy_box.dart:4379-4390`).
    fn merge_semantics() -> Self {
        Self {
            boundary: true,
            merging_descendants: true,
            excludes_subtree: false,
        }
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
        }
        constraints.biggest()
    }

    fn describe_semantics_configuration(&self, config: &mut SemanticsConfiguration) {
        if self.boundary {
            config.set_semantics_boundary(true);
        }
        if self.merging_descendants {
            config.set_merging_semantics_of_descendants(true);
        }
    }

    fn excludes_semantics_subtree(&self) -> bool {
        self.excludes_subtree
    }
}

fn constraints() -> BoxConstraints {
    BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0))
}

// ============================================================================
// Disabled semantics: no owner at all (ADR-0014 lazy-creation contract).
// ============================================================================

#[test]
fn semantics_disabled_produces_no_owner() {
    let run = RenderTester::mount(box_node(SemanticsLeaf::new(40.0).with_label("Submit")))
        .with_constraints(constraints())
        .run_to_semantics();

    assert!(
        run.semantics_owner().is_none(),
        "a PipelineOwner that never enabled semantics must never lazily \
         create a SemanticsOwner (ADR-0014)",
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
        .expect("the walk root always forms a SemanticsNode (ADR-0014 root special case)");
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
/// of the form `is_semantics_boundary() || has_content()` (the ADR's
/// flagged conflated predicate) — or one that computes the boundary
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
