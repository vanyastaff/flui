//! The incremental-publish contract of `SemanticsOwner::flush`.
//!
//! The flush serializes only changed nodes into the platform update and
//! returns immediately when nothing is dirty. FLUI rebuilds its semantics arena every assembly pass (flui-semantics ARCHITECTURE.md, semantics assembly),
//! so "dirty" alone cannot distinguish a real change from a rebuild that
//! reproduced the same tree — the flush diffs per-node payloads, keyed by
//! stable [`AccessibilityNodeId`], against the last delivered update. These
//! tests pin the observable halves of that contract:
//!
//! - a pass that changes nothing publishes **nothing**;
//! - a pass that changes one node publishes **that node**, not the tree.
//!
//! Every fixture drives the owner exactly the way the pipeline does
//! (`rebuild_semantics_owner`): clear the arena, reinsert every node, root
//! it, flush. The tests deliberately use only API that predates the diff so
//! they can run — and fail — against the pre-diff implementation.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_foundation::RenderId;
use flui_semantics::{SemanticsNode, SemanticsOwner, TreeUpdate};
use parking_lot::Mutex;

/// A render-backed node: production assembly always attaches a source
/// render id, and it is what makes a node publishable at all. Generation 7
/// keeps the stable id space visibly distinct from arena positions.
fn node(index: u32, label: &str) -> SemanticsNode {
    let mut node = SemanticsNode::new().with_source_render_id(RenderId::new_gen(
        index,
        core::num::NonZeroU32::new(7).expect("fixture generation is non-zero"),
    ));
    node.config_mut().set_label(label);
    node
}

/// Rebuild the owner's arena the way `run_semantics` does every pass:
/// clear, reinsert, root. `labels[i]` becomes child `i` of the root.
fn rebuild(owner: &mut SemanticsOwner, labels: &[&str]) {
    owner.clear();
    let root = owner.insert(node(0, "root"));
    for (index, label) in labels.iter().enumerate() {
        let child = owner.insert(node(
            u32::try_from(index).expect("fixture fits u32") + 1,
            label,
        ));
        owner.add_child(root, child);
    }
    owner.set_root(Some(root));
}

/// An owner whose callback records every delivered update.
fn recording_owner() -> (SemanticsOwner, Arc<Mutex<Vec<TreeUpdate>>>) {
    let received: Arc<Mutex<Vec<TreeUpdate>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&received);
    let owner = SemanticsOwner::new(Arc::new(move |update: &TreeUpdate| {
        sink.lock().push(update.clone());
    }));
    (owner, received)
}

/// **The incremental-publish contract.** An identical rebuild publishes
/// nothing (the idle contract); one changed label publishes exactly that node,
/// not its siblings or the unchanged root (the O(dirty) contract).
#[test]
fn a_rebuild_publishes_only_what_changed() {
    let (mut owner, received) = recording_owner();

    rebuild(&mut owner, &["alpha", "beta", "gamma"]);
    owner.flush();
    assert_eq!(
        received.lock().len(),
        1,
        "the first publish is the full tree"
    );

    rebuild(&mut owner, &["alpha", "beta", "gamma"]);
    owner.flush();
    assert_eq!(
        received.lock().len(),
        1,
        "an identical rebuild must not deliver a second update"
    );

    rebuild(&mut owner, &["alpha", "CHANGED", "gamma"]);
    owner.flush();

    let updates = received.lock();
    assert_eq!(updates.len(), 2, "the change itself must be delivered");
    let diff = &updates[1];
    assert_eq!(
        diff.nodes.len(),
        1,
        "one changed node publishes one node, not the tree: got {:?}",
        diff.nodes
            .iter()
            .map(|(id, node)| (id.0, node.label().map(str::to_owned)))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        diff.nodes[0].1.label(),
        Some("CHANGED"),
        "and it is the node whose content changed"
    );
}

