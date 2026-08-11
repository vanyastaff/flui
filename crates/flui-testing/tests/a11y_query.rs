//! End-to-end: a mounted render tree becomes a queryable accessibility tree.
//!
//! The module tests in `src/a11y.rs` build a `TreeUpdate` by hand, so they pin
//! the query logic and say nothing about whether the harness is wired to the
//! semantics phase at all. These tests start where a user does — a render tree
//! and a pumped frame — and are the ones that fail if `enable_semantics` stops
//! reaching the pipeline owner, if the semantics phase stops assembling, or if
//! the translation stops being invoked.

use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_rendering::prelude::*;
use flui_rendering::protocol::BoxProtocol;
use flui_semantics::{SemanticsConfiguration, SemanticsRole};
use flui_testing::HeadlessBinding;
use flui_testing::a11y::Role;
use flui_types::{Size, geometry::px};
use flui_view::{BuildOwner, tree::ElementTree};

/// A leaf carrying whatever semantics the test wants to see come out the other
/// end. Layout is fixed and irrelevant; the point is the config.
#[derive(Debug, Default)]
struct SemanticLeaf {
    label: String,
    button: bool,
    focused: bool,
    role: SemanticsRole,
    /// `None` leaves the node with no enabled-state concept at all, which is
    /// the case that distinguishes "not disabled" from "has no such state".
    enabled: Option<bool>,
}

impl flui_foundation::Diagnosticable for SemanticLeaf {}

impl RenderBox for SemanticLeaf {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        Size::new(px(10.0), px(10.0))
    }

    fn paint(&self, _ctx: &mut PaintCx<'_, Leaf>) {}

    fn describe_semantics_configuration(&self, config: &mut SemanticsConfiguration) {
        config.set_label(self.label.clone());
        config.set_button(self.button);
        config.set_focused(self.focused);
        config.set_role(self.role);
        config.set_enabled(self.enabled);
    }
}

fn binding_with(leaf: SemanticLeaf) -> HeadlessBinding {
    let mut owner = PipelineOwner::new();
    let root = owner.insert::<BoxProtocol>(Box::new(leaf));
    owner.set_root_id(Some(root));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));

    HeadlessBinding::with_tree(
        BuildOwner::new(),
        ElementTree::new(),
        PipelineCell::new(owner),
    )
}

/// A single-child container, so a test can put the focused control somewhere
/// other than the root. A single-node tree cannot distinguish "focus was
/// derived" from "focus fell back to the root" — they are the same node.
#[derive(Debug, Default)]
struct SemanticContainer;

impl flui_foundation::Diagnosticable for SemanticContainer {}

impl RenderBox for SemanticContainer {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            let size = ctx.layout_child(0, constraints);
            ctx.position_child(0, flui_types::Offset::ZERO);
            size
        } else {
            constraints.smallest()
        }
    }

    flui_rendering::forward_single_child_box_queries!();

    fn paint(&self, _ctx: &mut PaintCx<'_, Single>) {}
}

/// A container root with `leaf` as its only child, so the leaf is not the root.
fn binding_with_child(leaf: SemanticLeaf) -> HeadlessBinding {
    let mut owner = PipelineOwner::new();
    let root = owner.insert::<BoxProtocol>(Box::new(SemanticContainer));
    owner
        .insert_child_render_object(root, Box::new(leaf))
        .expect("the container accepts one child");
    owner.set_root_id(Some(root));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));

    HeadlessBinding::with_tree(
        BuildOwner::new(),
        ElementTree::new(),
        PipelineCell::new(owner),
    )
}

fn pump(binding: &mut HeadlessBinding) {
    binding.pump_frame(std::time::Duration::from_millis(16));
}

/// **The acceptance test.** A button in the render tree is findable by role,
/// with its label, through the same translation a screen reader receives.
#[test]
fn a_button_in_the_render_tree_is_findable_by_role() {
    let mut binding = binding_with(SemanticLeaf {
        label: "Submit".to_string(),
        button: true,
        ..Default::default()
    });

    binding.enable_semantics().expect("binding is tree-bound");
    pump(&mut binding);

    let tree = binding
        .a11y_tree()
        .expect("semantics was enabled before the frame, so a tree exists");
    let button = tree
        .find(Role::Button)
        .unwrap_or_else(|e| panic!("expected exactly one button: {e}"));

    assert_eq!(button.label(), Some("Submit"));
}

