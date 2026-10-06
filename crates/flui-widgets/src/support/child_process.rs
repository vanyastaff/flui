//! Runs one in-crate test body in a child process of the test binary.
//!
//! For a case whose failure mode is a process abort (a destructor panicking
//! during capacity unwind, ADR-0127) or one that spends a process-global
//! identity counter. Each case is its own `#[test]` so the runner schedules
//! the children independently; the child re-executes this binary, not cargo.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Set in the child, so the same test runs its body instead of spawning.
const CHILD: &str = "FLUI_WIDGETS_CHILD_CASE";

/// The exit status only a child that finished its body reports. libtest
/// exits 0 when its filter matches nothing and 101 when a test fails.
const COMPLETED: i32 = 73;

/// How long a child may run before it counts as hung.
const LIMIT: Duration = Duration::from_secs(10);

/// Declare a `#[test]` that runs `body` in its own child process.
macro_rules! child_test {
    ($(#[$meta:meta])* fn $name:ident() $body:block) => {
        $(#[$meta])*
        #[test]
        fn $name() {
            $crate::support::child_process::run(
                concat!(module_path!(), "::", stringify!($name)),
                || $body,
            );
        }
    };
}
pub(crate) use child_test;

/// In the child, run `body` and exit with [`COMPLETED`]. In the parent, spawn
/// the child for `path` (a `module_path!`-qualified test name) and assert it
/// completed within [`LIMIT`].
pub(crate) fn run(path: &str, body: impl FnOnce()) {
    if std::env::var_os(CHILD).is_some() {
        body();
        std::process::exit(COMPLETED);
    }
    // libtest names a test without its crate.
    let test = path.split_once("::").map_or(path, |(_, test)| test);
    let mut child = Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .env("RUST_BACKTRACE", "0")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("child test process");
    let mut stderr = child.stderr.take().expect("child stderr");
    // Drained on its own thread so a full pipe cannot stall the child.
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        stderr.read_to_string(&mut text).expect("child stderr");
        text
    });
    let started = Instant::now();
    let mut timed_out = false;
    while child.try_wait().expect("child status").is_none() {
        if started.elapsed() > LIMIT {
            timed_out = true;
            child.kill().expect("kill hung child");
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let status = child.wait().expect("reap child");
    let stderr = reader.join().expect("stderr reader");
    assert!(
        !timed_out && status.code() == Some(COMPLETED),
        "{test}: {status}; timed out: {timed_out}\n{stderr}"
    );
}
