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
        Size::new(10.0, 10.0)
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
