//! The Android lane's read of the OS accessibility tree on every host: the role table against
//! `accesskit_android`'s class choices, a dump matched to the app's tree by structure, role, name,
//! state and bounds, its refusals (two candidates alike, bounds off the display, a screen reader
//! running), and the lane reading and refusing through a scripted device.
//!
//! The dumps follow what `accesskit_android` 0.9.1 (`NodeWrapper::populate_node_info`) hands
//! Android and AOSP's `AccessibilityNodeInfoDumper` writes: the class from the node's AccessKit
//! role, the label (a static text's value) as the content description, the value as the text,
//! `checkable` and `checked` from a toggled state, `enabled`, and the bounding box placed at the
//! app's view on the display and clipped to it. [`served`] writes one from an app's own tree, as
//! the adapter would serve it; `android/accessibility.xml` is the scenario app's window written
//! out by hand.

use std::fmt::Write as _;

use flui_driver::Error;
use flui_driver::coordinates::DevicePoint;
use flui_driver::lane::android::adb::AdbReply;
use flui_driver::lane::android::device::{self, ScreenReader};
use flui_driver::lane::android::roles::{self, ROLES, VIEW};
use flui_driver::lane::android::safety::{self, Refusal};
use flui_driver::lane::android::tree::{self, Mapping, TreeError};
use flui_driver::lane::android::uiautomator::UiDump;
use flui_driver::lane::android::{self as android, AndroidLane};
use flui_driver::lane::windows::roles as uia_roles;
use flui_driver::lane::{AccessibilityApi, Lane as _};
use flui_driver::projection::Projection;
use flui_driver::protocol::semantics::SemanticsRole;
use flui_driver::protocol::tree::{ElementId, Node, Rect, Tree, WindowId};
use flui_driver::protocol::wire::{Checked, Role};

use crate::android_lane::{PACKAGE, placement, scripted};

/// The scenario app's window as `accesskit_android` serves it, written out by hand.
const CONTROLS: &str = include_str!("android/accessibility.xml");

/// Where the fixture's surface sits: below a 60-pixel status bar of a 1080 by 2400 display.
const MAPPING: Mapping = Mapping {
    origin: DevicePoint::new(0, 60),
    display: (1080, 2400),
};

fn id(number: u64) -> ElementId {
    ElementId::from_u64(number).expect("test handles are non-zero")
}

/// An app node of `role`, published as the AccessKit role `accesskit`, named `name`, at `rect` on
/// the surface.
fn app_node(
    number: u64,
    role: Role,
    accesskit: &str,
    name: Option<&str>,
    rect: (i32, i32, u32, u32),
) -> Node {
    let mut node = Node::new(id(number), role, accesskit);
    node.name = name.map(str::to_owned);
    node.surface_rect = Some(Rect {
        x: rect.0,
        y: rect.1,
        width: rect.2,
        height: rect.3,
    });
    node
}

fn identified(mut node: Node, identifier: &str) -> Node {
    node.identifier = Some(identifier.to_owned());
    node
}

/// The scenario app's tree as its read publishes it, the window [`CONTROLS`] shows.
fn controls() -> Tree {
    let mut root = app_node(1, Role::Window, "Window", None, (0, 0, 1080, 2340));
    root.window = Some("w1".parse::<WindowId>().expect("a window handle"));
    let mut subscribe = identified(
        app_node(
            5,
            Role::Switch,
            "Switch",
            Some("Subscribe"),
            (0, 150, 320, 80),
        ),
        "subscribe",
    );
    subscribe.checked = Some(Checked::False);
    let mut comment = identified(
        app_node(6, Role::TextInput, "TextInput", None, (0, 230, 400, 60)),
        "comment",
    );
    comment.value = Some(String::new());
    root.children = vec![
        app_node(2, Role::Label, "Label", Some("Gallery"), (0, 0, 140, 34)),
        identified(
            app_node(3, Role::Button, "Button", Some("Like"), (0, 34, 120, 80)),
            "like",
        ),
        identified(
            app_node(4, Role::Label, "Label", Some("Likes: 0"), (0, 114, 200, 36)),
            "likes",
        ),
        subscribe,
        comment,
        identified(
            app_node(7, Role::Label, "Label", Some("Pad: 0"), (0, 290, 480, 240)),
            "pad",
        ),
    ];
    Tree::new(vec![root], false)
}

