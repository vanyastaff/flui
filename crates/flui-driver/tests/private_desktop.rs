//! Where the live Windows tests may run: not on the desktop that takes the user's input, unless
//! the person chose that.
//!
//! The tests of `windows_desktop` open windows, install input hooks and, on the input desktop,
//! send real input. nextest starts each test on a desktop of its own, which takes no input
//! (`docs/testing.md`, "The Windows test desktop"); libtest, which `cargo test` and cargo-gamma
//! use, starts it on the desktop the person is working at. Each live test's setup asks
//! [`require`] first, so a run off nextest stops before it opens a window.
//!
//! The question is the lane's own: [`desktop_takes_input`] reads whether the calling thread's
//! desktop takes the user's input, the same read `flui-native`'s window tests make.
//! `FLUI_TEST_VISIBLE_DESKTOP=1` is the opt-in nextest's wrapper honours for the same purpose.

use std::ffi::OsStr;

use flui_driver::lane::windows::desktop_takes_input;

/// The opt-in that lets live tests run on the desktop that takes the user's input.
const VISIBLE: &str = "FLUI_TEST_VISIBLE_DESKTOP";

/// Stops a live test that would run on the desktop taking the user's input without the person
/// having asked for it: a test off nextest, which gives each test a desktop of its own.
pub(crate) fn require() {
    if let Err(refusal) = decide(desktop_takes_input(), std::env::var_os(VISIBLE).as_deref()) {
        panic!("{refusal}");
    }
}

/// Whether a test may run on a desktop that does (`takes_input`) or does not take the user's
/// input, given what `FLUI_TEST_VISIBLE_DESKTOP` holds; the refusal says what to run instead.
fn decide(takes_input: bool, visible: Option<&OsStr>) -> Result<(), String> {
    if !takes_input || visible.is_some_and(|value| value == "1") {
        return Ok(());
    }
    Err(format!(
        "this test opens windows and may send input, and it runs on the desktop that takes the \
         user's input: run it with `cargo nextest run`, which starts each test on a desktop of its \
         own (docs/testing.md, \"The Windows test desktop\"), or set {VISIBLE}=1 to run it here \
         on purpose"
    ))
}

#[test]
fn a_live_test_runs_off_the_input_desktop_or_where_the_person_chose() {
    let one = OsStr::new("1");
    for (takes_input, visible, allowed, row) in [
        (false, None, true, "a desktop of its own"),
        (false, Some(one), true, "a desktop of its own, opted in"),
        (true, Some(one), true, "the input desktop, opted in"),
        (true, None, false, "the input desktop, no opt-in"),
        (
            true,
            Some(OsStr::new("0")),
            false,
            "the input desktop, opted out",
        ),
        (
            true,
            Some(OsStr::new("")),
            false,
            "the input desktop, an empty opt-in",
        ),
        (
            true,
            Some(OsStr::new("true")),
            false,
            "the input desktop, not a 1",
        ),
    ] {
        let decided = decide(takes_input, visible);
        assert_eq!(decided.is_ok(), allowed, "{row}: {decided:?}");
    }
    let refusal = decide(true, None).expect_err("the input desktop is refused");
    assert!(
        refusal.contains("cargo nextest run") && refusal.contains(VISIBLE),
        "the refusal names nextest and the opt-in: {refusal}"
    );
}
