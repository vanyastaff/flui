//! The Windows lane's tables, on every host: keys, roles, the safety rules and their codes, what
//! the guard takes for the person's input, touch and pen frames, screen mapping, and the
//! projection of a tree.

use std::time::Duration;

use flui_driver::coordinates::{DevicePoint, to_device};
use flui_driver::lane::pacing;
use flui_driver::lane::walk::{self, Properties, Provider, ProviderError};
use flui_driver::lane::windows::activity::{self, LANE_TAG, MouseMotion, Observed};
use flui_driver::lane::windows::contacts::{Contacts, Frame, Phase, PointerKind};
use flui_driver::lane::windows::held::{HeldButtons, HeldKeys, KeyInput, Keystroke, MASK};
use flui_driver::lane::windows::keys::{self, KeyName, Modifier, TextStroke, VirtualKey};
use flui_driver::lane::windows::roles::{self, ControlType, ROLES, RoleFlags};
use flui_driver::lane::windows::safety::{self, Desk, Held, Landing, Refusal, check_event};
use flui_driver::lane::windows::screen;
use flui_driver::lane::{release_outcome, windows};
use flui_driver::projection::{Projection, ProjectionDifference};
use flui_driver::protocol::error::ErrorCode;
use flui_driver::protocol::geometry::{LogicalPoint, LogicalSize};
use flui_driver::protocol::semantics::SemanticsRole;
use flui_driver::protocol::session::Isolation;
use flui_driver::protocol::tree::{ElementId, Node, OUTLINE_TEXT_LIMIT, Tree, normalized_outline};
use flui_driver::protocol::wire::Role;

fn virtual_key(code: u16, extended: bool) -> KeyName {
    KeyName::Virtual {
        key: VirtualKey { code, extended },
        shift: false,
    }
}

#[test]
fn a_key_name_maps_to_its_virtual_key_or_to_the_layout() {
    let rows: &[(&str, Option<KeyName>)] = &[
        ("Enter", Some(virtual_key(0x0D, false))),
        ("Tab", Some(virtual_key(0x09, false))),
        ("Backspace", Some(virtual_key(0x08, false))),
        ("Escape", Some(virtual_key(0x1B, false))),
        ("Shift", Some(virtual_key(0x10, false))),
        ("Control", Some(virtual_key(0x11, false))),
        ("Alt", Some(virtual_key(0x12, false))),
        ("Meta", Some(virtual_key(0x5B, true))),
        ("ArrowLeft", Some(virtual_key(0x25, true))),
        ("ArrowDown", Some(virtual_key(0x28, true))),
        ("Home", Some(virtual_key(0x24, true))),
        ("Delete", Some(virtual_key(0x2E, true))),
        ("F1", Some(virtual_key(0x70, false))),
        ("F24", Some(virtual_key(0x87, false))),
        (" ", Some(virtual_key(0x20, false))),
        ("a", Some(virtual_key(0x41, false))),
        ("z", Some(virtual_key(0x5A, false))),
        ("0", Some(virtual_key(0x30, false))),
        (
            "A",
            Some(KeyName::Virtual {
                key: VirtualKey {
                    code: 0x41,
                    extended: false,
                },
                shift: true,
            }),
        ),
        ("+", Some(KeyName::Character('+'))),
        ("é", Some(KeyName::Character('é'))),
        ("Enterprise", None),
        ("", None),
    ];
    for (name, expected) in rows {
        assert_eq!(keys::resolve(name), *expected, "{name:?}");
    }
    assert!(keys::is_extended(0x25) && !keys::is_extended(0x41));
    assert_eq!(Modifier::of(keys::SHIFT), Some(Modifier::Shift));
    assert_eq!(Modifier::of(keys::META), Some(Modifier::Meta));
    assert_eq!(Modifier::of(keys::ENTER), None);
}

#[test]
fn text_is_typed_as_packets_and_line_breaks_as_keys() {
    let strokes = keys::text_strokes("a\r\nb\n\tc𝄞");
    let expected = [
        TextStroke::Unit(u16::from(b'a')),
        TextStroke::Key(keys::ENTER),
        TextStroke::Unit(u16::from(b'b')),
        TextStroke::Key(keys::ENTER),
        TextStroke::Key(keys::TAB),
        TextStroke::Unit(u16::from(b'c')),
        // A character past the basic plane is its two surrogate units.
        TextStroke::Unit(0xD834),
        TextStroke::Unit(0xDD1E),
    ];
    assert_eq!(strokes, expected);
    assert_eq!(keys::text_strokes("\r"), [TextStroke::Key(keys::ENTER)]);
}

/// The table was transcribed from the adapter the workspace locks: a bump fails here until the
/// table is read against the new one.
#[test]
fn the_role_table_was_read_against_the_locked_adapter() {
    let lock = include_str!("../../../Cargo.lock");
    let locked = lock
        .split("[[package]]")
        .find(|entry| entry.contains("name = \"accesskit_windows\""))
        .and_then(|entry| {
            entry
                .lines()
                .find_map(|line| line.strip_prefix("version = \""))
                .map(|version| version.trim_end_matches('"'))
        });
    assert_eq!(locked, Some(roles::ADAPTER_CHECKED_AGAINST));
}

/// One row for each role that publishes a node of its own, in vocabulary order.
#[test]
fn every_role_that_publishes_a_node_has_a_row_in_vocabulary_order() {
    let published: Vec<SemanticsRole> = SemanticsRole::ALL
        .iter()
        .copied()
        .filter(|role| {
            !matches!(
                role,
                SemanticsRole::None | SemanticsRole::Generic | SemanticsRole::Presentation
            )
        })
        .collect();
    let table: Vec<SemanticsRole> = ROLES.iter().map(|row| row.role).collect();
    assert_eq!(table, published);
    assert!(roles::role_row(SemanticsRole::None).is_none());
}

#[test]
fn a_control_type_reads_back_from_its_id() {
    assert_eq!(ControlType::from_id(50_000), Some(ControlType::Button));
    assert_eq!(ControlType::from_id(50_032), Some(ControlType::Window));
    assert_eq!(ControlType::from_id(50_040), Some(ControlType::AppBar));
    assert_eq!(ControlType::from_id(49_999), None);
    assert_eq!(ControlType::from_id(50_041), None);
    assert_eq!(ControlType::from_id(i32::MIN), None);
    for id in 50_000..=50_040 {
        let control = ControlType::from_id(id).expect("every id in the range names a type");
        assert_eq!(control.id(), id, "{}", control.name());
    }
}

#[test]
fn a_control_type_folds_to_its_wire_role_with_its_aria_role_and_flags() {
    let none = RoleFlags::default();
    let rows: &[(ControlType, &str, RoleFlags, Role)] = &[
        (ControlType::Button, "button", none, Role::Button),
        (ControlType::Button, "switch", none, Role::Switch),
        (ControlType::Text, "heading", none, Role::Heading),
        (ControlType::Text, "", none, Role::Label),
        (ControlType::DataItem, "row", none, Role::Row),
        (ControlType::DataItem, "cell", none, Role::Cell),
        (
            ControlType::DataItem,
            "columnheader",
            none,
            Role::ColumnHeader,
        ),
        (ControlType::Edit, "textbox", none, Role::TextInput),
        (
            ControlType::Edit,
            "textbox",
            RoleFlags::new(true, false),
            Role::PasswordInput,
        ),
        (ControlType::Window, "region", none, Role::Window),
        (
            ControlType::Window,
            "dialog",
            RoleFlags::new(false, true),
            Role::Dialog,
        ),
        (ControlType::TitleBar, "", none, Role::TitleBar),
        (ControlType::Custom, "", none, Role::Unknown),
    ];
    for (control, aria, flags, expected) in rows {
        assert_eq!(
            roles::wire_role(*control, aria, *flags),
            *expected,
            "{} {aria:?}",
            control.name()
        );
    }
}

