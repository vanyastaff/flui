//! Focus transitions and key delivery through the public focus API: notification
//! order, refused nested replacement, stale widget nodes, removal of the focused
//! node, traversal units, the key ledger and input modality.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use flui_foundation::geometry::Rect;
use flui_interaction::__runtime::mark_widget_owned;
use flui_interaction::focus::{
    EntryTarget, FocusAttachment, FocusCause, FocusManager, FocusNode, FocusNodeRegistration,
    FocusRequestOutcome, FocusScopeNode, FocusTreeError, KeyEventResult,
};
use flui_interaction::ids::FocusNodeId;
use flui_platform::{
    EventTime,
    keyboard::{Code, ImeComposition, Key, KeyEvent, KeyRelease, KeyRepeat, KeyState, NamedKey},
};

fn run_rows(rows: &[(&str, fn())]) {
    let mut failed = Vec::new();
    for &(name, row) in rows {
        if let Err(payload) = catch_unwind(row) {
            eprintln!("row `{name}` failed");
            failed.push(name);
            flui_foundation::panic::discard_payload(payload);
        }
    }
    assert!(failed.is_empty(), "failed rows: {failed:?}");
}

type Edges = Rc<RefCell<Vec<(Option<FocusNodeId>, Option<FocusNodeId>)>>>;

fn record_edges(manager: &FocusManager) -> Edges {
    let edges: Edges = Rc::default();
    let log = Rc::clone(&edges);
    manager.add_listener(Rc::new(move |previous, current| {
        log.borrow_mut().push((
            previous.map(|node| node.id()),
            current.map(|node| node.id()),
        ));
    }));
    edges
}

/// `count` nodes in one row below the root scope, left to right.
fn manager_with_nodes(
    count: usize,
) -> (Rc<FocusManager>, Vec<Rc<FocusNode>>, Vec<FocusAttachment>) {
    let manager = FocusManager::new();
    let nodes: Vec<_> = (0..count)
        .map(|index| FocusNode::with_debug_label(format!("node-{index}")))
        .collect();
    let attachments = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            node.set_rect(Rect::from_xywh(index as f64 * 20.0, 0.0, 10.0, 10.0));
            manager.root_scope().attach_node(node).expect("attach")
        })
        .collect();
    (manager, nodes, attachments)
}

fn primary(manager: &FocusManager) -> Option<FocusNodeId> {
    manager.primary_focus().map(|node| node.id())
}

// ── Transitions ─────────────────────────────────────────────────────────

#[test]
fn focus_transition_contracts() {
    run_rows(&[
        (
            "reentrant_request_during_notification_is_applied_after_and_published_in_order",
            reentrant_request_during_notification_is_applied_after_and_published_in_order,
        ),
        (
            "nested_replacement_publishes_no_stale_edge",
            nested_replacement_publishes_no_stale_edge,
        ),
        (
            "a widget's node it let go answers stale",
            widget_node_let_go_answers_stale,
        ),
        (
            "removal publishes one edge to the next candidate",
            removal_publishes_one_edge_to_the_next_candidate,
        ),
        (
            "removal of the last candidate wraps to the first",
            removal_of_the_last_candidate_wraps_to_the_first,
        ),
        (
            "removals in one frame keep the first position",
            removals_in_one_frame_keep_the_first_position,
        ),
        (
            "removal leaves no focus only without candidates",
            removal_leaves_no_focus_only_without_candidates,
        ),
        (
            "removal of the scope focuses the enclosing scope",
            removal_of_the_scope_focuses_the_enclosing_scope,
        ),
        (
            "removal never parks focus on a scope",
            removal_never_parks_focus_on_a_scope,
        ),
        (
            "a removed node put back keeps the focus",
            removed_node_put_back_keeps_the_focus,
        ),
        (
            "an explicit request replaces the successor",
            explicit_request_replaces_the_successor,
        ),
        (
            "traversal settles the removal first",
            traversal_settles_the_removal_first,
        ),
        (
            "removal inside a unit enters the unit",
            removal_inside_a_unit_enters_the_unit,
        ),
        (
            "a first focus asked behind a barred ancestor lands when it reopens",
            first_focus_behind_a_barred_ancestor_lands_when_it_reopens,
        ),
        (
            "a superseded inner first focus leaves the scope's restoration",
            superseded_inner_first_focus_leaves_the_scope_restoration,
        ),
    ]);
}

/// A scope inside a node that bars its descendants focus, which asked for
/// its first focus before it had a candidate, and the candidate that came
/// later.
struct PendingInnerFirstFocus {
    _inner: Rc<FocusScopeNode>,
    field: Rc<FocusNode>,
    _attachments: Vec<FocusAttachment>,
}

fn pending_inner_first_focus(outer: &Rc<FocusNode>) -> PendingInnerFirstFocus {
    let inner = FocusScopeNode::with_debug_label("inner");
    let field = FocusNode::with_debug_label("field");
    let mut attachments = vec![outer.attach_node(inner.as_focus_node()).expect("inner")];
    outer.set_descendants_are_focusable(false);
    let _ = inner.set_first_focus();
    attachments.push(inner.attach_node(&field).expect("field"));
    assert!(
        !field.has_primary_focus(),
        "the barred ancestor keeps it out"
    );
    PendingInnerFirstFocus {
        _inner: inner,
        field,
        _attachments: attachments,
    }
}

/// The request is accepted while barred, and fulfilled once the ancestor
/// lets its descendants take focus again.
fn first_focus_behind_a_barred_ancestor_lands_when_it_reopens() {
    let manager = FocusManager::new();
    let gate = FocusNode::with_debug_label("gate");
    let _gate = manager.root_scope().attach_node(&gate).expect("gate");
    let pending = pending_inner_first_focus(&gate);
    gate.set_descendants_are_focusable(true);
    assert!(pending.field.has_primary_focus());
}

