//! Where nextest starts a Windows test: on the run's own desktop, which takes no input, or,
//! under `FLUI_TEST_VISIBLE_DESKTOP=1`, on the caller's (ADR-0355). The wrapper in
//! `.config/nextest.toml` is what puts it there; without it this test runs where nextest runs.

#![cfg(target_os = "windows")]

#[test]
fn nextest_starts_a_test_on_the_runs_own_desktop() {
    let Ok(run) = std::env::var("NEXTEST_RUN_ID") else {
        eprintln!("not started by nextest: no test desktop to check");
        return;
    };
    let desktop = crate::this_threads_desktop();
    if std::env::var_os("FLUI_TEST_VISIBLE_DESKTOP").is_some_and(|visible| visible == "1") {
        assert!(
            !desktop.name.starts_with("flui-test-"),
            "FLUI_TEST_VISIBLE_DESKTOP=1 starts a test on the caller's desktop, not on {:?}",
            desktop.name
        );
        return;
    }
    assert_eq!(
        desktop.name,
        format!("flui-test-{run}"),
        "nextest starts a Windows test on its run's desktop (the wrapper script in \
         .config/nextest.toml)"
    );
    assert!(
        !desktop.takes_input,
        "the run's desktop takes no input, so nobody's mouse or keyboard reaches the test"
    );
}
