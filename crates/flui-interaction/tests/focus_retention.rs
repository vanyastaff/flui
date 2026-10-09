//! A caught callback failure leaves ownership with the callback's owner: the
//! failure path releases its own reference-counted clone, so dropping the owner
//! later still destroys the captures (ADR-0127).

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};

use flui_interaction::__runtime::{CloseMode, close_focus};
use flui_interaction::routing::{
    FocusManager, FocusNode, FocusScopeNode, FocusTraversalPolicy, KeyEventResult,
};
use flui_platform_api::{
    EventTime,
    keyboard::{Code, Key, KeyEvent, KeyState},
};

fn geometric_focus_navigation_pins_ranking_and_admission() {
    use flui_foundation::geometry::Rect;
    use flui_interaction::{FocusDirection, TraversalEdgeBehavior};
    for (name, source, rectangles, expected) in [
        (
            "beam before diagonal",
            Rect::new(0.0, 0.0, 10.0, 10.0),
            vec![
                Rect::new(11.0, 20.0, 21.0, 30.0),
                Rect::new(90.0, 0.0, 100.0, 10.0),
            ],
            Some(1),
        ),
        (
            "primary gap before secondary distance",
            Rect::new(0.0, 0.0, 10.0, 10.0),
            vec![
                Rect::new(20.0, 5.0, 30.0, 15.0),
                Rect::new(30.0, 0.0, 40.0, 10.0),
            ],
            Some(0),
        ),
        (
            "secondary distance breaks equal primary gap",
            Rect::new(0.0, 0.0, 10.0, 10.0),
            vec![
                Rect::new(20.0, 7.0, 30.0, 17.0),
                Rect::new(20.0, 1.0, 30.0, 11.0),
            ],
            Some(1),
        ),
        (
            "stable tree order breaks geometry ties",
            Rect::new(0.0, 0.0, 10.0, 10.0),
            vec![
                Rect::new(20.0, 0.0, 30.0, 10.0),
                Rect::new(20.0, 0.0, 30.0, 10.0),
            ],
            Some(0),
        ),
        (
            "strict center half-plane rejects behind and coincident",
            Rect::new(0.0, 0.0, 10.0, 10.0),
            vec![
                Rect::new(-20.0, 0.0, -10.0, 10.0),
                Rect::new(0.0, 20.0, 10.0, 30.0),
            ],
            None,
        ),
        (
            "missing zero and invalid rectangles are untargetable",
            Rect::new(0.0, 0.0, 10.0, 10.0),
            vec![
                Rect::ZERO,
                Rect::new(20.0, 0.0, f64::INFINITY, 10.0),
                Rect::new(f64::NAN, 0.0, 30.0, 10.0),
                Rect::new(20.0, 0.0, 20.0, 10.0),
                Rect::new(-1.0e308, 0.0, f64::MAX, 10.0),
            ],
            None,
        ),
        (
            "absent source geometry cannot navigate",
            Rect::ZERO,
            vec![Rect::new(20.0, 0.0, 30.0, 10.0)],
            None,
        ),
        (
            "fractional coordinates preserve ordering",
            Rect::new(0.25, 0.25, 0.75, 0.75),
            vec![
                Rect::new(0.9, 0.25, 1.4, 0.75),
                Rect::new(0.8, 0.25, 1.3, 0.75),
            ],
            Some(1),
        ),
        (
            "overflowing finite gaps remain ordered",
            Rect::new(-1.7e308, 0.0, -1.6e308, 10.0),
            vec![
                Rect::new(1.6e308, 0.0, 1.7e308, 10.0),
                Rect::new(1.0e308, 0.0, 1.1e308, 10.0),
            ],
            Some(1),
        ),
    ] {
        let manager = FocusManager::new();
        manager
            .root_scope()
            .set_traversal_edge_behavior(TraversalEdgeBehavior::Stop);
        let source_node = FocusNode::new();
        source_node.set_rect(source);
        let mut attachments = vec![
            manager
                .root_scope()
                .attach_node(&source_node)
                .expect("source"),
        ];
        let nodes: Vec<_> = rectangles
            .into_iter()
            .map(|rect| {
                let node = FocusNode::new();
                node.set_rect(rect);
                attachments.push(manager.root_scope().attach_node(&node).expect("candidate"));
                node
            })
            .collect();
        let _ = source_node.request_focus();
        assert_eq!(
            manager.focus_in_direction(FocusDirection::Right),
            expected.is_some(),
            "{name}"
        );
        let target = expected.map_or(&source_node, |index| &nodes[index]);
        assert!(target.has_primary_focus(), "{name}");
        drop(attachments);
    }
}

type CommitDiagnosticAction = (bool, Rc<dyn Fn()>);
thread_local! {
    static COMMIT_DIAGNOSTIC: RefCell<Option<CommitDiagnosticAction>> = const { RefCell::new(None) };
}

struct CommitFocusDiagnostic;

impl tracing::Subscriber for CommitFocusDiagnostic {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        metadata.target().ends_with("::focus") && *metadata.level() == tracing::Level::TRACE
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct Message(bool);
        impl tracing::field::Visit for Message {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 = format!("{value:?}").contains("focus changed");
                }
            }
        }
        let mut message = Message(false);
        event.record(&mut message);
        if message.0
            && let Some((fails, action)) = COMMIT_DIAGNOSTIC.with(|slot| slot.borrow_mut().take())
        {
            action();
            assert!(!fails, "focus commit diagnostic failure");
        }
    }
}