/// A scope that restores its own focus (a route a closing dialog uncovers)
/// supersedes the intents waiting inside it before it reopens: the focus
/// returns to what it remembers.
fn superseded_inner_first_focus_leaves_the_scope_restoration() {
    let manager = FocusManager::new();
    let route = FocusScopeNode::with_debug_label("route");
    let opener = FocusNode::with_debug_label("opener");
    let _route = manager
        .root_scope()
        .attach_node(route.as_focus_node())
        .expect("route");
    let _opener = route.attach_node(&opener).expect("opener");
    let _ = opener.request_focus();
    let pending = pending_inner_first_focus(route.as_focus_node());
    route.supersede_inner_first_focus();
    route.as_focus_node().set_descendants_are_focusable(true);
    assert!(route.set_first_focus());
    assert!(opener.has_primary_focus());
    assert!(!pending.field.has_primary_focus());
}

/// A listener that requests focus during a notification has its request
/// applied after that notification, and edges are published in order.
fn reentrant_request_during_notification_is_applied_after_and_published_in_order() {
    let (manager, nodes, _attachments) = manager_with_nodes(3);
    let _ = nodes[0].request_focus();
    let edges = record_edges(&manager);

    let next = Rc::clone(&nodes[2]);
    let intermediate = Rc::downgrade(&nodes[1]);
    let _subscription = nodes[1].subscribe(Rc::new(move || {
        if intermediate
            .upgrade()
            .is_some_and(|node| node.has_primary_focus())
        {
            let _ = next.request_focus();
        }
    }));

    let _ = nodes[1].request_focus();

    assert!(nodes[2].has_primary_focus());
    assert_eq!(
        edges.borrow().as_slice(),
        &[
            (Some(nodes[0].id()), Some(nodes[1].id())),
            (Some(nodes[1].id()), Some(nodes[2].id())),
        ]
    );
}

/// Replacing a node from a focus listener is refused before anything
/// changes, so no edge for it is published ahead of the one in flight.
fn nested_replacement_publishes_no_stale_edge() {
    let (manager, nodes, mut attachments) = manager_with_nodes(2);
    let _ = nodes[0].request_focus();
    let edges = record_edges(&manager);

    let held = Rc::new(RefCell::new(Some(attachments.remove(1))));
    let replacement = FocusNode::with_debug_label("replacement");
    let outcome = Rc::new(RefCell::new(None));
    let (listener_held, listener_replacement, listener_outcome) = (
        Rc::clone(&held),
        Rc::clone(&replacement),
        Rc::clone(&outcome),
    );
    manager.add_listener(Rc::new(move |_, current| {
        if current.is_some() && listener_outcome.borrow().is_none() {
            let result = listener_held
                .borrow()
                .as_ref()
                .expect("attachment")
                .replace_node(&listener_replacement)
                .map(|_| ());
            *listener_outcome.borrow_mut() = Some(result);
        }
    }));

    let _ = nodes[1].request_focus();

    assert_eq!(
        *outcome.borrow(),
        Some(Err(FocusTreeError::NotificationInFlight {
            node: nodes[1].id()
        }))
    );
    assert!(nodes[1].has_primary_focus());
    assert!(!replacement.is_attached());
    assert_eq!(
        edges.borrow().as_slice(),
        &[(Some(nodes[0].id()), Some(nodes[1].id()))]
    );
    // Outside a notification the same replacement goes through.
    let attachment = held.borrow_mut().take().expect("attachment");
    assert!(attachment.replace_node(&replacement).is_ok());
}

/// A widget's own node answers `Stale` once its widget detached or replaced
/// it, including after an ancestor's removal; an application's node detached
/// with its host still waits for its next attachment.
fn widget_node_let_go_answers_stale() {
    let (manager, nodes, _attachments) = manager_with_nodes(1);
    let _ = nodes[0].request_focus();

    let widget = FocusNode::with_debug_label("widget");
    mark_widget_owned(&widget);
    let attachment = manager.root_scope().attach_node(&widget).expect("attach");
    let _ = attachment.detach();
    assert_eq!(widget.request_focus(), FocusRequestOutcome::Stale);
    assert!(
        nodes[0].has_primary_focus(),
        "a stale request changes nothing"
    );
    manager.root_scope().attach_node(&widget).expect("attach");
    assert!(
        nodes[0].has_primary_focus(),
        "and nothing waits for an attachment"
    );

    let replaced = FocusNode::with_debug_label("replaced");
    mark_widget_owned(&replaced);
    let attachment = manager.root_scope().attach_node(&replaced).expect("attach");
    let _replacement = attachment.replace_node(&FocusNode::new()).expect("replace");
    assert_eq!(replaced.request_focus(), FocusRequestOutcome::Stale);

    let host = FocusNode::with_debug_label("host");
    let host_attachment = manager.root_scope().attach_node(&host).expect("attach");
    let inner = FocusNode::with_debug_label("inner");
    mark_widget_owned(&inner);
    let inner_attachment = host.attach_node(&inner).expect("attach");
    let _ = host_attachment.detach();
    let _ = inner_attachment.detach();
    assert_eq!(inner.request_focus(), FocusRequestOutcome::Stale);

    let external = FocusNode::with_debug_label("external");
    let attachment = manager.root_scope().attach_node(&external).expect("attach");
    let _ = attachment.detach();
    assert_eq!(external.request_focus(), FocusRequestOutcome::Queued);
    manager.root_scope().attach_node(&external).expect("attach");
    assert!(external.has_primary_focus());
}

fn removal_publishes_one_edge_to_the_next_candidate() {
    let (manager, nodes, mut attachments) = manager_with_nodes(3);
    let _ = nodes[1].request_focus();
    let edges = record_edges(&manager);

    let _ = attachments.remove(1).detach();
    assert!(edges.borrow().is_empty(), "removal publishes nothing");
    assert_eq!(primary(&manager), Some(nodes[1].id()));

    manager.settle_orphaned_focus();
    manager.settle_orphaned_focus();
    assert!(nodes[2].has_primary_focus());
    assert_eq!(
        edges.borrow().as_slice(),
        &[(Some(nodes[1].id()), Some(nodes[2].id()))]
    );
}

