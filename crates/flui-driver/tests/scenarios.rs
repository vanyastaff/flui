//! The scenarios that replace the device checks, written once on `flui-driver` and run on the
//! headless target here: synthetic input by default, so CI holds them. On Windows, macOS and
//! Linux the same functions run against the same apps in a window, through the OS lane as their
//! certification variant (`windows_desktop`, `macos_desktop`, `x11_desktop`), and on an Android
//! device and an iOS Simulator against the app on it (`android_device`, `ios_simulator`).
//!
//! - [`gallery`]: every control of the gallery reached by its identifier and acted on;
//! - [`accessibility`]: what the app publishes for its controls, an action through the semantic
//!   path, and, through an OS lane, the OS tree projected against the app's and an activation
//!   through the OS accessibility API;
//! - [`roles`]: each role of the role table publishes the wire role UI Automation folds it to,
//!   and through an OS lane reads back as the native role of the accessibility API the lane reads
//!   through (UI Automation's control type, the `AXRole`, the AT-SPI role, Android's class name,
//!   the role the iOS Simulator's bridge gives the traits `accesskit_ios` sets);
//! - [`input`], [`macos_input`], [`android_input`] and [`ios_input`]: the pointers, the wheel and
//!   keys reach the app, read back from its journal, and a press beside a control leaves it alone;
//!   on Android a touchpad's pinch too (`android_device` runs it through the Android lane, and
//!   `ios_simulator` the iOS one through the iOS Simulator lane);
//! - [`safe_area`]: a `SafeArea`'s child sits inset by the padding the window reports;
//! - [`notes`] and [`macos_notes`]: text, a select-all chord and an input-method composition in a
//!   text field;
//! - [`web_input`]: [`input`] and a composition into a field, through the session's path, which in
//!   headless Chrome is the Chrome lane (`chrome_browser`);
//! - [`conformance`]: a button's name, role and identifier, its activation and the effect read
//!   back: the conformance row a platform's run reports.

use flui_driver::lane::AccessibilityApi;
use flui_driver::lane::android::roles as android_roles;
use flui_driver::lane::android::{self, AndroidLane};
use flui_driver::lane::atspi::roles as atspi_roles;
use flui_driver::lane::ios_simulator;
use flui_driver::lane::ios_simulator::accessibility::HostScreen;
use flui_driver::lane::ios_simulator::roles as ios_roles;
use flui_driver::lane::macos::roles as ax_roles;
use flui_driver::lane::windows::roles::{ROLES, expected_wire_role};
use flui_driver::protocol::geometry::{LogicalInsets, LogicalPoint};
use flui_driver::protocol::input::{
    ActionSequence, DeviceClass, ImeAction, InputKind, InputSource, PointerAction, PointerButton,
    PointerDevice, ScrollUnit, TrackpadAction, WheelAction,
};
use flui_driver::protocol::report::Check;
use flui_driver::protocol::report::{Outcome, Report};
use flui_driver::protocol::semantics::SemanticsRole;
use flui_driver::protocol::session::Platform;
use flui_driver::protocol::tree::Node;
use flui_driver::protocol::verdict::Verdict;
use flui_driver::protocol::wire::Role;
use flui_driver::testing::ScriptedSimulatorScreen;
use flui_driver::{Driver, Error, InputPath, Launch, Locator, Result, expect};
use flui_foundation::geometry::EdgeInsets;
use flui_platform::{TargetPlatform, WindowInsets};
use flui_view::environment::PlatformConvention;

use crate::android_lane;
use crate::apps::{Controls, Insets, Notes, Roles, role_identifier, role_label};

/// The window every scenario app opens.
pub const WINDOW: (u32, u32) = (800, 600);

/// Where the element `identifier` takes a pointer, once it is actionable.
pub fn point_of(driver: &mut Driver, identifier: &str) -> Result<LogicalPoint> {
    let mut window = driver.main_window()?;
    let facts = window
        .locate(Locator::identifier(identifier))
        .resolve()?
        .actionability()?;
    facts
        .point
        .or_else(|| facts.rect.map(|rect| rect.center()))
        .ok_or_else(|| Error::Expectation {
            expected: format!("{identifier} to show a point"),
            actual: "no point".to_owned(),
        })
}

/// A press and release of `device` at `at`.
fn press(device: PointerDevice, at: LogicalPoint) -> ActionSequence {
    ActionSequence::new(vec![InputSource::Pointer {
        id: format!("scenario.{}", device.name()),
        device,
        actions: vec![
            PointerAction::Move {
                to: at,
                duration_ms: 0,
                pressure: None,
            },
            PointerAction::Down {
                button: PointerButton::Primary,
                pressure: None,
            },
            PointerAction::Pause { duration_ms: 50 },
            PointerAction::Up {
                button: PointerButton::Primary,
            },
        ],
    }])
}

/// The journal's entries past `since`.
fn journal_since(driver: &mut Driver, since: u64) -> Result<Vec<(DeviceClass, InputKind)>> {
    let journal = driver.main_window()?.journal()?;
    Ok(journal
        .entries
        .iter()
        .filter(|entry| entry.seq > since)
        .map(|entry| (entry.device, entry.kind))
        .collect())
}

fn last_seq(driver: &mut Driver) -> Result<u64> {
    let journal = driver.main_window()?.journal()?;
    Ok(journal.entries.last().map_or(0, |entry| entry.seq))
}

fn saw(entries: &[(DeviceClass, InputKind)], wanted: &[(DeviceClass, InputKind)]) -> Result<()> {
    let missing: Vec<_> = wanted.iter().filter(|w| !entries.contains(w)).collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(Error::Expectation {
        expected: format!("the journal to show {missing:?}"),
        actual: format!("{entries:?}"),
    })
}

