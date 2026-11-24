//! The Android lane's read of the OS accessibility tree on every host: the role table against
//! `accesskit_android`'s class choices, a dump matched to the app's tree by structure, order,
//! role, name, state and bounds, its refusals (two candidates the order does not tell apart,
//! bounds off the display, an accessibility service running), what a dump cannot show, and the
//! lane reading and refusing through a scripted device.
//!
//! Every dump here is literal XML, written by hand from what `accesskit_android` 0.9.1
//! (`NodeWrapper::populate_node_info`, `NodeWrapper::class_name`) hands Android and what AOSP's
//! `AccessibilityNodeInfoDumper` writes, never generated from the app's tree or the lane's tables:
//! the class from the node's AccessKit role, the label (a static text's value) as the content
//! description, the value as the text, `checkable` and `checked` from any toggled state, `enabled`,
//! the bounding box placed at the app's view on the display and clipped to it (left as it is when
//! it misses the display), and every node visible. The app's view is the decor view's first child
//! over the whole window (`FluiView.install`), beside the content frame that holds NativeActivity's
//! own content view, a plain view of the same bounds.

use flui_driver::Error;
use flui_driver::coordinates::DevicePoint;
use flui_driver::lane::android::adb::AdbReply;
use flui_driver::lane::android::device::{self, EnabledServices};
use flui_driver::lane::android::roles::{self, ROLES, VIEW};
use flui_driver::lane::android::safety::{self, Refusal};
use flui_driver::lane::android::tree::{self, Mapping, TreeError};
use flui_driver::lane::android::uiautomator::UiDump;
use flui_driver::lane::android::{self as android, AndroidLane};
use flui_driver::lane::windows::roles as uia_roles;
use flui_driver::lane::{AccessibilityApi, Lane as _};
use flui_driver::projection::{Projection, ProjectionDifference};
use flui_driver::protocol::geometry::LogicalSize;
use flui_driver::protocol::semantics::SemanticsRole;
use flui_driver::protocol::session::Placement;
use flui_driver::protocol::tree::{ElementId, Node, Rect, Tree, WindowId};
use flui_driver::protocol::wire::{Checked, Role};

use flui_driver::testing::ScriptedAdb;

use crate::android_lane::{PACKAGE, scripted, scripted_showing, windows_edge_to_edge};

/// The scenario app's window as `accesskit_android` serves it, edge to edge on a 1080 by 2400
/// display.
const CONTROLS: &str = include_str!("android/accessibility.xml");

/// A window of radio buttons, a slider, a list with rows past the display and the announcers.
const EDGE_CASES: &str = include_str!("android/edge_cases.xml");

/// Where the fixtures' surface sits: the whole display, an edge-to-edge window.
const MAPPING: Mapping = Mapping {
    origin: DevicePoint::new(0, 0),
    display: (1080, 2400),
};

/// The fixtures' window: the whole 1080 by 2400 display, at twice the logical pixels.
fn placement() -> Placement {
    let window: WindowId = "w1".parse().expect("a window handle");
    Placement::new(window, LogicalSize::new(540.0, 1200.0), 2.0)
}

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

/// The app's root over the whole window of the fixtures.
fn window_root() -> Node {
    let mut root = app_node(1, Role::Window, "Window", None, (0, 0, 1080, 2400));
    root.window = Some("w1".parse::<WindowId>().expect("a window handle"));
    root
}

/// The scenario app's tree as its read publishes it, the window [`CONTROLS`] shows: its content
/// below the 60-pixel status bar it draws under.
fn controls() -> Tree {
    let mut root = window_root();
    let mut subscribe = identified(
        app_node(
            5,
            Role::Switch,
            "Switch",
            Some("Subscribe"),
            (0, 210, 320, 80),
        ),
        "subscribe",
    );
    subscribe.checked = Some(Checked::False);
    let mut comment = identified(
        app_node(6, Role::TextInput, "TextInput", None, (0, 290, 400, 60)),
        "comment",
    );
    comment.value = Some(String::new());
    root.children = vec![
        app_node(2, Role::Label, "Label", Some("Gallery"), (0, 60, 140, 34)),
        identified(
            app_node(3, Role::Button, "Button", Some("Like"), (0, 94, 120, 80)),
            "like",
        ),
        identified(
            app_node(4, Role::Label, "Label", Some("Likes: 0"), (0, 174, 200, 36)),
            "likes",
        ),
        subscribe,
        comment,
        identified(
            app_node(7, Role::Label, "Label", Some("Pad: 0"), (0, 350, 480, 240)),
            "pad",
        ),
    ];
    Tree::new(vec![root], false)
}

