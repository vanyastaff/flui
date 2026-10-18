//! Activation from the keyboard does what a click does, once per physical
//! press; the navigation keys reach their intents; a panicking callback
//! leaves the keyboard working.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use flui_interaction::FocusManager;
use flui_interaction::focus::FocusNode;
use flui_platform_api::TargetPlatform;
use flui_platform_api::keyboard::{Key, KeyRepeat, KeyState, Modifiers, NamedKey};
use flui_view::EventCx;
use flui_widgets::interaction::{
    Action, ActionOutcome, Actions, ActivateIntent, BackIntent, CallbackAction, DismissIntent,
    Focus,
};
use flui_widgets::{RawButton, Row, SizedBox};

use super::{chord, down, key, press, tab, up};
use crate::common::harness::{Harness, mount};

/// A control a test can activate: how it is built decides what answers
/// [`ActivateIntent`].
#[derive(Clone, Copy, Debug)]
enum Control {
    /// `RawButton` with an `on_press`.
    RawButton,
    /// An app's own `Focus` + `Actions` control.
    Custom,
    /// A `Focus` + `Actions` control whose activation is disabled.
    Disabled,
}

/// An activation action that is declared but disabled: resolution stops at
/// it, so the key keeps bubbling.
struct DisabledActivate;

impl Action<ActivateIntent> for DisabledActivate {
    fn is_enabled(&self, _intent: &ActivateIntent) -> bool {
        false
    }

    fn invoke(&self, _cx: &mut EventCx<'_>, _intent: &ActivateIntent) -> ActionOutcome {
        panic!("a disabled activation must never be invoked");
    }
}

/// `control` alone in the tree, 40 × 40 at the origin, and the activation
/// count it bumps.
fn mount_control(control: Control) -> (Harness, Rc<Cell<u32>>) {
    let activations = Rc::new(Cell::new(0));
    let counted = Rc::clone(&activations);
    let harness = match control {
        Control::RawButton => mount(
            RawButton::new(SizedBox::new(40.0, 40.0))
                .on_press(move |_cx| counted.set(counted.get() + 1)),
        ),
        Control::Custom => mount(Actions::new(Focus::new(SizedBox::new(40.0, 40.0))).action(
            CallbackAction::new(move |_cx, _: &ActivateIntent| counted.set(counted.get() + 1)),
        )),
        Control::Disabled => {
            mount(Actions::new(Focus::new(SizedBox::new(40.0, 40.0))).action(DisabledActivate))
        }
    };
    (harness, activations)
}

/// Press `pressed`, hold it through five auto-repeats, release it. Returns
/// whether the key-down was consumed.
fn hold(manager: &FocusManager, pressed: &Key, count: impl Fn() -> u32) -> (bool, u32) {
    let consumed = down(manager, pressed);
    for _ in 0..5 {
        let _ = manager.dispatch_key_event(&key(
            KeyState::Down,
            pressed.clone(),
            KeyRepeat::AutoRepeat,
        ));
    }
    let held = count();
    up(manager, pressed);
    (consumed, held)
}