fn removal_of_the_last_candidate_wraps_to_the_first() {
    let (manager, nodes, mut attachments) = manager_with_nodes(3);
    let _ = nodes[2].request_focus();
    let _ = attachments.remove(2).detach();
    manager.settle_orphaned_focus();
    assert!(nodes[0].has_primary_focus());
}

fn removals_in_one_frame_keep_the_first_position() {
    let (manager, nodes, mut attachments) = manager_with_nodes(4);
    let _ = nodes[1].request_focus();
    let third = attachments.remove(2);
    let _ = attachments.remove(1).detach();
    let _ = third.detach();
    manager.settle_orphaned_focus();
    assert!(nodes[3].has_primary_focus());
}

fn removal_leaves_no_focus_only_without_candidates() {
    let (manager, nodes, mut attachments) = manager_with_nodes(1);
    let _ = nodes[0].request_focus();
    let edges = record_edges(&manager);
    let _ = attachments.remove(0).detach();
    manager.settle_orphaned_focus();
    assert_eq!(primary(&manager), None);
    assert_eq!(edges.borrow().as_slice(), &[(Some(nodes[0].id()), None)]);
}

fn removal_of_the_scope_focuses_the_enclosing_scope() {
    let manager = FocusManager::new();
    let outside = FocusNode::with_debug_label("outside");
    outside.set_rect(Rect::from_xywh(0.0, 0.0, 10.0, 10.0));
    let _outside = manager.root_scope().attach_node(&outside).expect("attach");
    let route = FocusScopeNode::with_debug_label("route");
    let route_attachment = manager
        .root_scope()
        .attach_node(route.as_focus_node())
        .expect("attach");
    let inside = FocusNode::with_debug_label("inside");
    route.attach_node(&inside).expect("attach");
    let _ = inside.request_focus();
    let edges = record_edges(&manager);

    let _ = route_attachment.detach();
    manager.settle_orphaned_focus();
    assert!(outside.has_primary_focus());
    assert_eq!(
        edges.borrow().as_slice(),
        &[(Some(inside.id()), Some(outside.id()))]
    );
}

fn removal_never_parks_focus_on_a_scope() {
    let manager = FocusManager::new();
    let route = FocusScopeNode::with_debug_label("route");
    let route_attachment = manager
        .root_scope()
        .attach_node(route.as_focus_node())
        .expect("attach");
    let inside = FocusNode::with_debug_label("inside");
    route.attach_node(&inside).expect("attach");
    let _ = inside.request_focus();
    let _ = route_attachment.detach();
    manager.settle_orphaned_focus();
    assert_eq!(primary(&manager), None);
}

fn removed_node_put_back_keeps_the_focus() {
    let (manager, nodes, mut attachments) = manager_with_nodes(2);
    let _ = nodes[0].request_focus();
    let edges = record_edges(&manager);
    let _ = attachments.remove(0).detach();
    let _back = manager.root_scope().attach_node(&nodes[0]).expect("attach");
    manager.settle_orphaned_focus();
    assert!(nodes[0].has_primary_focus());
    assert!(edges.borrow().is_empty());
}

fn explicit_request_replaces_the_successor() {
    let (manager, nodes, mut attachments) = manager_with_nodes(3);
    let _ = nodes[1].request_focus();
    let edges = record_edges(&manager);
    let _ = attachments.remove(1).detach();
    let _ = nodes[0].request_focus();
    manager.settle_orphaned_focus();
    assert!(nodes[0].has_primary_focus());
    assert_eq!(
        edges.borrow().as_slice(),
        &[(Some(nodes[1].id()), Some(nodes[0].id()))]
    );
}

fn traversal_settles_the_removal_first() {
    let (manager, nodes, mut attachments) = manager_with_nodes(4);
    let _ = nodes[1].request_focus();
    let edges = record_edges(&manager);
    let _ = attachments.remove(1).detach();
    assert!(manager.focus_next());
    assert_eq!(
        edges.borrow().as_slice(),
        &[
            (Some(nodes[1].id()), Some(nodes[2].id())),
            (Some(nodes[2].id()), Some(nodes[3].id())),
        ]
    );
}

fn removal_inside_a_unit_enters_the_unit() {
    let manager = FocusManager::new();
    let list = FocusNode::with_debug_label("list");
    let _list = manager.root_scope().attach_node(&list).expect("attach");
    let rows: Vec<_> = (0..3)
        .map(|index| FocusNode::with_debug_label(format!("row-{index}")))
        .collect();
    let mut row_attachments: Vec<_> = rows
        .iter()
        .map(|row| list.attach_node(row).expect("attach"))
        .collect();
    let entry_row = Rc::clone(&rows[2]);
    let _entry =
        list.register_traversal_entry(Rc::new(move || EntryTarget::Node(Rc::clone(&entry_row))));
    let _ = rows[0].request_focus();
    let _ = row_attachments.remove(0).detach();
    manager.settle_orphaned_focus();
    assert!(rows[2].has_primary_focus());
}

// ── Traversal units ─────────────────────────────────────────────────────

/// A list as one Tab stop: `before`, the list with three rows, `after`.
struct ListFixture {
    manager: Rc<FocusManager>,
    before: Rc<FocusNode>,
    list: Rc<FocusNode>,
    rows: Vec<Rc<FocusNode>>,
    after: Rc<FocusNode>,
    _attachments: Vec<FocusAttachment>,
}

