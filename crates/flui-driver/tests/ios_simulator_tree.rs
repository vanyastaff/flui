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
    ScreenSource, WindowIdentity,
};
use flui_driver::lane::ios_simulator::device::{self, Udid};
use flui_driver::lane::ios_simulator::roles::{
    self, ADAPTER_CHECKED_AGAINST, ROLES, RoleTrait, SCREEN_SUBROLE,
};
use flui_driver::lane::ios_simulator::tree::{
    self, BOUNDS_TOLERANCE_POINTS, MAX_SCREEN_DEPTH, Placing, ScreenCandidate, SearchReach,
    TreeError, WindowFacts, WindowTree,
};
use flui_driver::lane::walk::{MAX_DEPTH, MAX_NODES, ProviderError};
use flui_driver::lane::{AccessibilityApi, Lane as _};
use flui_driver::projection::Projection;
use flui_driver::protocol::error::Retry;
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
    request_shared(0)
}

/// [`request`] for a title `others` other listed simulators share.
fn request_shared(others: usize) -> ScreenRequest {
    ScreenRequest::new(TITLE, udid(), others, SIZE, 3.0)
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
    assert_eq!(
        accessibility::screen_title(&simulators, simulator).expect("listed"),
        (TITLE.to_owned(), 0)
    );
}