/// Enter and Select activate on key-down, Space on its release; holding any
/// of them activates once. Each keyboard sequence ends with the activation
/// count a click gives. A disabled control activates nothing and leaves the
/// key unconsumed for whatever encloses it.
pub(crate) fn keyboard_activation_matches_click() {
    let enter = Key::Named(NamedKey::Enter);
    let select = Key::Named(NamedKey::Select);
    let space = Key::character(" ");
    // (row, key, held through repeats, activations before the release)
    let rows: [(&str, &Key, bool, u32); 5] = [
        ("enter", &enter, false, 1),
        ("select", &select, false, 1),
        ("space", &space, false, 0),
        ("held enter", &enter, true, 1),
        ("held space", &space, true, 0),
    ];

    // The baseline: one click is one activation.
    let (click, clicks) = mount_control(Control::RawButton);
    click.dispatch_pointer_down(20.0, 20.0);
    click.dispatch_pointer_up(20.0, 20.0);
    assert_eq!(clicks.get(), 1, "a click activates once");

    for control in [Control::RawButton, Control::Custom, Control::Disabled] {
        for (name, pressed, held, before_release) in rows {
            let (harness, activations) = mount_control(control);
            let manager = harness.focus_manager();
            assert!(tab(&manager), "{control:?}: Tab focuses the control");
            let count = || activations.get();
            let (consumed, while_held) = if held {
                hold(&manager, pressed, count)
            } else {
                let consumed = down(&manager, pressed);
                let while_held = count();
                up(&manager, pressed);
                (consumed, while_held)
            };
            let expected = match control {
                Control::Disabled => 0,
                Control::RawButton | Control::Custom => clicks.get(),
            };
            assert_eq!(
                activations.get(),
                expected,
                "{control:?} × {name}: the keys activate as a click does"
            );
            if !matches!(control, Control::Disabled) {
                assert_eq!(
                    while_held, before_release,
                    "{control:?} × {name}: Space activates on its release, the others on key-down"
                );
            }
            if matches!(control, Control::Disabled) {
                assert!(
                    !consumed,
                    "{name}: a disabled control leaves the key to its host"
                );
            } else if before_release > 0 {
                assert!(
                    consumed,
                    "{control:?} × {name}: the activating key-down is consumed"
                );
            }
        }
    }
}

/// Escape asks to dismiss, the platform's back chord and `BrowserBack` ask to
/// go back, once per press; the other platform's back chord does nothing.
pub(crate) fn navigation_keys_reach_back_and_dismiss_once() {
    let backs = Rc::new(Cell::new(0));
    let dismissals = Rc::new(Cell::new(0));
    let (counted_back, counted_dismiss) = (Rc::clone(&backs), Rc::clone(&dismissals));
    let harness = mount(
        Actions::new(Focus::new(SizedBox::new(40.0, 40.0)))
            .action(CallbackAction::new(move |_cx, _: &BackIntent| {
                counted_back.set(counted_back.get() + 1);
            }))
            .action(CallbackAction::new(move |_cx, _: &DismissIntent| {
                counted_dismiss.set(counted_dismiss.get() + 1);
            })),
    );
    let manager = harness.focus_manager();
    assert!(tab(&manager));

    let escape = Key::Named(NamedKey::Escape);
    assert!(
        press(&manager, &escape),
        "Escape is consumed by the dismiss action"
    );
    let _ = hold(&manager, &escape, || 0);
    assert_eq!(dismissals.get(), 2, "Escape dismisses once per press");

    let apple = matches!(
        TargetPlatform::current(),
        TargetPlatform::MacOS | TargetPlatform::iOS
    );
    let (back, back_modifiers, foreign, foreign_modifiers) = if apple {
        (
            Key::character("["),
            Modifiers::META,
            Key::Named(NamedKey::ArrowLeft),
            Modifiers::ALT,
        )
    } else {
        (
            Key::Named(NamedKey::ArrowLeft),
            Modifiers::ALT,
            Key::character("["),
            Modifiers::META,
        )
    };
    assert!(
        chord(&manager, &back, back_modifiers),
        "the platform's back chord is consumed"
    );
    assert!(press(&manager, &Key::Named(NamedKey::BrowserBack)));
    assert_eq!(
        backs.get(),
        2,
        "the back chord and BrowserBack each go back once"
    );
    let _ = chord(&manager, &foreign, foreign_modifiers);
    assert_eq!(
        backs.get(),
        2,
        "the other platform's back chord does not go back"
    );
    assert_eq!(dismissals.get(), 2, "and nothing else dismissed");
}

/// Typing a space or pressing Escape in a field inside an activatable,
/// dismissible control neither activates nor dismisses it: the field keeps
/// those keys.
pub(crate) fn keys_typed_in_a_field_stay_in_the_field() {
    use flui_widgets::{EditableText, TextEditingController};

    let commands = Rc::new(Cell::new(0));
    let (activated, dismissed) = (Rc::clone(&commands), Rc::clone(&commands));
    let field = FocusNode::with_debug_label("field");
    let harness = crate::common::harness::mount_with_ime(
        Actions::new(EditableText::new(
            TextEditingController::new(),
            Rc::clone(&field),
        ))
        .action(CallbackAction::new(move |_cx, _: &ActivateIntent| {
            activated.set(activated.get() + 1);
        }))
        .action(CallbackAction::new(move |_cx, _: &DismissIntent| {
            dismissed.set(dismissed.get() + 1);
        })),
    );
    let _ = field.request_focus();
    assert!(field.has_primary_focus());
    let manager = harness.focus_manager();
    let _ = press(&manager, &Key::character(" "));
    let _ = press(&manager, &Key::Named(NamedKey::Escape));
    assert_eq!(commands.get(), 0, "the enclosing control saw neither key");
    assert!(field.has_primary_focus(), "and the field keeps the focus");
}