fn list_fixture(scope: Option<&Rc<FocusScopeNode>>) -> ListFixture {
    let manager = FocusManager::new();
    let parent = scope.map_or_else(
        || Rc::clone(manager.root_scope().as_focus_node()),
        |scope| Rc::clone(scope.as_focus_node()),
    );
    let mut attachments = Vec::new();
    if let Some(scope) = scope {
        attachments.push(
            manager
                .root_scope()
                .attach_node(scope.as_focus_node())
                .expect("attach"),
        );
    }
    let node = |label: &str, top: f64| {
        let node = FocusNode::with_debug_label(label);
        node.set_rect(Rect::from_xywh(0.0, top, 100.0, 10.0));
        node
    };
    let before = node("before", 0.0);
    let list = node("list", 20.0);
    let after = node("after", 100.0);
    let rows: Vec<_> = (0..3)
        .map(|index| node(&format!("row-{index}"), 20.0 + index as f64 * 20.0))
        .collect();
    attachments.push(parent.attach_node(&before).expect("attach"));
    attachments.push(parent.attach_node(&list).expect("attach"));
    for row in &rows {
        attachments.push(list.attach_node(row).expect("attach"));
    }
    attachments.push(parent.attach_node(&after).expect("attach"));
    ListFixture {
        manager,
        before,
        list,
        rows,
        after,
        _attachments: attachments,
    }
}

#[test]
fn traversal_unit_contracts() {
    run_rows(&[
        ("a unit is one tab stop", unit_is_one_tab_stop),
        (
            "pending and unit entries focus the unit",
            pending_and_unit_entries_focus_the_unit,
        ),
        (
            "an entry outside the unit focuses the unit",
            entry_outside_the_unit_focuses_the_unit,
        ),
        (
            "a panicking entry focuses the unit",
            panicking_entry_focuses_the_unit,
        ),
        (
            "scope history restores through the entry",
            scope_history_restores_through_the_entry,
        ),
        (
            "dropping the registration ends the unit",
            dropping_the_registration_ends_the_unit,
        ),
    ]);
}

fn unit_is_one_tab_stop() {
    let fixture = list_fixture(None);
    let entered = Rc::clone(&fixture.rows[1]);
    let _entry = fixture
        .list
        .register_traversal_entry(Rc::new(move || EntryTarget::Node(Rc::clone(&entered))));
    let manager = &fixture.manager;
    assert!(manager.focus_next());
    assert!(fixture.before.has_primary_focus());
    assert!(manager.focus_next());
    assert!(fixture.rows[1].has_primary_focus(), "Tab enters the list");
    assert!(manager.focus_next());
    assert!(
        fixture.after.has_primary_focus(),
        "Tab leaves the list whole"
    );
    assert!(manager.focus_previous());
    assert!(
        fixture.rows[1].has_primary_focus(),
        "Shift+Tab enters it too"
    );
    assert!(manager.focus_previous());
    assert!(fixture.before.has_primary_focus());
}

fn pending_and_unit_entries_focus_the_unit() {
    for target in [|| EntryTarget::Pending, || EntryTarget::Unit] {
        let fixture = list_fixture(None);
        let _entry = fixture.list.register_traversal_entry(Rc::new(target));
        let _ = fixture.after.request_focus();
        assert!(fixture.manager.focus_previous());
        assert!(
            fixture.list.has_primary_focus(),
            "Shift+Tab lands on the unit, not on its last row"
        );
    }
}

fn entry_outside_the_unit_focuses_the_unit() {
    let fixture = list_fixture(None);
    let outside = Rc::clone(&fixture.after);
    let _entry = fixture
        .list
        .register_traversal_entry(Rc::new(move || EntryTarget::Node(Rc::clone(&outside))));
    let _ = fixture.before.request_focus();
    assert!(fixture.manager.focus_next());
    assert!(fixture.list.has_primary_focus());
}

fn panicking_entry_focuses_the_unit() {
    let fixture = list_fixture(None);
    let _entry = fixture
        .list
        .register_traversal_entry(Rc::new(|| panic!("entry failure")));
    let _ = fixture.before.request_focus();
    let payload = catch_unwind(AssertUnwindSafe(|| fixture.manager.focus_next()))
        .expect_err("the entry's panic propagates");
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"entry failure"));
    assert!(fixture.list.has_primary_focus());
    assert!(fixture.manager.focus_next());
    assert!(fixture.after.has_primary_focus(), "traversal keeps working");
}

fn scope_history_restores_through_the_entry() {
    let route = FocusScopeNode::with_debug_label("route");
    let fixture = list_fixture(Some(&route));
    let entered = Rc::new(RefCell::new(Rc::clone(&fixture.rows[1])));
    let target = Rc::clone(&entered);
    let _entry = fixture.list.register_traversal_entry(Rc::new(move || {
        EntryTarget::Node(Rc::clone(&target.borrow()))
    }));
    let _ = fixture.rows[1].request_focus();
    fixture.manager.unfocus();
    *entered.borrow_mut() = Rc::clone(&fixture.rows[2]);
    assert!(route.set_first_focus());
    assert!(
        fixture.rows[2].has_primary_focus(),
        "the scope remembered the list, and entering it asks the list"
    );
}

fn dropping_the_registration_ends_the_unit() {
    let fixture = list_fixture(None);
    let entry = fixture
        .list
        .register_traversal_entry(Rc::new(|| EntryTarget::Unit));
    drop(entry);
    let _ = fixture.list.request_focus();
    assert!(fixture.manager.focus_next());
    assert!(fixture.rows[0].has_primary_focus());
}

// ── Keys ────────────────────────────────────────────────────────────────

fn key(state: KeyState, key: Key, code: Code) -> KeyEvent {
    KeyEvent::new(state, key, code, EventTime::from_nanos(0))
}

fn enter(state: KeyState, repeat: KeyRepeat) -> KeyEvent {
    key(state, Key::Named(NamedKey::Enter), Code::Enter).with_repeat(repeat)
}

fn tab(repeat: KeyRepeat) -> KeyEvent {
    key(KeyState::Down, Key::Named(NamedKey::Tab), Code::Tab).with_repeat(repeat)
}

/// Count every key event that reaches `node`'s handler.
fn count_keys(node: &Rc<FocusNode>) -> Rc<Cell<usize>> {
    let count = Rc::new(Cell::new(0));
    let seen = Rc::clone(&count);
    node.set_on_key_event(Rc::new(move |_| {
        seen.set(seen.get() + 1);
        KeyEventResult::Handled
    }));
    count
}