/// The scripted listing with the iPad Air entry made the target's twin: `iPhone 16` on iOS 18.0,
/// shut down, available as `available` says.
fn with_twin(available: bool) -> String {
    listing("Booted", true).replace(
        r#""isAvailable": true, "name": "iPad Air""#,
        &format!(r#""isAvailable": {available}, "name": "iPhone 16", "state": "Shutdown""#),
    )
}

/// Every other listed simulator of the title counts, available or not, booted or not: a window
/// shown before its runtime went stays on screen. Through the lane, the count reaches the read,
/// and the scripted window is taken by the rule the Mac's read takes one by: refused while it
/// names no UUID, taken once it names the device.
#[test]
fn every_simulator_sharing_the_title_counts() {
    for available in [true, false] {
        let twin = with_twin(available);
        assert_ne!(twin, listing("Booted", true));
        let simulators = device::parse_devices(twin.as_bytes()).expect("a listing");
        let simulator = device::choose(&simulators, &udid()).expect("the named simulator");
        assert_eq!(
            accessibility::screen_title(&simulators, simulator).expect("listed"),
            (TITLE.to_owned(), 1),
            "available: {available}"
        );
    }
    let xcrun = flui_driver::testing::ScriptedXcrun::new().reply(
        "simctl list devices",
        flui_driver::lane::ios_simulator::xcrun::XcrunReply::new(0, with_twin(false)),
    );
    let unnamed = ScriptedSimulatorScreen::new(TITLE, controls_screen(true));
    let (_dir, lane) = connect(&xcrun);
    let mut lane = lane.read_through(unnamed.accessibility());
    let refused = lane
        .read_tree(&placement(), &controls())
        .expect_err("a twin shares the title of a window that names no UUID");
    assert!(matches!(refused, Error::Ambiguous { .. }), "{refused:?}");
    assert_eq!(unnamed.requests(), [request_shared(1)]);

    let named = ScriptedSimulatorScreen::new(TITLE, controls_screen(true)).identity(vec![Some(
        format!("com.apple.CoreSimulator.SimDevice.{UDID}"),
    )]);
    let (_dir, lane) = connect(&xcrun);
    let mut lane = lane.read_through(named.accessibility());
    lane.read_tree(&placement(), &controls())
        .expect("the window names the device");
    assert_eq!(named.requests(), [request_shared(1)]);

    // The scripted window holds the identity's limit too: a text past it names something else.
    let long = ScriptedSimulatorScreen::new(TITLE, controls_screen(true)).identity(vec![Some(
        "x".repeat(accessibility::IDENTITY_TEXT_LIMIT + 1),
    )]);
    let (_dir, lane) = connect(&scripted());
    let mut lane = lane.read_through(long.accessibility());
    let refused = lane
        .read_tree(&placement(), &controls())
        .expect_err("a text past the limit names something else");
    assert!(matches!(refused, Error::NotFound { .. }), "{refused:?}");
}

/// The lane lists the simulators again for each read, so what changed since it connected is
/// seen: the target renamed or removed is refused, and a twin created since makes a window that
/// names no UUID ambiguous.
#[test]
fn each_read_lists_the_simulators_again() {
    let xcrun = scripted();
    let screen = ScriptedSimulatorScreen::new(TITLE, controls_screen(true));
    let (_dir, lane) = connect(&xcrun);
    let mut lane = lane.read_through(screen.accessibility());
    let listings = || {
        xcrun
            .log()
            .iter()
            .filter(|command| command.starts_with("simctl list devices"))
            .count()
    };
    let reply = |listing: String| {
        let _shared = xcrun.clone().reply_once(
            "simctl list devices",
            flui_driver::lane::ios_simulator::xcrun::XcrunReply::new(0, listing),
        );
    };

    let before = listings();
    lane.read_tree(&placement(), &controls()).expect("read");
    assert_eq!(listings(), before + 1, "the read listed the simulators");

    reply(listing("Booted", true).replace(r#""name": "iPhone 16""#, r#""name": "Mine""#));
    let renamed = lane
        .read_tree(&placement(), &controls())
        .expect_err("renamed since the lane connected");
    assert!(
        matches!(&renamed, Error::InvalidArgument { message, .. } if message.contains("Mine")),
        "{renamed:?}"
    );

    reply(listing("Booted", true).replace(UDID, "BBBBBBBB-CCCC-4DDD-8EEE-FFFFFFFFFFFF"));
    let removed = lane
        .read_tree(&placement(), &controls())
        .expect_err("removed since the lane connected");
    assert!(matches!(removed, Error::NotFound { .. }), "{removed:?}");

    reply(with_twin(true));
    let twin = lane
        .read_tree(&placement(), &controls())
        .expect_err("a twin created since the lane connected");
    assert!(matches!(twin, Error::Ambiguous { .. }), "{twin:?}");

    lane.read_tree(&placement(), &controls())
        .expect("the listing is the connected one again");
    assert_eq!(listings(), before + 5, "every read listed the simulators");
}

/// The window is chosen by what each window on screen of the title names, for every host: the
/// one that names the UDID wins over any that name none; two that name it, or two that name no
/// UUID when none names it, are refused; one that names no UUID is refused while a listed twin
/// shares the title; windows that each name another UUID are not found; and with no window of
/// the title on screen, the read says to open one.
#[test]
fn the_window_is_chosen_by_what_each_window_names() {
    use WindowIdentity::{Named, Other, Unknown};
    let ambiguous =
        |found: &flui_driver::Result<usize>| matches!(found, Err(Error::Ambiguous { .. }));
    let rows: &[(&[WindowIdentity], usize, Option<usize>)] = &[
        (&[Named], 0, Some(0)),
        (&[Unknown, Named], 0, Some(1)),
        (&[Named, Unknown], 1, Some(0)),
        (&[Other, Named, Unknown], 1, Some(1)),
        (&[Unknown], 0, Some(0)),
        (&[Other, Unknown], 0, Some(1)),
        (&[Named, Named], 0, None),
        (&[Unknown, Unknown], 0, None),
        (&[Unknown], 1, None),
        (&[Other, Unknown], 2, None),
    ];
    for (identities, others, taken) in rows {
        let found = tree::choose_window(&request_shared(*others), identities);
        match taken {
            Some(index) => assert_eq!(
                found.as_ref().ok(),
                Some(index),
                "{identities:?} with {others} twins: {found:?}"
            ),
            None => assert!(
                ambiguous(&found),
                "{identities:?} with {others} twins: {found:?}"
            ),
        }
    }

    let strangers = tree::choose_window(&request(), &[Other, Other]).expect_err("strangers");
    assert!(
        matches!(&strangers, Error::NotFound { message, .. } if message.contains("UUID")),
        "{strangers:?}"
    );
    let missing = tree::choose_window(&request(), &[]).expect_err("no window");
    assert!(
        matches!(&missing, Error::NotFound { message, .. }
            if message.contains("is on screen") && message.contains(TITLE) && !message.contains("UUID")),
        "{missing:?}"
    );
}

/// The target is found in the listing read now by its UDID: one renamed or removed since the
/// lane connected is refused, never matched by the title it had; one whose runtime went is still
/// the target.
#[test]
fn a_target_renamed_or_removed_since_connecting_is_refused() {
    let connected = device::parse_devices(listing("Booted", true).as_bytes()).expect("a listing");
    let target = device::choose(&connected, &udid())
        .expect("the named simulator")
        .clone();
    let renamed = listing("Booted", true).replace(r#""name": "iPhone 16""#, r#""name": "Mine""#);
    let renamed = device::parse_devices(renamed.as_bytes()).expect("a listing");
    let refused = accessibility::screen_title(&renamed, &target).expect_err("renamed");
    assert!(
        matches!(&refused, Error::InvalidArgument { message, .. } if message.contains("Mine")),
        "{refused:?}"
    );
    let removed = listing("Booted", true).replace(UDID, "BBBBBBBB-CCCC-4DDD-8EEE-FFFFFFFFFFFF");
    let removed = device::parse_devices(removed.as_bytes()).expect("a listing");
    assert!(matches!(
        accessibility::screen_title(&removed, &target),
        Err(Error::NotFound { .. })
    ));
    // A twin renamed to the target's old title does not stand in for it.
    let swapped = listing("Booted", true)
        .replace(r#""name": "iPhone 16""#, r#""name": "Mine""#)
        .replace(
            r#""isAvailable": true, "name": "iPad Air""#,
            r#""isAvailable": true, "name": "iPhone 16", "state": "Booted""#,
        );
    let swapped = device::parse_devices(swapped.as_bytes()).expect("a listing");
    assert!(matches!(
        accessibility::screen_title(&swapped, &target),
        Err(Error::InvalidArgument { .. })
    ));
    let unavailable = listing("Booted", false);
    let unavailable = device::parse_devices(unavailable.as_bytes()).expect("a listing");
    assert_eq!(
        accessibility::screen_title(&unavailable, &target).expect("still the target"),
        (TITLE.to_owned(), 0)
    );
}

/// A window names its simulator by an allow-list: the asked one's UDID makes it the device's;
/// any other UUID, another simulator's from any device set or a document's, makes it not; none
/// leaves the title to stand for it.
#[test]
fn a_window_naming_any_other_uuid_is_not_the_devices() {
    let ours = Some(format!("com.apple.CoreSimulator.SimDevice.{UDID}"));
    let other = Some("AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE".to_owned());
    let stranger = Some("file:///tmp/0F1E2D3C-4B5A-4697-8877-665544332211.session".to_owned());
    assert_eq!(
        accessibility::window_identity(&udid(), &[None, ours]),
        WindowIdentity::Named
    );
    assert_eq!(
        accessibility::window_identity(&udid(), &[other.clone(), None]),
        WindowIdentity::Other
    );
    assert_eq!(
        accessibility::window_identity(&udid(), &[stranger, None]),
        WindowIdentity::Other,
        "a UUID nobody checked is not the device's"
    );
    assert_eq!(
        accessibility::window_identity(&udid(), &[Some("iPhone 16".to_owned()), None]),
        WindowIdentity::Unknown
    );
    assert_eq!(
        accessibility::window_identity(&udid(), &[other, Some(UDID.to_ascii_lowercase())]),
        WindowIdentity::Named
    );

    // A run of the UUID's form is a UUID whatever stands beside it, a hexadecimal letter or
    // digit included: whatever the attribute's form turns out to be, another simulator's is never
    // read as none.
    let stranger = "0F1E2D3C-4B5A-4697-8877-665544332211";
    for form in [
        "device-{}",
        "SimulatorWindow-{}",
        "{}-0",
        "{}_main",
        "window{}",
        "SimDevice{}",
        "Device{}",
        "{}Device",
        "{}abc",
        "{}data",
        "A{}",
        "{}0",
    ] {
        assert_eq!(
            accessibility::window_identity(&udid(), &[Some(form.replace("{}", UDID))]),
            WindowIdentity::Named,
            "{form}"
        );
        assert_eq!(
            accessibility::window_identity(&udid(), &[Some(form.replace("{}", stranger))]),
            WindowIdentity::Other,
            "{form}"
        );
    }
    // A text past the limit is not read and names something else, the device's UDID in it
    // included; another text of the window that names the device still makes it the device's.
    let long = |udid: &str| {
        Some(format!(
            "{udid}{}",
            "x".repeat(accessibility::IDENTITY_TEXT_LIMIT)
        ))
    };
    assert_eq!(
        accessibility::window_identity(&udid(), &[long(UDID), None]),
        WindowIdentity::Other
    );
    assert_eq!(
        accessibility::window_identity(&udid(), &[Some(UDID.to_owned()), long(UDID)]),
        WindowIdentity::Named
    );
    let at_limit = Some("x".repeat(accessibility::IDENTITY_TEXT_LIMIT - UDID.len()) + UDID);
    assert_eq!(
        accessibility::window_identity(&udid(), &[at_limit]),
        WindowIdentity::Named,
        "a text of exactly the limit is read whole"
    );

    // Text with no run of the form names none: a UUID's groups cut short, or without dashes.
    for none in [
        UDID.replace('-', ""),
        UDID[..35].to_owned(),
        UDID.replacen('-', "_", 1),
        UDID.replacen('F', "G", 1),
    ] {
        assert_eq!(
            accessibility::window_identity(&udid(), &[Some(none.clone())]),
            WindowIdentity::Unknown,
            "{none}"
        );
    }
}

/// One node of a screen a test reads as a source.
struct Item {
    element: ScreenElement,
    children: Vec<usize>,
    /// The index from which a child read answers that the node went away.
    gone_after: Option<usize>,
    /// Whether a read of the node's own attributes answers that it went away.
    gone: bool,
    /// Whether a child read of the node does not answer in time.
    hung: bool,
}

impl Item {
    fn new(element: ScreenElement) -> Self {
        Self {
            element,
            children: Vec::new(),
            gone_after: None,
            gone: false,
            hung: false,
        }
    }
}

/// A screen of a test's, read as a source: its nodes by index, the screen first.
struct Source {
    nodes: Vec<Item>,
}

impl Source {
    /// Add a node of `role` under `parent`; its index.
    fn add(&mut self, parent: usize, role: &str) -> usize {
        let index = self.nodes.len();
        self.nodes.push(Item::new(ScreenElement::new(role)));
        self.nodes[parent].children.push(index);
        index
    }
}

impl ScreenSource for Source {
    type Element = usize;

    fn child_at(&mut self, parent: &usize, index: usize) -> Result<Option<usize>, ProviderError> {
        let item = &self.nodes[*parent];
        if item.hung {
            return Err(ProviderError::Timeout);
        }
        if item.gone_after.is_some_and(|after| index >= after) {
            return Err(ProviderError::Gone);
        }
        Ok(item.children.get(index).copied())
    }

    fn element(&mut self, element: &usize) -> Result<ScreenElement, ProviderError> {
        let item = &self.nodes[*element];
        if item.gone {
            return Err(ProviderError::Gone);
        }
        Ok(item.element.clone())
    }
}

/// A screen of a group holding the controls' elements and a list of `items` items.
fn source(items: usize) -> Source {
    let mut source = Source {
        nodes: vec![Item::new(ScreenElement::new("AXGroup"))],
    };
    let like = source.add(0, "AXButton");
    source.nodes[like].element = labelled(
        element("AXButton", None, (258.0, 239.0, 60.0, 33.0)),
        "Like",
    );
    let list = source.add(0, "AXGroup");
    for _ in 0..items {
        source.add(list, "AXGenericElement");
    }
    source
}

/// A screen holding a chain of `levels` elements, each the only child of the one before; the
/// deepest one's index.
fn chain(levels: usize) -> (Source, usize) {
    let mut source = Source {
        nodes: vec![Item::new(ScreenElement::new("AXGroup"))],
    };
    let mut parent = 0;
    for _ in 0..levels {
        parent = source.add(parent, "AXGroup");
    }
    (source, parent)
}

/// A list of children ends whole when the parent has no more, and is cut only when one is there
/// past the bound or the parent went away: a parent of exactly the bound's children is whole,
/// and a bound of zero asks only whether a first child is there.
#[test]
fn a_child_list_is_cut_only_past_its_bound_or_by_its_parent_gone() {
    use accessibility::{ChildList, ListCut, list_children};
    let parent = |count: usize, gone_at: Option<usize>| {
        move |index: usize| {
            if gone_at == Some(index) {
                Err(ProviderError::Gone)
            } else {
                Ok((index < count).then_some(index))
            }
        }
    };
    let listed = |children: &[usize], cut: Option<ListCut>| {
        Ok(ChildList {
            children: children.to_vec(),
            cut,
        })
    };
    let bound = Some(ListCut::Bound);
    assert_eq!(list_children(parent(3, None), 3), listed(&[0, 1, 2], None));
    assert_eq!(list_children(parent(4, None), 3), listed(&[0, 1, 2], bound));
    assert_eq!(list_children(parent(0, None), 0), listed(&[], None));
    assert_eq!(list_children(parent(1, None), 0), listed(&[], bound));
    assert_eq!(
        list_children(parent(3, Some(1)), 3),
        listed(&[0], Some(ListCut::ParentGone))
    );
    assert_eq!(
        list_children(|_| Err::<Option<usize>, _>(ProviderError::Timeout), 3),
        Err(ProviderError::Timeout)
    );
}

/// How many levels of `elements` a walk read, along each one's first child.
fn levels_read(elements: &[accessibility::Walked<usize>]) -> usize {
    let mut levels = 0;
    let mut level = elements;
    while let Some(first) = level.first() {
        levels += 1;
        level = &first.children;
    }
    levels
}

/// The walk reads [`MAX_DEPTH`] levels: the deepest level read has its children marked unread
/// when one is there or the element went away before it said, and a probe that does not answer
/// fails the walk, as every other read of it does.
#[test]
fn the_walk_reads_its_depth_and_marks_what_lies_past_it() {
    let (mut whole, _) = chain(MAX_DEPTH);
    let walked = accessibility::walk_screen(&mut whole, &0).expect("walked");
    assert_eq!(levels_read(&walked.elements), MAX_DEPTH);
    assert!(!walked.truncated, "nothing lies past the depth");

    let (mut deeper, deepest) = chain(MAX_DEPTH + 1);
    let walked = accessibility::walk_screen(&mut deeper, &0).expect("walked");
    assert_eq!(levels_read(&walked.elements), MAX_DEPTH);
    assert!(walked.truncated, "a level lies past the depth");
    let mut level = &walked.elements;
    let mut last = None;
    while let Some(first) = level.first() {
        last = Some(first);
        level = &first.children;
    }
    let last = last.expect("a level was read");
    assert_eq!(last.handle, deepest - 1);
    assert!(last.element.children_unread);

    let (mut gone, deepest) = chain(MAX_DEPTH);
    gone.nodes[deepest].gone_after = Some(0);
    let walked = accessibility::walk_screen(&mut gone, &0).expect("walked");
    assert!(
        walked.truncated,
        "an element gone before it said whether it has a child leaves its children unread"
    );

    let (mut hung, deepest) = chain(MAX_DEPTH + 1);
    hung.nodes[deepest - 1].hung = true;
    assert_eq!(
        accessibility::walk_screen(&mut hung, &0).map(|walk| walk.truncated),
        Err(ProviderError::Timeout),
        "the probe past the depth did not answer"
    );
}

/// The walk reads at most [`MAX_NODES`] elements, every one of a screen that has that many, and
/// leaves out an element that went away while it was read, reading its siblings on.
#[test]
fn the_walk_reads_its_budget_and_leaves_out_a_gone_element() {
    let flat = |count: usize| {
        let mut source = Source {
            nodes: vec![Item::new(ScreenElement::new("AXGroup"))],
        };
        for _ in 0..count {
            source.add(0, "AXGenericElement");
        }
        source
    };
    let walked = accessibility::walk_screen(&mut flat(MAX_NODES), &0).expect("walked");
    assert_eq!(walked.elements.len(), MAX_NODES);
    assert!(!walked.truncated && !walked.children_unread);

    let walked = accessibility::walk_screen(&mut flat(MAX_NODES + 1), &0).expect("walked");
    assert_eq!(walked.elements.len(), MAX_NODES);
    assert!(walked.truncated && walked.children_unread);

    // Two wide levels share one budget: no level is past it, their sum is.
    let mut wide = Source {
        nodes: vec![Item::new(ScreenElement::new("AXGroup"))],
    };
    for _ in 0..80 {
        let pane = wide.add(0, "AXGroup");
        for _ in 0..80 {
            wide.add(pane, "AXGenericElement");
        }
    }
    let walked = accessibility::walk_screen(&mut wide, &0).expect("walked");
    fn count(elements: &[accessibility::Walked<usize>]) -> usize {
        elements
            .iter()
            .map(|element| 1 + count(&element.children))
            .sum()
    }
    assert_eq!(count(&walked.elements), MAX_NODES, "the budget, in all");
    assert!(walked.truncated);

    let mut gone = source(3);
    let list = gone.nodes[0].children[1];
    let first = gone.nodes[list].children[0];
    gone.nodes[first].gone = true;
    let walked = accessibility::walk_screen(&mut gone, &0).expect("walked");
    let items = &walked.elements[1];
    assert_eq!(
        items
            .children
            .iter()
            .map(|item| item.handle)
            .collect::<Vec<usize>>(),
        gone.nodes[list].children[1..]
    );
    assert!(!items.element.children_unread && !walked.truncated);
}

/// The walk marks a level read short the same way whatever stopped it: a parent gone mid-read
/// or the node budget leave its children unread and the walk truncated; the screen's own
/// children left unread reach the projection's root.
#[test]
fn a_level_read_short_is_marked_whatever_stopped_it() {
    let whole = accessibility::walk_screen(&mut source(3), &0).expect("walked");
    assert!(!whole.truncated && !whole.children_unread);
    assert_eq!(whole.elements[1].children.len(), 3);
    assert!(!whole.elements[1].element.children_unread);

    let mut gone = source(3);
    gone.nodes[2].gone_after = Some(1);
    let walked = accessibility::walk_screen(&mut gone, &0).expect("walked");
    assert!(walked.truncated);
    assert_eq!(walked.elements[1].children.len(), 1);
    assert!(
        walked.elements[1].element.children_unread,
        "the list went away"
    );
    assert!(!walked.children_unread);

    let budget = accessibility::walk_screen(&mut source(MAX_NODES + 5), &0).expect("walked");
    assert!(budget.truncated);
    assert!(
        budget.elements[1].element.children_unread,
        "the list the budget cut is marked as a vanished one is"
    );

    let mut screen_gone = source(0);
    screen_gone.nodes[0].gone_after = Some(1);
    let walked = accessibility::walk_screen(&mut screen_gone, &0).expect("walked");
    assert!(walked.children_unread && walked.truncated);
    let screen = walked.screen(SCREEN);
    let mut app = controls();
    app.roots[0].children.truncate(2);
    let projected = tree::read(&screen, &app, placing()).expect("read");
    assert!(
        projected.roots[0].children_unread,
        "the root says its children went unread"
    );
    assert!(
        projected.truncated,
        "the projection says the read was short"
    );

    // A root list read short ends where the read stopped: its last elements are kept even in
    // an announcer's shape, since the announcers at the list's real end were never read.
    let whole = tree::read(&controls_screen(true), &controls(), placing()).expect("read");
    let mut cut = controls_screen(true);
    cut.children_unread = true;
    cut.truncated = true;
    let projected = tree::read(&cut, &controls(), placing()).expect("read");
    assert_eq!(
        projected.roots[0].children.len(),
        whole.roots[0].children.len() + 2,
        "the last two elements read are kept"
    );
    assert!(projected.roots[0].children_unread);
    assert!(projected.truncated);
    assert!(!whole.truncated);
}

/// What a node of a test's window does by the time its frame is read again, after the walk.
#[derive(Debug, Clone, Copy)]
enum Later {
    /// It moves to this frame.
    Moves(PointFrame),
    /// It goes away.
    Goes,
}

/// One node of a window a test builds.
#[derive(Debug, Clone)]
struct Fixture {
    facts: WindowFacts,
    identity: Vec<Option<String>>,
    children: Vec<usize>,
    /// The index from which a child read answers that the node went away.
    gone_after: Option<usize>,
    /// What a read of the node's facts answers instead of them: gone after it was listed, a
    /// timeout, a failure.
    facts_error: Option<ProviderError>,
    /// What a read of the node's frame alone answers, once the walk is done.
    later: Option<Later>,
}

/// A set of windows written out as nodes by index, each window a root.
struct Windows {
    nodes: Vec<Fixture>,
    /// How many elements' facts the search read.
    facts_read: usize,
    /// Whether the process answers as no trusted accessibility client: every failure is then
    /// the permission taken back.
    untrusted: bool,
}

impl Windows {
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            facts_read: 0,
            untrusted: false,
        }
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
            gone_after: None,
            facts_error: None,
            later: None,
        });
        if let Some(parent) = parent {
            self.nodes[parent].children.push(index);
        }
        index
    }
}

impl WindowTree for Windows {
    type Element = usize;

    fn child_at(&mut self, element: &usize, index: usize) -> Result<Option<usize>, ProviderError> {
        let node = &self.nodes[*element];
        if node.gone_after.is_some_and(|after| index >= after) {
            return Err(ProviderError::Gone);
        }
        Ok(node.children.get(index).copied())
    }

    fn failure(&mut self, error: ProviderError) -> Error {
        accessibility::read_failure(error, !self.untrusted, None)
    }

    fn frame(&mut self, element: &usize) -> Result<Option<PointFrame>, ProviderError> {
        let node = &self.nodes[*element];
        match node.later {
            Some(Later::Moves(frame)) => Ok(Some(frame)),
            Some(Later::Goes) => Err(ProviderError::Gone),
            None => Ok(node.facts.frame),
        }
    }

    fn facts(&mut self, element: &usize) -> Result<WindowFacts, ProviderError> {
        self.facts_read += 1;
        let node = &self.nodes[*element];
        if let Some(error) = &node.facts_error {
            return Err(error.clone());
        }
        Ok(node.facts.clone())
    }

    fn identity(&mut self, window: &usize) -> flui_driver::Result<Vec<Option<String>>> {
        Ok(self.nodes[*window].identity.clone())
    }
}

/// The same window as the screen's walk reads it: each node an element of its role, subrole
/// and frame.
impl ScreenSource for Windows {
    type Element = usize;

    fn child_at(&mut self, parent: &usize, index: usize) -> Result<Option<usize>, ProviderError> {
        WindowTree::child_at(self, parent, index)
    }

    fn element(&mut self, element: &usize) -> Result<ScreenElement, ProviderError> {
        let facts = &self.nodes[*element].facts;
        let mut read = ScreenElement::new(facts.role.clone());
        read.subrole.clone_from(&facts.subrole);
        read.frame = facts.frame;
        Ok(read)
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

/// A group that goes away while the search lists its children leaves the search cut short, which
/// a failure to find the screen names, never a whole window that shows no screen.
#[test]
fn a_group_gone_mid_search_cuts_the_search_short() {
    let mut windows = Windows::new();
    let window = windows.add(None, "AXWindow", None, Some((246.0, 185.0, 1100.0, 800.0)));
    let pane = windows.add(
        Some(window),
        "AXGroup",
        None,
        Some((466.0, 185.0, 660.0, 800.0)),
    );
    windows.add(
        Some(pane),
        "AXGroup",
        Some(SCREEN_SUBROLE),
        Some((657.0, 266.0, 393.0 * 0.74, 852.0 * 0.74)),
    );
    windows.nodes[pane].gone_after = Some(0);
    let refused = tree::find_screen(&mut windows, &request(), vec![window]).expect_err("cut");
    assert!(
        matches!(&refused, Error::NotFound { message, .. } if message.contains("went away")),
        "{refused:?}"
    );

    // A plain group of the window's shape seen by a cut search is not taken: the marked screen
    // may sit in the part not read.
    let drawn = (657.0, 266.0, 393.0 * 0.74, 852.0 * 0.74);
    let wrapper = windows.add(Some(window), "AXGroup", None, Some(drawn));
    let refused =
        tree::find_screen(&mut windows, &request(), vec![window]).expect_err("cut, plain group");
    assert!(
        matches!(&refused, Error::NotFound { message, .. } if message.contains("read again")),
        "{refused:?}"
    );
    // Read whole, the same plain group is the screen.
    windows.nodes[pane].gone_after = None;
    windows.nodes[pane].children.clear();
    let (found, _) = tree::find_screen(&mut windows, &request(), vec![window]).expect("whole");
    assert_eq!(found, wrapper);
}

/// The search lists a group's children through the shared bounded listing: a window of exactly
/// the budget's children is read whole, one more is cut, and groups past the deepest level left
/// unentered do not stop a plain group of the window's shape being taken.
#[test]
fn the_search_keeps_to_its_budget_and_depth() {
    let flat = |count: usize| {
        let mut windows = Windows::new();
        let window = windows.add(None, "AXWindow", None, Some((0.0, 0.0, 1100.0, 800.0)));
        for _ in 0..count {
            windows.add(Some(window), "AXButton", None, Some((0.0, 0.0, 10.0, 10.0)));
        }
        (windows, window)
    };
    let (mut whole, window) = flat(MAX_NODES);
    let refused = tree::find_screen(&mut whole, &request(), vec![window]).expect_err("no screen");
    assert!(
        matches!(&refused, Error::NotFound { message, .. }
            if message.contains("no group") && !message.contains("budget")),
        "exactly the budget is read whole: {refused:?}"
    );
    let (mut more, window) = flat(MAX_NODES + 1);
    let refused = tree::find_screen(&mut more, &request(), vec![window]).expect_err("no screen");
    assert!(
        matches!(&refused, Error::NotFound { message, .. } if message.contains("budget")),
        "one past the budget is cut: {refused:?}"
    );

    let mut deep = Windows::new();
    let window = deep.add(None, "AXWindow", None, Some((231.0, 142.0, 456.0, 972.0)));
    let screen = deep.add(
        Some(window),
        "AXGroup",
        None,
        Some((258.0, 222.0, 393.0, 852.0)),
    );
    let mut pane = deep.add(
        Some(window),
        "AXGroup",
        None,
        Some((231.0, 142.0, 10.0, 10.0)),
    );
    for _ in 0..MAX_SCREEN_DEPTH {
        pane = deep.add(
            Some(pane),
            "AXGroup",
            None,
            Some((231.0, 142.0, 10.0, 10.0)),
        );
    }
    let (found, _) = tree::find_screen(&mut deep, &request(), vec![window]).expect("to depth");
    assert_eq!(found, screen);

    // A cut beside the candidate stays a cut when the depth also stops the search.
    let beside = deep.add(
        Some(window),
        "AXGroup",
        None,
        Some((231.0, 142.0, 20.0, 20.0)),
    );
    deep.add(
        Some(beside),
        "AXButton",
        None,
        Some((231.0, 142.0, 5.0, 5.0)),
    );
    deep.nodes[beside].gone_after = Some(0);
    let refused = tree::find_screen(&mut deep, &request(), vec![window]).expect_err("cut");
    assert!(
        matches!(&refused, Error::NotFound { message, .. } if message.contains("read again")),
        "{refused:?}"
    );

    // With no screen, a search read to its depth says how far it read.
    deep.nodes[beside].gone_after = None;
    deep.nodes[window].children.retain(|child| *child != screen);
    let refused = tree::find_screen(&mut deep, &request(), vec![window]).expect_err("no screen");
    assert!(
        matches!(&refused, Error::NotFound { message, .. }
            if message.contains(&format!("within the {MAX_SCREEN_DEPTH} levels searched"))),
        "{refused:?}"
    );
}

/// Only a cut of the chrome refuses a plain screen: under Simulator.app's plain screen group the
/// app's elements may spend the budget or go away, and the screen is still taken; a cut above or
/// beside it (an element gone while listed, or between its listing and its facts) refuses, to
/// be read again; and the chrome's budget is shared across its levels, a cut by it refused with
/// no retry, since the chrome is the same at every read.
#[test]
fn the_search_refuses_only_a_cut_in_the_chrome() {
    let simulator_app = || {
        let mut windows = Windows::new();
        let window = windows.add(None, "AXWindow", None, Some((231.0, 142.0, 456.0, 972.0)));
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
        (windows, window, bezel, screen)
    };

    // The app's elements, past the budget, with one gone mid-listing: the screen's own business.
    let (mut windows, window, _, screen) = simulator_app();
    let list = windows.add(
        Some(screen),
        "AXGroup",
        None,
        Some((258.0, 222.0, 393.0, 400.0)),
    );
    for _ in 0..MAX_NODES + 10 {
        windows.add(
            Some(list),
            "AXButton",
            None,
            Some((258.0, 222.0, 40.0, 40.0)),
        );
    }
    windows.nodes[list].gone_after = Some(5);
    let (found, _) = tree::find_screen(&mut windows, &request(), vec![window]).expect("the screen");
    assert_eq!(found, screen);

    // A cut above the candidate: the window's list ends early after the bezel.
    let (mut windows, window, _, _) = simulator_app();
    windows.add(
        Some(window),
        "AXToolbar",
        None,
        Some((231.0, 142.0, 456.0, 52.0)),
    );
    windows.nodes[window].gone_after = Some(1);
    let refused = tree::find_screen(&mut windows, &request(), vec![window]).expect_err("above");
    assert!(
        matches!(&refused, Error::NotFound { message, .. } if message.contains("read again")),
        "{refused:?}"
    );

    // A cut beside it: a sibling of the screen gone between its listing and its facts.
    let (mut windows, window, bezel, _) = simulator_app();
    let button = windows.add(
        Some(bezel),
        "AXButton",
        None,
        Some((231.0, 300.0, 4.0, 40.0)),
    );
    windows.nodes[button].facts_error = Some(ProviderError::Gone);
    let refused = tree::find_screen(&mut windows, &request(), vec![window]).expect_err("beside");
    assert!(
        matches!(&refused, Error::NotFound { message, .. } if message.contains("read again")),
        "{refused:?}"
    );

    // Two wide levels of chrome share one budget: the search reads at most its budget in all.
    let mut wide = Windows::new();
    let window = wide.add(None, "AXWindow", None, Some((0.0, 0.0, 1100.0, 800.0)));
    for _ in 0..80 {
        let pane = wide.add(Some(window), "AXGroup", None, Some((0.0, 0.0, 30.0, 30.0)));
        for _ in 0..80 {
            wide.add(Some(pane), "AXButton", None, Some((0.0, 0.0, 10.0, 10.0)));
        }
    }
    let refused = tree::find_screen(&mut wide, &request(), vec![window]).expect_err("no screen");
    assert!(
        matches!(&refused, Error::NotFound { message, retry, .. }
            if message.contains("budget") && *retry == Retry::Never),
        "{refused:?}"
    );
    assert!(
        wide.facts_read <= MAX_NODES,
        "{} facts read past the budget",
        wide.facts_read
    );

    // The same chrome with a plain screen beside it: refused, and no retry is offered.
    let screen = wide.add(
        Some(window),
        "AXGroup",
        None,
        Some((258.0, 222.0, 393.0, 852.0)),
    );
    wide.nodes[window].children.rotate_right(1);
    assert_eq!(wide.nodes[window].children[0], screen);
    let refused = tree::find_screen(&mut wide, &request(), vec![window]).expect_err("cut");
    assert!(
        matches!(&refused, Error::NotFound { message, retry, .. }
            if message.contains("budget") && *retry == Retry::Never),
        "{refused:?}"
    );
}

/// The window, its chrome and its screen candidates in every shape a plain group can take, as no
/// run has recorded either window's tree yet: a `SCREEN_SUBROLE` group is taken wherever it is,
/// a plain wrapper of its own shape around it included and whatever cut the search; one plain
/// group is taken, even when the search under it was cut; a cut beside it refuses; two marked
/// screens of two frames are ambiguous.
#[test]
fn the_screen_is_chosen_in_every_shape_its_group_can_take() {
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Expect {
        Takes(&'static str),
        Cut,
        Ambiguous,
    }
    const DRAWN: (f64, f64, f64, f64) = (657.0, 266.0, 393.0 * 0.74, 852.0 * 0.74);
    /// A window with a sidebar, and the screen's own shapes under a pane; the node named
    /// `"screen"` is the one taken. Returns the windows, the window, and the named nodes.
    fn build(shape: &str) -> (Windows, usize, Vec<(&'static str, usize)>) {
        let mut windows = Windows::new();
        let window = windows.add(None, "AXWindow", None, Some((246.0, 185.0, 1100.0, 800.0)));
        windows.add(
            Some(window),
            "AXGroup",
            None,
            Some((246.0, 185.0, 220.0, 800.0)),
        );
        let pane = windows.add(
            Some(window),
            "AXGroup",
            None,
            Some((466.0, 185.0, 660.0, 800.0)),
        );
        let mut named = Vec::new();
        match shape {
            "wrapped marked" => {
                let wrapper = windows.add(Some(pane), "AXGroup", None, Some(DRAWN));
                let marked =
                    windows.add(Some(wrapper), "AXGroup", Some(SCREEN_SUBROLE), Some(DRAWN));
                named.push(("screen", marked));
            }
            "plain only" => {
                let plain = windows.add(Some(pane), "AXGroup", None, Some(DRAWN));
                named.push(("screen", plain));
            }
            "plain over an app past the budget" => {
                let plain = windows.add(Some(pane), "AXGroup", None, Some(DRAWN));
                let list = windows.add(
                    Some(plain),
                    "AXGroup",
                    None,
                    Some((657.0, 266.0, 290.0, 300.0)),
                );
                for _ in 0..MAX_NODES + 10 {
                    windows.add(
                        Some(list),
                        "AXButton",
                        None,
                        Some((657.0, 266.0, 40.0, 40.0)),
                    );
                }
                let gone = windows.add(
                    Some(plain),
                    "AXGroup",
                    None,
                    Some((657.0, 600.0, 290.0, 30.0)),
                );
                windows.nodes[gone].facts_error = Some(ProviderError::Gone);
                named.push(("screen", plain));
            }
            "cut beside" => {
                windows.add(Some(pane), "AXGroup", None, Some(DRAWN));
                let beside = windows.add(
                    Some(window),
                    "AXGroup",
                    None,
                    Some((1126.0, 185.0, 220.0, 800.0)),
                );
                windows.add(
                    Some(beside),
                    "AXButton",
                    None,
                    Some((1126.0, 185.0, 20.0, 20.0)),
                );
                windows.nodes[beside].gone_after = Some(0);
            }
            "marked beside a cut" => {
                let marked = windows.add(Some(pane), "AXGroup", Some(SCREEN_SUBROLE), Some(DRAWN));
                let beside = windows.add(
                    Some(window),
                    "AXGroup",
                    None,
                    Some((1126.0, 185.0, 220.0, 800.0)),
                );
                windows.nodes[beside].facts_error = Some(ProviderError::Gone);
                named.push(("screen", marked));
            }
            "two marked" => {
                windows.add(Some(pane), "AXGroup", Some(SCREEN_SUBROLE), Some(DRAWN));
                windows.add(
                    Some(pane),
                    "AXGroup",
                    Some(SCREEN_SUBROLE),
                    Some((300.0, 266.0, 393.0 * 0.74, 852.0 * 0.74)),
                );
            }
            other => unreachable!("no shape {other}"),
        }
        (windows, window, named)
    }
    let rows = [
        ("wrapped marked", Expect::Takes("screen")),
        ("plain only", Expect::Takes("screen")),
        ("plain over an app past the budget", Expect::Takes("screen")),
        ("cut beside", Expect::Cut),
        ("marked beside a cut", Expect::Takes("screen")),
        ("two marked", Expect::Ambiguous),
    ];
    for (shape, expect) in rows {
        let (mut windows, window, named) = build(shape);
        let found = tree::find_screen(&mut windows, &request(), vec![window]);
        match expect {
            Expect::Takes(name) => {
                let wanted = named
                    .iter()
                    .find(|(label, _)| *label == name)
                    .map(|(_, node)| *node);
                assert_eq!(
                    found.as_ref().ok().map(|(node, _)| *node),
                    wanted,
                    "{shape}: {found:?}"
                );
            }
            Expect::Cut => assert!(
                matches!(&found, Err(Error::NotFound { message, .. }) if message.contains("read again")),
                "{shape}: {found:?}"
            ),
            Expect::Ambiguous => assert!(
                matches!(&found, Err(Error::Ambiguous { .. })),
                "{shape}: {found:?}"
            ),
        }
    }

    // A marked screen found while the budget cut the chrome is still the screen.
    let (mut windows, window, named) = build("marked beside a cut");
    let beside = windows.nodes[window].children[2];
    windows.nodes[beside].facts_error = None;
    for _ in 0..MAX_NODES {
        windows.add(
            Some(beside),
            "AXButton",
            None,
            Some((1126.0, 185.0, 10.0, 10.0)),
        );
    }
    let (found, _) = tree::find_screen(&mut windows, &request(), vec![window]).expect("marked");
    assert_eq!(Some(found), named.first().map(|(_, node)| *node));
}

/// A facts read that does not answer, or fails, fails the search with what it answered: never a
/// cut the search reads again past.
#[test]
fn a_facts_read_that_fails_fails_the_search() {
    for (error, timed_out) in [
        (ProviderError::Timeout, true),
        (ProviderError::Failed("-25200".to_owned()), false),
    ] {
        let mut windows = Windows::new();
        let window = windows.add(None, "AXWindow", None, Some((231.0, 142.0, 456.0, 972.0)));
        let bezel = windows.add(
            Some(window),
            "AXGroup",
            None,
            Some((231.0, 194.0, 456.0, 920.0)),
        );
        windows.add(
            Some(bezel),
            "AXGroup",
            None,
            Some((258.0, 222.0, 393.0, 852.0)),
        );
        let button = windows.add(
            Some(bezel),
            "AXButton",
            None,
            Some((231.0, 300.0, 4.0, 40.0)),
        );
        windows.nodes[button].facts_error = Some(error.clone());
        let failed = tree::find_screen(&mut windows, &request(), vec![window]).expect_err("fails");
        if timed_out {
            assert!(
                matches!(failed, Error::Timeout { .. }),
                "{error:?}: {failed:?}"
            );
        } else {
            assert!(
                matches!(failed, Error::Platform { .. }),
                "{error:?}: {failed:?}"
            );
        }
    }
}

/// The read is the search, the walk of the screen's elements and the screen's frame read again
/// after it, the Mac's and this test tree's: a screen moved, resized or gone meanwhile answers
/// busy, to be read again, never the window gone; gone while the process is no longer trusted,
/// it is the permission taken back.
#[test]
fn a_screen_moved_or_gone_during_the_read_is_read_again() {
    let simulator_app = || {
        let mut windows = Windows::new();
        let window = windows.add(None, "AXWindow", None, Some((231.0, 142.0, 456.0, 972.0)));
        let screen = windows.add(
            Some(window),
            "AXGroup",
            None,
            Some((258.0, 222.0, 393.0, 852.0)),
        );
        windows.add(
            Some(screen),
            "AXButton",
            None,
            Some((258.0, 239.0, 60.0, 33.0)),
        );
        (windows, window, screen)
    };
    let (mut windows, window, _) = simulator_app();
    let (read, walk) = tree::read_screen(&mut windows, &request(), vec![window]).expect("steady");
    assert_eq!(read.frame, SCREEN);
    assert_eq!(read.elements.len(), 1);
    assert_eq!(walk.elements.len(), 1);

    let moved = PointFrame::new(SCREEN.x + 40.0, SCREEN.y, SCREEN.width, SCREEN.height);
    for later in [Later::Moves(moved), Later::Goes] {
        let (mut windows, window, screen) = simulator_app();
        windows.nodes[screen].later = Some(later);
        let refused = tree::read_screen(&mut windows, &request(), vec![window]).expect_err("busy");
        assert!(
            matches!(&refused, Error::Busy { message, .. } if message.contains("read again")),
            "{later:?}: {refused:?}"
        );
    }

    // A screen gone while the process is no longer trusted is the permission taken back, which
    // no read again serves.
    let (mut windows, window, screen) = simulator_app();
    windows.nodes[screen].later = Some(Later::Goes);
    windows.untrusted = true;
    let refused = tree::read_screen(&mut windows, &request(), vec![window]).expect_err("refused");
    assert_eq!(
        refused.missing_capability().as_deref(),
        Some("macos_permission_accessibility"),
        "{refused:?}"
    );
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
    // An app element of the screen's own frame, deeper: one screen reached twice.
    let content = windows.add(
        Some(screen),
        "AXGroup",
        None,
        Some((258.0, 222.0, 393.0, 852.0)),
    );
    // The app's scroll content, of the window's shape at half its size and elsewhere, deeper
    // still: the screen is the shallowest group of the shape, not one of two frames to refuse.
    windows.add(
        Some(content),
        "AXGroup",
        None,
        Some((308.0, 322.0, 196.5, 426.0)),
    );
    let (found, frame) =
        tree::find_screen(&mut windows, &request(), vec![window]).expect("the screen is found");
    assert_eq!(found, screen);
    assert_eq!(frame, SCREEN);
}

/// The window search takes its window by the allow-list: two that name no UUID are refused; one
/// that names any other UUID is left out, and when every one does the read says why; one that
/// names no UUID is refused while another listed simulator shares the title, and one that names
/// the device's UDID is taken even then and beside one that names none; two that name it are
/// refused; none is not found.
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

    // Any UUID that is not the device's leaves the window out, whatever lists it.
    windows.nodes[second].identity = vec![Some(
        "file:///tmp/0F1E2D3C-4B5A-4697-8877-665544332211.session".to_owned(),
    )];
    let (found, _) = tree::find_screen(&mut windows, &request(), vec![first, second])
        .expect("the window naming another UUID is left out");
    assert_eq!(windows.nodes[first].children, [found]);
    let refused = tree::find_screen(&mut windows, &request(), vec![second])
        .expect_err("every window names another UUID");
    assert!(
        matches!(&refused, Error::NotFound { message, .. } if message.contains("UUID")),
        "{refused:?}"
    );

    // A window that names no UUID could be a twin's: refused while one shares the title.
    let refused = tree::find_screen(&mut windows, &request_shared(1), vec![first])
        .expect_err("a twin shares the title");
    assert!(matches!(refused, Error::Ambiguous { .. }), "{refused:?}");
    windows.nodes[first].identity = vec![Some(UDID.to_owned())];
    let (found, _) = tree::find_screen(&mut windows, &request_shared(1), vec![first, second])
        .expect("the window naming the device is taken");
    assert_eq!(windows.nodes[first].children, [found]);

    // The window that names the device wins over one that names none.
    windows.nodes[second].identity = Vec::new();
    let (found, _) = tree::find_screen(&mut windows, &request(), vec![second, first])
        .expect("the window naming the device beside one naming none");
    assert_eq!(windows.nodes[first].children, [found]);

    // A window of the title whose text runs past the limit names something else, never none:
    // beside a window that names no UUID it is left out, and that one is taken, where two that
    // name none would be refused.
    let named = std::mem::take(&mut windows.nodes[first].identity);
    windows.nodes[second].identity = vec![
        Some("x".repeat(accessibility::IDENTITY_TEXT_LIMIT + 1)),
        None,
    ];
    let (found, _) = tree::find_screen(&mut windows, &request(), vec![second, first])
        .expect("the window naming none beside one past the limit");
    assert_eq!(windows.nodes[first].children, [found]);
    windows.nodes[first].identity = named;

    // Two windows that name the device are two of its windows: which one shows it is a guess.
    windows.nodes[second].identity = vec![Some(format!("device-{UDID}"))];
    let refused = tree::find_screen(&mut windows, &request(), vec![first, second])
        .expect_err("two windows name the device");
    assert!(matches!(refused, Error::Ambiguous { .. }), "{refused:?}");

    let missing = tree::find_screen(&mut windows, &request(), Vec::new()).expect_err("none");
    assert!(
        matches!(&missing, Error::NotFound { message, .. }
            if message.contains("is on screen") && message.contains(TITLE) && !message.contains("UUID")),
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

/// The screen is the group with the window's shape: an `iOSContentGroup` first, whatever cut the
/// chrome, else the shallowest, which a search whose chrome was cut (an element gone, or the
/// budget) does not take; a turned window's turned; none and two of another frame are refused.
#[test]
fn the_screen_is_the_group_with_the_windows_shape() {
    use SearchReach::{Budget, Gone, ToDepth, Whole};
    let simulator_app = [
        candidate((231.0, 142.0, 456.0, 52.0), None, 1),
        candidate((258.0, 222.0, 393.0, 852.0), None, 1),
    ];
    assert_eq!(
        tree::choose_screen(TITLE, &simulator_app, SIZE, Whole),
        Ok(1)
    );
    assert_eq!(
        tree::choose_screen(TITLE, &simulator_app, SIZE, ToDepth),
        Ok(1),
        "the levels searched were read whole"
    );
    for reach in [Gone, Budget] {
        assert_eq!(
            tree::choose_screen(TITLE, &simulator_app, SIZE, reach),
            Err(TreeError::SearchCut {
                title: TITLE.to_owned(),
                reach
            })
        );
    }
    let marked_deeper = [
        candidate((0.0, 0.0, 393.0, 852.0), None, 2),
        candidate((657.0, 266.0, 290.82, 630.48), Some(SCREEN_SUBROLE), 7),
    ];
    // A marked group marks itself as the screen: taken whatever cut the chrome.
    for reach in [Whole, ToDepth, Gone, Budget] {
        assert_eq!(
            tree::choose_screen(TITLE, &marked_deeper, SIZE, reach),
            Ok(1),
            "{reach:?}"
        );
    }
    let turned = [candidate((0.0, 0.0, 852.0, 393.0), None, 1)];
    assert_eq!(
        tree::choose_screen(TITLE, &turned, LogicalSize::new(852.0, 393.0), Whole),
        Ok(0)
    );
    assert!(matches!(
        tree::choose_screen(TITLE, &turned, SIZE, Gone),
        Err(TreeError::NoScreen { groups, reach: Gone, .. }) if groups.len() == 1
    ));
    // A cut that repeats at every read offers no retry; one by an element gone does.
    let retry = |reach| {
        let refused = TreeError::SearchCut {
            title: TITLE.to_owned(),
            reach,
        };
        match refused.into_error() {
            Error::NotFound { retry, .. } => retry,
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(retry(Budget), Retry::Never);
    assert_ne!(retry(Gone), Retry::Never);
    let two = [
        candidate((0.0, 0.0, 393.0, 852.0), None, 1),
        candidate((500.0, 0.0, 393.0, 852.0), None, 1),
    ];
    let refused = tree::choose_screen(TITLE, &two, SIZE, Whole).expect_err("two screens");
    assert!(matches!(&refused, TreeError::Ambiguous { candidates, .. } if candidates.len() == 2));
    let two_marked = [
        candidate((0.0, 0.0, 393.0, 852.0), Some(SCREEN_SUBROLE), 7),
        candidate((500.0, 0.0, 393.0, 852.0), Some(SCREEN_SUBROLE), 8),
    ];
    assert!(matches!(
        tree::choose_screen(TITLE, &two_marked, SIZE, Whole),
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
    // A read that stopped short cannot show the one match is the only one, identifier or not.
    let mut short = controls_screen(true);
    short.truncated = true;
    let tagged_like = short.elements[1].clone();
    assert!(tagged_like.identifier.is_some());
    let refused = tree::locate(&short, &tagged_like).expect_err("a read stopped short");
    assert!(
        matches!(&refused, Error::Ambiguous { message, .. } if message.contains("stopped short")),
        "{refused:?}"
    );
    let gone = labelled(element("AXButton", None, (0.0, 0.0, 1.0, 1.0)), "Gone");
    assert!(matches!(
        tree::locate(&screen, &gone),
        Err(Error::NotFound { .. })
    ));
}

/// A failure while the process is no longer trusted is the permission taken back, whatever the
/// call said; a call that did not answer names the deadline it ran under, or, when an element
/// refused the read's deadline, says it ran under the one the element kept.
#[test]
fn a_permission_taken_back_mid_read_is_named() {
    for error in [
        ProviderError::Failed("the accessibility API answered -25211".to_owned()),
        ProviderError::Timeout,
        ProviderError::Gone,
    ] {
        let refused = accessibility::read_failure(error, false, Some(Duration::from_secs(5)));
        assert_eq!(
            refused.missing_capability().as_deref(),
            Some("macos_permission_accessibility")
        );
    }
    let timeout = accessibility::read_failure(
        ProviderError::Timeout,
        true,
        Some(Duration::from_millis(1500)),
    );
    assert!(
        matches!(&timeout, Error::Timeout { message, .. } if message.contains("1500 ms")),
        "{timeout:?}"
    );
    let refused_deadline = accessibility::read_failure(ProviderError::Timeout, true, None);
    assert!(
        matches!(&refused_deadline, Error::Timeout { message, .. }
            if message.contains("refused") && !message.contains(" ms")),
        "{refused_deadline:?}"
    );
    assert!(matches!(
        accessibility::read_failure(
            ProviderError::Failed("x".to_owned()),
            true,
            Some(Duration::ZERO)
        ),
        Error::Platform { .. }
    ));
}

/// Whether the screen held still, as the read asks once the walk is done: the same frame does,
/// and a frame moved, resized or gone is refused as busy, to be read again.
#[test]
fn a_screen_holds_still_only_with_the_frame_it_was_found_at() {
    assert!(accessibility::screen_steady(SCREEN, Some(SCREEN)).is_ok());
    let moved = PointFrame::new(SCREEN.x + 40.0, SCREEN.y, SCREEN.width, SCREEN.height);
    let resized = PointFrame::new(
        SCREEN.x,
        SCREEN.y,
        SCREEN.width * 0.74,
        SCREEN.height * 0.74,
    );
    for after in [Some(moved), Some(resized), None] {
        let refused = accessibility::screen_steady(SCREEN, after).expect_err("moved");
        assert!(
            matches!(&refused, Error::Busy { message, .. } if message.contains("read again")),
            "{after:?}: {refused:?}"
        );
    }
}

/// A press names the deadline of the element it presses: the one the read gave it, or, when
/// that element refused it, the one it kept, whatever other elements of the read did; a press
/// not answered may have run; a failure while the process is no longer trusted is the
/// permission taken back.
#[test]
fn a_press_names_its_own_elements_deadline() {
    let given = Some(Duration::from_millis(5000));
    let unanswered = |answer: &flui_driver::Result<()>, ms: bool, may_have_run: bool| {
        matches!(answer, Err(Error::Timeout { message, effect, .. })
            if message.contains("5000 ms") == ms
                && message.contains("refused") != ms
                && effect.is_some() == may_have_run)
    };
    let not_offered = |deadline| {
        accessibility::pressed(
            Err(ProviderError::Timeout),
            || Ok(()),
            "Like",
            deadline,
            || true,
        )
    };
    let not_pressed = |deadline| {
        accessibility::pressed(
            Ok(true),
            || Err(ProviderError::Timeout),
            "Like",
            deadline,
            || true,
        )
    };
    for (deadline, ms) in [(given, true), (None, false)] {
        let answer = not_offered(deadline);
        assert!(unanswered(&answer, ms, false), "{answer:?}");
        let answer = not_pressed(deadline);
        assert!(unanswered(&answer, ms, true), "{answer:?}");
    }

    let mut ran = false;
    let refused = accessibility::pressed(
        Ok(false),
        || {
            ran = true;
            Ok(())
        },
        "Like",
        given,
        || true,
    );
    assert!(
        matches!(refused, Err(Error::ActionUnsupported { .. })),
        "{refused:?}"
    );
    assert!(!ran, "no press is made on an element that offers none");
    assert!(accessibility::pressed(Ok(true), || Ok(()), "Like", None, || true).is_ok());

    // Before the press is sent, a failure with the permission gone is the permission refused.
    let before = accessibility::pressed(
        Err(ProviderError::Timeout),
        || Ok(()),
        "Like",
        given,
        || false,
    );
    assert_eq!(
        before
            .expect_err("untrusted")
            .missing_capability()
            .as_deref(),
        Some("macos_permission_accessibility")
    );
    // Once it is sent, a press not answered or whose element went away may have run, and keeps
    // saying so with the permission gone; the message names the permission.
    let may_have_run = |answer: &flui_driver::Result<()>| match answer {
        Err(
            Error::Timeout {
                effect, message, ..
            }
            | Error::Gone {
                effect, message, ..
            },
        ) => effect.is_some() && message.contains("Accessibility permission"),
        _ => false,
    };
    for failure in [ProviderError::Timeout, ProviderError::Gone] {
        let answer =
            accessibility::pressed(Ok(true), || Err(failure.clone()), "Like", given, || false);
        assert!(may_have_run(&answer), "{failure:?}: {answer:?}");
    }
    let gone = accessibility::pressed(
        Ok(true),
        || Err(ProviderError::Gone),
        "Like",
        given,
        || true,
    );
    assert!(
        matches!(
            &gone,
            Err(Error::Gone {
                effect: Some(_),
                ..
            })
        ),
        "{gone:?}"
    );
    // Trusted, a lookup that finds the element gone or fails says so, and nothing was sent; a
    // press that failed otherwise is the platform's, not the permission.
    let lookup_gone =
        accessibility::pressed(Err(ProviderError::Gone), || Ok(()), "Like", given, || true);
    assert!(
        matches!(&lookup_gone, Err(Error::Gone { effect: None, .. })),
        "{lookup_gone:?}"
    );
    let lookup_failed = accessibility::pressed(
        Err(ProviderError::Failed("-25200".to_owned())),
        || Ok(()),
        "Like",
        given,
        || true,
    );
    assert!(
        matches!(&lookup_failed, Err(Error::Platform { effect: None, .. })),
        "{lookup_failed:?}"
    );
    let press_failed = accessibility::pressed(
        Ok(true),
        || Err(ProviderError::Failed("-25200".to_owned())),
        "Like",
        given,
        || true,
    );
    assert!(
        matches!(&press_failed, Err(Error::Platform { .. })),
        "{press_failed:?}"
    );

    // A press that failed otherwise with the permission gone is the permission refused.
    let failed = accessibility::pressed(
        Ok(true),
        || Err(ProviderError::Failed("-25211".to_owned())),
        "Like",
        given,
        || false,
    );
    assert_eq!(
        failed
            .expect_err("untrusted")
            .missing_capability()
            .as_deref(),
        Some("macos_permission_accessibility")
    );
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
