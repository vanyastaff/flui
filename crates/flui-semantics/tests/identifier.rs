//! The author's stable identifier (ADR-0240), through the public API: what
//! publication exports, what assistive technology never hears, and what the
//! merge and boundary rules do with it.

use std::sync::Arc;

use flui_foundation::RenderId;
use flui_foundation::geometry::{DevicePixelRatio, Rect};
use flui_protocol::{ElementId, ReadQuery, WindowId};
use flui_semantics::{
    AccessibilityNodeId, DuplicateIdentifier, Placement, SemanticsConfiguration,
    SemanticsDiagnostic, SemanticsNode, SemanticsOwner, SemanticsRole, TreeUpdate,
};
use parking_lot::Mutex;

const IDENTIFIER: &str = "row-7-delete";

fn render_id(index: u32) -> RenderId {
    RenderId::new_gen(
        index,
        flui_foundation::generation::Generation::try_from(
            core::num::NonZeroU32::new(5).expect("fixture generation is non-zero"),
        )
        .expect("a fixture generation leaves the reserved bit clear"),
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
        DevicePixelRatio::try_new(1.0).expect("1 is a ratio"),
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
                parent.can_absorb(&child, false) && {
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

/// Siblings that share an identifier break its one rule (ADR-0240 §4): the
/// read reports the clash beside the tree, naming the parent and every
/// sibling carrying it, and publishes the identifiers as written. The same
/// identifier on nodes that are not siblings is no clash, siblings are the
/// children an agent sees (a child lifted out of an unlabelled container is
/// one), and the check covers the children of a node whose children the
/// budget leaves out.
#[test]
fn siblings_sharing_an_identifier_are_reported_and_published_as_written() {
    let mut owner = SemanticsOwner::new(Arc::new(|_: &TreeUpdate| {}));
    let node = |index: u32, label: &str, identifier: Option<&str>| {
        let mut node = SemanticsNode::new().with_source_render_id(render_id(index));
        node.set_rect(Rect::from_ltwh(0.0, f64::from(index) * 10.0, 80.0, 10.0));
        let config = node.config_mut();
        if !label.is_empty() {
            config.set_label(label);
        }
        if let Some(identifier) = identifier {
            config.set_identifier(identifier);
        }
        node
    };
    let root = owner.insert(node(1, "List", None));
    let first = owner.insert(node(2, "Delete", Some("delete")));
    let second = owner.insert(node(3, "Delete", Some("delete")));
    let other = owner.insert(node(4, "Archive", Some("archive")));
    let group = owner.insert(node(5, "Group", None));
    let cousin = owner.insert(node(6, "Delete", Some("delete")));
    let container = owner.insert(node(7, "", None));
    let lifted = owner.insert(node(8, "Delete", Some("delete")));
    for child in [first, second, other, group, container] {
        owner.add_child(root, child);
    }
    owner.add_child(group, cousin);
    owner.add_child(container, lifted);
    owner.set_root(Some(root));
    owner.flush();

    let element = |index: u32| {
        ElementId::from_u64(AccessibilityNodeId::from(render_id(index)).as_u64())
            .expect("an accessibility id is non-zero")
    };
    let placement = Placement::new(
        DevicePixelRatio::try_new(1.0).expect("1 is a ratio"),
        WindowId::from_u64(1).expect("non-zero"),
    );
    let clash = vec![flui_protocol::TreeDiagnostic::DuplicateIdentifier {
        identifier: "delete".into(),
        parent: element(1),
        elements: vec![element(2), element(3), element(8)],
    }];

    let tree = owner
        .read_wire(&ReadQuery::new(), placement)
        .expect("the published tree reads");
    assert_eq!(tree.diagnostics, clash, "the whole read");
    let published: Vec<Option<&str>> = tree.roots[0]
        .children
        .iter()
        .map(|child| child.identifier.as_deref())
        .collect();
    assert_eq!(
        published,
        [
            Some("delete"),
            Some("delete"),
            Some("archive"),
            None,
            Some("delete")
        ],
        "the identifiers are published as their authors wrote them"
    );

    let shallow = owner
        .read_wire(&ReadQuery::new().with_max_depth(0), placement)
        .expect("the published tree reads");
    assert!(shallow.truncated);
    assert_eq!(
        shallow.diagnostics, clash,
        "a read that leaves the children out"
    );

    let below = owner
        .read_wire(&ReadQuery::new().with_root(element(5)), placement)
        .expect("the group reads");
    assert_eq!(below.diagnostics, Vec::new(), "an only child is no clash");
}

/// Debug builds queue a clash for the frame driver to report once it is
/// published, not on every flush, and again when an incremental publish
/// changes who shares the identifier.
#[test]
fn debug_builds_queue_a_published_clash_once_per_shape() {
    if !cfg!(debug_assertions) {
        return;
    }
    let mut owner = SemanticsOwner::new(Arc::new(|_: &TreeUpdate| {}));
    let node = |index: u32, identifier: &str| {
        let mut node = SemanticsNode::new().with_source_render_id(render_id(index));
        node.set_rect(Rect::from_ltwh(0.0, f64::from(index) * 10.0, 80.0, 10.0));
        let config = node.config_mut();
        config.set_label("Row");
        config.set_identifier(identifier);
        node
    };
    let mut root = SemanticsNode::new().with_source_render_id(render_id(1));
    root.set_rect(Rect::from_ltwh(0.0, 0.0, 200.0, 100.0));
    root.config_mut().set_label("List");
    let root = owner.insert(root);
    let rows: Vec<_> = [(2, "delete"), (3, "delete"), (4, "archive")]
        .into_iter()
        .map(|(index, identifier)| owner.insert(node(index, identifier)))
        .collect();
    for &row in &rows {
        owner.add_child(root, row);
    }
    owner.set_root(Some(root));
    let id = |index: u32| AccessibilityNodeId::from(render_id(index));
    type Clash = (String, AccessibilityNodeId, Vec<AccessibilityNodeId>);
    let clashes = |owner: &mut SemanticsOwner| -> Vec<Clash> {
        owner
            .take_diagnostics()
            .into_iter()
            .map(|diagnostic| match diagnostic {
                SemanticsDiagnostic::DuplicateIdentifier(DuplicateIdentifier {
                    identifier,
                    parent,
                    siblings,
                    ..
                }) => (identifier.to_string(), parent, siblings),
                other => panic!("only clashes are expected, got {other:?}"),
            })
            .collect()
    };
    let clash = |siblings: Vec<AccessibilityNodeId>| vec![("delete".to_owned(), id(1), siblings)];

    owner.flush();
    assert_eq!(
        clashes(&mut owner),
        clash(vec![id(2), id(3)]),
        "first publish"
    );

    owner.mark_dirty(rows[0]);
    owner.flush();
    assert_eq!(clashes(&mut owner), Vec::new(), "an unchanged clash");

    owner
        .get_mut(rows[2])
        .expect("the row is in the tree")
        .config_mut()
        .set_identifier("delete");
    owner.flush();
    assert_eq!(
        clashes(&mut owner),
        clash(vec![id(2), id(3), id(4)]),
        "a sibling joining the clash"
    );
}

/// A container named only by its identifier is published where an automation
/// client finds it: as a group beside other content, and inside a control
/// that takes its name from its contents as that control's identifier, so the
/// control keeps its name. A change of the ancestor's role republishes the
/// container, which did not change itself.
///
/// Fails if the container publishes as a generic container (every AccessKit
/// filter drops it with its identifier), as a group inside the button (the
/// button loses the name its text gives it), or if an incremental publish
/// leaves the container as the previous role placed it.
#[test]
fn a_container_named_only_by_its_identifier_is_a_group_or_names_its_control() {
    use std::collections::BTreeMap;

    use accesskit_consumer::{FilterResult, common_filter};

    let received: Arc<Mutex<Vec<TreeUpdate>>> = Arc::default();
    let sink = Arc::clone(&received);
    let mut owner = SemanticsOwner::new(Arc::new(move |update: &TreeUpdate| {
        sink.lock().push(update.clone());
    }));
    let node = |index: u32| {
        let mut node = SemanticsNode::new().with_source_render_id(render_id(index));
        node.set_rect(Rect::from_ltwh(0.0, 0.0, 80.0, 30.0));
        node
    };
    let mut root = node(1);
    root.config_mut().set_label("Inbox");
    let root = owner.insert(root);
    let host = owner.insert(node(2));
    let mut swatch = node(3);
    swatch.config_mut().set_identifier("swatch");
    let swatch = owner.insert(swatch);
    let mut text = node(4);
    text.config_mut().set_label("Save");
    let text = owner.insert(text);
    owner.add_child(root, host);
    owner.add_child(host, swatch);
    owner.add_child(swatch, text);
    owner.set_root(Some(root));

    // What the adapter holds after every delivered update, against what a
    // full publish of the same tree gives.
    let mut held: BTreeMap<u64, accesskit::Node> = BTreeMap::new();
    let mut publish = |owner: &mut SemanticsOwner| {
        owner.flush();
        for update in received.lock().drain(..) {
            held.extend(update.nodes.into_iter().map(|(id, node)| (id.0, node)));
        }
        let full = owner
            .to_accesskit_tree_update(None)
            .expect("the tree is published");
        for (id, node) in &full.nodes {
            assert_eq!(held.get(&id.0), Some(node), "node {id:?} as held");
        }
        accesskit_consumer::Tree::new(full, true)
    };
    // The nodes an adapter lists under the root, with what it reads of them.
    let shown = |tree: &accesskit_consumer::Tree| -> Vec<(accesskit::Role, Option<String>, Option<String>)> {
        let state = tree.state();
        let root = state.root();
        root.filtered_children(common_filter)
            .map(|node| {
                assert_eq!(common_filter(&node), FilterResult::Include);
                (node.role(), node.author_id().map(str::to_owned), node.label())
            })
            .collect()
    };

    let tree = publish(&mut owner);
    assert_eq!(
        shown(&tree),
        [(accesskit::Role::Group, Some("swatch".to_owned()), None)],
        "beside other content"
    );

    owner
        .get_mut(host)
        .expect("the host is in the tree")
        .config_mut()
        .set_role(SemanticsRole::Button);
    owner.mark_dirty(host);
    let tree = publish(&mut owner);
    assert_eq!(
        shown(&tree),
        [(
            accesskit::Role::Button,
            Some("swatch".to_owned()),
            Some("Save".to_owned())
        )],
        "inside a button"
    );

    owner
        .get_mut(host)
        .expect("the host is in the tree")
        .config_mut()
        .set_role(SemanticsRole::None);
    owner.mark_dirty(host);
    let tree = publish(&mut owner);
    assert_eq!(
        shown(&tree),
        [(accesskit::Role::Group, Some("swatch".to_owned()), None)],
        "back beside other content"
    );
    let _ = text;
}
