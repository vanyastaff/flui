//! [`Shortcuts`], [`CallbackShortcuts`], the intent/action bridge, Tab
//! traversal and activation keys against a mounted tree. The pure
//! `SingleActivator` matching test stays a unit test in
//! `src/interaction/shortcuts.rs`.

pub(crate) mod intent_tests {

    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_interaction::events::{Key, KeyEvent, KeyState, Modifiers};
    use flui_interaction::routing::FocusNode;
    use flui_widgets::SizedBox;
    use flui_widgets::interaction::{
        Actions, CallbackAction, Focus, Intent, Shortcuts, SingleActivator,
    };

    use crate::common::harness::mount;

    struct SaveIntent;
    impl Intent for SaveIntent {}

    fn ctrl_s() -> KeyEvent {
        KeyEvent {
            state: KeyState::Down,
            key: Key::Character("s".into()),
            modifiers: Modifiers::CONTROL,
            ..KeyEvent::default()
        }
    }

    /// `Shortcuts` end to end (ADR-0023): Ctrl+S bubbles from the focused field,
    /// `Shortcuts` maps it to `SaveIntent`, and the enclosing `Actions` chain
    /// invokes the bound action; the event is consumed.
    ///
    /// Red-check: drop the `resolve` call from `Shortcuts::build`'s handler —
    /// nothing runs and dispatch reports unhandled.
    pub(crate) fn a_shortcut_dispatches_its_intent_through_the_actions_chain() {
        let saves = Arc::new(AtomicUsize::new(0));
        let field = FocusNode::with_debug_label("intent-field");

        let saves_for_action = Arc::clone(&saves);
        let harness = mount(
            Actions::new(
                Shortcuts::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&field)))
                    .shortcut(SingleActivator::character("s").control(), SaveIntent),
            )
            .action(CallbackAction::new(move |_cx, _intent: &SaveIntent| {
                saves_for_action.fetch_add(1, Ordering::SeqCst);
            })),
        );
        let manager = harness.focus_manager();
        field.request_focus();

        assert!(manager.dispatch_key_event(&ctrl_s()), "consumed");
        assert_eq!(saves.load(Ordering::SeqCst), 1, "the action ran");

        // Bare "s" does not match the activator: unhandled, nothing runs.
        assert!(!manager.dispatch_key_event(&KeyEvent {
            modifiers: Modifiers::empty(),
            ..ctrl_s()
        }));
        assert_eq!(saves.load(Ordering::SeqCst), 1);
    }
}

pub(crate) mod tab_tests {
    use std::cell::Cell;
    use std::rc::{Rc, Weak};

    use flui_interaction::events::{Key, KeyEvent, KeyState, Modifiers, NamedKey};
    use flui_interaction::routing::{
        FocusAttachment, FocusDetachOutcome, FocusNode, FocusScopeNode, FocusTraversalPolicy,
        ReadingOrderPolicy,
    };
    use flui_view::ViewExt;
    use flui_view::prelude::*;
    use flui_widgets::interaction::{Focus, FocusScope};
    use flui_widgets::{Positioned, SizedBox, Stack};

    use crate::common::harness::mount;

    fn tab(shift: bool) -> KeyEvent {
        KeyEvent {
            state: KeyState::Down,
            key: Key::Named(NamedKey::Tab),
            modifiers: if shift {
                Modifiers::SHIFT
            } else {
                Modifiers::empty()
            },
            ..KeyEvent::default()
        }
    }

    #[derive(Debug)]
    struct ReplacingPolicy {
        scope: Weak<FocusScopeNode>,
        retired: Rc<Cell<bool>>,
    }

    impl FocusTraversalPolicy for ReplacingPolicy {
        fn sort_descendants(&self, nodes: &[Rc<FocusNode>]) -> Vec<Rc<FocusNode>> {
            self.scope
                .upgrade()
                .expect("the mounted scope remains alive")
                .set_traversal_policy(Rc::new(ReadingOrderPolicy));
            let mut order = ReadingOrderPolicy.sort_descendants(nodes);
            order.reverse();
            order
        }
    }