#[test]
fn keyboard_focus_contracts() {
    run_rows(&[
        (
            "key_repeats_follow_their_key_down",
            key_repeats_follow_their_key_down,
        ),
        (
            "a_release_after_focus_moved_only_cancels_its_press",
            a_release_after_focus_moved_only_cancels_its_press,
        ),
        (
            "a_press_again_without_its_release_cancels_the_first",
            a_press_again_without_its_release_cancels_the_first,
        ),
        (
            "a_cancellation_that_moves_focus_takes_the_new_press_along",
            a_cancellation_that_moves_focus_takes_the_new_press_along,
        ),
        (
            "a_cancellation_that_forgets_held_keys_keeps_the_new_press",
            a_cancellation_that_forgets_held_keys_keeps_the_new_press,
        ),
        (
            "a_failing_cancellation_stays_the_first_failure",
            a_failing_cancellation_stays_the_first_failure,
        ),
        (
            "traversal hands repeats to the new focus",
            traversal_hands_repeats_to_the_new_focus,
        ),
        (
            "repeats inside a unit follow the unit",
            repeats_inside_a_unit_follow_the_unit,
        ),
        (
            "orphan releases reach only global handlers",
            orphan_releases_reach_only_global_handlers,
        ),
        (
            "input method keys never reach the walk",
            input_method_keys_never_reach_the_walk,
        ),
        (
            "forgotten keys stop repeating",
            forgotten_keys_stop_repeating,
        ),
        (
            "a key-down no release follows is held by nobody",
            a_key_down_no_release_follows_is_held_by_nobody,
        ),
        (
            "an orphan release is reported with the ledger let go",
            an_orphan_release_is_reported_with_the_ledger_let_go,
        ),
        (
            "a panicking handler keeps its key's record",
            panicking_handler_keeps_its_keys_record,
        ),
        (
            "focus is visible after keyboard and assistive moves",
            focus_is_visible_after_keyboard_and_assistive_moves,
        ),
        (
            "a modality change notifies the focused node",
            modality_change_notifies_the_focused_node,
        ),
    ]);
}

/// Enter's key-down moves focus to another node programmatically (opens a
/// page); the held key's repeats and release do nothing there. A fresh press
/// on the new node repeats normally.
fn key_repeats_follow_their_key_down() {
    let (manager, nodes, _attachments) = manager_with_nodes(2);
    let _ = nodes[0].request_focus();
    let opener = Rc::downgrade(&nodes[1]);
    nodes[0].set_on_key_event(Rc::new(move |_| {
        if let Some(page) = opener.upgrade() {
            let _ = page.request_focus();
        }
        KeyEventResult::Handled
    }));
    let page = count_keys(&nodes[1]);

    assert!(
        manager
            .dispatch_key_event(&enter(KeyState::Down, KeyRepeat::First))
            .is_handled()
    );
    assert!(nodes[1].has_primary_focus());
    for _ in 0..5 {
        let _ = manager.dispatch_key_event(&enter(KeyState::Down, KeyRepeat::AutoRepeat));
    }
    let _ = manager.dispatch_key_event(&enter(KeyState::Up, KeyRepeat::First));
    assert_eq!(
        page.get(),
        0,
        "the held key's repeats and release did nothing"
    );

    let _ = manager.dispatch_key_event(&enter(KeyState::Down, KeyRepeat::First));
    let _ = manager.dispatch_key_event(&enter(KeyState::Down, KeyRepeat::AutoRepeat));
    let _ = manager.dispatch_key_event(&enter(KeyState::Up, KeyRepeat::First));
    assert_eq!(page.get(), 3, "a press on the new focus delivers all of it");
}

/// Record the codes of the presses `node` is told were cancelled.
fn record_cancels(node: &Rc<FocusNode>) -> (Rc<RefCell<Vec<Code>>>, FocusNodeRegistration) {
    let cancelled = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&cancelled);
    let registration = node.register_on_key_cancel(Rc::new(move |press: &KeyEvent| {
        assert_eq!(press.state(), KeyState::Down, "told of the key-down");
        seen.borrow_mut().push(press.code);
    }));
    (cancelled, registration)
}

/// A key held on one node while focus moves to another unit: its release is
/// no key event anywhere, so nothing on the press's path (a root binding
/// resolving at the primary focus) or at the new focus acts on it. Every node
/// on the press's path is told the key was cancelled instead, which ends what
/// the press began (a keyboard scroll).
fn a_release_after_focus_moved_only_cancels_its_press() {
    let (manager, nodes, _attachments) = manager_with_nodes(2);
    let root = manager.root_scope().as_focus_node();
    let _ = nodes[0].request_focus();
    let presser = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&presser);
    nodes[0].set_on_key_event(Rc::new(move |event| {
        seen.borrow_mut().push(event.state());
        if event.state() == KeyState::Down {
            KeyEventResult::Handled
        } else {
            KeyEventResult::Ignored
        }
    }));
    let releases = Rc::new(Cell::new(0));
    let bound = Rc::clone(&releases);
    root.set_on_key_event(Rc::new(move |event| {
        if event.state() == KeyState::Up {
            bound.set(bound.get() + 1);
        }
        KeyEventResult::Ignored
    }));
    let (pressed_cancels, _pressed) = record_cancels(&nodes[0]);
    let (root_cancels, _root) = record_cancels(root);
    let (since_cancels, _since) = record_cancels(&nodes[1]);
    let since = count_keys(&nodes[1]);
    let arrow = |state| key(state, Key::Named(NamedKey::ArrowDown), Code::ArrowDown);

    let _ = manager.dispatch_key_event(&arrow(KeyState::Down));
    assert!(manager.focus_next(), "focus moves to another unit");
    assert!(nodes[1].has_primary_focus());
    let _ = manager.dispatch_key_event(&arrow(KeyState::Down).with_repeat(KeyRepeat::AutoRepeat));
    assert!(
        pressed_cancels.borrow().is_empty(),
        "a held key is not cancelled before its release"
    );
    let _ = manager.dispatch_key_event(&arrow(KeyState::Up));
    assert_eq!(
        *presser.borrow(),
        [KeyState::Down],
        "the press's node hears neither the repeats nor the release"
    );
    assert_eq!(
        releases.get(),
        0,
        "no binding on the path hears the release"
    );
    assert_eq!(since.get(), 0, "the node focused since hears nothing");
    assert_eq!(
        *pressed_cancels.borrow(),
        [Code::ArrowDown],
        "the pressed node is told once"
    );
    assert_eq!(
        *root_cancels.borrow(),
        [Code::ArrowDown],
        "and so is its path"
    );
    assert!(since_cancels.borrow().is_empty(), "but not the new focus");

    // A cancel handler that panics: the rest of the path is still told, and
    // the first panic propagates once.
    let _ = nodes[1].request_focus();
    let _ = manager.dispatch_key_event(&arrow(KeyState::Down));
    let failing = nodes[1].register_on_key_cancel(Rc::new(|_: &KeyEvent| {
        panic!("cancel handler failed");
    }));
    let _ = nodes[0].request_focus();
    let failed = catch_unwind(AssertUnwindSafe(|| {
        manager.dispatch_key_event(&arrow(KeyState::Up))
    }));
    assert!(failed.is_err(), "the panic propagates");
    assert_eq!(
        *root_cancels.borrow(),
        [Code::ArrowDown, Code::ArrowDown],
        "after the panic the path is still told"
    );
    drop(failing);
    let _ = manager.dispatch_key_event(&arrow(KeyState::Down));
    assert_eq!(
        *presser.borrow(),
        [KeyState::Down, KeyState::Down],
        "the next press is delivered"
    );
}