fn desk() -> Desk {
    Desk::quiet()
}

fn on_target() -> Landing {
    Landing::new(DevicePoint::new(10, 10), true, None)
}

#[test]
fn each_safety_rule_refuses_with_its_code() {
    let held_shift = Held {
        keys: vec![keys::SHIFT.code],
        ..Held::default()
    };
    /// A row: its name, the desktop, where the event lands, what the lane holds, the refusal.
    type Row = (&'static str, Desk, Option<Landing>, Held, Option<Refusal>);
    let rows: Vec<Row> = vec![
        (
            "a quiet desktop",
            desk(),
            Some(on_target()),
            Held::default(),
            None,
        ),
        (
            "a locked or switched desktop",
            Desk {
                input_desktop: false,
                ..desk()
            },
            Some(on_target()),
            Held::default(),
            Some(Refusal::OffInputDesktop),
        ),
        (
            "the window is gone",
            Desk {
                window_alive: false,
                ..desk()
            },
            None,
            Held::default(),
            Some(Refusal::WindowGone),
        ),
        (
            "another window in front",
            Desk {
                in_front: false,
                foreground: Some("\"Notepad\"".into()),
                ..desk()
            },
            None,
            Held::default(),
            Some(Refusal::NotForeground {
                foreground: Some("\"Notepad\"".into()),
            }),
        ),
        (
            "an overlay over the point",
            desk(),
            Some(Landing::new(
                DevicePoint::new(10, 10),
                false,
                Some("\"Overlay\"".into()),
            )),
            Held::default(),
            Some(Refusal::Covered {
                point: DevicePoint::new(10, 10),
                covered_by: Some("\"Overlay\"".into()),
            }),
        ),
        (
            "a mouse button the person holds",
            Desk {
                buttons: 0b10,
                ..desk()
            },
            Some(on_target()),
            Held::default(),
            Some(Refusal::InputHeld {
                what: "a mouse button the lane did not press is held (buttons 0b00010)".into(),
            }),
        ),
        (
            "the lane's own button is no refusal",
            Desk {
                buttons: 0b01,
                ..desk()
            },
            Some(on_target()),
            Held {
                buttons: 0b01,
                ..Held::default()
            },
            None,
        ),
        (
            "a modifier the person holds",
            Desk {
                modifiers: vec![Modifier::Control],
                ..desk()
            },
            None,
            held_shift.clone(),
            Some(Refusal::InputHeld {
                what: "Control is held, and the lane did not press it".into(),
            }),
        ),
        (
            "the lane's own modifier is no refusal",
            Desk {
                modifiers: vec![Modifier::Shift],
                ..desk()
            },
            None,
            held_shift,
            None,
        ),
        (
            "a key the person holds",
            Desk {
                other_key: Some(0x57),
                ..desk()
            },
            None,
            Held::default(),
            Some(Refusal::InputHeld {
                what: "a key (virtual key 0x57) the lane did not press is held".into(),
            }),
        ),
    ];
    for (row, desk, landing, held, expected) in rows {
        assert_eq!(
            check_event(&desk, landing.as_ref(), &held).err(),
            expected,
            "{row}"
        );
    }
    let codes: &[(Refusal, Option<ErrorCode>, bool)] = &[
        (Refusal::OffInputDesktop, None, true),
        (Refusal::WindowGone, Some(ErrorCode::Gone), false),
        (
            Refusal::NotForeground { foreground: None },
            Some(ErrorCode::NotForeground),
            false,
        ),
        (
            Refusal::Covered {
                point: DevicePoint::new(0, 0),
                covered_by: None,
            },
            Some(ErrorCode::OutsideTarget),
            false,
        ),
        (
            Refusal::InputHeld {
                what: "a key".into(),
            },
            Some(ErrorCode::InputHeld),
            false,
        ),
        (
            Refusal::ShellHotkey {
                handler: "Task Manager",
            },
            Some(ErrorCode::NotSupported),
            true,
        ),
        (
            Refusal::UnknownKey {
                name: "Hyper".into(),
            },
            Some(ErrorCode::NotSupported),
            true,
        ),
        (
            Refusal::UnsentInput {
                what: "a pen's eraser".into(),
            },
            Some(ErrorCode::NotSupported),
            true,
        ),
    ];
    for (refusal, code, cannot_verify) in codes {
        let error = refusal.clone().into_error();
        assert_eq!(error.code(), *code, "{refusal:?}");
        assert_eq!(error.is_cannot_verify(), *cannot_verify, "{refusal:?}");
    }
}

/// The shared desktop, guarded, is the one mode the lane runs in; the Windows isolation modes are
/// refused as cannot-verify until their spikes pass, and another platform's are no argument.
#[test]
fn the_lane_runs_on_the_shared_desktop_and_refuses_unproven_isolation() {
    assert_eq!(safety::admit_isolation(Isolation::Shared), Ok(()));
    for isolation in [Isolation::Sandbox, Isolation::SecondSession] {
        let refused = safety::admit_isolation(isolation).expect_err("an unproven mode");
        assert_eq!(refused, Refusal::UnprovenIsolation { isolation });
        let error = refused.into_error();
        assert!(error.is_cannot_verify(), "{error:?}");
        assert_eq!(
            error.missing_capability(),
            Some(format!("windows_isolation_{}", isolation.name()))
        );
    }
    for isolation in [Isolation::InProcess, Isolation::Xvfb, Isolation::Emulator] {
        let error = safety::admit_isolation(isolation)
            .expect_err("no Windows mode")
            .into_error();
        assert_eq!(error.code(), Some(ErrorCode::InvalidArgument), "{error:?}");
    }
}

fn key(name: &str) -> VirtualKey {
    match keys::resolve(name) {
        Some(KeyName::Virtual { key, .. }) => key,
        other => panic!("{name} is no virtual key: {other:?}"),
    }
}

fn down(name: &str) -> KeyInput {
    KeyInput::Down {
        key: key(name),
        shift: false,
    }
}

fn up(name: &str) -> KeyInput {
    KeyInput::Up { key: key(name) }
}

/// A chord as key inputs: each key pressed in order, released in reverse.
fn chord(names: &[&str]) -> Vec<KeyInput> {
    let mut inputs: Vec<KeyInput> = names.iter().map(|name| down(name)).collect();
    inputs.extend(names.iter().rev().map(|name| up(name)));
    inputs
}

/// Send `inputs` as the lane does, every stroke taken.
fn send(keys: &mut HeldKeys, inputs: &[KeyInput]) {
    for input in inputs {
        for stroke in keys.plan(input) {
            let _ = keys.apply(stroke);
        }
    }
}

#[test]
fn shell_hotkeys_are_refused_and_app_chords_are_not() {
    let shell: &[&[&str]] = &[
        &["Meta"],
        &["Meta", "r"],
        &["Control", "Escape"],
        &["Alt", "Tab"],
        &["Alt", "Shift", "Tab"],
        &["Alt", "Escape"],
        &["Control", "Shift", "Escape"],
        &["Control", "Alt", "Delete"],
        &["CapsLock"],
        &["NumLock"],
        &["PrintScreen"],
        &["AudioVolumeUp"],
    ];
    let quiet = HeldKeys::new();
    for keys in shell {
        assert!(
            quiet.shell_hotkey(&chord(keys)).is_some(),
            "{keys:?} is the shell's"
        );
    }
    let app: &[&[&str]] = &[
        &["Control", "s"],
        &["Alt", "F4"],
        &["Control", "Tab"],
        &["Escape"],
        &["Shift", "Tab"],
        &["Control", "Shift", "s"],
        &["Alt", "d"],
        &["Control", "a"],
        &["Enter"],
        &["Shift"],
        // A chord of modifiers alone is the app's too: its release is masked.
        &["Alt", "Shift"],
        &["Control", "Shift"],
    ];
    for keys in app {
        assert_eq!(
            quiet.shell_hotkey(&chord(keys)),
            None,
            "{keys:?} is the app's"
        );
    }
}

/// The strokes `inputs` take from `keys`, sent as the lane sends them, every one taken.
fn planned(keys: &mut HeldKeys, inputs: &[KeyInput]) -> Vec<Keystroke> {
    let mut strokes = Vec::new();
    for input in inputs {
        for stroke in keys.plan(input) {
            let _ = keys.apply(stroke);
            strokes.push(stroke);
        }
    }
    strokes
}

/// A release that would complete a shortcut of the shell's (the fifth Shift tapped alone in a
/// row, as the OS counts it across the lane's sends; letting go of Control+Shift or Alt+Shift, a
/// click under it included, which sends no key) is preceded by the mask key, so the app gets its
/// keys and no stroke completes the shortcut; the check refuses none of them.
#[test]
fn a_release_that_would_complete_a_shell_shortcut_is_masked() {
    let taps =
        |count: usize| -> Vec<KeyInput> { (0..count).flat_map(|_| chord(&["Shift"])).collect() };
    let masks = |strokes: &[Keystroke]| {
        strokes
            .iter()
            .filter(|stroke| **stroke == Keystroke::Press(MASK))
            .count()
    };
    let cases: [(&str, Vec<KeyInput>, usize); 5] = [
        ("four Shift taps", taps(4), 0),
        ("five Shift taps", taps(5), 1),
        ("ten Shift taps", taps(10), 2),
        ("Control+Shift", chord(&["Control", "Shift"]), 1),
        ("Alt+Shift", chord(&["Alt", "Shift"]), 1),
    ];
    for (case, inputs, expected) in cases {
        assert_eq!(HeldKeys::new().shell_hotkey(&inputs), None, "{case}");
        let strokes = planned(&mut HeldKeys::new(), &inputs);
        assert_eq!(masks(&strokes), expected, "{case}: {strokes:?}");
        assert!(
            completed_by(&[], &strokes).iter().all(Option::is_none),
            "{case}: no stroke completes a shortcut"
        );
    }
    // The mask goes right before the release that would complete the chord.
    let control_shift = planned(&mut HeldKeys::new(), &chord(&["Control", "Shift"]));
    assert_eq!(
        control_shift[2..5],
        [
            Keystroke::Press(MASK),
            Keystroke::Release(MASK),
            Keystroke::Release(key("Shift")),
        ]
    );
    // Four taps sent before, then a fifth press: the OS counts on, and Windows may take the
    // shortcut at the press, so the mask goes before the press; the press is then the run's
    // first, and its release needs no mask.
    let mut keys = HeldKeys::new();
    send(&mut keys, &taps(4));
    let press = keys.plan(&down("Shift"));
    assert_eq!(
        press,
        [
            Keystroke::Press(MASK),
            Keystroke::Release(MASK),
            Keystroke::Press(key("Shift")),
        ]
    );
    assert!(
        completed_by(&taps(4), &press).iter().all(Option::is_none),
        "the fifth press completes nothing once masked"
    );
    send(&mut keys, &[down("Shift")]);
    assert_eq!(masks(&keys.plan(&up("Shift"))), 0);
}

/// Release everything `keys` holds, recording each stroke sent, every one taken.
fn release_recorded(keys: &mut HeldKeys) -> Vec<Keystroke> {
    let mut sent = Vec::new();
    keys.release_all(|stroke| {
        sent.push(stroke);
        Ok(())
    })
    .expect("released");
    sent
}

/// The strokes of `sent`, after `before`, read stroke by stroke as the OS reads them: what each
/// completes of the shell's.
fn completed_by(before: &[KeyInput], sent: &[Keystroke]) -> Vec<Option<&'static str>> {
    let mut os = HeldKeys::new();
    send(&mut os, before);
    sent.iter().map(|stroke| os.apply(*stroke)).collect()
}

