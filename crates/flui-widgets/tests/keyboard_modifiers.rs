//! Observed modifiers and composition ownership through mounted keyboard consumers.
use flui_interaction::routing::FocusNode;
use flui_platform_api::{
    EventTime, PlatformInput,
    keyboard::{Code, Key, KeyEvent, KeyRepeat, KeyState, Modifiers},
};
use flui_testing::{HeadlessHost, HeadlessWindow};
use flui_widgets::interaction::{
    Actions, CallbackAction, Focus, Intent, Shortcuts, SingleActivator,
};
use flui_widgets::{EditableText, SizedBox, TextEditingController};
use std::{cell::Cell, rc::Rc, time::Duration};

struct Command;
impl Intent for Command {}

struct Fixture {
    host: HeadlessHost,
    controller: TextEditingController,
    commands: Rc<Cell<usize>>,
}

impl Fixture {
    fn new(text_field: bool, activator: SingleActivator) -> Self {
        let controller = TextEditingController::new();
        let node = FocusNode::with_debug_label("modifier consumer");
        let commands = Rc::new(Cell::new(0));
        let observed = Rc::clone(&commands);
        let mut host = HeadlessHost::new(HeadlessWindow::new(400, 100));
        if text_field {
            let view = Shortcuts::new(
                Actions::new(EditableText::new(controller.clone(), Rc::clone(&node))).action(
                    CallbackAction::new(move |_, _: &Command| observed.set(observed.get() + 1)),
                ),
            )
            .shortcut(activator, Command);
            host.attach(&view).expect("fresh host");
        } else {
            let view = Shortcuts::new(
                Actions::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&node)))
                    .action(CallbackAction::new(move |_, _: &Command| {
                        observed.set(observed.get() + 1)
                    })),
            )
            .shortcut(activator, Command);
            host.attach(&view).expect("fresh host");
        }
        let _ = host.pump(Duration::ZERO);
        let _ = node.request_focus();
        let _ = host.pump(Duration::ZERO);
        assert!(node.has_primary_focus(), "actual mounted primary focus");
        Self {
            host,
            controller,
            commands,
        }
    }

    fn dispatch(&self, text: &str, modifiers: Modifiers, state: KeyState, repeat: KeyRepeat) {
        self.host.dispatch(PlatformInput::Keyboard(
            KeyEvent::new(
                state,
                Key::character(text),
                Code::KeyQ,
                EventTime::from_nanos(1),
            )
            .with_modifiers(modifiers)
            .with_repeat(repeat),
        ));
    }
}

#[test]
fn explicit_altgraph_character_reaches_text_instead_of_a_command() {
    let mut failures = Vec::new();
    let mut default_failures = Vec::new();
    for (name, modifiers) in [
        ("AltGraph only", Modifiers::ALT_GRAPH),
        (
            "AltGraph with Control",
            Modifiers::ALT_GRAPH | Modifiers::CONTROL,
        ),
        (
            "AltGraph with Control and Alt",
            Modifiers::ALT_GRAPH | Modifiers::CONTROL | Modifiers::ALT,
        ),
    ] {
        let fixture = Fixture::new(true, SingleActivator::character("@").control());
        fixture.dispatch("@", modifiers, KeyState::Down, KeyRepeat::First);
        let actual = (fixture.controller.text(), fixture.commands.get());
        eprintln!("{name}: text={:?}, commands={}", actual.0, actual.1);
        if actual != ("@".into(), 0) {
            failures.push((name, actual));
        }
        let field = Fixture::new(true, SingleActivator::character("@").control());
        field.controller.insert_str("seed");
        field.dispatch("a", modifiers, KeyState::Down, KeyRepeat::First);
        let after_a = field.controller.text();
        let selected = field.controller.has_selection();
        field.dispatch("z", Modifiers::NONE, KeyState::Down, KeyRepeat::First);
        let after_z = field.controller.text();
        eprintln!(
            "default select-all {name}: text={after_a:?}, selected={selected}, next={after_z:?}"
        );
        if after_a != "seeda" || selected || after_z != "seedaz" {
            default_failures.push((name, after_a, selected, after_z));
        }
    }
    assert!(
        failures.is_empty() && default_failures.is_empty(),
        "explicit-AltGraph text policy: authored={failures:?}, default={default_failures:?}"
    );
}