fn commit_diagnostics_preserve_focus_round_and_queued_requests() {
    for (diagnostic_fails, observer_fails, reenter) in [
        (true, false, false),
        (true, true, true),
        (false, true, true),
        (false, false, true),
    ] {
        let manager = FocusManager::new();
        let first = FocusNode::new();
        let next = FocusNode::new();
        let attachments = [
            manager.root_scope().attach_node(&first).expect("first"),
            manager.root_scope().attach_node(&next).expect("next"),
        ];
        let log = Rc::new(RefCell::new(Vec::new()));
        let callback_log = Rc::clone(&log);
        let first_probe = Rc::downgrade(&first);
        let fail = Rc::new(Cell::new(observer_fails));
        let callback_fail = Rc::clone(&fail);
        let subscription = first.subscribe(Rc::new(move || {
            let focused = first_probe.upgrade().expect("first").has_primary_focus();
            callback_log.borrow_mut().push(("node", focused));
            assert!(
                !(focused && callback_fail.get()),
                "focus observer second failure"
            );
        }));
        let manager_log = Rc::clone(&log);
        let first_id = first.id();
        manager.add_listener(Rc::new(move |_, new| {
            manager_log
                .borrow_mut()
                .push(("manager", new.is_some_and(|node| node.id() == first_id)));
        }));
        let next_probe = Rc::downgrade(&next);
        COMMIT_DIAGNOSTIC.with(|slot| {
            *slot.borrow_mut() = Some((
                diagnostic_fails,
                Rc::new(move || {
                    if reenter {
                        let _ = next_probe.upgrade().expect("next").request_focus();
                    }
                }),
            ));
        });
        let result = tracing::subscriber::with_default(CommitFocusDiagnostic, || {
            catch_unwind(AssertUnwindSafe(|| first.request_focus()))
        });
        COMMIT_DIAGNOSTIC.with(|slot| *slot.borrow_mut() = None);
        if diagnostic_fails || observer_fails {
            let payload = result.expect_err("failure resumes after committed rounds");
            assert_eq!(
                payload.downcast_ref::<&str>(),
                Some(&if diagnostic_fails {
                    "focus commit diagnostic failure"
                } else {
                    "focus observer second failure"
                })
            );
        } else {
            assert!(result.is_ok());
        }
        let expected = if reenter {
            vec![
                ("node", true),
                ("manager", true),
                ("node", false),
                ("manager", false),
            ]
        } else {
            vec![("node", true), ("manager", true)]
        };
        assert_eq!(
            *log.borrow(),
            expected,
            "diagnostic reentry must queue after the committed first round"
        );
        assert!(if reenter {
            next.has_primary_focus()
        } else {
            first.has_primary_focus()
        });
        fail.set(false);
        manager.unfocus();
        log.borrow_mut().clear();
        let _ = first.request_focus();
        assert_eq!(*log.borrow(), [("node", true), ("manager", true)]);
        drop(subscription);
        drop(attachments);
    }
}

fn directional_edges_match_linear_scope_outcomes() {
    use flui_foundation::geometry::Rect;
    use flui_interaction::{FocusDirection, TraversalEdgeBehavior};
    for direction in [
        FocusDirection::Up,
        FocusDirection::Down,
        FocusDirection::Left,
        FocusDirection::Right,
    ] {
        for edge in [
            TraversalEdgeBehavior::Stop,
            TraversalEdgeBehavior::ClosedLoop,
            TraversalEdgeBehavior::LeaveView,
            TraversalEdgeBehavior::ParentScope,
        ] {
            let manager = FocusManager::new();
            manager.root_scope().set_traversal_edge_behavior(edge);
            let nodes = [FocusNode::new(), FocusNode::new()];
            nodes[0].set_rect(Rect::new(0.0, 0.0, 10.0, 10.0));
            let rect = match direction {
                FocusDirection::Up => Rect::new(0.0, 20.0, 10.0, 30.0),
                FocusDirection::Down => Rect::new(0.0, -30.0, 10.0, -20.0),
                FocusDirection::Left => Rect::new(20.0, 0.0, 30.0, 10.0),
                FocusDirection::Right => Rect::new(-30.0, 0.0, -20.0, 10.0),
            };
            nodes[1].set_rect(rect);
            let _attachments = [
                manager.root_scope().attach_node(&nodes[0]).expect("source"),
                manager
                    .root_scope()
                    .attach_node(&nodes[1])
                    .expect("opposite"),
            ];
            let _ = nodes[0].request_focus();
            let moved = manager.focus_in_direction(direction);
            match edge {
                TraversalEdgeBehavior::Stop => {
                    assert!(!moved);
                    assert!(nodes[0].has_primary_focus());
                }
                TraversalEdgeBehavior::LeaveView => {
                    assert!(!moved);
                    assert!(manager.primary_focus().is_none());
                }
                TraversalEdgeBehavior::ClosedLoop | TraversalEdgeBehavior::ParentScope => {
                    assert!(moved);
                    assert!(nodes[1].has_primary_focus());
                }
            }
        }
    }
}

fn weak_traversal_links_revalidate_groups_and_registration_generations() {
    use flui_foundation::geometry::Rect;
    use flui_interaction::{FocusTraversalOverrides, ReadingOrderPolicy, TraversalEdgeBehavior};
    use flui_painting::typography::TextDirection;
    let manager = FocusManager::new();
    let group = FocusNode::new();
    group.set_can_request_focus(false);
    group.set_skip_traversal(true);
    let _group_policy = group.register_traversal_group(
        Rc::new(ReadingOrderPolicy),
        TextDirection::Ltr,
        TraversalEdgeBehavior::ClosedLoop,
    );
    let nodes = [FocusNode::new(), FocusNode::new(), FocusNode::new()];
    for (left, node) in [0.0, 20.0, 40.0].into_iter().zip(&nodes) {
        node.set_rect(Rect::new(left, 0.0, left + 10.0, 10.0));
    }
    let _attachments = [
        manager.root_scope().attach_node(&group).expect("group"),
        group.attach_node(&nodes[0]).expect("first"),
        group.attach_node(&nodes[1]).expect("second"),
        manager
            .root_scope()
            .attach_node(&nodes[2])
            .expect("outside"),
    ];
    let first = nodes[0]
        .register_traversal_overrides(FocusTraversalOverrides::default().with_next(&nodes[2]));
    let _ = nodes[0].request_focus();
    assert!(manager.focus_next());
    assert!(
        nodes[1].has_primary_focus(),
        "same-owner links cannot jump across policy groups"
    );
    let current = nodes[0]
        .register_traversal_overrides(FocusTraversalOverrides::default().with_next(&nodes[1]));
    drop(first);
    let previous = nodes[1]
        .register_traversal_overrides(FocusTraversalOverrides::default().with_previous(&nodes[0]));
    let _ = nodes[1].request_focus();
    assert!(manager.focus_previous());
    assert!(nodes[0].has_primary_focus());
    assert!(
        current.is_current(),
        "old cleanup cannot erase a newer override generation"
    );
    drop(previous);
    let detached = FocusNode::new();
    let probe = Rc::downgrade(&detached);
    let attachment = group.attach_node(&detached).expect("temporary target");
    let stale = nodes[0]
        .register_traversal_overrides(FocusTraversalOverrides::default().with_next(&detached));
    let _ = attachment.detach();
    drop(detached);
    assert!(
        probe.upgrade().is_none(),
        "explicit traversal links retain no target ownership"
    );
    assert!(manager.focus_next());
    assert!(
        nodes[1].has_primary_focus(),
        "a released target falls back to the ordinary group policy"
    );
    drop((stale, current));
}