/// A key pressed again while its release never arrived, after focus moved to
/// another unit: the new press belongs to the new focus, and the first
/// press's path is told its key was cancelled, since no release will reach
/// it now.
fn a_press_again_without_its_release_cancels_the_first() {
    let (manager, nodes, _attachments) = manager_with_nodes(2);
    let _ = nodes[0].request_focus();
    let (first_cancels, _first) = record_cancels(&nodes[0]);
    let (second_cancels, _second) = record_cancels(&nodes[1]);
    let second = count_keys(&nodes[1]);
    let arrow = |state| key(state, Key::Named(NamedKey::ArrowDown), Code::ArrowDown);

    let _ = manager.dispatch_key_event(&arrow(KeyState::Down));
    assert!(manager.focus_next(), "focus moves to another unit");
    let _ = manager.dispatch_key_event(&arrow(KeyState::Down));
    assert_eq!(
        *first_cancels.borrow(),
        [Code::ArrowDown],
        "the press whose release was lost is cancelled"
    );
    assert_eq!(second.get(), 1, "the new press reaches the new focus");
    let _ = manager.dispatch_key_event(&arrow(KeyState::Up));
    assert_eq!(second.get(), 2, "and so does its release");
    assert!(second_cancels.borrow().is_empty());
    assert_eq!(first_cancels.borrow().len(), 1, "told once");

    // Pressed again in the same unit, the next release still reaches the
    // press's path: nothing is cancelled.
    let _ = manager.dispatch_key_event(&arrow(KeyState::Down));
    let _ = manager.dispatch_key_event(&arrow(KeyState::Down));
    let _ = manager.dispatch_key_event(&arrow(KeyState::Up));
    assert!(second_cancels.borrow().is_empty());
}

/// The cancelled press's handler moves focus: the new press is delivered to
/// that focus, and so is its release.
fn a_cancellation_that_moves_focus_takes_the_new_press_along() {
    let (manager, nodes, _attachments) = manager_with_nodes(3);
    let _ = nodes[0].request_focus();
    let mover = Rc::downgrade(&nodes[2]);
    let _cancel = nodes[0].register_on_key_cancel(Rc::new(move |_: &KeyEvent| {
        if let Some(node) = mover.upgrade() {
            let _ = node.request_focus();
        }
    }));
    let (moved_cancels, _moved) = record_cancels(&nodes[2]);
    let moved = count_keys(&nodes[2]);
    let arrow = |state| key(state, Key::Named(NamedKey::ArrowDown), Code::ArrowDown);

    let _ = manager.dispatch_key_event(&arrow(KeyState::Down));
    let _ = nodes[1].request_focus();
    let _ = manager.dispatch_key_event(&arrow(KeyState::Down));
    assert!(nodes[2].has_primary_focus(), "the cancellation moved focus");
    assert_eq!(
        moved.get(),
        1,
        "the new press reaches the focus it moved to"
    );
    let _ = manager.dispatch_key_event(&arrow(KeyState::Up));
    assert_eq!(moved.get(), 2, "and so does its release");
    assert!(moved_cancels.borrow().is_empty(), "nothing cancels it");
}

/// The cancelled press's handler forgets every held key: the new press is
/// admitted after it, so its release still reaches the focus.
fn a_cancellation_that_forgets_held_keys_keeps_the_new_press() {
    let (manager, nodes, _attachments) = manager_with_nodes(2);
    let _ = nodes[0].request_focus();
    let forgetter = Rc::downgrade(&manager);
    let _cancel = nodes[0].register_on_key_cancel(Rc::new(move |_: &KeyEvent| {
        if let Some(manager) = forgetter.upgrade() {
            manager.forget_pressed_keys();
        }
    }));
    let (second_cancels, _second) = record_cancels(&nodes[1]);
    let second = count_keys(&nodes[1]);
    let arrow = |state| key(state, Key::Named(NamedKey::ArrowDown), Code::ArrowDown);

    let _ = manager.dispatch_key_event(&arrow(KeyState::Down));
    let _ = nodes[1].request_focus();
    let _ = manager.dispatch_key_event(&arrow(KeyState::Down));
    let _ = manager.dispatch_key_event(&arrow(KeyState::Up));
    assert_eq!(
        second.get(),
        2,
        "the new press and its release reach the focus"
    );
    assert!(
        second_cancels.borrow().is_empty(),
        "the new press was not forgotten"
    );
}

