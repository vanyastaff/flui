//! The X11 lane's and the AT-SPI client's tables, on every host: keysyms, the AT-SPI role table,
//! the safety rules and their codes, the plan every key goes through, what the guard takes for
//! the person's input, and the image layout `GetImage` answers in.

use flui_driver::coordinates::DevicePoint;
use flui_driver::lane::atspi::roles::{self as atspi, AtspiRole};
use flui_driver::lane::windows::roles as uia;
use flui_driver::lane::x11::activity::{EchoCount, Observed, RawKind};
use flui_driver::lane::x11::held;
use flui_driver::lane::x11::held::{
    HeldButtons, HeldInput, KeyInput, Keymap, Keystroke, MAX_WHEEL_CLICKS, WHEEL_STEP,
    scroll_within, wheel_clicks,
};
use flui_driver::lane::x11::image::{self, PixmapFormat};
use flui_driver::lane::x11::keys::{self, Keysym, Modifier, TextStroke};
use flui_driver::lane::x11::safety::{
    self, Desk, Held, Keycode, Landing, Refusal, STICKY_KEYS, check_event,
};
use flui_driver::protocol::error::ErrorCode;
use flui_driver::protocol::semantics::SemanticsRole;
use flui_driver::protocol::session::Isolation;
use flui_driver::protocol::wire::Role;
use flui_driver::{Error, Result};

#[test]
fn a_key_name_maps_to_its_keysym_and_a_character_to_its_own() {
    let rows: &[(&str, Option<u32>)] = &[
        ("Enter", Some(0xFF0D)),
        ("Tab", Some(0xFF09)),
        ("Backspace", Some(0xFF08)),
        ("Escape", Some(0xFF1B)),
        ("Shift", Some(0xFFE1)),
        ("Control", Some(0xFFE3)),
        ("Alt", Some(0xFFE9)),
        ("Meta", Some(0xFFEB)),
        ("ArrowLeft", Some(0xFF51)),
        ("Delete", Some(0xFFFF)),
        ("F1", Some(0xFFBE)),
        ("F12", Some(0xFFC9)),
        ("F24", Some(0xFFD5)),
        ("MediaPlayPause", Some(0x1008_FF14)),
        ("a", Some(0x61)),
        ("A", Some(0x41)),
        ("+", Some(0x2B)),
        (" ", Some(0x20)),
        ("ü", Some(0xFC)),
        ("ß", Some(0xDF)),
        ("日", Some(0x0100_65E5)),
        ("€", Some(0x0100_20AC)),
        ("😀", Some(0x0101_F600)),
        ("", None),
        ("ab", None),
        ("Hyper", None),
        ("\u{7}", None),
    ];
    for (name, keysym) in rows {
        assert_eq!(keys::resolve(name), keysym.map(Keysym), "{name:?}");
    }
    for (keysym, name) in [
        (0xFFE1, "Shift"),
        (0xFF0D, "Enter"),
        (0x61, "a"),
        (0xFC, "ü"),
        (0x0100_65E5, "日"),
        (0x1008_FF99, "keysym 0x1008ff99"),
    ] {
        assert_eq!(keys::name_of(Keysym(keysym)), name);
    }
    assert_eq!(Modifier::of(Keysym(0xFFE2)), Some(Modifier::Shift));
    assert_eq!(Modifier::of(Keysym(0xFFE8)), Some(Modifier::Alt));
    assert_eq!(Modifier::of(Keysym(0xFE03)), Some(Modifier::Level));
    assert_eq!(Modifier::of(Keysym(0x61)), None);
}

#[test]
fn text_is_typed_as_its_characters_and_line_breaks_as_keys() {
    assert_eq!(
        keys::text_strokes("a\r\nb\n\tü"),
        Ok(vec![
            TextStroke::Char(Keysym(0x61)),
            TextStroke::Key(keys::RETURN),
            TextStroke::Char(Keysym(0x62)),
            TextStroke::Key(keys::RETURN),
            TextStroke::Key(keys::TAB),
            TextStroke::Char(Keysym(0xFC)),
        ])
    );
    assert_eq!(keys::text_strokes("bell\u{7}"), Err('\u{7}'));
    assert_eq!(
        keys::text_strokes("\rb"),
        Ok(vec![
            TextStroke::Key(keys::RETURN),
            TextStroke::Char(Keysym(0x62))
        ]),
        "a lone carriage return is one Return, the next character kept"
    );
}

#[test]
fn the_atspi_role_table_was_read_against_the_locked_adapter() {
    let lock = include_str!("../../../Cargo.lock");
    let locked = lock
        .split("[[package]]")
        .find(|entry| entry.contains("name = \"accesskit_atspi_common\""))
        .and_then(|entry| {
            entry
                .lines()
                .find_map(|line| line.strip_prefix("version = \""))
                .map(|version| version.trim_end_matches('"'))
        });
    assert_eq!(locked, Some(atspi::ADAPTER_CHECKED_AGAINST));
}

/// One row for each role that publishes a node of its own, in vocabulary order, as the UI
/// Automation table has.
#[test]
fn every_role_that_publishes_a_node_has_an_atspi_row_in_vocabulary_order() {
    let table: Vec<SemanticsRole> = atspi::ROLES.iter().map(|row| row.role).collect();
    let windows: Vec<SemanticsRole> = uia::ROLES.iter().map(|row| row.role).collect();
    assert_eq!(table, windows);
    assert!(atspi::role_row(SemanticsRole::None).is_none());
    assert_eq!(
        atspi::role_row(SemanticsRole::Switch).map(|row| row.atspi),
        Some(AtspiRole::TOGGLE_BUTTON)
    );
}