fn dump(xml: &str) -> UiDump {
    UiDump::parse(xml).expect("the fixture is a dump")
}

/// The node `identifier` names in `nodes`.
fn find<'a>(nodes: &'a [Node], identifier: &str) -> Option<&'a Node> {
    nodes.iter().find_map(|node| {
        if node.identifier.as_deref() == Some(identifier) {
            Some(node)
        } else {
            find(&node.children, identifier)
        }
    })
}

/// `text` as an XML attribute value.
fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// A dump of `app` as `accesskit_android` serves it under the app's view and UI Automator writes
/// it: the view's frames of `package` above it, the surface's top-left at `origin` on a `display`
/// of that size. The app's root is the view's own node. The scenario apps publish no numeric
/// value, which the adapter would not set as text.
pub(crate) fn served(app: &Tree, package: &str, origin: (i32, i32), display: (i32, i32)) -> String {
    fn write_node(
        out: &mut String,
        node: &Node,
        package: &str,
        origin: (i32, i32),
        display: (i32, i32),
    ) {
        let toggled = node.checked.is_some();
        let bounds = node.surface_rect.map_or((0, 0, 0, 0), |rect| {
            let left = rect.x + origin.0;
            let top = rect.y + origin.1;
            let right = left + i32::try_from(rect.width).expect("a fixture's width fits");
            let bottom = top + i32::try_from(rect.height).expect("a fixture's height fits");
            // `getVisibleBoundsInScreen`: intersected with the display when they meet.
            let clipped = (
                left.max(0),
                top.max(0),
                right.min(display.0),
                bottom.min(display.1),
            );
            if clipped.0 < clipped.2 && clipped.1 < clipped.3 {
                clipped
            } else {
                (left, top, right, bottom)
            }
        });
        let _ = write!(
            out,
            "<node index=\"0\" text=\"{}\" resource-id=\"\" class=\"{}\" package=\"{package}\" \
             content-desc=\"{}\" checkable=\"{toggled}\" checked=\"{}\" clickable=\"false\" \
             enabled=\"{}\" focusable=\"{}\" focused=\"{}\" scrollable=\"false\" \
             long-clickable=\"false\" password=\"false\" selected=\"{}\" \
             bounds=\"[{},{}][{},{}]\">",
            escaped(node.value.as_deref().unwrap_or_default()),
            roles::class_for(&node.native_role, toggled),
            escaped(node.name.as_deref().unwrap_or_default()),
            matches!(node.checked, Some(Checked::True | Checked::Mixed)),
            !node.disabled,
            node.focusable,
            node.focused,
            node.selected == Some(true),
            bounds.0,
            bounds.1,
            bounds.2,
            bounds.3,
        );
        for child in &node.children {
            write_node(out, child, package, origin, display);
        }
        out.push_str("</node>");
    }
    let root = app.roots.first().and_then(|root| root.surface_rect);
    let (width, height) = root.map_or((0, 0), |rect| {
        (
            i32::try_from(rect.width).expect("a window's width fits"),
            i32::try_from(rect.height).expect("a window's height fits"),
        )
    });
    let frame = |bounds: (i32, i32, i32, i32)| {
        format!(
            "<node index=\"0\" text=\"\" resource-id=\"\" class=\"android.widget.FrameLayout\" \
             package=\"{package}\" content-desc=\"\" checkable=\"false\" checked=\"false\" \
             clickable=\"false\" enabled=\"true\" focusable=\"false\" focused=\"false\" \
             scrollable=\"false\" long-clickable=\"false\" password=\"false\" selected=\"false\" \
             bounds=\"[{},{}][{},{}]\">",
            bounds.0, bounds.1, bounds.2, bounds.3
        )
    };
    let mut out = String::from(
        "<?xml version='1.0' encoding='UTF-8' standalone='yes' ?><hierarchy rotation=\"0\">",
    );
    out.push_str(&frame((0, 0, display.0, display.1)));
    out.push_str(&frame((
        origin.0,
        origin.1,
        origin.0 + width,
        origin.1 + height,
    )));
    for root in &app.roots {
        write_node(&mut out, root, package, origin, display);
    }
    out.push_str("</node></node></hierarchy>");
    out
}