fn directional_provider_failure_preserves_first_failure_and_recovery() {
    use flui_foundation::geometry::Rect;
    use flui_interaction::{FocusDirection, TraversalEdgeBehavior};
    struct Capture {
        owner: Weak<FocusNode>,
        drops: Rc<Cell<usize>>,
        fail: bool,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
            if let Some(node) = self.owner.upgrade() {
                node.set_rect(Rect::new(0.0, 0.0, 10.0, 10.0));
            }
            assert!(!self.fail, "competing provider retirement");
        }
    }
    let navigations: [fn(&FocusManager) -> bool; 2] = [FocusManager::focus_next, |manager| {
        manager.focus_in_direction(FocusDirection::Right)
    }];
    for navigate in navigations {
        for (provider_fails, destructor_fails) in [(true, false), (true, true), (false, true)] {
            let manager = FocusManager::new();
            manager
                .root_scope()
                .set_traversal_edge_behavior(TraversalEdgeBehavior::Stop);
            let nodes = [FocusNode::new(), FocusNode::new()];
            nodes[0].set_rect(Rect::new(0.0, 0.0, 10.0, 10.0));
            nodes[1].set_rect(Rect::new(20.0, 0.0, 30.0, 10.0));
            let _attachments = [
                manager.root_scope().attach_node(&nodes[0]).expect("source"),
                manager.root_scope().attach_node(&nodes[1]).expect("target"),
            ];
            let drops = Rc::new(Cell::new(0));
            let captured = Capture {
                owner: Rc::downgrade(&nodes[0]),
                drops: Rc::clone(&drops),
                fail: destructor_fails,
            };
            let probe = Rc::downgrade(&nodes[0]);
            nodes[0].set_rect_provider(Rc::new(move || {
                let _ = &captured;
                probe.upgrade().expect("live source").clear_rect_provider();
                assert!(!provider_fails, "first provider failure");
                Some(Rect::new(0.0, 0.0, 10.0, 10.0))
            }));
            let _ = nodes[0].request_focus();
            let payload = catch_unwind(AssertUnwindSafe(|| navigate(&manager)))
                .expect_err("failure propagates");
            assert_eq!(
                flui_foundation::panic::payload_text(payload.as_ref()),
                Some(if provider_fails {
                    "first provider failure"
                } else {
                    "competing provider retirement"
                })
            );
            flui_foundation::panic::retain_opaque_payload(payload);
            assert_eq!(drops.get(), usize::from(!provider_fails));
            assert!(
                nodes[0].has_primary_focus(),
                "geometry failure cannot publish a focus change"
            );
            assert!(navigate(&manager));
            assert!(
                nodes[1].has_primary_focus(),
                "the cleared provider leaves the healthy fallback geometry usable"
            );
        }
    }
}

fn group_policy_replacement_preserves_failure_and_future_traversal() {
    use flui_foundation::geometry::Rect;
    use flui_interaction::{FocusNodeRegistration, ReadingOrderPolicy, TraversalEdgeBehavior};
    use flui_painting::typography::TextDirection;
    #[derive(Debug)]
    struct ReplacingPolicy {
        group: Weak<FocusNode>,
        replacement: Rc<RefCell<Option<FocusNodeRegistration>>>,
        drops: Rc<Cell<usize>>,
        sorting_fails: bool,
        retirement_fails: bool,
    }
    impl FocusTraversalPolicy for ReplacingPolicy {
        fn order(&self, nodes: &mut [Rc<FocusNode>], direction: TextDirection) {
            let group = self.group.upgrade().expect("live group");
            *self.replacement.borrow_mut() = Some(group.register_traversal_group(
                Rc::new(ReadingOrderPolicy),
                direction,
                TraversalEdgeBehavior::Stop,
            ));
            assert!(!self.sorting_fails, "first group sorting failure");
            ReadingOrderPolicy.order(nodes, direction);
        }
    }
    impl Drop for ReplacingPolicy {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
            if let Some(group) = self.group.upgrade() {
                assert!(
                    group.is_attached(),
                    "policy retirement occurs outside the group borrow"
                );
            }
            assert!(!self.retirement_fails, "competing group retirement failure");
        }
    }
    for (sorting_fails, retirement_fails) in [(true, false), (true, true), (false, true)] {
        let manager = FocusManager::new();
        let group = FocusNode::new();
        group.set_can_request_focus(false);
        group.set_skip_traversal(true);
        let nodes = [FocusNode::new(), FocusNode::new()];
        nodes[0].set_rect(Rect::new(0.0, 0.0, 10.0, 10.0));
        nodes[1].set_rect(Rect::new(20.0, 0.0, 30.0, 10.0));
        let _attachments = [
            manager.root_scope().attach_node(&group).expect("group"),
            group.attach_node(&nodes[0]).expect("first"),
            group.attach_node(&nodes[1]).expect("second"),
        ];
        let replacement = Rc::new(RefCell::new(None));
        let drops = Rc::new(Cell::new(0));
        let _registration = group.register_traversal_group(
            Rc::new(ReplacingPolicy {
                group: Rc::downgrade(&group),
                replacement: Rc::clone(&replacement),
                drops: Rc::clone(&drops),
                sorting_fails,
                retirement_fails,
            }),
            TextDirection::Ltr,
            TraversalEdgeBehavior::Stop,
        );
        let _ = nodes[0].request_focus();
        let payload = catch_unwind(AssertUnwindSafe(|| manager.focus_next()))
            .expect_err("group failure propagates");
        assert_eq!(
            flui_foundation::panic::payload_text(payload.as_ref()),
            Some(if sorting_fails {
                "first group sorting failure"
            } else {
                "competing group retirement failure"
            })
        );
        flui_foundation::panic::retain_opaque_payload(payload);
        assert_eq!(drops.get(), usize::from(!sorting_fails));
        assert!(nodes[0].has_primary_focus());
        assert!(manager.focus_next());
        assert!(
            nodes[1].has_primary_focus(),
            "the replacement policy serves the next healthy traversal"
        );
        let registration = replacement.borrow_mut().take();
        drop(registration);
    }
}