#[test]
fn altgraph_preserves_glyph_shortcuts_without_claiming_ctrl_alt_commands() {
    let mut failures = Vec::new();
    for (name, activator, modifiers) in [
        (
            "bare character",
            SingleActivator::character("@"),
            Modifiers::ALT_GRAPH,
        ),
        (
            "Control character",
            SingleActivator::character("@").control(),
            Modifiers::ALT_GRAPH | Modifiers::CONTROL,
        ),
        (
            "Control Alt character",
            SingleActivator::character("@").control().alt(),
            Modifiers::ALT_GRAPH | Modifiers::CONTROL | Modifiers::ALT,
        ),
    ] {
        let fixture = Fixture::new(false, activator);
        fixture.dispatch("@", modifiers, KeyState::Down, KeyRepeat::First);
        eprintln!("{name}: commands={}", fixture.commands.get());
        let expected = usize::from(name == "bare character");
        if fixture.commands.get() != expected {
            failures.push((name, fixture.commands.get()));
        }
    }
    assert!(
        failures.is_empty(),
        "AltGraph glyph and command policy: {failures:?}"
    );
}

#[test]
fn ctrl_alt_fallback_depends_on_whether_a_text_field_is_focused() {
    for field in [false, true] {
        let fixture = Fixture::new(field, SingleActivator::character("s").control().alt());
        fixture.dispatch(
            "s",
            Modifiers::CONTROL | Modifiers::ALT,
            KeyState::Down,
            KeyRepeat::First,
        );
        assert_eq!(fixture.controller.text(), if field { "s" } else { "" });
        assert_eq!(fixture.commands.get(), usize::from(!field));
        eprintln!(
            "Ctrl+Alt+S text_field={field}: text={:?}, commands={}",
            fixture.controller.text(),
            fixture.commands.get()
        );
    }
}

#[test]
fn locks_release_repeat_and_next_operation_preserve_existing_behavior() {
    for locks in [
        Modifiers::NONE,
        Modifiers::CAPS_LOCK,
        Modifiers::NUM_LOCK,
        Modifiers::SCROLL_LOCK,
        Modifiers::FN_LOCK,
        Modifiers::SYMBOL_LOCK,
        Modifiers::CAPS_LOCK | Modifiers::NUM_LOCK | Modifiers::SCROLL_LOCK,
    ] {
        let fixture = Fixture::new(
            true,
            SingleActivator::character("s")
                .control()
                .allow_repeats(false),
        );
        fixture.dispatch(
            "S",
            Modifiers::CONTROL | locks,
            KeyState::Down,
            KeyRepeat::First,
        );
        assert_eq!(
            fixture.commands.get(),
            1,
            "locks do not defeat the existing command"
        );
        assert_eq!(fixture.controller.text(), "", "command never inserts text");
        fixture.dispatch(
            "S",
            Modifiers::CONTROL | locks,
            KeyState::Up,
            KeyRepeat::AutoRepeat,
        );
        fixture.dispatch(
            "S",
            Modifiers::CONTROL | locks,
            KeyState::Down,
            KeyRepeat::AutoRepeat,
        );
        assert_eq!(
            fixture.commands.get(),
            1,
            "release and excluded repeat do not invoke"
        );
        fixture.dispatch(
            "q",
            Modifiers::ALT_GRAPH | Modifiers::CONTROL | Modifiers::ALT | locks,
            KeyState::Down,
            KeyRepeat::First,
        );
        fixture.dispatch(
            "q",
            Modifiers::ALT_GRAPH | Modifiers::CONTROL | Modifiers::ALT | locks,
            KeyState::Down,
            KeyRepeat::AutoRepeat,
        );
        fixture.dispatch(
            "q",
            Modifiers::ALT_GRAPH | locks,
            KeyState::Up,
            KeyRepeat::First,
        );
        fixture.dispatch("z", locks, KeyState::Down, KeyRepeat::First);
        fixture.dispatch(
            "S",
            Modifiers::CONTROL | locks,
            KeyState::Down,
            KeyRepeat::First,
        );
        assert_eq!(
            fixture.controller.text(),
            "qqz",
            "two text Downs, no Up insertion, healthy plain text"
        );
        assert_eq!(
            fixture.commands.get(),
            2,
            "next healthy command still arrives"
        );
    }
}