    impl Drop for ReplacingPolicy {
        fn drop(&mut self) {
            let Some(scope) = self.scope.upgrade() else {
                return;
            };
            let live_nodes = scope.sorted_traversal_order(None).len();
            scope.set_traversal_policy(Rc::new(ReadingOrderPolicy));
            self.retired.set(live_nodes == 3);
        }
    }

    /// **Tab works, end to end** (ADR-0026): a real key event enters
    /// `dispatch_key_event`, bubbles from the focused field (ADR-0023),
    /// matches the `Shortcuts` activator, resolves `NextFocusIntent` through
    /// the enclosing `Actions`, and moves the focus in reading order.
    ///
    /// The traversal actions come from `DefaultFocusTraversal`, which the
    /// harness's `FocusRoot` installs.
    pub(crate) fn tab_and_shift_tab_move_the_focus_through_the_actions_chain() {
        let scope = FocusScopeNode::with_debug_label("tab-scope");
        let left = FocusNode::with_debug_label("left");
        let middle = FocusNode::with_debug_label("middle");
        let right = FocusNode::with_debug_label("right");

        let field = |x: f64, node: &Rc<FocusNode>| {
            Positioned::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(node)))
                .left(x)
                .top(0.0)
                .width(10.0)
                .height(10.0)
                .into_view()
                .boxed()
        };

        let harness = mount(FocusScope::with_external_node(
            Rc::clone(&scope),
            Stack::new(vec![
                field(0.0, &left),
                field(20.0, &middle),
                field(40.0, &right),
            ]),
        ));
        let manager = harness.focus_manager();
        left.request_focus();

        assert!(manager.dispatch_key_event(&tab(false)), "Tab is consumed");
        assert!(
            middle.has_primary_focus(),
            "Tab moved the focus to the next node in reading order"
        );

        assert!(manager.dispatch_key_event(&tab(true)), "Shift+Tab too");
        assert!(left.has_primary_focus(), "and it stepped back");

        // Begin inside the order so the outgoing-policy assertion does not
        // depend on what happens when traversal reaches a scope edge.
        middle.request_focus();
        assert!(middle.has_primary_focus());
        let retired = Rc::new(Cell::new(false));
        scope.set_traversal_policy(Rc::new(ReplacingPolicy {
            scope: Rc::downgrade(&scope),
            retired: Rc::clone(&retired),
        }));
        assert!(manager.dispatch_key_event(&tab(false)));
        assert!(
            left.has_primary_focus(),
            "the current key uses the outgoing reverse reading-order policy"
        );
        assert!(
            retired.get(),
            "policy destruction can reenter the same scope"
        );
        assert!(manager.dispatch_key_event(&tab(false)));
        assert!(
            middle.has_primary_focus(),
            "the next key uses the replacement reading-order policy"
        );
    }

    #[derive(Debug)]
    struct PolicyCapture {
        drops: Rc<Cell<usize>>,
        panic: bool,
    }

    impl Drop for PolicyCapture {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
            assert!(!self.panic, "first policy capture retirement");
        }
    }

    #[derive(Debug)]
    struct UnwindingPolicy {
        scope: Weak<FocusScopeNode>,
        detached_candidate: FocusAttachment,
        panic_sort: bool,
        capture: PolicyCapture,
    }

    impl FocusTraversalPolicy for UnwindingPolicy {
        fn sort_descendants(&self, nodes: &[Rc<FocusNode>]) -> Vec<Rc<FocusNode>> {
            let _ = &self.capture;
            self.scope
                .upgrade()
                .expect("the mounted scope remains alive")
                .set_traversal_policy(Rc::new(ReadingOrderPolicy));
            assert_eq!(
                self.detached_candidate.detach(),
                FocusDetachOutcome::Detached
            );
            assert!(!self.panic_sort, "first traversal sort failure");
            nodes.iter().rev().cloned().collect()
        }
    }

    struct CandidateCapture(Rc<Cell<usize>>);

    impl Drop for CandidateCapture {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
            panic!("detached candidate aggregate capture");
        }
    }

    pub(crate) fn tab_traversal_preserves_failure_before_policy_and_candidate_retirement() {
        use std::io::Read;
        use std::panic::{AssertUnwindSafe, catch_unwind};

        const CHILD: &str = "FLUI_TAB_POLICY_UNWIND_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let mut child =
                std::process::Command::new(std::env::current_exe().expect("test binary"))
                    .args([
                        "--exact",
                        "contracts::focus_actions_and_shortcuts",
                        "--nocapture",
                    ])
                    .env(CHILD, "1")
                    .env("RUST_BACKTRACE", "0")
                    .env("RUST_LIB_BACKTRACE", "0")
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .expect("spawn bounded policy retirement test");
            let mut stdout = child.stdout.take().expect("child stdout");
            let mut stderr = child.stderr.take().expect("child stderr");
            let stdout_reader = std::thread::spawn(move || {
                let mut bytes = Vec::new();
                stdout.read_to_end(&mut bytes).expect("read child stdout");
                bytes
            });
            let stderr_reader = std::thread::spawn(move || {
                let mut bytes = Vec::new();
                stderr.read_to_end(&mut bytes).expect("read child stderr");
                bytes
            });
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            loop {
                if let Some(status) = child.try_wait().expect("poll policy retirement test") {
                    let stdout = stdout_reader.join().expect("child stdout reader");
                    let stderr = stderr_reader.join().expect("child stderr reader");
                    assert!(
                        status.success(),
                        "policy retirement subprocess failed: {status}\n{}",
                        String::from_utf8_lossy(&stderr)
                    );
                    assert!(
                        String::from_utf8_lossy(&stdout)
                            .contains("Tab policy retirement child completed"),
                        "the child must execute the policy cases, not match zero tests"
                    );
                    return;
                }
                if std::time::Instant::now() >= deadline {
                    child.kill().expect("stop timed-out policy retirement test");
                    child.wait().expect("reap policy retirement test");
                    stdout_reader.join().expect("child stdout reader");
                    stderr_reader.join().expect("child stderr reader");
                    panic!("policy retirement subprocess timed out");
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }

        for (panic_sort, panic_capture, expected, expected_policy_drops) in [
            (true, true, "first traversal sort failure", 0),
            (true, false, "first traversal sort failure", 0),
            (false, true, "first policy capture retirement", 1),
        ] {
            let scope = FocusScopeNode::with_debug_label("policy-retirement-scope");
            let left = FocusNode::with_debug_label("left");
            let middle = FocusNode::with_debug_label("middle");
            let right = FocusNode::with_debug_label("right");
            let field = |x: f64, node: &Rc<FocusNode>| {
                Positioned::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(node)))
                    .left(x)
                    .top(0.0)
                    .width(10.0)
                    .height(10.0)
                    .into_view()
                    .boxed()
            };
            let harness = mount(FocusScope::with_external_node(
                Rc::clone(&scope),
                Stack::new(vec![
                    field(0.0, &left),
                    field(20.0, &middle),
                    field(40.0, &right),
                ]),
            ));
            let manager = harness.focus_manager();
            left.request_focus();

            let policy_drops = Rc::new(Cell::new(0));
            let candidate_drops = Rc::new(Cell::new(0));
            let candidate = FocusNode::with_debug_label("detached-policy-candidate");
            candidate
                .register_context(Rc::new((
                    CandidateCapture(Rc::clone(&candidate_drops)),
                    CandidateCapture(Rc::clone(&candidate_drops)),
                )))
                .relinquish();
            let detached_candidate = scope.attach_node(&candidate).expect("attach candidate");
            drop(candidate);
            scope.set_traversal_policy(Rc::new(UnwindingPolicy {
                scope: Rc::downgrade(&scope),
                detached_candidate,
                panic_sort,
                capture: PolicyCapture {
                    drops: Rc::clone(&policy_drops),
                    panic: panic_capture,
                },
            }));

            let outcome =
                catch_unwind(AssertUnwindSafe(|| manager.dispatch_key_event(&tab(false))));
            let payload = outcome.expect_err("the first traversal failure propagates");
            assert_eq!(
                flui_foundation::panic::payload_text(payload.as_ref()),
                Some(expected)
            );
            flui_foundation::panic::retain_opaque_payload(payload);
            assert_eq!(policy_drops.get(), expected_policy_drops);
            assert_eq!(
                candidate_drops.get(),
                0,
                "detached candidate ownership stays out of the failed unwind"
            );
            assert!(
                left.has_primary_focus(),
                "the failed sort published no focus step"
            );
            assert!(manager.dispatch_key_event(&tab(false)));
            assert!(
                middle.has_primary_focus(),
                "the replacement policy serves the next key"
            );
            assert!(manager.dispatch_key_event(&tab(true)));
            assert!(left.has_primary_focus());
        }
        println!("Tab policy retirement child completed");
    }
}

