//! Where the live Windows tests may run: on a desktop of their own, unless the person chose the
//! one they work at.
//!
//! The tests of `windows_desktop` open windows, install input hooks and, on the input desktop,
//! send real input. nextest starts each test on a desktop of its own, `flui-test-<run id>`
//! (`docs/testing.md`, "The Windows test desktop"; the wrapper is `tools/xtask`'s `test-desktop`,
//! and `flui-native`'s `windows_test_desktop` checks the same name); libtest, which `cargo test`
//! and cargo-gamma use, starts them on `Default`, the desktop the person is working at. Each live
//! test's setup (`session`, `live_guard`, `live_job` of `windows_desktop`) asks [`require`] first,
//! so a run off nextest stops before it opens a window.
//!
//! The question is the desktop's name, not whether it takes input at the moment: a locked
//! workstation reports `Default` as taking none, and a test in flight there would send into the
//! session when the person unlocks it. `FLUI_TEST_VISIBLE_DESKTOP=1` is the opt-in the wrapper
//! honours for the same purpose.

use std::ffi::OsStr;

/// The opt-in that lets live tests run on the desktop the person works at.
const VISIBLE: &str = "FLUI_TEST_VISIBLE_DESKTOP";

/// The run id the wrapper names its desktop with when nextest gave none.
const MANUAL_RUN: &str = "manual";

/// Stops a live test that is not on a desktop of its own and whose person has not asked for it:
/// a test off nextest, or one whose desktop Windows would not name.
#[cfg(windows)]
pub(crate) fn require() {
    let run = std::env::var("NEXTEST_RUN_ID").ok();
    let desktop = flui_driver::lane::windows::thread_desktop_name();
    let visible = std::env::var_os(VISIBLE);
    if let Err(refusal) = decide(desktop.as_deref(), run.as_deref(), visible.as_deref()) {
        panic!("{refusal}");
    }
}

/// Whether a test on the desktop named `desktop` (`None` when Windows would not say), in the
/// nextest run `run` (`None` outside nextest), may run, given what `FLUI_TEST_VISIBLE_DESKTOP`
/// holds. Only the run's own desktop, named as the wrapper names it, or the opt-in allows it; the
/// refusal says what to run instead.
fn decide(desktop: Option<&str>, run: Option<&str>, visible: Option<&OsStr>) -> Result<(), String> {
    let own = format!("flui-test-{}", run.unwrap_or(MANUAL_RUN));
    if desktop == Some(own.as_str()) || visible.is_some_and(|value| value == "1") {
        return Ok(());
    }
    let found = desktop.map_or_else(
        || "a desktop Windows would not name".to_owned(),
        |name| format!("the desktop {name:?}"),
    );
    Err(format!(
        "this test opens windows and may send input, and it runs on {found}, not on the desktop \
         {own:?} of its own: run it with `cargo nextest run`, which starts each test there \
         (docs/testing.md, \"The Windows test desktop\"), or set {VISIBLE}=1 to run it where it is \
         on purpose"
    ))
}

#[test]
fn a_live_test_runs_on_the_runs_own_desktop_or_where_the_person_chose() {
    let one = OsStr::new("1");
    let rows = [
        (
            Some("flui-test-r1"),
            Some("r1"),
            None,
            true,
            "the run's own desktop",
        ),
        (
            Some("flui-test-r1"),
            Some("r1"),
            Some(one),
            true,
            "the run's own, opted in",
        ),
        (
            Some("flui-test-manual"),
            None,
            None,
            true,
            "the wrapper's desktop outside nextest",
        ),
        (
            Some("Default"),
            Some("r1"),
            None,
            false,
            "the person's desktop",
        ),
        (
            Some("Default"),
            None,
            None,
            false,
            "the person's desktop, outside nextest",
        ),
        (
            Some("Default"),
            Some("r1"),
            Some(one),
            true,
            "the person's desktop, opted in",
        ),
        (
            Some("Default"),
            Some("r1"),
            Some(OsStr::new("0")),
            false,
            "opted out",
        ),
        (
            Some("Default"),
            Some("r1"),
            Some(OsStr::new("")),
            false,
            "an empty opt-in",
        ),
        (
            Some("Default"),
            Some("r1"),
            Some(OsStr::new("true")),
            false,
            "an opt-in that is not 1",
        ),
        (
            Some("flui-test-r2"),
            Some("r1"),
            None,
            false,
            "another run's desktop",
        ),
        (
            Some("Winlogon"),
            Some("r1"),
            None,
            false,
            "the lock screen's desktop",
        ),
        (
            Some("other"),
            None,
            None,
            false,
            "a desktop of another name",
        ),
        (
            None,
            Some("r1"),
            None,
            false,
            "a desktop Windows would not name",
        ),
        (
            None,
            Some("r1"),
            Some(one),
            true,
            "an unnamed desktop, opted in",
        ),
    ];
    for (desktop, run, visible, allowed, row) in rows {
        let decided = decide(desktop, run, visible);
        assert_eq!(decided.is_ok(), allowed, "{row}: {decided:?}");
    }
    let refusal = decide(Some("Default"), Some("r1"), None).expect_err("the person's desktop");
    assert!(
        refusal.contains("cargo nextest run")
            && refusal.contains(VISIBLE)
            && refusal.contains("flui-test-r1"),
        "the refusal names nextest, the opt-in and the desktop to use: {refusal}"
    );
}

/// The body of the function `name` in `source`: from its signature to the first line that is a
/// lone closing brace.
fn body<'a>(source: &'a str, name: &str) -> &'a str {
    let signature = format!("\nfn {name}(");
    let start = source
        .find(&signature)
        .unwrap_or_else(|| panic!("windows_desktop.rs has no `fn {name}`"));
    let rest = &source[start..];
    let end = rest.find("\n}\n").expect("the function ends");
    &rest[..end]
}

/// What opens a window, installs the hooks or starts a job in `windows_desktop` is built in one
/// setup function each, and each of those asks `private_desktop::require` first: a test that
/// built its own would run on the person's desktop under libtest without being asked. A runtime
/// test cannot show this under nextest, where `require` passes, so the source is read.
#[test]
fn the_live_setups_ask_before_they_open_anything() {
    let source = include_str!("windows_desktop.rs");
    for (setup, built) in [
        ("session", "Driver::launch("),
        ("live_guard", "WindowsGuard::new("),
        ("live_job", "KillOnCloseJob::try_new("),
    ] {
        let function = body(source, setup);
        let asked = function
            .find("private_desktop::require()")
            .unwrap_or_else(|| panic!("`{setup}` does not ask `private_desktop::require`"));
        let opened = function
            .find(built)
            .unwrap_or_else(|| panic!("`{setup}` does not build with `{built}`"));
        assert!(asked < opened, "`{setup}` asks after it has built");
        assert_eq!(
            source.matches(built).count(),
            1,
            "`{built}` is built outside `{setup}`: a live test must go through the setup"
        );
    }
}
