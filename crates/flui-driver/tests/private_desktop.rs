//! Where the live Windows tests may run: on a desktop of their own, unless the person chose the
//! one they work at.
//!
//! The tests of `windows_desktop` open windows, install input hooks and, on the input desktop,
//! send real input. nextest starts each test on a desktop of its own, `flui-test-<run id>`
//! (`docs/testing.md`, "The Windows test desktop"; the wrapper is `tools/test-desktop`,
//! and `flui-native`'s `windows_test_desktop` checks the same name); libtest, which `cargo test`
//! and cargo-gamma use, starts them on `Default`, the desktop the person is working at. A live
//! test asks [`require`] first and passes the [`LiveDesktop`] it returns to the setup that opens
//! a window, installs the hooks, starts a job or sends input, so a run off nextest stops before
//! any of that, and a setup without the check does not compile.
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

/// The proof that [`require`] ran on this thread's desktop: a live setup takes `&LiveDesktop`, so
/// one that skips the check does not compile. Only [`require`] makes one.
#[cfg(windows)]
pub(crate) struct LiveDesktop(());

/// Stops a live test that is not on a desktop of its own and whose person has not asked for it:
/// a test off nextest, or one whose desktop Windows would not name.
#[cfg(windows)]
pub(crate) fn require() -> LiveDesktop {
    let run = std::env::var("NEXTEST_RUN_ID").ok();
    let desktop = this_threads_desktop_name();
    let visible = std::env::var_os(VISIBLE);
    if let Err(refusal) = decide(desktop.as_deref(), run.as_deref(), visible.as_deref()) {
        panic!("{refusal}");
    }
    LiveDesktop(())
}

/// The name of the calling thread's desktop (`Default` for the one a person works at, a locked
/// workstation's included); `None` when Windows refuses either call.
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "reads the calling thread's own desktop and its name"
)]
fn this_threads_desktop_name() -> Option<String> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::StationsAndDesktops::{
        GetThreadDesktop, GetUserObjectInformationW, UOI_NAME,
    };
    use windows::Win32::System::Threading::GetCurrentThreadId;
    // SAFETY: the thread's own desktop handle, which belongs to the thread and is not closed.
    let desktop = unsafe { GetThreadDesktop(GetCurrentThreadId()) }.ok()?;
    let mut name = [0_u16; 256];
    // SAFETY: `name` is a writable buffer of the size passed.
    unsafe {
        GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(name.as_mut_ptr().cast()),
            u32::try_from(size_of_val(&name)).expect("BUG: 512 bytes fit a u32"),
            None,
        )
    }
    .ok()?;
    let len = name.iter().position(|&unit| unit == 0)?;
    Some(String::from_utf16_lossy(&name[..len]))
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

/// What a `&LiveDesktop` parameter cannot hold: a constructor of something live, written in a
/// function that does not take the token. Every spelling of one in `windows_desktop.rs` must sit
/// in a function whose signature takes `&LiveDesktop`; the token itself is what the compiler
/// holds for the setups.
#[test]
fn what_opens_a_window_or_sends_input_is_written_only_where_the_token_is_taken() {
    let source = include_str!("windows_desktop.rs");
    assert!(
        source.contains("Driver::launch("),
        "the check no longer finds the live setups: update its spellings"
    );
    for spelling in [
        "Driver::launch(",
        "Driver::attach(",
        "WindowsGuard::new(",
        "WindowsGuard::default(",
        "KillOnCloseJob::try_new(",
        "SendInput(",
    ] {
        for (at, _) in source.match_indices(spelling) {
            let start = source[..at].rfind("\nfn ").expect("inside a function");
            let end = start + source[start..].find("{\n").expect("a body");
            assert!(
                source[start..end].contains("&LiveDesktop"),
                "`{spelling}` is written in a function that does not take `&LiveDesktop`: {}",
                source[start..end].trim()
            );
        }
    }
}