/// Every arm of `accesskit_android` 0.9.1's `NodeWrapper::class_name`, written out from its
/// source: the AccessKit roles of each arm and the class it names.
#[test]
fn each_accesskit_role_takes_the_class_accesskit_android_names() {
    let arms: &[(&[&str], &str)] = &[
        (
            &[
                "TextInput",
                "MultilineTextInput",
                "SearchInput",
                "EmailInput",
                "NumberInput",
                "PasswordInput",
                "PhoneNumberInput",
                "UrlInput",
            ],
            "android.widget.EditText",
        ),
        (&["Slider"], "android.widget.SeekBar"),
        (
            &[
                "ColorWell",
                "ComboBox",
                "EditableComboBox",
                "DateInput",
                "DateTimeInput",
                "WeekInput",
                "MonthInput",
                "TimeInput",
            ],
            "android.widget.Spinner",
        ),
        (
            &["Button", "PdfActionableHighlight"],
            "android.widget.Button",
        ),
        (&["CheckBox"], "android.widget.CheckBox"),
        (&["RadioButton"], "android.widget.RadioButton"),
        (&["RadioGroup"], "android.widget.RadioGroup"),
        (&["Switch"], "android.widget.ToggleButton"),
        (&["Canvas", "Image", "SvgRoot"], "android.widget.ImageView"),
        (
            &["Meter", "ProgressIndicator"],
            "android.widget.ProgressBar",
        ),
        (&["TabList"], "android.widget.TabWidget"),
        (&["Grid", "Table", "TreeGrid"], "android.widget.GridView"),
        (
            &["DescriptionList", "List", "ListBox", "ScrollView"],
            "android.widget.ListView",
        ),
        (&["Dialog"], "android.app.Dialog"),
        (&["RootWebArea"], "android.webkit.WebView"),
        (
            &["MenuItem", "MenuItemCheckBox", "MenuItemRadio"],
            "android.view.MenuItem",
        ),
        (&["Label"], "android.widget.TextView"),
        // The `_` arm, among them roles a screen reader names: a link, a heading, a tab.
        (
            &[
                "Window",
                "GenericContainer",
                "Link",
                "Heading",
                "Tab",
                "AlertDialog",
                "Paragraph",
                "Unknown",
                "a role a newer AccessKit adds",
            ],
            "android.view.View",
        ),
    ];
    for (accesskit_roles, class) in arms {
        for accesskit_role in *accesskit_roles {
            assert_eq!(
                roles::class_for(accesskit_role, false),
                *class,
                "{accesskit_role}"
            );
        }
    }
    // A button with a toggled state is the adapter's one choice that reads the node's state.
    assert_eq!(
        roles::class_for("Button", true),
        "android.widget.ToggleButton"
    );
    for not_a_button in ["CheckBox", "Switch", "Label", "Link"] {
        assert_eq!(
            roles::class_for(not_a_button, true),
            roles::class_for(not_a_button, false),
            "{not_a_button}"
        );
    }
}

/// The table was transcribed from the adapter the workspace locks: a bump fails here until the
/// table is read against the new one.
#[test]
fn the_class_table_was_read_against_the_locked_adapter() {
    let lock = include_str!("../../../Cargo.lock");
    let locked = lock
        .split("[[package]]")
        .find(|entry| entry.contains("name = \"accesskit_android\""))
        .and_then(|entry| {
            entry
                .lines()
                .find_map(|line| line.strip_prefix("version = \""))
                .map(|version| version.trim_end_matches('"'))
        });
    assert_eq!(locked, Some(roles::ADAPTER_CHECKED_AGAINST));
}