#[test]
fn an_atspi_role_reads_back_by_its_name() {
    assert_eq!(AtspiRole::BUTTON.name(), Some("button"));
    assert_eq!(AtspiRole::TOGGLE_BUTTON.name(), Some("toggle button"));
    assert_eq!(AtspiRole::ENTRY.name(), Some("entry"));
    assert_eq!(AtspiRole::FRAME.name(), Some("frame"));
    assert_eq!(AtspiRole(129).name(), Some("push button menu"));
    assert_eq!(AtspiRole(130).name(), None);
    assert_eq!(AtspiRole(130).native_name(), "role 130");
}

/// Through AT-SPI each role reads as the wire role the app publishes for it, which is the UI
/// Automation fold, except where one AT-SPI role stands for several: a grid reads as a table, a
/// grid cell as a cell, a pane and a scroll view as a group.
#[test]
fn an_atspi_role_folds_to_the_wire_role_the_app_publishes_where_atspi_keeps_it_apart() {
    for (row, windows) in atspi::ROLES.iter().zip(uia::ROLES) {
        let published = uia::expected_wire_role(windows);
        let read = atspi::expected_wire_role(row);
        if atspi::keeps_apart(row.role) {
            assert_eq!(read, published, "{}", row.role.name());
        } else {
            assert_ne!(read, published, "{} is kept apart", row.role.name());
        }
    }
    assert_eq!(atspi::wire_role(AtspiRole::TABLE), Role::Table);
    assert_eq!(
        atspi::wire_role(AtspiRole::TITLE_BAR),
        Role::TitleBar,
        "a title bar, which the projection leaves out"
    );
    assert_eq!(atspi::wire_role(AtspiRole::PANEL), Role::Group);
    assert_eq!(atspi::wire_role(AtspiRole(0)), Role::Unknown);
}

fn on_target() -> Landing {
    Landing::new(DevicePoint::new(10, 10), true, None)
}

fn code(refusal: Refusal) -> Option<ErrorCode> {
    refusal.into_error().code()
}

#[test]
fn each_safety_rule_refuses_with_its_code() {
    let held = Held::default();
    assert_eq!(
        check_event(&Desk::quiet(), Some(&on_target()), &held),
        Ok(())
    );
    let rows: Vec<(Desk, Option<Landing>, ErrorCode)> = vec![
        (
            Desk {
                window_alive: false,
                ..Desk::quiet()
            },
            None,
            ErrorCode::Gone,
        ),
        (
            Desk {
                focused: false,
                focus: Some("window 0x400001".into()),
                ..Desk::quiet()
            },
            None,
            ErrorCode::NotForeground,
        ),
        (
            Desk::quiet(),
            Some(Landing::new(
                DevicePoint::new(5, 5),
                false,
                Some("window 0x600001".into()),
            )),
            ErrorCode::OutsideTarget,
        ),
        (
            Desk {
                buttons: 0b1,
                ..Desk::quiet()
            },
            None,
            ErrorCode::InputHeld,
        ),
        (
            Desk {
                keys: vec![Keycode(50)],
                ..Desk::quiet()
            },
            None,
            ErrorCode::InputHeld,
        ),
    ];
    for (desk, landing, expected) in rows {
        let refused = check_event(&desk, landing.as_ref(), &held).expect_err("refused");
        assert_eq!(code(refused.clone()), Some(expected), "{refused:?}");
    }
    // What the lane holds itself is no one else's.
    let mine = Held {
        buttons: 0b1,
        keys: vec![Keycode(50)],
    };
    let desk = Desk {
        buttons: 0b1,
        keys: vec![Keycode(50)],
        ..Desk::quiet()
    };
    assert_eq!(check_event(&desk, None, &mine), Ok(()));
    for (refusal, capability) in [
        (Refusal::ShellHotkey { handler: "x" }, "os_shell_hotkey"),
        (Refusal::UnknownKey { name: "x".into() }, "os_key"),
        (Refusal::UnsentInput { what: "x".into() }, "os_input_device"),
        (Refusal::NoSpareKey { text: "x".into() }, "os_key"),
    ] {
        let error = refusal.into_error();
        assert_eq!(error.code(), Some(ErrorCode::NotSupported));
        assert_eq!(error.missing_capability().as_deref(), Some(capability));
    }
}

#[test]
fn the_lane_runs_on_a_desktops_server_or_an_xvfb_and_refuses_other_modes() {
    assert_eq!(safety::admit_isolation(Isolation::Shared), Ok(()));
    assert_eq!(safety::admit_isolation(Isolation::Xvfb), Ok(()));
    for isolation in [
        Isolation::Seat,
        Isolation::Sandbox,
        Isolation::InProcess,
        Isolation::Emulator,
    ] {
        let refused = safety::admit_isolation(isolation).expect_err("refused");
        assert_eq!(code(refused), Some(ErrorCode::InvalidArgument));
    }
}