/// The published tree is in physical pixels, and a ratio change on an
/// otherwise idle tree republishes it.
///
/// AccessKit reads a node's bounds under its ancestors' transforms as
/// physical pixels relative to the window. The tree keeps logical rects, the
/// root carries the scale, and a changed ratio resends the root alone: the
/// descendants inherit the new transform without being re-serialized.
#[test]
fn the_published_root_scales_logical_bounds_to_physical_pixels() {
    use flui_foundation::geometry::Rect as LogicalRect;

    let (mut owner, received) = recording_owner();
    owner.set_device_pixel_ratio(1.5);
    owner.clear();
    let root = owner.insert(node(0, "root"));
    let mut button = node(1, "button");
    button.set_rect(LogicalRect::from_ltrb(10.0, 20.0, 78.0, 36.0));
    let button = owner.insert(button);
    owner.add_child(root, button);
    owner.set_root(Some(root));
    owner.flush();

    let effective = |update: &TreeUpdate, root_update: &TreeUpdate| {
        let root_id = root_update.tree.as_ref().expect("a full update").root;
        let root_transform = update
            .nodes
            .iter()
            .chain(&root_update.nodes)
            .find(|(id, _)| *id == root_id)
            .and_then(|(_, node)| node.transform().copied())
            .unwrap_or(accesskit::Affine::IDENTITY);
        let (_, button) = root_update
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("button"))
            .expect("the button is published");
        root_transform.transform_rect_bbox(button.bounds().expect("the button has bounds"))
    };

    let first = received.lock()[0].clone();
    let button_bounds = first
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("button"))
        .and_then(|(_, node)| node.bounds())
        .expect("the button is published with bounds");
    assert_eq!(
        button_bounds,
        accesskit::Rect::new(10.0, 20.0, 78.0, 36.0),
        "node bounds stay logical; the scale lives on the root"
    );
    assert_eq!(
        effective(&first, &first),
        accesskit::Rect::new(15.0, 30.0, 117.0, 54.0),
        "at 150% a 68x16 control is 102x24 physical pixels"
    );

    owner.set_device_pixel_ratio(2.0);
    owner.flush();
    let updates = received.lock();
    assert_eq!(updates.len(), 2, "a ratio change must be delivered");
    let change = &updates[1];
    assert_eq!(
        change.nodes.len(),
        1,
        "only the root carries the scale, so only the root is resent"
    );
    assert_eq!(
        effective(change, &first),
        accesskit::Rect::new(20.0, 40.0, 156.0, 72.0),
        "after the ratio changed to 2.0"
    );
}

#[test]
fn detaching_checks_the_actual_parent_and_preserves_reparenting() {
    let (mut owner, received) = recording_owner();
    let root = owner.insert(node(0, "root"));
    let first = owner.insert(node(1, "first parent"));
    let second = owner.insert(node(2, "second parent"));
    let child = owner.insert(node(3, "child"));
    owner.set_root(Some(root));
    owner.add_child(root, first);
    owner.add_child(root, second);
    owner.add_child(first, child);
    owner.flush();

    owner.remove_child(second, child);
    owner.remove_child(flui_foundation::SemanticsId::new(99), child);
    assert_eq!(owner.tree().parent(child), Some(first));
    assert_eq!(owner.tree().children(first), Some(&[child][..]));
    owner.flush();
    assert_eq!(received.lock().len(), 1, "a refused detach changes nothing");

    owner.add_child(second, child);
    assert_eq!(owner.tree().parent(child), Some(second));
    assert_eq!(owner.tree().children(first), Some(&[][..]));
    assert_eq!(owner.tree().children(second), Some(&[child][..]));
    owner.flush();
    {
        let updates = received.lock();
        let moved = &updates[1];
        let first_id = owner
            .get(first)
            .expect("first parent")
            .accessibility_id()
            .expect("render-backed");
        let second_id = owner
            .get(second)
            .expect("second parent")
            .accessibility_id()
            .expect("render-backed");
        let child_id = owner
            .get(child)
            .expect("child")
            .accessibility_id()
            .expect("render-backed");
        let published = |id| {
            &moved
                .nodes
                .iter()
                .find(|(node_id, _)| node_id.0 == id)
                .expect("changed parent published")
                .1
        };
        assert_eq!(published(first_id.as_u64()).children(), []);
        assert_eq!(
            published(second_id.as_u64()).children(),
            &[accesskit::NodeId(child_id.as_u64())]
        );
    }

    owner.remove_child(second, child);
    assert_eq!(owner.tree().parent(child), None);
    assert_eq!(owner.tree().children(second), Some(&[][..]));
    owner.add_child(first, child);
    assert_eq!(owner.tree().parent(child), Some(first));
    assert_eq!(owner.tree().children(first), Some(&[child][..]));
}

