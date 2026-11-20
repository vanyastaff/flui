//! The Android lane against a real app on a device or an emulator: the `android-input` scenario
//! through the OS lane, the app started from its APK with its endpoint as intent extras and driven
//! over the forwarded port.
//!
//! The run takes only what it is named: the device by its `adb` serial (`FLUI_ANDROID_SERIAL`),
//! the scenario app's APK (`FLUI_ANDROID_SCENARIO_APK`, the `scenario_app` example packaged by
//! `flui-cli`'s Android build) and its application id (`FLUI_ANDROID_SCENARIO_PACKAGE`). Without
//! them it asks `adb` nothing and comes out `cannot_verify`, naming what is missing;
//! `FLUI_REQUIRE_ANDROID=1` makes that a failure, for a run on a device.

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

/// The scenario app started on the named device, driven through the Android lane.
fn session() -> Result<flui_driver::Driver> {
    let adb = android::host_device()?;
    let app = AndroidApp::try_new(
        named("FLUI_ANDROID_SCENARIO_PACKAGE")?,
        "android.app.NativeActivity",
    )?
    .apk(named("FLUI_ANDROID_SCENARIO_APK")?);
    let mut driver = android::launch(Box::new(adb), &app, START)?;
    driver.set_input_policy(InputPolicy::require(InputPath::Os));
    Ok(driver)
}

#[test]
fn the_android_input_scenario_runs_through_the_os_lane() {
    let mut report = Report::new();
    match session() {
        Ok(mut driver) => {
            driver
                .declare(Scenario::new("android-input").certification())
                .expect("the scenario is declared");
            report.run_scenario("android-input", &mut driver, scenarios::android_input);
        }
        Err(error) => {
            let _ = report.check("the scenario app starts on the device", || -> Result<()> {
                Err(error)
            });
        }
    }
    let required = std::env::var_os("FLUI_REQUIRE_ANDROID").is_some_and(|value| value == "1");
    if required {
        assert_eq!(report.verdict(), Verdict::Pass, "{:#?}", report.checks());
    } else {
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
}