/// A US keymap's first keys, as `GetKeyboardMapping` answers with two keysyms a key: Shift on 50,
/// Control on 37, Alt on 64, Super on 133, letters, digits, Return, Tab, F-keys, and keys with
/// nothing on them from 200 up.
fn keymap() -> Keymap {
    let mut syms = vec![(0_u32, 0_u32); 256 - 8];
    let mut put = |code: usize, first: u32, second: u32| syms[code - 8] = (first, second);
    put(10, 0x31, 0x21); // 1 !
    put(24, 0x71, 0x51); // q Q
    put(37, 0xFFE3, 0);
    put(38, 0x61, 0x41); // a A
    put(39, 0x73, 0x53);
    put(23, 0xFF09, 0);
    put(36, 0xFF0D, 0);
    put(50, 0xFFE1, 0);
    put(64, 0xFFE9, 0);
    put(65, 0x20, 0);
    put(67, 0xFFBE, 0); // F1
    put(68, 0xFFBF, 0); // F2
    put(22, 0xFF08, 0);
    put(119, 0xFFFF, 0);
    put(133, 0xFFEB, 0);
    put(56, 0x62, 0); // b, its capital implied
    for code in 8..200 {
        if syms[code - 8] == (0, 0) {
            // Every key below 200 carries something, so the spare keys are 200 and up.
            syms[code - 8] = (0x1008_FFA0 + u32::try_from(code).expect("a keycode"), 0);
        }
    }
    Keymap::new(
        8,
        2,
        syms.into_iter()
            .flat_map(|(first, second)| [Keysym(first), Keysym(second)])
            .collect(),
    )
}

fn down(name: &str) -> KeyInput {
    KeyInput::Down {
        keysym: keys::resolve(name).expect("a key"),
        name: name.to_owned(),
    }
}

fn up(name: &str) -> KeyInput {
    KeyInput::Up {
        keysym: keys::resolve(name).expect("a key"),
        name: name.to_owned(),
    }
}

/// Send `inputs`, every stroke taken by the server.
fn send(held: &mut HeldInput, inputs: &[KeyInput]) {
    let keymap = keymap();
    for input in inputs {
        for stroke in held.plan(input, &keymap).expect("planned") {
            held.deliver(stroke, &keymap, || Ok(()), || Ok(()))
                .expect("sent");
        }
    }
}

#[test]
fn the_keymap_says_which_key_and_level_carries_a_keysym() {
    let keymap = keymap();
    assert_eq!(keymap.place(Keysym(0x61)), Some((Keycode(38), false)));
    assert_eq!(keymap.place(Keysym(0x41)), Some((Keycode(38), true)));
    assert_eq!(keymap.place(Keysym(0x21)), Some((Keycode(10), true)));
    assert_eq!(
        keymap.place(Keysym(0x42)),
        Some((Keycode(56), true)),
        "a letter's capital, implied by its lower case alone"
    );
    assert_eq!(keymap.place(Keysym(0xFC)), None);
    assert_eq!(keymap.place(keys::NO_SYMBOL), None);
    assert_eq!(keymap.spare().first(), Some(&Keycode(255)));
    assert_eq!(keymap.spare().last(), Some(&Keycode(200)));
}

#[test]
fn keys_and_text_are_planned_against_the_keymap_and_what_the_lane_holds() {
    let keymap = keymap();
    let held = HeldInput::new();
    assert_eq!(
        held.plan(&down("a"), &keymap),
        Ok(vec![Keystroke::Press(Keycode(38))])
    );
    assert_eq!(
        held.plan(&down("A"), &keymap),
        Ok(vec![
            Keystroke::Press(Keycode(50)),
            Keystroke::Press(Keycode(38)),
            Keystroke::Release(Keycode(50)),
        ]),
        "a capital takes Shift around its press"
    );
    assert_eq!(
        held.plan(&KeyInput::Text("aA".into()), &keymap),
        Ok(vec![
            Keystroke::Press(Keycode(38)),
            Keystroke::Release(Keycode(38)),
            Keystroke::Press(Keycode(50)),
            Keystroke::Press(Keycode(38)),
            Keystroke::Release(Keycode(38)),
            Keystroke::Release(Keycode(50)),
        ])
    );
    // With Shift held by the sequence, a first-level character is typed without it, and Shift
    // is held again after.
    let mut shifted = HeldInput::new();
    send(&mut shifted, &[down("Shift")]);
    assert_eq!(
        shifted.plan(&KeyInput::Text("a".into()), &keymap),
        Ok(vec![
            Keystroke::Release(Keycode(50)),
            Keystroke::Press(Keycode(38)),
            Keystroke::Release(Keycode(38)),
            Keystroke::Press(Keycode(50)),
        ])
    );
    // A character on no key goes through a spare key mapped for the text, each its own key,
    // with a pause after the mapping and before the emptying.
    assert_eq!(
        held.plan(&KeyInput::Text("üaü日".into()), &keymap),
        Ok(vec![
            Keystroke::Map {
                key: Keycode(255),
                keysym: Keysym(0xFC),
            },
            Keystroke::Map {
                key: Keycode(254),
                keysym: Keysym(0x0100_65E5),
            },
            Keystroke::Settle,
            Keystroke::Press(Keycode(255)),
            Keystroke::Release(Keycode(255)),
            Keystroke::Press(Keycode(38)),
            Keystroke::Release(Keycode(38)),
            Keystroke::Press(Keycode(255)),
            Keystroke::Release(Keycode(255)),
            Keystroke::Press(Keycode(254)),
            Keystroke::Release(Keycode(254)),
            Keystroke::Settle,
            Keystroke::Unmap(Keycode(255)),
            Keystroke::Unmap(Keycode(254)),
        ])
    );
    assert!(matches!(
        held.plan(&down("ü"), &keymap),
        Err(Refusal::UnknownKey { .. })
    ));
    assert!(matches!(
        held.plan(&KeyInput::Text("\u{7}".into()), &keymap),
        Err(Refusal::UnsentInput { .. })
    ));
    let crowded = Keymap::new(8, 1, vec![Keysym(0x61), Keysym(0x62)]);
    assert!(matches!(
        held.plan(&KeyInput::Text("ü".into()), &crowded),
        Err(Refusal::NoSpareKey { .. })
    ));
}

