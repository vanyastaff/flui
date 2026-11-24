//! The iOS Simulator lane's read of the app's accessibility tree on every host: the role table
//! against `accesskit_ios`'s trait choices, the device's window and its screen found in windows
//! shaped as Simulator.app and DeviceHub draw them, the elements matched to the app's tree by
//! role, label, state and bounds, the refusals (two windows, a title two simulators share, two
//! screens, two candidates alike, a press that could mean two elements, the permission taken
//! back), and the lane reading and pressing through a scripted Simulator window
//! (`ScriptedSimulatorScreen`).
//!
//! The screens are written out by hand as the Mac's accessibility API would serve them, from
//! what `accesskit_ios` 0.2.2 hands UIKit (`NodeWrapper`, `PlatformNode` and
//! `filter_for_is_accessibility_element`) and what the bridge's vocabulary names
//! (`FBSimulatorBridgeProtocol/BridgeAXTranslatorRole.swift` in `idb`), never computed with the
//! lane's own table: a role typed out per element, the label as `AXDescription`, a static text's
//! as its `AXValue` (AppKit's static text carries its text there), a toggled state's "1" or "0"
//! as the value, the hint as `AXHelp`, the author's identifier as `AXIdentifier`, a node with an
//! action, a value or a toggled state an element whose descendants fold into its label, a
//! container absent, and FLUI's two announcers served last as static text with no area. Frames
//! are in the Mac's points: the window's device pixels divided by its scale, from the screen's
//! origin.

use std::time::Duration;

use flui_driver::Error;
use flui_driver::lane::ios_simulator::accessibility::{
    self, HostAccessibility as _, HostScreen, PointFrame, ScreenElement, ScreenRequest,
    WindowIdentity,
};
use flui_driver::lane::ios_simulator::device::{self, Udid};
use flui_driver::lane::ios_simulator::roles::{
    self, ADAPTER_CHECKED_AGAINST, ROLES, RoleTrait, SCREEN_SUBROLE,
};
use flui_driver::lane::ios_simulator::tree::{
    self, BOUNDS_TOLERANCE_POINTS, Placing, ScreenCandidate, TreeError, WindowFacts, WindowTree,
};
use flui_driver::lane::walk::{MAX_NODES, ProviderError};
use flui_driver::lane::{AccessibilityApi, Lane as _};
use flui_driver::projection::Projection;
use flui_driver::protocol::geometry::LogicalSize;
use flui_driver::protocol::session::Placement;
use flui_driver::protocol::tree::{ElementId, Node, Rect, Tree, WindowId};
use flui_driver::protocol::wire::{Checked, Role};
use flui_driver::testing::ScriptedSimulatorScreen;

use crate::ios_simulator_lane::{UDID, connect, listing, scripted};

/// The title of the scripted simulator's window: `iPhone 16` on iOS 18.0.
pub(crate) const TITLE: &str = "iPhone 16 \u{2013} iOS 18.0";

/// An iPhone's window, 393 by 852 points at 3x.
const SIZE: LogicalSize = LogicalSize::new(393.0, 852.0);

/// Where Simulator.app draws that window's screen, at its size.
const SCREEN: PointFrame = PointFrame::new(258.0, 222.0, 393.0, 852.0);

fn placing() -> Placing {
    Placing {
        screen: SCREEN,
        size: SIZE,
        scale: 3.0,
    }
}

fn udid() -> Udid {
    Udid::try_new(UDID).expect("a UDID")
}

fn request() -> ScreenRequest {
    ScreenRequest::new(TITLE, udid(), SIZE, 3.0)
}

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

/// An element of `role` (and `subrole`) framed at `frame` in the Mac's points.
pub(crate) fn element(
    role: &str,
    subrole: Option<&str>,
    frame: (f64, f64, f64, f64),
) -> ScreenElement {
    let mut element = ScreenElement::new(role);
    element.subrole = subrole.map(str::to_owned);
    element.frame = Some(PointFrame::new(frame.0, frame.1, frame.2, frame.3));
    element
}

/// `element` labelled `label`, as the bridge serves an iOS label.
pub(crate) fn labelled(mut element: ScreenElement, label: &str) -> ScreenElement {
    element.description = Some(label.to_owned());
    element
}

/// `element` with the static text `text`, as AppKit's static text carries it.
pub(crate) fn text(mut element: ScreenElement, text: &str) -> ScreenElement {
    element.value = Some(text.to_owned());
    element
}

/// `element` carrying `identifier`.
pub(crate) fn tagged(mut element: ScreenElement, identifier: &str) -> ScreenElement {
    element.identifier = Some(identifier.to_owned());
    element
}

/// FLUI's two announcers as `accesskit_ios` serves them: static text with no label and no area,
/// at the screen's origin `(x, y)`.
pub(crate) fn announcers(x: f64, y: f64) -> [ScreenElement; 2] {
    [
        element("AXStaticText", None, (x, y, 0.0, 0.0)),
        element("AXStaticText", None, (x, y, 0.0, 0.0)),
    ]
}