fn failed_label_delivery_retries_unchanged_input() {
    assert_delivery_recovers(|owner, _, child| {
        owner
            .get_mut(child)
            .expect("live child")
            .config_mut()
            .set_label("changed");
    });
}

fn failed_focus_delivery_retries_unchanged_input() {
    assert_delivery_recovers(|owner, _, child| {
        owner
            .get_mut(child)
            .expect("live child")
            .config_mut()
            .set_focused(true);
    });
}

fn failed_removal_delivery_retries_unchanged_input() {
    assert_delivery_recovers(|owner, _, child| {
        drop(owner.remove(child));
    });
}

fn assert_delivery_recovers(
    change: fn(&mut SemanticsOwner, flui_foundation::SemanticsId, flui_foundation::SemanticsId),
) {
    let fail_delivery = Arc::new(AtomicBool::new(false));
    let received = Arc::new(Mutex::new(Vec::<TreeUpdate>::new()));
    let sink = Arc::clone(&received);
    let fail = Arc::clone(&fail_delivery);
    let mut owner = SemanticsOwner::new(Arc::new(move |update| {
        assert!(
            !fail.load(Ordering::Relaxed),
            "delivery failed before acceptance"
        );
        sink.lock().push(update.clone());
    }));
    let root = owner.insert(node(0, "root"));
    let child = owner.insert(node(1, "original"));
    owner.add_child(root, child);
    owner.set_root(Some(root));
    assert_eq!(owner.flush(), 2);
    change(&mut owner, root, child);
    let expected = owner.to_accesskit_tree_update(None).expect("rooted tree");

    fail_delivery.store(true, Ordering::Relaxed);
    let failure = catch_unwind(AssertUnwindSafe(|| owner.flush()))
        .expect_err("the callback rejects this attempt");
    let is_delivery_failure = failure
        .downcast_ref::<String>()
        .is_some_and(|message| message.contains("delivery failed before acceptance"))
        || failure
            .downcast_ref::<&str>()
            .is_some_and(|message| message.contains("delivery failed before acceptance"));
    std::mem::forget(failure);
    assert!(
        is_delivery_failure,
        "the original delivery failure remains authoritative"
    );
    assert!(owner.needs_flush(), "failed delivery keeps work pending");
    assert_eq!(received.lock().len(), 1);

    fail_delivery.store(false, Ordering::Relaxed);
    owner.flush();
    {
        let updates = received.lock();
        assert_eq!(updates.len(), 2, "unchanged input must retry delivery");
        let mut actual = updates[0]
            .nodes
            .iter()
            .cloned()
            .collect::<std::collections::HashMap<_, _>>();
        for (id, node) in &updates[1].nodes {
            actual.insert(*id, node.clone());
        }
        // Removal is conveyed by the parent's child list, not a tombstone.
        for (id, node) in &expected.nodes {
            assert_eq!(
                actual.get(id),
                Some(node),
                "retry delivers the current node"
            );
        }
        assert_eq!(
            updates[1].focus, expected.focus,
            "retry delivers current focus"
        );
    }
    assert!(!owner.needs_flush());
    assert_eq!(owner.flush(), 0);
    assert_eq!(
        received.lock().len(),
        2,
        "accepted retry leaves no duplicate debt"
    );

    owner
        .get_mut(root)
        .expect("root remains live")
        .config_mut()
        .set_label("next operation");
    assert_eq!(owner.flush(), 1);
    assert_eq!(
        received.lock().len(),
        3,
        "the next operation still progresses"
    );
}