fn directional_geometry_snapshots_run_once_and_respect_reentrant_focus() {
    use flui_foundation::geometry::Rect;
    use flui_interaction::{FocusDirection, TraversalEdgeBehavior};
    let manager = FocusManager::new();
    manager
        .root_scope()
        .set_traversal_edge_behavior(TraversalEdgeBehavior::ClosedLoop);
    let nodes = [FocusNode::new(), FocusNode::new()];
    let reads = [Rc::new(Cell::new(0)), Rc::new(Cell::new(0))];
    let _attachments = [
        manager.root_scope().attach_node(&nodes[0]).expect("left"),
        manager.root_scope().attach_node(&nodes[1]).expect("right"),
    ];
    for ((node, reads), left) in nodes.iter().zip(&reads).zip([0.0, 20.0]) {
        let reads = Rc::clone(reads);
        node.set_rect_provider(Rc::new(move || {
            reads.set(reads.get() + 1);
            Some(Rect::new(left, 0.0, left + 10.0, 10.0))
        }));
    }
    let _ = nodes[1].request_focus();
    assert!(manager.focus_in_direction(FocusDirection::Right));
    assert!(nodes[0].has_primary_focus());
    assert_eq!(
        [reads[0].get(), reads[1].get()],
        [1, 1],
        "wrap reuses each provider snapshot from the same directional step"
    );
    let chosen = Rc::downgrade(&nodes[1]);
    nodes[0].set_rect_provider(Rc::new(move || {
        let _ = chosen
            .upgrade()
            .expect("live reentrant target")
            .request_focus();
        Some(Rect::new(0.0, 0.0, 10.0, 10.0))
    }));
    assert!(
        !manager.focus_in_direction(FocusDirection::Right),
        "the outdated step does not replace focus chosen reentrantly by its provider"
    );
    assert!(nodes[1].has_primary_focus());
}

fn subscription_withdrawal_preserves_independent_listeners() {
    let manager = FocusManager::new();
    let node = FocusNode::new();
    let _attachment = manager.root_scope().attach_node(&node).expect("attach");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let first_calls = Rc::clone(&calls);
    let first = node.subscribe(Rc::new(move || first_calls.borrow_mut().push("first")));
    let second_calls = Rc::clone(&calls);
    let second = node.subscribe(Rc::new(move || second_calls.borrow_mut().push("second")));
    let _ = node.request_focus();
    assert_eq!(*calls.borrow(), ["first", "second"]);
    calls.borrow_mut().clear();
    drop(first);
    manager.unfocus();
    assert_eq!(*calls.borrow(), ["second"]);
    calls.borrow_mut().clear();
    drop(second);
    let _ = node.request_focus();
    assert!(calls.borrow().is_empty());

    let detached = FocusNode::new();
    let owner = Rc::downgrade(&detached);
    let (captured, capture_owner) = capture();
    let subscription = detached.subscribe(Rc::new(move || {
        let _ = &captured;
    }));
    drop(detached);
    assert!(owner.upgrade().is_none());
    assert!(capture_owner.upgrade().is_none());
    drop(subscription);
}

fn subscription_retirement_can_reenter_the_same_node() {
    struct SubscribeOnDrop {
        node: Weak<FocusNode>,
        calls: Rc<Cell<usize>>,
        replacement: Rc<RefCell<Option<flui_interaction::FocusSubscription>>>,
    }
    impl Drop for SubscribeOnDrop {
        fn drop(&mut self) {
            let node = self.node.upgrade().expect("caller retains node");
            let calls = Rc::clone(&self.calls);
            *self.replacement.borrow_mut() =
                Some(node.subscribe(Rc::new(move || calls.set(calls.get() + 1))));
        }
    }
    let manager = FocusManager::new();
    let node = FocusNode::new();
    let _attachment = manager.root_scope().attach_node(&node).expect("attach");
    let calls = Rc::new(Cell::new(0));
    let replacement = Rc::new(RefCell::new(None));
    let captured = SubscribeOnDrop {
        node: Rc::downgrade(&node),
        calls: Rc::clone(&calls),
        replacement: Rc::clone(&replacement),
    };
    let subscription = node.subscribe(Rc::new(move || {
        let _ = &captured;
    }));
    drop(subscription);
    let _ = node.request_focus();
    assert_eq!(calls.get(), 1);
    let previous = replacement.borrow_mut().take();
    drop(previous);
    manager.unfocus();
    assert_eq!(calls.get(), 1);
}

