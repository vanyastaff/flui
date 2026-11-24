//! The iOS Simulator lane's read of the app's accessibility tree on every host: the role table
//! against `accesskit_ios`'s trait choices and the Simulator bridge's roles, the screen found in
//! the device's window and its points placed on the app's window, the elements matched to the
//! app's tree by role, label, state and bounds, its refusals (two screens, two candidates alike,
//! no window of the device's title, a lane handed no read), and the lane reading and pressing
//! through a scripted Simulator window (`ScriptedSimulatorScreen`).
//!
//! The screens follow what `accesskit_ios` 0.2.2 hands UIKit (`NodeWrapper`, `PlatformNode` and
//! `filter_for_is_accessibility_element`): the label (a static text's value) as
//! `accessibilityLabel`, the value or a toggled state's "1" and "0" as `accessibilityValue`, the
//! description as the hint, the author's identifier as `accessibilityIdentifier`, the traits of
//! the node's role, a node with an action, a value or a toggled state an element whose descendants
//! fold into its label, and a node with children otherwise a container, absent from the elements
//! VoiceOver reads. [`served`] writes one from an app's own tree, as the Simulator would show it.

use flui_driver::Error;
use flui_driver::lane::ios_simulator::accessibility::{
    self, HostAccessibility as _, PointFrame, ScreenRequest,
};
use flui_driver::lane::ios_simulator::device::{self, Udid};
use flui_driver::lane::ios_simulator::roles::{
    self, ADAPTER_CHECKED_AGAINST, ROLES, RoleTrait, SCREEN_SUBROLE,
};
use flui_driver::lane::ios_simulator::tree::{
    self, BOUNDS_TOLERANCE_POINTS, ScreenCandidate, TreeError,
};
use flui_driver::lane::{AccessibilityApi, Lane as _};
use flui_driver::projection::Projection;
use flui_driver::protocol::geometry::LogicalSize;
use flui_driver::protocol::session::Placement;
use flui_driver::protocol::tree::{ElementId, Node, Rect, Tree, WindowId};
use flui_driver::protocol::wire::{Checked, Role};
use flui_driver::testing::ScriptedSimulatorScreen;

use crate::ios_simulator_lane::{connect, scripted};

/// The title of the scripted simulator's window: `iPhone 16` on iOS 18.0.
pub(crate) const TITLE: &str = "iPhone 16 \u{2013} iOS 18.0";

/// An iPhone's window, 393 by 852 points at 3x.
const SIZE: LogicalSize = LogicalSize::new(393.0, 852.0);

fn id(number: u64) -> ElementId {
    ElementId::from_u64(number).expect("test handles are non-zero")
}

fn window() -> WindowId {
    "w1".parse().expect("a window handle")
}

