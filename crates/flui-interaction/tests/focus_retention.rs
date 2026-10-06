//! A caught callback failure leaves ownership with the callback's owner: the
//! failure path releases its own reference-counted clone, so dropping the owner
//! later still destroys the captures (ADR-0127).

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};

use flui_interaction::__runtime::{CloseMode, close_focus};
use flui_interaction::events::{Key, KeyEvent, KeyState, Modifiers};
use flui_interaction::routing::{
    FocusManager, FocusNode, FocusScopeNode, FocusTraversalPolicy, KeyEventResult,
};

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

/// The same holds for a preserving gesture close: router routes, global
/// handlers and the cursor callback the caller still holds are released with
/// the caller rather than leaked by the closed binding.
fn closing_gestures_leave_shared_callbacks_with_their_caller() {
    use flui_interaction::__runtime::{close_gestures, close_mouse_tracker};
    use flui_interaction::GestureBinding;
    use flui_interaction::PointerId;
    use flui_interaction::events::PointerEvent;

    let (route_capture, route_probe) = capture();
    let (global_capture, global_probe) = capture();
    let (cursor_capture, cursor_probe) = capture();
    let route: Rc<dyn Fn(&PointerEvent)> = Rc::new(move |_| {
        let _ = &route_capture;
    });
    let global: Rc<dyn Fn(&PointerEvent)> = Rc::new(move |_| {
        let _ = &global_capture;
    });
    let cursor: flui_interaction::routing::CursorChangeCallback = Rc::new(move |_, _| {
        let _ = &cursor_capture;
    });
    let binding = GestureBinding::new();
    binding
        .pointer_router()
        .add_route(PointerId::PRIMARY, Rc::clone(&route));
    binding
        .pointer_router()
        .add_global_handler(Rc::clone(&global));
    binding
        .mouse_tracker()
        .set_cursor_change_callback(Rc::clone(&cursor));
    close_gestures(&binding, CloseMode::PreservingFailure);
    close_mouse_tracker(binding.mouse_tracker(), CloseMode::PreservingFailure);
    drop((route, global, cursor));
    assert!(
        route_probe.upgrade().is_none(),
        "the route capture is released"
    );
    assert!(
        global_probe.upgrade().is_none(),
        "the global capture is released"
    );
    assert!(
        cursor_probe.upgrade().is_none(),
        "the cursor capture is released"
    );
    drop(binding);
}

/// A dispatch owner that rejects registrations while its close is preserving
/// releases the shared payload and mouse callbacks its caller still holds.
fn closing_dispatch_leaves_shared_payloads_with_their_caller() {
    use flui_interaction::__runtime::{CloseWindow, close_dispatch, presentation_dispatch};
    use flui_interaction::InteractionLane;
    use flui_interaction::routing::{MouseEnterCallback, MouseRegionCallbacks};

    let (payload_capture, payload_probe) = capture();
    let (enter_capture, enter_probe) = capture();
    let payload: Rc<dyn std::any::Any> = Rc::new(payload_capture);
    let on_enter: MouseEnterCallback = Rc::new(move |_, _| {
        let _ = &enter_capture;
    });
    let lane = InteractionLane::try_new().expect("lane");
    let handle = presentation_dispatch(&lane.dispatch_handle());
    let mut window = CloseWindow::new();
    window.dispatch(&handle);
    window.preserve();
    close_dispatch(&handle, CloseMode::PreservingFailure);
    assert!(handle.register_local_payload(Rc::clone(&payload)).is_err());
    assert!(
        handle
            .register_mouse_region(MouseRegionCallbacks {
                on_enter: Some(Rc::clone(&on_enter)),
                ..MouseRegionCallbacks::default()
            })
            .is_err()
    );
    drop((payload, on_enter));
    assert!(
        payload_probe.upgrade().is_none(),
        "the payload capture is released"
    );
    assert!(
        enter_probe.upgrade().is_none(),
        "the mouse callback capture is released"
    );
    drop(window);
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
            "closing gestures",
            closing_gestures_leave_shared_callbacks_with_their_caller,
        ),
        (
            "closing dispatch",
            closing_dispatch_leaves_shared_payloads_with_their_caller,
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

/// Records its label into a shared log when the node's context is dropped.
struct DropRecorder(&'static str, Rc<RefCell<Vec<&'static str>>>);

impl Drop for DropRecorder {
    fn drop(&mut self) {
        self.1.borrow_mut().push(self.0);
    }
}

#[test]
fn healthy_close_retires_children_before_their_parent() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let manager = FocusManager::new();
    let parent = FocusNode::with_debug_label("parent");
    let first = FocusNode::with_debug_label("first");
    let nested = FocusNode::with_debug_label("nested");
    let second = FocusNode::with_debug_label("second");
    let attachments = [
        manager
            .root_scope()
            .attach_node(&parent)
            .expect("attach parent"),
        parent.attach_node(&first).expect("attach first"),
        first.attach_node(&nested).expect("attach nested"),
        parent.attach_node(&second).expect("attach second"),
    ];
    for (node, label) in [
        (&parent, "parent"),
        (&first, "first"),
        (&nested, "nested"),
        (&second, "second"),
    ] {
        node.register_context(Rc::new(DropRecorder(label, Rc::clone(&log))))
            .relinquish();
    }
    // Within one node: key handler, then listeners, then context.
    let recorder = DropRecorder("nested listener", Rc::clone(&log));
    nested.add_listener(Rc::new(move || {
        let _ = &recorder;
    }));
    let recorder = DropRecorder("nested key handler", Rc::clone(&log));
    nested.set_on_key_event(Rc::new(move |_| {
        let _ = &recorder;
        KeyEventResult::Ignored
    }));
    manager.close();
    assert_eq!(
        *log.borrow(),
        [
            "nested key handler",
            "nested listener",
            "nested",
            "first",
            "second",
            "parent"
        ]
    );
    drop(attachments);
}
