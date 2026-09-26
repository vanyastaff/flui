//! [`Shortcuts`], [`CallbackShortcuts`], the intent/action bridge, Tab
//! traversal and activation keys against a mounted tree. The pure
//! `SingleActivator` matching test stays a unit test in
//! `src/interaction/shortcuts.rs`.

mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_interaction::events::{Key, KeyEvent, KeyState, Modifiers};
    use flui_interaction::routing::{FocusNode, KeyEventResult};
    use flui_widgets::SizedBox;
    use flui_widgets::interaction::{CallbackShortcuts, Focus, SingleActivator};

    use crate::common::harness::mount;

    fn key_down(character: &str, modifiers: Modifiers) -> KeyEvent {
        KeyEvent {
            state: KeyState::Down,
            key: Key::Character(character.into()),
            modifiers,
            ..KeyEvent::default()
        }
    }

    /// `Shortcuts` end to end (ADR-0023): a shortcut above a focused `Focus` fires
    /// only for keys that subtree **ignored** — a key the focused handler
    /// consumed never reaches the binding, and a matching ignored key fires
    /// every binding while counting as handled.
    ///
    /// Red-check: revert `dispatch_key_event` to the earlier flat dispatch —
    /// the binding never fires and the second assertion fails.
    #[test]
    fn a_shortcut_fires_only_for_keys_the_focused_subtree_ignored() {
        let fired = Arc::new(AtomicUsize::new(0));
        let field = FocusNode::with_debug_label("shortcut-field");

        // The inner "field" consumes the character "x" and ignores all else.
        let inner = Focus::new(SizedBox::new(10.0, 10.0))
            .focus_node(Rc::clone(&field))
            .on_key_event(Rc::new(|event| match &event.key {
                Key::Character(c) if c == "x" => KeyEventResult::Handled,
                _ => KeyEventResult::Ignored,
            }));

        let fired_for_binding = Arc::clone(&fired);
        let harness = mount(CallbackShortcuts::new(inner).binding(
            SingleActivator::character("d").control(),
            move || {
                fired_for_binding.fetch_add(1, Ordering::SeqCst);
            },
        ));
        let manager = harness.focus_manager();
        field.request_focus();

        // Consumed below: never bubbles to the shortcut.
        assert!(manager.dispatch_key_event(&key_down("x", Modifiers::empty())));
        assert_eq!(
            fired.load(Ordering::SeqCst),
            0,
            "a consumed key stays below"
        );

        // Ignored below and matching: the binding fires, the event is handled.
        assert!(manager.dispatch_key_event(&key_down("d", Modifiers::CONTROL)));
        assert_eq!(fired.load(Ordering::SeqCst), 1, "the shortcut fired");

        // Ignored below and not matching: unhandled, nothing fires.
        assert!(!manager.dispatch_key_event(&key_down("q", Modifiers::empty())));
        assert_eq!(fired.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn callback_shortcuts_accept_owner_local_rc_state() {
        let fired = Rc::new(Cell::new(0));
        let fired_for_binding = Rc::clone(&fired);
        let field = FocusNode::with_debug_label("owner-local-shortcut-field");
        let harness = mount(
            CallbackShortcuts::new(
                Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&field)),
            )
            .binding(SingleActivator::character("l").control(), move || {
                fired_for_binding.set(fired_for_binding.get() + 1);
            }),
        );
        let manager = harness.focus_manager();
        field.request_focus();

        assert!(manager.dispatch_key_event(&key_down("l", Modifiers::CONTROL)));
        assert_eq!(fired.get(), 1, "shortcut callback captured Rc<Cell<_>>");
    }
}