/// The shortcut check reads what the OS saw of the lane's keys from their one owner, a release
/// of everything included, and nothing the lane sends completes a shortcut of the shell's: a
/// sequence may leave a chord of modifiers down, since the release of everything breaks the chord
/// with the mask key before it lets go of a modifier.
#[test]
fn the_shortcut_check_follows_every_key_the_os_saw_a_release_of_everything_included() {
    let chords: [[&str; 2]; 3] = [["Control", "Shift"], ["Alt", "Shift"], ["Shift", "Alt"]];
    for [first, second] in chords {
        let left_down = [down(first), down(second)];
        assert_eq!(
            HeldKeys::new().shell_hotkey(&left_down),
            None,
            "{first}+{second} left down is the app's"
        );

        // A run that ends, or is interrupted, with the chord down.
        let mut keys = HeldKeys::new();
        send(&mut keys, &left_down);
        let sent = release_recorded(&mut keys);
        assert_eq!(
            sent,
            [
                Keystroke::Press(MASK),
                Keystroke::Release(MASK),
                Keystroke::Release(key(second)),
                Keystroke::Release(key(first)),
            ]
        );
        assert_eq!(
            completed_by(&left_down, &sent),
            [None; 4],
            "no stroke of the release completes a shortcut"
        );
        assert_eq!(keys.held(), [] as [u16; 0]);
    }
    assert_eq!(
        HeldKeys::new().shell_hotkey(&[down("Control"), down("Shift"), down("s")]),
        None,
        "a key pressed under the modifiers breaks the chord"
    );

    // The mask refused: no release that would complete the chord is sent, and both stay held.
    let left_down = [down("Control"), down("Shift")];
    let mut keys = HeldKeys::new();
    send(&mut keys, &left_down);
    let mut sent = Vec::new();
    let refused = keys.release_all(|stroke| {
        if stroke == Keystroke::Press(MASK) {
            return Err(flui_driver::Error::Protocol {
                message: "refused".into(),
            });
        }
        sent.push(stroke);
        Ok(())
    });
    assert!(
        matches!(&refused, Err(flui_driver::Error::ReleaseFailed { held, .. })
            if held == &["Control", "Shift"]),
        "the keys left held are named: {refused:?}"
    );
    assert_eq!(
        sent,
        [] as [Keystroke; 0],
        "nothing that completes the chord"
    );
    assert_eq!(keys.held(), [key("Control").code, key("Shift").code]);

    // Alt released by the release of everything: a Tab after the run is the app's.
    let mut keys = HeldKeys::new();
    send(&mut keys, &[down("Alt")]);
    let sent = release_recorded(&mut keys);
    assert_eq!(completed_by(&[down("Alt")], &sent), [None; 3]);
    assert_eq!(keys.shell_hotkey(&chord(&["Tab"])), None);

    // Shift pressed alone by five runs, each ended by the release of everything: the mask key
    // between the taps keeps them from adding up.
    let mut keys = HeldKeys::new();
    let mut os = HeldKeys::new();
    for run in 0..5 {
        let shift = [down("Shift")];
        assert_eq!(keys.shell_hotkey(&shift), None, "run {run}");
        send(&mut keys, &shift);
        send(&mut os, &shift);
        for stroke in release_recorded(&mut keys) {
            assert_eq!(os.apply(stroke), None, "run {run}: {stroke:?}");
        }
    }
}