fn placement() -> Placement {
    Placement::new(window(), SIZE, 3.0)
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

/// The scenario app's tree as its read publishes it, on an iPhone's window at 3x.
fn controls() -> Tree {
    let mut root = app_node(1, Role::Window, "Window", None, (0, 0, 1179, 2556));
    root.window = Some(window());
    let mut subscribe = identified(
        app_node(
            5,
            Role::Switch,
            "Switch",
            Some("Subscribe"),
            (0, 150, 480, 120),
        ),
        "subscribe",
    );
    subscribe.checked = Some(Checked::False);
    let mut comment = identified(
        app_node(6, Role::TextInput, "TextInput", None, (0, 270, 600, 90)),
        "comment",
    );
    comment.value = Some(String::new());
    root.children = vec![
        app_node(2, Role::Label, "Label", Some("Gallery"), (0, 0, 210, 51)),
        identified(
            app_node(3, Role::Button, "Button", Some("Like"), (0, 51, 180, 99)),
            "like",
        ),
        identified(
            app_node(4, Role::Label, "Label", Some("Likes: 0"), (0, 105, 300, 45)),
            "likes",
        ),
        subscribe,
        comment,
        identified(
            app_node(7, Role::Label, "Label", Some("Pad: 0"), (0, 360, 720, 360)),
            "pad",
        ),
    ];
    Tree::new(vec![root], false)
}

/// Whether `node` is an element of its own as `accesskit_ios` decides it: a node with an action,
/// a value or a toggled state, or one with no children.
fn is_element(node: &Node) -> bool {
    node.children.is_empty()
        || !node.actions.is_empty()
        || node.checked.is_some()
        || node.value.is_some()
}

/// `node` as the Simulator's bridge shows the element `accesskit_ios` serves for it, numbered
/// from `next`.
fn element(node: &Node, next: &mut u64, with_identifiers: bool) -> Node {
    let toggled = node.checked.is_some();
    let native = roles::native_for(&node.native_role, toggled);
    let (ax_role, subrole) = native
        .split_once('/')
        .map_or((native.as_str(), None), |(role, subrole)| {
            (role, Some(subrole))
        });
    let mut element = Node::new(
        id(*next),
        roles::wire_role(ax_role, subrole),
        native.clone(),
    );
    *next += 1;
    element.name.clone_from(&node.name);
    element.description.clone_from(&node.description);
    if toggled {
        element.checked = Some(match node.checked {
            Some(Checked::False) => Checked::False,
            _ => Checked::True,
        });
    } else {
        // An empty `accessibilityValue` reads as none.
        element.value = node.value.clone().filter(|value| !value.is_empty());
    }
    if with_identifiers {
        element.automation_id.clone_from(&node.identifier);
    }
    element.disabled = node.disabled;
    element.focused = node.focused;
    element.selected = node.selected.filter(|selected| *selected);
    element.rect = node.surface_rect;
    element
}

/// The elements VoiceOver reads under `node`'s children: each child that is an element of its
/// own, and the elements of each that is a container, in their place.
fn elements_under(node: &Node, next: &mut u64, with_identifiers: bool, into: &mut Vec<Node>) {
    for child in &node.children {
        if is_element(child) {
            into.push(element(child, next, with_identifiers));
        } else {
            elements_under(child, next, with_identifiers, into);
        }
    }
}

/// The screen of `app` as the Simulator shows it through the Mac's accessibility API, before the
/// lane matches it: the screen's group at the window's whole extent, the elements VoiceOver reads
/// under it, each placed on the window in device pixels. `with_identifiers` serves the author's
/// identifier as each element's `AXIdentifier`.
pub(crate) fn served(app: &Tree, with_identifiers: bool) -> Tree {
    let root = app.roots.first().expect("the app's tree has its window");
    let mut screen = Node::new(id(1), Role::Group, "AXGroup");
    screen.rect = root.surface_rect;
    let mut next = 2;
    elements_under(root, &mut next, with_identifiers, &mut screen.children);
    Tree::new(vec![screen], false)
}

/// Every arm of `accesskit_ios` 0.2.2's `NodeWrapper::traits` that sets a role's trait, written
/// out from its source: the AccessKit roles of each arm and the trait it sets.
#[test]
fn each_accesskit_role_takes_the_trait_accesskit_ios_sets() {
    assert_eq!(ADAPTER_CHECKED_AGAINST, "0.2.2");
    let arms: &[(&[&str], RoleTrait)] = &[
        (
            &[
                "Button",
                "DefaultButton",
                "DisclosureTriangle",
                "CheckBox",
                "RadioButton",
                "Switch",
                "MenuItemCheckBox",
                "MenuItemRadio",
                "Tab",
            ],
            RoleTrait::Button,
        ),
        (&["Link"], RoleTrait::Link),
        (&["Image"], RoleTrait::Image),
        (&["Label"], RoleTrait::StaticText),
        (&["Heading"], RoleTrait::Header),
        (&["Slider", "SpinButton"], RoleTrait::Adjustable),
        (&["SearchInput"], RoleTrait::SearchField),
        (
            &[
                "TextInput",
                "List",
                "ProgressIndicator",
                "Window",
                "Group",
                "MenuItem",
            ],
            RoleTrait::None,
        ),
    ];
    for (accesskit_roles, expected) in arms {
        for accesskit in *accesskit_roles {
            assert_eq!(roles::role_trait(accesskit), *expected, "{accesskit}");
        }
    }
}

/// Each trait reads as the role the bridge's vocabulary names for it, a toggle as a check box of
/// the switch subrole whatever its other trait, and each folds back to the wire role it stands
/// for.
#[test]
fn each_trait_reads_as_the_bridges_role_and_folds_back() {
    let table: &[(RoleTrait, &str, Role)] = &[
        (RoleTrait::Button, "AXButton", Role::Button),
        (RoleTrait::Link, "AXLink", Role::Link),
        (RoleTrait::Image, "AXImage", Role::Image),
        (RoleTrait::StaticText, "AXStaticText", Role::Label),
        (RoleTrait::Header, "AXHeading", Role::Heading),
        (RoleTrait::Adjustable, "AXSlider", Role::Slider),
        (
            RoleTrait::SearchField,
            "AXTextField/AXSearchField",
            Role::TextInput,
        ),
        (RoleTrait::None, "AXGenericElement", Role::Group),
    ];
    for (role_trait, native, wire) in table {
        let (ax_role, subrole) = roles::ax_role(*role_trait, false);
        assert_eq!(
            roles::native_role(ax_role, subrole),
            *native,
            "{role_trait:?}"
        );
        assert_eq!(roles::wire_role(ax_role, subrole), *wire, "{role_trait:?}");
        assert_eq!(
            roles::ax_role(*role_trait, true),
            ("AXCheckBox", Some(roles::SWITCH_SUBROLE)),
            "{role_trait:?} toggled"
        );
    }
    assert_eq!(roles::native_for("Switch", true), "AXCheckBox/AXSwitch");
    assert_eq!(
        roles::wire_role("AXCheckBox", Some("AXSwitch")),
        Role::Switch
    );
    assert_eq!(roles::wire_role("AXCheckBox", None), Role::CheckBox);
    assert_eq!(roles::wire_role("AXGroup", None), Role::Group);
    assert_eq!(roles::wire_role("AXWindow", None), Role::Unknown);
    assert_eq!(roles::native_role("AXGroup", Some("")), "AXGroup");
}

/// The table holds every role of the vocabulary but the three that publish no node, once each,
/// in the Windows lane's order, and its rows read as the AccessKit role's trait does.
#[test]
fn the_role_table_covers_the_vocabulary_once() {
    let windows: Vec<_> = flui_driver::lane::windows::roles::ROLES
        .iter()
        .map(|row| row.role)
        .collect();
    let ios: Vec<_> = ROLES.iter().map(|row| row.role).collect();
    assert_eq!(ios, windows);
    for row in ROLES {
        assert_eq!(
            row.native(),
            roles::native_for(row.accesskit, false),
            "{:?}",
            row.role
        );
    }
}

#[test]
fn the_window_is_titled_by_the_device_and_its_runtime() {
    let listing = crate::ios_simulator_lane::listing("Booted", true);
    let simulators = device::parse_devices(listing.as_bytes()).expect("a listing");
    let udid = Udid::try_new(crate::ios_simulator_lane::UDID).expect("a UDID");
    let simulator = device::choose(&simulators, &udid).expect("the named simulator");
    assert_eq!(accessibility::window_title(simulator), TITLE);
}

fn candidate(frame: (f64, f64, f64, f64), subrole: Option<&str>, depth: usize) -> ScreenCandidate {
    ScreenCandidate {
        frame: PointFrame::new(frame.0, frame.1, frame.2, frame.3),
        subrole: subrole.map(str::to_owned),
        depth,
    }
}

/// A group is taken for the screen only as Simulator.app or DeviceHub places it, and only with
/// the shape of the app's window: Simulator.app's at its size beside the bezel's groups,
/// DeviceHub's drawn at three quarters and reached along two paths, a turned window's turned; none
/// and two of another frame are refused.
#[test]
fn the_screen_is_the_group_with_the_windows_shape() {
    assert!(tree::is_screen_candidate("AXGroup", None, 1));
    assert!(tree::is_screen_candidate(
        "AXGroup",
        Some(SCREEN_SUBROLE),
        7
    ));
    assert!(!tree::is_screen_candidate("AXGroup", None, 7));
    assert!(!tree::is_screen_candidate("AXButton", None, 1));

    // Simulator.app: the bezel's toolbar and the screen at its size.
    let simulator_app = [
        candidate((231.0, 142.0, 456.0, 52.0), None, 1),
        candidate((258.0, 222.0, 393.0, 852.0), None, 1),
    ];
    assert_eq!(tree::choose_screen(TITLE, &simulator_app, SIZE), Ok(1));
    // DeviceHub: drawn at 0.74, the same group reached twice.
    let device_hub = [
        candidate((657.0, 266.0, 290.82, 630.48), Some(SCREEN_SUBROLE), 6),
        candidate((657.0, 266.0, 290.82, 630.48), Some(SCREEN_SUBROLE), 7),
    ];
    assert_eq!(tree::choose_screen(TITLE, &device_hub, SIZE), Ok(0));
    let turned = [candidate((0.0, 0.0, 852.0, 393.0), None, 1)];
    assert_eq!(
        tree::choose_screen(TITLE, &turned, LogicalSize::new(852.0, 393.0)),
        Ok(0)
    );
    assert!(matches!(
        tree::choose_screen(TITLE, &turned, SIZE),
        Err(TreeError::NoScreen { groups, .. }) if groups.len() == 1
    ));
    let two = [
        candidate((0.0, 0.0, 393.0, 852.0), None, 1),
        candidate((500.0, 0.0, 393.0, 852.0), None, 1),
    ];
    let refused = tree::choose_screen(TITLE, &two, SIZE).expect_err("two screens");
    assert!(matches!(&refused, TreeError::Ambiguous { candidates, .. } if candidates.len() == 2));
    assert!(matches!(refused.into_error(), Error::Ambiguous { .. }));
    for unaddressable in [
        (0.0, 0.0, 0.0, 0.0),
        (f64::NAN, 0.0, 393.0, 852.0),
        (0.0, 0.0, f64::INFINITY, 852.0),
    ] {
        assert_eq!(
            tree::screen_scale(
                PointFrame::new(
                    unaddressable.0,
                    unaddressable.1,
                    unaddressable.2,
                    unaddressable.3
                ),
                SIZE
            ),
            None,
            "{unaddressable:?}"
        );
    }
    assert_eq!(
        tree::screen_scale(
            PointFrame::new(0.0, 0.0, 393.0, 852.0),
            LogicalSize::new(0.0, 0.0)
        ),
        None
    );
}

/// An element's frame in the Mac's points lands on the app's window in its device pixels, from
/// the screen's origin and at the scale the window draws it: at its size and drawn at three
/// quarters alike. A frame with no area, or off any finite place, lands nowhere.
#[test]
fn an_elements_points_land_on_the_windows_pixels() {
    let at_size = PointFrame::new(258.0, 222.0, 393.0, 852.0);
    assert_eq!(
        tree::window_rect(
            PointFrame::new(278.0, 272.0, 60.0, 33.0),
            at_size,
            SIZE,
            3.0
        ),
        Some(Rect {
            x: 60,
            y: 150,
            width: 180,
            height: 99,
        })
    );
    let drawn = PointFrame::new(657.0, 266.0, 393.0 * 0.74, 852.0 * 0.74);
    assert_eq!(
        tree::window_rect(
            PointFrame::new(
                657.0 + 20.0 * 0.74,
                266.0 + 50.0 * 0.74,
                60.0 * 0.74,
                33.0 * 0.74
            ),
            drawn,
            SIZE,
            3.0
        ),
        Some(Rect {
            x: 60,
            y: 150,
            width: 180,
            height: 99,
        })
    );
    for nowhere in [
        PointFrame::new(278.0, 272.0, 0.0, 33.0),
        PointFrame::new(f64::NAN, 272.0, 60.0, 33.0),
        PointFrame::new(1e300, 272.0, 60.0, 33.0),
    ] {
        assert_eq!(
            tree::window_rect(nowhere, at_size, SIZE, 3.0),
            None,
            "{nowhere:?}"
        );
    }
    assert_eq!(
        tree::window_rect(
            PointFrame::new(278.0, 272.0, 60.0, 33.0),
            PointFrame::new(258.0, 222.0, 852.0, 393.0),
            SIZE,
            3.0
        ),
        None,
        "a screen of another shape places nothing"
    );
}

/// The scenario app's screen as the Simulator shows it reads as what the app published: each
/// element matched to its node, carrying its role and identifier, the switch's state from its
/// value and the field's empty value; the projections equal.
#[test]
fn the_screen_projects_to_the_apps_tree() {
    let app = controls();
    for with_identifiers in [true, false] {
        let projected =
            tree::read(&served(&app, with_identifiers), &app, 3.0).expect("the screen matches");
        let like = tree::element_of(&projected.roots, "like").expect("like matched");
        assert_eq!(like.role, Role::Button);
        assert_eq!(like.native_role, "AXButton");
        assert_eq!(like.name.as_deref(), Some("Like"));
        let subscribe = tree::element_of(&projected.roots, "subscribe").expect("matched");
        assert_eq!(subscribe.role, Role::Switch);
        assert_eq!(subscribe.native_role, "AXCheckBox/AXSwitch");
        assert_eq!(subscribe.checked, Some(Checked::False));
        let comment = tree::element_of(&projected.roots, "comment").expect("matched");
        assert_eq!(comment.native_role, "AXGenericElement");
        assert_eq!(comment.value.as_deref(), Some(""));
        assert_eq!(projected.roots[0].window, Some(window()));
        let projection = Projection::of(&app, &projected);
        assert!(
            projection.is_equal(),
            "{with_identifiers}: {:?}",
            projection.differences()
        );
    }
}

/// An element is matched among the descendants of the node its nearest matched ancestor fits:
/// a list the app publishes is a container VoiceOver does not read, so its items are the
/// screen's own elements and still find their nodes; the root read as an unnamed group is lifted.
#[test]
fn the_elements_of_a_container_voiceover_skips_find_their_nodes() {
    let mut list = app_node(2, Role::List, "List", None, (0, 0, 1179, 600));
    list.children = vec![
        identified(
            app_node(
                3,
                Role::ListItem,
                "ListItem",
                Some("First"),
                (0, 0, 1179, 150),
            ),
            "first",
        ),
        identified(
            app_node(
                4,
                Role::ListItem,
                "ListItem",
                Some("Second"),
                (0, 150, 1179, 150),
            ),
            "second",
        ),
    ];
    let mut root = app_node(1, Role::Window, "Window", None, (0, 0, 1179, 2556));
    root.children = vec![list];
    let app = Tree::new(vec![root], false);
    let screen = served(&app, false);
    assert_eq!(screen.roots[0].children.len(), 2, "the list is no element");
    let projected = tree::read(&screen, &app, 3.0).expect("matched");
    for identifier in ["first", "second"] {
        assert_eq!(
            tree::element_of(&projected.roots, identifier).map(|node| node.role),
            Some(Role::ListItem),
            "{identifier}"
        );
    }

    // The app's root as a container: one unnamed group holding the elements.
    let mut wrapped = served(&controls(), false);
    let mut container = Node::new(id(100), Role::Group, "AXGroup");
    container.children = std::mem::take(&mut wrapped.roots[0].children);
    wrapped.roots[0].children = vec![container];
    let projected = tree::read(&wrapped, &controls(), 3.0).expect("matched");
    assert!(Projection::of(&controls(), &projected).is_equal());
}

/// A change made to a served element.
type Change = dyn Fn(&mut Node);

/// An element fits a node only when every fact agrees: another label, role, state, identifier or
/// a frame past the tolerance leaves it unmatched, carrying the coarse role and no identifier; a
/// frame within the tolerance still fits.
#[test]
fn an_element_fits_only_the_node_whose_facts_it_shows() {
    let app = controls();
    // The tolerance at 3x, in device pixels.
    const TOLERANCE: i32 = 5;
    assert_eq!((BOUNDS_TOLERANCE_POINTS * 3.0).ceil(), f64::from(TOLERANCE));
    let changed = |change: &dyn Fn(&mut Node)| {
        let mut screen = served(&app, true);
        let like = screen.roots[0]
            .children
            .iter_mut()
            .find(|node| node.name.as_deref() == Some("Like"))
            .expect("like served");
        change(like);
        tree::read(&screen, &app, 3.0).expect("read")
    };
    let unmatched: &[(&str, &Change)] = &[
        ("label", &|node| node.name = Some("Liked".to_owned())),
        ("role", &|node| {
            node.native_role = "AXLink".to_owned();
            node.role = Role::Link;
        }),
        ("enabled", &|node| node.disabled = true),
        ("identifier", &|node| {
            node.automation_id = Some("other".to_owned())
        }),
        ("state", &|node| node.checked = Some(Checked::True)),
        ("frame", &|node| {
            if let Some(rect) = &mut node.rect {
                rect.x += TOLERANCE + 1;
            }
        }),
        ("no frame", &|node| node.rect = None),
    ];
    for (fact, change) in unmatched {
        let projected = changed(*change);
        assert_eq!(tree::element_of(&projected.roots, "like"), None, "{fact}");
        // The element stays, second as it was served, with no identifier and the coarse role.
        let like = &projected.roots[0].children[1];
        assert_eq!(like.identifier, None, "{fact}");
        assert_eq!(
            like.role,
            roles::wire_role(
                like.native_role.split('/').next().unwrap_or_default(),
                like.native_role.split_once('/').map(|(_, subrole)| subrole)
            ),
            "{fact}"
        );
    }
    let projected = changed(&|node| {
        if let Some(rect) = &mut node.rect {
            rect.x += TOLERANCE;
        }
    });
    assert!(tree::element_of(&projected.roots, "like").is_some());
}

/// Two elements alike for one node, or two nodes alike for one element, are refused: the lane
/// does not guess. An app that published no tree is refused too.
#[test]
fn an_ambiguous_match_is_refused() {
    let app = controls();
    let mut twice = served(&app, false);
    let like = twice.roots[0].children[1].clone();
    twice.roots[0].children.push(like);
    let refused = tree::read(&twice, &app, 3.0).expect_err("two elements fit like");
    assert!(
        matches!(&refused, TreeError::Ambiguous { node, candidates }
            if node.contains("like") && candidates.len() == 2),
        "{refused:?}"
    );
    assert!(matches!(refused.into_error(), Error::Ambiguous { .. }));

    let mut alike = controls();
    let mut copy = alike.roots[0].children[1].clone();
    copy.identifier = Some("like-again".to_owned());
    alike.roots[0].children.push(copy);
    let refused = tree::read(&served(&controls(), false), &alike, 3.0)
        .expect_err("two nodes fit one element");
    assert!(
        matches!(refused, TreeError::Ambiguous { .. }),
        "{refused:?}"
    );

    let refused = tree::read(&served(&app, false), &Tree::new(Vec::new(), false), 3.0)
        .expect_err("no app tree");
    assert_eq!(refused, TreeError::NoAppTree);
    assert!(matches!(refused.into_error(), Error::NotFound { .. }));
}

/// The lane connected to a scripted Simulator whose window shows `app`'s screen, and the window,
/// with the directory the lane's hold lives in.
pub(crate) fn lane_showing(
    app: &Tree,
) -> (
    tempfile::TempDir,
    flui_driver::lane::ios_simulator::IosSimulatorLane,
    ScriptedSimulatorScreen,
) {
    let screen = ScriptedSimulatorScreen::new(TITLE, served(app, true));
    let (dir, lane) = connect(&scripted());
    (dir, lane.read_through(screen.accessibility()), screen)
}

/// Through the lane: it says it reads the Simulator's bridge, reads the window's tree as the app
/// published it, asking for the device's window by its title and the app's window's size, and
/// presses the element the app's identifier names through the bridge, finding it as it read it.
#[test]
fn the_lane_reads_and_presses_through_the_simulators_window() {
    let app = controls();
    let (_dir, mut lane, screen) = lane_showing(&app);
    assert_eq!(
        lane.capabilities().accessibility,
        Some(AccessibilityApi::SimulatorAx)
    );
    assert!(AccessibilityApi::SimulatorAx.matches_app_tree());
    let projected = lane.read_tree(&placement(), &app).expect("the tree");
    assert!(Projection::of(&app, &projected).is_equal());
    assert_eq!(
        screen.requests(),
        [ScreenRequest::new(TITLE, SIZE, 3.0)],
        "the device's window, for the app's window"
    );
    lane.invoke(&placement(), "like", &app).expect("pressed");
    let pressed = screen.presses();
    assert!(
        matches!(pressed.as_slice(), [like] if like.native_role == "AXButton"
            && like.name.as_deref() == Some("Like")),
        "{pressed:?}"
    );
}

/// The lane presses nothing it cannot find: an identifier the app does not publish, or an element
/// no node fits, is not found before any press.
#[test]
fn the_lane_presses_nothing_it_cannot_match() {
    let app = controls();
    let (_dir, mut lane, screen) = lane_showing(&app);
    let unknown = lane
        .invoke(&placement(), "nobody", &app)
        .expect_err("no such identifier");
    assert!(matches!(unknown, Error::NotFound { .. }), "{unknown:?}");
    assert_eq!(
        screen.requests(),
        Vec::<ScreenRequest>::new(),
        "nothing read for an unknown identifier"
    );

    let mut renamed = controls();
    renamed.roots[0].children[1].name = Some("Liked".to_owned());
    let unmatched = lane
        .invoke(&placement(), "like", &renamed)
        .expect_err("the element shows another label");
    assert!(
        matches!(&unmatched, Error::NotFound { message, .. } if message.contains("VoiceOver")),
        "{unmatched:?}"
    );
    assert_eq!(screen.presses(), Vec::<Node>::new());
}

/// A read of a window that is not on screen says so; a lane handed no read reads and presses
/// nothing, naming what it cannot.
#[test]
fn the_lane_refuses_a_window_it_cannot_see_and_a_read_it_was_not_handed() {
    let app = controls();
    let elsewhere = ScriptedSimulatorScreen::new("iPad Air \u{2013} iOS 18.0", served(&app, true));
    let (_dir, lane) = connect(&scripted());
    let mut lane = lane.read_through(elsewhere.accessibility());
    let missing = lane.read_tree(&placement(), &app).expect_err("no window");
    assert!(
        matches!(&missing, Error::NotFound { message, .. } if message.contains(TITLE)),
        "{missing:?}"
    );

    let (_dir, mut bare) = connect(&scripted());
    assert_eq!(bare.capabilities().accessibility, None);
    let refused = bare.read_tree(&placement(), &app).expect_err("no read");
    assert_eq!(
        refused.missing_capability().as_deref(),
        Some("os_accessibility_tree")
    );
    let refused = bare
        .invoke(&placement(), "like", &app)
        .expect_err("no press");
    assert_eq!(refused.missing_capability().as_deref(), Some("os_invoke"));

    // The scripted window refuses a press of an element it does not show, as the Mac's does.
    let mut window = ScriptedSimulatorScreen::new(TITLE, served(&app, true));
    let ghost = Node::new(id(99), Role::Button, "AXButton");
    let refused = window
        .press(&ScreenRequest::new(TITLE, SIZE, 3.0), &ghost)
        .expect_err("no such element");
    assert!(matches!(refused, Error::NotFound { .. }));
}
