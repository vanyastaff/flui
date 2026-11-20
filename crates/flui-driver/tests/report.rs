//! The report's verdict, its exit codes and its lists, and how a failure decides whether a
//! check failed or could not be made.

use flui_driver::protocol::act::{Effect, EffectKind};
use flui_driver::protocol::error::{ErrorCode, ErrorObject, Retry};
use flui_driver::report::{VERDICT_LINE, verdict_exit_code};
use flui_driver::{Check, Error, Isolation, Outcome, Report, Verdict};

fn passed(name: &str) -> Check {
    Check::new(name, Outcome::Passed)
}

fn failed(name: &str) -> Check {
    Check::from_result::<()>(
        name,
        &Err(Error::Expectation {
            expected: "1".into(),
            actual: "0".into(),
        }),
    )
}

fn unsupported(name: &str, capability: &str) -> Check {
    Check::from_result::<()>(
        name,
        &Err(Error::NotSupported {
            capability: Some(capability.into()),
            message: "missing".into(),
            retry: Retry::Never,
        }),
    )
}

#[test]
fn the_verdict_is_fail_then_cannot_verify_then_pass() {
    let interrupted = Check::from_result::<()>(
        "os tap",
        &Err(Error::InterruptedByUser {
            lane: "windows".into(),
            event: "a key press".into(),
        }),
    );
    let no_device = Check::from_result::<()>(
        "android tap",
        &Err(Error::Lane {
            lane: "android".into(),
            unavailable: true,
            message: "no device".into(),
        }),
    );
    let lane_failed = Check::from_result::<()>(
        "android tap",
        &Err(Error::Lane {
            lane: "android".into(),
            unavailable: false,
            message: "adb refused the event".into(),
        }),
    );
    let rows: Vec<(&str, Vec<Check>, Verdict, Vec<&str>)> = vec![
        ("nothing checked", vec![], Verdict::CannotVerify, vec![]),
        ("one pass", vec![passed("a")], Verdict::Pass, vec![]),
        (
            "a pass and a failure",
            vec![passed("a"), failed("b")],
            Verdict::Fail,
            vec![],
        ),
        (
            "a pass and a missing capability",
            vec![passed("a"), unsupported("b", "os_input")],
            Verdict::CannotVerify,
            vec!["os_input"],
        ),
        (
            "a failure outranks a missing capability",
            vec![unsupported("a", "os_input"), failed("b")],
            Verdict::Fail,
            vec!["os_input"],
        ),
        (
            "the user interrupting an OS run",
            vec![passed("a"), interrupted],
            Verdict::CannotVerify,
            vec!["uninterrupted_os_input"],
        ),
        (
            "a lane the host cannot run",
            vec![no_device],
            Verdict::CannotVerify,
            vec!["android_lane"],
        ),
        (
            "a lane that ran and failed",
            vec![lane_failed],
            Verdict::Fail,
            vec![],
        ),
        (
            "each missing capability listed once",
            vec![unsupported("a", "os_input"), unsupported("b", "os_input")],
            Verdict::CannotVerify,
            vec!["os_input"],
        ),
    ];
    for (row, checks, verdict, missing) in rows {
        let mut report = Report::new();
        for check in checks {
            report.push(check);
        }
        assert_eq!(report.verdict(), verdict, "{row}");
        assert_eq!(report.exit_code(), verdict_exit_code(verdict), "{row}");
        assert_eq!(report.cannot_verify(), missing, "{row}");
        let printed = report.to_string();
        assert_eq!(
            printed.lines().last(),
            Some(format!("{VERDICT_LINE}{}", verdict.name()).as_str()),
            "{row}: the last line is the verdict"
        );
        let json = report.to_json();
        assert_eq!(json["verdict"], verdict.name(), "{row}");
        assert_eq!(json["exit_code"], report.exit_code(), "{row}");
    }
    assert_eq!(
        [Verdict::Pass, Verdict::Fail, Verdict::CannotVerify].map(verdict_exit_code),
        [0, 1, 2]
    );
}