/// The table rows every role of the Windows lane's table in its order, and each row's class is
/// what the adapter gives the AccessKit role it names.
#[test]
fn the_role_table_composes_each_role_with_the_class_of_its_accesskit_role() {
    let android: Vec<SemanticsRole> = ROLES.iter().map(|row| row.role).collect();
    let uia: Vec<SemanticsRole> = uia_roles::ROLES.iter().map(|row| row.role).collect();
    assert_eq!(android, uia);
    for row in ROLES {
        assert_eq!(row.class, roles::class_for(row.accesskit, false), "{row:?}");
        assert_eq!(roles::role_row(row.role), Some(*row));
    }
    assert_eq!(roles::role_row(SemanticsRole::Generic), None);
}

/// The roles whose label FLUI publishes as the description, since ARIA forbids them a name:
/// `accesskit_android` hands Android no description, so TalkBack reads their nodes empty and the
/// roles scenario cannot find them by their label.
#[test]
fn talkback_reads_no_label_of_these_roles() {
    let unnamed: Vec<SemanticsRole> = ROLES
        .iter()
        .filter(|row| !row.named)
        .map(|row| row.role)
        .collect();
    assert_eq!(
        unnamed,
        vec![
            SemanticsRole::Paragraph,
            SemanticsRole::Caption,
            SemanticsRole::Code,
            SemanticsRole::Emphasis,
            SemanticsRole::Strong,
            SemanticsRole::Insertion,
            SemanticsRole::Deletion,
        ]
    );
    assert!(
        ROLES
            .iter()
            .filter(|row| !row.named)
            .all(|row| row.class == VIEW)
    );
}

#[test]
fn a_class_folds_to_the_role_a_screen_reader_tells_from_it() {
    for (class, role) in [
        ("android.widget.Button", Role::Button),
        ("android.widget.ToggleButton", Role::Button),
        ("android.widget.EditText", Role::TextInput),
        ("android.widget.TextView", Role::Label),
        ("android.widget.CheckBox", Role::CheckBox),
        ("android.widget.ListView", Role::List),
        ("android.view.View", Role::Group),
        ("android.widget.FrameLayout", Role::Unknown),
        ("", Role::Unknown),
    ] {
        assert_eq!(roles::wire_role(class), role, "{class}");
    }
}

/// The scenario app's window, read from the dump the adapter serves: each node found by its
/// role, name, state and bounds (the button's right and bottom edges a pixel short, as the
/// adapter truncates them), carrying the app's identifier, and the projection equal to the app's.
#[test]
fn a_dump_is_matched_to_the_apps_tree_and_projects_to_what_it_published() {
    let app = controls();
    let os = tree::read(&dump(CONTROLS), PACKAGE, &app, MAPPING).expect("the dump matches");
    let like = find(&os.roots, "like").expect("the button is matched");
    assert_eq!(like.role, Role::Button);
    assert_eq!(like.native_role, "android.widget.Button");
    assert_eq!(like.name.as_deref(), Some("Like"));
    assert_eq!(
        like.rect,
        Some(Rect {
            x: 0,
            y: 94,
            width: 119,
            height: 79
        }),
        "the rectangle is the dump's, on the display"
    );
    let subscribe = find(&os.roots, "subscribe").expect("the switch is matched");
    assert_eq!(
        (subscribe.role, subscribe.checked),
        (Role::Switch, Some(Checked::False))
    );
    assert_eq!(
        find(&os.roots, "comment").and_then(|node| node.value.as_deref()),
        Some(""),
        "an empty text reads as the app's empty value"
    );
    assert_eq!(os.roots[0].window, app.roots[0].window);
    let projection = Projection::of(&app, &os);
    assert!(projection.is_equal(), "{:?}", projection.differences());
    assert!(!os.truncated);
}