#[test]
fn chords_the_desktop_takes_are_refused_and_app_chords_are_not() {
    let keymap = keymap();
    let held = HeldInput::new();
    let refused = |inputs: &[KeyInput]| held.shell_hotkey(inputs, &keymap).expect("planned");
    assert_eq!(
        refused(&[down("Control"), down("a"), up("a"), up("Control")]),
        None
    );
    assert_eq!(
        refused(&[KeyInput::Text("Grüße 日本\n\t".into())]),
        None,
        "typed text"
    );
    assert!(refused(&[down("Meta")]).is_some(), "the Super key");
    assert!(
        refused(&[down("Control"), down("Alt"), down("F2")]).is_some(),
        "the virtual-terminal switch"
    );
    assert!(
        refused(&[down("Control"), down("Alt"), down("Backspace")]).is_some(),
        "the terminate key"
    );
    assert!(refused(&[down("Alt"), down("Tab")]).is_some());
    assert!(
        refused(&[down("Alt"), KeyInput::Text("\t".into())]).is_some(),
        "a typed tab under Alt is the switcher too"
    );
    assert!(refused(&[down("Alt"), down("F1")]).is_some());
    assert_eq!(refused(&[down("F1")]), None);
}

/// Every chord the desktop takes is named by what takes it, and the same keys without the
/// modifiers that make them a desktop chord go to the app.
#[test]
fn each_desktop_chord_names_what_takes_it() {
    use flui_driver::lane::x11::keys::Modifier::{Alt, Control, Shift, Super};
    let rows: &[(u32, &[Modifier], Option<&str>)] = &[
        (0xFFEB, &[], Some("the desktop shell (the Super key)")),
        (0x61, &[Super], Some("the desktop shell (the Super key)")),
        (
            0xFFE5,
            &[],
            Some("the keyboard's lock state, shared by every window"),
        ),
        (
            0xFF7F,
            &[],
            Some("the keyboard's lock state, shared by every window"),
        ),
        (
            0xFF14,
            &[],
            Some("the keyboard's lock state, shared by every window"),
        ),
        (0xFF61, &[], Some("the screenshot tool")),
        (
            0x1008_FF11,
            &[],
            Some("the system's volume and media controls"),
        ),
        (
            0x1008_FF17,
            &[],
            Some("the system's volume and media controls"),
        ),
        (0x1008_FE01, &[], Some("the virtual-terminal switch")),
        (0x1008_FE0C, &[], Some("the virtual-terminal switch")),
        (0xFED5, &[], Some("the X server's terminate key")),
        (0xFFBF, &[Control, Alt], Some("the virtual-terminal switch")),
        (
            0xFF08,
            &[Control, Alt],
            Some("the X server's terminate key"),
        ),
        (
            0xFFFF,
            &[Control, Alt],
            Some("the session's log-out dialog"),
        ),
        (0xFF09, &[Alt], Some("the window manager's switcher")),
        (0xFF1B, &[Alt], Some("the window manager's switcher")),
        (0xFFBF, &[Alt], Some("the window manager's keys")),
        // The same keys under fewer modifiers are the app's.
        (0xFFBF, &[Control], None),
        (0xFF08, &[Alt], None),
        (0xFF08, &[Control], None),
        (0xFFFF, &[Control], None),
        (0xFFFF, &[Alt], None),
        (0xFF09, &[Control, Shift], None),
        (0x1008_FE0D, &[], None),
        (0x61, &[Control, Alt], None),
    ];
    for (keysym, held, expected) in rows {
        assert_eq!(
            safety::press_takes(Keysym(*keysym), held),
            *expected,
            "{keysym:#x} under {held:?}"
        );
    }
}