/// Typed text reaches the keyboard as keys too: its Tab under a held Alt is the task switcher, and
/// the check reads it from the same plan the lane sends.
#[test]
fn typed_text_goes_through_the_shell_hotkey_check_as_the_keys_it_sends() {
    let quiet = HeldKeys::new();
    for (text, expected) in [
        ("\t", Some("the Windows task switcher")),
        ("a\tb", Some("the Windows task switcher")),
        ("plain", None),
    ] {
        let inputs = [down("Alt"), KeyInput::Text(text.to_owned()), up("Alt")];
        assert_eq!(quiet.shell_hotkey(&inputs), expected, "{text:?}");
    }
}

/// A key Windows took stays recorded when the guard's echo of it never comes, so the release of
/// everything releases it; a key Windows refused is never recorded.
#[test]
fn a_key_windows_took_is_held_even_when_its_echo_never_comes() {
    let echo_never_comes = || {
        Err(flui_driver::Error::Lane {
            lane: "windows".into(),
            unavailable: true,
            message: "no echo".into(),
        })
    };
    let mut keys = HeldKeys::new();
    let control = Keystroke::Press(keys::CONTROL);
    assert!(keys.deliver(control, || Ok(()), echo_never_comes).is_err());
    assert_eq!(
        keys.held(),
        [keys::CONTROL.code],
        "the press Windows took is held"
    );
    assert_eq!(
        release_recorded(&mut keys),
        [
            Keystroke::Press(MASK),
            Keystroke::Release(MASK),
            Keystroke::Release(keys::CONTROL),
        ]
    );
    assert_eq!(keys.held(), [] as [u16; 0]);

    let refused = || {
        Err(flui_driver::Error::Protocol {
            message: "refused".into(),
        })
    };
    assert!(keys.deliver(control, refused, || Ok(())).is_err());
    assert_eq!(
        keys.held(),
        [] as [u16; 0],
        "a press Windows refused is not held"
    );

    let mut buttons = HeldButtons::default();
    assert!(
        buttons
            .deliver(0, true, || Ok(()), echo_never_comes)
            .is_err()
    );
    assert_eq!(buttons.bits(), 0b1, "the button Windows took is held");
}

/// A capital letter holds Shift around its key unless Shift is already held, and the Shift it
/// added is released with the key.
#[test]
fn keys_and_text_are_planned_against_what_the_lane_holds() {
    let a = VirtualKey {
        code: 0x41,
        extended: false,
    };
    let mut held = HeldKeys::new();
    let down = held.plan(&KeyInput::Down {
        key: a,
        shift: true,
    });
    assert_eq!(down, [Keystroke::ShiftFor(0x41), Keystroke::Press(a)]);
    for stroke in down {
        held.apply(stroke);
    }
    assert_eq!(held.held(), [keys::SHIFT.code, 0x41]);
    let up = held.plan(&KeyInput::Up { key: a });
    assert_eq!(up, [Keystroke::Release(a), Keystroke::UnshiftFor(0x41)]);
    for stroke in up {
        held.apply(stroke);
    }
    assert_eq!(held.held(), [] as [u16; 0]);
    assert_eq!(
        held.plan(&KeyInput::Text("x\n".to_owned())),
        [
            Keystroke::Unit(u16::from(b'x')),
            Keystroke::Press(keys::ENTER),
            Keystroke::Release(keys::ENTER),
        ]
    );
}

/// A key or button whose release Windows refused stays held, for the next release to try again,
/// and the failure is reported.
#[test]
fn a_release_windows_refused_stays_held() {
    let mut held = HeldKeys::new();
    for stroke in [Keystroke::Press(keys::CONTROL), Keystroke::Press(keys::TAB)] {
        held.apply(stroke);
    }
    let refused = held.release_all(|stroke| {
        if stroke == Keystroke::Release(keys::TAB) {
            Err(flui_driver::Error::Protocol {
                message: "refused".into(),
            })
        } else {
            Ok(())
        }
    });
    assert!(refused.is_err(), "the refusal is reported");
    assert_eq!(
        held.held(),
        [keys::TAB.code],
        "the refused key is still held"
    );
    held.release_all(|_| Ok(())).expect("released this time");
    assert_eq!(held.held(), [] as [u16; 0]);

    let mut buttons = HeldButtons::default();
    buttons.apply(0, true);
    buttons.apply(1, true);
    let refused = buttons.release_all(|button| {
        if button == 1 {
            Err(flui_driver::Error::Protocol {
                message: "refused".into(),
            })
        } else {
            Ok(())
        }
    });
    assert!(refused.is_err());
    assert_eq!(buttons.bits(), 0b10, "the refused button is still held");
}

/// A release refused with `part` named, as a test injection answers.
fn refused(part: &str) -> flui_driver::Result<()> {
    Err(flui_driver::Error::Protocol {
        message: format!("{part} refused"),
    })
}

/// A failed release's lane, the names of what it left held and its message.
fn release_failure(outcome: &flui_driver::Result<()>) -> Option<(String, Vec<String>, String)> {
    match outcome {
        Err(flui_driver::Error::ReleaseFailed {
            lane,
            held,
            message,
            ..
        }) => Some((lane.clone(), held.clone(), message.clone())),
        _ => None,
    }
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<String>>()
}

/// The lane's release of everything tries every part whatever the one before came to, and names
/// what each still holds with each failed part's first failure, in order: a refused key does not
/// hide the buttons and contacts left down, nor why they stayed.
#[test]
fn the_release_of_everything_names_what_every_part_still_holds() {
    let mut keys = HeldKeys::new();
    keys.apply(Keystroke::Press(keys::TAB));
    let mut buttons = HeldButtons::default();
    for button in 0..5 {
        buttons.apply(button, true);
    }
    let mut contacts = Contacts::new();
    let mut os = Injected::default();
    for (pointer, kind) in [("a", PointerKind::Touch), ("p", PointerKind::Pen)] {
        contacts
            .move_to(pointer, kind, point(10, 10), |f| os.take(f))
            .expect("a placement");
        contacts
            .down(pointer, kind, |f| os.take(f))
            .expect("a press");
    }
    let mut refusing = Injected {
        refuse: true,
        ..Injected::default()
    };
    let everything = windows::release_held(
        &mut keys,
        &mut buttons,
        &mut contacts,
        |_| refused("the keys"),
        |_| refused("the mouse"),
        |f| refusing.take(f),
    );
    assert_eq!(
        release_failure(&everything),
        Some((
            "windows".to_owned(),
            names(&[
                "Tab",
                "the left mouse button",
                "the right mouse button",
                "the middle mouse button",
                "the back mouse button",
                "the forward mouse button",
                "the touch contact a",
                "the pen p",
            ]),
            "protocol: the keys refused; protocol: the mouse refused; \
             protocol: Windows refused the frame"
                .to_owned()
        )),
        "{everything:?}"
    );
    assert_eq!(keys.held(), [keys::TAB.code]);
    assert_eq!(
        buttons.bits(),
        0b1_1111,
        "every refused button is still held"
    );

    // Only what failed is named: a part that released everything adds nothing.
    let mut buttons = HeldButtons::default();
    buttons.apply(1, true);
    buttons.apply(4, true);
    let mouse = windows::release_held(
        &mut HeldKeys::new(),
        &mut buttons,
        &mut Contacts::new(),
        |_| Ok(()),
        |button| {
            if button == 1 {
                refused("the mouse")
            } else {
                Ok(())
            }
        },
        |f| os.take(f),
    );
    assert_eq!(
        release_failure(&mouse),
        Some((
            "windows".to_owned(),
            names(&["the right mouse button"]),
            "protocol: the mouse refused".to_owned()
        ))
    );

    // A pen whose lift went out but whose leave was refused is still in range.
    let mut pen = Contacts::new();
    pen.move_to("p", PointerKind::Pen, point(1, 1), |f| os.take(f))
        .expect("in range");
    pen.down("p", PointerKind::Pen, |f| os.take(f))
        .expect("a pen press");
    let in_range = pen.release_all(|f| match f.contacts.first().map(|c| c.phase) {
        Some(Phase::Leave) => refused("the leave"),
        _ => Ok(()),
    });
    assert_eq!(
        release_failure(&in_range),
        Some((
            "windows".to_owned(),
            names(&["the pen p in range"]),
            "protocol: the leave refused".to_owned()
        ))
    );

    // The mask refused while Control and Tab are held: both releases still go out, so the keys
    // hold nothing and answer the mask's failure bare, and it leads the message of what the
    // mouse left held.
    let mut keys = HeldKeys::new();
    send(&mut keys, &[down("Control"), down("Tab")]);
    let mut buttons = HeldButtons::default();
    buttons.apply(0, true);
    let after_mask = windows::release_held(
        &mut keys,
        &mut buttons,
        &mut Contacts::new(),
        |stroke| {
            if stroke == Keystroke::Press(MASK) {
                refused("the mask")
            } else {
                Ok(())
            }
        },
        |_| refused("the mouse"),
        |f| os.take(f),
    );
    assert_eq!(keys.held(), [] as [u16; 0], "both keys were released");
    assert_eq!(
        release_failure(&after_mask),
        Some((
            "windows".to_owned(),
            names(&["the left mouse button"]),
            "protocol: the mask refused; protocol: the mouse refused".to_owned()
        )),
        "{after_mask:?}"
    );
    // With nothing held anywhere, the first bare failure is the answer.
    assert!(matches!(
        release_outcome([refused("the mask"), Ok(())]),
        Err(flui_driver::Error::Protocol { message }) if message == "the mask refused"
    ));
    assert!(matches!(
        release_outcome([Ok(()), refused("the first"), refused("the second")]),
        Err(flui_driver::Error::Protocol { message }) if message == "the first refused"
    ));
    assert!(release_outcome([Ok(()), Ok(()), Ok(())]).is_ok());
}