/// A node that is not where the app put it, or not what it is, is no match: it projects with the
/// coarse role of its class and no identifier, and the projection shows the difference.
#[test]
fn a_node_off_by_more_than_the_tolerance_or_of_another_class_is_not_matched() {
    let app = controls();
    // Two pixels off on an edge is the adapter's rounding; three is another place.
    let near = CONTROLS.replace("bounds=\"[0,94][119,173]\"", "bounds=\"[0,94][122,173]\"");
    let os = tree::read(&dump(&near), PACKAGE, &app, MAPPING).expect("the dump matches");
    assert!(find(&os.roots, "like").is_some());
    let far = CONTROLS.replace("bounds=\"[0,94][119,173]\"", "bounds=\"[0,94][123,173]\"");
    let os = tree::read(&dump(&far), PACKAGE, &app, MAPPING).expect("the rest matches");
    assert_eq!(find(&os.roots, "like"), None);
    assert!(!Projection::of(&app, &os).is_equal());
    // A switch served as a plain button, a name changed, a state flipped: no match either.
    for changed in [
        CONTROLS.replace("android.widget.ToggleButton", "android.widget.Button"),
        CONTROLS.replace("content-desc=\"Subscribe\"", "content-desc=\"Subscribed\""),
        CONTROLS.replace(
            "checkable=\"true\" checked=\"false\"",
            "checkable=\"true\" checked=\"true\"",
        ),
    ] {
        let os = tree::read(&dump(&changed), PACKAGE, &app, MAPPING).expect("the rest matches");
        let unmatched = os.roots[0]
            .children
            .iter()
            .find(|node| node.native_role.contains("Button") && node.identifier.is_none());
        assert!(
            unmatched.is_some_and(|node| node.role == Role::Button),
            "{changed}"
        );
        assert_eq!(find(&os.roots, "subscribe"), None);
    }
}

/// A child is matched only among the children of the node its parent matched: the same node one
/// level down is another node of the tree.
#[test]
fn a_child_is_matched_among_its_matched_parents_children_only() {
    let app = controls();
    // The button moved inside the pad's node: same class, name and bounds, another place in the
    // tree.
    let button = "<node index=\"1\" text=\"\" resource-id=\"\" class=\"android.widget.Button\" package=\"dev.flui.scenario\" content-desc=\"Like\" checkable=\"false\" checked=\"false\" clickable=\"false\" enabled=\"true\" focusable=\"true\" focused=\"false\" scrollable=\"false\" long-clickable=\"false\" password=\"false\" selected=\"false\" bounds=\"[0,94][119,173]\" />";
    assert!(CONTROLS.contains(button));
    let moved = CONTROLS.replace(button, "").replace(
        "bounds=\"[0,350][480,590]\" />",
        &format!("bounds=\"[0,350][480,590]\">{button}</node>"),
    );
    let os = tree::read(&dump(&moved), PACKAGE, &app, MAPPING).expect("the rest matches");
    assert_eq!(find(&os.roots, "like"), None);
    let pad = find(&os.roots, "pad").expect("the pad is matched");
    assert_eq!(
        pad.children.len(),
        1,
        "the moved button is projected where it is"
    );
    // A node of another package inside the app's view is not the app's.
    let foreign = CONTROLS.replace(
        "package=\"dev.flui.scenario\" content-desc=\"Gallery\"",
        "package=\"com.other\" content-desc=\"Gallery\"",
    );
    let os = tree::read(&dump(&foreign), PACKAGE, &app, MAPPING).expect("the rest matches");
    assert_eq!(os.roots[0].children.len(), 5);
}