/// `gallery`: see the [module docs](self).
///
/// # Errors
///
/// When the app cannot be reached; each check's failure is recorded in `report`.
pub fn gallery(driver: &mut Driver, report: &mut Report) -> Result<()> {
    driver.main_window()?;
    let _ = report.check("a tap on like counts one like", || {
        let mut window = driver.main_window()?;
        window.locate(Locator::identifier("like")).tap()?;
        expect(window.locate(Locator::identifier("likes"))).to_have_name("Likes: 1")
    });
    let _ = report.check("a tap on subscribe turns it on", || {
        let mut window = driver.main_window()?;
        window.locate(Locator::identifier("subscribe")).tap()?;
        expect(window.locate(Locator::identifier("subscribe"))).to_be_checked()
    });
    let _ = report.check("text typed into comment is its value", || {
        let mut window = driver.main_window()?;
        window
            .locate(Locator::identifier("comment"))
            .type_text("hello")?;
        expect(window.locate(Locator::identifier("comment"))).to_have_value("hello")
    });
    let _ = report.check("a tap on the pad counts one press", || {
        let mut window = driver.main_window()?;
        window.locate(Locator::identifier("pad")).tap()?;
        expect(window.locate(Locator::identifier("pad"))).to_have_name("Pad: 1")
    });
    Ok(())
}

/// The node `identifier` names in `nodes`.
fn find<'a>(nodes: &'a [Node], identifier: &str) -> Option<&'a Node> {
    nodes.iter().find_map(|node| {
        let named = node.identifier.as_deref().or(node.automation_id.as_deref());
        if named == Some(identifier) {
            Some(node)
        } else {
            find(&node.children, identifier)
        }
    })
}

fn published(node: Option<&Node>, role: Role, name: &str) -> Result<()> {
    match node {
        Some(node) if node.role == role && node.name.as_deref() == Some(name) => Ok(()),
        other => Err(Error::Expectation {
            expected: format!("a {} named {name:?}", role.name()),
            actual: format!("{other:?}"),
        }),
    }
}

/// `accessibility`: see the [module docs](self).
///
/// # Errors
///
/// When the app cannot be reached; each check's failure is recorded in `report`.
pub fn accessibility(driver: &mut Driver, report: &mut Report) -> Result<()> {
    driver.main_window()?;
    let _ = report.check("like is a button named Like", || {
        let tree = driver.main_window()?.read()?;
        published(find(&tree.roots, "like"), Role::Button, "Like")
    });
    let _ = report.check("subscribe is a switch named Subscribe", || {
        let tree = driver.main_window()?.read()?;
        published(find(&tree.roots, "subscribe"), Role::Switch, "Subscribe")
    });
    let _ = report.check("like is invoked through the semantic path", || {
        let mut window = driver.main_window()?;
        let outcome = window
            .locate(Locator::identifier("like"))
            .require(InputPath::Semantic)
            .tap()?;
        if outcome.via != InputPath::Semantic {
            return Err(Error::Expectation {
                expected: "the semantic path".to_owned(),
                actual: outcome.via.name().to_owned(),
            });
        }
        expect(window.locate(Locator::identifier("likes"))).to_have_name("Likes: 1")
    });
    if driver.lane_name().is_none() {
        return Ok(());
    }
    let _ = report.check(
        "the OS reads like as a button with its automation id",
        || {
            let tree = driver.main_window()?.os_tree()?;
            published(find(&tree.roots, "like"), Role::Button, "Like")
        },
    );
    let _ = report.check("the OS tree projects to what the app published", || {
        let projection = driver.main_window()?.compare_projection()?;
        if projection.is_equal() {
            return Ok(());
        }
        Err(Error::Expectation {
            expected: projection.app.clone(),
            actual: format!("{:?}", projection.differences()),
        })
    });
    let _ = report.check("like is invoked through the OS accessibility API", || {
        let mut window = driver.main_window()?;
        window.os_invoke("like")?;
        expect(window.locate(Locator::identifier("likes"))).to_have_name("Likes: 2")
    });
    Ok(())
}

/// `roles`: see the [module docs](self).
///
/// # Errors
///
/// When the app cannot be reached; each check's failure is recorded in `report`.
pub fn roles(driver: &mut Driver, report: &mut Report) -> Result<()> {
    published_roles(driver, report)?;
    // The table the OS's roles are read against is the one of the accessibility API the lane
    // reads through.
    let Some(api) = driver
        .lane_capabilities()
        .and_then(|capabilities| capabilities.accessibility)
    else {
        return Ok(());
    };
    let (name, natives): (&str, Vec<(SemanticsRole, String)>) = match api {
        AccessibilityApi::UiAutomation => (
            "UI Automation",
            ROLES
                .iter()
                .map(|row| (row.role, row.control.name().to_owned()))
                .collect::<Vec<(SemanticsRole, String)>>(),
        ),
        AccessibilityApi::Ax => (
            "the accessibility API",
            ax_roles::ROLES
                .iter()
                .map(|row| (row.role, row.native()))
                .collect::<Vec<(SemanticsRole, String)>>(),
        ),
        AccessibilityApi::AtSpi => (
            "AT-SPI",
            atspi_roles::ROLES
                .iter()
                .map(|row| (row.role, row.atspi.native_name()))
                .collect::<Vec<(SemanticsRole, String)>>(),
        ),
        // Android's tree names no identifier: its nodes are found by the lane's match, which is
        // what the rows check.
        AccessibilityApi::UiAutomator => return matched_roles(driver, report),
        // The Simulator's bridge carries no identifier a run has confirmed: its elements are
        // found by the lane's match too.
        AccessibilityApi::SimulatorAx => return simulator_roles(driver, report),
        other => {
            let _ = report.check("the OS reads each role", || -> Result<()> {
                Err(Error::Expectation {
                    expected: "a role table for the lane's accessibility API".to_owned(),
                    actual: format!("{other:?}"),
                })
            });
            return Ok(());
        }
    };
    os_roles(driver, report, name, &natives)
}

/// Each role of the role table publishes the wire role the role table folds it to.
fn published_roles(driver: &mut Driver, report: &mut Report) -> Result<()> {
    let tree = driver.main_window()?.read()?;
    let mut by_name = Vec::new();
    collect(&tree.roots, &mut by_name);
    for row in ROLES {
        let label = role_label(row.role);
        let expected = expected_wire_role(row);
        let _ = report.check(
            format!("{} publishes {}", row.role.name(), expected.name()),
            || match by_name.iter().find(|(name, _)| *name == label) {
                Some((_, role)) if *role == expected => Ok(()),
                found => Err(Error::Expectation {
                    expected: format!("{label:?} as {}", expected.name()),
                    actual: format!("{found:?}"),
                }),
            },
        );
    }
    Ok(())
}

