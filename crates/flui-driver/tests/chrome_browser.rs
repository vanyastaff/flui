//! The `web-input` scenario in a real headless Chrome, through the Chrome lane: the scenario app's
//! web build (`FLUI_WEB_INPUT_BUNDLE`, the directory `flui build` writes for the web target of
//! `examples/scenario_app.rs`'s `Controls`) launched by `Driver::launch`, which serves it, starts
//! the ChromeDriver `CHROMEDRIVER` names (the test never lets the lane fetch one) and opens Chrome
//! headless, so no window reaches the desktop.
//!
//! A host without the build, Chrome or ChromeDriver reports the scenario `cannot_verify`, never a
//! pass and never a silent skip (R33); `FLUI_REQUIRE_CHROME` turns that into a failure, for a job
//! that provides them.

use flui_driver::{Artifact, Driver, InputPath, InputPolicy, Launch, Report, Verdict};

use crate::scenarios;

#[test]
fn the_web_input_scenario_runs_in_headless_chrome() {
    let mut report = Report::new();
    let required = std::env::var_os("FLUI_REQUIRE_CHROME").is_some();
    let Some(bundle) = std::env::var_os("FLUI_WEB_INPUT_BUNDLE") else {
        report.not_run(
            "web-input",
            "no web build of the scenario app: set FLUI_WEB_INPUT_BUNDLE",
        );
        assert!(
            !required,
            "FLUI_REQUIRE_CHROME is set and there is no build"
        );
        assert_eq!(report.verdict(), Verdict::CannotVerify);
        return;
    };
    // Never fetched here: a test reaches no network, so only `CHROMEDRIVER` supplies it.
    if std::env::var_os("CHROMEDRIVER").is_none() {
        report.not_run("web-input", "no ChromeDriver named: set CHROMEDRIVER");
        assert!(
            !required,
            "FLUI_REQUIRE_CHROME is set and there is no ChromeDriver"
        );
        assert_eq!(report.verdict(), Verdict::CannotVerify);
        return;
    }
    let launched = Driver::launch(
        Launch::artifact(Artifact::WasmBundle(bundle.into()))
            .viewport(scenarios::WINDOW.0, scenarios::WINDOW.1),
    );
    let mut driver = match launched {
        Ok(driver) => driver.with_input_policy(InputPolicy::require(InputPath::Os)),
        Err(error) if error.is_cannot_verify() && !required => {
            report.not_run("web-input", error.to_string());
            assert_eq!(report.verdict(), Verdict::CannotVerify);
            return;
        }
        Err(error) => panic!("the web build did not start in headless Chrome: {error}"),
    };
    report.run_scenario("web-input", &mut driver, scenarios::web_input);
    assert_eq!(report.lane(), Some("chrome"));
    assert_eq!(report.verdict(), Verdict::Pass, "{:#?}", report.checks());
}