/// Semantics stays off until asked for, so a harness that never opts in pays
/// nothing — and a query against it reports "no tree", not "no buttons".
#[test]
fn without_enable_semantics_there_is_no_tree_to_query() {
    let mut binding = binding_with(SemanticLeaf {
        label: "Submit".to_string(),
        button: true,
        ..Default::default()
    });

    pump(&mut binding);

    assert!(!binding.semantics_enabled());
    assert!(
        binding.a11y_tree().is_none(),
        "an un-built tree must be distinguishable from an empty one; \
         returning Some(empty) would let `find_all(Button)` assert `[]` \
         about a tree that was never assembled"
    );
}

/// Enabling after the frame does not retroactively build a tree — the phase it
/// controls has already run. Pinned because the failure is silent: the query
/// returns `None` and reads as "no semantics" rather than "you asked too late".
#[test]
fn enabling_after_the_frame_leaves_nothing_to_query_until_the_next_one() {
    let mut binding = binding_with(SemanticLeaf {
        label: "Submit".to_string(),
        button: true,
        ..Default::default()
    });

    pump(&mut binding);
    binding.enable_semantics().expect("binding is tree-bound");

    assert!(binding.semantics_enabled());

    pump(&mut binding);
    assert!(
        binding.a11y_tree().is_some(),
        "the frame after enabling assembles the tree"
    );
}

/// A structural role has no flag to carry it, so it reaches AccessKit only if
/// the explicit-role half of `resolve_role` is wired through the whole chain:
/// config -> node data -> translation -> query.
#[test]
fn a_structural_role_survives_the_whole_chain() {
    let mut binding = binding_with(SemanticLeaf {
        label: "Overview".to_string(),
        role: SemanticsRole::Tab,
        ..Default::default()
    });

    binding.enable_semantics().expect("binding is tree-bound");
    pump(&mut binding);

    let tree = binding.a11y_tree().expect("tree exists");
    let tab = tree
        .find(Role::Tab)
        .unwrap_or_else(|e| panic!("expected exactly one tab: {e}"));

    assert_eq!(tab.label(), Some("Overview"));
}

/// A node with no enabled-state concept must not be announced as disabled.
/// Getting this wrong makes a screen reader call every plain container
/// unavailable, and no role assertion would catch it.
#[test]
fn a_node_without_enabled_state_is_not_announced_as_disabled() {
    let mut binding = binding_with(SemanticLeaf {
        label: "Submit".to_string(),
        button: true,
        enabled: None,
        ..Default::default()
    });

    binding.enable_semantics().expect("binding is tree-bound");
    pump(&mut binding);

    let tree = binding.a11y_tree().expect("tree exists");
    let button = tree.find(Role::Button).expect("one button");

    assert!(
        !button.is_disabled(),
        "absence of enabled-state is not disablement"
    );
}

/// The disabled case, so the test above is pinning a distinction rather than a
/// constant `false`.
#[test]
fn an_explicitly_disabled_node_is_announced_as_disabled() {
    let mut binding = binding_with(SemanticLeaf {
        label: "Submit".to_string(),
        button: true,
        enabled: Some(false),
        ..Default::default()
    });

    binding.enable_semantics().expect("binding is tree-bound");
    pump(&mut binding);

    let tree = binding.a11y_tree().expect("tree exists");
    let button = tree.find(Role::Button).expect("one button");

    assert!(button.is_disabled());
}

/// `A11yTree::focus()` must name the focused control, not the root. The focused
/// leaf is deliberately a **child**: in a single-node tree "focus was derived"
/// and "focus fell back to the root" are the same answer, so that shape cannot
/// fail and would not be a test.
///
/// Before focus was derived from the tree, the harness passed `None` through and
/// every query reported the root — confidently wrong, so a focus assertion would
/// have inspected the wrong node and still passed.
#[test]
fn focus_names_the_focused_child_rather_than_the_root() {
    let mut binding = binding_with_child(SemanticLeaf {
        label: "Submit".to_string(),
        button: true,
        focused: true,
        ..Default::default()
    });

    binding.enable_semantics().expect("binding is tree-bound");
    pump(&mut binding);

    let tree = binding.a11y_tree().expect("tree exists");
    let focused = tree.focus().expect("a focused node is reachable");

    assert_eq!(focused.role(), Role::Button, "the child, not the container");
    assert_eq!(focused.label(), Some("Submit"));
    assert_ne!(
        focused.id(),
        tree.raw().tree.as_ref().expect("tree").root,
        "the fixture is only meaningful while the focused node is not the root"
    );
}