/// Two candidates of the same role, name, state and bounds are refused, never guessed between:
/// two nodes of the dump for one app node, two app nodes for one node of the dump, two roots.
#[test]
fn two_candidates_alike_are_an_ambiguity_never_a_guess() {
    let app = controls();
    let button = "<node index=\"1\" text=\"\" resource-id=\"\" class=\"android.widget.Button\" package=\"dev.flui.scenario\" content-desc=\"Like\" checkable=\"false\" checked=\"false\" clickable=\"false\" enabled=\"true\" focusable=\"true\" focused=\"false\" scrollable=\"false\" long-clickable=\"false\" password=\"false\" selected=\"false\" bounds=\"[0,94][119,173]\" />";
    let twice = CONTROLS.replace(button, &format!("{button}{button}"));
    match tree::read(&dump(&twice), PACKAGE, &app, MAPPING) {
        Err(TreeError::Ambiguous { node, candidates }) => {
            assert!(node.contains("\"like\""), "{node}");
            assert_eq!(candidates.len(), 2, "{candidates:?}");
        }
        other => panic!("two nodes for one: {other:?}"),
    }
    let mut doubled = controls();
    let mut twin = doubled.roots[0].children[1].clone();
    twin.id = id(8);
    twin.identifier = Some("like-twin".to_owned());
    doubled.roots[0].children.push(twin);
    match tree::read(&dump(CONTROLS), PACKAGE, &doubled, MAPPING) {
        Err(TreeError::Ambiguous { candidates, .. }) => assert_eq!(candidates.len(), 2),
        other => panic!("two app nodes for one node: {other:?}"),
    }
    // Two windows of the app side by side that both fit its root.
    let view = "<node index=\"0\" text=\"\" resource-id=\"\" class=\"android.view.View\" package=\"dev.flui.scenario\" content-desc=\"\" checkable=\"false\" checked=\"false\" clickable=\"false\" enabled=\"true\" focusable=\"true\" focused=\"true\" scrollable=\"false\" long-clickable=\"false\" password=\"false\" selected=\"false\" bounds=\"[0,60][1080,2400]\" />";
    let two_roots = CONTROLS.replace(
        "</node></node></hierarchy>",
        &format!("</node>{view}</node></hierarchy>"),
    );
    let refused = tree::read(&dump(&two_roots), PACKAGE, &app, MAPPING)
        .expect_err("two roots fit")
        .into_error();
    assert!(
        matches!(&refused, Error::Ambiguous { candidates, .. } if candidates.len() == 2),
        "{refused:?}"
    );
}

/// Bounds the device sent off the display place a node nowhere: it keeps no rectangle and fits
/// only an app node with nothing on the display either; a root off the display is no window.
#[test]
fn bounds_off_the_display_place_a_node_nowhere() {
    let app = controls();
    for off in [
        "[0,2400][120,2480]",
        "[-200,94][-80,173]",
        "[0,94][1200,173]",
        "[119,173][0,94]",
        "[0,94][0,94]",
    ] {
        let lying = CONTROLS.replace("[0,94][119,173]", off);
        let os = tree::read(&dump(&lying), PACKAGE, &app, MAPPING).expect("the rest matches");
        let button = os.roots[0]
            .children
            .iter()
            .find(|node| node.name.as_deref() == Some("Like"))
            .expect("the button is projected");
        assert_eq!(
            (button.rect, button.identifier.as_deref()),
            (None, None),
            "{off}"
        );
    }
    // An app node scrolled out of view, and its node of the dump with bounds past the display:
    // matched on role, name and state.
    let mut scrolled = controls();
    scrolled.roots[0].children[1].surface_rect = Some(Rect {
        x: 0,
        y: 2400,
        width: 120,
        height: 80,
    });
    let below = CONTROLS.replace("[0,94][119,173]", "[0,2460][120,2540]");
    let os = tree::read(&dump(&below), PACKAGE, &scrolled, MAPPING).expect("the dump matches");
    assert_eq!(
        find(&os.roots, "like").map(|node| node.rect),
        Some(None),
        "matched, with no rectangle"
    );
    let off_root = CONTROLS.replace("bounds=\"[0,60][1080,2400]\"><node index=\"0\" text=\"\" resource-id=\"\" class=\"android.widget.TextView\"", "bounds=\"[0,60][1080,2401]\"><node index=\"0\" text=\"\" resource-id=\"\" class=\"android.widget.TextView\"");
    assert_ne!(off_root, CONTROLS);
    assert_eq!(
        tree::read(&dump(&off_root), PACKAGE, &app, MAPPING),
        Err(TreeError::NoWindow {
            package: PACKAGE.to_owned()
        })
    );
    let refused = TreeError::NoWindow {
        package: PACKAGE.to_owned(),
    }
    .into_error();
    assert!(matches!(refused, Error::NotFound { .. }), "{refused:?}");
}

