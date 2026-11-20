//! The macOS lane's tables, on every host: keys, roles, the safety rules and their codes, the
//! system's shortcuts, what the guard takes for the person's input, the pointer's events, screen
//! mapping across displays, the window frame's projection, and the release gates.

use flui_driver::coordinates::{DevicePoint, WindowGeometry, to_device};
use flui_driver::lane::OsCapture;
use flui_driver::lane::macos::activity::{
    self, LANE_TAG, Motion, Observed, PRIVATE_STATE, TapEvent, Watcher,
};
use flui_driver::lane::macos::held::{HeldKeys, KeyInput, Keystroke, TEXT_KEY};
use flui_driver::lane::macos::keys::{self, KeyCode, KeyName, Modifier, TextStroke};
use flui_driver::lane::macos::pointer::{
    MouseButton, MouseEvent, MouseKind, Point, Pointer, Step, Wheel,
};
use flui_driver::lane::macos::roles::{self, ROLES};
use flui_driver::lane::macos::safety::{
    self, Desk, KeyEdge, Landing, Permission, Refusal, check_event, press_takes,
};
use flui_driver::lane::macos::screen::{self, PointRect, WindowInfo};
use flui_driver::lane::macos::version::MacosVersion;
use flui_driver::lane::macos::{callback, frame};
use flui_driver::lane::release_outcome;
use flui_driver::lane::windows;
use flui_driver::projection::Projection;
use flui_driver::protocol::error::{ErrorCode, HandleKind};
use flui_driver::protocol::geometry::{LogicalPoint, LogicalSize};
use flui_driver::protocol::input::{PointerButton, PointerDevice};
use flui_driver::protocol::semantics::SemanticsRole;
use flui_driver::protocol::session::Isolation;
use flui_driver::protocol::tree::{ElementId, Node, Tree, normalized_outline};
use flui_driver::protocol::wire::Role;
use flui_driver::{Error, Result};

#[test]
fn a_key_name_maps_to_its_key_code_and_the_character_it_types() {
    let character = |code: u16, shift: bool, character: char| KeyName::Character {
        key: KeyCode(code),
        shift,
        character,
    };
    let rows: &[(&str, Option<KeyName>)] = &[
        ("Enter", Some(KeyName::Named(keys::RETURN))),
        ("Meta", Some(KeyName::Named(keys::COMMAND))),
        ("Alt", Some(KeyName::Named(keys::OPTION))),
        ("Control", Some(KeyName::Named(keys::CONTROL))),
        ("Fn", Some(KeyName::Named(keys::FUNCTION))),
        ("Backspace", Some(KeyName::Named(KeyCode(0x33)))),
        ("Delete", Some(KeyName::Named(KeyCode(0x75)))),
        ("Insert", Some(KeyName::Named(KeyCode(0x72)))),
        ("ArrowUp", Some(KeyName::Named(keys::UP))),
        ("F11", Some(KeyName::Named(keys::F11))),
        ("a", Some(character(0x00, false, 'a'))),
        ("A", Some(character(0x00, true, 'A'))),
        ("z", Some(character(0x06, false, 'z'))),
        ("1", Some(character(0x12, false, '1'))),
        ("!", Some(character(0x12, true, '!'))),
        ("?", Some(character(0x2C, true, '?'))),
        (" ", Some(character(0x31, false, ' '))),
        ("`", Some(character(0x32, false, '`'))),
        // No ANSI key types these: they are typed as text, never pressed.
        ("é", None),
        ("日", None),
        ("PrintScreen", None),
        ("Hyper", None),
        ("", None),
        ("ab", None),
    ];
    for (name, expected) in rows {
        assert_eq!(keys::resolve(name), *expected, "{name:?}");
    }
    assert_eq!(Modifier::of(keys::RIGHT_COMMAND), Some(Modifier::Command));
    assert_eq!(Modifier::of(keys::RETURN), None);
    // What a release that failed names.
    for (code, name) in [
        (0x38, "Shift"),
        (0x37, "Meta"),
        (0x3C, "Shift"),
        (0x00, "a"),
        (0x31, "Space"),
        (0x24, "Enter"),
        (0x6C, "key code 0x6c"),
    ] {
        assert_eq!(keys::name_of(code), name, "{code:#04x}");
    }
}

fn runs(strokes: &[TextStroke]) -> Vec<String> {
    strokes
        .iter()
        .map(|stroke| match stroke {
            TextStroke::Text(chunk) => chunk.text(),
            TextStroke::Key(key) => format!("<{:#04x}>", key.0),
            _ => "?".to_owned(),
        })
        .collect()
}

#[test]
fn text_is_typed_in_runs_of_at_most_twenty_units_and_line_breaks_as_keys() {
    assert_eq!(
        runs(&keys::text_strokes("ab\r\ncd\te\nf\r")),
        ["ab", "<0x24>", "cd", "<0x30>", "e", "<0x24>", "f", "<0x24>"]
    );
    // A carriage return alone is one line break, and the character after it is typed.
    assert_eq!(runs(&keys::text_strokes("a\rb")), ["a", "<0x24>", "b"]);
    let long = "x".repeat(25);
    assert_eq!(
        runs(&keys::text_strokes(&long)),
        ["x".repeat(20), "x".repeat(5)]
    );
    // A chunk is its units: what an earlier run left in the buffer past them does not count.
    let after_a_run = keys::text_strokes(&format!("{}b", "a".repeat(20)))[1];
    let alone = keys::text_strokes("b")[0];
    assert_eq!(after_a_run, alone);
    assert_ne!(alone, keys::text_strokes("c")[0]);
    let hash = |stroke: &TextStroke| -> u64 {
        let mut hasher = std::hash::DefaultHasher::new();
        std::hash::Hash::hash(stroke, &mut hasher);
        std::hash::Hasher::finish(&hasher)
    };
    assert_eq!(hash(&after_a_run), hash(&alone));
    let TextStroke::Text(chunk) = alone else {
        panic!("a run of text: {alone:?}");
    };
    assert_eq!(format!("{chunk:?}"), "TextChunk(\"b\")");
    // A pair of surrogates is never split between two events.
    let edge = format!("{}😀", "x".repeat(19));
    assert_eq!(
        runs(&keys::text_strokes(&edge)),
        ["x".repeat(19), "😀".to_owned()]
    );
    for stroke in keys::text_strokes(&"日本語😀".repeat(9)) {
        if let TextStroke::Text(chunk) = stroke {
            assert!(chunk.units().len() <= keys::TEXT_CHUNK_UNITS);
            assert!(String::from_utf16(chunk.units()).is_ok(), "{chunk:?}");
        }
    }
    assert_eq!(keys::text_strokes(""), Vec::<TextStroke>::new());
}

/// The table was transcribed from the adapter the workspace locks: a bump fails here until the
/// table is read against the new one.
#[test]
fn the_role_table_was_read_against_the_locked_adapter() {
    let lock = include_str!("../../../Cargo.lock");
    let locked = lock
        .split("[[package]]")
        .find(|entry| entry.contains("name = \"accesskit_macos\""))
        .and_then(|entry| {
            entry
                .lines()
                .find_map(|line| line.strip_prefix("version = \""))
                .map(|version| version.trim_end_matches('"'))
        });
    assert_eq!(locked, Some(roles::ADAPTER_CHECKED_AGAINST));
}

/// The scenario app's roles page is built from the Windows table: the macOS table lists the same
/// roles in the same order, so the page has a node for each of its rows.
#[test]
fn every_role_has_a_row_in_the_order_of_the_windows_table() {
    let macos: Vec<SemanticsRole> = ROLES.iter().map(|row| row.role).collect();
    let windows: Vec<SemanticsRole> = windows::roles::ROLES.iter().map(|row| row.role).collect();
    assert_eq!(macos, windows);
    assert!(roles::role_row(SemanticsRole::None).is_none());
    assert_eq!(
        roles::role_row(SemanticsRole::Switch).map(|row| row.native()),
        Some("AXCheckBox/AXSwitch".to_owned())
    );
}