/// Each role read back through the OS accessibility API `api` as the native role `natives` names
/// for it.
fn os_roles(
    driver: &mut Driver,
    report: &mut Report,
    api: &str,
    natives: &[(SemanticsRole, String)],
) -> Result<()> {
    let os = driver.main_window()?.os_tree()?;
    let mut os_nodes = Vec::new();
    collect_nodes(&os.roots, &mut os_nodes);
    for (role, native) in natives {
        let label = role_label(*role);
        let _ = report.check(
            format!("{api} reads {} as {native}", role.name()),
            || match os_nodes.iter().find(|node| {
                node.name.as_deref() == Some(&label) || node.description.as_deref() == Some(&label)
            }) {
                Some(node) if node.native_role == *native => Ok(()),
                found => Err(Error::Expectation {
                    expected: format!("{label:?} as a {native}"),
                    actual: format!("{:?}", found.map(|node| &node.native_role)),
                }),
            },
        );
    }
    Ok(())
}

/// Each role read back through UI Automator, the lane matching Android's nodes to the app's: the
/// node the lane matched to the row's node, found by the identifier the match carries over, has
/// the class `accesskit_android` gives the role, and the row's label as its name, or no name for
/// a role whose label reaches Android nowhere
/// (`android_tree::talkback_reads_no_label_of_these_roles`).
fn matched_roles(driver: &mut Driver, report: &mut Report) -> Result<()> {
    let os = driver.main_window()?.os_tree()?;
    for row in android_roles::ROLES {
        let label = role_label(row.role);
        let name = row.named.then_some(label.as_str());
        let _ = report.check(
            format!("UI Automator reads {} as {}", row.role.name(), row.class),
            || match find(&os.roots, &role_identifier(row.role)) {
                Some(node) if node.native_role == row.class && node.name.as_deref() == name => {
                    Ok(())
                }
                found => Err(Error::Expectation {
                    expected: format!(
                        "the node matched to {label:?}, a {} named {name:?}",
                        row.class
                    ),
                    actual: format!("{:?}", found.map(|node| (&node.native_role, &node.name))),
                }),
            },
        );
    }
    Ok(())
}

/// Each role read back through the Simulator's bridge, found as the element the lane matched to
/// the role's node (by its identifier, which the match carries over): the native role the iOS
/// table gives it, and the label, or for a role ARIA forbids a name the hint, that VoiceOver reads.
fn simulator_roles(driver: &mut Driver, report: &mut Report) -> Result<()> {
    let os = driver.main_window()?.os_tree()?;
    for row in ios_roles::ROLES {
        let label = role_label(row.role);
        let native = row.native();
        let _ = report.check(
            format!(
                "the Simulator's accessibility bridge reads {} as {native}",
                row.role.name()
            ),
            || match find(&os.roots, &role_identifier(row.role)) {
                Some(node)
                    if node.native_role == native
                        && (node.name.as_deref() == Some(&label)
                            || node.description.as_deref() == Some(&label)) =>
                {
                    Ok(())
                }
                found => Err(Error::Expectation {
                    expected: format!("the element matched to {label:?}, a {native}"),
                    actual: format!(
                        "{:?}",
                        found.map(|node| (&node.native_role, &node.name, &node.description))
                    ),
                }),
            },
        );
    }
    Ok(())
}

/// Each node by its name, and by its description: a role ARIA forbids a name (a paragraph, a
/// caption) publishes its label as its description.
fn collect(nodes: &[Node], into: &mut Vec<(String, Role)>) {
    for node in nodes {
        for text in [&node.name, &node.description].into_iter().flatten() {
            into.push((text.clone(), node.role));
        }
        collect(&node.children, into);
    }
}

fn collect_nodes<'a>(nodes: &'a [Node], into: &mut Vec<&'a Node>) {
    for node in nodes {
        into.push(node);
        collect_nodes(&node.children, into);
    }
}

/// `input`: see the [module docs](self).
///
/// # Errors
///
/// When the app cannot be reached; each check's failure is recorded in `report`.
pub fn input(driver: &mut Driver, report: &mut Report) -> Result<()> {
    input_with(
        driver,
        report,
        &[
            (PointerDevice::Mouse, DeviceClass::Mouse),
            (PointerDevice::Touch, DeviceClass::Touch),
            (PointerDevice::Pen, DeviceClass::Pen),
        ],
        corner(),
    )
}

/// A point beside the controls: near the bottom right corner of the scenarios' window.
fn corner() -> LogicalPoint {
    LogicalPoint::new(f64::from(WINDOW.0) - 20.0, f64::from(WINDOW.1) - 20.0)
}

/// `android_input`: [`input`] with the touch screen first, as a phone takes it, and a touchpad's
/// pinch. The press beside the controls lands below them, halfway across the window: a phone's
/// window is its screen, whose corners are the system's gestures. Through the Android lane a pen
/// goes through a `uinput` device, so on a device whose shell may not register one the pen row
/// is `cannot_verify` naming `uinput_device`; a touchpad's gesture goes through no OS lane, so
/// that row is `cannot_verify` there whatever the device.
///
/// # Errors
///
/// When the app cannot be reached; each check's failure is recorded in `report`.
pub fn android_input(driver: &mut Driver, report: &mut Report) -> Result<()> {
    let size = driver.main_window()?.placement()?.size;
    input_with(
        driver,
        report,
        &[
            (PointerDevice::Touch, DeviceClass::Touch),
            (PointerDevice::Mouse, DeviceClass::Mouse),
            (PointerDevice::Pen, DeviceClass::Pen),
        ],
        LogicalPoint::new(size.width / 2.0, size.height * 0.9),
    )?;
    let pad = point_of(driver, "pad")?;
    let _ = report.check("a touchpad's pinch over the pad reaches the app", || {
        let since = last_seq(driver)?;
        driver
            .main_window()?
            .perform(ActionSequence::new(vec![InputSource::Trackpad {
                id: "scenario.touchpad".to_owned(),
                actions: vec![
                    TrackpadAction::Begin { at: pad },
                    TrackpadAction::Pinch { scale: 1.25 },
                    TrackpadAction::End,
                ],
            }]))?;
        saw(
            &journal_since(driver, since)?,
            &[(DeviceClass::Trackpad, InputKind::Gesture)],
        )
    });
    Ok(())
}