#[test]
fn a_screen_reader_is_told_by_its_service_or_by_touch_exploration() {
    let talkback =
        "com.google.android.marvin.talkback/com.google.android.marvin.talkback.TalkBackService";
    let samsung = "com.samsung.android.accessibility.talkback/com.samsung.android.marvin.talkback.TalkBackService";
    let many_then_talkback = format!(
        "{}:{talkback}\n0\n",
        (0..200)
            .map(|index| format!("com.example{index}/.Service"))
            .collect::<Vec<_>>()
            .join(":")
    );
    for (printed, reader) in [
        ("null\n0\n", None),
        ("null\nnull\n", None),
        ("com.example.passwords/.Autofill\n0\n", None),
        (
            &format!("{talkback}\n1\n") as &str,
            Some(ScreenReader::Service(talkback.to_owned())),
        ),
        (
            &format!("com.example/.A:{samsung}\nnull\n"),
            Some(ScreenReader::Service(samsung.to_owned())),
        ),
        (
            "com.example.reader/.Reader\n1\n",
            Some(ScreenReader::TouchExploration),
        ),
        (
            many_then_talkback.as_str(),
            Some(ScreenReader::Service(talkback.to_owned())),
        ),
        // A package that only starts like TalkBack's is another app.
        ("com.google.android.marvin.talkbackx/.S\n0\n", None),
    ] {
        assert_eq!(
            device::parse_screen_reader(printed),
            Ok(reader.clone()),
            "{printed:?}"
        );
        assert_eq!(
            safety::check_dump(printed).is_err(),
            reader.is_some(),
            "{printed:?}"
        );
    }
    // A reply of another shape cannot say no screen reader runs, so the dump is refused: the first
    // command failed and printed nothing (the shell reports only the second's status), a line
    // came before the values, or a value is not one `settings` prints.
    for printed in [
        "",
        "\n\n",
        "0\n",
        "\n0\n",
        "WARNING: linker: unused DT entry\nnull\n1\n",
        "null\nyes\n",
    ] {
        assert!(device::parse_screen_reader(printed).is_err(), "{printed:?}");
        let refused = safety::check_dump(printed)
            .expect_err("an unread reply refuses the dump")
            .into_error();
        assert_eq!(
            refused.missing_capability().as_deref(),
            Some(safety::WINDOW_DUMP),
            "{printed:?}: {refused:?}"
        );
    }
    let refused = Refusal::ScreenReaderOn {
        reader: ScreenReader::TouchExploration,
    }
    .into_error();
    assert!(refused.is_cannot_verify(), "{refused:?}");
    assert_eq!(
        refused.missing_capability().as_deref(),
        Some(safety::WINDOW_DUMP)
    );
}

/// The lane reads the tree from one dump, asking first whether a screen reader runs, and says it
/// reads through UI Automator.
#[test]
fn the_lane_reads_the_tree_from_a_dump_once_no_screen_reader_runs() {
    let adb = scripted(true, false).reply_once("exec-out uiautomator", AdbReply::new(0, CONTROLS));
    let mut lane = AndroidLane::connect(adb.device(), PACKAGE).expect("the lane connects");
    assert_eq!(
        lane.capabilities().accessibility,
        Some(AccessibilityApi::UiAutomator)
    );
    let os = lane
        .read_tree(&placement(), &controls())
        .expect("the lane reads the tree");
    assert!(find(&os.roots, "like").is_some(), "{os:?}");
    let log = adb.log();
    let settings = log
        .iter()
        .position(|line| line.starts_with("shell settings get secure"))
        .expect("the settings were read");
    let dumped = log
        .iter()
        .position(|line| line.starts_with("exec-out uiautomator dump"))
        .expect("the window was dumped");
    assert!(settings < dumped, "{log:?}");
    assert_eq!(adb.commands("exec-out uiautomator dump").len(), 1);
}

/// With TalkBack on, the lane neither reads the tree nor locates the window: no dump goes out.
#[test]
fn a_running_screen_reader_stops_every_dump_before_it_starts() {
    let talkback = "com.google.android.marvin.talkback/.TalkBackService\n1\n";
    let adb = scripted(true, false)
        .reply_once("shell settings get secure", AdbReply::new(0, talkback))
        .reply_once("shell settings get secure", AdbReply::new(0, talkback));
    let mut lane = AndroidLane::connect(adb.device(), PACKAGE).expect("the lane connects");
    let read = lane
        .read_tree(&placement(), &controls())
        .expect_err("no tree while TalkBack reads");
    let located = lane
        .locate_window(&placement())
        .expect_err("no window while TalkBack reads");
    for refused in [read, located] {
        assert_eq!(
            refused.missing_capability().as_deref(),
            Some(safety::WINDOW_DUMP),
            "{refused:?}"
        );
    }
    assert_eq!(adb.commands("exec-out uiautomator"), Vec::<String>::new());
}

