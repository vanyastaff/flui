//! The Android lane against a real app on a device or an emulator: the `android-input`,
//! `android-a11y` and `android-roles` scenarios through the OS lane, the app started from its APK
//! with its endpoint as intent extras and driven over the forwarded port.
//!
//! The run takes only what it is named: the device by its `adb` serial (`FLUI_ANDROID_SERIAL`),
//! the scenario app's APK (`FLUI_ANDROID_SCENARIO_APK`, the `scenario_app` example packaged by
//! `flui-cli`'s Android build) and its application id (`FLUI_ANDROID_SCENARIO_PACKAGE`), and for
//! the roles the same app built to show its roles page (`FLUI_ANDROID_ROLES_APK`,
//! `FLUI_ANDROID_ROLES_PACKAGE`). Without them it asks `adb` nothing and comes out
//! `cannot_verify`, naming what is missing; `FLUI_REQUIRE_ANDROID=1` makes that a failure, for a
//! run on a device. The accessibility reads take UI Automator's dump, which the lane refuses while
//! any accessibility service runs, so they run with every service off.

use std::time::Duration;

use flui_driver::lane::android::{self, AndroidApp};
use flui_driver::{Error, InputPath, InputPolicy, Outcome, Report, Result, Scenario, Verdict};

use crate::scenarios;

/// How long the app may take to start and bind its endpoint.
const START: Duration = Duration::from_secs(60);

/// The value of the environment variable `name`, or the cannot-verify failure naming it.
fn named(name: &str) -> Result<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Lane {
            lane: android::LANE.to_owned(),
            unavailable: true,
            message: format!("{name} is not set"),
        })
}

/// The app the variables `package` and `apk` name started on the named device, driven through the
/// Android lane.
fn session_of(package: &str, apk: &str) -> Result<flui_driver::Driver> {
    let adb = android::host_device()?;
    let app = AndroidApp::try_new(named(package)?, "android.app.NativeActivity")?.apk(named(apk)?);
    let mut driver = android::launch(Box::new(adb), &app, START)?;
    driver.set_input_policy(InputPolicy::require(InputPath::Os));
    Ok(driver)
}

/// The scenario app (its controls) started on the named device.
fn session() -> Result<flui_driver::Driver> {
    session_of("FLUI_ANDROID_SCENARIO_PACKAGE", "FLUI_ANDROID_SCENARIO_APK")
}

/// `scenario` run as `name` on `session`, or the check that says why the app did not start.
fn report_of(
    name: &str,
    session: Result<flui_driver::Driver>,
    scenario: impl FnOnce(&mut flui_driver::Driver, &mut Report) -> Result<()>,
) -> Report {
    let mut report = Report::new();
    match session {
        Ok(mut driver) => {
            driver
                .declare(Scenario::new(name).certification())
                .expect("the scenario is declared");
            report.run_scenario(name, &mut driver, scenario);
        }
        Err(error) => {
            let _ = report.check("the scenario app starts on the device", || -> Result<()> {
                Err(error)
            });
        }
    }
    report
}

fn required() -> bool {
    std::env::var_os("FLUI_REQUIRE_ANDROID").is_some_and(|value| value == "1")
}

/// A run without a device, or without the variables naming the app, is `cannot_verify` and says
/// why; it never fails.
fn unverified_says_why(report: &Report) {
    assert_ne!(report.verdict(), Verdict::Fail, "{:#?}", report.checks());
    if report.verdict() == Verdict::CannotVerify {
        assert!(
            report
                .checks()
                .iter()
                .any(|check| matches!(check.outcome, Outcome::CannotVerify { .. })),
            "a run that could not be verified says why: {:#?}",
            report.checks()
        );
    }
}

/// The roles scenario through the Android lane: the node the lane matches to each role's reads
/// back as the class `accesskit_android` gives it, in the dump UI Automator takes with no
/// accessibility service on. It takes
/// the scenario app built to show its roles page: `FLUI_ANDROID_ROLES_APK` and
/// `FLUI_ANDROID_ROLES_PACKAGE`.
#[test]
fn the_android_roles_scenario_runs_through_the_os_lane() {
    let report = report_of(
        "android-roles",
        session_of("FLUI_ANDROID_ROLES_PACKAGE", "FLUI_ANDROID_ROLES_APK"),
        scenarios::roles,
    );
    if required() {
        assert_eq!(report.verdict(), Verdict::Pass, "{:#?}", report.checks());
    } else {
        unverified_says_why(&report);
    }
}

/// The accessibility scenario through the Android lane: the button's role, name and identifier
/// read from UI Automator's dump, matched to the app's tree, and the projections equal. Its
/// activation through the accessibility API is `cannot_verify` (`os_invoke`) on every device: the
/// lane performs no accessibility action.
#[test]
fn the_android_accessibility_scenario_runs_through_the_os_lane() {
    let report = report_of("android-a11y", session(), scenarios::accessibility);
    if !required() {
        unverified_says_why(&report);
        return;
    }
    for check in report.checks() {
        let expected_cannot_verify =
            check.name == "like is invoked through the OS accessibility API";
        match &check.outcome {
            Outcome::Passed => assert!(!expected_cannot_verify, "{check:?}"),
            Outcome::CannotVerify {
                capability: Some(capability),
                ..
            } if expected_cannot_verify && capability == "os_invoke" => {}
            other => panic!("{}: {other:?}", check.name),
        }
    }
}

#[test]
fn the_android_input_scenario_runs_through_the_os_lane() {
    let report = report_of("android-input", session(), scenarios::android_input);
    if required() {
        assert_eq!(report.verdict(), Verdict::Pass, "{:#?}", report.checks());
    } else {
        unverified_says_why(&report);
    }
}