/// The cancelled press's handler fails, and so does a listener hearing the
/// new press show focus: the cancellation's failure is the one that
/// propagates, and the new press is still delivered.
fn a_failing_cancellation_stays_the_first_failure() {
    let (manager, nodes, _attachments) = manager_with_nodes(2);
    let _ = nodes[0].request_focus();
    let _cancel = nodes[0].register_on_key_cancel(Rc::new(|_: &KeyEvent| {
        panic!("cancel failure");
    }));
    let second = count_keys(&nodes[1]);
    let arrow = |state| key(state, Key::Named(NamedKey::ArrowDown), Code::ArrowDown);

    let _ = manager.dispatch_key_event(&arrow(KeyState::Down));
    let _ = nodes[1].request_focus();
    manager.note_pointer_input();
    let _subscription = nodes[1].subscribe(Rc::new(|| panic!("modality failure")));
    let failure = catch_unwind(AssertUnwindSafe(|| {
        manager.dispatch_key_event(&arrow(KeyState::Down))
    }))
    .expect_err("the failures propagate");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("cancel failure")
    );
    assert_eq!(second.get(), 1, "the new press was delivered");
}

/// Tab moves focus by traversal; its repeats keep moving it.
fn traversal_hands_repeats_to_the_new_focus() {
    let (manager, nodes, _attachments) = manager_with_nodes(4);
    let tabber = Rc::downgrade(&manager);
    manager
        .root_scope()
        .as_focus_node()
        .set_on_key_event(Rc::new(move |_| {
            if let Some(manager) = tabber.upgrade() {
                manager.focus_next();
            }
            KeyEventResult::Handled
        }));
    let _ = nodes[0].request_focus();
    let _ = manager.dispatch_key_event(&tab(KeyRepeat::First));
    let _ = manager.dispatch_key_event(&tab(KeyRepeat::AutoRepeat));
    let _ = manager.dispatch_key_event(&tab(KeyRepeat::AutoRepeat));
    assert!(nodes[3].has_primary_focus());
}

/// A key held on a list row keeps repeating when focus moves to another row
/// of the same list.
fn repeats_inside_a_unit_follow_the_unit() {
    let fixture = list_fixture(None);
    let _entry = fixture
        .list
        .register_traversal_entry(Rc::new(|| EntryTarget::Unit));
    let rows: Vec<_> = fixture.rows.iter().map(Rc::downgrade).collect();
    let presses = Rc::new(Cell::new(0));
    let seen = Rc::clone(&presses);
    fixture.list.set_on_key_event(Rc::new(move |_| {
        seen.set(seen.get() + 1);
        if let Some(row) = rows.get(seen.get()).and_then(std::rc::Weak::upgrade) {
            let _ = row.request_focus();
        }
        KeyEventResult::Handled
    }));
    let _ = fixture.rows[0].request_focus();
    let down = key(
        KeyState::Down,
        Key::Named(NamedKey::ArrowDown),
        Code::ArrowDown,
    );
    let _ = fixture.manager.dispatch_key_event(&down);
    let _ = fixture
        .manager
        .dispatch_key_event(&down.with_repeat(KeyRepeat::AutoRepeat));
    assert_eq!(presses.get(), 2);
    assert!(fixture.rows[2].has_primary_focus());
}

fn orphan_releases_reach_only_global_handlers() {
    let (manager, nodes, _attachments) = manager_with_nodes(1);
    let _ = nodes[0].request_focus();
    let node_keys = count_keys(&nodes[0]);
    let global = Rc::new(Cell::new(0));
    let seen = Rc::clone(&global);
    manager.add_global_key_handler(Rc::new(move |_| {
        seen.set(seen.get() + 1);
        KeyEventResult::Ignored
    }));
    let _ = manager.dispatch_key_event(&enter(KeyState::Up, KeyRepeat::First));
    let _ = manager.dispatch_key_event(&enter(KeyState::Down, KeyRepeat::AutoRepeat));
    assert_eq!(node_keys.get(), 0);
    assert_eq!(global.get(), 2);
}

fn input_method_keys_never_reach_the_walk() {
    let (manager, nodes, _attachments) = manager_with_nodes(1);
    let _ = nodes[0].request_focus();
    let node_keys = count_keys(&nodes[0]);
    let composing =
        enter(KeyState::Down, KeyRepeat::First).with_composition(ImeComposition::Active);
    let _ = manager.dispatch_key_event(&composing);
    let _ = manager.dispatch_key_event(&enter(KeyState::Down, KeyRepeat::AutoRepeat));
    let _ = manager.dispatch_key_event(&enter(KeyState::Up, KeyRepeat::First));
    let process = key(KeyState::Down, Key::Named(NamedKey::Process), Code::KeyA);
    let _ = manager.dispatch_key_event(&process);
    let _ = manager.dispatch_key_event(&key(KeyState::Up, Key::character("a"), Code::KeyA));
    assert_eq!(node_keys.get(), 0);
}

fn forgotten_keys_stop_repeating() {
    let (manager, nodes, _attachments) = manager_with_nodes(1);
    let _ = nodes[0].request_focus();
    let node_keys = count_keys(&nodes[0]);
    let (cancelled, _registration) = record_cancels(&nodes[0]);
    let _ = manager.dispatch_key_event(&enter(KeyState::Down, KeyRepeat::First));
    manager.forget_pressed_keys();
    assert_eq!(
        *cancelled.borrow(),
        [Code::Enter],
        "the held key's path is told it ended"
    );
    let _ = manager.dispatch_key_event(&enter(KeyState::Down, KeyRepeat::AutoRepeat));
    let _ = manager.dispatch_key_event(&enter(KeyState::Up, KeyRepeat::First));
    assert_eq!(node_keys.get(), 1);
    assert_eq!(cancelled.borrow().len(), 1, "once");
}