#[test]
fn failed_incremental_delivery_preserves_retry_and_progress() {
    let cases: &[(&str, fn())] = &[
        (
            "failed_label_delivery_retries_unchanged_input",
            failed_label_delivery_retries_unchanged_input,
        ),
        (
            "failed_focus_delivery_retries_unchanged_input",
            failed_focus_delivery_retries_unchanged_input,
        ),
        (
            "failed_removal_delivery_retries_unchanged_input",
            failed_removal_delivery_retries_unchanged_input,
        ),
    ];
    let mut failures = Vec::new();
    for &(name, run) in cases {
        if let Err(payload) = catch_unwind(run) {
            failures.push(name);
            std::mem::forget(payload);
        }
    }
    assert!(
        failures.is_empty(),
        "delivery recovery rows failed: {failures:?}"
    );
}

fn selected_properties_publish_when_a_consumer_skips_empty_annotations() {
    use flui_semantics::{SemanticsConfiguration, SemanticsProperties};
    let properties = SemanticsProperties {
        selected: Some(true),
        ..SemanticsProperties::default()
    };
    let (mut owner, updates) = recording_owner();
    if !properties.is_empty() {
        let root = owner
            .insert(node(0, "").with_config(SemanticsConfiguration::from_properties(&properties)));
        owner.set_root(Some(root));
        owner.flush();
    }
    let updates = updates.lock();
    assert_eq!(
        updates.len(),
        1,
        "selection-only annotation must reach the platform"
    );
    assert_eq!(updates[0].nodes[0].1.is_selected(), Some(true));
}

fn absent_properties_and_empty_collections_are_empty() {
    let properties = flui_semantics::SemanticsProperties::default();
    assert!(properties.is_empty());
}

fn explicit_enabled_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        enabled: Some(false),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_checked_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        checked: Some(false),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_mixed_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        mixed: Some(false),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_selected_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        selected: Some(false),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_toggled_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        toggled: Some(false),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_expanded_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        expanded: Some(false),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_focused_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        focused: Some(false),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_focusable_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        focusable: Some(false),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_button_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        button: Some(false),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_link_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        link: Some(false),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_header_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        header: Some(false),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_image_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        image: Some(false),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_text_field_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        text_field: Some(false),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_slider_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        slider: Some(false),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_read_only_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        read_only: Some(false),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_hidden_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        hidden: Some(false),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_obscured_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        obscured: Some(false),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_multiline_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        multiline: Some(false),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_scopes_route_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        scopes_route: Some(false),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_names_route_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        names_route: Some(false),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_in_mutually_exclusive_group_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        in_mutually_exclusive_group: Some(false),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_live_region_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        live_region: Some(false),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_label_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        label: Some(flui_semantics::AttributedString::new("")),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_value_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        value: Some(flui_semantics::AttributedString::new("")),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_increased_value_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        increased_value: Some(flui_semantics::AttributedString::new("")),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_decreased_value_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        decreased_value: Some(flui_semantics::AttributedString::new("")),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_hint_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        hint: Some(flui_semantics::AttributedString::new("")),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_text_direction_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        text_direction: Some(flui_semantics::TextDirection::Ltr),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_sort_key_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        sort_key: Some(flui_semantics::SemanticsSortKey::new(0.0)),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_tags_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        tags: std::iter::once(flui_semantics::SemanticsTag::new("tag")).collect(),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_custom_actions_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        custom_actions: vec![flui_semantics::CustomSemanticsAction::new(1, "action")],
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

fn explicit_hint_overrides_is_a_present_property() {
    let properties = flui_semantics::SemanticsProperties {
        hint_overrides: Some(flui_semantics::SemanticsHintOverrides::default()),
        ..flui_semantics::SemanticsProperties::default()
    };
    assert!(!properties.is_empty());
}