#[test]
fn a_check_asked_for_and_not_run_keeps_the_report_from_passing() {
    let mut report = Report::new();
    report.push(passed("layout"));
    report.not_run("contrast", "no background colour is known for the label");
    assert_eq!(report.verdict(), Verdict::CannotVerify);
    assert_eq!(report.checked().count(), 1);
    let not_checked: Vec<&str> = report
        .not_checked()
        .map(|check| check.name.as_str())
        .collect();
    assert_eq!(not_checked, ["contrast"]);
}

#[test]
fn a_scenario_that_checked_nothing_cannot_be_verified() {
    let mut report = Report::new();
    let mut driver = flui_driver::Driver::launch(flui_driver::Launch::headless(|| {
        flui_widgets::Text::new("nothing to check")
    }))
    .expect("the headless app starts");
    report.run_scenario("empty", &mut driver, |_, _| Ok(()));
    assert_eq!(report.verdict(), Verdict::CannotVerify, "{report}");
    let not_checked: Vec<&str> = report
        .not_checked()
        .map(|check| check.name.as_str())
        .collect();
    assert_eq!(not_checked, ["empty"]);

    let mut checked = Report::new();
    checked.run_scenario("one check", &mut driver, |driver, report| {
        report.check("reads the window", || driver.main_window().map(|_| ()))
    });
    assert_eq!(checked.verdict(), Verdict::Pass, "{checked}");
}

#[test]
fn the_report_names_the_isolation_and_the_announcements() {
    let mut report = Report::new();
    report.push(passed("tap"));
    report.set_isolation(Isolation::Shared);
    report.announce("OS input: will take mouse on this desktop for about 3 s");
    let printed = report.to_string();
    assert!(printed.contains("isolation: shared"), "{printed}");
    assert!(printed.contains("announced: OS input"), "{printed}");
    assert_eq!(report.to_json()["isolation"], "shared");
}

#[test]
fn an_error_reads_into_its_code_with_the_retry_and_effect_it_carries() {
    for &code in ErrorCode::ALL {
        let error = Error::from_wire(ErrorObject::new(code, "m"));
        assert_eq!(error.code(), Some(code), "{code}");
        assert_eq!(error.retry(), code.default_retry(), "{code}");
        // The retry the app said is kept, whatever the code's own would be.
        for said in [Retry::Never, Retry::Soon, Retry::WhenAppears] {
            let error = Error::from_wire(ErrorObject::new(code, "m").retry(said));
            assert_eq!(error.retry(), said, "{code} said {said}");
        }
    }
    let unknown: ErrorObject = serde_json::from_value(serde_json::json!({
        "code": "rate_limited", "message": "a code a newer app added", "retry": "soon"
    }))
    .expect("an unknown code still reads");
    let unknown = Error::from_wire(unknown);
    assert_eq!(unknown.code(), Some(ErrorCode::Platform));
    assert_eq!(unknown.retry(), Retry::Soon);

    let rows: Vec<(&str, Error, Retry, bool, bool)> = vec![
        (
            "busy",
            Error::from_wire(ErrorObject::new(ErrorCode::Busy, "m")),
            Retry::Soon,
            false,
            false,
        ),
        (
            "not found",
            Error::from_wire(ErrorObject::new(ErrorCode::NotFound, "m")),
            Retry::WhenAppears,
            false,
            false,
        ),
        (
            "a read that timed out, which the app says to ask again soon",
            Error::from_wire(ErrorObject::new(ErrorCode::Timeout, "m").retry(Retry::Soon)),
            Retry::Soon,
            false,
            false,
        ),
        (
            "an act that timed out",
            Error::from_wire(
                ErrorObject::new(ErrorCode::Timeout, "m")
                    .effect(Effect::new(EffectKind::MayHaveRun)),
            ),
            Retry::Never,
            true,
            false,
        ),
        (
            "a missing capability",
            Error::from_wire(ErrorObject::new(ErrorCode::NotSupported, "m")),
            Retry::Never,
            false,
            true,
        ),
        (
            "the driver's own transport",
            Error::Protocol {
                message: "m".into(),
            },
            Retry::Never,
            false,
            false,
        ),
    ];
    for (row, error, retry, may_have_run, cannot_verify) in rows {
        assert_eq!(error.retry(), retry, "{row}");
        assert_eq!(error.may_have_run(), may_have_run, "{row}");
        assert_eq!(error.is_cannot_verify(), cannot_verify, "{row}");
    }
}
