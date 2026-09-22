//! Streaming execution of a platform build tool.
//!
//! Gradle, `wasm-pack`, `xcodebuild`, `cargo ndk` and `adb` print their own
//! progress; the builders let that output through to the terminal and only
//! care whether the tool succeeded. Contrast [`crate::proc`], which captures
//! the output of short probes under a deadline, and [`super::cargo`], which
//! reads cargo's JSON protocol line by line.

use std::process::Stdio;
use tokio::process::Command;

use crate::build::error::{BuildError, BuildResult};

/// Run `command` to completion with the terminal as its stdout and stderr.
///
/// The child is killed if the future is dropped before the tool exits, so a
/// cancelled build never leaves a Gradle daemon or an `xcodebuild` behind.
///
/// # Errors
///
/// `BuildError::CommandFailed` when the tool cannot be started or exits with
/// a non-zero status.
pub(crate) async fn run(command: &mut Command) -> BuildResult<()> {
    let std = command.as_std();
    let rendered = std::iter::once(std.get_program())
        .chain(std.get_args())
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    match std.get_current_dir() {
        Some(dir) => crate::ui::debug(format!("running in {}: {rendered}", dir.display())),
        None => crate::ui::debug(format!("running: {rendered}")),
    }

    let status = command
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .status()
        .await
        .map_err(|error| BuildError::CommandFailed {
            command: rendered.clone(),
            exit_code: -1,
            stderr: error.to_string(),
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(BuildError::CommandFailed {
            command: rendered,
            exit_code: status.code().unwrap_or(-1),
            stderr: "see the tool's output above".to_string(),
        })
    }
}