#[test]
fn lock_states_do_not_disable_slider_navigation() {
    use crate::common::{lay_out, tight};
    use flui_platform_api::keyboard::NamedKey;
    use flui_rendering::semantics::NumericRange;
    use flui_widgets::Slider;
    use std::cell::RefCell;
    let mut failures = Vec::new();
    for locks in [
        Modifiers::NONE,
        Modifiers::CAPS_LOCK,
        Modifiers::NUM_LOCK,
        Modifiers::SCROLL_LOCK,
        Modifiers::CAPS_LOCK | Modifiers::NUM_LOCK,
    ] {
        let proposals = Rc::new(RefCell::new(Vec::new()));
        let observed = Rc::clone(&proposals);
        let laid = lay_out(
            Slider::new(NumericRange::new(25.0, 0.0, 100.0, 10.0).expect("valid range"))
                .on_changed(move |_, value| observed.borrow_mut().push(value)),
            tight(100.0, 32.0),
        );
        laid.dispatch_pointer_down(25.0, 16.0);
        laid.dispatch_pointer_up(25.0, 16.0);
        assert!(
            laid.focus_manager().primary_focus().is_some(),
            "actual pointer focus"
        );
        proposals.borrow_mut().clear();
        let event = KeyEvent::new(
            KeyState::Down,
            Key::Named(NamedKey::ArrowRight),
            Code::ArrowRight,
            EventTime::from_nanos(1),
        )
        .with_modifiers(locks);
        let handled = laid.focus_manager().dispatch_key_event(&event).is_handled();
        let values = proposals.borrow().clone();
        eprintln!("slider {locks:?}: handled={handled}, proposals={values:?}");
        if !handled || values != [35.0] {
            failures.push((locks, handled, values));
        }
    }
    assert!(
        failures.is_empty(),
        "incidental locks must not disable a navigation key: {failures:?}"
    );
}

#[test]
fn a_composing_key_does_not_fire_shortcuts() {
    use flui_platform_api::keyboard::ImeComposition;
    let mut failures = Vec::new();
    for field in [false, true] {
        let fixture = Fixture::new(field, SingleActivator::character("s").control());
        fixture.host.dispatch(PlatformInput::Keyboard(
            KeyEvent::new(
                KeyState::Down,
                Key::character("s"),
                Code::KeyS,
                EventTime::from_nanos(1),
            )
            .with_modifiers(Modifiers::CONTROL)
            .with_composition(ImeComposition::Active),
        ));
        eprintln!(
            "composing key field={field}: commands={}",
            fixture.commands.get()
        );
        if fixture.commands.get() != 0 {
            failures.push((field, fixture.commands.get()));
        }
        let before = fixture.commands.get();
        fixture.dispatch("s", Modifiers::CONTROL, KeyState::Down, KeyRepeat::First);
        assert_eq!(
            fixture.commands.get(),
            before + 1,
            "next noncomposing command arrives"
        );
    }
    let mut field_failures = Vec::new();
    for key in [
        Key::character("x"),
        Key::Named(flui_platform_api::keyboard::NamedKey::Enter),
    ] {
        let controller = TextEditingController::with_text("seed");
        let node = FocusNode::with_debug_label("composing-source field");
        let submitted = Rc::new(Cell::new(0));
        let observed = Rc::clone(&submitted);
        let view = EditableText::new(controller.clone(), Rc::clone(&node))
            .on_submitted(move |_, _| observed.set(observed.get() + 1));
        let mut host = HeadlessHost::new(HeadlessWindow::new(400, 100));
        host.attach(&view).expect("fresh host");
        let _ = host.pump(Duration::ZERO);
        let _ = node.request_focus();
        assert!(node.has_primary_focus());
        host.dispatch(PlatformInput::Keyboard(
            KeyEvent::new(
                KeyState::Down,
                key.clone(),
                Code::Unidentified,
                EventTime::from_nanos(1),
            )
            .with_composition(ImeComposition::Active),
        ));
        let actual = (controller.text(), submitted.get());
        eprintln!(
            "composing-source field {key:?}: text={:?}, submitted={}",
            actual.0, actual.1
        );
        if actual != ("seed".into(), 0) {
            field_failures.push((key, actual));
        }
        let before = controller.text();
        host.dispatch(PlatformInput::Keyboard(KeyEvent::new(
            KeyState::Down,
            Key::character("z"),
            Code::KeyZ,
            EventTime::from_nanos(2),
        )));
        assert_eq!(
            controller.text(),
            format!("{before}z"),
            "next noncomposing input arrives"
        );
    }
    assert!(
        failures.is_empty() && field_failures.is_empty(),
        "KeyEvent composition ownership contract: shortcuts={failures:?}, fields={field_failures:?}"
    );
}