/// `macos_input`: [`input`] with the mouse, the one pointer a macOS app reads as itself: Quartz
/// has no touch event, and the AppKit host reads a pen's tablet events as the mouse (the
/// `pen-and-stylus` row of `crates/flui-native/platform-parity.toml`).
///
/// # Errors
///
/// When the app cannot be reached; each check's failure is recorded in `report`.
pub fn macos_input(driver: &mut Driver, report: &mut Report) -> Result<()> {
    input_with(
        driver,
        report,
        &[(PointerDevice::Mouse, DeviceClass::Mouse)],
        corner(),
    )
}

/// `ios_input`: [`input`] with touch, the pointer an iPhone takes, and the press beside the
/// controls below them, halfway across the window: a phone's window is its screen, whose edges
/// are the system's gestures. It replaces the tap and the no-target tap of the iOS device check.
/// Through the iOS Simulator lane, which sends no input, each row is `cannot_verify` naming
/// `ios_simulator_hid` (`ios_simulator::the_ios_scenarios_run_on_a_simulator`).
///
/// # Errors
///
/// When the app cannot be reached; each check's failure is recorded in `report`.
pub fn ios_input(driver: &mut Driver, report: &mut Report) -> Result<()> {
    let size = driver.main_window()?.placement()?.size;
    input_with(
        driver,
        report,
        &[(PointerDevice::Touch, DeviceClass::Touch)],
        LogicalPoint::new(size.width / 2.0, size.height * 0.9),
    )
}

/// `safe_area`: the window reports a safe area (a top inset in portrait, where a phone's status
/// bar or notch takes the top, and some inset in landscape), and the child of a `SafeArea` sits
/// inset from the window's top left by exactly that padding, where the marker outside it does not
/// move. On iOS that is UIKit's `safeAreaInsets` reaching the window's `MediaQuery` and
/// `SafeArea` (the iOS safe-area device check): a window that reports no inset fails, since a
/// padding that never arrived and a `SafeArea` that ignored it would both read as zero.
///
/// # Errors
///
/// When the app cannot be reached; the check's failure is recorded in `report`.
pub fn safe_area(driver: &mut Driver, report: &mut Report) -> Result<()> {
    driver.main_window()?;
    let _ = report.check(
        "a SafeArea insets its child by the padding the window reports",
        || {
            let size = driver.main_window()?.placement()?.size;
            let tree = driver.main_window()?.read()?;
            let label = find(&tree.roots, "insets")
                .and_then(|node| node.name.clone())
                .unwrap_or_default();
            let padding: Vec<f64> = label
                .split(' ')
                .filter_map(|side| side.parse::<f64>().ok())
                .collect();
            let &[top, right, bottom, left] = padding.as_slice() else {
                return Err(Error::Expectation {
                    expected: "the padding as four numbers".to_owned(),
                    actual: format!("{label:?}"),
                });
            };
            let portrait = size.height > size.width;
            let reported = if portrait {
                top > 0.0
            } else {
                [top, right, bottom, left].iter().any(|side| *side > 0.0)
            };
            if !reported {
                return Err(Error::Expectation {
                    expected: if portrait {
                        "a top inset in a portrait window".to_owned()
                    } else {
                        "an inset on some side".to_owned()
                    },
                    actual: format!("the padding {label:?}"),
                });
            }
            let origin = |driver: &mut Driver, identifier: &str| -> Result<(f64, f64)> {
                let rect = driver
                    .main_window()?
                    .locate(Locator::identifier(identifier))
                    .resolve()?
                    .actionability()?
                    .rect
                    .ok_or_else(|| Error::Expectation {
                        expected: format!("{identifier} to have bounds"),
                        actual: "none".to_owned(),
                    })?;
                Ok((rect.x, rect.y))
            };
            let outside = origin(driver, "outside")?;
            let inside = origin(driver, "inside")?;
            let offset = (inside.0 - outside.0, inside.1 - outside.1);
            if (offset.0 - left).abs() <= 0.5 && (offset.1 - top).abs() <= 0.5 {
                return Ok(());
            }
            Err(Error::Expectation {
                expected: format!(
                    "the inside marker {left} right of and {top} below the outside one"
                ),
                actual: format!("{} right and {} below", offset.0, offset.1),
            })
        },
    );
    Ok(())
}

/// [`input`] with the pointers `devices`, each read back from the journal as its class, and the
/// press beside the controls at `beside`.
fn input_with(
    driver: &mut Driver,
    report: &mut Report,
    devices: &[(PointerDevice, DeviceClass)],
    beside: LogicalPoint,
) -> Result<()> {
    let pad = point_of(driver, "pad")?;
    let mut presses = 0;
    for &(device, class) in devices {
        let checked = report.check(
            format!("a {} press on the pad reaches it", device.name()),
            || {
                let since = last_seq(driver)?;
                driver.main_window()?.perform(press(device, pad))?;
                expect(driver.main_window()?.locate(Locator::identifier("pad")))
                    .to_have_name(&format!("Pad: {}", presses + 1))?;
                saw(
                    &journal_since(driver, since)?,
                    &[
                        (class, InputKind::PointerDown),
                        (class, InputKind::PointerUp),
                    ],
                )
            },
        );
        presses += u32::from(checked.is_ok());
    }
    let _ = report.check("a press beside the controls leaves the pad's count", || {
        driver
            .main_window()?
            .perform(press(PointerDevice::Mouse, beside))?;
        expect(driver.main_window()?.locate(Locator::identifier("pad")))
            .to_have_name(&format!("Pad: {presses}"))
    });
    let _ = report.check("a turn of the wheel over the pad reaches the app", || {
        let since = last_seq(driver)?;
        driver
            .main_window()?
            .perform(ActionSequence::new(vec![InputSource::Wheel {
                id: "scenario.wheel".to_owned(),
                actions: vec![WheelAction::Scroll {
                    at: pad,
                    delta_x: 0.0,
                    delta_y: 40.0,
                    unit: ScrollUnit::Pixel,
                }],
            }]))?;
        let entries = journal_since(driver, since)?;
        if entries.iter().any(|(_, kind)| *kind == InputKind::Scroll) {
            return Ok(());
        }
        Err(Error::Expectation {
            expected: "a scroll in the journal".to_owned(),
            actual: format!("{entries:?}"),
        })
    });
    let _ = report.check("Tab reaches like and Enter activates it", || {
        let since = last_seq(driver)?;
        let window = driver.main_window()?.id();
        // Keys to whatever holds focus, with no element pressed first.
        driver.call_value(
            "element.press_keys",
            serde_json::json!({ "window": window, "keys": ["Tab", "Enter"] }),
        )?;
        expect(driver.main_window()?.locate(Locator::identifier("likes")))
            .to_have_name("Likes: 1")?;
        saw(
            &journal_since(driver, since)?,
            &[
                (DeviceClass::Keyboard, InputKind::KeyDown),
                (DeviceClass::Keyboard, InputKind::KeyUp),
            ],
        )
    });
    Ok(())
}