#[test]
fn semantics_property_presence_includes_every_public_annotation() {
    let cases: &[(&str, fn())] = &[
        (
            "absent properties",
            absent_properties_and_empty_collections_are_empty,
        ),
        (
            "selection publication",
            selected_properties_publish_when_a_consumer_skips_empty_annotations,
        ),
        (
            "explicit_enabled_is_a_present_property",
            explicit_enabled_is_a_present_property,
        ),
        (
            "explicit_checked_is_a_present_property",
            explicit_checked_is_a_present_property,
        ),
        (
            "explicit_mixed_is_a_present_property",
            explicit_mixed_is_a_present_property,
        ),
        (
            "explicit_selected_is_a_present_property",
            explicit_selected_is_a_present_property,
        ),
        (
            "explicit_toggled_is_a_present_property",
            explicit_toggled_is_a_present_property,
        ),
        (
            "explicit_expanded_is_a_present_property",
            explicit_expanded_is_a_present_property,
        ),
        (
            "explicit_focused_is_a_present_property",
            explicit_focused_is_a_present_property,
        ),
        (
            "explicit_focusable_is_a_present_property",
            explicit_focusable_is_a_present_property,
        ),
        (
            "explicit_button_is_a_present_property",
            explicit_button_is_a_present_property,
        ),
        (
            "explicit_link_is_a_present_property",
            explicit_link_is_a_present_property,
        ),
        (
            "explicit_header_is_a_present_property",
            explicit_header_is_a_present_property,
        ),
        (
            "explicit_image_is_a_present_property",
            explicit_image_is_a_present_property,
        ),
        (
            "explicit_text_field_is_a_present_property",
            explicit_text_field_is_a_present_property,
        ),
        (
            "explicit_slider_is_a_present_property",
            explicit_slider_is_a_present_property,
        ),
        (
            "explicit_read_only_is_a_present_property",
            explicit_read_only_is_a_present_property,
        ),
        (
            "explicit_hidden_is_a_present_property",
            explicit_hidden_is_a_present_property,
        ),
        (
            "explicit_obscured_is_a_present_property",
            explicit_obscured_is_a_present_property,
        ),
        (
            "explicit_multiline_is_a_present_property",
            explicit_multiline_is_a_present_property,
        ),
        (
            "explicit_scopes_route_is_a_present_property",
            explicit_scopes_route_is_a_present_property,
        ),
        (
            "explicit_names_route_is_a_present_property",
            explicit_names_route_is_a_present_property,
        ),
        (
            "explicit_in_mutually_exclusive_group_is_a_present_property",
            explicit_in_mutually_exclusive_group_is_a_present_property,
        ),
        (
            "explicit_live_region_is_a_present_property",
            explicit_live_region_is_a_present_property,
        ),
        (
            "explicit_label_is_a_present_property",
            explicit_label_is_a_present_property,
        ),
        (
            "explicit_value_is_a_present_property",
            explicit_value_is_a_present_property,
        ),
        (
            "explicit_increased_value_is_a_present_property",
            explicit_increased_value_is_a_present_property,
        ),
        (
            "explicit_decreased_value_is_a_present_property",
            explicit_decreased_value_is_a_present_property,
        ),
        (
            "explicit_hint_is_a_present_property",
            explicit_hint_is_a_present_property,
        ),
        (
            "explicit_text_direction_is_a_present_property",
            explicit_text_direction_is_a_present_property,
        ),
        (
            "explicit_sort_key_is_a_present_property",
            explicit_sort_key_is_a_present_property,
        ),
        (
            "explicit_tags_is_a_present_property",
            explicit_tags_is_a_present_property,
        ),
        (
            "explicit_custom_actions_is_a_present_property",
            explicit_custom_actions_is_a_present_property,
        ),
        (
            "explicit_hint_overrides_is_a_present_property",
            explicit_hint_overrides_is_a_present_property,
        ),
    ];
    let mut failures = Vec::new();
    for &(name, run) in cases {
        if let Err(payload) = catch_unwind(run) {
            failures.push(name);
            std::mem::forget(payload);
        }
    }
    assert!(
        failures.is_empty(),
        "property presence rows failed: {failures:?}"
    );
}

