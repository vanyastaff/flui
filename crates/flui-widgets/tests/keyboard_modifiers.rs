//! Observed modifiers and composition ownership through mounted keyboard consumers.
use flui_interaction::focus::FocusNode;
use flui_platform::{
    EventTime, PlatformInput,
    keyboard::{Code, ImeComposition, Key, KeyEvent, KeyRepeat, KeyState, Modifiers, NamedKey},
};
use flui_testing::{HeadlessHost, HeadlessWindow};
use flui_view::prelude::*;
use flui_widgets::interaction::{
    Actions, CallbackAction, CallbackShortcuts, Focus, Intent, Shortcuts, SingleActivator,
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
        Self::with_boundary(text_field, activator, false)
    }

    fn with_boundary(text_field: bool, activator: SingleActivator, callback: bool) -> Self {
        let controller = TextEditingController::new();
        let node = FocusNode::with_debug_label("modifier consumer");
        let commands = Rc::new(Cell::new(0));
        let observed = Rc::clone(&commands);
        let mut host = HeadlessHost::new(HeadlessWindow::new(400, 100));
        let child = if text_field {
            EditableText::new(controller.clone(), Rc::clone(&node))
                .into_view()
                .boxed()
        } else {
            Focus::new(SizedBox::new(10.0, 10.0))
                .focus_node(Rc::clone(&node))
                .into_view()
                .boxed()
        };
        if callback {
            let view = CallbackShortcuts::new(child).binding(activator, move |_| {
                observed.set(observed.get() + 1);
            });
            host.attach(&view).expect("fresh host");
        } else {
            let view = Shortcuts::new(Actions::new(child).action(CallbackAction::new(
                move |_, _: &Command| observed.set(observed.get() + 1),
            )))
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

pub(crate) fn explicit_altgraph_character_reaches_text_instead_of_a_command() {
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

pub(crate) fn altgraph_preserves_glyph_shortcuts_without_claiming_ctrl_alt_commands() {
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

pub(crate) fn ctrl_alt_fallback_depends_on_whether_a_text_field_is_focused() {
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

pub(crate) fn locks_release_repeat_and_next_operation_preserve_existing_behavior() {
    for locks in [
        Modifiers::NONE,
        Modifiers::CAPS_LOCK,
        Modifiers::NUM_LOCK,
        Modifiers::SCROLL_LOCK,
        Modifiers::FN_LOCK,
        Modifiers::SYMBOL_LOCK,
        Modifiers::FN | Modifiers::SYMBOL,
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

pub(crate) fn lock_states_do_not_disable_slider_navigation() {
    use crate::common::{lay_out, tight};
    use flui_platform::keyboard::NamedKey;
    use flui_rendering::semantics::NumericRange;
    use flui_widgets::Slider;
    use std::cell::RefCell;
    let mut failures = Vec::new();
    for locks in [
        Modifiers::NONE,
        Modifiers::CAPS_LOCK,
        Modifiers::NUM_LOCK,
        Modifiers::SCROLL_LOCK,
        Modifiers::FN_LOCK,
        Modifiers::SYMBOL_LOCK,
        Modifiers::FN | Modifiers::SYMBOL,
        Modifiers::CAPS_LOCK | Modifiers::NUM_LOCK,
    ] {
        let proposals = Rc::new(RefCell::new(Vec::new()));
        let observed = Rc::clone(&proposals);
        let laid = lay_out(
            Slider::new(NumericRange::try_new(25.0, 0.0, 100.0, 10.0).expect("valid range"))
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

pub(crate) fn a_composing_key_does_not_fire_shortcuts() {
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
        Key::Named(flui_platform::keyboard::NamedKey::Enter),
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

pub(crate) fn both_shortcut_boundaries_preserve_meta_shift_and_full_glyphs() {
    let aliases = Modifiers::ALT_GRAPH | Modifiers::CONTROL | Modifiers::ALT;
    for callback in [false, true] {
        for (name, activator, modifiers, field, commands, text) in [
            (
                "glyph",
                SingleActivator::character("का"),
                aliases,
                false,
                1,
                "",
            ),
            (
                "alias command",
                SingleActivator::character("का").control(),
                aliases,
                false,
                0,
                "",
            ),
            (
                "field full glyph",
                SingleActivator::character("का").control(),
                aliases,
                true,
                0,
                "का",
            ),
            (
                "Meta priority",
                SingleActivator::character("का").meta(),
                aliases | Modifiers::META,
                true,
                1,
                "",
            ),
            (
                "exact Shift",
                SingleActivator::character("का"),
                aliases | Modifiers::SHIFT,
                false,
                0,
                "",
            ),
            (
                "required Shift",
                SingleActivator::character("का").shift(),
                aliases | Modifiers::SHIFT,
                false,
                1,
                "",
            ),
            (
                "glyph ignores Shift",
                SingleActivator::character_ignoring_shift("का"),
                aliases | Modifiers::SHIFT,
                false,
                1,
                "",
            ),
        ] {
            let fixture = Fixture::with_boundary(field, activator, callback);
            fixture.dispatch("का", modifiers, KeyState::Down, KeyRepeat::First);
            assert_eq!(
                fixture.commands.get(),
                commands,
                "{name}, callback={callback}"
            );
            assert_eq!(
                fixture.controller.text(),
                text,
                "{name}, callback={callback}"
            );
        }
        let fixture =
            Fixture::with_boundary(false, SingleActivator::character("s").control(), callback);
        let composing = KeyEvent::new(
            KeyState::Down,
            Key::character("s"),
            Code::KeyS,
            EventTime::from_nanos(1),
        )
        .with_modifiers(Modifiers::CONTROL | Modifiers::CAPS_LOCK)
        .with_composition(ImeComposition::Active);
        fixture.host.dispatch(PlatformInput::Keyboard(composing));
        assert_eq!(
            fixture.commands.get(),
            0,
            "source owns composing shortcut, callback={callback}"
        );
        fixture.dispatch(
            "S",
            Modifiers::CONTROL | Modifiers::CAPS_LOCK,
            KeyState::Down,
            KeyRepeat::First,
        );
        assert_eq!(
            fixture.commands.get(),
            1,
            "next command, callback={callback}"
        );
        let fixture = Fixture::with_boundary(
            false,
            SingleActivator::character("का").allow_repeats(false),
            callback,
        );
        fixture.dispatch("का", aliases, KeyState::Down, KeyRepeat::First);
        fixture.dispatch("का", aliases, KeyState::Down, KeyRepeat::AutoRepeat);
        fixture.dispatch("का", aliases, KeyState::Up, KeyRepeat::First);
        assert_eq!(
            fixture.commands.get(),
            1,
            "excluded repeat and Up, callback={callback}"
        );
        fixture.dispatch("का", aliases, KeyState::Down, KeyRepeat::First);
        assert_eq!(
            fixture.commands.get(),
            2,
            "next first Down, callback={callback}"
        );
    }
}

pub(crate) fn altgraph_word_aliases_preserve_character_movement() {
    for (key, modifiers, selection, text) in [
        (
            NamedKey::ArrowLeft,
            Modifiers::ALT_GRAPH | Modifiers::CONTROL,
            5..5,
            "one two",
        ),
        (
            NamedKey::ArrowLeft,
            Modifiers::ALT_GRAPH | Modifiers::ALT,
            5..5,
            "one two",
        ),
        (
            NamedKey::ArrowLeft,
            Modifiers::ALT_GRAPH | Modifiers::CONTROL | Modifiers::SHIFT,
            5..6,
            "one two",
        ),
        (
            NamedKey::ArrowRight,
            Modifiers::ALT_GRAPH | Modifiers::CONTROL,
            7..7,
            "one two",
        ),
        (
            NamedKey::Backspace,
            Modifiers::ALT_GRAPH | Modifiers::CONTROL,
            5..5,
            "one to",
        ),
        (
            NamedKey::Delete,
            Modifiers::ALT_GRAPH | Modifiers::CONTROL,
            6..6,
            "one tw",
        ),
    ] {
        let fixture = Fixture::new(true, SingleActivator::character("s").control());
        fixture.controller.insert_str("one two");
        fixture.controller.set_caret_byte_offset(6);
        fixture.host.dispatch(PlatformInput::Keyboard(
            KeyEvent::new(
                KeyState::Down,
                Key::Named(key),
                Code::Unidentified,
                EventTime::from_nanos(1),
            )
            .with_modifiers(modifiers | Modifiers::NUM_LOCK),
        ));
        assert_eq!(
            fixture.controller.text(),
            text,
            "{key:?} with {modifiers:?}"
        );
        assert_eq!(
            fixture.controller.selection(),
            selection,
            "{key:?} with {modifiers:?}"
        );
    }
}

pub(crate) fn source_composition_preserves_editing_and_submit() {
    use crate::common::harness::mount;
    for key in [
        Key::character("का"),
        Key::Named(NamedKey::Backspace),
        Key::Named(NamedKey::Delete),
        Key::Named(NamedKey::ArrowLeft),
        Key::Named(NamedKey::ArrowRight),
        Key::Named(NamedKey::Home),
        Key::Named(NamedKey::End),
        Key::Named(NamedKey::Enter),
    ] {
        let controller = TextEditingController::with_text("seed");
        controller.set_caret_byte_offset(2);
        let node = FocusNode::with_debug_label("source composition field");
        let submitted = Rc::new(Cell::new(0));
        let observed = Rc::clone(&submitted);
        let harness = mount(
            EditableText::new(controller.clone(), Rc::clone(&node))
                .on_submitted(move |_, _| observed.set(observed.get() + 1)),
        );
        let _ = node.request_focus();
        let manager = harness.focus_manager();
        let event = KeyEvent::new(
            KeyState::Down,
            key.clone(),
            Code::Unidentified,
            EventTime::from_nanos(1),
        )
        .with_composition(ImeComposition::Active);
        assert!(
            !manager.dispatch_key_event(&event).is_handled(),
            "native composition default for {key:?}"
        );
        assert_eq!(controller.text(), "seed", "{key:?}");
        assert_eq!(controller.selection(), 2..2, "{key:?}");
        assert_eq!(submitted.get(), 0, "{key:?}");
        assert!(
            manager
                .dispatch_key_event(&KeyEvent::new(
                    KeyState::Down,
                    Key::character("z"),
                    Code::KeyZ,
                    EventTime::from_nanos(2)
                ))
                .is_handled()
        );
        assert_eq!(controller.text(), "sezed", "healthy next input for {key:?}");
    }
    // A source flag does not replace the field's independently owned IME range.
    let controller = TextEditingController::new();
    let node = FocusNode::with_debug_label("controller-owned composition");
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&node),
    ));
    let _ = node.request_focus();
    harness.dispatch_ime(&flui_platform::ImeEvent::Preedit {
        text: "ni".into(),
        cursor: Some((0, 2)),
    });
    assert_eq!(controller.composing_range(), Some(0..2));
    let event = KeyEvent::new(
        KeyState::Down,
        Key::character("x"),
        Code::KeyX,
        EventTime::from_nanos(1),
    )
    .with_composition(ImeComposition::Inactive);
    assert!(
        harness
            .focus_manager()
            .dispatch_key_event(&event)
            .is_handled()
    );
    assert_eq!(controller.text(), "ni");
    assert_eq!(
        controller.composing_range(),
        Some(0..2),
        "Inactive source cannot cancel controller composition"
    );
    harness.dispatch_ime(&flui_platform::ImeEvent::Commit("你".into()));
    assert_eq!(controller.text(), "你");
    assert!(!controller.is_composing());
    assert!(
        harness
            .focus_manager()
            .dispatch_key_event(&event)
            .is_handled()
    );
    assert_eq!(
        controller.text(),
        "你x",
        "normal typing resumes after real commit"
    );
}

pub(crate) fn source_composition_leaves_traversal_and_activation_unclaimed() {
    use crate::common::harness::mount;
    use flui_widgets::{ActivateIntent, Row};
    for key in [
        Key::Named(NamedKey::Tab),
        Key::Named(NamedKey::Enter),
        Key::character(" "),
    ] {
        let left = FocusNode::with_debug_label("source composition left");
        let right = FocusNode::with_debug_label("source composition right");
        let activations = Rc::new(Cell::new(0));
        let observed = Rc::clone(&activations);
        let harness = mount(
            Actions::new(Row::new((
                Focus::new(SizedBox::new(20.0, 20.0)).focus_node(Rc::clone(&left)),
                Focus::new(SizedBox::new(20.0, 20.0)).focus_node(Rc::clone(&right)),
            )))
            .action(CallbackAction::new(move |_, _: &ActivateIntent| {
                observed.set(observed.get() + 1);
            })),
        );
        let _ = left.request_focus();
        let manager = harness.focus_manager();
        let event = KeyEvent::new(
            KeyState::Down,
            key.clone(),
            Code::Unidentified,
            EventTime::from_nanos(1),
        );
        assert!(
            !manager
                .dispatch_key_event(&event.clone().with_composition(ImeComposition::Active))
                .is_handled(),
            "native default for {key:?}"
        );
        assert!(
            left.has_primary_focus(),
            "composition cannot traverse: {key:?}"
        );
        assert_eq!(activations.get(), 0, "composition cannot activate: {key:?}");
        // Space activates on its release, so the healthy press is both halves.
        let pressed = manager.dispatch_key_event(&event).is_handled();
        let released = manager
            .dispatch_key_event(&KeyEvent::new(
                KeyState::Up,
                key.clone(),
                Code::Unidentified,
                EventTime::from_nanos(2),
            ))
            .is_handled();
        assert!(pressed || released, "healthy next key: {key:?}");
        if key == Key::Named(NamedKey::Tab) {
            assert!(right.has_primary_focus());
        } else {
            assert_eq!(activations.get(), 1);
        }
    }
}

pub(crate) fn slider_glyph_and_composition_policy() {
    use crate::common::{lay_out, tight};
    use flui_rendering::semantics::NumericRange;
    use flui_widgets::Slider;
    use std::cell::RefCell;
    let aliases = Modifiers::ALT_GRAPH | Modifiers::CONTROL | Modifiers::ALT;
    for (key, modifiers, composition, expected) in [
        (
            Key::character("+"),
            aliases | Modifiers::SHIFT | Modifiers::SYMBOL_LOCK,
            ImeComposition::Inactive,
            Some(35.0),
        ),
        (
            Key::character("-"),
            aliases | Modifiers::FN,
            ImeComposition::Inactive,
            Some(15.0),
        ),
        (
            Key::Named(NamedKey::ArrowRight),
            aliases | Modifiers::CAPS_LOCK,
            ImeComposition::Inactive,
            Some(35.0),
        ),
        (
            Key::Named(NamedKey::ArrowRight),
            Modifiers::CONTROL,
            ImeComposition::Inactive,
            None,
        ),
        (
            Key::Named(NamedKey::ArrowRight),
            Modifiers::SHIFT,
            ImeComposition::Inactive,
            None,
        ),
        (
            Key::character("+"),
            Modifiers::NONE,
            ImeComposition::Active,
            None,
        ),
        (
            Key::Named(NamedKey::ArrowRight),
            Modifiers::NONE,
            ImeComposition::Active,
            None,
        ),
        (
            Key::Named(NamedKey::Home),
            Modifiers::CAPS_LOCK,
            ImeComposition::Active,
            None,
        ),
    ] {
        let proposals = Rc::new(RefCell::new(Vec::new()));
        let observed = Rc::clone(&proposals);
        let laid = lay_out(
            Slider::new(NumericRange::try_new(25.0, 0.0, 100.0, 10.0).expect("valid range"))
                .on_changed(move |_, value| observed.borrow_mut().push(value)),
            tight(100.0, 32.0),
        );
        laid.dispatch_pointer_down(25.0, 16.0);
        laid.dispatch_pointer_up(25.0, 16.0);
        proposals.borrow_mut().clear();
        let manager = laid.focus_manager();
        let event = KeyEvent::new(
            KeyState::Down,
            key.clone(),
            Code::Unidentified,
            EventTime::from_nanos(1),
        )
        .with_modifiers(modifiers)
        .with_composition(composition);
        assert_eq!(
            manager.dispatch_key_event(&event).is_handled(),
            expected.is_some(),
            "{key:?}, {modifiers:?}, {composition:?}"
        );
        assert_eq!(
            &*proposals.borrow(),
            &expected.into_iter().collect::<Vec<_>>()
        );
        assert!(
            !manager
                .dispatch_key_event(&KeyEvent::new(
                    KeyState::Up,
                    Key::Named(NamedKey::ArrowRight),
                    Code::ArrowRight,
                    EventTime::from_nanos(2)
                ))
                .is_handled()
        );
        assert!(
            manager
                .dispatch_key_event(&KeyEvent::new(
                    KeyState::Down,
                    Key::Named(NamedKey::ArrowRight),
                    Code::ArrowRight,
                    EventTime::from_nanos(3)
                ))
                .is_handled()
        );
        assert_eq!(
            proposals.borrow().last(),
            Some(&35.0),
            "healthy next slider key"
        );
    }
}