/// Space pressed on one button and released after Tab moved the focus to the
/// next activates neither: the release belongs to the press, not to the
/// focus. A fresh press on the next button activates it once.
pub(crate) fn a_space_released_after_tab_activates_nothing() {
    let (first, second) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let (counted_first, counted_second) = (Rc::clone(&first), Rc::clone(&second));
    let harness = mount(Row::new((
        RawButton::new(SizedBox::new(40.0, 40.0))
            .on_press(move |_cx| counted_first.set(counted_first.get() + 1)),
        RawButton::new(SizedBox::new(40.0, 40.0))
            .on_press(move |_cx| counted_second.set(counted_second.get() + 1)),
    )));
    let manager = harness.focus_manager();
    let space = Key::character(" ");
    assert!(tab(&manager), "Tab focuses the first button");
    let pressed = manager.primary_focus().expect("the first button");
    let _ = down(&manager, &space);
    assert!(tab(&manager), "Tab moves on while Space is held");
    assert!(!Rc::ptr_eq(
        &pressed,
        &manager.primary_focus().expect("the second button")
    ));
    up(&manager, &space);
    assert_eq!(
        (first.get(), second.get()),
        (0, 0),
        "the release activates neither button"
    );
    let _ = press(&manager, &space);
    assert_eq!((first.get(), second.get()), (0, 1), "a fresh press does");
}

/// A panic in a button's press or in a focus listener reaches the caller of
/// that dispatch; the next Tab still moves the focus and the next button still
/// activates.
pub(crate) fn keyboard_continues_after_callback_panic() {
    // A press that panics.
    let activations = Rc::new(Cell::new(0));
    let counted = Rc::clone(&activations);
    let harness = mount(Row::new((
        RawButton::new(SizedBox::new(40.0, 40.0))
            .on_press(|_cx| -> () { panic!("on_press failed") }),
        RawButton::new(SizedBox::new(40.0, 40.0))
            .on_press(move |_cx| counted.set(counted.get() + 1)),
    )));
    let manager = harness.focus_manager();
    let enter = Key::Named(NamedKey::Enter);
    assert!(tab(&manager));
    let first = manager
        .primary_focus()
        .expect("Tab focused the first button");
    // Whether the panic reaches this caller is the runtime boundary's
    // business; what is pinned is the next operation.
    drop(catch_unwind(AssertUnwindSafe(|| down(&manager, &enter))));
    up(&manager, &enter);
    assert!(
        tab(&manager),
        "Tab after the panicking press moves the focus"
    );
    let second = manager.primary_focus().expect("the second button");
    assert!(!Rc::ptr_eq(&first, &second));
    assert!(press(&manager, &enter));
    assert_eq!(activations.get(), 1, "and the next button activates");

    // A focus listener that panics on gaining focus.
    let failing = FocusNode::with_debug_label("panicking listener");
    let next = FocusNode::with_debug_label("after the panicking listener");
    let harness = mount(Row::new((
        Focus::new(SizedBox::new(40.0, 40.0))
            .focus_node(Rc::clone(&failing))
            .on_focus_change(|_cx, focused| assert!(!focused, "focus listener failed")),
        Focus::new(SizedBox::new(40.0, 40.0)).focus_node(Rc::clone(&next)),
    )));
    let manager = harness.focus_manager();
    drop(catch_unwind(AssertUnwindSafe(|| tab(&manager))));
    up(&manager, &Key::Named(NamedKey::Tab));
    assert!(
        failing.has_primary_focus(),
        "the focus moved before its listener failed"
    );
    assert!(tab(&manager), "the next Tab is handled");
    assert!(next.has_primary_focus(), "and moves the focus on");
}