#[test]
fn an_ax_role_folds_to_its_wire_role_with_its_subrole() {
    let rows: &[(&str, Option<&str>, Role)] = &[
        ("AXButton", None, Role::Button),
        ("AXCheckBox", None, Role::CheckBox),
        ("AXCheckBox", Some("AXSwitch"), Role::Switch),
        ("AXCheckBox", Some("AXToggle"), Role::Button),
        ("AXRadioButton", Some("AXTabButton"), Role::Tab),
        ("AXTextField", None, Role::TextInput),
        (
            "AXTextField",
            Some("AXSecureTextField"),
            Role::PasswordInput,
        ),
        ("AXTextArea", None, Role::TextInput),
        ("AXStaticText", None, Role::Label),
        ("Heading", None, Role::Heading),
        ("AXRow", Some("AXOutlineRow"), Role::TreeItem),
        ("AXGroup", None, Role::Group),
        ("AXGroup", Some("AXApplicationStatus"), Role::Status),
        ("AXGroup", Some("AXCodeStyleGroup"), Role::Label),
        ("AXWindow", None, Role::Window),
        ("AXWindow", Some("AXDialog"), Role::Dialog),
        ("AXUnknown", Some("AXApplicationGroup"), Role::Group),
        ("AXLevelIndicator", Some("AXMeter"), Role::ProgressIndicator),
        ("AXGroup", Some("AXMeter"), Role::ProgressIndicator),
        ("AXSheet", None, Role::Dialog),
        ("AXScrollArea", None, Role::Pane),
        ("AXUnknown", None, Role::Unknown),
        ("AXSomethingNew", None, Role::Unknown),
    ];
    for (ax_role, subrole, expected) in rows {
        assert_eq!(
            roles::wire_role(ax_role, *subrole),
            *expected,
            "{ax_role} {subrole:?}"
        );
    }
    assert_eq!(roles::native_role("AXButton", None), "AXButton");
    assert_eq!(
        roles::native_role("AXGroup", Some("AXDocument")),
        "AXGroup/AXDocument"
    );
}

/// The accessibility API keeps every role the app publishes apart, as UI Automation reads it,
/// but the ones listed: those VoiceOver reads as another role too. A change to the adapter or the
/// fold that loses or recovers one fails here.
#[test]
fn the_accessibility_api_folds_each_role_as_ui_automation_does_but_these() {
    let differing: Vec<SemanticsRole> = ROLES
        .iter()
        .zip(windows::roles::ROLES)
        .filter(|(macos, windows)| {
            roles::expected_wire_role(macos) != windows::roles::expected_wire_role(windows)
        })
        .map(|(macos, _)| macos.role)
        .collect();
    assert_eq!(
        differing,
        [
            SemanticsRole::ListBoxOption,
            SemanticsRole::MenuItemCheckbox,
            SemanticsRole::MenuItemRadio,
            SemanticsRole::TreeGrid,
            SemanticsRole::Grid,
            SemanticsRole::GridCell,
            SemanticsRole::ColumnHeader,
            SemanticsRole::RowHeader,
            SemanticsRole::ListItem,
            SemanticsRole::Caption,
            SemanticsRole::Term,
            SemanticsRole::Window,
            SemanticsRole::Pane,
            SemanticsRole::ScrollView,
        ]
    );
}

fn on_target() -> Landing {
    Landing::new(DevicePoint::new(10, 10), true, None)
}

