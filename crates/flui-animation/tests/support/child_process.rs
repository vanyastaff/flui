//! Runs table rows in child processes of the current test binary, for rows
//! whose failure mode is a process abort or a deadlock rather than a panic.

use std::io::Read;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// Names the row a child process runs.
const CASE: &str = "FLUI_CHILD_CASE";

/// The exit code a child reports after its row passed. It differs from the
/// harness's own success code, so a filter that selects no test cannot pass.
const PASSED: i32 = 86;

/// How long a row may run before it counts as deadlocked.
const LIMIT: Duration = Duration::from_secs(10);

/// The row this process runs, when it is a child.
pub(crate) fn selected_case() -> Option<String> {
    std::env::var(CASE).ok()
}

/// Reports the selected row as passed and ends the child process.
pub(crate) fn pass() -> ! {
    std::process::exit(PASSED)
}

/// Runs `cases` as rows of `test` (its exact path inside this binary), each in
/// its own child process, then panics once naming every row that failed,
/// aborted or ran past the time limit.
pub(crate) fn run_rows(test: &str, cases: &[&str]) {
    let mut failures = Vec::new();
    for case in cases {
        let (status, output) = run_child(test, case);
        if status.code() != Some(PASSED) {
            failures.push(format!("{case}: {status}\n{output}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Runs one row as the only case of `test`, in a child process: the child runs
/// `row` and reports it passed; the parent fails if the child aborted, failed
/// or ran past the time limit.
pub(crate) fn run_single(test: &str, row: fn()) {
    if selected_case().is_some() {
        row();
        pass();
    }
    run_rows(test, &["row"]);
}

fn run_child(test: &str, case: &str) -> (ExitStatus, String) {
    let mut child = Command::new(std::env::current_exe().expect("test executable"))
        // `--include-ignored` lets an ignored contract row run its child too,
        // so `--run-ignored` reports the row's own failure, not "no test ran".
        .args(["--exact", test, "--include-ignored", "--nocapture"])
        .env(CASE, case)
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
    let mut stalled = false;
    while child.try_wait().expect("child status").is_none() {
        if started.elapsed() > LIMIT {
            child.kill().expect("kill stalled child");
            stalled = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let status = child.wait().expect("child exit");
    let output = reader.join().expect("stderr reader");
    if stalled {
        return (
            status,
            format!("{output}killed: still running after {LIMIT:?} (deadlock)\n"),
        );
    }
    (status, output)
}