/// `notes`: see the [module docs](self).
///
/// # Errors
///
/// When the app cannot be reached; each check's failure is recorded in `report`.
pub fn notes(driver: &mut Driver, report: &mut Report) -> Result<()> {
    notes_with(driver, report, "Control+a")
}

/// `macos_notes`: [`notes`] with macOS's select-all chord, Command and A, which the app takes on
/// a macOS host by its platform's conventions.
///
/// # Errors
///
/// When the app cannot be reached; each check's failure is recorded in `report`.
pub fn macos_notes(driver: &mut Driver, report: &mut Report) -> Result<()> {
    notes_with(driver, report, "Meta+a")
}

/// [`notes`] with `select_all` as the chord that selects the note before it is cleared.
fn notes_with(driver: &mut Driver, report: &mut Report, select_all: &str) -> Result<()> {
    driver.main_window()?;
    let _ = report.check(
        "typed text, accents and ideographs included, is the note",
        || {
            let mut window = driver.main_window()?;
            window
                .locate(Locator::identifier("note"))
                .type_text("Grüße 日本")?;
            expect(window.locate(Locator::identifier("note"))).to_have_value("Grüße 日本")?;
            expect(window.locate(Locator::identifier("length"))).to_have_name("Length: 8")
        },
    );
    let _ = report.check(format!("{select_all} and Backspace clear the note"), || {
        let mut window = driver.main_window()?;
        window
            .locate(Locator::identifier("note"))
            .press_keys(&[select_all, "Backspace"])?;
        expect(window.locate(Locator::identifier("length"))).to_have_name("Length: 0")
    });
    let _ = report.check("a composition commits its conversion", || {
        let mut window = driver.main_window()?;
        // A text service's composition is out of an OS lane's reach: it goes through the app's
        // own input path whatever the session's, into the field focused by that path too.
        window
            .locate(Locator::identifier("note"))
            .require(InputPath::Synthetic)
            .tap()?;
        window.perform_via(
            ActionSequence::new(vec![InputSource::Ime {
                actions: vec![
                    ImeAction::SetComposition {
                        text: "にほん".to_owned(),
                        cursor: None,
                        caret_hidden: false,
                    },
                    ImeAction::Commit {
                        text: "日本".to_owned(),
                    },
                ],
            }]),
            flui_driver::InputPolicy::require(InputPath::Synthetic),
        )?;
        expect(window.locate(Locator::identifier("note"))).to_have_value("日本")?;
        expect(window.locate(Locator::identifier("length"))).to_have_name("Length: 2")
    });
    Ok(())
}

/// `web_input`: [`input`]'s pointers, wheel and keys, then an input method's composition into the
/// comment field, every action through the session's path: on the Chrome lane, trusted BiDi input
/// and the DevTools input method; on the headless target, synthetic input.
///
/// # Errors
///
/// When the app cannot be reached; each check's failure is recorded in `report`.
pub fn web_input(driver: &mut Driver, report: &mut Report) -> Result<()> {
    input(driver, report)?;
    let _ = report.check("a composition into comment commits its conversion", || {
        let mut window = driver.main_window()?;
        window.locate(Locator::identifier("comment")).tap()?;
        window.perform(ActionSequence::new(vec![InputSource::Ime {
            actions: vec![
                ImeAction::SetComposition {
                    text: "にほん".to_owned(),
                    cursor: None,
                    caret_hidden: false,
                },
                ImeAction::Commit {
                    text: "日本".to_owned(),
                },
            ],
        }]))?;
        expect(window.locate(Locator::identifier("comment"))).to_have_value("日本")
    });
    Ok(())
}

/// `conformance`: see the [module docs](self).
///
/// # Errors
///
/// When the app cannot be reached; each check's failure is recorded in `report`.
pub fn conformance(driver: &mut Driver, report: &mut Report) -> Result<()> {
    driver.main_window()?;
    let _ = report.check(
        "a button is published with its name, role and identifier",
        || {
            let tree = driver.main_window()?.read()?;
            published(find(&tree.roots, "like"), Role::Button, "Like")
        },
    );
    let _ = report.check("activating the button changes what it controls", || {
        let mut window = driver.main_window()?;
        window.locate(Locator::identifier("like")).tap()?;
        expect(window.locate(Locator::identifier("likes"))).to_have_name("Likes: 1")
    });
    if driver.lane_name().is_some() {
        let _ = report.check(
            "assistive technology reads the button as the app published it",
            || {
                let tree = driver.main_window()?.os_tree()?;
                published(find(&tree.roots, "like"), Role::Button, "Like")
            },
        );
    }
    Ok(())
}

/// A headless session of `app`.
fn headless<V: flui_view::View + Clone + 'static>(
    app: impl FnOnce() -> V + Send + 'static,
) -> Driver {
    Driver::launch(Launch::headless(app).window_size(WINDOW.0, WINDOW.1))
        .expect("the headless app starts")
}