/// A public owner fixture exposing one native numeric control.
struct NumericControl {
    owner: SemanticsOwner,
    id: flui_foundation::SemanticsId,
    target: flui_semantics::AccessibilityNodeId,
    received: Arc<Mutex<Vec<f64>>>,
}

impl NumericControl {
    fn new() -> Self {
        use flui_semantics::{ActionArgs, NumericRange, SemanticsAction};
        let (mut owner, _) = recording_owner();
        let mut control = node(1, "numeric control");
        let target = control.accessibility_id().expect("render-backed identity");
        let received = Arc::new(Mutex::new(Vec::new()));
        let numeric = Arc::clone(&received);
        control
            .config_mut()
            .set_numeric_range(NumericRange::new(0.0, 0.0, 10.0, 1.0).expect("finite fixture"));
        control.config_mut().add_action(
            SemanticsAction::SetNumericValue,
            Arc::new(move |_, args| {
                let Some(ActionArgs::SetNumericValue { value }) = args else {
                    panic!("numeric callback lost its payload");
                };
                numeric.lock().push(value);
            }),
        );
        let id = owner.insert(control);
        owner.set_root(Some(id));
        owner.flush();
        Self {
            owner,
            id,
            target,
            received,
        }
    }

    fn resolve(
        &self,
        arguments: Option<flui_semantics::ActionArgs>,
    ) -> Result<flui_semantics::SemanticsActionInvocation, flui_semantics::SemanticsActionError>
    {
        use flui_semantics::{SemanticsAction, SemanticsActionRequest};
        self.owner.resolve_action(SemanticsActionRequest {
            node_id: self.target,
            action: SemanticsAction::SetNumericValue,
            arguments,
        })
    }

    fn set(
        &self,
        value: f64,
    ) -> Result<flui_semantics::SemanticsActionInvocation, flui_semantics::SemanticsActionError>
    {
        self.resolve(Some(flui_semantics::ActionArgs::SetNumericValue { value }))
    }

    fn narrow_and_publish(&mut self) {
        self.owner
            .get_mut(self.id)
            .expect("control remains live")
            .config_mut()
            .set_numeric_range(
                flui_semantics::NumericRange::new(0.0, 0.0, 5.0, 1.0)
                    .expect("finite narrower range"),
            );
        self.owner.mark_dirty(self.id);
        assert_eq!(
            self.owner.flush(),
            1,
            "the narrower metadata reaches the adapter"
        );
    }
}

fn an_exact_value_inside_the_current_range_reaches_the_handler() {
    let mut control = NumericControl::new();
    control.narrow_and_publish();
    control
        .set(4.375)
        .expect("the current range admits an exact value off the step")
        .invoke();
    assert_eq!(*control.received.lock(), [4.375]);
}

fn a_value_outside_the_current_range_is_refused() {
    use flui_semantics::SemanticsActionError;
    let mut control = NumericControl::new();
    control
        .set(9.0)
        .expect("the published range admits 9")
        .invoke();
    control.narrow_and_publish();
    assert_eq!(
        control.set(9.0).err(),
        Some(SemanticsActionError::InvalidNumericValue {
            node_id: control.target
        })
    );
    assert_eq!(*control.received.lock(), [9.0]);
}

fn non_finite_or_missing_values_are_refused() {
    use flui_semantics::{ActionArgs, SemanticsActionError};
    let control = NumericControl::new();
    let refused = Some(SemanticsActionError::InvalidNumericValue {
        node_id: control.target,
    });
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 10.5] {
        assert_eq!(control.set(value).err(), refused, "{value} was admitted");
    }
    assert_eq!(control.resolve(None).err(), refused);
    assert_eq!(
        control
            .resolve(Some(ActionArgs::SetText { text: "5".into() }))
            .err(),
        refused
    );
    assert!(control.received.lock().is_empty());
}