fn subscription_failure_preserves_unwind_and_allows_recovery() {
    struct FailingCapture(Rc<Cell<usize>>);
    impl Drop for FailingCapture {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
            panic!("subscription capture");
        }
    }
    for already_unwinding in [false, true] {
        let manager = FocusManager::new();
        let node = FocusNode::new();
        let _attachment = manager.root_scope().attach_node(&node).expect("attach");
        let drops = Rc::new(Cell::new(0));
        let captured = FailingCapture(Rc::clone(&drops));
        let subscription = node.subscribe(Rc::new(move || {
            let _ = &captured;
        }));
        let failure = catch_unwind(AssertUnwindSafe(|| {
            let subscription = subscription;
            assert!(!already_unwinding, "earlier failure");
            drop(subscription);
        }))
        .expect_err("failure propagates");
        assert_eq!(
            flui_foundation::panic::payload_text(failure.as_ref()),
            Some(if already_unwinding {
                "earlier failure"
            } else {
                "subscription capture"
            })
        );
        flui_foundation::panic::retain_opaque_payload(failure);
        assert_eq!(drops.get(), usize::from(!already_unwinding));
        let calls = Rc::new(Cell::new(0));
        let observed = Rc::clone(&calls);
        let healthy = node.subscribe(Rc::new(move || observed.set(observed.get() + 1)));
        let _ = node.request_focus();
        assert_eq!(calls.get(), 1);
        drop(healthy);
        manager.unfocus();
        assert_eq!(calls.get(), 1);
    }
}

fn key_dispatch_preserves_propagation_outcomes() {
    for outcome in [
        KeyEventResult::Ignored,
        KeyEventResult::Handled,
        KeyEventResult::SkipRemainingHandlers,
    ] {
        let manager = FocusManager::new();
        let node = FocusNode::new();
        let _attachment = manager.root_scope().attach_node(&node).expect("attach");
        let _ = node.request_focus();
        let calls = Rc::new(Cell::new(0));
        let observed = Rc::clone(&calls);
        node.set_on_key_event(Rc::new(move |_| {
            observed.set(observed.get() + 1);
            KeyEventResult::Handled
        }));
        manager.add_global_key_handler(Rc::new(move |_| outcome));
        assert_eq!(
            manager.dispatch_key_event(&key_event()),
            if outcome == KeyEventResult::Ignored {
                KeyEventResult::Handled
            } else {
                outcome
            }
        );
        assert_eq!(calls.get(), usize::from(outcome == KeyEventResult::Ignored));
        manager.clear_global_key_handlers();
        node.set_on_key_event(Rc::new(move |_| outcome));
        assert_eq!(manager.dispatch_key_event(&key_event()), outcome);
    }
}

fn key_event() -> KeyEvent {
    KeyEvent::new(
        KeyState::Down,
        Key::character("a"),
        Code::KeyA,
        EventTime::from_nanos(0),
    )
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
        let _ = node.request_focus();
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
        let _ = manager.dispatch_key_event(&key_event()).is_handled();
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
        let _ = node.handle_key_event(&key_event());
    });
    drop(node);
    assert!(probe.upgrade().is_none(), "the handler capture is released");
}

/// A close that already owes a failure retains only last owners: a handler
/// its caller still holds, whether withdrawn by the close or rejected by the
/// closed manager, is released with the caller.
fn closing_manager_leaves_shared_callbacks_with_their_caller() {
    let (capture, probe) = capture();
    let handler: Rc<dyn Fn(&KeyEvent) -> KeyEventResult> = Rc::new(move |_| {
        let _ = &capture;
        KeyEventResult::Ignored
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
    binding.pointer_router().add_route(
        PointerId::new(core::num::NonZeroU64::MIN),
        Rc::clone(&route),
    );
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
    fn order(&self, _: &mut [Rc<FocusNode>], _: flui_painting::typography::TextDirection) {
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

fn node_listener_failure_publishes_every_committed_edge() {
    assert_focus_notification_recovery(true, 0);
}

fn competing_manager_listener_failures_preserve_the_first_edge_failure() {
    assert_focus_notification_recovery(false, 2);
}

fn node_and_manager_listener_failures_preserve_the_node_failure() {
    assert_focus_notification_recovery(true, 1);
}

fn assert_focus_notification_recovery(node_panics: bool, manager_panics: usize) {
    let manager = FocusManager::new();
    let node = FocusNode::with_debug_label("notification destination");
    let attachment = manager.root_scope().attach_node(&node).expect("attach");
    let log = Rc::new(RefCell::new(Vec::new()));
    let fail = Rc::new(Cell::new(true));

    let mut subscriptions = Vec::new();
    for (label, panics) in [("node first", node_panics), ("node later", false)] {
        let log = Rc::clone(&log);
        let fail = Rc::clone(&fail);
        let node_probe = Rc::downgrade(&node);
        subscriptions.push(node.subscribe(Rc::new(move || {
            let focused = node_probe.upgrade().expect("live node").has_primary_focus();
            log.borrow_mut().push((label, focused));
            if panics && fail.get() {
                std::panic::panic_any("first node failure");
            }
        })));
    }
    for (index, label) in ["manager first", "manager second", "manager later"]
        .into_iter()
        .enumerate()
    {
        let log = Rc::clone(&log);
        let fail = Rc::clone(&fail);
        manager.add_listener(Rc::new(move |_, new| {
            log.borrow_mut().push((label, new.is_some()));
            if index < manager_panics && fail.get() {
                std::panic::panic_any(if index == 0 {
                    "first manager failure"
                } else {
                    "second manager failure"
                });
            }
        }));
    }

    let payload = catch_unwind(AssertUnwindSafe(|| node.request_focus()))
        .expect_err("the first listener failure propagates after the round");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&if node_panics {
            "first node failure"
        } else {
            "first manager failure"
        })
    );
    assert!(
        node.has_primary_focus(),
        "the edge committed before publication"
    );
    assert_eq!(
        *log.borrow(),
        [
            ("node first", true),
            ("node later", true),
            ("manager first", true),
            ("manager second", true),
            ("manager later", true),
        ],
        "every registered observer sees the committed transition"
    );

    fail.set(false);
    log.borrow_mut().clear();
    manager.unfocus();
    assert_eq!(
        *log.borrow(),
        [
            ("node first", false),
            ("node later", false),
            ("manager first", false),
            ("manager second", false),
            ("manager later", false),
        ],
        "the next focus transition publishes normally after containment"
    );
    drop(subscriptions);
    drop(attachment);
}

fn reentrant_listener_replacement_survives_a_failed_notification() {
    let manager = FocusManager::new();
    let node = FocusNode::with_debug_label("reentrant listener");
    let attachment = manager.root_scope().attach_node(&node).expect("attach");
    let log = Rc::new(RefCell::new(Vec::new()));
    let subscription = Rc::new(RefCell::new(None));
    let node_probe = Rc::downgrade(&node);
    let callback_log = Rc::clone(&log);
    let callback_subscription = Rc::clone(&subscription);
    let guard = node.subscribe(Rc::new(move || {
        callback_log.borrow_mut().push("reentrant");
        let node = node_probe.upgrade().expect("live node");
        let previous = callback_subscription
            .borrow_mut()
            .take()
            .expect("registered listener");
        drop(previous);
        let late_log = Rc::clone(&callback_log);
        *callback_subscription.borrow_mut() =
            Some(node.subscribe(Rc::new(move || late_log.borrow_mut().push("late"))));
        std::panic::panic_any("reentrant listener failure");
    }));
    *subscription.borrow_mut() = Some(guard);
    let stable_log = Rc::clone(&log);
    let _stable_subscription =
        node.subscribe(Rc::new(move || stable_log.borrow_mut().push("stable")));
    let edge_log = Rc::clone(&log);
    manager.add_listener(Rc::new(move |_, _| edge_log.borrow_mut().push("manager")));

    let payload =
        catch_unwind(AssertUnwindSafe(|| node.request_focus())).expect_err("listener failed");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"reentrant listener failure")
    );
    assert_eq!(
        *log.borrow(),
        ["reentrant", "stable", "manager"],
        "replacement waits for the next round and remaining observers receive this edge"
    );
    log.borrow_mut().clear();
    manager.unfocus();
    assert_eq!(
        *log.borrow(),
        ["stable", "late", "manager"],
        "the replacement listener receives the next healthy transition"
    );
    drop(attachment);
}