fn run(
    name: &str,
    mut driver: Driver,
    scenario: impl FnOnce(&mut Driver, &mut Report) -> Result<()>,
) -> Report {
    let mut report = Report::new();
    driver.run_scenario(&mut report, name, scenario);
    report
}

fn passed(report: &Report) {
    let failed: Vec<&Check> = report
        .checks()
        .iter()
        .filter(|check| check.outcome != Outcome::Passed)
        .collect();
    assert_eq!(report.verdict(), Verdict::Pass, "{failed:#?}");
    assert!(report.checks().len() > 1, "the scenario made its checks");
}

#[test]
fn the_gallery_scenario_passes_on_the_headless_target() {
    passed(&run("gallery", headless(|| Controls), gallery));
}

#[test]
fn the_accessibility_scenario_passes_on_the_headless_target() {
    passed(&run("windows-a11y", headless(|| Controls), accessibility));
}

#[test]
fn each_role_publishes_the_wire_role_ui_automation_folds_it_to() {
    passed(&run("roles", headless(|| Roles), roles));
}

#[test]
fn the_input_scenario_passes_on_the_headless_target() {
    passed(&run("windows-input", headless(|| Controls), input));
}

#[test]
fn the_notes_scenario_passes_on_the_headless_target() {
    passed(&run("windows-notes", headless(|| Notes), notes));
}

/// The macOS input scenario, and the half of the macOS roles scenario that needs no OS lane: the
/// role the app publishes for each row. The accessibility API's half reads an OS tree, so it runs
/// on a Mac (`macos_desktop::every_role_reads_back_as_its_ax_role_through_the_accessibility_api`),
/// and the table it reads against is held by `macos_lane`'s tables.
#[test]
fn the_macos_input_scenario_and_the_published_roles_pass_on_the_headless_target() {
    passed(&run("macos-input", headless(|| Controls), macos_input));
    passed(&run("macos-roles", headless(|| Roles), roles));
}

/// The macOS variant of the notes scenario, its select-all chord Command and A, on an app that
/// follows macOS's conventions as it does on a Mac.
#[test]
fn the_macos_notes_scenario_passes_with_the_macos_conventions_on_the_headless_target() {
    passed(&run(
        "macos-notes",
        headless(|| PlatformConvention::new(TargetPlatform::MacOS, Notes)),
        macos_notes,
    ));
}

/// The web input scenario on the headless target, synthetic; in headless Chrome through the Chrome
/// lane in `chrome_browser`.
#[test]
fn the_web_input_scenario_passes_on_the_headless_target() {
    passed(&run("web-input", headless(|| Controls), web_input));
}
/// The Android input scenario on the headless target, whose windows take no injected touchpad
/// gesture yet: that row alone is `cannot_verify`, naming what is missing. Through the Android
/// lane it runs on a device or an emulator
/// (`android_device::the_android_input_scenario_runs_through_the_os_lane`).
#[test]
fn the_android_input_scenario_passes_on_the_headless_target_but_its_touchpad_row() {
    let report = run("android-input", headless(|| Controls), android_input);
    let (touchpad, rest): (Vec<&Check>, Vec<&Check>) = report
        .checks()
        .iter()
        .partition(|check| check.name.contains("touchpad"));
    let failed: Vec<&&Check> = rest
        .iter()
        .filter(|check| check.outcome != Outcome::Passed)
        .collect();
    assert_eq!(failed, Vec::<&&Check>::new());
    assert!(rest.len() > 5, "the scenario made its checks: {rest:#?}");
    match touchpad.as_slice() {
        [check] => assert!(
            matches!(
                &check.outcome,
                Outcome::CannotVerify { capability: Some(capability), .. }
                    if capability == "input.inject"
            ),
            "{check:?}"
        ),
        other => panic!("one touchpad row: {other:#?}"),
    }
}

/// The roles page of the headless scenario app as `accesskit_android` serves it, written out by
/// hand from the adapter's source: the class of each row's AccessKit role, its label as the
/// content description (none for the seven roles whose label FLUI publishes as the description),
/// its bounds, edge to edge in an 800 by 600 window.
const ROLES_WINDOW: &str = include_str!("android/roles_window.xml");

/// The controls of the headless scenario app after one like, as `accesskit_android` serves them,
/// written out by hand likewise.
const CONTROLS_WINDOW: &str = include_str!("android/controls_window.xml");

/// `driver`, a headless app, driven through the Android lane on a scripted emulator whose window
/// `uiautomator dump` answers with `window`, edge to edge in an 800 by 600 window.
fn through_android(driver: Driver, window: &str) -> Driver {
    let size = |pixels: u32| i32::try_from(pixels).expect("the window's size fits");
    let windows = android_lane::windows_edge_to_edge(size(WINDOW.0), size(WINDOW.1));
    let adb = android_lane::scripted_showing(true, None, window, &windows)
        .refuse_stream("shell uinput -");
    let lane =
        AndroidLane::connect(adb.device(), android_lane::PACKAGE).expect("the lane connects");
    driver.with_lane(lane).expect("the lane starts")
}

/// The headless half of the Android roles scenario: each role publishes the AccessKit role the
/// Android table composes with `accesskit_android`'s class, and through the Android lane, reading
/// the roles page as the adapter serves it, the lane matches every row's node and reads it as the
/// table's class. On a device or an emulator it reads the dump UI Automator takes
/// (`android_device::the_android_roles_scenario_runs_through_the_os_lane`).
#[test]
fn the_android_roles_scenario_passes_through_the_lane_on_the_headless_app() {
    let mut driver = headless(|| Roles);
    let app = driver
        .main_window()
        .and_then(|mut window| window.read())
        .expect("the app's tree");
    for row in android_roles::ROLES {
        let node = find(&app.roots, &role_identifier(row.role))
            .unwrap_or_else(|| panic!("{} is published", row.role.name()));
        assert_eq!(node.native_role, row.accesskit, "{}", row.role.name());
    }
    let report = run(
        "android-roles",
        through_android(driver, ROLES_WINDOW),
        roles,
    );
    passed(&report);
    assert_eq!(report.lane(), Some(android::LANE));
    let rows = report
        .checks()
        .iter()
        .filter(|check| check.name.starts_with("UI Automator reads "))
        .count();
    assert_eq!(rows, android_roles::ROLES.len(), "{:#?}", report.checks());
}

