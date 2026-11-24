//! A window's instruction count (ADR-0396 §3): the hold runs its own test
//! again in a child process under callgrind, which collects instructions only
//! while the window with the hold's number runs.
//!
//! `FLUI_PERF_INSTRUCTIONS=callgrind` turns the count on; unset, nothing is
//! counted. The child is marked by a variable of its own and compares,
//! records and blesses nothing: the parent holds what the child counted. The
//! marker and the collected function are private to this kit, so the only
//! thing a target names is the switch.

use std::num::NonZeroU32;
use std::process::Command;

/// The switch: `callgrind` counts each held window's instructions.
const INSTRUCTIONS_VARIABLE: &str = "FLUI_PERF_INSTRUCTIONS";

/// The marker of a child: the number of the one window it collects.
const WINDOW_VARIABLE: &str = "FLUI_PERF_COLLECTED_WINDOW";

/// The function a child's callgrind collects in (`--toggle-collect`), the
/// name of [`super::window`]'s out-of-line window.
pub(super) const WINDOW_SYMBOL: &str = "flui_perf_collected_window";

/// Whether this run counts instructions, as the switch says.
///
/// # Panics
///
/// When the switch names anything but `callgrind`.
pub(super) fn counting() -> bool {
    parse_switch(std::env::var(INSTRUCTIONS_VARIABLE).ok().as_deref())
        .unwrap_or_else(|error| panic!("{error}"))
}

fn parse_switch(value: Option<&str>) -> Result<bool, String> {
    match value {
        None => Ok(false),
        Some("callgrind") => Ok(true),
        Some(other) => Err(format!(
            "{INSTRUCTIONS_VARIABLE}={other:?} names no counter; use `callgrind`"
        )),
    }
}

/// The window this process collects, when it is a child.
///
/// # Panics
///
/// When the marker holds no window number.
pub(super) fn collected_window() -> Option<NonZeroU32> {
    let value = std::env::var_os(WINDOW_VARIABLE)?;
    let window = value
        .to_str()
        .and_then(|value| value.parse::<NonZeroU32>().ok())
        .unwrap_or_else(|| panic!("{WINDOW_VARIABLE}={} names no window", value.display()));
    Some(window)
}

/// Whether this process is a child, which holds nothing.
pub(super) fn is_child() -> bool {
    std::env::var_os(WINDOW_VARIABLE).is_some()
}

/// The instructions window `window` of the running test costs: the test runs
/// again under callgrind, alone, collecting only inside that window.
///
/// # Panics
///
/// When the running thread is not a test's, valgrind does not start, the test
/// fails under it, or it collects nothing (it no longer opens that window).
pub(super) fn count(window: NonZeroU32) -> u64 {
    let thread = std::thread::current();
    let test = thread
        .name()
        .filter(|name| *name != "main")
        .unwrap_or_else(|| {
            panic!(
                "{INSTRUCTIONS_VARIABLE}=callgrind runs the test again by name, \
                 and this thread is not one libtest named after its test"
            )
        });
    let binary = std::env::current_exe()
        .unwrap_or_else(|error| panic!("the test binary's path is unknown: {error}"));
    let output = Command::new("valgrind")
        .args([
            "--tool=callgrind",
            "--callgrind-out-file=/dev/null",
            &format!("--toggle-collect=*{WINDOW_SYMBOL}*"),
        ])
        .arg(&binary)
        .args(["--exact", test, "--test-threads=1"])
        .env(WINDOW_VARIABLE, window.to_string())
        .output()
        .unwrap_or_else(|error| {
            panic!("{INSTRUCTIONS_VARIABLE}=callgrind runs valgrind, which did not start: {error}")
        });
    let stderr = String::from_utf8_lossy(&output.stderr);
    let report = || {
        format!(
            "{test} under callgrind, window {window}:\n{}{stderr}",
            String::from_utf8_lossy(&output.stdout)
        )
    };
    assert!(output.status.success(), "failed: {}", report());
    match collected(&stderr) {
        Some(instructions) if instructions > 0 => instructions,
        _ => panic!(
            "collected nothing, so the run opened no window {window} or the window \
             function lost its name: {}",
            report()
        ),
    }
}

/// The count of callgrind's `Collected : <n>` line.
fn collected(stderr: &str) -> Option<u64> {
    stderr.lines().find_map(|line| {
        let (_, count) = line.split_once("Collected :")?;
        count.trim().parse::<u64>().ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unset counts nothing, `callgrind` counts, and anything else is refused
    /// rather than read as unset.
    #[test]
    fn the_switch_is_callgrind_or_unset() {
        assert_eq!(parse_switch(None), Ok(false));
        assert_eq!(parse_switch(Some("callgrind")), Ok(true));
        for refused in ["", "Callgrind", "cachegrind"] {
            assert!(parse_switch(Some(refused)).is_err(), "{refused:?}");
        }
    }

    /// The count is read from the line callgrind ends its run with, among
    /// the others it writes.
    #[test]
    fn the_count_is_callgrinds_collected_line() {
        let stderr = "==41== Callgrind, a call-graph generating cache profiler\n\
                      ==41== Events    : Ir\n\
                      ==41== Collected : 861448\n\
                      ==41== I   refs:      861,448\n";
        assert_eq!(collected(stderr), Some(861_448));
        assert_eq!(collected("==41== Events    : Ir\n"), None);
    }
}