struct QueuedNodeContext {
    manager: Weak<FocusManager>,
    dropped: Rc<Cell<bool>>,
}

struct NestedCloseContext {
    dropped: Rc<Cell<bool>>,
    panics: bool,
}

impl Drop for NestedCloseContext {
    fn drop(&mut self) {
        self.dropped.set(true);
        if self.panics {
            std::panic::panic_any("nested close retirement failure");
        }
    }
}

fn nested_close_after_observer_failure_retains_healthy_captures() {
    assert_nested_close_preserves_observer_failure(false);
}

fn nested_close_retirement_cannot_compete_with_the_earlier_observer_failure() {
    assert_nested_close_preserves_observer_failure(true);
}

fn assert_nested_close_preserves_observer_failure(panicking_retirement: bool) {
    use flui_interaction::routing::FocusRequestOutcome;

    let manager = FocusManager::new();
    let node = FocusNode::with_debug_label("nested close destination");
    let attachment = manager.root_scope().attach_node(&node).expect("attach");
    let dropped = Rc::new(Cell::new(false));
    node.register_context(Rc::new(NestedCloseContext {
        dropped: Rc::clone(&dropped),
        panics: panicking_retirement,
    }))
    .relinquish();
    manager.add_listener(Rc::new(move |_, new| {
        if new.is_some() {
            std::panic::panic_any("earlier observer failure");
        }
    }));
    let manager_probe = Rc::downgrade(&manager);
    manager.add_listener(Rc::new(move |_, new| {
        if new.is_some() {
            manager_probe.upgrade().expect("live manager").close();
        }
    }));
    let payload = catch_unwind(AssertUnwindSafe(|| node.request_focus()))
        .expect_err("earlier observer failure propagates");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"earlier observer failure")
    );
    assert!(manager.is_closed());
    assert!(manager.primary_focus().is_none());
    assert!(
        !dropped.get(),
        "nested terminal cleanup retains opaque captures after first failure"
    );
    assert_eq!(node.request_focus(), FocusRequestOutcome::OwnerClosed);
    manager.close();
    assert!(!dropped.get());
    drop(attachment);
}

struct StaleFocusDiagnosticPanic;

impl tracing::Subscriber for StaleFocusDiagnosticPanic {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        metadata.target().ends_with("::focus") && *metadata.level() == tracing::Level::TRACE
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct Message(bool);
        impl tracing::field::Visit for Message {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 = format!("{value:?}").contains("skipping a queued focus transition");
                }
            }
        }
        let mut message = Message(false);
        event.record(&mut message);
        if message.0 {
            std::panic::panic_any("queued focus diagnostic failure");
        }
    }
}

fn queued_diagnostic_failure_keeps_the_accepted_tail_deliverable() {
    assert_queued_diagnostic_recovery(false);
}

fn earlier_observer_failure_survives_a_queued_diagnostic_failure() {
    assert_queued_diagnostic_recovery(true);
}