/// The rows of the roles scenario through the Android lane check the lane's match, not a lookup
/// by name: a row the lane cannot match fails though its node, named, is in the dump.
#[test]
fn an_android_role_row_fails_when_the_lane_cannot_match_its_node() {
    let moved = ROLES_WINDOW.replace("bounds=\"[380,0][420,4]\"", "bounds=\"[380,0][420,9]\"");
    assert_ne!(moved, ROLES_WINDOW);
    let report = run(
        "android-roles",
        through_android(headless(|| Roles), &moved),
        roles,
    );
    let failed: Vec<&str> = report
        .checks()
        .iter()
        .filter(|check| check.outcome != Outcome::Passed)
        .map(|check| check.name.as_str())
        .collect();
    assert!(
        failed.contains(&"UI Automator reads button as android.widget.Button"),
        "{failed:?}"
    );
}

/// The headless half of the Android accessibility scenario: through the Android lane, reading the
/// controls as the adapter serves them, the OS tree shows the button with the app's identifier and
/// projects to what the app published; the activation through the accessibility API is
/// `cannot_verify`, naming `os_invoke`, since the lane performs no accessibility action.
#[test]
fn the_android_accessibility_scenario_passes_through_the_lane_but_its_activation() {
    let report = run(
        "android-a11y",
        through_android(headless(|| Controls), CONTROLS_WINDOW),
        accessibility,
    );
    let (activation, rest): (Vec<&Check>, Vec<&Check>) = report
        .checks()
        .iter()
        .partition(|check| check.name == "like is invoked through the OS accessibility API");
    let failed: Vec<&&Check> = rest
        .iter()
        .filter(|check| check.outcome != Outcome::Passed)
        .collect();
    assert_eq!(failed, Vec::<&&Check>::new());
    // Five checks and the scenario's own row.
    assert_eq!(rest.len(), 6, "{rest:#?}");
    match activation.as_slice() {
        [check] => assert!(
            matches!(
                &check.outcome,
                Outcome::CannotVerify { capability: Some(capability), .. }
                    if capability == "os_invoke"
            ),
            "{check:?}"
        ),
        other => panic!("one activation row: {other:#?}"),
    }
}

/// `driver`, a headless app, driven through the iOS Simulator lane on a scripted Simulator whose
/// window serves `page`, the app's page written out as the Simulator would serve it
/// (`ios_simulator_tree::controls_page`, `roles_page`), and the window, with the directory the
/// lane's hold lives in.
fn through_simulator(
    driver: Driver,
    page: HostScreen,
) -> (tempfile::TempDir, Driver, ScriptedSimulatorScreen) {
    let screen = ScriptedSimulatorScreen::new(crate::ios_simulator_tree::TITLE, page);
    let (dir, lane) = crate::ios_simulator_lane::connect(&crate::ios_simulator_lane::scripted());
    let driver = driver
        .with_lane(lane.read_through(screen.accessibility()))
        .expect("the lane starts");
    (dir, driver, screen)
}

/// The headless half of the iOS roles scenario: each role publishes the AccessKit role the iOS
/// table composes with `accesskit_ios`'s traits and the bridge's roles, and the scenario passes
/// through the iOS Simulator lane reading the roles page written out as the Simulator would serve
/// it, so a role the table gets wrong fails its row. On a Simulator it reads the Mac's
/// accessibility API on the device's window
/// (`ios_simulator::the_ios_roles_scenario_runs_through_the_simulators_window`).
#[test]
fn the_ios_roles_scenario_passes_through_the_lane_on_the_headless_app() {
    let mut driver = headless(|| Roles);
    let app = driver
        .main_window()
        .and_then(|mut window| window.read())
        .expect("the app's tree");
    let mut nodes = Vec::new();
    collect_nodes(&app.roots, &mut nodes);
    for row in ios_roles::ROLES {
        let label = role_label(row.role);
        let node = nodes
            .iter()
            .find(|node| {
                node.name.as_deref() == Some(&label) || node.description.as_deref() == Some(&label)
            })
            .unwrap_or_else(|| panic!("{label:?} is published"));
        assert_eq!(node.native_role, row.accesskit, "{label}");
    }
    let (_dir, driver, _screen) =
        through_simulator(driver, crate::ios_simulator_tree::roles_page());
    let report = run("ios-roles", driver, roles);
    passed(&report);
    assert_eq!(report.lane(), Some(ios_simulator::LANE));
    let rows = report
        .checks()
        .iter()
        .filter(|check| {
            check
                .name
                .starts_with("the Simulator's accessibility bridge reads ")
        })
        .count();
    assert_eq!(rows, ios_roles::ROLES.len(), "{:#?}", report.checks());
}

/// The rows of the roles scenario through the iOS Simulator lane check what the bridge serves
/// against the table: a row whose element the bridge serves with another role fails.
#[test]
fn an_ios_role_row_fails_when_the_bridge_serves_another_role() {
    let mut page = crate::ios_simulator_tree::roles_page();
    page.elements[0].role = "AXGenericElement".to_owned();
    let (_dir, driver, _screen) = through_simulator(headless(|| Roles), page);
    let report = run("ios-roles", driver, roles);
    let failed: Vec<&str> = report
        .checks()
        .iter()
        .filter(|check| check.outcome != Outcome::Passed)
        .map(|check| check.name.as_str())
        .collect();
    assert!(
        failed.contains(&"the Simulator's accessibility bridge reads button as AXButton"),
        "{failed:?}"
    );
}

