//! End-to-end: a mounted render tree becomes a queryable accessibility tree.
//!
//! The module tests in `src/a11y.rs` build a `TreeUpdate` by hand, so they pin
//! the query logic and say nothing about whether the harness is wired to the
//! semantics phase at all. These tests start where a user does — a render tree
//! and a pumped frame — and are the ones that fail if `enable_semantics` stops
//! reaching the pipeline owner, if the semantics phase stops assembling, or if
//! the translation stops being invoked.

use flui_foundation::geometry::Size;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_rendering::prelude::*;
use flui_rendering::protocol::BoxProtocol;
use flui_semantics::{SemanticsConfiguration, SemanticsRole};
use flui_testing::HeadlessBinding;
use flui_testing::a11y::Role;
use flui_testing::{A11yQuery, A11yQueryError, A11yTree};
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

    fn perform_layout(
        &mut self,
        _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        Ok(Size::new(10.0, 10.0))
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
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let root = owner.insert::<BoxProtocol>(Box::new(leaf));
    owner.set_root_id(Some(root));
    owner.set_root_constraints(Some(BoxConstraints::new(0.0, 200.0, 0.0, 200.0)));

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
pub(crate) fn a_button_in_the_render_tree_is_findable_by_role() {
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

/// Selectors refuse duplicates and preserve every matching subject in reading
/// order, even when the payload's emission order differs from the tree.
pub(crate) fn unique_queries_preserve_subjects_and_complete_failure_diagnostics() {
    use accesskit::{Node, NodeId, TreeId, TreeInfo, TreeUpdate};

    let root_id = NodeId(1);
    let mut root = Node::new(Role::Window);
    root.set_children(vec![NodeId(2), NodeId(3), NodeId(4), NodeId(5)]);
    let button = |name: &str, value: &str| {
        let mut node = Node::new(Role::Button);
        node.set_label(name);
        node.set_value(value);
        node
    };
    let mut textbox = Node::new(Role::TextInput);
    textbox.set_label("Unique");
    let tree = A11yTree::new(TreeUpdate {
        nodes: vec![
            (NodeId(4), button("Repeated", "third")),
            (NodeId(3), button("Repeated", "second")),
            (NodeId(5), textbox),
            (NodeId(2), button("Repeated", "first")),
            (root_id, root),
        ],
        tree: Some(TreeInfo::new(root_id)),
        tree_id: TreeId::ROOT,
        focus: root_id,
    });

    assert_eq!(
        tree.find(Role::TextInput)
            .expect("one textbox is present")
            .id(),
        NodeId(5)
    );
    assert_eq!(
        tree.find_by_label("Unique")
            .expect("one subject has the unique label")
            .id(),
        NodeId(5)
    );
    for (result, query) in [
        (tree.find(Role::Image), A11yQuery::Role(Role::Image)),
        (
            tree.find_by_label("Absent"),
            A11yQuery::Label("Absent".to_string()),
        ),
    ] {
        let Err(A11yQueryError::NotFound {
            query: actual,
            tree: actual_tree,
        }) = result
        else {
            panic!("a missing subject must report NotFound");
        };
        assert_eq!(actual, query);
        assert_eq!(
            actual_tree,
            "  Window\n  Button label=\"Repeated\" value=\"first\"\n  Button label=\"Repeated\" value=\"second\"\n  Button label=\"Repeated\" value=\"third\"\n  TextInput label=\"Unique\""
        );
    }
    let expected = [
        "Button label=\"Repeated\" value=\"first\"",
        "Button label=\"Repeated\" value=\"second\"",
        "Button label=\"Repeated\" value=\"third\"",
    ];
    for (result, query) in [
        (tree.find(Role::Button), A11yQuery::Role(Role::Button)),
        (
            tree.find_by_label("Repeated"),
            A11yQuery::Label("Repeated".to_string()),
        ),
    ] {
        let Err(A11yQueryError::Ambiguous {
            query: actual,
            matches,
        }) = result
        else {
            panic!("three matching subjects must report Ambiguous");
        };
        assert_eq!(actual, query);
        assert_eq!(matches, expected);
    }
}