/// The parts that panic, the part the OS refuses, the panic resumed, what the log names.
type ReleaseRow<'a> = (&'a [&'a str], Option<&'a str>, &'a str, Option<&'a str>);

/// A part whose release panics keeps no other part held: every part is tried, the first panic is
/// resumed, and what a part that was refused leaves held is logged at error level first, since
/// the panic is all the caller gets.
#[test]
fn a_panicking_part_of_the_release_still_lets_the_others_go() {
    let rows: [ReleaseRow<'_>; 4] = [
        (&["keys"], None, "the keys panic", None),
        (&["buttons"], None, "the buttons panic", None),
        (
            &["contacts"],
            Some("buttons"),
            "the contacts panic",
            Some("the left mouse button"),
        ),
        (
            &["keys", "buttons"],
            Some("contacts"),
            "the keys panic",
            Some("the touch contact a"),
        ),
    ];
    for (panicking, refusing, resumed_text, logged) in rows {
        let act = |part: &str| -> flui_driver::Result<()> {
            assert!(!panicking.contains(&part), "the {part} panic");
            if refusing == Some(part) {
                return refused(part);
            }
            Ok(())
        };
        let mut keys = HeldKeys::new();
        keys.apply(Keystroke::Press(keys::TAB));
        let mut buttons = HeldButtons::default();
        buttons.apply(0, true);
        let mut contacts = Contacts::new();
        let mut os = Injected::default();
        contacts
            .move_to("a", PointerKind::Touch, point(10, 10), |f| os.take(f))
            .expect("a placement");
        contacts
            .down("a", PointerKind::Touch, |f| os.take(f))
            .expect("a press");
        let mut lifted = Injected::default();
        let (resumed, captured) = flui_testing::log_capture::capture(|| {
            #[expect(
                clippy::disallowed_methods,
                reason = "the test catches the resumed panic to assert on what ran before it"
            )]
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                windows::release_held(
                    &mut keys,
                    &mut buttons,
                    &mut contacts,
                    |_| act("keys"),
                    |_| act("buttons"),
                    |f| {
                        act("contacts")?;
                        lifted.take(f)
                    },
                )
            }))
        });
        let payload = resumed.expect_err("the first panic is resumed");
        assert_eq!(
            payload.downcast_ref::<String>().map(String::as_str),
            Some(resumed_text),
            "{panicking:?}"
        );
        let tried = |part: &str| !panicking.contains(&part) && refusing != Some(part);
        if tried("keys") {
            assert_eq!(
                keys.held(),
                [] as [u16; 0],
                "{panicking:?}: the key was released"
            );
        }
        if tried("buttons") {
            assert_eq!(buttons.bits(), 0, "{panicking:?}: the button was released");
        }
        if tried("contacts") {
            assert_eq!(
                lifted.last_contacts(),
                [(0, Phase::Up, true)],
                "{panicking:?}: the touch contact was lifted"
            );
        }
        let errors = captured
            .at_level(tracing::Level::ERROR)
            .filter(|record| record.contains("input stays held after a release that panicked"))
            .collect::<Vec<_>>();
        match logged {
            None => assert!(errors.is_empty(), "{panicking:?}: {captured}"),
            Some(name) => assert!(
                errors.len() == 1 && errors[0].contains(name),
                "{panicking:?}: {name} is logged: {captured}"
            ),
        }
    }
}

/// A pause is waited out whole, in slices short enough to look at the guard between them.
#[test]
fn a_pause_is_waited_out_whole_in_watched_slices() {
    for pause in [
        Duration::ZERO,
        Duration::from_millis(1),
        Duration::from_millis(120),
        Duration::from_secs(25),
    ] {
        let slices: Vec<Duration> = pacing::slices(pause).collect();
        assert_eq!(slices.iter().sum::<Duration>(), pause, "{pause:?}");
        assert!(slices.iter().all(|slice| *slice <= pacing::WATCH_SLICE));
    }
}

#[test]
fn only_input_with_both_of_the_lanes_marks_is_its_own() {
    let mouse = |injected, extra| Observed::Mouse {
        motion: MouseMotion::Move,
        injected,
        extra,
    };
    let rows: &[(&str, Observed, Option<&str>)] = &[
        ("the lane's move", mouse(true, LANE_TAG), None),
        ("a physical move", mouse(false, 0), Some("a mouse move")),
        (
            "a physical move that claims the tag",
            mouse(false, LANE_TAG),
            Some("a mouse move"),
        ),
        (
            "another program's injected move",
            mouse(true, 0),
            Some("a mouse move"),
        ),
        (
            "a move Windows promotes from the lane's touch",
            mouse(true, 0xFF51_5701),
            None,
        ),
        (
            "a move Windows promotes from a finger, which the digitizer stream reports",
            mouse(false, 0xFF51_5781),
            None,
        ),
        (
            "a physical button",
            Observed::Mouse {
                motion: MouseMotion::Button,
                injected: false,
                extra: 0,
            },
            Some("a mouse button"),
        ),
        (
            "a physical wheel",
            Observed::Mouse {
                motion: MouseMotion::Wheel,
                injected: false,
                extra: 0,
            },
            Some("a turn of the wheel"),
        ),
        (
            "the lane's key",
            Observed::Key {
                injected: true,
                extra: LANE_TAG,
            },
            None,
        ),
        (
            "a physical key",
            Observed::Key {
                injected: false,
                extra: 0,
            },
            Some("a key press"),
        ),
        (
            "another program's key",
            Observed::Key {
                injected: true,
                extra: 7,
            },
            Some("a key press"),
        ),
        (
            "a finger on the digitizer",
            Observed::Digitizer {
                pen: false,
                from_device: true,
            },
            Some("a touch"),
        ),
        (
            "a pen on the digitizer",
            Observed::Digitizer {
                pen: true,
                from_device: true,
            },
            Some("a pen"),
        ),
        (
            "the lane's synthetic device, which no device sends",
            Observed::Digitizer {
                pen: false,
                from_device: false,
            },
            None,
        ),
    ];
    for (row, observed, expected) in rows {
        assert_eq!(activity::foreign(*observed), *expected, "{row}");
    }
}