/// The headless half of the iOS accessibility scenario: through the iOS Simulator lane, reading
/// the controls page written out as the Simulator would serve it after one like, the OS tree
/// shows the button with the app's identifier and projects to what the app published, and the
/// activation presses the button's element through the bridge. The scripted window does not
/// reach the headless app, so the count the activation would raise stays where it was and that
/// row fails on its effect alone; on a Simulator the press reaches the app
/// (`ios_simulator::the_ios_accessibility_scenario_runs_through_the_simulators_window`).
#[test]
fn the_ios_accessibility_scenario_passes_through_the_lane_but_the_activations_effect() {
    let (_dir, driver, screen) = through_simulator(
        headless(|| Controls),
        crate::ios_simulator_tree::controls_page(),
    );
    let report = run("ios-a11y", driver, accessibility);
    let (activation, rest): (Vec<&Check>, Vec<&Check>) = report
        .checks()
        .iter()
        .partition(|check| check.name == "like is invoked through the OS accessibility API");
    let failed: Vec<&&Check> = rest
        .iter()
        .filter(|check| check.outcome != Outcome::Passed)
        .collect();
    assert_eq!(failed, Vec::<&&Check>::new());
    // Five checks and the scenario's own row.
    assert_eq!(rest.len(), 6, "{rest:#?}");
    match activation.as_slice() {
        [check] => assert!(
            matches!(&check.outcome, Outcome::Failed { reason } if reason.contains("Likes: 2")),
            "{check:?}"
        ),
        other => panic!("one activation row: {other:#?}"),
    }
    let pressed = screen.presses();
    assert!(
        matches!(pressed.as_slice(), [like] if like.role == "AXButton"
            && like.description.as_deref() == Some("Like")),
        "the activation pressed the button's element: {pressed:?}"
    );
}

/// The iOS input scenario on the headless target, synthetic; through the iOS Simulator lane,
/// which sends no input, its rows are `cannot_verify` (`ios_simulator`).
#[test]
fn the_ios_input_scenario_passes_on_the_headless_target() {
    passed(&run("ios-input", headless(|| Controls), ios_input));
}

/// A portrait iPhone's window, 393 by 852, with `top` and `bottom` safe-area insets as its host
/// reports them.
fn phone(top: f64, bottom: f64) -> Driver {
    let insets = WindowInsets::ZERO
        .try_with_safe_area(EdgeInsets::new(top, 0.0, bottom, 0.0))
        .expect("finite insets");
    let launch = Launch::headless(|| Insets)
        .window_size(393, 852)
        .window_insets(insets)
        .expect("a headless window takes its insets");
    Driver::launch(launch).expect("the headless app starts")
}

/// The placement the agent serves states the safe area the host reports, side by side, and the
/// platform-neutral conventions a headless window follows unless it names a platform.
#[test]
fn a_placement_states_the_safe_area_the_host_reports() {
    let insets = WindowInsets::ZERO
        .try_with_safe_area(EdgeInsets::new(59.0, 3.0, 34.0, 7.0))
        .expect("finite insets");
    let launch = Launch::headless(|| Insets)
        .window_size(393, 852)
        .window_insets(insets)
        .expect("a headless window takes its insets");
    let mut driver = Driver::launch(launch).expect("the headless app starts");
    let placement = driver
        .main_window()
        .and_then(|mut window| window.placement())
        .expect("the window's placement");
    assert_eq!(
        placement.safe_area,
        Some(LogicalInsets::new(59.0, 3.0, 34.0, 7.0)),
        "{placement:?}"
    );
    assert_eq!(placement.platform, Some(Platform::Neutral), "{placement:?}");
}

/// A native window's insets are its OS's: a launch of anything but a headless view refuses them.
#[test]
fn only_a_headless_launch_takes_window_insets() {
    let refused = Launch::executable("app").window_insets(WindowInsets::ZERO);
    assert!(
        matches!(&refused, Err(Error::InvalidArgument { message, .. }) if message.contains("headless")),
        "{refused:?}"
    );
}

/// The safe-area scenario in a window whose host reports an iPhone's notch and home indicator,
/// so the offset it measures is the padding and not zero; on a simulator it reads the padding
/// UIKit gives (`ios_simulator`).
#[test]
fn the_safe_area_scenario_measures_a_padded_window_on_the_headless_target() {
    let mut driver = phone(59.0, 34.0);
    let tree = driver
        .main_window()
        .and_then(|mut window| window.read())
        .expect("the tree");
    assert_eq!(
        find(&tree.roots, "insets").and_then(|node| node.name.as_deref()),
        Some("59 0 34 0"),
        "the app reads the window's padding"
    );
    let report = run("ios-safe-area", driver, safe_area);
    assert_eq!(report.verdict(), Verdict::Pass, "{:#?}", report.checks());
    assert!(
        report
            .checks()
            .iter()
            .any(|check| check.name.contains("SafeArea") && check.outcome == Outcome::Passed),
        "{:#?}",
        report.checks()
    );
}

/// A portrait window that reports no safe area fails the scenario: a padding that never arrived
/// reads as zero, as does a `SafeArea` that ignored it.
#[test]
fn the_safe_area_scenario_fails_a_portrait_window_that_reports_no_inset() {
    for (top, bottom) in [(0.0, 34.0), (0.0, 0.0)] {
        let report = run("ios-safe-area", phone(top, bottom), safe_area);
        assert_eq!(report.verdict(), Verdict::Fail, "{:#?}", report.checks());
        let failed: Vec<&Check> = report
            .checks()
            .iter()
            .filter(|check| matches!(check.outcome, Outcome::Failed { .. }))
            .collect();
        assert!(
            matches!(
                failed.as_slice(),
                [check] if check.name == "a SafeArea insets its child by the padding the window reports"
                    && matches!(&check.outcome, Outcome::Failed { reason }
                        if reason.contains("a top inset in a portrait window"))
            ),
            "the safe-area check fails for the missing top inset: {failed:#?}"
        );
    }
}

/// The conformance row a platform's run reports: here on the headless target, through the OS lane
/// on Windows (`windows_desktop::the_conformance_scenario_runs_through_the_os_lane`).
#[test]
fn the_conformance_scenario_passes_on_the_headless_target() {
    let report = run("conformance", headless(|| Controls), conformance);
    passed(&report);
    assert_eq!(report.lane(), None, "the headless run takes no OS lane");
}
