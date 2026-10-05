//! A caught callback failure leaves ownership with the callback's owner: the
//! failure path releases its own reference-counted clone, so dropping the owner
//! later still destroys the captures (ADR-0127).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};

use flui_interaction::__runtime::{CloseMode, close_focus};
use flui_interaction::events::{Key, KeyEvent, KeyState, Modifiers};
use flui_interaction::routing::{FocusManager, FocusNode, FocusScopeNode, FocusTraversalPolicy};

fn key_event() -> KeyEvent {
    KeyEvent {
        state: KeyState::Down,
        key: Key::Character("a".into()),
        modifiers: Modifiers::default(),
        ..KeyEvent::default()
    }
}

/// A capture the callback owns, and a weak probe that outlives it.
fn capture() -> (Rc<()>, Weak<()>) {
    let capture = Rc::new(());
    let probe = Rc::downgrade(&capture);
    (capture, probe)
}

fn expect_failure(run: impl FnOnce()) {
    let payload = catch_unwind(AssertUnwindSafe(run)).expect_err("the callback failure propagates");
    flui_foundation::panic::retain_opaque_payload(payload);
}

fn focus_listener_capture_dies_with_its_manager() {
    let (capture, probe) = capture();
    let manager = FocusManager::new();
    let node = FocusNode::with_debug_label("focused");
    let attachment = manager.root_scope().attach_node(&node).expect("attach");
    manager.add_listener(Rc::new(move |_, new| {
        let _ = &capture;
        // Only the gain fails; teardown's focus loss is healthy.
        assert!(new.is_none(), "focus listener");
    }));
    expect_failure(|| {
        node.request_focus();
    });
    drop((attachment, node, manager));
    assert!(
        probe.upgrade().is_none(),
        "the listener capture is released"
    );
}

fn global_key_handler_capture_dies_with_its_manager() {
    let (capture, probe) = capture();
    let manager = FocusManager::new();
    manager.add_global_key_handler(Rc::new(move |_| {
        let _ = &capture;
        panic!("global key handler");
    }));
    expect_failure(|| {
        manager.dispatch_key_event(&key_event());
    });
    drop(manager);
    assert!(probe.upgrade().is_none(), "the handler capture is released");
}

fn node_key_handler_capture_dies_with_its_node() {
    let (capture, probe) = capture();
    let node = FocusNode::with_debug_label("handler");
    node.set_on_key_event(Rc::new(move |_| {
        let _ = &capture;
        panic!("node key handler");
    }));
    expect_failure(|| {
        node.handle_key_event(&key_event());
    });
    drop(node);
    assert!(probe.upgrade().is_none(), "the handler capture is released");
}

/// A close that already owes a failure retains only last owners: a handler
/// its caller still holds, whether withdrawn by the close or rejected by the
/// closed manager, is released with the caller.
fn closing_manager_leaves_shared_callbacks_with_their_caller() {
    let (capture, probe) = capture();
    let handler: Rc<dyn Fn(&KeyEvent) -> bool> = Rc::new(move |_| {
        let _ = &capture;
        false
    });
    let manager = FocusManager::new();
    manager.add_global_key_handler(Rc::clone(&handler));
    close_focus(&manager, CloseMode::PreservingFailure);
    manager.add_global_key_handler(Rc::clone(&handler));
    drop(handler);
    assert!(probe.upgrade().is_none(), "the handler capture is released");
    drop(manager);
}

#[derive(Debug)]
struct PanickingPolicy(#[expect(dead_code, reason = "held for its lifetime")] Rc<()>);

impl FocusTraversalPolicy for PanickingPolicy {
    fn sort_descendants(&self, _: &[Rc<FocusNode>]) -> Vec<Rc<FocusNode>> {
        panic!("traversal policy");
    }
}

fn traversal_policy_and_candidates_die_with_their_scope() {
    let (capture, probe) = capture();
    let scope = FocusScopeNode::with_debug_label("scope");
    let node = FocusNode::with_debug_label("candidate");
    let candidate = Rc::downgrade(&node);
    let attachment = scope.attach_node(&node).expect("attach");
    scope.set_traversal_policy(Rc::new(PanickingPolicy(capture)));
    expect_failure(|| {
        scope.sorted_traversal_order(None);
    });
    drop((attachment, node, scope));
    assert!(probe.upgrade().is_none(), "the policy capture is released");
    assert!(candidate.upgrade().is_none(), "the candidate is released");
}

#[test]
fn caught_callback_failures_leave_captures_with_their_owner() {
    let cases: &[(&str, fn())] = &[
        (
            "focus listener",
            focus_listener_capture_dies_with_its_manager,
        ),
        (
            "global key handler",
            global_key_handler_capture_dies_with_its_manager,
        ),
        (
            "node key handler",
            node_key_handler_capture_dies_with_its_node,
        ),
        (
            "closing manager",
            closing_manager_leaves_shared_callbacks_with_their_caller,
        ),
        (
            "traversal policy",
            traversal_policy_and_candidates_die_with_their_scope,
        ),
    ];
    let mut failed = Vec::new();
    for &(name, case) in cases {
        if let Err(payload) = catch_unwind(case) {
            failed.push(name);
            flui_foundation::panic::retain_opaque_payload(payload);
        }
    }
    assert!(failed.is_empty(), "failed cases: {failed:?}");
}
