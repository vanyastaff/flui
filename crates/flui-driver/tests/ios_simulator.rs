//! The iOS Simulator lane against a real app on a booted simulator: the app installed and started
//! through `simctl` with its socket and token as launch values, driven over that socket, its
//! window captured by the simulator, and the `ios-*` scenarios run on it.
//!
//! The run takes only what it is named: the simulator by its UDID (`FLUI_IOS_SIMULATOR_UDID`),
//! the scenario apps built for the simulator (`FLUI_IOS_SCENARIO_APP`, an `.app` serving the
//! apps of `tests/apps.rs` as `examples/scenario_app.rs` does, picked by
//! `FLUI_DRIVER_SCENARIO_APP`; that example builds for the desktops alone, since its `flui-app`
//! dev-dependency is theirs) and its bundle identifier (`FLUI_IOS_SCENARIO_BUNDLE`). Without them
//! it asks `xcrun` nothing and comes out `cannot_verify`, naming what is missing;
//! `FLUI_REQUIRE_IOS_SIMULATOR=1` makes that a failure, for a run on a Mac.
//!
//! The lane sends no input ([`flui_driver::lane::ios_simulator::xcode`]), so the input scenario
//! runs twice: on the synthetic path, where it passes, and through the lane, where each row is
//! `cannot_verify` naming `ios_simulator_hid`.

use std::time::Duration;

use flui_driver::lane::ios_simulator::xcode::HID_CAPABILITY;
use flui_driver::lane::ios_simulator::{self, LANE, SimulatorApp};
use flui_driver::pixels::CaptureSource;
use flui_driver::protocol::report::{Outcome, Report};
use flui_driver::protocol::verdict::Verdict;
use flui_driver::{Driver, Error, InputPath, InputPolicy, Result, Scenario};

use crate::scenarios;

/// How long the app may take to start and bind its endpoint.
const START: Duration = Duration::from_secs(90);

/// The value of the environment variable `name`, or the cannot-verify failure naming it.
fn named(name: &str) -> Result<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Lane {
            lane: LANE.to_owned(),
            unavailable: true,
            message: format!("{name} is not set"),
        })
}

/// The scenario app `app` started on the named simulator, driven through the iOS Simulator lane.
fn session(app: &str) -> Result<Driver> {
    let (xcrun, udid) = ios_simulator::host_simulator()?;
    let scenario_app = SimulatorApp::try_new(named("FLUI_IOS_SCENARIO_BUNDLE")?)?
        .app_bundle(named("FLUI_IOS_SCENARIO_APP")?)
        .env("FLUI_DRIVER_SCENARIO_APP", app)?;
    ios_simulator::launch(Box::new(xcrun), &udid, &scenario_app, START)
}

/// The simulator's image of the window: the window's size at its scale, showing the app's
/// controls rather than one flat colour.
fn captured(driver: &mut Driver) -> Result<()> {
    let mut window = driver.main_window()?;
    let placement = window.placement()?;
    let capture = window.capture(CaptureSource::Os)?;
    let image = capture.image();
    let device = |logical: f64| (logical * placement.scale).round() as u32;
    let size = (device(placement.size.width), device(placement.size.height));
    if (
        image.width().abs_diff(size.0),
        image.height().abs_diff(size.1),
    ) > (2, 2)
    {
        return Err(Error::Expectation {
            expected: format!("a capture of {} x {}", size.0, size.1),
            actual: format!("{} x {}", image.width(), image.height()),
        });
    }
    let first = image.pixel(0, image.height() / 2);
    let differs = (0..image.height())
        .step_by(4)
        .flat_map(|y| (0..image.width()).step_by(4).map(move |x| (x, y)))
        .any(|(x, y)| image.pixel(x, y) != first);
    if differs {
        Ok(())
    } else {
        Err(Error::Expectation {
            expected: "the app's controls".to_owned(),
            actual: "one flat colour".to_owned(),
        })
    }
}

#[test]
fn the_ios_scenarios_run_on_a_simulator() {
    let required = std::env::var_os("FLUI_REQUIRE_IOS_SIMULATOR").is_some_and(|value| value == "1");
    let mut synthetic = Report::new();
    let mut os = Report::new();
    let mut started = false;
    match session("controls") {
        Ok(mut driver) => {
            started = true;
            driver.run_scenario(&mut synthetic, "ios-input", scenarios::ios_input);
            let _ = synthetic.check("the simulator captures the app's window", || {
                captured(&mut driver)
            });
            driver.set_input_policy(InputPolicy::require(InputPath::Os));
            driver
                .declare(Scenario::new("ios-input").certification())
                .expect("the scenario is declared");
            driver.run_scenario(&mut os, "ios-input", scenarios::ios_input);
        }
        Err(error) => {
            let _ = synthetic.check(
                "the scenario app starts on the simulator",
                || -> Result<()> { Err(error) },
            );
        }
    }
    match session("insets") {
        Ok(mut driver) => {
            driver.run_scenario(&mut synthetic, "ios-safe-area", scenarios::safe_area);
        }
        Err(error) => {
            let _ = synthetic.check(
                "the safe-area app starts on the simulator",
                || -> Result<()> { Err(error) },
            );
        }
    }
    if required {
        assert_eq!(
            synthetic.verdict(),
            Verdict::Pass,
            "{:#?}",
            synthetic.checks()
        );
    } else {
        assert_ne!(
            synthetic.verdict(),
            Verdict::Fail,
            "{:#?}",
            synthetic.checks()
        );
    }
    if started {
        // Every row through the lane is refused for the input it would send, never passed or
        // failed on input that never went out.
        let refused = |check: &&flui_driver::protocol::report::Check| {
            matches!(
                &check.outcome,
                Outcome::CannotVerify { capability: Some(capability), .. }
                    if capability == HID_CAPABILITY
            )
        };
        assert!(
            !os.checks().is_empty() && os.checks().iter().all(|check| refused(&check)),
            "{:#?}",
            os.checks()
        );
    }
}
