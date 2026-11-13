//! End-to-end: a laid-out widget tree becomes a queryable accessibility tree.
//!
//! The module tests in `src/a11y.rs` build a `TreeUpdate` by hand, so they pin
//! the query logic and say nothing about whether the harness is wired to the
//! semantics phase at all. These tests start where a user does — a widget tree
//! and a pumped frame of its UI runtime — and are the ones that fail if
//! `enable_semantics` stops reaching the pipeline owner, if the semantics phase
//! stops assembling, or if the translation stops being invoked.

use std::time::Duration;

use flui_rendering::pipeline::PipelineCell;
use flui_semantics::{SemanticsConfiguration, SemanticsRole};
use flui_testing::a11y::Role;
use flui_testing::widgets::{LaidOut, lay_out, loose};
use flui_testing::{A11yQuery, A11yQueryError, A11yTree};
use flui_widgets::{Semantics, SizedBox};

/// A 10×10 leaf labelled `label` with the button role, `selected` when given,
/// laid out with semantics on.
fn button_with(label: &str, selected: Option<bool>) -> LaidOut {
    let mut config = SemanticsConfiguration::default();
    config.set_label(label.to_owned());
    config.set_role(SemanticsRole::Button);
    if let Some(selected) = selected {
        config.set_selected(selected);
    }
    let mut laid = lay_out(
        Semantics::from_configuration(config).child(SizedBox::new(10.0, 10.0)),
        loose(200.0),
    );
    laid.enable_semantics();
    laid
}

/// **The acceptance test.** A button in the tree is findable by role, with its
/// label, through the same translation a screen reader receives.
pub(crate) fn a_button_in_the_render_tree_is_findable_by_role() {
    let mut laid = button_with("Submit", None);
    laid.pump_for(Duration::from_millis(16));

    let tree = laid
        .a11y_tree()
        .expect("semantics was enabled before the frame, so a tree exists");
    let button = tree
        .find(Role::Button)
        .unwrap_or_else(|e| panic!("expected exactly one button: {e}"));

    assert_eq!(button.label(), Some("Submit"));
}

/// A frame fails a test that leaves out an attribute its role does not
/// support: a button carries no `selected`. A subscriber that disables
/// semantics while it handles the warning does not hide the finding.
pub(crate) fn a_headless_frame_fails_on_an_attribute_its_role_does_not_support() {
    thread_local! {
        #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
        static PIPELINE: std::cell::RefCell<Option<PipelineCell>> =
            const { std::cell::RefCell::new(None) };
    }

    /// Disables semantics on each semantics warning, through the pipeline.
    struct Disabling;
    impl tracing::Subscriber for Disabling {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            if event.metadata().target().starts_with("flui_semantics") {
                PIPELINE.with(|cell| {
                    if let Some(pipeline) = cell.borrow().as_ref() {
                        pipeline.with_mut(|owner| owner.set_semantics_enabled(false));
                    }
                });
            }
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    let fails = |subscriber: Option<Disabling>| {
        let mut laid = button_with("Submit", Some(true));
        PIPELINE.with(|cell| *cell.borrow_mut() = Some(laid.pipeline_owner()));
        let frame = std::panic::AssertUnwindSafe(|| laid.pump_for(Duration::from_millis(16)));
        let failure = match subscriber {
            Some(subscriber) => {
                tracing::subscriber::with_default(subscriber, || std::panic::catch_unwind(frame))
            }
            None => std::panic::catch_unwind(frame),
        };
        PIPELINE.with(|cell| *cell.borrow_mut() = None);
        let failure = failure.expect_err("the frame left `selected` out of a button");
        let message = failure
            .downcast_ref::<String>()
            .map_or_default(String::as_str);
        assert!(message.contains("attribute: Selected"), "{message}");
    };
    fails(None);
    fails(Some(Disabling));
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