/// [`controls`] as the Simulator serves it on Simulator.app's screen at [`SCREEN`], written out:
/// the frames are the app's rectangles divided by 3 from the screen's origin. `identifiers`
/// serves each author's identifier.
fn controls_screen(identifiers: bool) -> HostScreen {
    let tag = |element: ScreenElement, identifier: &str| {
        if identifiers {
            tagged(element, identifier)
        } else {
            element
        }
    };
    let mut subscribe = labelled(
        element("AXCheckBox", Some("AXSwitch"), (258.0, 272.0, 160.0, 40.0)),
        "Subscribe",
    );
    subscribe.value = Some("0".to_owned());
    let mut elements = vec![
        text(
            element("AXStaticText", None, (258.0, 222.0, 70.0, 17.0)),
            "Gallery",
        ),
        tag(
            labelled(
                element("AXButton", None, (258.0, 239.0, 60.0, 33.0)),
                "Like",
            ),
            "like",
        ),
        tag(
            text(
                element("AXStaticText", None, (258.0, 257.0, 100.0, 15.0)),
                "Likes: 0",
            ),
            "likes",
        ),
        tag(subscribe, "subscribe"),
        // A text field carries no trait, so the bridge serves a generic element; an empty value
        // reads as none.
        tag(
            element("AXGenericElement", None, (258.0, 312.0, 200.0, 30.0)),
            "comment",
        ),
        tag(
            text(
                element("AXStaticText", None, (258.0, 342.0, 240.0, 120.0)),
                "Pad: 0",
            ),
            "pad",
        ),
    ];
    elements.extend(announcers(258.0, 222.0));
    HostScreen::new(SCREEN, elements)
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
/// in the Windows lane's order.
#[test]
fn the_role_table_covers_the_vocabulary_once() {
    let windows: Vec<_> = flui_driver::lane::windows::roles::ROLES
        .iter()
        .map(|row| row.role)
        .collect();
    let ios: Vec<_> = ROLES.iter().map(|row| row.role).collect();
    assert_eq!(ios, windows);
}

#[test]
fn the_window_is_titled_by_the_device_and_its_runtime() {
    let listing = listing("Booted", true);
    let simulators = device::parse_devices(listing.as_bytes()).expect("a listing");
    let simulator = device::choose(&simulators, &udid()).expect("the named simulator");
    assert_eq!(accessibility::window_title(simulator), TITLE);
    accessibility::check_title_names_one(&simulators, simulator).expect("one iPhone 16 on 18.0");
}

/// A title two listed simulators share is refused, whatever the other's state: DeviceHub keeps a
/// window titled for a simulator that shut down.
#[test]
fn a_title_two_simulators_share_is_refused() {
    let twice = listing("Booted", true).replace(
        r#""name": "iPad Air""#,
        r#""name": "iPhone 16", "state": "Shutdown""#,
    );
    let simulators = device::parse_devices(twice.as_bytes()).expect("a listing");
    let simulator = device::choose(&simulators, &udid()).expect("the named simulator");
    let refused = accessibility::check_title_names_one(&simulators, simulator)
        .expect_err("two simulators titled alike");
    assert!(
        matches!(&refused, Error::Ambiguous { candidates, .. } if candidates.len() == 2),
        "{refused:?}"
    );

    // Through the lane: refused before the window is read.
    let xcrun = flui_driver::testing::ScriptedXcrun::new().reply(
        "simctl list devices",
        flui_driver::lane::ios_simulator::xcrun::XcrunReply::new(0, twice),
    );
    let screen = ScriptedSimulatorScreen::new(TITLE, controls_screen(true));
    let (_dir, lane) = connect(&xcrun);
    let mut lane = lane.read_through(screen.accessibility());
    let refused = lane
        .read_tree(&placement(), &controls())
        .expect_err("refused");
    assert!(matches!(refused, Error::Ambiguous { .. }), "{refused:?}");
    assert_eq!(screen.requests(), Vec::<ScreenRequest>::new());
}

/// A window's attributes name its simulator when one carries a UDID: the asked one's is the
/// window, another's is not, and none leaves the title to stand for it.
#[test]
fn a_window_naming_another_simulator_is_not_the_devices() {
    let ours = Some(format!("com.apple.CoreSimulator.SimDevice.{UDID}"));
    let other = Some("AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE".to_owned());
    assert_eq!(
        accessibility::window_identity(&udid(), &[None, ours]),
        WindowIdentity::Named
    );
    assert_eq!(
        accessibility::window_identity(&udid(), &[other.clone(), None]),
        WindowIdentity::Other
    );
    assert_eq!(
        accessibility::window_identity(&udid(), &[Some("iPhone 16".to_owned()), None]),
        WindowIdentity::Unknown
    );
    assert_eq!(
        accessibility::window_identity(&udid(), &[other, Some(UDID.to_ascii_lowercase())]),
        WindowIdentity::Named
    );
}

/// One node of a window a test builds.
#[derive(Debug, Clone)]
struct Fixture {
    facts: WindowFacts,
    identity: Vec<Option<String>>,
    children: Vec<usize>,
}

/// A set of windows written out as nodes by index, each window a root.
struct Windows {
    nodes: Vec<Fixture>,
}

impl Windows {
    fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    /// Add a node of `role` and `subrole` at `frame` under `parent`; its index.
    fn add(
        &mut self,
        parent: Option<usize>,
        role: &str,
        subrole: Option<&str>,
        frame: Option<(f64, f64, f64, f64)>,
    ) -> usize {
        let index = self.nodes.len();
        self.nodes.push(Fixture {
            facts: WindowFacts {
                role: role.to_owned(),
                subrole: subrole.map(str::to_owned),
                frame: frame.map(|frame| PointFrame::new(frame.0, frame.1, frame.2, frame.3)),
            },
            identity: Vec::new(),
            children: Vec::new(),
        });
        if let Some(parent) = parent {
            self.nodes[parent].children.push(index);
        }
        index
    }
}

impl WindowTree for Windows {
    type Element = usize;

    fn children(&mut self, element: &usize) -> flui_driver::Result<Vec<usize>> {
        Ok(self.nodes[*element].children.clone())
    }

    fn facts(&mut self, element: &usize) -> flui_driver::Result<WindowFacts> {
        Ok(self.nodes[*element].facts.clone())
    }

    fn identity(&mut self, window: &usize) -> flui_driver::Result<Vec<Option<String>>> {
        Ok(self.nodes[*window].identity.clone())
    }
}

/// DeviceHub's window as `mgcrea/cupertino` measured it on Xcode 27: a sidebar and an inspector
/// beside nested panes, the screen an `iOSContentGroup` at depth 7, reached along a second path
/// at depth 8, drawn at 0.74, and the app's own groups under it.
#[test]
fn the_search_reaches_devicehubs_screen_seven_levels_down() {
    let mut windows = Windows::new();
    let window = windows.add(None, "AXWindow", None, Some((246.0, 185.0, 1100.0, 800.0)));
    windows.add(
        Some(window),
        "AXGroup",
        None,
        Some((246.0, 185.0, 220.0, 800.0)),
    );
    let mut pane = windows.add(
        Some(window),
        "AXGroup",
        None,
        Some((466.0, 185.0, 660.0, 800.0)),
    );
    // Panes at depths 2 to 6.
    for _ in 0..5 {
        pane = windows.add(
            Some(pane),
            "AXGroup",
            None,
            Some((466.0, 185.0, 660.0, 800.0)),
        );
    }
    // A pane of the device's proportions that is no screen.
    windows.add(
        Some(pane),
        "AXGroup",
        None,
        Some((480.0, 200.0, 39.3, 85.2)),
    );
    let drawn = (657.0, 266.0, 393.0 * 0.74, 852.0 * 0.74);
    let screen = windows.add(Some(pane), "AXGroup", Some(SCREEN_SUBROLE), Some(drawn));
    windows.add(Some(screen), "AXGroup", None, Some(drawn));
    let around = windows.add(
        Some(pane),
        "AXGroup",
        None,
        Some((657.0, 266.0, 400.0, 700.0)),
    );
    windows.add(Some(around), "AXGroup", Some(SCREEN_SUBROLE), Some(drawn));
    let (found, frame) =
        tree::find_screen(&mut windows, &request(), vec![window]).expect("the screen is found");
    assert_eq!(
        found, screen,
        "the shallower of the two paths, an iOSContentGroup"
    );
    assert_eq!(frame, PointFrame::new(drawn.0, drawn.1, drawn.2, drawn.3));
}

/// Simulator.app's window: the screen a child of a group the bezel adds, beside the bezel's
/// buttons and the toolbar, the app's elements under it.
#[test]
fn the_search_reaches_a_screen_inside_simulator_apps_bezel() {
    let mut windows = Windows::new();
    let window = windows.add(None, "AXWindow", None, Some((231.0, 142.0, 456.0, 972.0)));
    windows.add(
        Some(window),
        "AXButton",
        None,
        Some((231.0, 300.0, 4.0, 40.0)),
    );
    windows.add(
        Some(window),
        "AXToolbar",
        None,
        Some((231.0, 142.0, 456.0, 52.0)),
    );
    let bezel = windows.add(
        Some(window),
        "AXGroup",
        None,
        Some((231.0, 194.0, 456.0, 920.0)),
    );
    let screen = windows.add(
        Some(bezel),
        "AXGroup",
        None,
        Some((258.0, 222.0, 393.0, 852.0)),
    );
    // An app element of the screen's own shape, deeper: the screen is the shallowest.
    windows.add(
        Some(screen),
        "AXGroup",
        None,
        Some((258.0, 222.0, 393.0, 852.0)),
    );
    let (found, frame) =
        tree::find_screen(&mut windows, &request(), vec![window]).expect("the screen is found");
    assert_eq!(found, screen);
    assert_eq!(frame, SCREEN);
}

/// Two windows of the title are refused unless one names another simulator; none is not found.
#[test]
fn the_search_refuses_two_windows_and_finds_none_in_none() {
    let mut windows = Windows::new();
    let first = windows.add(None, "AXWindow", None, None);
    windows.add(
        Some(first),
        "AXGroup",
        None,
        Some((258.0, 222.0, 393.0, 852.0)),
    );
    let second = windows.add(None, "AXWindow", None, None);
    windows.add(
        Some(second),
        "AXGroup",
        None,
        Some((900.0, 222.0, 393.0, 852.0)),
    );
    let refused = tree::find_screen(&mut windows, &request(), vec![first, second])
        .expect_err("two windows titled alike");
    assert!(matches!(refused, Error::Ambiguous { .. }), "{refused:?}");

    windows.nodes[second].identity = vec![Some("AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE".to_owned())];
    let (found, _) = tree::find_screen(&mut windows, &request(), vec![first, second])
        .expect("the other simulator's window is left out");
    assert_eq!(windows.nodes[first].children, [found]);

    let missing = tree::find_screen(&mut windows, &request(), Vec::new()).expect_err("none");
    assert!(
        matches!(&missing, Error::NotFound { message, .. } if message.contains(TITLE)),
        "{missing:?}"
    );
}

fn candidate(frame: (f64, f64, f64, f64), subrole: Option<&str>, depth: usize) -> ScreenCandidate {
    ScreenCandidate {
        frame: PointFrame::new(frame.0, frame.1, frame.2, frame.3),
        subrole: subrole.map(str::to_owned),
        depth,
    }
}

/// The screen is the group with the window's shape: an `iOSContentGroup` first, else the
/// shallowest; a turned window's turned; none and two of another frame are refused.
#[test]
fn the_screen_is_the_group_with_the_windows_shape() {
    let simulator_app = [
        candidate((231.0, 142.0, 456.0, 52.0), None, 1),
        candidate((258.0, 222.0, 393.0, 852.0), None, 1),
    ];
    assert_eq!(
        tree::choose_screen(TITLE, &simulator_app, SIZE, false),
        Ok(1)
    );
    let marked_deeper = [
        candidate((0.0, 0.0, 393.0, 852.0), None, 2),
        candidate((657.0, 266.0, 290.82, 630.48), Some(SCREEN_SUBROLE), 7),
    ];
    assert_eq!(
        tree::choose_screen(TITLE, &marked_deeper, SIZE, false),
        Ok(1)
    );
    let turned = [candidate((0.0, 0.0, 852.0, 393.0), None, 1)];
    assert_eq!(
        tree::choose_screen(TITLE, &turned, LogicalSize::new(852.0, 393.0), false),
        Ok(0)
    );
    assert!(matches!(
        tree::choose_screen(TITLE, &turned, SIZE, true),
        Err(TreeError::NoScreen { groups, cut_short: true, .. }) if groups.len() == 1
    ));
    let two = [
        candidate((0.0, 0.0, 393.0, 852.0), None, 1),
        candidate((500.0, 0.0, 393.0, 852.0), None, 1),
    ];
    let refused = tree::choose_screen(TITLE, &two, SIZE, false).expect_err("two screens");
    assert!(matches!(&refused, TreeError::Ambiguous { candidates, .. } if candidates.len() == 2));
    let two_marked = [
        candidate((0.0, 0.0, 393.0, 852.0), Some(SCREEN_SUBROLE), 7),
        candidate((500.0, 0.0, 393.0, 852.0), Some(SCREEN_SUBROLE), 8),
    ];
    assert!(matches!(
        tree::choose_screen(TITLE, &two_marked, SIZE, false),
        Err(TreeError::Ambiguous { .. })
    ));
    for unaddressable in [(0.0, 0.0, 0.0, 0.0), (f64::NAN, 0.0, 393.0, 852.0)] {
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
}

/// An element's frame in the Mac's points lands on the app's window in its device pixels, from
/// the screen's origin and at the scale the window draws it: at its size and drawn at three
/// quarters alike. A frame with no area, or off any finite place, lands nowhere.
#[test]
fn an_elements_points_land_on_the_windows_pixels() {
    let expected = Some(Rect {
        x: 60,
        y: 150,
        width: 180,
        height: 99,
    });
    assert_eq!(
        tree::window_rect(PointFrame::new(278.0, 272.0, 60.0, 33.0), SCREEN, SIZE, 3.0),
        expected
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
        expected
    );
    for nowhere in [
        PointFrame::new(278.0, 272.0, 0.0, 33.0),
        PointFrame::new(f64::NAN, 272.0, 60.0, 33.0),
        PointFrame::new(1e300, 272.0, 60.0, 33.0),
    ] {
        assert_eq!(
            tree::window_rect(nowhere, SCREEN, SIZE, 3.0),
            None,
            "{nowhere:?}"
        );
    }
}

/// A static text's label is its value, where AppKit's static text carries it, read as no value
/// besides; any other element's label is its description, else its title, and its value stays
/// its value.
#[test]
fn a_static_texts_label_is_its_value() {
    let gallery = text(
        element("AXStaticText", None, (258.0, 222.0, 70.0, 17.0)),
        "Gallery",
    );
    let node = tree::describe(&gallery, id(1), placing());
    assert_eq!(node.name.as_deref(), Some("Gallery"));
    assert_eq!(node.value, None);
    let mut titled = element("AXStaticText", None, (258.0, 222.0, 70.0, 17.0));
    titled.title = Some("Title".to_owned());
    titled.value = Some("Value".to_owned());
    let node = tree::describe(&titled, id(1), placing());
    assert_eq!(node.name.as_deref(), Some("Title"));
    assert_eq!(node.value.as_deref(), Some("Value"));
    let mut field = element("AXGenericElement", None, (258.0, 312.0, 200.0, 30.0));
    field.value = Some("hello".to_owned());
    let node = tree::describe(&field, id(1), placing());
    assert_eq!(
        node.name, None,
        "only static text reads its value as its label"
    );
    assert_eq!(node.value.as_deref(), Some("hello"));
}

/// The scenario app's screen as the Simulator serves it reads as what the app published: each
/// element matched to its node, carrying its role and identifier, the switch's state from its
/// value, the field's empty value and the static texts' labels from their values; the
/// announcers are left out, and the projections equal.
#[test]
fn the_screen_projects_to_the_apps_tree() {
    let app = controls();
    for identifiers in [true, false] {
        let screen = controls_screen(identifiers);
        let projected = tree::read(&screen, &app, placing()).expect("the screen matches");
        let like = tree::element_of(&projected.roots, "like").expect("like matched");
        assert_eq!(
            (like.role, like.native_role.as_str()),
            (Role::Button, "AXButton")
        );
        let subscribe = tree::element_of(&projected.roots, "subscribe").expect("matched");
        assert_eq!(subscribe.native_role, "AXCheckBox/AXSwitch");
        assert_eq!(subscribe.checked, Some(Checked::False));
        let comment = tree::element_of(&projected.roots, "comment").expect("matched");
        assert_eq!(comment.value.as_deref(), Some(""));
        let pad = tree::element_of(&projected.roots, "pad").expect("matched");
        assert_eq!(pad.name.as_deref(), Some("Pad: 0"));
        assert_eq!(
            projected.roots[0].children.len(),
            6,
            "the announcers are left out"
        );
        let projection = Projection::of(&app, &projected);
        assert!(
            projection.is_equal(),
            "{identifiers}: {:?}",
            projection.differences()
        );
        let found = tree::matched_element(&screen, &app, placing(), "like")
            .expect("matched")
            .expect("like has its element");
        assert_eq!(found.description.as_deref(), Some("Like"));
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
    let screen = HostScreen::new(
        SCREEN,
        vec![
            labelled(
                element("AXGenericElement", None, (258.0, 222.0, 393.0, 50.0)),
                "First",
            ),
            labelled(
                element("AXGenericElement", None, (258.0, 272.0, 393.0, 50.0)),
                "Second",
            ),
        ],
    );
    let projected = tree::read(&screen, &app, placing()).expect("matched");
    for identifier in ["first", "second"] {
        assert_eq!(
            tree::element_of(&projected.roots, identifier).map(|node| node.role),
            Some(Role::ListItem),
            "{identifier}"
        );
    }

    let mut wrapped = controls_screen(false);
    let mut container = ScreenElement::new("AXGroup");
    container.children = std::mem::take(&mut wrapped.elements);
    wrapped.elements = vec![container];
    let projected = tree::read(&wrapped, &controls(), placing()).expect("matched");
    assert!(Projection::of(&controls(), &projected).is_equal());
}

/// An app node matched once is matched to nothing else: an element inside an element no node
/// fits, alike to one a sibling matched, stays unmatched.
#[test]
fn an_app_node_is_matched_once() {
    let mut screen = controls_screen(false);
    let mut unmatched = labelled(
        element("AXGroup", None, (258.0, 500.0, 10.0, 10.0)),
        "Elsewhere",
    );
    unmatched.children = vec![labelled(
        element("AXButton", None, (258.0, 239.0, 60.0, 33.0)),
        "Like",
    )];
    screen.elements.insert(2, unmatched);
    let projected = tree::read(&screen, &controls(), placing()).expect("read");
    let likes: usize = count(&projected.roots, "like");
    assert_eq!(likes, 1, "{projected:#?}");
}

fn count(nodes: &[Node], identifier: &str) -> usize {
    nodes
        .iter()
        .map(|node| {
            usize::from(node.identifier.as_deref() == Some(identifier))
                + count(&node.children, identifier)
        })
        .sum()
}

/// An app subtree past the walk's budget is compared in part, and the projection says so.
#[test]
fn an_app_past_the_budget_projects_as_truncated() {
    let mut app = controls();
    let filler = (0..=u64::try_from(MAX_NODES).expect("the budget fits"))
        .map(|number| app_node(100 + number, Role::Label, "Label", None, (0, 0, 1, 1)))
        .collect::<Vec<Node>>();
    app.roots[0].children.extend(filler);
    let projected = tree::read(&controls_screen(true), &app, placing()).expect("read");
    assert!(projected.truncated);
    let whole = tree::read(&controls_screen(true), &controls(), placing()).expect("read");
    assert!(!whole.truncated);
}

/// A change made to a served element.
type Change = dyn Fn(&mut ScreenElement);

/// An element fits a node only when every fact agrees: another label, role, state, identifier or
/// a frame past the tolerance leaves it unmatched; a frame within the tolerance still fits.
#[test]
fn an_element_fits_only_the_node_whose_facts_it_shows() {
    // The tolerance at 3x, in device pixels: 5, under two Mac points.
    assert_eq!((BOUNDS_TOLERANCE_POINTS * 3.0).ceil(), 5.0);
    let changed = |change: &Change| {
        let mut screen = controls_screen(true);
        change(&mut screen.elements[1]);
        tree::read(&screen, &controls(), placing()).expect("read")
    };
    let unmatched: &[(&str, &Change)] = &[
        ("label", &|element| {
            element.description = Some("Liked".to_owned());
        }),
        ("role", &|element| element.role = "AXLink".to_owned()),
        ("enabled", &|element| element.enabled = Some(false)),
        ("identifier", &|element| {
            element.identifier = Some("other".to_owned());
        }),
        ("frame", &|element| {
            if let Some(frame) = &mut element.frame {
                frame.x += 2.0;
            }
        }),
        ("no frame", &|element| element.frame = None),
    ];
    for (fact, change) in unmatched {
        let projected = changed(change);
        assert_eq!(tree::element_of(&projected.roots, "like"), None, "{fact}");
        assert_eq!(projected.roots[0].children[1].identifier, None, "{fact}");
    }
    let projected = changed(&|element| {
        if let Some(frame) = &mut element.frame {
            frame.x += 1.5;
        }
    });
    assert!(tree::element_of(&projected.roots, "like").is_some());
}

/// Two elements alike for one node, or two nodes alike for one element, are refused: the lane
/// does not guess. An app that published no tree is refused too.
#[test]
fn an_ambiguous_match_is_refused() {
    let app = controls();
    let mut twice = controls_screen(false);
    let like = twice.elements[1].clone();
    twice.elements.insert(2, like);
    let refused = tree::read(&twice, &app, placing()).expect_err("two elements fit like");
    assert!(
        matches!(&refused, TreeError::Ambiguous { node, candidates }
            if node.contains("like") && candidates.len() == 2),
        "{refused:?}"
    );

    let mut alike = controls();
    let mut copy = alike.roots[0].children[1].clone();
    copy.id = id(50);
    copy.identifier = Some("like-again".to_owned());
    alike.roots[0].children.push(copy);
    let refused = tree::read(&controls_screen(false), &alike, placing())
        .expect_err("two nodes fit one element");
    assert!(
        matches!(refused, TreeError::Ambiguous { .. }),
        "{refused:?}"
    );

    let refused = tree::read(
        &controls_screen(false),
        &Tree::new(Vec::new(), false),
        placing(),
    )
    .expect_err("no app tree");
    assert_eq!(refused, TreeError::NoAppTree);
}

/// A press finds its element again by the facts a read gave, and refuses when none or more than
/// one reads as it.
#[test]
fn a_press_refuses_an_element_it_cannot_tell_apart() {
    let screen = controls_screen(false);
    let like = screen.elements[1].clone();
    assert_eq!(tree::locate(&screen, &like).expect("one"), [1]);
    let mut twice = screen.clone();
    let mut wrapper = ScreenElement::new("AXGroup");
    wrapper.children = vec![like.clone()];
    twice.elements.push(wrapper);
    let refused = tree::locate(&twice, &like).expect_err("two read alike");
    assert!(matches!(refused, Error::Ambiguous { .. }), "{refused:?}");
    let gone = labelled(element("AXButton", None, (0.0, 0.0, 1.0, 1.0)), "Gone");
    assert!(matches!(
        tree::locate(&screen, &gone),
        Err(Error::NotFound { .. })
    ));
}

/// A failure while the process is no longer trusted is the permission taken back, whatever the
/// call said; a call that did not answer names the deadline it ran under.
#[test]
fn a_permission_taken_back_mid_read_is_named() {
    for error in [
        ProviderError::Failed("the accessibility API answered -25211".to_owned()),
        ProviderError::Timeout,
        ProviderError::Gone,
    ] {
        let refused = accessibility::read_failure(error, false, Duration::from_secs(5));
        assert_eq!(
            refused.missing_capability().as_deref(),
            Some("macos_permission_accessibility")
        );
    }
    let timeout =
        accessibility::read_failure(ProviderError::Timeout, true, Duration::from_millis(1500));
    assert!(
        matches!(&timeout, Error::Timeout { message, .. } if message.contains("1500 ms")),
        "{timeout:?}"
    );
    assert!(matches!(
        accessibility::read_failure(ProviderError::Failed("x".to_owned()), true, Duration::ZERO),
        Error::Platform { .. }
    ));
}

/// The lane connected to a scripted Simulator whose window shows `screen`, and the window, with
/// the directory the lane's hold lives in.
fn lane_showing(
    screen: HostScreen,
) -> (
    tempfile::TempDir,
    flui_driver::lane::ios_simulator::IosSimulatorLane,
    ScriptedSimulatorScreen,
) {
    let screen = ScriptedSimulatorScreen::new(TITLE, screen);
    let (dir, lane) = connect(&scripted());
    (dir, lane.read_through(screen.accessibility()), screen)
}

/// Through the lane: it says it reads the Simulator's bridge, reads the window's tree as the app
/// published it, asking for the device's window by its title, its simulator and the app's
/// window's size, and presses the element the app's identifier names.
#[test]
fn the_lane_reads_and_presses_through_the_simulators_window() {
    let app = controls();
    let (_dir, mut lane, screen) = lane_showing(controls_screen(false));
    assert_eq!(
        lane.capabilities().accessibility,
        Some(AccessibilityApi::SimulatorAx)
    );
    assert!(AccessibilityApi::SimulatorAx.matches_app_tree());
    assert!(AccessibilityApi::SimulatorAx.activates());
    assert!(!AccessibilityApi::UiAutomator.activates());
    let projected = lane.read_tree(&placement(), &app).expect("the tree");
    assert!(Projection::of(&app, &projected).is_equal());
    assert_eq!(projected.roots[0].window, Some(window()));
    assert_eq!(screen.requests(), [request()]);
    lane.invoke(&placement(), "like", &app).expect("pressed");
    let pressed = screen.presses();
    assert!(
        matches!(pressed.as_slice(), [like] if like.role == "AXButton"
            && like.description.as_deref() == Some("Like")),
        "{pressed:?}"
    );
}

/// The lane presses nothing it cannot find: an identifier the app does not publish, or an element
/// no node fits, is not found before any press.
#[test]
fn the_lane_presses_nothing_it_cannot_match() {
    let app = controls();
    let (_dir, mut lane, screen) = lane_showing(controls_screen(false));
    let unknown = lane
        .invoke(&placement(), "nobody", &app)
        .expect_err("no such identifier");
    assert!(matches!(unknown, Error::NotFound { .. }), "{unknown:?}");
    assert_eq!(screen.requests(), Vec::<ScreenRequest>::new());

    let mut renamed = controls();
    renamed.roots[0].children[1].name = Some("Liked".to_owned());
    let unmatched = lane
        .invoke(&placement(), "like", &renamed)
        .expect_err("the element shows another label");
    assert!(
        matches!(&unmatched, Error::NotFound { message, .. } if message.contains("VoiceOver")),
        "{unmatched:?}"
    );
    assert_eq!(screen.presses(), Vec::<ScreenElement>::new());
}

/// A read of a window that is not on screen says so; a lane handed no read reads and presses
/// nothing, naming what it cannot.
#[test]
fn the_lane_refuses_a_window_it_cannot_see_and_a_read_it_was_not_handed() {
    let app = controls();
    let elsewhere =
        ScriptedSimulatorScreen::new("iPad Air \u{2013} iOS 18.0", controls_screen(true));
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

    let mut window = ScriptedSimulatorScreen::new(TITLE, controls_screen(true));
    let ghost = labelled(element("AXButton", None, (0.0, 0.0, 1.0, 1.0)), "Ghost");
    let refused = window
        .press(&request(), &ghost)
        .expect_err("no such element");
    assert!(matches!(refused, Error::NotFound { .. }));
}

/// Where the scripted Simulator draws the headless scenario window's screen: 800 by 600 points at
/// scale 1, at its size.
const PAGE: PointFrame = PointFrame::new(100.0, 50.0, 800.0, 600.0);

/// The headless scenario app's controls page, after one like, as the Simulator would serve it,
/// written out: each frame is the app's rectangle at scale 1 from [`PAGE`]'s origin.
pub(crate) fn controls_page() -> HostScreen {
    let mut subscribe = labelled(
        element("AXCheckBox", Some("AXSwitch"), (420.0, 83.0, 160.0, 41.0)),
        "Subscribe",
    );
    subscribe.value = Some("0".to_owned());
    let mut elements = vec![
        text(
            element("AXStaticText", None, (478.0, 50.0, 43.0, 16.0)),
            "Gallery",
        ),
        tagged(
            labelled(element("AXButton", None, (487.0, 66.0, 25.0, 17.0)), "Like"),
            "like",
        ),
        // An empty `Semantics` with no child: static text with no area.
        tagged(
            text(
                element("AXStaticText", None, (500.0, 83.0, 0.0, 1.0)),
                "Likes: 1",
            ),
            "likes",
        ),
        tagged(subscribe, "subscribe"),
        tagged(
            element("AXGenericElement", None, (112.0, 131.0, 776.0, 18.0)),
            "comment",
        ),
        tagged(
            text(
                element("AXStaticText", None, (380.0, 156.0, 240.0, 121.0)),
                "Pad: 0",
            ),
            "pad",
        ),
    ];
    elements.extend(announcers(PAGE.x, PAGE.y));
    HostScreen::new(PAGE, elements)
}

/// How a row of the roles page reaches the bridge.
#[derive(Clone, Copy)]
enum Named {
    /// Its label is `accessibilityLabel`, served as `AXDescription`.
    Label,
    /// A role ARIA forbids a name: FLUI publishes its label as the description, which
    /// `accesskit_ios` hands UIKit as the hint, served as `AXHelp`.
    Hint,
}

/// The roles page's rows in vocabulary order: the role's name, the `AXRole` and `AXSubrole` the
/// bridge serves for the trait `accesskit_ios` 0.2.2 gives its AccessKit role (a button's trait
/// for the button-like roles, none for most, which the bridge serves as a generic element), and
/// where its label goes.
const ROLE_ROWS: &[(&str, &str, Option<&str>, Named)] = &[
    ("button", "AXButton", None, Named::Label),
    ("checkBox", "AXButton", None, Named::Label),
    ("radioButton", "AXButton", None, Named::Label),
    ("switch", "AXButton", None, Named::Label),
    ("link", "AXLink", None, Named::Label),
    ("slider", "AXSlider", None, Named::Label),
    ("spinButton", "AXSlider", None, Named::Label),
    ("comboBox", "AXGenericElement", None, Named::Label),
    ("scrollBar", "AXGenericElement", None, Named::Label),
    ("separator", "AXGenericElement", None, Named::Label),
    ("textInput", "AXGenericElement", None, Named::Label),
    ("multilineTextInput", "AXGenericElement", None, Named::Label),
    ("passwordInput", "AXGenericElement", None, Named::Label),
    (
        "searchInput",
        "AXTextField",
        Some("AXSearchField"),
        Named::Label,
    ),
    ("emailInput", "AXGenericElement", None, Named::Label),
    ("numberInput", "AXGenericElement", None, Named::Label),
    ("phoneNumberInput", "AXGenericElement", None, Named::Label),
    ("urlInput", "AXGenericElement", None, Named::Label),
    ("dateInput", "AXGenericElement", None, Named::Label),
    ("timeInput", "AXGenericElement", None, Named::Label),
    ("listBox", "AXGenericElement", None, Named::Label),
    ("listBoxOption", "AXGenericElement", None, Named::Label),
    ("radioGroup", "AXGenericElement", None, Named::Label),
    ("menu", "AXGenericElement", None, Named::Label),
    ("menuBar", "AXGenericElement", None, Named::Label),
    ("menuItem", "AXGenericElement", None, Named::Label),
    ("menuItemCheckbox", "AXButton", None, Named::Label),
    ("menuItemRadio", "AXButton", None, Named::Label),
    ("tab", "AXButton", None, Named::Label),
    ("tabList", "AXGenericElement", None, Named::Label),
    ("tabPanel", "AXGenericElement", None, Named::Label),
    ("tree", "AXGenericElement", None, Named::Label),
    ("treeItem", "AXGenericElement", None, Named::Label),
    ("treeGrid", "AXGenericElement", None, Named::Label),
    ("grid", "AXGenericElement", None, Named::Label),
    ("gridCell", "AXGenericElement", None, Named::Label),
    ("toolbar", "AXGenericElement", None, Named::Label),
    ("table", "AXGenericElement", None, Named::Label),
    ("row", "AXGenericElement", None, Named::Label),
    ("rowGroup", "AXGenericElement", None, Named::Label),
    ("cell", "AXGenericElement", None, Named::Label),
    ("columnHeader", "AXGenericElement", None, Named::Label),
    ("rowHeader", "AXGenericElement", None, Named::Label),
    ("list", "AXGenericElement", None, Named::Label),
    ("listItem", "AXGenericElement", None, Named::Label),
    ("heading", "AXHeading", None, Named::Label),
    ("paragraph", "AXGenericElement", None, Named::Hint),
    ("image", "AXImage", None, Named::Label),
    ("figure", "AXGenericElement", None, Named::Label),
    ("caption", "AXGenericElement", None, Named::Hint),
    ("article", "AXGenericElement", None, Named::Label),
    ("feed", "AXGenericElement", None, Named::Label),
    ("group", "AXGenericElement", None, Named::Label),
    ("note", "AXGenericElement", None, Named::Label),
    ("blockquote", "AXGenericElement", None, Named::Label),
    ("code", "AXGenericElement", None, Named::Hint),
    ("emphasis", "AXGenericElement", None, Named::Hint),
    ("strong", "AXGenericElement", None, Named::Hint),
    ("insertion", "AXGenericElement", None, Named::Hint),
    ("deletion", "AXGenericElement", None, Named::Hint),
    ("term", "AXGenericElement", None, Named::Label),
    ("definition", "AXGenericElement", None, Named::Label),
    ("time", "AXGenericElement", None, Named::Label),
    ("math", "AXGenericElement", None, Named::Label),
    ("meter", "AXGenericElement", None, Named::Label),
    ("progressBar", "AXGenericElement", None, Named::Label),
    ("tooltip", "AXGenericElement", None, Named::Label),
    ("document", "AXGenericElement", None, Named::Label),
    ("application", "AXGenericElement", None, Named::Label),
    ("banner", "AXGenericElement", None, Named::Label),
    ("main", "AXGenericElement", None, Named::Label),
    ("navigation", "AXGenericElement", None, Named::Label),
    ("complementary", "AXGenericElement", None, Named::Label),
    ("contentInfo", "AXGenericElement", None, Named::Label),
    ("search", "AXGenericElement", None, Named::Label),
    ("form", "AXGenericElement", None, Named::Label),
    ("region", "AXGenericElement", None, Named::Label),
    ("alert", "AXGenericElement", None, Named::Label),
    ("status", "AXGenericElement", None, Named::Label),
    ("log", "AXGenericElement", None, Named::Label),
    ("marquee", "AXGenericElement", None, Named::Label),
    ("timer", "AXGenericElement", None, Named::Label),
    ("dialog", "AXGenericElement", None, Named::Label),
    ("alertDialog", "AXGenericElement", None, Named::Label),
    ("window", "AXGenericElement", None, Named::Label),
    ("pane", "AXGenericElement", None, Named::Label),
    ("scrollView", "AXGenericElement", None, Named::Label),
];

/// The headless scenario app's roles page as the Simulator would serve it, written out: a 40 by
/// 4 point row per role, one below the other, centred in [`PAGE`].
pub(crate) fn roles_page() -> HostScreen {
    let mut top = PAGE.y;
    let mut elements: Vec<ScreenElement> = Vec::new();
    for (name, role, subrole, named) in ROLE_ROWS {
        let mut row = element(role, *subrole, (PAGE.x + 380.0, top, 40.0, 4.0));
        let label = format!("{name} fixture");
        match named {
            Named::Label => row.description = Some(label),
            Named::Hint => row.help = Some(label),
        }
        elements.push(tagged(row, &format!("role.{name}")));
        top += 4.0;
    }
    elements.extend(announcers(PAGE.x, PAGE.y));
    HostScreen::new(PAGE, elements)
}

/// The written-out roles page holds a row for each row of the lane's table, in its order.
#[test]
fn the_roles_page_has_a_row_for_each_role() {
    let names: Vec<&str> = ROLE_ROWS.iter().map(|(name, ..)| *name).collect();
    let roles: Vec<&str> = ROLES.iter().map(|row| row.role.name()).collect();
    assert_eq!(names, roles);
}
