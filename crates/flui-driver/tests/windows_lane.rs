//! The Windows lane's tables, on every host: keys, roles, the safety rules and their codes, what
//! the guard takes for the person's input, touch and pen frames, screen mapping, and the
//! projection of a tree.

use flui_driver::coordinates::{DevicePoint, to_device};
use flui_driver::lane::windows::activity::{self, LANE_TAG, MouseMotion, Observed};
use flui_driver::lane::windows::contacts::{Contacts, Phase, PointerKind};
use flui_driver::lane::windows::keys::{self, KeyName, Modifier, TextStroke, VirtualKey};
use flui_driver::lane::windows::roles::{self, ControlType, ROLES, RoleFlags};
use flui_driver::lane::windows::safety::{
    self, Desk, Held, KeyEdge, Landing, Refusal, check_event,
};
use flui_driver::lane::windows::screen;
use flui_driver::projection::{Projection, ProjectionDifference};
use flui_driver::protocol::error::ErrorCode;
use flui_driver::protocol::geometry::{LogicalPoint, LogicalSize};
use flui_driver::protocol::semantics::SemanticsRole;
use flui_driver::protocol::session::Isolation;
use flui_driver::protocol::tree::{ElementId, Node, Tree, normalized_outline};
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

/// A chord as its key edges: each key pressed in order, released in reverse.
fn chord(names: &[&str]) -> Vec<KeyEdge> {
    let keys: Vec<VirtualKey> = names
        .iter()
        .map(|name| match keys::resolve(name) {
            Some(KeyName::Virtual { key, .. }) => key,
            other => panic!("{name} is no virtual key: {other:?}"),
        })
        .collect();
    let mut edges: Vec<KeyEdge> = keys.iter().map(|key| KeyEdge::Down(*key)).collect();
    edges.extend(keys.iter().rev().map(|key| KeyEdge::Up(*key)));
    edges
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
        &["Alt", "Shift"],
        &["Control", "Shift"],
        &["Shift"],
        &["CapsLock"],
        &["NumLock"],
        &["PrintScreen"],
        &["AudioVolumeUp"],
    ];
    for keys in shell {
        assert!(
            safety::shell_hotkey(&chord(keys)).is_some(),
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
    ];
    for keys in app {
        assert_eq!(
            safety::shell_hotkey(&chord(keys)),
            None,
            "{keys:?} is the app's"
        );
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

#[test]
fn touch_and_pen_frames_carry_every_contact_and_their_phases() {
    let mut contacts = Contacts::new();
    assert_eq!(
        contacts.move_to("a", PointerKind::Touch, point(10, 10)),
        None,
        "a touch that is not down only places itself"
    );
    let down = contacts
        .down("a", PointerKind::Touch)
        .expect("a touch press");
    assert_eq!(down.contacts.len(), 1);
    assert_eq!(down.contacts[0].phase, Phase::Down);
    assert_eq!(down.contacts[0].id, 0);
    contacts.move_to("b", PointerKind::Touch, point(50, 10));
    let second = contacts
        .down("b", PointerKind::Touch)
        .expect("a second press");
    let phases: Vec<(u32, Phase)> = second.contacts.iter().map(|c| (c.id, c.phase)).collect();
    assert_eq!(phases, [(0, Phase::Update), (1, Phase::Down)]);
    let moved = contacts
        .move_to("a", PointerKind::Touch, point(12, 10))
        .expect("a contact that is down moves");
    assert_eq!(moved.contacts[0].at, point(12, 10));
    assert_eq!(moved.contacts[1].phase, Phase::Update);
    let lifted = contacts.up("a", PointerKind::Touch).expect("a release");
    let phases: Vec<(u32, Phase)> = lifted.contacts.iter().map(|c| (c.id, c.phase)).collect();
    assert_eq!(phases, [(0, Phase::Up), (1, Phase::Update)]);
    // The freed id is the lowest, and the next finger takes it.
    contacts.move_to("c", PointerKind::Touch, point(90, 10));
    let third = contacts
        .down("c", PointerKind::Touch)
        .expect("a third press");
    assert!(
        third
            .contacts
            .iter()
            .any(|c| c.id == 0 && c.phase == Phase::Down)
    );
    assert!(
        contacts.up("a", PointerKind::Touch).is_err(),
        "a lifted contact is up"
    );
    assert!(
        contacts.down("never-moved", PointerKind::Touch).is_err(),
        "a press with no position is refused"
    );

    let hover = contacts
        .move_to("pen", PointerKind::Pen, point(5, 5))
        .expect("a pen in range hovers");
    assert_eq!(hover.contacts[0].phase, Phase::Hover);
    assert_eq!(
        contacts
            .down("pen", PointerKind::Pen)
            .expect("a pen press")
            .contacts[0]
            .phase,
        Phase::Down
    );
    assert_eq!(
        contacts
            .move_to("pen", PointerKind::Pen, point(6, 5))
            .expect("a pen stroke")
            .contacts[0]
            .phase,
        Phase::Update
    );

    let released = contacts.release_all();
    let summary: Vec<(PointerKind, Vec<Phase>)> = released
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
    assert!(
        contacts.release_all().is_empty(),
        "nothing is left to release"
    );

    let mut many = Contacts::new();
    for finger in 0..10 {
        let id = format!("f{finger}");
        many.move_to(&id, PointerKind::Touch, point(finger, 0));
        many.down(&id, PointerKind::Touch).expect("ten fingers fit");
    }
    many.move_to("f10", PointerKind::Touch, point(10, 0));
    assert!(
        many.down("f10", PointerKind::Touch).is_err(),
        "an eleventh contact is refused"
    );
}

#[test]
fn a_client_origin_maps_back_to_its_device_pixel_at_every_scale() {
    for dpi in [72, 96, 120, 144, 168, 192, 240, 288, 384, 768] {
        let scale = screen::scale_of(dpi);
        for device in (-7_681..7_681).step_by(13) {
            let geometry = screen::window_geometry(
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