/// The run of lone Shift taps counts Shift alone: another key pressed starts it again, a key
/// released is no tap, and Shift pressed beside a held key is not alone.
#[test]
fn only_a_lone_shift_counts_toward_sticky_keys() {
    let keymap = keymap();
    let tap = [down("Shift"), up("Shift")];
    // Five releases of a letter are not five Shift taps.
    let mut letters = HeldInput::new();
    for _ in 0..5 {
        send(&mut letters, &[down("a"), up("a")]);
    }
    assert_eq!(letters.shell_hotkey(&tap, &keymap), Ok(None));
    // Four taps, then a key pressed and released: the run starts again.
    let mut broken = HeldInput::new();
    for _ in 0..4 {
        send(&mut broken, &tap);
    }
    send(&mut broken, &[down("a"), up("a")]);
    for _ in 0..4 {
        assert_eq!(broken.shell_hotkey(&tap, &keymap), Ok(None));
        send(&mut broken, &tap);
    }
    assert_eq!(broken.shell_hotkey(&tap, &keymap), Ok(Some(STICKY_KEYS)));
    // Shift pressed while another key is down is not alone: five such taps are no shortcut.
    let held_a: Vec<KeyInput> = std::iter::once(down("a"))
        .chain((0..5).flat_map(|_| [down("Shift"), up("Shift")]))
        .collect::<Vec<KeyInput>>();
    assert_eq!(HeldInput::new().shell_hotkey(&held_a, &keymap), Ok(None));
    // A key pressed while Shift is down ends Shift's being alone, though Shift is released first.
    let crossed: Vec<KeyInput> = (0..5)
        .flat_map(|_| [down("Shift"), down("a"), up("Shift"), up("a")])
        .collect::<Vec<KeyInput>>();
    assert_eq!(HeldInput::new().shell_hotkey(&crossed, &keymap), Ok(None));
    // ... so a crossed tap and four lone ones make four in a row, not five.
    let crossed_then_lone: Vec<KeyInput> = [down("Shift"), down("a"), up("Shift"), up("a")]
        .into_iter()
        .chain((0..4).flat_map(|_| [down("Shift"), up("Shift")]))
        .collect::<Vec<KeyInput>>();
    assert_eq!(
        HeldInput::new().shell_hotkey(&crossed_then_lone, &keymap),
        Ok(None)
    );
    // A tap after a shortcut-free run that ended in a press of Shift itself still counts.
    let mut again = HeldInput::new();
    for _ in 0..4 {
        send(&mut again, &tap);
    }
    send(
        &mut again,
        &[down("Shift"), down("a"), up("a"), up("Shift")],
    );
    for _ in 0..4 {
        send(&mut again, &tap);
    }
    assert_eq!(again.shell_hotkey(&tap, &keymap), Ok(Some(STICKY_KEYS)));
}

#[test]
fn only_the_fifth_shift_tap_in_a_row_is_the_sticky_keys_shortcut() {
    let keymap = keymap();
    let tap = [down("Shift"), up("Shift")];
    let mut held = HeldInput::new();
    for _ in 0..4 {
        assert_eq!(held.shell_hotkey(&tap, &keymap), Ok(None));
        send(&mut held, &tap);
    }
    assert_eq!(held.shell_hotkey(&tap, &keymap), Ok(Some(STICKY_KEYS)));
    // Another key starts the count again.
    send(&mut held, &[down("a"), up("a")]);
    assert_eq!(held.shell_hotkey(&tap, &keymap), Ok(None));
    // Shift held with another key is no tap.
    let mut chorded = HeldInput::new();
    for _ in 0..6 {
        send(
            &mut chorded,
            &[down("Shift"), down("a"), up("a"), up("Shift")],
        );
    }
    assert_eq!(chorded.shell_hotkey(&tap, &keymap), Ok(None));
}

fn refuse() -> Result<()> {
    Err(Error::Lane {
        lane: "x11".into(),
        unavailable: false,
        message: "refused".into(),
    })
}

#[test]
fn a_key_the_server_took_is_held_even_when_its_echo_never_comes() {
    let keymap = keymap();
    let mut held = HeldInput::new();
    let stroke = Keystroke::Press(Keycode(38));
    let echo = held.deliver(stroke, &keymap, || Ok(()), refuse);
    assert!(echo.is_err());
    assert_eq!(held.keys(), vec![Keycode(38)]);
    let sent = held.deliver(Keystroke::Press(Keycode(39)), &keymap, refuse, || Ok(()));
    assert!(sent.is_err());
    assert_eq!(held.keys(), vec![Keycode(38)], "a press the server refused");
}

#[test]
fn a_release_or_emptying_the_server_refused_stays_held() {
    let keymap = keymap();
    let mut held = HeldInput::new();
    send(&mut held, &[down("Control"), down("a")]);
    held.deliver(
        Keystroke::Map {
            key: Keycode(255),
            keysym: Keysym(0xFC),
        },
        &keymap,
        || Ok(()),
        || Ok(()),
    )
    .expect("mapped");
    let mut sent = Vec::new();
    let released = held.release_all(&keymap, |stroke| {
        sent.push(stroke);
        if stroke == Keystroke::Release(Keycode(37)) {
            refuse()
        } else {
            Ok(())
        }
    });
    assert!(
        matches!(&released, Err(Error::ReleaseFailed { lane, held, .. })
            if lane == "x11" && held == &vec!["Control".to_owned()]),
        "{released:?}"
    );
    assert_eq!(
        sent,
        vec![
            // Control is held: the mask key (the highest spare key not mapped) goes first.
            Keystroke::Press(Keycode(254)),
            Keystroke::Release(Keycode(254)),
            Keystroke::Release(Keycode(38)),
            Keystroke::Release(Keycode(37)),
            Keystroke::Unmap(Keycode(255)),
        ],
        "the mask, then the newest key first, the mapped keys last"
    );
    assert_eq!(held.keys(), vec![Keycode(37)]);
    assert!(held.mapped().is_empty(), "every mapped key was emptied");
    held.release_all(&keymap, |_| Ok(())).expect("released");
    assert!(held.keys().is_empty(), "the refused release went out again");

    let mut buttons = HeldButtons::default();
    buttons
        .deliver(1, true, || Ok(()), || Ok(()))
        .expect("down");
    buttons
        .deliver(3, true, || Ok(()), refuse)
        .expect_err("no echo");
    assert_eq!(buttons.bits(), 0b101);
    let released = buttons.release_all(|button| if button == 3 { refuse() } else { Ok(()) });
    assert!(
        matches!(&released, Err(Error::ReleaseFailed { held, .. })
            if held == &vec!["mouse button 3".to_owned()]),
        "{released:?}"
    );
    assert_eq!(buttons.bits(), 0b100);
}