pub(crate) mod activation_tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use flui_interaction::events::{Key, KeyEvent, KeyState, Modifiers, NamedKey};
    use flui_interaction::routing::FocusNode;
    use flui_widgets::SizedBox;
    use flui_widgets::interaction::{Actions, ActivateIntent, CallbackAction, Focus};

    use crate::common::harness::mount;

    fn key_down(key: Key) -> KeyEvent {
        KeyEvent {
            state: KeyState::Down,
            key,
            modifiers: Modifiers::empty(),
            ..KeyEvent::default()
        }
    }

    /// Enter, Space and Select activate the focused control through the
    /// root bindings — `WidgetsApp`'s `_defaultShortcuts` (`app.dart:1265-1269`,
    /// tag `3.44.0`) — and an activation key no control claims keeps bubbling.
    ///
    /// Red-check: drop the three `ActivateIntent` bindings from
    /// `DefaultFocusTraversal` — every dispatch is ignored.
    pub(crate) fn enter_space_and_select_activate_the_focused_control() {
        let runs = Rc::new(Cell::new(0));
        let button = FocusNode::with_debug_label("button");
        let counted = Rc::clone(&runs);
        let harness = mount(
            Actions::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&button)))
                .action(CallbackAction::new(move |_cx, _: &ActivateIntent| {
                    counted.set(counted.get() + 1);
                })),
        );
        let manager = harness.focus_manager();
        button.request_focus();

        for key in [
            Key::Named(NamedKey::Enter),
            Key::Character(" ".into()),
            Key::Named(NamedKey::Select),
        ] {
            assert!(
                manager.dispatch_key_event(&key_down(key.clone())),
                "{key:?} is consumed"
            );
        }
        assert_eq!(runs.get(), 3, "each key activated the control once");
    }
}

