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
    pub(crate) fn tab_and_shift_tab_move_the_focus_through_the_actions_chain() {
        let scope = FocusScopeNode::with_debug_label("tab-scope");
        let left = FocusNode::with_debug_label("left");
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