#[test]
fn each_safety_rule_refuses_with_its_code() {
    /// A row: its name, the desktop, where the event lands, the refusal.
    type Row = (&'static str, Desk, Option<Landing>, Option<Refusal>);
    let rows: Vec<Row> = vec![
        ("a quiet desktop", Desk::quiet(), Some(on_target()), None),
        (
            "the window is gone",
            Desk {
                window_alive: false,
                ..Desk::quiet()
            },
            None,
            Some(Refusal::WindowGone),
        ),
        (
            "another window in front",
            Desk {
                in_front: false,
                foreground: Some("Finder (window 7)".into()),
                ..Desk::quiet()
            },
            None,
            Some(Refusal::NotForeground {
                foreground: Some("Finder (window 7)".into()),
            }),
        ),
        (
            "the Dock over the point",
            Desk::quiet(),
            Some(Landing::new(
                DevicePoint::new(10, 10),
                false,
                Some("Dock (window 3)".into()),
            )),
            Some(Refusal::Covered {
                point: DevicePoint::new(10, 10),
                covered_by: Some("Dock (window 3)".into()),
            }),
        ),
        (
            "a mouse button someone holds",
            Desk {
                buttons: 0b10,
                ..Desk::quiet()
            },
            Some(on_target()),
            Some(Refusal::InputHeld {
                what: "a mouse button is held (buttons 0b010)".into(),
            }),
        ),
        (
            "a modifier someone holds",
            Desk {
                modifiers: vec![Modifier::Command],
                ..Desk::quiet()
            },
            None,
            Some(Refusal::InputHeld {
                what: "Command is held".into(),
            }),
        ),
        (
            "a key someone holds",
            Desk {
                other_key: Some(0x0D),
                ..Desk::quiet()
            },
            None,
            Some(Refusal::InputHeld {
                what: "a key (key code 0x0d) is held".into(),
            }),
        ),
    ];
    for (row, desk, landing, expected) in rows {
        assert_eq!(
            check_event(&desk, landing.as_ref()).err(),
            expected,
            "{row}"
        );
    }
    let codes: &[(Refusal, Option<ErrorCode>, Option<&str>)] = &[
        (Refusal::WindowGone, Some(ErrorCode::Gone), None),
        (
            Refusal::NotForeground { foreground: None },
            Some(ErrorCode::NotForeground),
            None,
        ),
        (
            Refusal::Covered {
                point: DevicePoint::new(0, 0),
                covered_by: None,
            },
            Some(ErrorCode::OutsideTarget),
            None,
        ),
        (
            Refusal::InputHeld {
                what: "a key".into(),
            },
            Some(ErrorCode::InputHeld),
            None,
        ),
        (
            Refusal::SystemShortcut {
                handler: "Spotlight",
            },
            Some(ErrorCode::NotSupported),
            Some("os_shell_hotkey"),
        ),
        (
            Refusal::UnknownKey { name: "é".into() },
            Some(ErrorCode::NotSupported),
            Some("os_key"),
        ),
        (
            Refusal::UnsentInput {
                what: "touch".into(),
            },
            Some(ErrorCode::NotSupported),
            Some("os_input_device"),
        ),
        (
            Refusal::MissingPermission {
                permission: Permission::Accessibility,
            },
            Some(ErrorCode::NotSupported),
            Some("macos_permission_accessibility"),
        ),
        (
            Refusal::MissingPermission {
                permission: Permission::InputMonitoring,
            },
            Some(ErrorCode::NotSupported),
            Some("macos_permission_input_monitoring"),
        ),
        (
            Refusal::MissingPermission {
                permission: Permission::ScreenRecording,
            },
            Some(ErrorCode::NotSupported),
            Some("macos_permission_screen_recording"),
        ),
    ];
    for (refusal, code, capability) in codes {
        let error = refusal.clone().into_error();
        assert_eq!(error.code(), *code, "{refusal:?}");
        assert_eq!(
            error.is_cannot_verify(),
            capability.is_some(),
            "{refusal:?}"
        );
        assert_eq!(
            error.missing_capability().as_deref(),
            *capability,
            "{refusal:?}"
        );
    }
    // A missing permission says where it is granted.
    let message = Refusal::MissingPermission {
        permission: Permission::InputMonitoring,
    }
    .into_error()
    .to_string();
    assert!(
        message.contains("Privacy & Security > Input Monitoring"),
        "{message}"
    );
}

/// The shared desktop, guarded, is the one mode the lane runs in; a second session and a virtual
/// machine are refused as cannot-verify until a run on a Mac shows them, and another platform's
/// modes are no argument.
#[test]
fn the_lane_runs_on_the_shared_desktop_and_refuses_unproven_isolation() {
    assert_eq!(safety::admit_isolation(Isolation::Shared), Ok(()));
    for isolation in [Isolation::SecondSession, Isolation::VirtualMachine] {
        let refused = safety::admit_isolation(isolation).expect_err("an unproven mode");
        assert_eq!(refused, Refusal::UnprovenIsolation { isolation });
        let error = refused.into_error();
        assert!(error.is_cannot_verify(), "{error:?}");
        assert_eq!(
            error.missing_capability(),
            Some(format!("macos_isolation_{}", isolation.name()))
        );
    }
    for isolation in [Isolation::InProcess, Isolation::Sandbox, Isolation::Xvfb] {
        let error = safety::admit_isolation(isolation)
            .expect_err("no macOS mode")
            .into_error();
        assert_eq!(error.code(), Some(ErrorCode::InvalidArgument), "{error:?}");
    }
}

fn input_down(name: &str) -> KeyInput {
    match keys::resolve(name) {
        Some(KeyName::Named(key)) => KeyInput::Down {
            key,
            shift: false,
            character: None,
        },
        Some(KeyName::Character {
            key,
            shift,
            character,
        }) => KeyInput::Down {
            key,
            shift,
            character: Some(character),
        },
        other => panic!("{name} is no key: {other:?}"),
    }
}

fn input_up(name: &str) -> KeyInput {
    match keys::resolve(name) {
        Some(KeyName::Named(key)) => KeyInput::Up {
            key,
            character: None,
        },
        Some(KeyName::Character { key, character, .. }) => KeyInput::Up {
            key,
            character: Some(character),
        },
        other => panic!("{name} is no key: {other:?}"),
    }
}

/// A chord as key inputs: each key pressed in order, released in reverse.
fn chord(names: &[&str]) -> Vec<KeyInput> {
    let mut inputs: Vec<KeyInput> = names.iter().map(|name| input_down(name)).collect();
    inputs.extend(names.iter().rev().map(|name| input_up(name)));
    inputs
}

/// Post `inputs` as the lane does, every stroke posted.
fn post_all(keys: &mut HeldKeys, inputs: &[KeyInput]) {
    for input in inputs {
        for stroke in keys.plan(input) {
            keys.deliver(stroke, |_, _| Ok(()))
                .expect("every stroke is posted");
        }
    }
}

#[test]
fn system_shortcuts_are_refused_and_app_chords_are_not() {
    let system: &[(&[&str], &str)] = &[
        (&["Meta", "Tab"], "the app switcher"),
        (&["Meta", "Shift", "Tab"], "the app switcher"),
        (&["Meta", "`"], "the window switcher"),
        (&["Meta", " "], "Spotlight"),
        (&["Meta", "Alt", " "], "Finder's search"),
        (&["Control", "Meta", " "], "the character viewer"),
        (&["Meta", "Alt", "Escape"], "Force Quit"),
        (&["Meta", "Shift", "3"], "the screenshot tool"),
        (&["Meta", "Shift", "4"], "the screenshot tool"),
        (&["Meta", "Shift", "5"], "the screenshot tool"),
        (&["Control", "Meta", "Shift", "4"], "the screenshot tool"),
        (&["Control", "Meta", "q"], "the lock screen"),
        (&["Meta", "Shift", "q"], "log out"),
        (&["Meta", "Alt", "d"], "the Dock"),
        (&["Meta", "F5"], "VoiceOver"),
        (&["Meta", "Alt", "8"], "Zoom and the colour filters"),
        (&["Meta", "Alt", "="], "Zoom"),
        (&["Meta", "Alt", "-"], "Zoom"),
        (&["Control", "ArrowUp"], "Mission Control"),
        // Command with a key it takes nothing for leaves the Control shortcuts in force.
        (&["Control", "Meta", "ArrowUp"], "Mission Control"),
        (&["Control", "ArrowDown"], "Mission Control"),
        (&["Control", "ArrowLeft"], "the switch between Spaces"),
        (&["Control", "2"], "the switch between Spaces"),
        (&["Control", " "], "the input-source switch"),
        (&["Control", "F2"], "the system's keyboard navigation"),
        (&["F11"], "Show Desktop"),
        (
            &["Fn"],
            "the Globe key (the input source, the emoji viewer, dictation)",
        ),
        (
            &["CapsLock"],
            "the keyboard's lock state, shared by every app",
        ),
        (&["AudioVolumeUp"], "the system's volume controls"),
    ];
    let quiet = HeldKeys::new();
    for (keys, handler) in system {
        assert_eq!(
            quiet.system_shortcut(&chord(keys)),
            Some(*handler),
            "{keys:?}"
        );
    }
    let app: &[&[&str]] = &[
        &["Meta", "a"],
        &["Meta", "c"],
        &["Meta", "Shift", "z"],
        &["Meta", "q"],
        &["Meta", "w"],
        &["Control", "a"],
        &["Control", "Tab"],
        &["Alt", "ArrowLeft"],
        &["Shift", "3"],
        &["Shift", "F11"],
        &["Meta", "3"],
        // Without the Option the system's shortcut needs, the app has these.
        &["Meta", "Escape"],
        &["Meta", "d"],
        &["Meta", "8"],
        &["Meta", "="],
        &["Escape"],
        &["Enter"],
        // One Command tap is a key the app reads; Siri takes two in a row.
        &["Meta"],
    ];
    for keys in app {
        assert_eq!(quiet.system_shortcut(&chord(keys)), None, "{keys:?}");
    }
    // Typed text's Tab is a key under the Command the sequence holds.
    let typed = [
        input_down("Meta"),
        KeyInput::Text("\t".into()),
        input_up("Meta"),
    ];
    assert_eq!(quiet.system_shortcut(&typed), Some("the app switcher"));
    assert_eq!(
        press_takes(keys::TAB, &[keys::COMMAND.0]),
        Some("the app switcher")
    );
    assert_eq!(press_takes(keys::TAB, &[]), None);
}

/// Sticky Keys turns on at the fifth Shift tapped alone in a row, Mouse Keys at the fifth Option,
/// Siri at the second Command; counted across the lane's sends, and another key or another
/// modifier between starts the count again.
#[test]
fn lone_modifier_taps_that_toggle_a_system_feature_are_refused() {
    let taps = |name: &str, count: usize| -> Vec<KeyInput> {
        (0..count).flat_map(|_| chord(&[name])).collect()
    };
    let quiet = HeldKeys::new();
    assert_eq!(quiet.system_shortcut(&taps("Shift", 4)), None);
    assert_eq!(
        quiet.system_shortcut(&taps("Shift", 5)),
        Some(safety::STICKY_KEYS)
    );
    assert_eq!(quiet.system_shortcut(&taps("Alt", 4)), None);
    assert_eq!(
        quiet.system_shortcut(&taps("Alt", 5)),
        Some(safety::MOUSE_KEYS)
    );
    assert_eq!(quiet.system_shortcut(&taps("Meta", 1)), None);
    assert_eq!(quiet.system_shortcut(&taps("Meta", 2)), Some(safety::SIRI));
    // Taps posted before count on.
    let mut keys = HeldKeys::new();
    post_all(&mut keys, &taps("Shift", 4));
    assert_eq!(
        keys.system_shortcut(&taps("Shift", 1)),
        Some(safety::STICKY_KEYS)
    );
    // Another key between, typed text included, starts the count again.
    let mut broken = taps("Meta", 1);
    broken.push(KeyInput::Text("x".into()));
    broken.extend(taps("Meta", 1));
    assert_eq!(quiet.system_shortcut(&broken), None);
    let mut broken = taps("Shift", 4);
    broken.extend(chord(&["a"]));
    broken.extend(taps("Shift", 4));
    assert_eq!(quiet.system_shortcut(&broken), None);
    // So does another modifier tapped alone.
    let mut mixed = taps("Meta", 1);
    mixed.extend(taps("Shift", 1));
    mixed.extend(taps("Meta", 1));
    assert_eq!(quiet.system_shortcut(&mixed), None);
    // A sequence that leaves the fifth Shift down alone is refused before its release.
    let mut pending = taps("Shift", 4);
    pending.push(input_down("Shift"));
    assert_eq!(quiet.system_shortcut(&pending), Some(safety::STICKY_KEYS));
    // One down after fewer taps is not the fifth, nor a first Command the second.
    for count in [0, 3] {
        let mut early = taps("Shift", count);
        early.push(input_down("Shift"));
        assert_eq!(quiet.system_shortcut(&early), None, "{count} taps");
    }
    assert_eq!(quiet.system_shortcut(&[input_down("Meta")]), None);
    // A Shift pressed while another key is held is no tap.
    let mut under_a = vec![input_down("a")];
    under_a.extend(taps("Shift", 5));
    under_a.push(input_up("a"));
    assert_eq!(quiet.system_shortcut(&under_a), None);
    // A Shift released after another key went down under it is no tap either, that key still
    // down or not.
    let mut chorded = vec![
        input_down("Shift"),
        input_down("a"),
        input_up("Shift"),
        input_up("a"),
    ];
    chorded.extend(taps("Shift", 4));
    assert_eq!(quiet.system_shortcut(&chorded), None);
    // Another modifier's taps count for that modifier alone, released or held at the end.
    let mut after_option = taps("Alt", 4);
    after_option.extend(taps("Shift", 1));
    assert_eq!(quiet.system_shortcut(&after_option), None);
    let mut held_after_option = taps("Alt", 4);
    held_after_option.push(input_down("Shift"));
    assert_eq!(quiet.system_shortcut(&held_after_option), None);
}

#[test]
fn keys_and_text_are_planned_against_what_the_lane_holds() {
    let mut keys = HeldKeys::new();
    let capital = input_down("A");
    assert_eq!(
        keys.plan(&capital),
        [
            Keystroke::ShiftFor(0x00),
            Keystroke::Press(KeyCode(0x00), Some('A'))
        ]
    );
    post_all(&mut keys, &[capital]);
    assert_eq!(keys.held(), [keys::SHIFT.0, 0x00]);
    assert_eq!(
        keys.plan(&input_up("A")),
        [
            Keystroke::Release(KeyCode(0x00), Some('A')),
            Keystroke::UnshiftFor(0x00)
        ]
    );
    post_all(&mut keys, &[input_up("A")]);
    assert_eq!(keys.held(), [0_u16; 0]);
    // Shift already held: no Shift is added, and none is released for the key.
    post_all(&mut keys, &[input_down("Shift")]);
    assert_eq!(
        keys.plan(&input_down("A")),
        [Keystroke::Press(KeyCode(0x00), Some('A'))]
    );
    let strokes = HeldKeys::new().plan(&KeyInput::Text("hi\n".into()));
    assert!(
        matches!(
            strokes.as_slice(),
            [
                Keystroke::Text(chunk),
                Keystroke::Press(keys::RETURN, None),
                Keystroke::Release(keys::RETURN, None),
            ] if chunk.text() == "hi"
        ),
        "{strokes:?}"
    );
}

/// Every event carries the modifiers the lane holds as its flags, a modifier's own press its
/// flag and its release not, and text carries no key the lane keeps holding.
#[test]
fn every_event_carries_the_modifiers_the_lane_holds() {
    let mut keys = HeldKeys::new();
    let command = Keystroke::Press(keys::COMMAND, None);
    assert_eq!(keys.flags_after(command), Modifier::Command.flag());
    let mut posted = Vec::new();
    keys.deliver(command, |stroke, flags| {
        posted.push((stroke, flags));
        Ok(())
    })
    .expect("posted");
    keys.deliver(
        Keystroke::Press(KeyCode(0x00), Some('a')),
        |stroke, flags| {
            posted.push((stroke, flags));
            Ok(())
        },
    )
    .expect("posted");
    assert_eq!(keys.flags(), Modifier::Command.flag());
    let text = keys.plan(&KeyInput::Text("x".into()));
    for stroke in text {
        keys.deliver(stroke, |stroke, flags| {
            posted.push((stroke, flags));
            Ok(())
        })
        .expect("posted");
    }
    // Text presses and releases its key in one stroke: it leaves held only what was held before
    // it, the key it shares with `a` included.
    assert_eq!(keys.held(), [keys::COMMAND.0, TEXT_KEY.0]);
    assert_eq!(keys.flags_after(Keystroke::Release(keys::COMMAND, None)), 0);
    assert_eq!(
        posted.iter().map(|(_, flags)| *flags).collect::<Vec<_>>(),
        [Modifier::Command.flag(); 3]
    );
}

#[test]
fn a_key_is_held_only_once_its_event_was_posted() {
    let mut keys = HeldKeys::new();
    let refused = keys.deliver(Keystroke::Press(keys::CONTROL, None), |_, _| {
        Err(Error::Lane {
            lane: "macos".into(),
            unavailable: false,
            message: "no event".into(),
        })
    });
    assert!(refused.is_err());
    assert!(keys.held().is_empty(), "a press never posted is not held");
    keys.deliver(Keystroke::Press(keys::CONTROL, None), |_, _| Ok(()))
        .expect("posted");
    let failed = keys.release_all(|_, _| {
        Err(Error::Lane {
            lane: "macos".into(),
            unavailable: false,
            message: "no event".into(),
        })
    });
    assert!(
        matches!(&failed, Err(Error::ReleaseFailed { held, .. }) if held == &["Control"]),
        "the lane names what it could not release: {failed:?}"
    );
    assert_eq!(
        keys.held(),
        [keys::CONTROL.0],
        "a release never posted stays held"
    );
    let mut released = Vec::new();
    keys.release_all(|stroke, flags| {
        released.push((stroke, flags));
        Ok(())
    })
    .expect("released");
    assert_eq!(released, [(Keystroke::Release(keys::CONTROL, None), 0)]);
    assert_eq!(keys.held(), [0_u16; 0]);
}

/// The release of everything never completes a tap shortcut: a release that would is not posted,
/// its key stays held, and the lane says which and why.
#[test]
fn a_release_that_would_complete_a_tap_shortcut_is_not_posted() {
    let mut keys = HeldKeys::new();
    for _ in 0..4 {
        post_all(&mut keys, &chord(&["Shift"]));
    }
    post_all(&mut keys, &[input_down("Shift")]);
    let mut posted = Vec::new();
    let refused = keys.release_all(|stroke, _| {
        posted.push(stroke);
        Ok(())
    });
    assert!(
        matches!(
            &refused,
            Err(Error::ReleaseFailed { held, message, .. })
                if held == &["Shift"] && message.contains(safety::STICKY_KEYS)
        ),
        "{refused:?}"
    );
    assert!(posted.is_empty(), "{posted:?}");
    assert_eq!(keys.held(), [keys::SHIFT.0]);
}

#[test]
fn only_events_with_all_three_marks_of_the_lanes_source_are_its_own() {
    let own = 4242;
    let rows: &[(Observed, Option<&str>)] = &[
        (
            Observed::new(Motion::Move, PRIVATE_STATE, LANE_TAG, 4242),
            None,
        ),
        (
            Observed::new(Motion::Key, PRIVATE_STATE, LANE_TAG, 4242),
            None,
        ),
        // The hardware's own state: a hand on the mouse or the keyboard.
        (Observed::new(Motion::Move, 1, 0, 0), Some("a mouse move")),
        (Observed::new(Motion::Key, 1, 0, 0), Some("a key press")),
        (Observed::new(Motion::Scroll, 1, 0, 0), Some("a scroll")),
        (Observed::new(Motion::Tablet, 1, 0, 0), Some("a pen")),
        // Another program posting from a private source, untagged or under another process.
        (
            Observed::new(Motion::Button, PRIVATE_STATE, 0, 4242),
            Some("a mouse button"),
        ),
        (
            Observed::new(Motion::Button, PRIVATE_STATE, LANE_TAG, 7),
            Some("a mouse button"),
        ),
        // The tag on a source sharing the session's state.
        (
            Observed::new(Motion::Move, 0, LANE_TAG, 4242),
            Some("a mouse move"),
        ),
    ];
    for (observed, expected) in rows {
        assert_eq!(activity::foreign(*observed, own), *expected, "{observed:?}");
    }
}

fn at(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

#[test]
fn a_move_with_a_button_down_is_a_drag_and_a_press_lands_where_the_pointer_went() {
    let mut pointer = Pointer::new();
    let mouse = PointerDevice::Mouse;
    let post = |pointer: &mut Pointer, step: Step| -> Vec<MouseEvent> {
        let events = pointer.plan(step).expect("a step the lane posts");
        pointer
            .deliver(&events, |_| Ok(()))
            .expect("every event posted");
        events
    };
    let kinds = |events: &[MouseEvent]| -> Vec<(MouseKind, Point)> {
        events.iter().map(|event| (event.kind, event.at)).collect()
    };
    assert_eq!(
        kinds(&post(
            &mut pointer,
            Step::Move {
                device: mouse,
                to: at(10.5, 20.5)
            }
        )),
        [(MouseKind::Moved, at(10.5, 20.5))]
    );
    assert_eq!(
        kinds(&post(
            &mut pointer,
            Step::Down {
                device: mouse,
                button: PointerButton::Primary
            }
        )),
        [(MouseKind::Down(MouseButton::LEFT), at(10.5, 20.5))]
    );
    assert_eq!(
        kinds(&post(
            &mut pointer,
            Step::Move {
                device: mouse,
                to: at(30.5, 20.5)
            }
        )),
        [(MouseKind::Dragged(MouseButton::LEFT), at(30.5, 20.5))]
    );
    assert_eq!(
        kinds(&post(
            &mut pointer,
            Step::Up {
                device: mouse,
                button: PointerButton::Primary
            }
        )),
        [(MouseKind::Up(MouseButton::LEFT), at(30.5, 20.5))]
    );
    assert_eq!(
        kinds(&post(
            &mut pointer,
            Step::Move {
                device: mouse,
                to: at(1.5, 1.5)
            }
        )),
        [(MouseKind::Moved, at(1.5, 1.5))]
    );
    // The secondary and middle buttons are their own Quartz buttons, and drag as themselves.
    post(
        &mut pointer,
        Step::Down {
            device: mouse,
            button: PointerButton::Middle,
        },
    );
    assert_eq!(
        kinds(&post(
            &mut pointer,
            Step::Move {
                device: mouse,
                to: at(2.5, 2.5)
            }
        )),
        [(MouseKind::Dragged(MouseButton::MIDDLE), at(2.5, 2.5))]
    );
    assert_eq!(pointer.buttons(), 0b100);
}

fn capability(result: Result<Vec<MouseEvent>>) -> Option<String> {
    match result {
        Err(Error::NotSupported { capability, .. }) => capability,
        other => panic!("not refused as unsupported: {other:?}"),
    }
}

/// The mouse is the one pointer the lane sends: touch has no Quartz event, and a pen's tablet
/// events reach FLUI's AppKit host as the mouse, so both are refused before any event, as is a
/// press with nowhere to land.
#[test]
fn input_the_app_would_not_read_as_itself_is_refused_before_any_event() {
    let pointer = Pointer::new();
    for device in [PointerDevice::Touch, PointerDevice::Pen] {
        assert_eq!(
            capability(pointer.plan(Step::Move {
                device,
                to: at(1.0, 1.0)
            })),
            Some("os_input_device".to_owned()),
            "{device:?}"
        );
        assert_eq!(
            capability(pointer.plan(Step::Down {
                device,
                button: PointerButton::Primary
            })),
            Some("os_input_device".to_owned()),
            "{device:?}"
        );
    }
    assert_eq!(
        capability(pointer.plan(Step::Down {
            device: PointerDevice::Mouse,
            button: PointerButton::Primary
        })),
        Some("os_input_device".to_owned()),
        "a press before the pointer moved anywhere"
    );
    let mut placed = Pointer::new();
    let moved = placed
        .plan(Step::Move {
            device: PointerDevice::Mouse,
            to: at(1.0, 1.0),
        })
        .expect("a mouse move");
    placed.deliver(&moved, |_| Ok(())).expect("posted");
    assert_eq!(
        capability(placed.plan(Step::Up {
            device: PointerDevice::Mouse,
            button: PointerButton::Primary
        })),
        Some("os_input_device".to_owned()),
        "a release of a button that is not down"
    );
}

#[test]
fn a_pointer_change_is_held_only_once_its_event_was_posted() {
    let failing = |_: &MouseEvent| -> Result<()> {
        Err(Error::Lane {
            lane: "macos".into(),
            unavailable: false,
            message: "no event".into(),
        })
    };
    let mut pointer = Pointer::new();
    let moved = pointer
        .plan(Step::Move {
            device: PointerDevice::Mouse,
            to: at(3.5, 3.5),
        })
        .expect("a move");
    pointer.deliver(&moved, |_| Ok(())).expect("posted");
    let down = pointer
        .plan(Step::Down {
            device: PointerDevice::Mouse,
            button: PointerButton::Primary,
        })
        .expect("a press");
    assert!(pointer.deliver(&down, failing).is_err());
    assert_eq!(pointer.buttons(), 0, "a press never posted is not held");
    pointer.deliver(&down, |_| Ok(())).expect("posted");
    assert_eq!(pointer.buttons(), 0b1);
    let failed = pointer.release_all(failing);
    assert!(
        matches!(&failed, Err(Error::ReleaseFailed { held, .. })
            if held == &["the left mouse button"]),
        "the lane names what it could not release: {failed:?}"
    );
    assert_eq!(pointer.buttons(), 0b1, "a release never posted stays held");
    pointer.release_all(|_| Ok(())).expect("released");
    assert_eq!(pointer.buttons(), 0);
}

/// A scroll of device pixels turns the wheels by points, the unit a Quartz pixel-unit scroll
/// counts: the same at 1x, half at 2x, the part of a point carried into the next turn, each wheel
/// the other way round, and a turn past an `i32` refused with nothing carried.
#[test]
fn a_scroll_turns_the_wheels_by_points_the_other_way() {
    let mut wheel = Wheel::new();
    assert_eq!(wheel.turn(0, 40, 1.0).ok(), Some((-40, 0)));
    assert_eq!(wheel.turn(-12, 0, 1.0).ok(), Some((0, 12)));
    assert_eq!(wheel.turn(3, -5, 1.0).ok(), Some((5, -3)));
    let mut retina = Wheel::new();
    assert_eq!(
        retina.turn(0, 40, 2.0).ok(),
        Some((-20, 0)),
        "40 pixels are 20 points"
    );
    assert_eq!(
        retina.turn(0, 1, 2.0).ok(),
        Some((0, 0)),
        "half a point turns nothing yet"
    );
    assert_eq!(
        retina.turn(0, 1, 2.0).ok(),
        Some((-1, 0)),
        "two halves make a point"
    );
    assert_eq!(
        retina.turn(-3, 0, 2.0).ok(),
        Some((0, 1)),
        "the half left over waits"
    );
    assert_eq!(retina.turn(-1, 0, 2.0).ok(), Some((0, 1)));
    let mut wide = Wheel::new();
    for (x, y, scale) in [
        (i32::MIN, 0, 1.0),
        (0, i32::MIN, 1.0),
        (0, i32::MAX, 0.5),
        (0, 1, 0.0),
        (0, 1, -1.0),
        (0, 1, f64::NAN),
        (0, 1, f64::INFINITY),
    ] {
        let refused = wide.turn(x, y, scale);
        assert!(
            matches!(refused, Err(Error::InvalidArgument { .. })),
            "({x}, {y}) at {scale}: {refused:?}"
        );
    }
    assert_eq!(wide, Wheel::new(), "a refused turn carries nothing");
    // The widest turns whose negation an i32 still holds go out.
    assert_eq!(
        Wheel::new().turn(i32::MIN + 1, 0, 1.0).ok(),
        Some((0, i32::MAX))
    );
    assert_eq!(
        Wheel::new().turn(i32::MAX, 0, 1.0).ok(),
        Some((0, i32::MIN + 1))
    );
}

/// The person can take a permission back mid-run: every post asks first, and a release the
/// system no longer permits is refused, its key or button kept held and named.
#[test]
fn a_release_without_accessibility_is_refused_and_names_what_stays_held() {
    let post_without = |granted: bool| {
        safety::permitted(Permission::Accessibility, granted).map_err(Refusal::into_error)
    };
    let mut keys = HeldKeys::new();
    keys.deliver(Keystroke::Press(keys::COMMAND, None), |_, _| Ok(()))
        .expect("posted");
    let refused = keys.release_all(|_, _| post_without(false));
    assert!(
        matches!(&refused, Err(Error::ReleaseFailed { held, message, .. })
            if held == &["Meta"] && message.contains("Accessibility")),
        "{refused:?}"
    );
    assert_eq!(
        keys.held(),
        [keys::COMMAND.0],
        "the release was never posted"
    );
    keys.release_all(|_, _| post_without(true))
        .expect("granted again, the release goes out");
    assert_eq!(keys.held(), [0_u16; 0]);

    let mut pointer = Pointer::new();
    for step in [
        Step::Move {
            device: PointerDevice::Mouse,
            to: at(4.5, 4.5),
        },
        Step::Down {
            device: PointerDevice::Mouse,
            button: PointerButton::Secondary,
        },
    ] {
        let events = pointer.plan(step).expect("a mouse step");
        pointer.deliver(&events, |_| Ok(())).expect("posted");
    }
    let refused = pointer.release_all(|_| post_without(false));
    assert!(
        matches!(&refused, Err(Error::ReleaseFailed { held, .. })
            if held == &["the right mouse button"]),
        "{refused:?}"
    );
    assert_eq!(pointer.buttons(), 0b10);
    assert_eq!(
        safety::permitted(Permission::Accessibility, false)
            .map_err(Refusal::into_error)
            .err()
            .and_then(|error| error.missing_capability()),
        Some("macos_permission_accessibility".to_owned())
    );
}

/// Code macOS calls back into never lets a panic out: it answers the failure its caller chose.
#[test]
fn a_callback_that_panics_answers_its_failure() {
    let answered = callback::contained_or(
        Err("the handler panicked"),
        || -> std::result::Result<u8, &str> { panic!("a completion handler's own failure") },
    );
    assert_eq!(answered, Err("the handler panicked"));
    assert_eq!(callback::contained_or(0, || 7), 7);
}

/// The guard ends the run whenever it can no longer tell the person's input from the lane's: its
/// tap turned off, its watcher gone, or reading an event panicked; and the run then cannot be
/// verified, as on Windows.
#[test]
fn a_guard_that_stopped_watching_ends_the_run_as_cannot_verify() {
    let own = 4242;
    let ours = Observed::new(Motion::Move, PRIVATE_STATE, LANE_TAG, 4242);
    let hand = Observed::new(Motion::Key, 1, 0, 0);
    assert_eq!(
        activity::tap_report(|| TapEvent::Disabled, own),
        Some(activity::STOPPED_WATCHING)
    );
    assert_eq!(activity::tap_report(|| TapEvent::Event(ours), own), None);
    assert_eq!(
        activity::tap_report(|| TapEvent::Event(hand), own),
        Some("a key press")
    );
    assert_eq!(activity::tap_report(|| TapEvent::Other, own), None);
    assert_eq!(
        activity::tap_report(|| panic!("an event's fields could not be read"), own),
        Some(activity::STOPPED_WATCHING)
    );
    assert_eq!(
        activity::guard_report(Watcher::Gone),
        Some(activity::STOPPED_WATCHING)
    );
    assert_eq!(activity::guard_report(Watcher::Quiet), None);
    assert_eq!(
        activity::guard_report(Watcher::Reported("a mouse move")),
        Some("a mouse move")
    );
    let ended = Error::InterruptedByUser {
        lane: "macos".to_owned(),
        event: activity::STOPPED_WATCHING.to_owned(),
    };
    assert!(ended.is_cannot_verify());
}

/// The held-input rule reads every key a keyboard sends for a key someone holds, but the
/// modifiers, which the flags say, and Caps Lock, whose locked state may read as held.
#[test]
fn the_held_scan_leaves_out_the_modifiers_and_caps_lock() {
    assert!(keys::scanned_for_held(0x00), "a letter");
    assert!(keys::scanned_for_held(keys::RETURN.0));
    assert!(keys::scanned_for_held(keys::F11.0));
    assert!(!keys::scanned_for_held(keys::CAPS_LOCK.0));
    for modifier in [
        keys::SHIFT,
        keys::RIGHT_SHIFT,
        keys::CONTROL,
        keys::OPTION,
        keys::COMMAND,
        keys::FUNCTION,
    ] {
        assert!(!keys::scanned_for_held(modifier.0), "{modifier:?}");
    }
    assert!(!keys::scanned_for_held(0x80), "no keyboard sends it");
}

/// A surface point the driver maps to a device pixel comes back, as the point the lane posts at,
/// inside that same backing pixel and inside the window's content: on a standard display, a
/// Retina one, a 3x one, on displays left of and above the main one (negative coordinates), and
/// for content at a fractional point.
#[test]
fn a_surface_point_lands_in_the_backing_pixel_it_was_aimed_at_on_every_display() {
    let size = LogicalSize::new(800.0, 600.0);
    let frames = [
        PointRect::new(100.0, 50.0, 800.0, 628.0),
        PointRect::new(-1440.0, 120.0, 800.0, 628.0),
        PointRect::new(200.0, -900.0, 800.0, 600.0),
        PointRect::new(10.5, 33.25, 800.0, 628.0),
    ];
    let points = [
        (0.0, 0.0),
        (0.25, 0.75),
        (399.5, 299.5),
        (799.9, 599.9),
        (123.456, 7.891),
    ];
    for scale in [1.0, 2.0, 3.0] {
        for frame in frames {
            let content = screen::content_rect(frame, size);
            let geometry = screen::window_geometry(at(content.x, content.y), size, scale);
            for (x, y) in points {
                let device = to_device(&geometry, LogicalPoint::new(x, y))
                    .expect("a point inside the surface maps");
                let posted =
                    screen::points_of(device, scale, content).expect("a pixel in the content");
                assert_eq!(
                    screen::device_of(posted, scale),
                    Some(device),
                    "({x}, {y}) at {scale} in {frame:?}"
                );
                assert!(
                    content.contains(posted),
                    "({x}, {y}) at {scale} in {frame:?} posted at {posted:?}"
                );
            }
        }
    }
    // A content edge half a point into a pixel: the event lands in the half inside the window.
    let content = PointRect::new(10.5, 10.0, 800.0, 600.0);
    assert_eq!(
        screen::points_of(DevicePoint::new(810, 20), 1.0, content).ok(),
        Some(at(810.25, 20.5))
    );
    let outside = screen::points_of(DevicePoint::new(811, 20), 1.0, content);
    assert!(
        matches!(outside, Err(Error::OutsideTarget { .. })),
        "{outside:?}"
    );
    // A pixel that only touches the content's edge has no part inside it.
    let touching = screen::points_of(
        DevicePoint::new(100, 5),
        1.0,
        PointRect::new(0.0, 0.0, 100.0, 100.0),
    );
    assert!(
        matches!(touching, Err(Error::OutsideTarget { .. })),
        "{touching:?}"
    );
    for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let refused = screen::points_of(DevicePoint::new(11, 11), scale, content);
        assert!(
            matches!(refused, Err(Error::InvalidArgument { .. })),
            "{scale}: {refused:?}"
        );
    }
    // A pixel an i32 addresses maps, to its last one each way; one past it does not.
    let edge = |value: f64| screen::device_of(at(value, 0.0), 1.0).map(|device| device.x);
    assert_eq!(edge(f64::from(i32::MIN)), Some(i32::MIN));
    assert_eq!(edge(f64::from(i32::MAX)), Some(i32::MAX));
    assert_eq!(edge(f64::from(i32::MIN) - 1.0), None);
    assert_eq!(edge(f64::from(i32::MAX) + 1.0), None);
    assert_eq!(edge(f64::NEG_INFINITY), None);
    assert_eq!(screen::device_of(at(f64::NAN, 0.0), 2.0), None);
    assert_eq!(screen::device_of(at(f64::MAX, 0.0), 2.0), None);
}

/// AppKit lays a window out from the bottom left: the content is flush with the frame's bottom
/// and left edges, the title bar above it; a full-size content view fills the frame.
#[test]
fn the_content_sits_at_the_bottom_left_of_its_window_frame() {
    let size = LogicalSize::new(800.0, 600.0);
    assert_eq!(
        screen::content_origin(PointRect::new(100.0, 50.0, 800.0, 628.0), size),
        at(100.0, 78.0)
    );
    assert_eq!(
        screen::content_origin(PointRect::new(-1440.0, -300.0, 800.0, 600.0), size),
        at(-1440.0, -300.0)
    );
    let rows: &[(PointRect, bool)] = &[
        (PointRect::new(0.0, 0.0, 800.0, 628.0), true),
        (PointRect::new(0.0, 0.0, 800.0, 600.0), true),
        (PointRect::new(0.0, 0.0, 800.5, 628.0), true),
        (PointRect::new(0.0, 0.0, 801.0, 628.0), true),
        (PointRect::new(0.0, 0.0, 801.5, 628.0), false),
        (PointRect::new(0.0, 0.0, 820.0, 628.0), false),
        (PointRect::new(0.0, 0.0, 800.0, 590.0), false),
        (PointRect::new(0.0, 0.0, 800.0, 800.0), false),
    ];
    for (frame, expected) in rows {
        assert_eq!(screen::is_window(*frame, size), *expected, "{frame:?}");
    }
    let geometry: WindowGeometry = screen::window_geometry(at(100.0, 78.0), size, 2.0);
    assert_eq!(
        to_device(&geometry, LogicalPoint::new(0.0, 0.0)).ok(),
        Some(DevicePoint::new(200, 156))
    );
}

#[test]
fn the_window_under_a_point_is_the_frontmost_that_shows_there() {
    let windows = [
        WindowInfo::new(
            1,
            90,
            25,
            PointRect::new(0.0, 0.0, 1440.0, 25.0),
            1.0,
            "Window Server",
        ),
        WindowInfo::new(
            2,
            91,
            20,
            PointRect::new(300.0, 820.0, 800.0, 80.0),
            1.0,
            "Dock",
        ),
        WindowInfo::new(
            3,
            92,
            0,
            PointRect::new(0.0, 0.0, 1440.0, 900.0),
            0.0,
            "Overlay",
        ),
        WindowInfo::new(
            4,
            93,
            0,
            PointRect::new(100.0, 50.0, 800.0, 628.0),
            1.0,
            "scenario",
        ),
        WindowInfo::new(
            5,
            94,
            0,
            PointRect::new(0.0, 0.0, 1440.0, 900.0),
            1.0,
            "Finder",
        ),
    ];
    let number = |point: Point| screen::window_at(&windows, point).map(|window| window.number);
    assert_eq!(number(at(10.0, 10.0)), Some(1), "the menu bar");
    assert_eq!(number(at(400.0, 850.0)), Some(2), "the Dock");
    assert_eq!(
        number(at(150.0, 100.0)),
        Some(4),
        "the app, past a clear overlay"
    );
    assert_eq!(number(at(1000.0, 500.0)), Some(5), "the desktop's window");
    assert_eq!(
        number(at(900.0, 100.0)),
        Some(5),
        "the frame's right edge is outside"
    );
    assert_eq!(number(at(-5.0, 10.0)), None);
    assert_eq!(
        screen::front_window(&windows).map(|window| window.number),
        Some(4),
        "the frontmost ordinary, visible window"
    );
    assert_eq!(windows[3].describe(), "scenario (window 4)");
}

#[test]
fn a_window_capture_is_cut_to_its_content() {
    // A 3 x 5 frame whose pixel at (x, y) is [x, y, 0, 255]: the bottom three rows are content.
    let (width, height) = (3_u32, 5_u32);
    let rgba = (0..height)
        .flat_map(|y| (0..width).flat_map(move |x| [x as u8, y as u8, 0, 255]))
        .collect();
    let frame = OsCapture::try_new(width, height, rgba).expect("a capture");
    let content =
        screen::crop_content(&frame, LogicalSize::new(3.0, 3.0), 1.0).expect("the content fits");
    assert_eq!((content.width, content.height), (3, 3));
    assert_eq!(content.pixel(0, 0), Some([0, 2, 0, 255]));
    assert_eq!(content.pixel(2, 2), Some([2, 4, 0, 255]));
    let half = screen::crop_content(&frame, LogicalSize::new(1.0, 1.0), 2.0).expect("fits");
    assert_eq!((half.width, half.height), (2, 2));
    assert_eq!(half.pixel(1, 0), Some([1, 3, 0, 255]));
    let empty = screen::crop_content(&frame, LogicalSize::new(0.0, 0.0), 1.0).expect("fits");
    assert_eq!((empty.width, empty.height), (0, 0));
    // A frame with no title bar is all content.
    let whole = screen::crop_content(&frame, LogicalSize::new(3.0, 5.0), 1.0).expect("fits");
    assert_eq!((whole.width, whole.height), (3, 5));
    assert_eq!(whole.pixel(2, 4), Some([2, 4, 0, 255]));
    assert!(screen::crop_content(&frame, LogicalSize::new(3.0, 6.0), 1.0).is_err());
    assert!(screen::crop_content(&frame, LogicalSize::new(4.0, 1.0), 1.0).is_err());
    let negative = screen::crop_content(&frame, LogicalSize::new(-1.0, 1.0), 1.0);
    assert!(
        matches!(&negative, Err(Error::InvalidArgument { message, .. }) if message.contains("no size")),
        "{negative:?}"
    );
}

fn node(id: u64, role: Role, native: &str, name: Option<&str>) -> Node {
    let mut node = Node::new(
        ElementId::from_u64(id).expect("a non-zero handle"),
        role,
        native,
    );
    node.name = name.map(str::to_owned);
    node
}

/// The window frame's own buttons are left out, and the group the adapter reports for the app's
/// root node gives its children to the window: the projection of the OS tree is then the app's.
#[test]
fn the_frame_controls_are_left_out_and_the_content_root_gives_the_window_its_children() {
    let mut like = node(10, Role::Button, "AXButton", Some("Like"));
    like.automation_id = Some("like".to_owned());
    let mut content = node(3, Role::Group, "AXGroup", None);
    content.children = vec![like];
    let mut os_window = node(1, Role::Window, "AXWindow", Some("flui-driver scenario"));
    os_window.children = vec![
        node(
            2,
            Role::Button,
            &roles::native_role("AXButton", Some("AXCloseButton")),
            None,
        ),
        content,
        node(
            4,
            Role::Button,
            &roles::native_role("AXButton", Some("AXZoomButton")),
            None,
        ),
        node(
            5,
            Role::Button,
            &roles::native_role("AXButton", Some("AXMinimizeButton")),
            None,
        ),
    ];
    let projected = frame::project(os_window.clone());
    assert_eq!(projected.children.len(), 1, "{projected:#?}");
    assert_eq!(projected.children[0].name.as_deref(), Some("Like"));
    assert!(!projected.children_unread);
    // Children the walk left unread under the lifted group are the window's now.
    let mut cut = os_window;
    cut.children[1].children_unread = true;
    assert!(frame::project(cut).children_unread);

    let mut app_like = node(7, Role::Button, "Button", Some("Like"));
    app_like.identifier = Some("like".to_owned());
    let mut app_window = node(1, Role::Window, "Window", None);
    app_window.children = vec![app_like];
    let app = Tree::new(vec![app_window], false);
    let os = Tree::new(vec![projected], false);
    assert_eq!(
        normalized_outline(&app.roots),
        normalized_outline(&os.roots)
    );
    assert!(Projection::of(&app, &os).is_equal());

    // A named group is the app's own node, never lifted.
    let mut named = node(3, Role::Group, "AXGroup", Some("Toolbar"));
    named.children = vec![node(10, Role::Button, "AXButton", Some("Like"))];
    let mut window = node(1, Role::Window, "AXWindow", None);
    window.children = vec![named];
    assert_eq!(
        frame::project(window).children[0].name.as_deref(),
        Some("Toolbar")
    );
}

#[test]
fn a_release_is_read_by_its_version_and_gates_its_calls() {
    let rows: &[(&str, Option<MacosVersion>)] = &[
        ("14.5", Some(MacosVersion::new(14, 5))),
        ("10.15.7", Some(MacosVersion::new(10, 15))),
        ("26.0", Some(MacosVersion::new(26, 0))),
        ("15", Some(MacosVersion::new(15, 0))),
        (" 13.6.1\n", Some(MacosVersion::new(13, 6))),
        ("", None),
        ("fourteen", None),
        ("14.x", None),
        ("14.5.1.2", None),
    ];
    for (text, expected) in rows {
        assert_eq!(MacosVersion::parse(text), *expected, "{text:?}");
    }
    assert!(!MacosVersion::new(10, 14).has_privacy_preflight());
    assert!(MacosVersion::new(10, 15).has_privacy_preflight());
    assert!(!MacosVersion::new(13, 6).has_window_screenshot());
    assert!(MacosVersion::new(14, 0).has_window_screenshot());
}

fn no_event() -> Error {
    Error::Lane {
        lane: "macos".into(),
        unavailable: false,
        message: "no event".into(),
    }
}

/// The Shift the lane adds for a key goes with that key: released after it, never for another
/// key, and owed no more once the key or the Shift is up; typed text holds nothing.
#[test]
fn the_shift_the_lane_adds_for_a_key_goes_with_it() {
    let a = KeyCode(0x00);
    let mut keys = HeldKeys::new();
    post_all(&mut keys, &[input_down("a")]);
    assert_eq!(
        keys.plan(&input_up("a")),
        [Keystroke::Release(a, Some('a'))],
        "a key pressed without an added Shift is released alone"
    );

    let mut keys = HeldKeys::new();
    post_all(
        &mut keys,
        &[input_down("A"), input_down("b"), input_up("b")],
    );
    assert_eq!(
        keys.plan(&input_up("A")),
        [Keystroke::Release(a, Some('A')), Keystroke::UnshiftFor(a.0)],
        "another key's release leaves the Shift owed to its own key"
    );

    let mut keys = HeldKeys::new();
    post_all(&mut keys, &[input_down("A")]);
    let _ = keys.apply(Keystroke::Release(a, Some('A')));
    assert_eq!(keys.held(), [keys::SHIFT.0]);
    assert_eq!(
        keys.plan(&input_up("A")),
        [Keystroke::Release(a, Some('A'))],
        "a released key is owed no Shift"
    );

    let mut keys = HeldKeys::new();
    post_all(&mut keys, &[input_down("A")]);
    let _ = keys.apply(Keystroke::Release(keys::SHIFT, None));
    assert_eq!(
        keys.plan(&input_up("A")),
        [Keystroke::Release(a, Some('A'))],
        "a Shift already up is owed to nothing"
    );

    let mut keys = HeldKeys::new();
    post_all(&mut keys, &[KeyInput::Text("x".into())]);
    assert!(keys.held().is_empty(), "{:?}", keys.held());
    let text = keys.plan(&KeyInput::Text("x".into()));
    assert_eq!(
        text[0].edges(),
        [KeyEdge::Down(TEXT_KEY), KeyEdge::Up(TEXT_KEY)]
    );
}

/// The release of everything records each key it released, and names only the ones it could not.
#[test]
fn the_release_of_everything_keeps_what_it_released_apart_from_what_failed() {
    let mut keys = HeldKeys::new();
    post_all(&mut keys, &[input_down("Control"), input_down("a")]);
    let failed = keys.release_all(|stroke, _| match stroke {
        Keystroke::Release(key, _) if key == keys::CONTROL => Err(no_event()),
        _ => Ok(()),
    });
    assert!(
        matches!(&failed, Err(Error::ReleaseFailed { held, .. }) if held == &["Control"]),
        "{failed:?}"
    );
    assert_eq!(keys.held(), [keys::CONTROL.0]);

    // The lane's outcome names what the keys and the mouse both still hold, the keys' failure
    // first.
    let stuck = |held: &str, message: &str| -> Result<()> {
        Err(Error::ReleaseFailed {
            lane: "macos".into(),
            held: vec![held.to_owned()],
            message: message.to_owned(),
        })
    };
    let both = release_outcome(
        stuck("Control", "keys"),
        stuck("the left mouse button", "mouse"),
    );
    assert!(
        matches!(&both, Err(Error::ReleaseFailed { held, message, .. })
            if held == &["Control", "the left mouse button"] && message == "keys"),
        "{both:?}"
    );
    let mouse = release_outcome(Ok(()), stuck("the left mouse button", "mouse"));
    assert!(
        matches!(&mouse, Err(Error::ReleaseFailed { message, .. }) if message == "mouse"),
        "{mouse:?}"
    );
    assert!(release_outcome(Ok(()), Ok(())).is_ok());
}

/// A press needs a place and a button that is up, a release a button that is down; each button a
/// pointer names is its Quartz number; the release of everything posts one release per button
/// held, and names what it could not release.
#[test]
fn each_mouse_button_is_pressed_once_and_released_where_the_pointer_is() {
    let device = PointerDevice::Mouse;
    let down = |button| Step::Down { device, button };
    let up = |button| Step::Up { device, button };
    let mut pointer = Pointer::new();
    assert_eq!(pointer.position(), None);
    for step in [down(PointerButton::Primary), up(PointerButton::Primary)] {
        assert_eq!(
            capability(pointer.plan(step)),
            Some("os_input_device".to_owned()),
            "{step:?} before the pointer moved"
        );
    }
    let to = at(5.5, 6.5);
    let moved = pointer.plan(Step::Move { device, to }).expect("a move");
    pointer.deliver(&moved, |_| Ok(())).expect("posted");
    assert_eq!(pointer.position(), Some(to));
    for (button, quartz) in [
        (PointerButton::Middle, MouseButton::MIDDLE),
        (PointerButton::Back, MouseButton::BACK),
        (PointerButton::Forward, MouseButton::FORWARD),
    ] {
        let pressed = pointer.plan(down(button)).expect("a press");
        assert_eq!(
            pressed.iter().map(|event| event.kind).collect::<Vec<_>>(),
            [MouseKind::Down(quartz)]
        );
        pointer.deliver(&pressed, |_| Ok(())).expect("posted");
        assert_eq!(
            capability(pointer.plan(down(button))),
            Some("os_input_device".to_owned()),
            "a second press of {button:?}"
        );
    }
    assert_eq!(MouseButton::FORWARD.number(), 4);
    let mut released = Vec::new();
    pointer
        .release_all(|event| {
            released.push((event.kind, event.at));
            Ok(())
        })
        .expect("released");
    assert_eq!(
        released,
        [
            (MouseKind::Up(MouseButton::MIDDLE), to),
            (MouseKind::Up(MouseButton::BACK), to),
            (MouseKind::Up(MouseButton::FORWARD), to),
        ]
    );
    assert_eq!(pointer.buttons(), 0);

    for button in [PointerButton::Primary, PointerButton::Secondary] {
        let pressed = pointer.plan(down(button)).expect("a press");
        pointer.deliver(&pressed, |_| Ok(())).expect("posted");
    }
    let failed = pointer.release_all(|event| match event.kind {
        MouseKind::Up(MouseButton::RIGHT) => Err(no_event()),
        _ => Ok(()),
    });
    assert!(
        matches!(&failed, Err(Error::ReleaseFailed { held, .. })
            if held == &["the right mouse button"]),
        "{failed:?}"
    );
    assert_eq!(pointer.buttons(), 0b10);

    // A button recorded down on a pointer that never moved has nowhere to be released: it stays
    // held and is named.
    let mut lost = Pointer::new();
    for button in [
        PointerButton::Middle,
        PointerButton::Back,
        PointerButton::Forward,
    ] {
        lost.apply(&pointer.plan(down(button)).expect("a press")[0]);
    }
    let mut posted = 0;
    let stranded = lost.release_all(|_| {
        posted += 1;
        Ok(())
    });
    assert!(
        matches!(&stranded, Err(Error::ReleaseFailed { held, .. })
        if held == &[
            "the middle mouse button",
            "the back mouse button",
            "the forward mouse button",
        ]),
        "{stranded:?}"
    );
    assert_eq!(posted, 0);
}

#[test]
fn a_window_that_is_gone_is_named_as_the_window_handle() {
    let gone = Refusal::WindowGone.into_error();
    assert!(
        matches!(&gone, Error::Gone {
            kind: Some(HandleKind::Window),
            handle: None,
            exit: None,
            message,
            ..
        } if message.contains("window is gone")),
        "{gone:?}"
    );
}
