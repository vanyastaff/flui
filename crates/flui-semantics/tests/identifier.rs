//! The author's stable identifier (ADR-0240), through the public API: what
//! publication exports, what assistive technology never hears, and what the
//! merge and boundary rules do with it.

use std::sync::Arc;

use flui_foundation::RenderId;
use flui_foundation::geometry::{DevicePixelRatio, Rect};
use flui_protocol::{ElementId, ReadQuery, WindowId};
use flui_semantics::{
    AccessibilityNodeId, Placement, SemanticsConfiguration, SemanticsNode, SemanticsOwner,
    SemanticsRole, TreeUpdate,
};
use parking_lot::Mutex;

const IDENTIFIER: &str = "row-7-delete";

fn render_id(index: u32) -> RenderId {
    RenderId::new_gen(
        index,
        core::num::NonZeroU32::new(5).expect("fixture generation is non-zero"),
    )
}

/// A labelled button named by [`IDENTIFIER`] under a window root, published
/// once: the update the adapter received and the owner that sent it.
fn published_button() -> (SemanticsOwner, TreeUpdate) {
    let received: Arc<Mutex<Vec<TreeUpdate>>> = Arc::default();
    let sink = Arc::clone(&received);
    let mut owner = SemanticsOwner::new(Arc::new(move |update: &TreeUpdate| {
        sink.lock().push(update.clone());
    }));
    let mut root = SemanticsNode::new().with_source_render_id(render_id(1));
    root.set_rect(Rect::from_ltwh(0.0, 0.0, 200.0, 100.0));
    root.config_mut().set_label("Inbox");
    let root = owner.insert(root);
    let mut button = SemanticsNode::new().with_source_render_id(render_id(2));
    button.set_rect(Rect::from_ltwh(10.0, 10.0, 80.0, 30.0));
    let config = button.config_mut();
    config.set_role(SemanticsRole::Button);
    config.set_label("Delete");
    config.set_identifier(IDENTIFIER);
    let button = owner.insert(button);
    owner.add_child(root, button);
    owner.set_root(Some(root));
    owner.flush();
    let update = received
        .lock()
        .pop()
        .expect("the first flush publishes the tree");
    (owner, update)
}

fn button_id() -> AccessibilityNodeId {
    AccessibilityNodeId::from(render_id(2))
}

/// The published AccessKit node of the button.
fn published_node(update: &TreeUpdate) -> &accesskit::Node {
    update
        .nodes
        .iter()
        .find(|(id, _)| id.0 == button_id().as_u64())
        .map(|(_, node)| node)
        .expect("the button is published")
}

/// The button as an agent reads it from the wire.
fn wire_button(owner: &SemanticsOwner) -> flui_protocol::Node {
    let element =
        ElementId::from_u64(button_id().as_u64()).expect("an accessibility id is non-zero");
    let placement = Placement::new(
        DevicePixelRatio::new(1.0).expect("1 is a ratio"),
        WindowId::from_u64(1).expect("non-zero"),
    );
    owner
        .read_wire(&ReadQuery::new().with_root(element), placement)
        .expect("the published tree reads")
        .roots
        .into_iter()
        .next()
        .expect("one root")
}

/// A configuration that names its node and says `label`.
fn named(label: &str, identifier: &str) -> SemanticsConfiguration {
    let mut config = SemanticsConfiguration::new();
    config.set_label(label);
    config.set_identifier(identifier);
    config
}

/// One claim ADR-0240 makes about the identifier, and whether it holds.
struct Claim {
    claim: &'static str,
    holds: fn() -> bool,
}

/// Exported, never announced, never merged into another node, and no
/// boundary of its own: every claim holds.
#[test]
fn the_identifier_is_exported_unannounced_unmerged_and_no_boundary() {
    let claims = [
        Claim {
            claim: "exported to the adapter as author_id",
            holds: || published_node(&published_button().1).author_id() == Some(IDENTIFIER),
        },
        Claim {
            claim: "exported on the wire as identifier, not automation_id",
            holds: || {
                let node = wire_button(&published_button().0);
                node.identifier.as_deref() == Some(IDENTIFIER) && node.automation_id.is_none()
            },
        },
        Claim {
            claim: "not announced: the adapter's name, description and value leave it out",
            holds: || {
                let (_, update) = published_button();
                let node = published_node(&update);
                node.label() == Some("Delete")
                    && node.description().is_none()
                    && node.value().is_none()
                    && node.tooltip().is_none()
            },
        },
        Claim {
            claim: "not announced: the wire name, description and value leave it out",
            holds: || {
                let node = wire_button(&published_button().0);
                node.name.as_deref() == Some("Delete")
                    && node.description.is_none()
                    && node.value.is_none()
            },
        },
        Claim {
            claim: "not merged: a parent absorbing the node takes its label, not its identifier",
            holds: || {
                let mut parent = SemanticsConfiguration::new();
                parent.set_label("Row 7");
                parent.absorb(&named("Delete", IDENTIFIER));
                parent.identifier().is_none()
                    && parent
                        .label()
                        .is_some_and(|label| !label.as_str().contains(IDENTIFIER))
            },
        },
        Claim {
            claim: "not merged: a merged parent keeps its own identifier",
            holds: || {
                let mut parent = named("Row 7", "row-7");
                parent.absorb(&named("Delete", IDENTIFIER));
                parent.identifier() == Some("row-7")
            },
        },
        Claim {
            claim: "no boundary: setting it forms no node and annotates nothing",
            holds: || {
                let mut config = SemanticsConfiguration::new();
                config.set_identifier(IDENTIFIER);
                config.identifier() == Some(IDENTIFIER)
                    && !config.is_semantics_boundary()
                    && !config.has_been_annotated()
            },
        },
        Claim {
            claim: "no boundary: an identifier alone contributes nothing to a parent",
            holds: || {
                let mut parent = named("Row 7", "row-7");
                let before = parent.clone();
                let mut child = SemanticsConfiguration::new();
                child.set_identifier(IDENTIFIER);
                parent.is_compatible_with(&child) && {
                    parent.absorb(&child);
                    parent == before
                }
            },
        },
    ];
    let failed: Vec<&str> = claims
        .iter()
        .filter(|claim| !(claim.holds)())
        .map(|claim| claim.claim)
        .collect();
    assert!(
        failed.is_empty(),
        "these identifier claims fail: {failed:#?}"
    );
}