/// The app's tree of the window [`EDGE_CASES`] shows, as its read publishes it: two radio buttons
/// it reads as selected and not (the read follows UI Automation, which offers a radio button's
/// toggled state as a selection), a slider whose value is a number, and a list of four unnamed
/// rows, two on the display and two it keeps past the list's viewport, below the display.
fn edge_cases() -> Tree {
    let mut root = window_root();
    let mut small = identified(
        app_node(
            2,
            Role::RadioButton,
            "RadioButton",
            Some("Small"),
            (0, 100, 540, 100),
        ),
        "small",
    );
    small.selected = Some(true);
    let mut large = identified(
        app_node(
            3,
            Role::RadioButton,
            "RadioButton",
            Some("Large"),
            (540, 100, 540, 100),
        ),
        "large",
    );
    large.selected = Some(false);
    let mut volume = identified(
        app_node(
            4,
            Role::Slider,
            "Slider",
            Some("Volume"),
            (0, 200, 1080, 100),
        ),
        "volume",
    );
    volume.value = Some("50".to_owned());
    let mut items = identified(
        app_node(5, Role::List, "List", Some("Items"), (0, 1400, 1080, 1000)),
        "items",
    );
    items.children = (0..4)
        .map(|row| {
            identified(
                app_node(
                    6 + row,
                    Role::ListItem,
                    "ListItem",
                    None,
                    (
                        0,
                        1400 + 500 * i32::try_from(row).expect("four rows"),
                        1080,
                        500,
                    ),
                ),
                &format!("row-{row}"),
            )
        })
        .collect();
    // A grid cell, which the read always reports as a selection, and a row of a multi-select
    // list that publishes a selection: the toggled state each has, which the adapter publishes as
    // checkable and checked, the read does not report.
    let mut cell = identified(
        app_node(
            10,
            Role::GridCell,
            "GridCell",
            Some("Cell"),
            (0, 300, 540, 100),
        ),
        "cell",
    );
    cell.selected = Some(false);
    let mut apples = identified(
        app_node(
            11,
            Role::ListItem,
            "ListItem",
            Some("Apples"),
            (540, 300, 540, 100),
        ),
        "apples",
    );
    apples.selected = Some(true);
    // The other selection roles the read reports a selection of: a tab, a tree item whose node
    // the adapter publishes with no toggled state (not checkable), a list box option and a menu
    // list option.
    let selection = |number, role, accesskit, name: &str, x, selected| {
        let mut node = identified(
            app_node(number, role, accesskit, Some(name), (x, 400, 270, 100)),
            &name.to_lowercase(),
        );
        node.selected = Some(selected);
        node
    };
    let home = selection(12, Role::Tab, "Tab", "Home", 0, true);
    let folder = selection(13, Role::TreeItem, "TreeItem", "Folder", 270, false);
    let red = selection(14, Role::ListItem, "ListBoxOption", "Red", 540, false);
    let medium = selection(15, Role::ListItem, "MenuListOption", "Medium", 810, true);
    root.children = vec![
        small, large, volume, items, cell, apples, home, folder, red, medium,
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
    // A twin of the button beside it in the app's tree: one node of the dump for two app nodes
    // the order does not tell apart.
    let mut doubled = controls();
    let mut twin = doubled.roots[0].children[1].clone();
    twin.id = id(8);
    twin.identifier = Some("like-twin".to_owned());
    doubled.roots[0].children.insert(2, twin.clone());
    match tree::read(&dump(CONTROLS), PACKAGE, &doubled, MAPPING) {
        Err(TreeError::Ambiguous { candidates, .. }) => assert_eq!(candidates.len(), 2),
        other => panic!("two app nodes for one node: {other:?}"),
    }
    // The same twin after the pad: the adapter adds children in the app's order, so the button
    // of the dump, before the label the app lists after the button, is the first, and the twin
    // the adapter would have added last is not in the dump.
    let mut last = controls();
    last.roots[0].children.push(twin);
    let os = tree::read(&dump(CONTROLS), PACKAGE, &last, MAPPING).expect("the order settles it");
    assert!(find(&os.roots, "like").is_some());
    assert_eq!(find(&os.roots, "like-twin"), None);
    // Two windows of the app side by side that both fit its root.
    let view = "<node index=\"0\" text=\"\" resource-id=\"\" class=\"android.view.View\" package=\"dev.flui.scenario\" content-desc=\"\" checkable=\"false\" checked=\"false\" clickable=\"false\" enabled=\"true\" focusable=\"true\" focused=\"true\" scrollable=\"false\" long-clickable=\"false\" password=\"false\" selected=\"false\" bounds=\"[0,0][1080,2400]\" />";
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
    let off_root = CONTROLS.replace("bounds=\"[0,0][1080,2400]\"><node index=\"0\" text=\"\" resource-id=\"\" class=\"android.widget.TextView\"", "bounds=\"[0,0][1080,2401]\"><node index=\"0\" text=\"\" resource-id=\"\" class=\"android.widget.TextView\"");
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

/// The services a device runs, read from the two lines `settings get` prints: a list never set
/// (`null`) and a list emptied when its last service was turned off (an empty line) are both no
/// service, and the dump goes out.
#[test]
fn a_list_never_set_or_emptied_is_no_service() {
    for printed in ["null\n0\n", "null\nnull\n", "\n0\n", "\nnull\n", " \n 0 \n"] {
        assert_eq!(
            device::parse_enabled_services(printed),
            Ok(EnabledServices::default()),
            "{printed:?}"
        );
        assert_eq!(safety::check_dump(printed), Ok(()), "{printed:?}");
    }
}

/// Every accessibility service a dump would suspend refuses it, named in the refusal: a screen
/// reader, and services that turn on no touch exploration (Voice Access, Samsung's Universal
/// Switch); touch exploration alone refuses it too.
#[test]
fn any_accessibility_service_refuses_the_dump_naming_it() {
    let talkback = "com.google.android.marvin.talkback/.TalkBackService";
    let voice_access = "com.google.android.apps.accessibility.voiceaccess/.JustSpeakService";
    let universal_switch = "com.samsung.accessibility/.universalswitch.UniversalSwitchService";
    for (printed, services, touch_exploration) in [
        (format!("{talkback}\n1\n"), vec![talkback], true),
        (format!("{voice_access}\n0\n"), vec![voice_access], false),
        (
            format!("{universal_switch}:{voice_access}\nnull\n"),
            vec![universal_switch, voice_access],
            false,
        ),
        ("null\n1\n".to_owned(), Vec::new(), true),
    ] {
        let read = device::parse_enabled_services(&printed).expect("the reply reads");
        assert_eq!(read.services, services, "{printed:?}");
        assert_eq!(read.touch_exploration, touch_exploration, "{printed:?}");
        let refused = safety::check_dump(&printed).expect_err("a service refuses the dump");
        assert!(matches!(refused, Refusal::ServicesOn { .. }), "{refused:?}");
        let refused = refused.into_error();
        assert!(refused.is_cannot_verify(), "{refused:?}");
        assert_eq!(
            refused.missing_capability().as_deref(),
            Some(safety::TREE_DUMP)
        );
        for service in &services {
            assert!(refused.to_string().contains(service), "{refused}");
        }
    }
}

/// A reply of another shape cannot say that no service runs, so the dump is refused: the first
/// command failed and printed nothing (the shell reports only the second's status), a line came
/// before the values, or a value is not one `settings` prints.
#[test]
fn a_reply_of_another_shape_refuses_the_dump() {
    for printed in [
        "",
        "\n",
        "0\n",
        "WARNING: linker: unused DT entry\nnull\n1\n",
        "null\nyes\n",
    ] {
        assert!(
            device::parse_enabled_services(printed).is_err(),
            "{printed:?}"
        );
        let refused = safety::check_dump(printed)
            .expect_err("an unread reply refuses the dump")
            .into_error();
        assert_eq!(
            refused.missing_capability().as_deref(),
            Some(safety::TREE_DUMP),
            "{printed:?}: {refused:?}"
        );
    }
}

/// A device of the fixtures' window, edge to edge, whose `uinput` the shell may not open.
fn device_showing(xml: &str) -> ScriptedAdb {
    scripted_showing(false, None, xml, &windows_edge_to_edge(1080, 2400))
        .refuse_stream("shell uinput -")
}

/// An emulator of the fixtures' window, edge to edge.
fn emulator_showing(xml: &str) -> ScriptedAdb {
    scripted_showing(true, None, xml, &windows_edge_to_edge(1080, 2400))
        .refuse_stream("shell uinput -")
}

/// On a device, the lane reads the tree from one dump, asking first which services run, and says
/// it reads through UI Automator; a list the user emptied lets it read.
#[test]
fn the_lane_reads_the_tree_from_a_dump_once_no_service_runs() {
    let adb =
        device_showing(CONTROLS).reply_once("shell settings get secure", AdbReply::new(0, "\n0\n"));
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

/// On a device with TalkBack or Voice Access on, the lane reads no tree, and no dump goes out; it
/// still locates the window, which takes no dump.
#[test]
fn a_running_service_stops_every_dump_before_it_starts() {
    let talkback = "com.google.android.marvin.talkback/.TalkBackService\n1\n";
    let voice_access = "com.google.android.apps.accessibility.voiceaccess/.JustSpeakService\n0\n";
    let adb = device_showing(CONTROLS)
        .reply_once("shell settings get secure", AdbReply::new(0, talkback))
        .reply_once("shell settings get secure", AdbReply::new(0, voice_access));
    let mut lane = AndroidLane::connect(adb.device(), PACKAGE).expect("the lane connects");
    for reader in ["TalkBack", "Voice Access"] {
        let refused = lane
            .read_tree(&placement(), &controls())
            .expect_err("no tree while a service runs");
        assert_eq!(
            refused.missing_capability().as_deref(),
            Some(safety::TREE_DUMP),
            "{reader}: {refused:?}"
        );
    }
    lane.locate_window(&placement())
        .expect("the window is located without a dump");
    assert_eq!(adb.commands("exec-out uiautomator"), Vec::<String>::new());
}

/// An emulator can run TalkBack as a device does, which the dump would suspend: the read of its
/// tree is refused the same way, and with no service on it is read.
#[test]
fn an_emulator_running_a_service_refuses_the_tree_too() {
    let talkback = "com.google.android.marvin.talkback/.TalkBackService\n1\n";
    let adb = emulator_showing(CONTROLS)
        .reply_once("shell settings get secure", AdbReply::new(0, talkback));
    let mut lane = AndroidLane::connect(adb.device(), PACKAGE).expect("the lane connects");
    let refused = lane
        .read_tree(&placement(), &controls())
        .expect_err("no tree while TalkBack runs on the emulator");
    assert_eq!(
        refused.missing_capability().as_deref(),
        Some(safety::TREE_DUMP)
    );
    assert_eq!(adb.commands("exec-out uiautomator"), Vec::<String>::new());
    let os = lane
        .read_tree(&placement(), &controls())
        .expect("with no service on, the emulator's tree is read");
    assert!(find(&os.roots, "like").is_some());
}

/// The window is located before and after the dump: a window that moved while the device dumped
/// it, or a dump of another turn than the window's, is no snapshot the mapping holds for, and the
/// read answers busy rather than match the dump against the wrong place.
#[test]
fn a_window_that_moves_or_turns_during_the_dump_is_busy() {
    let moved = windows_edge_to_edge(1080, 2400)
        .replace("frame=[0,0][1080,2400]", "frame=[40,0][1120,2400]");
    let turned = CONTROLS.replace("rotation=\"0\"", "rotation=\"1\"");
    assert_ne!(turned, CONTROLS);
    for (case, adb) in [
        (
            "moved",
            emulator_showing(CONTROLS)
                .reply_once("shell dumpsys window displays", AdbReply::new(0, moved)),
        ),
        (
            "turned",
            emulator_showing(CONTROLS).reply_once("exec-out uiautomator", AdbReply::new(0, turned)),
        ),
    ] {
        let mut lane = AndroidLane::connect(adb.device(), PACKAGE).expect("the lane connects");
        let refused = lane
            .read_tree(&placement(), &controls())
            .expect_err("the window did not hold still");
        assert!(matches!(refused, Error::Busy { .. }), "{case}: {refused:?}");
        assert_eq!(
            adb.commands("shell dumpsys window displays").len(),
            2,
            "{case}"
        );
    }
}

/// An activation through the OS accessibility API is refused with its reason, before the device
/// is asked anything.
#[test]
fn an_activation_through_the_accessibility_api_is_refused_naming_os_invoke() {
    let adb = scripted(true, false);
    let mut lane = AndroidLane::connect(adb.device(), PACKAGE).expect("the lane connects");
    let before = adb.log().len();
    let refused = lane
        .invoke(&placement(), "like", &controls())
        .expect_err("the lane activates nothing");
    assert_eq!(refused.missing_capability().as_deref(), Some("os_invoke"));
    assert!(refused.to_string().contains("ACTION_CLICK"), "{refused}");
    assert_eq!(adb.log().len(), before, "{:?}", adb.log());
    assert_eq!(android::LANE, lane.name());
}

/// The app's root is the child of the decor view that fits it, where `FluiView.install` puts the
/// app's view: NativeActivity's own content view inside the content frame has the same class,
/// bounds and states in an edge-to-edge window, and is not taken for it.
#[test]
fn the_root_is_the_decor_views_child_not_nativeactivitys_content_view() {
    let app = controls();
    let os = tree::read(&dump(CONTROLS), PACKAGE, &app, MAPPING).expect("one root fits");
    assert_eq!(os.roots.len(), 1);
    assert_eq!(
        os.roots[0].children.len(),
        6,
        "the root read is the app's view, with its six nodes"
    );
    // The app's view gone (its tree not served yet): the content view alone is no window.
    let without_view = without_app_view(CONTROLS);
    assert_eq!(
        tree::read(&dump(&without_view), PACKAGE, &app, MAPPING),
        Err(TreeError::NoWindow {
            package: PACKAGE.to_owned()
        })
    );
}

fn edge_cases_read() -> Tree {
    tree::read(&dump(EDGE_CASES), PACKAGE, &edge_cases(), MAPPING).expect("the dump matches")
}

/// A radio button the app's read reports as selected or not, which the adapter publishes as
/// checkable and checked, is matched, and projects its state as the app's read does.
#[test]
fn a_radio_buttons_selection_is_the_checked_state_the_adapter_publishes() {
    let os = edge_cases_read();
    for (identifier, selected) in [("small", true), ("large", false)] {
        let radio = find(&os.roots, identifier).expect("the radio button is matched");
        assert_eq!(radio.native_role, "android.widget.RadioButton");
        assert_eq!(
            (radio.selected, radio.checked),
            (Some(selected), None),
            "{identifier}"
        );
    }
    // The radio button flipped on Android: no match.
    let flipped = EDGE_CASES.replace(
        "content-desc=\"Small\" checkable=\"true\" checked=\"true\"",
        "content-desc=\"Small\" checkable=\"true\" checked=\"false\"",
    );
    assert_ne!(flipped, EDGE_CASES);
    let os =
        tree::read(&dump(&flipped), PACKAGE, &edge_cases(), MAPPING).expect("the rest matches");
    assert_eq!(find(&os.roots, "small"), None);
}

/// A grid cell, which the app's read always reports as a selection, and a multi-select list row
/// that publishes one, have a toggled state the read does not report and the adapter publishes as
/// checkable and checked: each is matched, and projects its selection as the read does. So does a
/// node of every other selection role, a tab, a list box option, a menu list option, and a tree
/// item whose node the adapter publishes with no toggled state at all.
#[test]
fn a_selection_roles_toggled_state_the_read_does_not_report_is_no_mismatch() {
    let os = edge_cases_read();
    for (identifier, selected) in [
        ("cell", None),
        ("apples", Some(true)),
        ("home", Some(true)),
        ("folder", None),
        ("red", None),
        ("medium", Some(true)),
    ] {
        let node = find(&os.roots, identifier)
            .unwrap_or_else(|| panic!("{identifier} is matched: {os:?}"));
        assert_eq!(
            (node.checked, node.selected),
            (None, selected),
            "{identifier}"
        );
    }
}

/// A list hands Android one row past its viewport, the rest of the rows the app keeps past it to
/// itself: the row the dump shows past the display, unnamed like the row after it, is the one the
/// adapter keeps, and no ambiguity is reported.
#[test]
fn rows_past_the_viewport_are_matched_as_the_adapter_publishes_them() {
    let os = edge_cases_read();
    let items = find(&os.roots, "items").expect("the list is matched");
    let rows: Vec<Option<&str>> = items
        .children
        .iter()
        .map(|row| row.identifier.as_deref())
        .collect();
    assert_eq!(rows, vec![Some("row-0"), Some("row-1"), Some("row-2")]);
    assert_eq!(
        items.children[2].rect, None,
        "the row past the display has no rectangle"
    );
    // Both rows past the viewport in the dump, as a list that does not clip them hands Android:
    // every row is matched, in the order the adapter adds them.
    let row = "<node index=\"2\" text=\"\" resource-id=\"\" class=\"android.view.View\" package=\"dev.flui.scenario\" content-desc=\"\" checkable=\"false\" checked=\"false\" clickable=\"false\" enabled=\"true\" focusable=\"false\" focused=\"false\" scrollable=\"false\" long-clickable=\"false\" password=\"false\" selected=\"false\" bounds=\"[0,2400][1080,2900]\" />";
    assert!(EDGE_CASES.contains(row));
    let both = EDGE_CASES.replace(row, &format!("{row}{row}"));
    let os = tree::read(&dump(&both), PACKAGE, &edge_cases(), MAPPING).expect("the dump matches");
    let items = find(&os.roots, "items").expect("the list is matched");
    let rows: Vec<Option<&str>> = items
        .children
        .iter()
        .map(|row| row.identifier.as_deref())
        .collect();
    assert_eq!(
        rows,
        vec![Some("row-0"), Some("row-1"), Some("row-2"), Some("row-3")]
    );
    // Three unnamed rows past the display where the app keeps two: which two the dump shows
    // nothing tells, and the lane does not guess.
    let three = EDGE_CASES.replace(row, &format!("{row}{row}{row}"));
    assert!(matches!(
        tree::read(&dump(&three), PACKAGE, &edge_cases(), MAPPING),
        Err(TreeError::Ambiguous { .. })
    ));
}

/// What the adapter serves and a dump cannot show is no difference: the slider's range value,
/// which Android holds as `RangeInfo`, projects as the app published it, and the announcers the
/// app's read leaves out are left out of the projection. The projection of the whole window is
/// the app's, but for the rows the list keeps to itself.
#[test]
fn what_a_dump_cannot_show_is_no_difference() {
    let app = edge_cases();
    let os = edge_cases_read();
    assert_eq!(
        find(&os.roots, "volume").and_then(|node| node.value.as_deref()),
        Some("50")
    );
    assert_eq!(
        os.roots[0].children.len(),
        10,
        "the two announcers are left out"
    );
    let differences = Projection::of(&app, &os).differences();
    assert_eq!(
        differences,
        vec![ProjectionDifference::MissingFromOs(
            "  - list_item [identifier=\"row-3\"]".to_owned()
        )],
        "only the row the adapter does not hand Android differs"
    );
    // A text field's empty text is no range: its numeric value is the dump's to show.
    let mut text = edge_cases();
    text.roots[0].children[2].native_role = "TextInput".to_owned();
    text.roots[0].children[2].role = Role::TextInput;
    let reread = EDGE_CASES.replace("android.widget.SeekBar", "android.widget.EditText");
    let os = tree::read(&dump(&reread), PACKAGE, &text, MAPPING).expect("the dump matches");
    assert_eq!(
        find(&os.roots, "volume").and_then(|node| node.value.as_deref()),
        None
    );
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

/// An activation through the accessibility API is refused by the Android lane before the app's
/// tree is read: the lane acts on nothing, so a read that fails never stands in for its refusal.
#[cfg(any(unix, windows))]
#[test]
fn an_activation_is_refused_before_the_apps_tree_is_read() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use flui_driver::Driver;
    use flui_driver::protocol::method::{self, Method as _};
    use flui_driver::testing::{Reply, ScriptedAgent};

    let reads = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&reads);
    let placement = serde_json::to_value(placement()).expect("a placement serializes");
    let tree = serde_json::to_value(controls()).expect("a tree serializes");
    let agent = ScriptedAgent::start(
        &[
            method::WindowsList::NAME,
            method::WindowPlacement::NAME,
            method::SemanticsRead::NAME,
        ],
        Box::new(move |name, _| match name {
            "window.placement" => Reply::Result(placement.clone()),
            "semantics.read" => {
                counted.fetch_add(1, Ordering::SeqCst);
                Reply::Result(tree.clone())
            }
            _ => Reply::Result(serde_json::json!({ "windows": ["w1"] })),
        }),
    )
    .expect("the stand-in binds its endpoint");
    let adb = emulator_showing(CONTROLS);
    let lane = AndroidLane::connect(adb.device(), PACKAGE).expect("the lane connects");
    let mut driver = Driver::attach(&agent.endpoint(), agent.token(), Duration::from_secs(10))
        .expect("the driver attaches")
        .with_lane(lane)
        .expect("the lane starts");
    let refused = driver
        .main_window()
        .and_then(|mut window| window.os_invoke("like"))
        .expect_err("the lane activates nothing");
    assert_eq!(refused.missing_capability().as_deref(), Some("os_invoke"));
    assert_eq!(
        reads.load(Ordering::SeqCst),
        0,
        "the app's tree was not read"
    );
}

/// A node's children are cut to the room the node budget has left before they are matched: a
/// node the walk reaches with the budget spent aligns none of its children, so two of them alike,
/// which would be an ambiguity, cost nothing and fail nothing.
#[test]
fn children_past_the_node_budget_are_cut_before_they_are_matched() {
    let filler = format!(
        "<node index=\"0\" text=\"\" resource-id=\"\" class=\"android.view.View\" \
         package=\"{PACKAGE}\" content-desc=\"\" checkable=\"false\" checked=\"false\" \
         clickable=\"false\" enabled=\"true\" focusable=\"false\" focused=\"false\" \
         scrollable=\"false\" long-clickable=\"false\" password=\"false\" selected=\"false\" \
         bounds=\"[0,600][10,610]\" />"
    );
    let row = format!(
        "<node index=\"0\" text=\"\" resource-id=\"\" class=\"android.widget.TextView\" \
         package=\"{PACKAGE}\" content-desc=\"Row\" checkable=\"false\" checked=\"false\" \
         clickable=\"false\" enabled=\"true\" focusable=\"false\" focused=\"false\" \
         scrollable=\"false\" long-clickable=\"false\" password=\"false\" selected=\"false\" \
         bounds=\"[0,400][100,420]\" />"
    );
    // The root and the label take handles 1 and 2, the label's fillers the next ones, and the
    // button, the count, the switch and the field four more. With MAX_NODES - 7 fillers the pad
    // takes the budget's last handle, so none is left for its two rows; with one filler fewer the
    // budget leaves room for exactly one of them, the budget's last handle.
    let gallery = "content-desc=\"Gallery\" checkable=\"false\" checked=\"false\" clickable=\"false\" enabled=\"true\" focusable=\"false\" focused=\"false\" scrollable=\"false\" long-clickable=\"false\" password=\"false\" selected=\"false\" bounds=\"[0,60][140,94]\" />";
    let pad = "bounds=\"[0,350][480,590]\" />";
    assert!(CONTROLS.contains(gallery) && CONTROLS.contains(pad));
    for (fillers, rows_kept) in [
        (flui_driver::lane::walk::MAX_NODES - 7, 0),
        (flui_driver::lane::walk::MAX_NODES - 8, 1),
    ] {
        let wide = CONTROLS
            .replace(
                gallery,
                &format!(
                    "{}>{}</node>",
                    gallery.trim_end_matches(" />"),
                    filler.repeat(fillers)
                ),
            )
            .replace(
                pad,
                &format!("bounds=\"[0,350][480,590]\">{row}{row}</node>"),
            );
        let mut app = controls();
        let mut pad_row = app_node(8, Role::Label, "Label", Some("Row"), (0, 400, 100, 20));
        pad_row.identifier = Some("row".to_owned());
        app.roots[0].children[5].children = vec![pad_row];
        let os =
            tree::read(&dump(&wide), PACKAGE, &app, MAPPING).expect("the budget cuts the rows");
        assert!(os.truncated);
        let pad = find(&os.roots, "pad").expect("the pad is read");
        assert_eq!(pad.children.len(), rows_kept, "{fillers} fillers: {pad:?}");
        assert!(pad.children_unread, "{fillers} fillers");
    }
}

/// A root wider than the node budget is cut, and the child the cut leaves last stays, though it
/// has the announcers' shape (static text with no area): only the last children of a root read
/// whole are the announcers.
#[test]
fn a_root_cut_by_the_budget_keeps_its_last_kept_child() {
    let filler = format!(
        "<node index=\"0\" text=\"\" resource-id=\"\" class=\"android.view.View\" \
         package=\"{PACKAGE}\" content-desc=\"\" checkable=\"false\" checked=\"false\" \
         clickable=\"false\" enabled=\"true\" focusable=\"false\" focused=\"false\" \
         scrollable=\"false\" long-clickable=\"false\" password=\"false\" selected=\"false\" \
         bounds=\"[0,600][10,610]\" />"
    );
    let label = format!(
        "<node index=\"0\" text=\"\" resource-id=\"\" class=\"android.widget.TextView\" \
         package=\"{PACKAGE}\" content-desc=\"\" checkable=\"false\" checked=\"false\" \
         clickable=\"false\" enabled=\"true\" focusable=\"false\" focused=\"false\" \
         scrollable=\"false\" long-clickable=\"false\" password=\"false\" selected=\"false\" \
         bounds=\"[0,0][0,0]\" />"
    );
    // The root takes handle 1, so the budget leaves room for MAX_NODES - 1 of its children: the
    // six of the app's, then fillers, then the label as the last one kept, then more.
    let room = flui_driver::lane::walk::MAX_NODES - 1;
    let pad = "bounds=\"[0,350][480,590]\" />";
    let wide = CONTROLS.replacen(
        pad,
        &format!(
            "{pad}{}{label}{}",
            filler.repeat(room - 6 - 1),
            filler.repeat(10)
        ),
        1,
    );
    let os = tree::read(&dump(&wide), PACKAGE, &controls(), MAPPING).expect("the dump matches");
    assert!(os.truncated && os.roots[0].children_unread);
    let children = &os.roots[0].children;
    assert_eq!(children.len(), room);
    assert_eq!(
        children.last().map(|node| node.native_role.as_str()),
        Some("android.widget.TextView")
    );
}

/// The app's tree and the OS tree compared are of one revision: a tree that moved while the dump
/// was taken is read again, and one that moves on every read is reported as busy, never taken for
/// a difference of the OS.
#[cfg(any(unix, windows))]
#[test]
fn a_tree_that_moves_while_the_os_tree_is_read_is_read_again() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use flui_driver::Driver;
    use flui_driver::protocol::method::{self, Method as _};
    use flui_driver::testing::{Reply, ScriptedAdb, ScriptedAgent};

    // The revision each read of the tree reports, in turn, the last one ever after.
    let session = |revisions: Vec<u64>| -> (ScriptedAgent, Driver, Arc<AtomicUsize>, ScriptedAdb) {
        let reads = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&reads);
        let placement = serde_json::to_value(placement()).expect("a placement serializes");
        let tree = serde_json::to_value(controls()).expect("a tree serializes");
        let agent = ScriptedAgent::start(
            &[
                method::WindowsList::NAME,
                method::WindowPlacement::NAME,
                method::SemanticsRead::NAME,
            ],
            Box::new(move |name, _| match name {
                "window.placement" => Reply::Result(placement.clone()),
                "semantics.read" => {
                    let read = counted.fetch_add(1, Ordering::SeqCst);
                    let revision = revisions[read.min(revisions.len() - 1)];
                    let mut tree = tree.clone();
                    tree["revision"] = serde_json::json!(revision);
                    Reply::Result(tree)
                }
                _ => Reply::Result(serde_json::json!({ "windows": ["w1"] })),
            }),
        )
        .expect("the stand-in binds its endpoint");
        let adb = emulator_showing(CONTROLS);
        let lane = AndroidLane::connect(adb.device(), PACKAGE).expect("the lane connects");
        let driver = Driver::attach(&agent.endpoint(), agent.token(), Duration::from_secs(10))
            .expect("the driver attaches")
            .with_lane(lane)
            .expect("the lane starts");
        (agent, driver, reads, adb)
    };

    // Revision 1 read, the dump taken, revision 2 read back: read again at 2, and it holds.
    let (_agent, mut driver, reads, adb) = session(vec![1, 2]);
    let projection = driver
        .main_window()
        .and_then(|mut window| window.compare_projection())
        .expect("the second reading holds");
    assert!(projection.is_equal(), "{:?}", projection.differences());
    assert_eq!(
        reads.load(Ordering::SeqCst),
        4,
        "two readings, each read back once"
    );
    assert_eq!(adb.commands("exec-out uiautomator dump").len(), 2);

    // A match the lane could not make while the tree moved (the app's view not served yet) was
    // made against a tree the app no longer showed: read again, and it holds.
    let (_agent, mut driver, _reads, adb) = session(vec![1, 2]);
    let _first = adb.clone().reply_once(
        "exec-out uiautomator",
        AdbReply::new(0, without_app_view(CONTROLS)),
    );
    let os = driver
        .main_window()
        .and_then(|mut window| window.os_tree())
        .expect("the second reading matches");
    assert!(find(&os.roots, "like").is_some());
    assert_eq!(adb.commands("exec-out uiautomator dump").len(), 2);

    // An ambiguity met while the tree moved (two buttons alike in a dump of a tree the app no
    // longer shows) is read again too.
    let button = "<node index=\"1\" text=\"\" resource-id=\"\" class=\"android.widget.Button\" package=\"dev.flui.scenario\" content-desc=\"Like\" checkable=\"false\" checked=\"false\" clickable=\"false\" enabled=\"true\" focusable=\"true\" focused=\"false\" scrollable=\"false\" long-clickable=\"false\" password=\"false\" selected=\"false\" bounds=\"[0,94][119,173]\" />";
    assert!(CONTROLS.contains(button));
    let twice = CONTROLS.replace(button, &format!("{button}{button}"));
    let (_agent, mut driver, _reads, adb) = session(vec![1, 2]);
    let _first = adb
        .clone()
        .reply_once("exec-out uiautomator", AdbReply::new(0, twice));
    let os = driver
        .main_window()
        .and_then(|mut window| window.os_tree())
        .expect("the second reading matches");
    assert!(find(&os.roots, "like").is_some());
    assert_eq!(adb.commands("exec-out uiautomator dump").len(), 2);

    // The same failed match on a tree that held still is the answer.
    let (_agent, mut driver, _reads, adb) = session(vec![1]);
    let _first = adb.clone().reply_once(
        "exec-out uiautomator",
        AdbReply::new(0, without_app_view(CONTROLS)),
    );
    let unmatched = driver
        .main_window()
        .and_then(|mut window| window.os_tree())
        .expect_err("no window in the dump");
    assert!(matches!(unmatched, Error::NotFound { .. }), "{unmatched:?}");
    assert_eq!(adb.commands("exec-out uiautomator dump").len(), 1);

    // A tree that moves on every reading, as one republishing every frame does, is read again
    // for as long as the bound in time allows, however many readings that is, then busy.
    let (_agent, mut driver, _reads, adb) = session((1..=1_000_000).collect());
    let started = std::time::Instant::now();
    let moving = driver
        .main_window()
        .and_then(|mut window| window.os_tree())
        .expect_err("the tree never held still");
    assert!(matches!(moving, Error::Busy { .. }), "{moving:?}");
    // The driver has read again for the whole bound: the same call is no surer to succeed.
    assert_eq!(moving.retry(), flui_driver::protocol::error::Retry::Never);
    assert!(
        adb.commands("exec-out uiautomator dump").len() > 3,
        "read again past any count of three"
    );
    assert!(
        started.elapsed() >= Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );

    // A device whose dump outlasts the bound alone, as a phone's can: the tree that moved during
    // the first reading is read again, and holds.
    let slow = Duration::from_millis(2_100);
    let (_agent, mut driver, _reads, adb) = session(vec![1, 2]);
    let _slow = adb
        .clone()
        .reply_once_after("exec-out uiautomator", AdbReply::new(0, CONTROLS), slow)
        .reply_once_after("exec-out uiautomator", AdbReply::new(0, CONTROLS), slow);
    let os = driver
        .main_window()
        .and_then(|mut window| window.os_tree())
        .expect("the second reading holds though the first outlasted the bound");
    assert!(find(&os.roots, "like").is_some());
    assert_eq!(adb.commands("exec-out uiautomator dump").len(), 2);

    // And a tree that keeps moving on such a device is read at least three times before it is
    // busy, the bound counted from the end of the first reading.
    let (_agent, mut driver, _reads, adb) = session((1..=1_000_000).collect());
    let _slow = (0..3).fold(adb.clone(), |adb, _| {
        adb.reply_once_after("exec-out uiautomator", AdbReply::new(0, CONTROLS), slow)
    });
    let moving = driver
        .main_window()
        .and_then(|mut window| window.os_tree())
        .expect_err("the tree never held still");
    assert!(matches!(moving, Error::Busy { .. }), "{moving:?}");
    assert_eq!(adb.commands("exec-out uiautomator dump").len(), 3);

    // Only the first dump slow: the bound, counted from its end, leaves room for readings past
    // the third.
    let (_agent, mut driver, _reads, adb) = session((1..=1_000_000).collect());
    let _slow =
        adb.clone()
            .reply_once_after("exec-out uiautomator", AdbReply::new(0, CONTROLS), slow);
    let moving = driver
        .main_window()
        .and_then(|mut window| window.os_tree())
        .expect_err("the tree never held still");
    assert!(matches!(moving, Error::Busy { .. }), "{moving:?}");
    assert!(adb.commands("exec-out uiautomator dump").len() > 3);
}

/// `xml` without the app's view, as a dump taken before the app serves its tree shows it.
fn without_app_view(xml: &str) -> String {
    let start = xml
        .find("<node index=\"0\" text=\"\" resource-id=\"\" class=\"android.view.View\"")
        .expect("the app's view");
    let end = xml
        .find("<node index=\"1\" text=\"\" resource-id=\"\" class=\"android.widget.LinearLayout\"")
        .expect("the content root");
    format!("{}{}", &xml[..start], &xml[end..])
}