fn point(x: i32, y: i32) -> DevicePoint {
    DevicePoint::new(x, y)
}

/// The frames a test injection was handed, and whether it accepts the next one.
#[derive(Default)]
struct Injected {
    frames: Vec<Frame>,
    refuse: bool,
}

impl Injected {
    fn take(&mut self, frame: &Frame) -> flui_driver::Result<()> {
        if self.refuse {
            return Err(flui_driver::Error::Protocol {
                message: "Windows refused the frame".into(),
            });
        }
        self.frames.push(frame.clone());
        Ok(())
    }

    fn last(&self) -> &Frame {
        self.frames.last().expect("a frame was injected")
    }

    /// The last frame's contacts as (id, phase, primary).
    fn last_contacts(&self) -> Vec<(u32, Phase, bool)> {
        self.last()
            .contacts
            .iter()
            .map(|c| (c.id, c.phase, c.primary))
            .collect()
    }
}

#[test]
fn touch_and_pen_frames_carry_every_contact_and_their_phases() {
    let mut contacts = Contacts::new();
    let mut os = Injected::default();
    contacts
        .move_to("a", PointerKind::Touch, point(10, 10), |f| os.take(f))
        .expect("a placement");
    assert!(
        os.frames.is_empty(),
        "a touch that is not down only places itself"
    );
    contacts
        .down("a", PointerKind::Touch, |f| os.take(f))
        .expect("a touch press");
    assert_eq!(os.last_contacts(), [(0, Phase::Down, true)]);
    contacts
        .move_to("b", PointerKind::Touch, point(50, 10), |f| os.take(f))
        .expect("a placement");
    contacts
        .down("b", PointerKind::Touch, |f| os.take(f))
        .expect("a second press");
    assert_eq!(
        os.last_contacts(),
        [(0, Phase::Update, true), (1, Phase::Down, false)]
    );
    contacts
        .move_to("a", PointerKind::Touch, point(12, 10), |f| os.take(f))
        .expect("a contact that is down moves");
    assert_eq!(os.last().contacts[0].at, point(12, 10));
    contacts
        .up("a", PointerKind::Touch, |f| os.take(f))
        .expect("a release");
    assert_eq!(
        os.last_contacts(),
        [(0, Phase::Up, true), (1, Phase::Update, false)]
    );
    // The freed id is the lowest, and the next finger takes it.
    contacts
        .move_to("c", PointerKind::Touch, point(90, 10), |f| os.take(f))
        .expect("a placement");
    contacts
        .down("c", PointerKind::Touch, |f| os.take(f))
        .expect("a third press");
    assert!(os.last_contacts().contains(&(0, Phase::Down, false)));
    assert!(
        contacts
            .up("a", PointerKind::Touch, |f| os.take(f))
            .is_err(),
        "a lifted contact is up"
    );
    assert!(
        contacts
            .down("never-moved", PointerKind::Touch, |f| os.take(f))
            .is_err(),
        "a press with no position is refused"
    );

    contacts
        .move_to("pen", PointerKind::Pen, point(5, 5), |f| os.take(f))
        .expect("a pen in range hovers");
    assert_eq!(os.last_contacts(), [(0, Phase::Hover, true)]);
    contacts
        .down("pen", PointerKind::Pen, |f| os.take(f))
        .expect("a pen press");
    assert_eq!(os.last_contacts(), [(0, Phase::Down, true)]);

    let mut released = Injected::default();
    contacts
        .release_all(|f| released.take(f))
        .expect("everything lifts");
    let summary: Vec<(PointerKind, Vec<Phase>)> = released
        .frames
        .iter()
        .map(|frame| (frame.kind, frame.contacts.iter().map(|c| c.phase).collect()))
        .collect();
    assert_eq!(
        summary,
        [
            (PointerKind::Touch, vec![Phase::Up, Phase::Up]),
            (PointerKind::Pen, vec![Phase::Up]),
            (PointerKind::Pen, vec![Phase::Leave]),
        ]
    );
    let mut again = Injected::default();
    contacts
        .release_all(|f| again.take(f))
        .expect("nothing left");
    assert!(again.frames.is_empty(), "nothing is left to release");

    let mut many = Contacts::new();
    let mut os = Injected::default();
    for finger in 0..10 {
        let id = format!("f{finger}");
        many.move_to(&id, PointerKind::Touch, point(finger, 0), |f| os.take(f))
            .expect("a placement");
        many.down(&id, PointerKind::Touch, |f| os.take(f))
            .expect("ten fingers fit");
    }
    many.move_to("f10", PointerKind::Touch, point(10, 0), |f| os.take(f))
        .expect("a placement");
    assert!(
        many.down("f10", PointerKind::Touch, |f| os.take(f))
            .is_err(),
        "an eleventh contact is refused"
    );
}

/// What Windows refused is not what it holds: a refused press is never lifted, a refused release
/// is lifted again, and `release_all` lifts only what the OS took.
#[test]
fn a_contact_is_held_only_once_windows_took_it() {
    let mut contacts = Contacts::new();
    let mut os = Injected::default();
    contacts
        .move_to("a", PointerKind::Touch, point(10, 10), |f| os.take(f))
        .expect("a placement");
    os.refuse = true;
    assert!(
        contacts
            .down("a", PointerKind::Touch, |f| os.take(f))
            .is_err()
    );
    os.refuse = false;
    let mut released = Injected::default();
    contacts
        .release_all(|f| released.take(f))
        .expect("nothing held");
    assert!(released.frames.is_empty(), "a refused press is not held");

    contacts
        .move_to("a", PointerKind::Touch, point(10, 10), |f| os.take(f))
        .expect("a placement");
    contacts
        .down("a", PointerKind::Touch, |f| os.take(f))
        .expect("a press");
    os.refuse = true;
    assert!(
        contacts
            .up("a", PointerKind::Touch, |f| os.take(f))
            .is_err()
    );
    let mut lifted = Injected::default();
    contacts
        .release_all(|f| lifted.take(f))
        .expect("the release");
    assert_eq!(
        lifted.last_contacts(),
        [(0, Phase::Up, true)],
        "a refused release is still held, and lifted again"
    );

    let mut pen = Contacts::new();
    let mut os = Injected::default();
    pen.move_to("p", PointerKind::Pen, point(1, 1), |f| os.take(f))
        .expect("in range");
    pen.down("p", PointerKind::Pen, |f| os.take(f))
        .expect("a pen press");
    let mut refusing = Injected {
        refuse: true,
        ..Injected::default()
    };
    assert!(pen.release_all(|f| refusing.take(f)).is_err());
    let mut lifted = Injected::default();
    pen.release_all(|f| lifted.take(f))
        .expect("lifted this time");
    assert_eq!(
        lifted.frames.len(),
        2,
        "the pen is lifted, then leaves range"
    );
}