/// A character a host posts without a key (Windows' Alt+numpad entry) reaches
/// the focus as a press, and nothing holds it: no release follows, so losing
/// activation later cancels nothing on its path.
fn a_key_down_no_release_follows_is_held_by_nobody() {
    let (manager, nodes, _attachments) = manager_with_nodes(1);
    let _ = nodes[0].request_focus();
    let node_keys = count_keys(&nodes[0]);
    let (cancelled, _registration) = record_cancels(&nodes[0]);
    let stray = key(KeyState::Down, Key::character("é"), Code::Unidentified)
        .with_release(KeyRelease::Never);
    assert!(manager.dispatch_key_event(&stray).is_handled());
    assert_eq!(node_keys.get(), 1, "the character reaches the focus");
    manager.forget_pressed_keys();
    assert!(
        cancelled.borrow().is_empty(),
        "a key nobody holds is not cancelled"
    );
}

thread_local! {
    /// What `ReenteringWarning` runs at a focus warning, once.
    static AT_FOCUS_WARNING: RefCell<Option<Rc<dyn Fn()>>> = const { RefCell::new(None) };
}

/// A log subscriber that calls back into the focus manager at its first
/// warning, as a subscriber that forwards to the app may.
struct ReenteringWarning;

impl tracing::Subscriber for ReenteringWarning {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        metadata.target().contains("::focus::") && *metadata.level() == tracing::Level::WARN
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
    fn event(&self, _: &tracing::Event<'_>) {
        if let Some(action) = AT_FOCUS_WARNING.with(|slot| slot.borrow_mut().take()) {
            action();
        }
    }
}

/// The release of a code-less key nobody holds is reported, and the report
/// may call back into the manager: the ledger is let go before it.
fn an_orphan_release_is_reported_with_the_ledger_let_go() {
    let (manager, nodes, _attachments) = manager_with_nodes(1);
    let _ = nodes[0].request_focus();
    let node_keys = count_keys(&nodes[0]);
    let reentered = Rc::new(Cell::new(false));
    let seen = Rc::clone(&reentered);
    let reentrant = Rc::clone(&manager);
    AT_FOCUS_WARNING.with(|slot| {
        *slot.borrow_mut() = Some(Rc::new(move || {
            reentrant.forget_pressed_keys();
            seen.set(true);
        }));
    });
    flui_testing::log_capture::disarm_interest_cache();
    let release = key(KeyState::Up, Key::character("é"), Code::Unidentified);
    let result = tracing::subscriber::with_default(ReenteringWarning, || {
        catch_unwind(AssertUnwindSafe(|| manager.dispatch_key_event(&release)))
    });
    AT_FOCUS_WARNING.with(|slot| *slot.borrow_mut() = None);
    assert!(
        !result
            .expect("the report may reenter the manager")
            .is_handled(),
        "an orphan release is no key event"
    );
    assert!(reentered.get(), "the release was reported");
    assert_eq!(node_keys.get(), 0, "the walk does not see it");
}

fn panicking_handler_keeps_its_keys_record() {
    let (manager, nodes, _attachments) = manager_with_nodes(1);
    let _ = nodes[0].request_focus();
    let calls = Rc::new(Cell::new(0));
    let seen = Rc::clone(&calls);
    nodes[0].set_on_key_event(Rc::new(move |_| {
        seen.set(seen.get() + 1);
        assert!(seen.get() > 1, "first press fails");
        KeyEventResult::Handled
    }));
    let failed = catch_unwind(AssertUnwindSafe(|| {
        manager.dispatch_key_event(&enter(KeyState::Down, KeyRepeat::First))
    }));
    assert!(failed.is_err());
    let _ = manager.dispatch_key_event(&enter(KeyState::Down, KeyRepeat::AutoRepeat));
    let _ = manager.dispatch_key_event(&enter(KeyState::Up, KeyRepeat::First));
    assert_eq!(calls.get(), 3);
}

fn focus_is_visible_after_keyboard_and_assistive_moves() {
    let (manager, nodes, _attachments) = manager_with_nodes(2);
    let _ = nodes[0].request_focus();
    assert!(
        !nodes[0].has_visible_focus(),
        "a programmatic focus is not shown"
    );
    assert!(manager.focus_next());
    assert!(nodes[1].has_visible_focus(), "traversal shows focus");
    let _ = nodes[0].request_focus();
    assert!(
        nodes[0].has_visible_focus(),
        "programmatic keeps the modality"
    );
    assert!(!nodes[1].has_visible_focus());

    manager.note_pointer_input();
    assert!(!nodes[0].has_visible_focus(), "a pointer press hides it");
    let alt = key(KeyState::Down, Key::Named(NamedKey::Alt), Code::AltLeft);
    let _ = manager.dispatch_key_event(&alt);
    let _ =
        manager.dispatch_key_event(&key(KeyState::Up, Key::Named(NamedKey::Alt), Code::AltLeft));
    assert!(
        !nodes[0].has_visible_focus(),
        "a modifier alone does not show it"
    );
    let _ = manager.dispatch_key_event(&key(KeyState::Down, Key::character("a"), Code::KeyA));
    assert!(nodes[0].has_visible_focus(), "any other key-down shows it");

    manager.note_pointer_input();
    assert_eq!(
        nodes[1].request_focus_with(FocusCause::Assistive),
        FocusRequestOutcome::Focused
    );
    assert!(
        nodes[1].has_visible_focus(),
        "assistive technology shows it"
    );
}

fn modality_change_notifies_the_focused_node() {
    let (manager, nodes, _attachments) = manager_with_nodes(1);
    assert!(manager.focus_next());
    let notified = Rc::new(Cell::new(0));
    let seen = Rc::clone(&notified);
    let _subscription = nodes[0].subscribe(Rc::new(move || seen.set(seen.get() + 1)));
    manager.note_pointer_input();
    manager.note_pointer_input();
    assert_eq!(notified.get(), 1, "only a change notifies");
    assert!(manager.focus_next(), "Tab wraps onto the only node");
    assert_eq!(notified.get(), 2, "and shows its focus again");
    assert!(nodes[0].has_visible_focus());
}