fn assert_queued_diagnostic_recovery(earlier_failure: bool) {
    let manager = FocusManager::new();
    let first = FocusNode::with_debug_label("first");
    let last = FocusNode::with_debug_label("last");
    let attachments = [
        manager
            .root_scope()
            .attach_node(&first)
            .expect("attach first"),
        manager
            .root_scope()
            .attach_node(&last)
            .expect("attach last"),
    ];
    let queued = FocusNode::with_debug_label("stale diagnostic target");
    let queued_attachment = manager
        .root_scope()
        .attach_node(&queued)
        .expect("attach queued");
    let queued_owner = RefCell::new(Some((queued, queued_attachment)));
    let last_probe = Rc::downgrade(&last);
    let first_id = first.id();
    manager.add_listener(Rc::new(move |_, new| {
        if new.as_ref().is_none_or(|node| node.id() != first_id) {
            return;
        }
        let Some((queued, attachment)) = queued_owner.borrow_mut().take() else {
            return;
        };
        let _ = queued.request_focus();
        let _ = last_probe.upgrade().expect("live last").request_focus();
        let _ = attachment.detach();
        if earlier_failure {
            std::panic::panic_any("earlier observer failure");
        }
    }));
    let log = Rc::new(RefCell::new(Vec::new()));
    let edge_log = Rc::clone(&log);
    manager.add_listener(Rc::new(move |previous, new| {
        edge_log.borrow_mut().push((
            previous.as_ref().map(|node| node.id()),
            new.as_ref().map(|node| node.id()),
        ));
    }));
    flui_testing::log_capture::disarm_interest_cache();
    let payload = tracing::subscriber::with_default(StaleFocusDiagnosticPanic, || {
        catch_unwind(AssertUnwindSafe(|| first.request_focus()))
    })
    .expect_err("first failure propagates after diagnostics and delivery");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&if earlier_failure {
            "earlier observer failure"
        } else {
            "queued focus diagnostic failure"
        })
    );
    let last_id = last.id();
    assert_eq!(
        *log.borrow(),
        [(None, Some(first_id)), (Some(first_id), Some(last_id))]
    );
    assert!(last.has_primary_focus());
    log.borrow_mut().clear();
    manager.unfocus();
    assert_eq!(*log.borrow(), [(Some(last_id), None)]);
    drop(attachments);
}

impl Drop for QueuedNodeContext {
    fn drop(&mut self) {
        self.dropped.set(true);
        self.manager.upgrade().expect("live manager").unfocus();
        std::panic::panic_any("queued node retirement failure");
    }
}

fn queued_retirement_failure_keeps_the_accepted_tail_deliverable() {
    assert_queued_retirement_recovery(false);
}

fn earlier_observer_failure_retains_the_retired_queued_last_owner() {
    assert_queued_retirement_recovery(true);
}

fn assert_queued_retirement_recovery(earlier_failure: bool) {
    use flui_interaction::routing::FocusRequestOutcome;

    let manager = FocusManager::new();
    let first = FocusNode::with_debug_label("first");
    let last = FocusNode::with_debug_label("last");
    let attachments = [
        manager
            .root_scope()
            .attach_node(&first)
            .expect("attach first"),
        manager
            .root_scope()
            .attach_node(&last)
            .expect("attach last"),
    ];
    let queued = FocusNode::with_debug_label("queued stale target");
    let queued_attachment = manager
        .root_scope()
        .attach_node(&queued)
        .expect("attach queued");
    let dropped = Rc::new(Cell::new(false));
    queued
        .register_context(Rc::new(QueuedNodeContext {
            manager: Rc::downgrade(&manager),
            dropped: Rc::clone(&dropped),
        }))
        .relinquish();
    let queued_owner = RefCell::new(Some((queued, queued_attachment)));
    let last_probe = Rc::downgrade(&last);
    let first_id = first.id();
    manager.add_listener(Rc::new(move |_, new| {
        if new.as_ref().is_none_or(|node| node.id() != first_id) {
            return;
        }
        let Some((queued, attachment)) = queued_owner.borrow_mut().take() else {
            return;
        };
        assert_eq!(queued.request_focus(), FocusRequestOutcome::Focused);
        assert_eq!(
            last_probe.upgrade().expect("live last").request_focus(),
            FocusRequestOutcome::Focused
        );
        let _ = attachment.detach();
        drop((queued, attachment));
        if earlier_failure {
            std::panic::panic_any("earlier observer failure");
        }
    }));
    let log = Rc::new(RefCell::new(Vec::new()));
    let edge_log = Rc::clone(&log);
    manager.add_listener(Rc::new(move |previous, new| {
        edge_log.borrow_mut().push((
            previous.as_ref().map(|node| node.id()),
            new.as_ref().map(|node| node.id()),
        ));
    }));
    let payload = catch_unwind(AssertUnwindSafe(|| first.request_focus()))
        .expect_err("first failure propagates after queued retirement");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&if earlier_failure {
            "earlier observer failure"
        } else {
            "queued node retirement failure"
        })
    );
    assert_eq!(dropped.get(), !earlier_failure);
    let last_id = last.id();
    let expected = if earlier_failure {
        vec![(None, Some(first_id)), (Some(first_id), Some(last_id))]
    } else {
        vec![
            (None, Some(first_id)),
            (Some(first_id), None),
            (None, Some(last_id)),
        ]
    };
    assert_eq!(
        *log.borrow(),
        expected,
        "healthy accepted tail survives retirement"
    );
    assert!(last.has_primary_focus());
    log.borrow_mut().clear();
    manager.unfocus();
    assert_eq!(*log.borrow(), [(Some(last_id), None)]);
    drop(attachments);
}

fn node_failure_keeps_accepted_focus_requests_deliverable() {
    assert_queued_focus_recovery(true, false);
}

fn manager_failure_keeps_accepted_focus_requests_deliverable() {
    assert_queued_focus_recovery(false, false);
}

fn competing_queued_focus_failures_preserve_the_first_failure() {
    assert_queued_focus_recovery(false, true);
}