/// What the lane records is what the server took: each key held once, each mapped key once, a
/// key released by the code it was pressed with, a mapping waiting for no echo, and a keymap read
/// at any number of keysyms a key.
#[test]
fn the_held_records_follow_what_the_server_took() {
    let keymap = keymap();
    assert_eq!(keymap.per_keycode(), 2);
    let one_level = Keymap::new(10, 0, vec![Keysym(0x61), Keysym(0x62)]);
    assert_eq!(one_level.place(Keysym(0x62)), Some((Keycode(11), false)));
    let three_levels = Keymap::new(10, 3, vec![Keysym(0x62), keys::NO_SYMBOL, Keysym(0x99)]);
    assert_eq!(
        three_levels.place(Keysym(0x42)),
        Some((Keycode(10), true)),
        "a lower case alone on its first two levels types its capital"
    );

    let mut held = HeldInput::new();
    send(&mut held, &[down("Shift"), down("a"), down("a")]);
    assert_eq!(
        held.keys(),
        vec![Keycode(50), Keycode(38)],
        "a key held once"
    );
    assert_eq!(
        held.plan(&up("a"), &keymap),
        Ok(vec![Keystroke::Release(Keycode(38))]),
        "released by the code it was pressed with"
    );
    // A key the lane does not hold is not released: the plan is empty, so no lone Shift tap goes
    // uncounted on a release the server would drop.
    assert_eq!(HeldInput::new().plan(&up("a"), &keymap), Ok(vec![]));
    assert_eq!(HeldInput::new().plan(&up("Shift"), &keymap), Ok(vec![]));
    // A key pressed for its capital is released by its lower case, by the key the lane holds.
    let mut capital = HeldInput::new();
    send(&mut capital, &[down("A")]);
    assert_eq!(
        capital.plan(&up("a"), &keymap),
        Ok(vec![Keystroke::Release(Keycode(38))])
    );
    let map = Keystroke::Map {
        key: Keycode(255),
        keysym: Keysym(0xFC),
    };
    held.deliver(map, &keymap, || Ok(()), refuse)
        .expect("a mapping waits for no echo");
    held.deliver(map, &keymap, || Ok(()), || Ok(()))
        .expect("mapped again");
    assert_eq!(held.mapped(), vec![Keycode(255)], "a key mapped once");

    // No Shift on the keymap: a capital is typed through a spare key, never its key unshifted.
    let no_shift = Keymap::new(
        8,
        2,
        vec![Keysym(0x61), Keysym(0x41), keys::NO_SYMBOL, keys::NO_SYMBOL],
    );
    assert_eq!(
        HeldInput::new().plan(&KeyInput::Text("A".into()), &no_shift),
        Ok(vec![
            Keystroke::Map {
                key: Keycode(9),
                keysym: Keysym(0x41),
            },
            Keystroke::Settle,
            Keystroke::Press(Keycode(9)),
            Keystroke::Release(Keycode(9)),
            Keystroke::Settle,
            Keystroke::Unmap(Keycode(9)),
        ])
    );
    // As many characters off the keymap as spare keys: planned.
    assert!(
        HeldInput::new()
            .plan(&KeyInput::Text("ü".into()), &no_shift)
            .is_ok()
    );

    // A mask the server refused, every key then released: that first failure, not a release
    // failure, since nothing stays held.
    let mut masked = HeldInput::new();
    send(&mut masked, &[down("Control")]);
    let released = masked.release_all(&keymap, |stroke| match stroke {
        Keystroke::Press(Keycode(255)) => refuse(),
        _ => Ok(()),
    });
    assert!(matches!(released, Err(Error::Lane { .. })), "{released:?}");
    assert!(
        masked.keys().is_empty(),
        "Control was released after the mask"
    );

    // Buttons are released by their own numbers, and one refused is named.
    let mut buttons = HeldButtons::default();
    for button in [1, 3] {
        buttons
            .deliver(button, true, || Ok(()), || Ok(()))
            .expect("down");
    }
    let mut sent = Vec::new();
    let released = buttons.release_all(|button| {
        sent.push(button);
        if button == 1 { refuse() } else { Ok(()) }
    });
    assert_eq!(sent, vec![1, 3]);
    assert!(
        matches!(&released, Err(Error::ReleaseFailed { held, .. })
            if held == &vec!["mouse button 1".to_owned()]),
        "{released:?}"
    );
}

/// The release of everything names everything still held, whichever half failed: a key release
/// that failed plainly and a button release that failed both leave their keys and buttons named.
#[test]
fn a_release_failure_names_every_key_and_button_still_held() {
    let plain = refuse();
    let buttons = Err(Error::ReleaseFailed {
        lane: "x11".to_owned(),
        held: vec!["mouse button 3".to_owned()],
        message: "refused".to_owned(),
    });
    let held = vec!["Control".to_owned(), "mouse button 3".to_owned()];
    let answer = held::release_answer(plain, buttons, held.clone());
    assert!(
        matches!(&answer, Err(Error::ReleaseFailed { held: named, .. }) if named == &held),
        "{answer:?}"
    );
    assert!(held::release_answer(Ok(()), Ok(()), Vec::new()).is_ok());
    assert!(
        matches!(
            held::release_answer(refuse(), Ok(()), Vec::new()),
            Err(Error::Lane { .. })
        ),
        "nothing stays held: the failure itself"
    );
}