mod intent_tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_interaction::events::{Key, KeyEvent, KeyState, Modifiers};
    use flui_interaction::routing::FocusNode;
    use flui_widgets::SizedBox;
    use flui_widgets::interaction::{
        Action, ActionOutcome, Actions, CallbackAction, Focus, Intent, Shortcuts, SingleActivator,
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
    #[test]
    fn a_shortcut_dispatches_its_intent_through_the_actions_chain() {
        let saves = Arc::new(AtomicUsize::new(0));
        let field = FocusNode::with_debug_label("intent-field");

        let saves_for_action = Arc::clone(&saves);
        let harness = mount(
            Actions::new(
                Shortcuts::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&field)))
                    .shortcut(SingleActivator::character("s").control(), SaveIntent),
            )
            .action(CallbackAction::new(move |_intent: &SaveIntent| {
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

    #[test]
    fn shortcut_actions_accept_owner_local_rc_state() {
        let saves = Rc::new(Cell::new(0));
        let saves_for_action = Rc::clone(&saves);
        let field = FocusNode::with_debug_label("owner-local-intent-field");
        let harness = mount(
            Actions::new(
                Shortcuts::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&field)))
                    .shortcut(SingleActivator::character("s").control(), SaveIntent),
            )
            .action(CallbackAction::new(move |_intent: &SaveIntent| {
                saves_for_action.set(saves_for_action.get() + 1);
            })),
        );
        let manager = harness.focus_manager();
        field.request_focus();

        assert!(manager.dispatch_key_event(&ctrl_s()), "consumed");
        assert_eq!(saves.get(), 1, "action callback captured Rc<Cell<_>>");
    }

    #[test]
    fn shortcut_intents_accept_owner_local_rc_payloads() {
        struct OwnerLocalIntent {
            marker: Rc<Cell<u32>>,
        }
        impl Intent for OwnerLocalIntent {}

        let marker = Rc::new(Cell::new(7));
        let seen = Rc::new(Cell::new(0));
        let seen_for_action = Rc::clone(&seen);
        let field = FocusNode::with_debug_label("owner-local-intent-payload-field");
        let harness = mount(
            Actions::new(
                Shortcuts::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&field)))
                    .shortcut(
                        SingleActivator::character("s").control(),
                        OwnerLocalIntent {
                            marker: Rc::clone(&marker),
                        },
                    ),
            )
            .action(CallbackAction::new(move |intent: &OwnerLocalIntent| {
                seen_for_action.set(intent.marker.get());
            })),
        );
        let manager = harness.focus_manager();
        field.request_focus();

        assert!(manager.dispatch_key_event(&ctrl_s()), "consumed");
        assert_eq!(
            seen.get(),
            7,
            "shortcut intent carried an owner-local Rc<Cell<_>> payload"
        );
    }

    /// The default [`Action::to_key_event_result`](flui_widgets::interaction::Action::to_key_event_result)
    /// maps a `NotPerformed` outcome to `SkipRemainingHandlers`
    /// (`actions.dart:312-314`): the action runs, but the event reports
    /// unconsumed and stops bubbling.
    ///
    /// Flutter parity (`actions_test.dart`, tag `3.44.0`): stands in for
    /// `'Base Action class default toKeyEventResult delegates to
    /// consumesKey'`. **Adapted, documented divergence**: Flutter splits the
    /// question across two independently overridable methods,
    /// `consumesKey`/`toKeyEventResult`, which can disagree; FLUI collapsed
    /// them into the one method this test exercises (ADR-0023/ADR-0026) —
    /// there is no separate `consumes_key` to assert delegates to anything.
    #[test]
    fn a_non_consuming_action_runs_but_leaves_the_event_unconsumed() {
        // An action that runs but changes nothing declines the key, so the
        // event keeps bubbling instead of being swallowed.
        struct NonConsuming(Arc<AtomicUsize>);
        impl Action<SaveIntent> for NonConsuming {
            fn invoke(&self, _intent: &SaveIntent) -> ActionOutcome {
                self.0.fetch_add(1, Ordering::SeqCst);
                ActionOutcome::NotPerformed
            }
        }

        let runs = Arc::new(AtomicUsize::new(0));
        let field = FocusNode::with_debug_label("nonconsuming-field");
        let harness = mount(
            Actions::new(
                Shortcuts::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&field)))
                    .shortcut(SingleActivator::character("s").control(), SaveIntent),
            )
            .action(NonConsuming(Arc::clone(&runs))),
        );
        let manager = harness.focus_manager();
        field.request_focus();

        assert!(
            !manager.dispatch_key_event(&ctrl_s()),
            "SkipRemainingHandlers reports the event unconsumed"
        );
        assert_eq!(runs.load(Ordering::SeqCst), 1, "the action still ran");
    }
}

mod tab_tests {
    use std::rc::Rc;

    use flui_interaction::events::{Key, KeyEvent, KeyState, Modifiers, NamedKey};
    use flui_interaction::routing::{FocusNode, FocusScopeNode};
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