fn assert_queued_focus_recovery(from_node: bool, competing: bool) {
    let manager = FocusManager::new();
    let nodes = [
        FocusNode::with_debug_label("first"),
        FocusNode::with_debug_label("second"),
        FocusNode::with_debug_label("last"),
    ];
    let attachments: Vec<_> = nodes
        .iter()
        .map(|node| manager.root_scope().attach_node(node).expect("attach"))
        .collect();
    let fail = Rc::new(Cell::new(true));
    let manager_probe = Rc::downgrade(&manager);
    let second_probe = Rc::downgrade(&nodes[1]);
    let last_probe = Rc::downgrade(&nodes[2]);
    let fail_probe = Rc::clone(&fail);
    let queue_then_fail = Rc::new(move || {
        if !fail_probe.get() {
            return;
        }
        let manager = manager_probe.upgrade().expect("live manager");
        assert_eq!(
            second_probe.upgrade().expect("live second").request_focus(),
            flui_interaction::FocusRequestOutcome::Focused
        );
        manager.unfocus();
        assert_eq!(
            last_probe.upgrade().expect("live last").request_focus(),
            flui_interaction::FocusRequestOutcome::Focused
        );
        std::panic::panic_any("first queued focus failure");
    });
    let first_id = nodes[0].id();
    let _node_subscription = if from_node {
        let first_probe = Rc::downgrade(&nodes[0]);
        Some(nodes[0].subscribe(Rc::new(move || {
            if first_probe
                .upgrade()
                .expect("live first")
                .has_primary_focus()
            {
                queue_then_fail();
            }
        })))
    } else {
        manager.add_listener(Rc::new(move |_, new| {
            if new.as_ref().is_some_and(|node| node.id() == first_id) {
                queue_then_fail();
            }
        }));
        None
    };
    let second_id = nodes[1].id();
    let later_failure = Rc::clone(&fail);
    manager.add_listener(Rc::new(move |_, new| {
        if competing
            && later_failure.get()
            && new.as_ref().is_some_and(|node| node.id() == second_id)
        {
            std::panic::panic_any("second queued focus failure");
        }
    }));
    let log = Rc::new(RefCell::new(Vec::new()));
    let edge_log = Rc::clone(&log);
    manager.add_listener(Rc::new(move |previous, new| {
        edge_log.borrow_mut().push((
            previous.as_ref().map(|node| node.id()),
            new.as_ref().map(|node| node.id()),
        ));
    }));
    let payload = catch_unwind(AssertUnwindSafe(|| nodes[0].request_focus()))
        .expect_err("first listener failure propagates after delivery");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"first queued focus failure")
    );
    let last_id = nodes[2].id();
    assert_eq!(
        *log.borrow(),
        [
            (None, Some(first_id)),
            (Some(first_id), Some(second_id)),
            (Some(second_id), None),
            (None, Some(last_id)),
        ],
        "every accepted request publishes FIFO before the first failure resumes"
    );
    assert!(nodes[2].has_primary_focus());
    fail.set(false);
    log.borrow_mut().clear();
    manager.unfocus();
    assert_eq!(*log.borrow(), [(Some(last_id), None)]);
    drop(attachments);
}

#[test]
fn caught_callback_failures_leave_captures_with_their_owner() {
    let cases: &[(&str, fn())] = &[
        (
            "commit diagnostics, competing observers and reentrant FIFO",
            commit_diagnostics_preserve_focus_round_and_queued_requests,
        ),
        (
            "directional geometry snapshots and reentrant focus",
            directional_geometry_snapshots_run_once_and_respect_reentrant_focus,
        ),
        (
            "group policy replacement and failure recovery",
            group_policy_replacement_preserves_failure_and_future_traversal,
        ),
        (
            "weak traversal ownership and generations",
            weak_traversal_links_revalidate_groups_and_registration_generations,
        ),
        (
            "directional provider failure and recovery",
            directional_provider_failure_preserves_first_failure_and_recovery,
        ),
        (
            "geometric focus ranking and admission",
            geometric_focus_navigation_pins_ranking_and_admission,
        ),
        (
            "directional scope edges",
            directional_edges_match_linear_scope_outcomes,
        ),
        (
            "subscription ownership and independent listeners",
            subscription_withdrawal_preserves_independent_listeners,
        ),
        (
            "subscription retirement reentry",
            subscription_retirement_can_reenter_the_same_node,
        ),
        (
            "subscription failure preservation and recovery",
            subscription_failure_preserves_unwind_and_allows_recovery,
        ),
        (
            "key dispatch propagation outcomes",
            key_dispatch_preserves_propagation_outcomes,
        ),
        (
            "nested close preserves healthy captures after observer failure",
            nested_close_after_observer_failure_retains_healthy_captures,
        ),
        (
            "nested close preserves competing captures after observer failure",
            nested_close_retirement_cannot_compete_with_the_earlier_observer_failure,
        ),
        (
            "queued diagnostic failure and accepted tail",
            queued_diagnostic_failure_keeps_the_accepted_tail_deliverable,
        ),
        (
            "earlier observer failure and queued diagnostic",
            earlier_observer_failure_survives_a_queued_diagnostic_failure,
        ),
        (
            "queued retirement failure and accepted tail",
            queued_retirement_failure_keeps_the_accepted_tail_deliverable,
        ),
        (
            "earlier observer failure and queued retirement",
            earlier_observer_failure_retains_the_retired_queued_last_owner,
        ),
        (
            "accepted focus requests after node failure",
            node_failure_keeps_accepted_focus_requests_deliverable,
        ),
        (
            "accepted focus requests after manager failure",
            manager_failure_keeps_accepted_focus_requests_deliverable,
        ),
        (
            "competing queued focus failures",
            competing_queued_focus_failures_preserve_the_first_failure,
        ),
        (
            "node notification failure",
            node_listener_failure_publishes_every_committed_edge,
        ),
        (
            "competing manager notification failures",
            competing_manager_listener_failures_preserve_the_first_edge_failure,
        ),
        (
            "node and manager notification failures",
            node_and_manager_listener_failures_preserve_the_node_failure,
        ),
        (
            "reentrant listener replacement",
            reentrant_listener_replacement_survives_a_failed_notification,
        ),
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
    let _subscription = nested.subscribe(Rc::new(move || {
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