/// The first contact down while none is down is primary for its whole life; once it lifts, no
/// remaining contact becomes primary until every contact has lifted (Microsoft Learn, "Pointer
/// Flags", `POINTER_FLAG_PRIMARY`).
#[test]
fn the_primary_contact_is_the_first_down_and_stays_so() {
    let mut contacts = Contacts::new();
    let mut os = Injected::default();
    let press = |contacts: &mut Contacts, os: &mut Injected, id: &str, x: i32| {
        contacts
            .move_to(id, PointerKind::Touch, point(x, 0), |f| os.take(f))
            .expect("a placement");
        contacts
            .down(id, PointerKind::Touch, |f| os.take(f))
            .expect("a press");
    };
    press(&mut contacts, &mut os, "a", 0);
    press(&mut contacts, &mut os, "b", 10);
    contacts
        .up("a", PointerKind::Touch, |f| os.take(f))
        .expect("the primary lifts");
    press(&mut contacts, &mut os, "c", 20);
    assert_eq!(
        os.last_contacts(),
        [(1, Phase::Update, false), (0, Phase::Down, false)],
        "after the primary lifts, no contact is primary while any is down"
    );
    contacts
        .up("b", PointerKind::Touch, |f| os.take(f))
        .expect("a release");
    contacts
        .up("c", PointerKind::Touch, |f| os.take(f))
        .expect("the last release");
    press(&mut contacts, &mut os, "d", 30);
    assert_eq!(
        os.last_contacts(),
        [(0, Phase::Down, true)],
        "the first contact down after every contact lifted is primary"
    );
}

#[test]
fn a_client_origin_maps_back_to_its_device_pixel_at_every_scale() {
    for dpi in [72, 96, 120, 144, 168, 192, 240, 288, 384, 768] {
        let scale = screen::scale_of(dpi);
        for device in (-7_681..7_681).step_by(13) {
            let geometry = flui_driver::lane::screen::window_geometry(
                point(device, -device),
                LogicalSize::new(800.0, 600.0),
                scale,
            );
            assert_eq!(
                to_device(&geometry, LogicalPoint::new(0.0, 0.0)).expect("the origin maps"),
                point(device, -device),
                "device {device} at {dpi} dpi"
            );
            // A whole logical step lands on the device pixel it covers.
            let step = to_device(&geometry, LogicalPoint::new(10.0, 10.0)).expect("a point maps");
            let expected = (10.0 * scale).floor() as i32;
            assert_eq!(step, point(device + expected, -device + expected));
        }
    }
    assert!(screen::is_window(
        1_000,
        750,
        120,
        LogicalSize::new(800.0, 600.0),
        1.25
    ));
    assert!(screen::is_window(
        1_001,
        751,
        120,
        LogicalSize::new(800.0, 600.0),
        1.25
    ));
    assert!(!screen::is_window(
        1_000,
        750,
        96,
        LogicalSize::new(800.0, 600.0),
        1.25
    ));
    assert!(!screen::is_window(
        640,
        480,
        96,
        LogicalSize::new(800.0, 600.0),
        1.0
    ));
}

#[test]
fn an_absolute_move_lands_on_the_pixel_it_names() {
    for (start, extent) in [(0, 1920), (-1920, 3840), (-2560, 7680), (0, 1), (100, 3)] {
        for at in start..start + extent {
            let normalized = screen::normalized(at, start, extent).expect("on the desktop");
            assert!((0..=65_535).contains(&normalized));
            let lands = start + (i64::from(normalized) * i64::from(extent) / 65_536) as i32;
            assert_eq!(lands, at, "{at} on a desktop from {start} of {extent}");
        }
    }
    assert_eq!(screen::normalized(-1, 0, 1920), None);
    assert_eq!(screen::normalized(1920, 0, 1920), None);
    assert_eq!(screen::normalized(0, 0, 0), None);
}

fn node(id: u64, role: Role, name: &str) -> Node {
    let mut node = Node::new(
        ElementId::from_u64(id).expect("a nonzero handle"),
        role,
        "native",
    );
    node.name = Some(name.to_owned());
    node
}

#[test]
fn the_projection_drops_what_one_backend_reports_and_names_the_differences() {
    let mut app_root = node(1, Role::Window, "the app");
    let mut like = node(2, Role::Button, "Like");
    like.identifier = Some("like".into());
    like.focused = true;
    let mut likes = node(3, Role::Label, "Likes");
    likes.value = Some("1".into());
    app_root.children = vec![like, likes.clone()];

    let mut os_root = node(10, Role::Window, "flui-driver scenario");
    let mut title = node(11, Role::TitleBar, "flui-driver scenario");
    title.children = vec![node(12, Role::Button, "Close")];
    let mut os_like = node(13, Role::Button, "Like");
    os_like.automation_id = Some("like".into());
    os_like.class_name = Some("FluiWindow".into());
    let mut os_likes = likes;
    os_likes.id = ElementId::from_u64(14).expect("a nonzero handle");
    // The native host publishes the announcer's live regions, which a read reports as
    // announcements, not nodes.
    let mut announcer = node(15, Role::Label, "");
    announcer.name = None;
    announcer.automation_id = Some("flui:announcer:polite".into());
    os_root.children = vec![title, os_like, os_likes, announcer];

    let app = Tree::new(vec![app_root], false);
    let os = Tree::new(vec![os_root], false);
    assert_eq!(
        normalized_outline(&app.roots),
        "- button \"Like\" [identifier=\"like\"]\n- label \"Likes\" [value=\"1\"]\n"
    );
    let projection = Projection::of(&app, &os);
    assert!(projection.is_equal(), "{projection:?}");
    assert_eq!(projection.differences(), []);

    let mut renamed = os;
    renamed.roots[0].children[1].name = Some("Liked".into());
    let differs = Projection::of(&app, &renamed);
    assert!(!differs.is_equal());
    assert_eq!(
        differs.differences(),
        [
            ProjectionDifference::OnlyInOs("- button \"Liked\" [identifier=\"like\"]".into()),
            ProjectionDifference::MissingFromOs("- button \"Like\" [identifier=\"like\"]".into()),
        ]
    );
}

/// A reader of an OS tree keeps the first `OUTLINE_TEXT_LIMIT` characters of a text, and the
/// projection cuts the app's at the same place, so a long label compares equal.
#[test]
fn a_long_text_is_cut_at_the_same_place_on_both_sides() {
    let long: String = "é".repeat(OUTLINE_TEXT_LIMIT + 500);
    let cut: String = long.chars().take(OUTLINE_TEXT_LIMIT).collect();
    let mut app_root = node(1, Role::Window, "app");
    app_root.children = vec![node(2, Role::Label, &long)];
    let mut os_root = node(10, Role::Window, "os");
    os_root.children = vec![node(11, Role::Label, &cut)];
    let projection = Projection::of(
        &Tree::new(vec![app_root], false),
        &Tree::new(vec![os_root], false),
    );
    assert!(projection.is_equal(), "{:?}", projection.differences());
}

/// A tree a test lays out: each element's node, its children, and how the provider answers.
struct FakeTree {
    nodes: Vec<(Node, Vec<usize>)>,
    /// The element whose properties time out.
    hung: Option<usize>,
    /// The element that went away.
    gone: Option<usize>,
    /// The elements whose properties were read, in order.
    read: Vec<usize>,
}

impl FakeTree {
    fn new(nodes: Vec<(Node, Vec<usize>)>) -> Self {
        Self {
            nodes,
            hung: None,
            gone: None,
            read: Vec::new(),
        }
    }

    fn parent_of(&self, element: usize) -> Option<usize> {
        self.nodes
            .iter()
            .position(|(_, children)| children.contains(&element))
    }
}

impl Provider for FakeTree {
    type Element = usize;

    fn first_child(&mut self, element: &usize) -> Result<Option<usize>, ProviderError> {
        Ok(self.nodes[*element].1.first().copied())
    }