    /// **Tab works, end to end** (ADR-0026): a real key event enters
    /// `dispatch_key_event`, bubbles from the focused field (ADR-0023),
    /// matches the `Shortcuts` activator, resolves `NextFocusIntent` through
    /// the enclosing `Actions`, and moves the focus in reading order.
    ///
    /// The traversal actions come from `DefaultFocusTraversal`, which the
    /// harness's `FocusRoot` installs.
    #[test]
    fn tab_and_shift_tab_move_the_focus_through_the_actions_chain() {
        let scope = FocusScopeNode::with_debug_label("tab-scope");
        let left = FocusNode::with_debug_label("left");
        let right = FocusNode::with_debug_label("right");

        let field = |x: f32, node: &Rc<FocusNode>| {
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
            Stack::new(vec![field(0.0, &left), field(20.0, &right)]),
        ));
        let manager = harness.focus_manager();
        left.request_focus();

        assert!(manager.dispatch_key_event(&tab(false)), "Tab is consumed");
        assert!(
            right.has_primary_focus(),
            "Tab moved the focus to the next node in reading order"
        );

        assert!(manager.dispatch_key_event(&tab(true)), "Shift+Tab too");
        assert!(left.has_primary_focus(), "and it stepped back");
    }

    /// `NextFocusAction`'s key result is **what the traversal did**
    /// (`focus_traversal.dart:2340-2348`): with a `Stop` edge and nowhere to
    /// go, the action runs, moves nothing, and reports the event
    /// **unconsumed** — so an outer handler still gets its chance. This is the
    /// channel ADR-0023 dropped and ADR-0026's review chose to reopen with
    /// a breaking `invoke -> ActionOutcome` rather than a second, silently
    /// divergent method.
    ///
    /// Red-check: make `to_key_event_result` ignore the outcome (the trait
    /// default) — the dead Tab reports handled and swallows the key.
    #[test]
    fn a_tab_with_nowhere_to_go_reports_the_key_unconsumed() {
        use flui_interaction::routing::TraversalEdgeBehavior;

        let scope = FocusScopeNode::with_debug_label("dead-end-scope");
        scope.set_traversal_edge_behavior(TraversalEdgeBehavior::Stop);
        let only = FocusNode::with_debug_label("only");

        let harness = mount(FocusScope::with_external_node(
            Rc::clone(&scope),
            Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&only)),
        ));
        let manager = harness.focus_manager();
        only.request_focus();

        assert!(
            !manager.dispatch_key_event(&tab(false)),
            "a Tab that moved nothing is reported unconsumed"
        );
        assert!(only.has_primary_focus(), "and the focus stayed put");
    }
}

