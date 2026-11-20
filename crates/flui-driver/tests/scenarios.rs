//! The scenarios that replace the Windows, macOS and X11 device checks, written once on
//! `flui-driver` and run on the headless target here: synthetic input by default, so CI holds
//! them. On Windows, macOS and Linux the same functions run against the same apps in a window,
//! through the OS lane as their certification variant (`windows_desktop`, `macos_desktop`,
//! `x11_desktop`).
//!
//! - [`gallery`]: every control of the gallery reached by its identifier and acted on;
//! - [`accessibility`]: what the app publishes for its controls, an action through the semantic
//!   path, and, through an OS lane, the OS tree projected against the app's and an activation
//!   through the OS accessibility API;
//! - [`roles`]: each role of the role table publishes the wire role UI Automation folds it to,
//!   and through an OS lane reads back as the native role of the accessibility API the lane reads
//!   through (UI Automation's control type, the `AXRole`, the AT-SPI role);
//! - [`input`] and [`macos_input`]: the pointers, the wheel and keys reach the app, read back from
//!   its journal, and a press beside a control leaves it alone;
//! - [`notes`] and [`macos_notes`]: text, a select-all chord and an input-method composition in a
//!   text field;
//! - [`web_input`]: [`input`] and a composition into a field, through the session's path, which in
//!   headless Chrome is the Chrome lane (`chrome_browser`);
//! - [`conformance`]: a button's name, role and identifier, its activation and the effect read
//!   back: the conformance row a platform's run reports.

use flui_driver::lane::AccessibilityApi;
use flui_driver::lane::atspi::roles as atspi_roles;
use flui_driver::lane::macos::roles as ax_roles;
use flui_driver::lane::windows::roles::{ROLES, expected_wire_role};
use flui_driver::protocol::geometry::LogicalPoint;
use flui_driver::protocol::input::{
    ActionSequence, DeviceClass, ImeAction, InputKind, InputSource, PointerAction, PointerButton,
    PointerDevice, ScrollUnit, WheelAction,
};
use flui_driver::protocol::semantics::SemanticsRole;
use flui_driver::protocol::tree::Node;
use flui_driver::protocol::wire::Role;
use flui_driver::report::Check;
use flui_driver::{
    Driver, Error, InputPath, Launch, Locator, Outcome, Report, Result, Verdict, expect,
};
use flui_platform::TargetPlatform;
use flui_view::environment::PlatformConvention;

use crate::apps::{Controls, Notes, Roles, role_label};

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
    )
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
    )
}

/// [`input`] with the pointers `devices`, each read back from the journal as its class.
fn input_with(
    driver: &mut Driver,
    report: &mut Report,
    devices: &[(PointerDevice, DeviceClass)],
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
        let beside = LogicalPoint::new(f64::from(WINDOW.0) - 20.0, f64::from(WINDOW.1) - 20.0);
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
    report.run_scenario(name, &mut driver, scenario);
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

/// The conformance row a platform's run reports: here on the headless target, through the OS lane
/// on Windows (`windows_desktop::the_conformance_scenario_runs_through_the_os_lane`).
#[test]
fn the_conformance_scenario_passes_on_the_headless_target() {
    let report = run("conformance", headless(|| Controls), conformance);
    passed(&report);
    assert_eq!(report.lane(), None, "the headless run takes no OS lane");
}