/// A wheel click is a press and a release held as a button's are: a click whose echo never came
/// is held, and the release of everything releases it, so no wheel button stays down on the
/// server.
#[test]
fn a_wheel_click_left_down_is_released_with_the_rest() {
    let mut buttons = HeldButtons::default();
    let mut sent = Vec::new();
    let scrolled = buttons.scroll(
        0,
        1,
        |button, pressed| {
            sent.push((button, pressed));
            Ok(())
        },
        refuse,
    );
    assert!(scrolled.is_err(), "the press's echo never came");
    assert_eq!(sent, vec![(5, true)], "nothing after the click left down");
    assert_eq!(buttons.bits(), 1 << 4, "wheel button 5 is held");
    let mut released = Vec::new();
    buttons
        .release_all(|button| {
            released.push(button);
            Ok(())
        })
        .expect("released");
    assert_eq!(released, vec![5]);
    assert_eq!(buttons.bits(), 0);
    // A scroll that goes through turns whole clicks, each released.
    let mut clicks = Vec::new();
    buttons
        .scroll(
            -1,
            2 * WHEEL_STEP,
            |button, pressed| {
                clicks.push((button, pressed));
                Ok(())
            },
            || Ok(()),
        )
        .expect("scrolled");
    assert_eq!(
        clicks,
        vec![
            (5, true),
            (5, false),
            (5, true),
            (5, false),
            (6, true),
            (6, false)
        ]
    );
    assert_eq!(buttons.bits(), 0);
}

#[test]
fn a_scroll_past_the_click_limit_is_refused() {
    let limit = i32::try_from(MAX_WHEEL_CLICKS).expect("small") * WHEEL_STEP;
    assert_eq!(scroll_within(0, limit), Ok(()));
    assert_eq!(scroll_within(-limit, 0), Ok(()));
    for (delta_x, delta_y) in [
        (0, limit + 1),
        (-limit - 1, 0),
        (0, i32::MAX),
        (i32::MIN, 0),
    ] {
        let refused = scroll_within(delta_x, delta_y).expect_err("refused");
        assert!(
            matches!(refused, Refusal::UnsentInput { .. }),
            "({delta_x}, {delta_y}): {refused:?}"
        );
        assert_eq!(code(refused), Some(ErrorCode::NotSupported));
    }
}

/// The release of everything completes no StickyKeys shortcut: with four lone Shift taps counted
/// and Shift held, it presses and releases the mask key first, which ends the run of taps; with no
/// key free for the mask, the release of Shift is not sent, and Shift stays held.
#[test]
fn the_release_of_everything_completes_no_sticky_keys_shortcut() {
    let keymap = keymap();
    let tap = [down("Shift"), up("Shift")];
    let mut held = HeldInput::new();
    for _ in 0..4 {
        send(&mut held, &tap);
    }
    send(&mut held, &[down("Shift")]);
    let mut sent = Vec::new();
    held.release_all(&keymap, |stroke| {
        sent.push(stroke);
        Ok(())
    })
    .expect("released");
    assert_eq!(
        sent,
        vec![
            Keystroke::Press(Keycode(255)),
            Keystroke::Release(Keycode(255)),
            Keystroke::Release(Keycode(50)),
        ]
    );
    assert_eq!(
        held.shell_hotkey(&tap, &keymap),
        Ok(None),
        "the mask ended the run of taps"
    );

    // No key free for the mask: the fifth lone release would complete StickyKeys, so it is not
    // sent, and Shift stays held, named.
    let full = Keymap::new(49, 1, vec![Keysym(0x61), keys::SHIFT_L]);
    let mut cornered = HeldInput::new();
    let mut sent = Vec::new();
    for _ in 0..4 {
        for input in &tap {
            for stroke in cornered.plan(input, &full).expect("planned") {
                cornered
                    .deliver(stroke, &full, || Ok(()), || Ok(()))
                    .expect("sent");
            }
        }
    }
    for stroke in cornered.plan(&down("Shift"), &full).expect("planned") {
        cornered
            .deliver(stroke, &full, || Ok(()), || Ok(()))
            .expect("sent");
    }
    let released = cornered.release_all(&full, |stroke| {
        sent.push(stroke);
        Ok(())
    });
    assert!(sent.is_empty(), "nothing went out: {sent:?}");
    assert!(
        matches!(&released, Err(Error::ReleaseFailed { held, .. })
            if held == &vec!["Shift".to_owned()]),
        "{released:?}"
    );
    assert_eq!(cornered.keys(), vec![Keycode(50)]);
}

#[test]
fn a_scroll_turns_the_wheel_in_whole_clicks() {
    assert_eq!(wheel_clicks(0, true), None);
    assert_eq!(wheel_clicks(1, true), Some((5, 1)));
    assert_eq!(wheel_clicks(-1, true), Some((4, 1)));
    assert_eq!(wheel_clicks(WHEEL_STEP, true), Some((5, 1)));
    assert_eq!(wheel_clicks(WHEEL_STEP + 1, true), Some((5, 2)));
    assert_eq!(wheel_clicks(-3 * WHEEL_STEP, false), Some((6, 3)));
    assert_eq!(wheel_clicks(7, false), Some((7, 1)));
    assert_eq!(
        wheel_clicks(i32::MIN, true).map(|(button, _)| button),
        Some(4)
    );
}