/// A numeric setter is admitted only with a finite value inside the node's
/// current range; a refused value never reaches the handler.
#[test]
fn numeric_setters_are_checked_against_the_current_range() {
    let cases: &[(&str, fn())] = &[
        (
            "exact_value_in_range",
            an_exact_value_inside_the_current_range_reaches_the_handler,
        ),
        (
            "value_outside_narrowed_range",
            a_value_outside_the_current_range_is_refused,
        ),
        (
            "non_finite_or_missing",
            non_finite_or_missing_values_are_refused,
        ),
    ];
    let mut failures = Vec::new();
    for &(name, run) in cases {
        if let Err(payload) = catch_unwind(run) {
            failures.push(name);
            std::mem::forget(payload);
        }
    }
    assert!(
        failures.is_empty(),
        "numeric range admission rows failed: {failures:?}"
    );
}

struct RevealFixture {
    owner: SemanticsOwner,
    outer: flui_foundation::SemanticsId,
    inner: flui_foundation::SemanticsId,
    target: flui_foundation::SemanticsId,
    identity: flui_semantics::AccessibilityNodeId,
    calls: Arc<Mutex<Vec<&'static str>>>,
    failures: [Arc<AtomicBool>; 2],
    retired: Arc<std::sync::atomic::AtomicUsize>,
}

struct RevealCapture {
    retired: Arc<std::sync::atomic::AtomicUsize>,
    panic_on_drop: bool,
}

impl Drop for RevealCapture {
    fn drop(&mut self) {
        self.retired.fetch_add(1, Ordering::SeqCst);
        assert!(!self.panic_on_drop, "reveal capture retirement");
    }
}

impl RevealFixture {
    fn new(panic_on_drop: bool) -> Self {
        use flui_foundation::geometry::Rect;
        use flui_semantics::{ActionArgs, SemanticsAction};
        let (mut owner, _) = recording_owner();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let failures = [
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        ];
        let retired = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut make_owner = |index, name, failure: &Arc<AtomicBool>, panic_on_drop| {
            let mut ancestor = node(index, name);
            ancestor.set_reveal_rect(Rect::new(0.0, 0.0, 200.0, 200.0));
            let sink = Arc::clone(&calls);
            let failure = Arc::clone(failure);
            let capture = RevealCapture {
                retired: Arc::clone(&retired),
                panic_on_drop,
            };
            ancestor.config_mut().add_action(
                SemanticsAction::ShowOnScreen,
                Arc::new(move |_, args| {
                    let _capture = &capture;
                    assert!(matches!(args, Some(ActionArgs::ShowOnScreen { .. })));
                    sink.lock().push(name);
                    assert!(!failure.load(Ordering::SeqCst), "{name} reveal failure");
                }),
            );
            owner.insert(ancestor)
        };
        let outer = make_owner(0, "outer", &failures[1], false);
        let inner = make_owner(1, "inner", &failures[0], panic_on_drop);
        let mut target_node = node(2, "reveal target");
        target_node.set_reveal_rect(Rect::new(0.0, 600.0, 40.0, 640.0));
        target_node.config_mut().set_has_reveal_ancestor(true);
        let identity = target_node
            .accessibility_id()
            .expect("render-backed target");
        let target = owner.insert(target_node);
        owner.add_child(outer, inner);
        owner.add_child(inner, target);
        owner.set_root(Some(outer));
        Self {
            owner,
            outer,
            inner,
            target,
            identity,
            calls,
            failures,
            retired,
        }
    }

    fn resolve(&self) -> flui_semantics::SemanticsActionInvocation {
        self.owner
            .resolve_action(flui_semantics::SemanticsActionRequest::new(
                self.identity,
                flui_semantics::SemanticsAction::ShowOnScreen,
            ))
            .expect("rooted descendant reveal")
    }
}