/// An activation through the OS accessibility API is refused with its reason, before the device
/// is asked anything.
#[test]
fn an_activation_through_the_accessibility_api_is_refused_naming_os_invoke() {
    let adb = scripted(true, false);
    let mut lane = AndroidLane::connect(adb.device(), PACKAGE).expect("the lane connects");
    let before = adb.log().len();
    let refused = lane
        .invoke(&placement(), "like")
        .expect_err("the lane activates nothing");
    assert_eq!(refused.missing_capability().as_deref(), Some("os_invoke"));
    assert!(refused.to_string().contains("ACTION_CLICK"), "{refused}");
    assert_eq!(adb.log().len(), before, "{:?}", adb.log());
    assert_eq!(android::LANE, lane.name());
}

/// [`served`] writes the hand-written fixture's window from the same tree: the generator the
/// headless scenarios read through is the adapter's output, not another shape.
#[test]
fn a_served_dump_reads_as_the_hand_written_one() {
    let app = controls();
    let written = tree::read(&dump(CONTROLS), PACKAGE, &app, MAPPING).expect("the fixture matches");
    let generated = served(&app, PACKAGE, (0, 60), (1080, 2400));
    let read =
        tree::read(&dump(&generated), PACKAGE, &app, MAPPING).expect("the served dump matches");
    let outline = |tree: &Tree| flui_driver::protocol::tree::normalized_outline(&tree.roots);
    assert_eq!(outline(&read), outline(&written));
}

/// A dump deeper or wider than the walk budgets of every lane projects cut, marked where it was
/// cut: the matched window still reads, its button still found.
#[test]
fn a_dump_past_the_walk_budgets_projects_cut_and_says_so() {
    let node = |close: bool| {
        format!(
            "<node index=\"0\" text=\"\" resource-id=\"\" class=\"android.view.View\" \
             package=\"{PACKAGE}\" content-desc=\"\" checkable=\"false\" checked=\"false\" \
             clickable=\"false\" enabled=\"true\" focusable=\"false\" focused=\"false\" \
             scrollable=\"false\" long-clickable=\"false\" password=\"false\" selected=\"false\" \
             bounds=\"[0,600][10,610]\"{}>",
            if close { " /" } else { "" }
        )
    };
    let after_like = "bounds=\"[0,94][119,173]\" />";
    let deep = format!(
        "{}{}",
        node(false).repeat(flui_driver::lane::walk::MAX_DEPTH + 8),
        "</node>".repeat(flui_driver::lane::walk::MAX_DEPTH + 8)
    );
    let wide = format!(
        "{}{}</node>",
        node(false),
        node(true).repeat(flui_driver::lane::walk::MAX_NODES + 1)
    );
    for (shape, inserted) in [("deep", deep), ("wide", wide)] {
        let xml = CONTROLS.replacen(after_like, &format!("{after_like}{inserted}"), 1);
        let os = tree::read(&dump(&xml), PACKAGE, &controls(), MAPPING).expect("the dump matches");
        assert!(os.truncated, "{shape}: the projection says it was cut");
        assert!(
            find(&os.roots, "like").is_some(),
            "{shape}: the window still reads"
        );
        let mut nodes = Vec::new();
        let mut stack: Vec<&Node> = os.roots.iter().collect();
        while let Some(node) = stack.pop() {
            nodes.push(node);
            stack.extend(node.children.iter());
        }
        assert!(
            nodes.iter().any(|node| node.children_unread),
            "{shape}: the node it was cut at says its children went unread"
        );
        assert!(
            nodes.len() <= flui_driver::lane::walk::MAX_NODES + 1,
            "{shape}: {} nodes past the budget",
            nodes.len()
        );
    }
}