#[test]
fn only_the_xtest_events_the_lane_sent_are_its_own() {
    let mut count = EchoCount::new();
    assert_eq!(
        count.foreign(Observed::new(RawKind::Motion, false)),
        Some("a mouse move"),
        "any other device is the person's"
    );
    assert_eq!(
        count.foreign(Observed::new(RawKind::Key, true)),
        Some("input another program faked through XTEST"),
        "an XTEST event the lane did not send"
    );
    count.sent();
    count.sent();
    assert!(!count.settled());
    assert_eq!(count.foreign(Observed::new(RawKind::Button, true)), None);
    assert_eq!(count.foreign(Observed::new(RawKind::Key, true)), None);
    assert!(count.settled());
    assert_eq!(count.seen(), 2);
    assert!(
        count
            .foreign(Observed::new(RawKind::Motion, true))
            .is_some()
    );
    assert_eq!(
        count.foreign(Observed::new(RawKind::Touch, false)),
        Some("a touch")
    );
    assert_eq!(
        count.foreign(Observed::new(RawKind::Button, false)),
        Some("a mouse button")
    );
    assert_eq!(
        count.foreign(Observed::new(RawKind::Key, false)),
        Some("a key press")
    );
}

#[test]
fn an_image_reads_into_rgba_rows_whatever_its_layout() {
    // Two by two at 32 bits a pixel, least significant byte first: blue, green, red, pad.
    let format = PixmapFormat {
        width: 2,
        height: 2,
        bits_per_pixel: 32,
        scanline_pad: 32,
        lsb_first: true,
        red_mask: 0x00FF_0000,
        green_mask: 0x0000_FF00,
        blue_mask: 0x0000_00FF,
    };
    let data = [
        0x01, 0x02, 0x03, 0, 0x10, 0x20, 0x30, 0, //
        0xFF, 0x00, 0x00, 0, 0x00, 0x00, 0xFF, 0,
    ];
    assert_eq!(
        image::rgba(&format, &data).expect("read"),
        vec![
            0x03, 0x02, 0x01, 0xFF, 0x30, 0x20, 0x10, 0xFF, //
            0x00, 0x00, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0xFF,
        ]
    );
    // The most significant byte first, the same pixels.
    let msb = PixmapFormat {
        lsb_first: false,
        ..format
    };
    let swapped: Vec<u8> = data
        .chunks(4)
        .flat_map(|pixel| pixel.iter().rev().copied().collect::<Vec<u8>>())
        .collect();
    assert_eq!(
        image::rgba(&msb, &swapped).expect("read"),
        image::rgba(&format, &data).expect("read")
    );
    // Sixteen bits a pixel, 5-6-5, each row padded to 32 bits: one pixel of three padding bytes.
    let rgb565 = PixmapFormat {
        width: 1,
        height: 2,
        bits_per_pixel: 16,
        scanline_pad: 32,
        lsb_first: true,
        red_mask: 0xF800,
        green_mask: 0x07E0,
        blue_mask: 0x001F,
    };
    assert_eq!(
        image::rgba(&rgb565, &[0x00, 0xF8, 0, 0, 0x1F, 0x00, 0, 0]).expect("read"),
        vec![0xFF, 0, 0, 0xFF, 0, 0, 0xFF, 0xFF]
    );
    assert!(image::rgba(&format, &data[..12]).is_err(), "too short");
    // 24 bits a pixel, three pixels a row padded to 32 bits: each row is 12 bytes, 9 of them
    // pixels, so the second row starts after three bytes of padding.
    let packed = PixmapFormat {
        width: 3,
        height: 2,
        bits_per_pixel: 24,
        scanline_pad: 32,
        ..format
    };
    let rows = [
        1, 2, 3, 4, 5, 6, 7, 8, 9, 0xEE, 0xEE, 0xEE, //
        10, 11, 12, 13, 14, 15, 16, 17, 18, 0xEE, 0xEE, 0xEE,
    ];
    assert_eq!(
        image::rgba(&packed, &rows).expect("read"),
        vec![
            3, 2, 1, 255, 6, 5, 4, 255, 9, 8, 7, 255, //
            12, 11, 10, 255, 15, 14, 13, 255, 18, 17, 16, 255,
        ]
    );
    assert!(
        image::rgba(&packed, &rows[..23]).is_err(),
        "one byte short of the last row's padding"
    );
    // A five-bit channel is scaled to eight bits with rounding: 16 of 31 is 132, 30 of 31 is 247;
    // a channel no mask names is 0.
    let five = PixmapFormat {
        width: 2,
        height: 1,
        bits_per_pixel: 16,
        scanline_pad: 16,
        lsb_first: true,
        red_mask: 0x001F,
        green_mask: 0,
        blue_mask: 0x001F,
    };
    assert_eq!(
        image::rgba(&five, &[16, 0, 30, 0]).expect("read"),
        vec![132, 0, 132, 255, 247, 0, 247, 255]
    );
    let odd = PixmapFormat {
        bits_per_pixel: 8,
        ..format
    };
    assert!(image::rgba(&odd, &data).is_err(), "eight bits a pixel");
}