fn reveal_stale_snapshot_is_refused_and_fresh_replacement_recovers() {
    for replace_target in [true, false] {
        let mut fixture = RevealFixture::new(false);
        let held = fixture.resolve();
        let id = if replace_target {
            fixture.target
        } else {
            fixture.inner
        };
        let replacement = fixture.owner.get(id).expect("live node").clone();
        assert!(fixture.owner.tree_mut().replace_node(id, replacement));
        held.invoke();
        assert!(
            fixture.calls.lock().is_empty(),
            "replaced membership is stale"
        );
        fixture.resolve().invoke();
        assert_eq!(*fixture.calls.lock(), ["inner", "outer"]);
    }
}

fn reveal_removed_and_reparented_paths_do_not_deliver_old_geometry() {
    for remove in [true, false] {
        let mut fixture = RevealFixture::new(false);
        let held = fixture.resolve();
        if remove {
            fixture.owner.remove(fixture.target);
        } else {
            fixture.owner.add_child(fixture.outer, fixture.target);
        }
        held.invoke();
        assert!(fixture.calls.lock().is_empty());
        if !remove {
            fixture.resolve().invoke();
            assert_eq!(*fixture.calls.lock(), ["outer"]);
        }
    }
}

fn reveal_first_body_failure_keeps_live_outer_progress_and_recovery() {
    for competing in [false, true] {
        let fixture = RevealFixture::new(false);
        fixture.failures[0].store(true, Ordering::SeqCst);
        fixture.failures[1].store(competing, Ordering::SeqCst);
        let first = catch_unwind(AssertUnwindSafe(|| fixture.resolve().invoke()))
            .expect_err("inner callback fails");
        assert_eq!(
            first.downcast_ref::<String>().map(String::as_str),
            Some("inner reveal failure")
        );
        assert_eq!(*fixture.calls.lock(), ["inner", "outer"]);
        fixture.failures[0].store(false, Ordering::SeqCst);
        fixture.failures[1].store(false, Ordering::SeqCst);
        fixture.resolve().invoke();
        assert_eq!(*fixture.calls.lock(), ["inner", "outer", "inner", "outer"]);
        let retired = Arc::clone(&fixture.retired);
        drop(fixture);
        assert_eq!(
            retired.load(Ordering::SeqCst),
            0,
            "failed snapshot retains captures after final owner release"
        );
    }
}

fn reveal_healthy_capture_retirement_and_failed_retirement_keep_authority() {
    let fixture = RevealFixture::new(false);
    fixture.resolve().invoke();
    let retired = Arc::clone(&fixture.retired);
    drop(fixture);
    assert_eq!(retired.load(Ordering::SeqCst), 2);

    let mut fixture = RevealFixture::new(true);
    let held = fixture.resolve();
    fixture.owner.clear();
    let first =
        catch_unwind(AssertUnwindSafe(|| held.invoke())).expect_err("capture retirement fails");
    assert_eq!(
        first.downcast_ref::<&str>().copied(),
        Some("reveal capture retirement")
    );
    assert!(
        fixture.calls.lock().is_empty(),
        "stale geometry never invokes a callback"
    );
    assert_eq!(
        fixture.retired.load(Ordering::SeqCst),
        1,
        "first retirement failure retains the remaining callback"
    );
}

#[test]
fn descendant_reveal_snapshots_preserve_identity_failure_and_recovery() {
    let cases: &[(&str, fn())] = &[
        (
            "replaced membership and recovery",
            reveal_stale_snapshot_is_refused_and_fresh_replacement_recovers,
        ),
        (
            "removed and reparented geometry",
            reveal_removed_and_reparented_paths_do_not_deliver_old_geometry,
        ),
        (
            "single and competing body failure",
            reveal_first_body_failure_keeps_live_outer_progress_and_recovery,
        ),
        (
            "healthy and failed capture retirement",
            reveal_healthy_capture_retirement_and_failed_retirement_keep_authority,
        ),
    ];
    let mut failures = Vec::new();
    for &(name, run) in cases {
        if let Err(payload) = catch_unwind(run) {
            failures.push(name);
            std::mem::forget(payload);
        }
    }
    assert!(
        failures.is_empty(),
        "descendant reveal rows failed: {failures:?}"
    );
}