mod activation_tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use flui_interaction::events::{Key, KeyEvent, KeyState, Modifiers, NamedKey};
    use flui_interaction::routing::FocusNode;
    use flui_widgets::SizedBox;
    use flui_widgets::interaction::{
        Actions, ActivateIntent, CallbackAction, Focus, Intent, Shortcuts, SingleActivator,
    };

    use crate::common::harness::mount;

    fn key_down(key: Key) -> KeyEvent {
        KeyEvent {
            state: KeyState::Down,
            key,
            modifiers: Modifiers::empty(),
            ..KeyEvent::default()
        }
    }

    struct SaveIntent;
    impl Intent for SaveIntent {}

    /// **An intent resolves at the primary focus** (ADR-0079), as Flutter's
    /// `ShortcutManager` resolves against `primaryFocus.context`: an `Actions`
    /// between the focused widget and the `Shortcuts` answers the intent —
    /// the shape every button's activation has.
    ///
    /// Red-check: resolve from the `Shortcuts` widget's own position (drop the
    /// `chain_at` lookup) — nothing above the `Shortcuts` binds `SaveIntent`,
    /// so the key is ignored and the action never runs.
    #[test]
    fn an_actions_between_the_focus_and_the_shortcuts_answers_the_intent() {
        let runs = Rc::new(Cell::new(0));
        let field = FocusNode::with_debug_label("field");
        let counted = Rc::clone(&runs);
        let harness = mount(
            Shortcuts::new(
                Actions::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&field)))
                    .action(CallbackAction::new(move |_: &SaveIntent| {
                        counted.set(counted.get() + 1);
                    })),
            )
            .shortcut(SingleActivator::character("s").control(), SaveIntent),
        );
        let manager = harness.focus_manager();
        field.request_focus();

        let ctrl_s = KeyEvent {
            modifiers: Modifiers::CONTROL,
            ..key_down(Key::Character("s".into()))
        };
        assert!(manager.dispatch_key_event(&ctrl_s), "consumed");
        assert_eq!(runs.get(), 1, "the action below the Shortcuts ran");
    }

    /// A focused `FocusScope` node resolves at its own position too: its
    /// backing node can hold the primary focus, and an `Actions` between it
    /// and the `Shortcuts` must answer.
    ///
    /// Red-check: drop the `record_action_chain` calls from
    /// `FocusScopeState` — the scope's node has no record, the `Shortcuts`
    /// falls back to its own position, and the action never runs.
    #[test]
    fn a_focused_scope_resolves_intents_at_its_own_position() {
        use flui_interaction::routing::FocusScopeNode;

        use flui_widgets::interaction::FocusScope;

        let runs = Rc::new(Cell::new(0));
        let scope = FocusScopeNode::with_debug_label("scope");
        let counted = Rc::clone(&runs);
        let harness = mount(
            Shortcuts::new(
                Actions::new(FocusScope::with_external_node(
                    Rc::clone(&scope),
                    SizedBox::new(10.0, 10.0),
                ))
                .action(CallbackAction::new(move |_: &SaveIntent| {
                    counted.set(counted.get() + 1);
                })),
            )
            .shortcut(SingleActivator::character("s").control(), SaveIntent),
        );
        let manager = harness.focus_manager();
        scope.as_focus_node().request_focus();
        assert!(
            scope.as_focus_node().has_primary_focus(),
            "the empty scope holds the focus"
        );

        let ctrl_s = KeyEvent {
            modifiers: Modifiers::CONTROL,
            ..key_down(Key::Character("s".into()))
        };
        assert!(manager.dispatch_key_event(&ctrl_s), "consumed");
        assert_eq!(
            runs.get(),
            1,
            "the action between the scope and the Shortcuts ran"
        );
    }

    /// Enter, Space and Select activate the focused control through the
    /// root bindings — `WidgetsApp`'s `_defaultShortcuts` (`app.dart:1265-1269`,
    /// tag `3.44.0`) — and an activation key no control claims keeps bubbling.
    ///
    /// Red-check: drop the three `ActivateIntent` bindings from
    /// `DefaultFocusTraversal` — every dispatch is ignored.
    #[test]
    fn enter_space_and_select_activate_the_focused_control() {
        let runs = Rc::new(Cell::new(0));
        let button = FocusNode::with_debug_label("button");
        let counted = Rc::clone(&runs);
        let harness = mount(
            Actions::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&button)))
                .action(CallbackAction::new(move |_: &ActivateIntent| {
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

    /// **The first Tab into a window with nothing focused** reaches the
    /// default bindings and focuses the first control. Found on a live
    /// Windows window: the key walk started at the primary focus, there was
    /// none, and every key was dropped — no control was reachable from the
    /// keyboard at all.
    ///
    /// Red-check: drop `claim_unfocused_keys` from
    /// `DefaultFocusTraversalState::claim_unfocused_keys` — the Tab is
    /// ignored and nothing gains focus.
    #[test]
    fn the_first_tab_with_nothing_focused_focuses_the_first_control() {
        let button = FocusNode::with_debug_label("button");
        let harness = mount(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&button)));
        let manager = harness.focus_manager();
        assert!(
            manager.primary_focus().is_none(),
            "the window opens with nothing focused"
        );

        assert!(
            manager.dispatch_key_event(&key_down(Key::Named(NamedKey::Tab))),
            "Tab is consumed"
        );
        assert!(
            button.has_primary_focus(),
            "and it brought the focus to the control"
        );
    }

    /// With no control answering `ActivateIntent`, Enter is not swallowed:
    /// nothing at the root binds an action to it, so the key is reported
    /// unconsumed and an outer handler (or the platform) still gets it.
    #[test]
    fn an_unclaimed_activation_key_is_not_consumed() {
        let field = FocusNode::with_debug_label("plain");
        let harness = mount(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&field)));
        let manager = harness.focus_manager();
        field.request_focus();

        assert!(!manager.dispatch_key_event(&key_down(Key::Named(NamedKey::Enter))));
    }
}