/// Shortcut callbacks and actions receive the key event's `EventCx`
/// (ADR-0086): they write signals directly, and the signal's reader
/// rebuilds on the next frame.
pub(crate) mod event_cx_tests {
    use std::rc::Rc;

    use flui_interaction::events::{Key, KeyEvent, KeyState, Modifiers};
    use flui_interaction::routing::FocusNode;
    use flui_view::prelude::*;
    use flui_widgets::SizedBox;
    use flui_widgets::interaction::{
        Actions, CallbackAction, CallbackShortcuts, Focus, Intent, Shortcuts, SingleActivator,
    };

    use crate::common::harness::{Harness, mount};
    use crate::common::{ProbeSignals, SignalProbe};

    struct SaveIntent;
    impl Intent for SaveIntent {}

    fn ctrl_s() -> KeyEvent {
        KeyEvent {
            state: KeyState::Down,
            key: Key::Character("s".into()),
            modifiers: Modifiers::CONTROL,
            ..KeyEvent::default()
        }
    }

    fn field(node: &Rc<FocusNode>) -> Focus {
        Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(node))
    }

    fn press_ctrl_s(harness: &Harness, node: &Rc<FocusNode>) -> bool {
        node.request_focus();
        harness.focus_manager().dispatch_key_event(&ctrl_s())
    }

    pub(crate) fn callback_shortcut_writes_a_signal_and_rebuilds_its_reader() {
        let node = FocusNode::with_debug_label("shortcut-write-field");
        let probe_node = Rc::clone(&node);
        let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
            CallbackShortcuts::new(field(&probe_node))
                .binding(SingleActivator::character("s").control(), move |cx| {
                    count.update(cx, |n| *n += 1)
                })
        });
        let mut harness = mount(probe.view());

        assert!(
            press_ctrl_s(&harness, &node),
            "the binding consumed the key"
        );
        assert_eq!(probe.value(), Ok(1), "the write landed at dispatch");
        harness.tick();
        assert_eq!(probe.reads(), [0, 1], "and the reader rebuilt once");
    }

    pub(crate) fn a_refused_write_in_a_callback_shortcut_is_reported_not_panicked() {
        let node = FocusNode::with_debug_label("shortcut-refused-field");
        let probe_node = Rc::clone(&node);
        let probe = SignalProbe::new(move |ProbeSignals { released, .. }| {
            CallbackShortcuts::new(field(&probe_node))
                .binding(SingleActivator::character("s").control(), move |cx| {
                    released.set(cx, 1)
                })
        });
        let mut harness = mount(probe.view());

        let (consumed, log) = flui_testing::log_capture::capture(|| press_ctrl_s(&harness, &node));

        assert!(consumed, "the binding still fired");
        assert!(
            log.contains("an event callback's signal write was refused"),
            "the refusal is logged at the dispatch boundary: {log}"
        );
        harness.tick();
        assert_eq!(probe.value(), Ok(0), "other state is intact");
    }

    pub(crate) fn a_shortcut_action_writes_through_the_key_events_cx() {
        let node = FocusNode::with_debug_label("action-write-field");
        let probe_node = Rc::clone(&node);
        let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
            Actions::new(
                Shortcuts::new(field(&probe_node))
                    .shortcut(SingleActivator::character("s").control(), SaveIntent),
            )
            .action(CallbackAction::new(move |cx, _: &SaveIntent| {
                count.update(cx, |n| *n += 1)
            }))
        });
        let mut harness = mount(probe.view());

        assert!(press_ctrl_s(&harness, &node), "the action consumed the key");
        assert_eq!(probe.value(), Ok(1));
        harness.tick();
        assert_eq!(probe.reads(), [0, 1], "the reader rebuilt once");
    }

    pub(crate) fn a_refused_write_in_a_shortcut_action_is_reported_not_panicked() {
        let node = FocusNode::with_debug_label("action-refused-field");
        let probe_node = Rc::clone(&node);
        let probe = SignalProbe::new(move |ProbeSignals { released, .. }| {
            Actions::new(
                Shortcuts::new(field(&probe_node))
                    .shortcut(SingleActivator::character("s").control(), SaveIntent),
            )
            .action(CallbackAction::new(move |cx, _: &SaveIntent| {
                released.set(cx, 1)
            }))
        });
        let mut harness = mount(probe.view());

        let (consumed, log) = flui_testing::log_capture::capture(|| press_ctrl_s(&harness, &node));

        assert!(consumed, "the action still counts as performed");
        assert!(
            log.contains("an event callback's signal write was refused"),
            "the refusal is logged at the dispatch boundary: {log}"
        );
        harness.tick();
        assert_eq!(probe.value(), Ok(0), "other state is intact");
    }

    /// A closure bound with `let` before it reaches `CallbackAction::new`
    /// names its borrowed intent through `callback_ref`.
    pub(crate) fn a_let_bound_action_closure_compiles_through_callback_ref() {
        let node = FocusNode::with_debug_label("action-let-bound-field");
        let probe_node = Rc::clone(&node);
        let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
            let save = callback_ref(move |cx, _intent: &SaveIntent| count.set(cx, 9));
            Actions::new(
                Shortcuts::new(field(&probe_node))
                    .shortcut(SingleActivator::character("s").control(), SaveIntent),
            )
            .action(CallbackAction::new(save))
        });
        let harness = mount(probe.view());

        assert!(press_ctrl_s(&harness, &node));
        assert_eq!(probe.value(), Ok(9));
    }
}