    fn next_sibling(&mut self, element: &usize) -> Result<Option<usize>, ProviderError> {
        let Some(parent) = self.parent_of(*element) else {
            // A root's sibling is another window: present, and not the app's.
            return Ok(Some(self.nodes.len() - 1));
        };
        let siblings = &self.nodes[parent].1;
        let at = siblings
            .iter()
            .position(|sibling| sibling == element)
            .expect("BUG: a child is among its parent's children");
        Ok(siblings.get(at + 1).copied())
    }

    fn node(&mut self, element: &usize, id: ElementId) -> Result<Node, ProviderError> {
        self.read.push(*element);
        if self.hung == Some(*element) {
            return Err(ProviderError::Timeout);
        }
        if self.gone == Some(*element) {
            return Err(ProviderError::Gone);
        }
        let mut node = self.nodes[*element].0.clone();
        node.id = id;
        Ok(node)
    }
}

/// A window with a button and a list of three items, and, last, another window beside it.
fn fake_window() -> FakeTree {
    let mut like = node(1, Role::Button, "Like");
    like.automation_id = Some("like".into());
    let mut third = node(1, Role::ListItem, "third");
    third.automation_id = Some("third".into());
    let mut other = node(1, Role::Window, "another app");
    other.automation_id = Some("like".into());
    FakeTree::new(vec![
        (node(1, Role::Window, "app"), vec![1, 2]),
        (like, vec![]),
        (node(1, Role::List, "items"), vec![3, 4, 5]),
        (node(1, Role::ListItem, "first"), vec![]),
        (node(1, Role::ListItem, "second"), vec![]),
        (third, vec![]),
        (other, vec![]),
    ])
}

const UIA_TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn the_walk_reads_the_window_and_leaves_a_gone_element_out() {
    let mut tree = fake_window();
    tree.gone = Some(4);
    let read = walk::read(&mut tree, &0, UIA_TIMEOUT).expect("the window reads");
    assert!(!read.truncated);
    let outline = normalized_outline(&read.roots);
    assert_eq!(
        outline,
        "- button \"Like\" [identifier=\"like\"]\n- list \"items\"\n  - list_item \"first\"\n  \
         - list_item \"third\" [identifier=\"third\"]\n"
    );
    assert!(
        !tree.read.contains(&6),
        "the window beside the app's is not read"
    );
}

/// A provider that does not answer fails the read with a timeout at once, never a node of
/// defaults.
#[test]
fn a_provider_that_times_out_fails_the_read_with_a_timeout() {
    let mut tree = fake_window();
    tree.hung = Some(2);
    let refused = walk::read(&mut tree, &0, UIA_TIMEOUT).expect_err("a hung provider");
    assert_eq!(refused.code(), Some(ErrorCode::Timeout), "{refused:?}");
    assert_eq!(
        tree.read,
        [0, 1, 2],
        "the walk stopped at the element that hung"
    );
}

/// After the first property read that times out, no further read of the element is made: a hung
/// provider costs one timeout, not one per property.
#[test]
fn no_property_is_read_after_the_first_timeout() {
    let mut reads = Properties::new();
    let mut calls = 0;
    let mut call = |answer: Result<u32, ProviderError>| {
        calls += 1;
        answer
    };
    assert_eq!(reads.read(|| call(Ok(1))), Some(1));
    assert_eq!(
        reads.read(|| call(Err(ProviderError::Failed("not offered".into())))),
        None,
        "a property not offered reads as its default"
    );
    assert_eq!(reads.read(|| call(Err(ProviderError::Timeout))), None);
    assert_eq!(reads.read(|| call(Ok(2))), None);
    assert_eq!(reads.read(|| call(Ok(3))), None);
    assert_eq!(calls, 3, "nothing is asked after the timeout");
    assert_eq!(reads.finish(()), Err(ProviderError::Timeout));
}

/// Asking an element for a control pattern and invoking it keep the provider's answers apart: a
/// pattern the element does not offer is `action_unsupported`, a hung provider a timeout (one that
/// may have run, for the invoke), an element that went away `gone`.
#[test]
fn a_pattern_request_and_an_invoke_tell_their_failures_apart() {
    let timeout = Duration::from_millis(250);
    let pattern = |answer: Result<Option<()>, ProviderError>| {
        walk::pattern(answer, "like", "invoke", timeout)
            .expect_err("no pattern")
            .code()
    };
    assert_eq!(
        walk::pattern(Ok(Some(7)), "like", "invoke", timeout).ok(),
        Some(7)
    );
    assert_eq!(pattern(Ok(None)), Some(ErrorCode::ActionUnsupported));
    assert_eq!(
        pattern(Err(ProviderError::Timeout)),
        Some(ErrorCode::Timeout)
    );
    assert_eq!(pattern(Err(ProviderError::Gone)), Some(ErrorCode::Gone));
    assert_ne!(
        pattern(Err(ProviderError::Failed("broken".into()))),
        Some(ErrorCode::ActionUnsupported),
        "a provider's failure is not a missing pattern"
    );

    assert!(walk::invoked(Ok(()), "like", timeout).is_ok());
    let hung = walk::invoked(Err(ProviderError::Timeout), "like", timeout).expect_err("hung");
    assert_eq!(hung.code(), Some(ErrorCode::Timeout));
    assert!(
        matches!(&hung, flui_driver::Error::Timeout { effect: Some(effect), .. }
            if effect.kind == flui_driver::protocol::act::EffectKind::MayHaveRun),
        "{hung:?}"
    );
    let gone = walk::invoked(Err(ProviderError::Gone), "like", timeout).expect_err("gone");
    assert_eq!(gone.code(), Some(ErrorCode::Gone));
    assert!(
        gone.may_have_run(),
        "an element or provider that went during the invoke may have run it first: {gone:?}"
    );
    let before =
        walk::pattern::<()>(Err(ProviderError::Gone), "like", "invoke", timeout).expect_err("gone");
    assert!(
        !before.may_have_run(),
        "gone before the invoke was sent: nothing ran"
    );
}

/// The search for an automation id walks the app's window only, within its budgets, and tells a
/// hung provider apart from an id nobody has.
#[test]
fn the_search_for_an_automation_id_is_bounded_and_tells_its_failures_apart() {
    let mut tree = fake_window();
    assert_eq!(
        walk::find(&mut tree, 0, "third", UIA_TIMEOUT).expect("found"),
        5
    );
    let mut tree = fake_window();
    let missing = walk::find(&mut tree, 0, "nobody", UIA_TIMEOUT).expect_err("absent");
    assert_eq!(missing.code(), Some(ErrorCode::NotFound), "{missing:?}");
    assert!(
        !tree.read.contains(&6),
        "another window's element is not the app's"
    );
    let mut tree = fake_window();
    tree.hung = Some(3);
    let hung = walk::find(&mut tree, 0, "third", UIA_TIMEOUT).expect_err("hung");
    assert_eq!(hung.code(), Some(ErrorCode::Timeout), "{hung:?}");

    // A chain deeper than the walk goes is cut, and said to be.
    let depth = walk::MAX_DEPTH + 10;
    let mut chain: Vec<(Node, Vec<usize>)> = (0..depth)
        .map(|level| (node(1, Role::Group, "level"), vec![level + 1]))
        .collect();
    let mut deepest = node(1, Role::Button, "deep");
    deepest.automation_id = Some("deep".into());
    chain.push((deepest, vec![]));
    chain.push((node(1, Role::Window, "another app"), vec![]));
    let mut tree = FakeTree::new(chain);
    let cut = walk::find(&mut tree, 0, "deep", UIA_TIMEOUT).expect_err("too deep");
    assert_eq!(cut.code(), Some(ErrorCode::NotFound));
    assert!(cut.to_string().contains("levels"), "{cut}");
    let read = walk::read(&mut tree, &0, UIA_TIMEOUT).expect("a read");
    assert!(read.truncated, "a read cut by depth says so");
}
