//! A device check as data. Every step is computed before the first one runs,
//! so a test on any host can hold a check against the recipe it replaced,
//! and only [`execute`] touches the machine.

use std::borrow::Cow;
use std::ffi::OsString;
use std::fmt;
use std::fs::File;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus, Stdio};
use std::time::Duration;

use anyhow::Context as _;
use regex::Regex;

use crate::fonts::find_python;

/// The program a step runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Program {
    /// The cargo running xtask (`$CARGO`), else `cargo` on `PATH`.
    Cargo,
    /// A Python >= 3.10 interpreter from `PATH`, resolved before the first
    /// step so a missing one is reported before a long build, not after it.
    Python,
    /// A tool found on `PATH`.
    Tool(&'static str),
    /// A binary by path, relative to the repository root unless absolute.
    Path(PathBuf),
}

/// Where a command's standard output goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Sink {
    /// The terminal, as a plain recipe line.
    Inherit,
    /// `>/dev/null`.
    Null,
    /// `> file`, relative to the repository root; the file is created even
    /// when the command cannot start, as a shell redirection is.
    File(PathBuf),
}

/// A command line with its redirections, spelled the way a recipe line was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Run {
    program: Program,
    args: Vec<OsString>,
    env: Vec<(&'static str, &'static str)>,
    stdout: Sink,
    /// `2>/dev/null`.
    quiet_stderr: bool,
    /// `|| true`: neither a failure nor a missing tool stops the plan.
    tolerate_failure: bool,
}

impl Run {
    pub(super) fn new(program: Program) -> Self {
        Self {
            program,
            args: Vec::new(),
            env: Vec::new(),
            stdout: Sink::Inherit,
            quiet_stderr: false,
            tolerate_failure: false,
        }
    }

    pub(super) fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub(super) fn args<I>(mut self, args: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// `KEY=value command`: set for this command only.
    pub(super) fn env(mut self, key: &'static str, value: &'static str) -> Self {
        self.env.push((key, value));
        self
    }

    pub(super) fn stdout(mut self, sink: Sink) -> Self {
        self.stdout = sink;
        self
    }

    /// `2>/dev/null`.
    pub(super) fn quiet_stderr(mut self) -> Self {
        self.quiet_stderr = true;
        self
    }

    /// `>/dev/null 2>&1`.
    pub(super) fn silent(self) -> Self {
        self.stdout(Sink::Null).quiet_stderr()
    }

    /// `|| true`.
    pub(super) fn or_true(mut self) -> Self {
        self.tolerate_failure = true;
        self
    }
}

/// What a recipe echoed after its driver failed: exit 2 means the host could
/// not take the measurement, which decides nothing about the framework.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Announce {
    pub(super) cannot_verify: &'static str,
    pub(super) failed: &'static str,
}

impl Announce {
    /// The line the recipe echoed after its driver exited with `code`.
    fn line_for(&self, code: u8) -> Option<&'static str> {
        match code {
            0 => None,
            2 => Some(self.cannot_verify),
            _ => Some(self.failed),
        }
    }
}

/// One step of a check. A failing step ends the plan with the exit code the
/// recipe's `bash -euo pipefail` would have ended it with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Step {
    /// `echo`.
    Echo(&'static str),
    /// A command; a failure ends the plan with its exit code unless it is
    /// tolerated.
    Run(Run),
    /// `rm -rf`.
    RemoveDir(PathBuf),
    /// `mkdir -p`.
    CreateDir(PathBuf),
    /// `cp`.
    Copy { from: PathBuf, to: PathBuf },
    /// `sleep`.
    Sleep(Duration),
    /// Runs a probe with stdout and stderr captured together, prints what it
    /// printed, and ends the plan with 1 after `failure` unless the probe
    /// exited 0 and printed every marker.
    Probe {
        run: Run,
        markers: &'static [&'static str],
        failure: &'static str,
    },
    /// Runs a driver with the terminal attached and ends the plan with its
    /// exit code, announced when the recipe announced it.
    Driver {
        run: Run,
        announce: Option<Announce>,
    },
    /// Runs a check this process performs itself — an OS client API with no
    /// command-line tool in front of it — and ends the plan with its exit
    /// code, announced as a driver's is.
    Native { check: Native, announce: Announce },
    /// Ends the plan with 1 after `failure` and the last `tail` lines of
    /// `log` containing `excerpt`, unless each pattern matches a line of
    /// `log`.
    RequireLog {
        log: PathBuf,
        patterns: &'static [&'static str],
        failure: &'static str,
        excerpt: &'static str,
        tail: usize,
    },
    /// Counts the processes whose command line contains the pattern
    /// (`pgrep -f`), for a later [`Step::RequireSurvivor`].
    CountProcesses(&'static str),
    /// Ends the plan with 1 after the message unless the last
    /// [`Step::CountProcesses`] found a process.
    RequireSurvivor(&'static str),
    /// Ends the plan with 1 after `failure` when the two files are
    /// byte-identical; the recipe compared their MD5 digests.
    RequireDiffer {
        a: PathBuf,
        b: PathBuf,
        failure: &'static str,
    },
}

impl Step {
    fn runs_python(&self) -> bool {
        match self {
            Self::Run(run) | Self::Probe { run, .. } | Self::Driver { run, .. } => {
                run.program == Program::Python
            }
            _ => false,
        }
    }
}

/// A check xtask runs in-process rather than as a child program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Native {
    /// The built `a11y_probe` driven through UI Automation
    /// ([`super::windows_a11y`]).
    WindowsA11y { probe: PathBuf },
}

impl Native {
    /// Its exit code: 0 pass, 1 fail, 2 cannot verify on this host.
    fn run(&self, root: &Path) -> anyhow::Result<u8> {
        match self {
            Self::WindowsA11y { probe } => windows_a11y(&root.join(probe)),
        }
    }
}

#[cfg(windows)]
use super::windows_a11y::run as windows_a11y;

/// Unreachable in practice: the check is skipped off Windows before any step
/// runs.
#[cfg(not(windows))]
fn windows_a11y(_probe: &Path) -> anyhow::Result<u8> {
    anyhow::bail!("the UI Automation client only exists on Windows")
}

/// Whether the plan goes on after a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Continue,
    Exit(u8),
}

/// Runs `steps` from the repository root, where the recipes ran; the first
/// step that ends the plan decides the exit code.
pub(super) fn execute(steps: &[Step], root: &Path) -> anyhow::Result<ExitCode> {
    let mut executor = Executor::new(root, steps)?;
    for step in steps {
        if let Flow::Exit(code) = executor.step(step)? {
            return Ok(ExitCode::from(code));
        }
    }
    Ok(ExitCode::SUCCESS)
}

struct Executor<'a> {
    root: &'a Path,
    python: Option<String>,
    /// The last [`Step::CountProcesses`] result.
    processes: Option<usize>,
}

impl<'a> Executor<'a> {
    fn new(root: &'a Path, steps: &[Step]) -> anyhow::Result<Self> {
        let python = if steps.iter().any(Step::runs_python) {
            Some(find_python().context(
                "no Python >= 3.10 on PATH; the drivers under tools/device-checks need one",
            )?)
        } else {
            None
        };
        Ok(Self {
            root,
            python,
            processes: None,
        })
    }

    fn step(&mut self, step: &Step) -> anyhow::Result<Flow> {
        match step {
            Step::Echo(line) => println!("{line}"),
            Step::Run(run) => return self.run(run),
            Step::RemoveDir(dir) => remove_all(&self.root.join(dir))?,
            Step::CreateDir(dir) => {
                let dir = self.root.join(dir);
                std::fs::create_dir_all(&dir)
                    .with_context(|| format!("creating {}", dir.display()))?;
            }
            Step::Copy { from, to } => {
                let (from, to) = (self.root.join(from), self.root.join(to));
                std::fs::copy(&from, &to)
                    .with_context(|| format!("copying {} to {}", from.display(), to.display()))?;
            }
            Step::Sleep(duration) => std::thread::sleep(*duration),
            Step::Probe {
                run,
                markers,
                failure,
            } => return self.probe(run, markers, failure),
            Step::Driver { run, announce } => return self.driver(run, announce.as_ref()),
            Step::Native { check, announce } => {
                let code = check.run(self.root)?;
                if let Some(line) = announce.line_for(code) {
                    println!("{line}");
                }
                return Ok(Flow::Exit(code));
            }
            Step::RequireLog {
                log,
                patterns,
                failure,
                excerpt,
                tail,
            } => return self.require_log(log, patterns, failure, excerpt, *tail),
            Step::CountProcesses(pattern) => {
                let output = Command::new("pgrep")
                    .args(["-f", pattern])
                    .stderr(Stdio::inherit())
                    .output()
                    .context("running `pgrep`")?;
                self.processes = Some(count_lines(&output.stdout));
            }
            Step::RequireSurvivor(failure) => {
                let found = self
                    .processes
                    .expect("BUG: a plan counts processes before requiring one to survive");
                if found < 1 {
                    println!("{failure}");
                    return Ok(Flow::Exit(1));
                }
            }
            Step::RequireDiffer { a, b, failure } => {
                if self.read(a)? == self.read(b)? {
                    println!("{failure}");
                    return Ok(Flow::Exit(1));
                }
            }
        }
        Ok(Flow::Continue)
    }

    /// The executable `program` names.
    fn executable(&self, program: &Program) -> OsString {
        match program {
            Program::Cargo => std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()),
            Program::Python => self
                .python
                .as_deref()
                .expect("BUG: the interpreter is resolved before a plan that runs Python starts")
                .into(),
            Program::Tool(name) => name.into(),
            Program::Path(path) => self.root.join(path).into_os_string(),
        }
    }

    fn command(&self, run: &Run) -> anyhow::Result<Command> {
        let mut command = Command::new(self.executable(&run.program));
        command
            .args(&run.args)
            .envs(run.env.iter().copied())
            .current_dir(self.root);
        match &run.stdout {
            Sink::Inherit => {}
            Sink::Null => {
                command.stdout(Stdio::null());
            }
            Sink::File(path) => {
                let path = self.root.join(path);
                let file =
                    File::create(&path).with_context(|| format!("creating {}", path.display()))?;
                command.stdout(file);
            }
        }
        if run.quiet_stderr {
            command.stderr(Stdio::null());
        }
        Ok(command)
    }

    fn run(&self, run: &Run) -> anyhow::Result<Flow> {
        let status = self.command(run).and_then(|mut command| {
            command
                .status()
                .with_context(|| format!("running `{}`", run.program))
        });
        match status {
            Ok(status) if status.success() => Ok(Flow::Continue),
            _ if run.tolerate_failure => Ok(Flow::Continue),
            Ok(status) => Ok(Flow::Exit(exit_code(status))),
            Err(error) => Err(error),
        }
    }

    fn probe(&self, run: &Run, markers: &[&str], failure: &str) -> anyhow::Result<Flow> {
        let (succeeded, output) = self.capture(run)?;
        {
            let mut stdout = std::io::stdout().lock();
            stdout.write_all(trim_trailing_newlines(&output))?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
        if probe_passed(succeeded, &output, markers) {
            Ok(Flow::Continue)
        } else {
            println!("{failure}");
            Ok(Flow::Exit(1))
        }
    }

    /// Runs `run` with stdout and stderr on one pipe, as `$(… 2>&1)` does,
    /// so the two streams keep their order; returns whether it exited 0.
    fn capture(&self, run: &Run) -> anyhow::Result<(bool, Vec<u8>)> {
        let (mut reader, writer) = std::io::pipe().context("creating a pipe")?;
        let spawned = {
            let mut command = self.command(run)?;
            command
                .stdout(writer.try_clone().context("duplicating a pipe")?)
                .stderr(writer);
            // `command` drops here, and with it this process's write ends:
            // the read below then ends when the probe's output does.
            command.spawn()
        };
        let mut child = match spawned {
            Ok(child) => child,
            // A shell captures its own "cannot execute" line and fails the
            // check on it; so does this.
            Err(error) => return Ok((false, format!("{}: {error}", run.program).into_bytes())),
        };
        let mut output = Vec::new();
        reader
            .read_to_end(&mut output)
            .context("reading the probe's output")?;
        let status = child.wait().context("waiting for the probe")?;
        Ok((status.success(), output))
    }

    fn driver(&self, run: &Run, announce: Option<&Announce>) -> anyhow::Result<Flow> {
        let status = self
            .command(run)?
            .status()
            .with_context(|| format!("running `{}`", run.program))?;
        let code = exit_code(status);
        if let Some(line) = announce.and_then(|announce| announce.line_for(code)) {
            println!("{line}");
        }
        Ok(Flow::Exit(code))
    }

    fn require_log(
        &self,
        log: &Path,
        patterns: &[&str],
        failure: &str,
        excerpt: &str,
        tail: usize,
    ) -> anyhow::Result<Flow> {
        let bytes = self.read(log)?;
        let text = String::from_utf8_lossy(&bytes);
        if every_pattern_matches(&text, patterns) {
            return Ok(Flow::Continue);
        }
        println!("{failure}");
        for line in last_lines_containing(&text, excerpt, tail) {
            println!("{line}");
        }
        Ok(Flow::Exit(1))
    }

    fn read(&self, path: &Path) -> anyhow::Result<Vec<u8>> {
        let path = self.root.join(path);
        std::fs::read(&path).with_context(|| format!("reading {}", path.display()))
    }
}

/// `rm -rf`: a file or a directory tree, and a missing path is no error.
fn remove_all(path: &Path) -> anyhow::Result<()> {
    let removed = match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => Err(error),
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
    };
    removed.with_context(|| format!("removing {}", path.display()))
}

/// The exit code a shell reports for `status`: the child's own, or 128 plus
/// the signal that killed it.
fn exit_code(status: ExitStatus) -> u8 {
    if let Some(signal) = killed_by(status) {
        return u8::try_from(128 + signal).unwrap_or(u8::MAX);
    }
    status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .unwrap_or(1)
}

#[cfg(unix)]
fn killed_by(status: ExitStatus) -> Option<i32> {
    std::os::unix::process::ExitStatusExt::signal(&status)
}

#[cfg(not(unix))]
fn killed_by(_status: ExitStatus) -> Option<i32> {
    None
}

/// `printf '%s\n' "$(…)"`: command substitution drops the trailing newlines
/// and `printf` puts one back.
fn trim_trailing_newlines(output: &[u8]) -> &[u8] {
    let end = output
        .iter()
        .rposition(|&byte| byte != b'\n')
        .map_or(0, |last| last + 1);
    &output[..end]
}

/// Whether a probe passed: exit 0 and every marker somewhere in its output
/// (`grep -q` per marker).
fn probe_passed(succeeded: bool, output: &[u8], markers: &[&str]) -> bool {
    succeeded
        && markers.iter().all(|marker| {
            output
                .windows(marker.len())
                .any(|window| window == marker.as_bytes())
        })
}

/// Whether each pattern matches some line of `text`, as one `grep -q` per
/// pattern does.
fn every_pattern_matches(text: &str, patterns: &[&str]) -> bool {
    patterns.iter().all(|pattern| {
        let pattern =
            Regex::new(pattern).expect("BUG: a plan's log patterns are valid regular expressions");
        text.lines().any(|line| pattern.is_match(line))
    })
}

/// `grep needle | tail -count`.
fn last_lines_containing<'t>(text: &'t str, needle: &str, count: usize) -> Vec<&'t str> {
    let matching: Vec<&str> = text.lines().filter(|line| line.contains(needle)).collect();
    matching[matching.len().saturating_sub(count)..].to_vec()
}

/// `wc -l`: the newlines in `output`, each of which starts one more piece.
fn count_lines(output: &[u8]) -> usize {
    output.split(|&byte| byte == b'\n').skip(1).count()
}

/// A path the way the recipes spelled it: `/`-separated on every host.
fn show(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

/// An argument as a shell would need it: quoted when it would split or
/// expand.
fn quote(arg: &str) -> Cow<'_, str> {
    let plain = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-./,:=+@%".contains(c));
    if plain {
        Cow::Borrowed(arg)
    } else {
        Cow::Owned(format!("'{}'", arg.replace('\'', r"'\''")))
    }
}

impl fmt::Display for Program {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cargo => f.write_str("cargo"),
            Self::Python => f.write_str("python"),
            Self::Tool(name) => f.write_str(name),
            Self::Path(path) => f.write_str(&quote(&show(path))),
        }
    }
}

impl fmt::Display for Run {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (key, value) in &self.env {
            write!(f, "{key}={} ", quote(value))?;
        }
        write!(f, "{}", self.program)?;
        for arg in &self.args {
            write!(f, " {}", quote(&show(Path::new(arg))))?;
        }
        match &self.stdout {
            Sink::Inherit => {}
            Sink::Null => f.write_str(" >/dev/null")?,
            Sink::File(path) => write!(f, " > {}", quote(&show(path)))?,
        }
        if self.quiet_stderr {
            f.write_str(if self.stdout == Sink::Null {
                " 2>&1"
            } else {
                " 2>/dev/null"
            })?;
        }
        if self.tolerate_failure {
            f.write_str(" || true")?;
        }
        Ok(())
    }
}

/// Renders a step as the shell line it replaces, so a plan reads side by
/// side with its recipe.
impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Echo(line) => write!(f, "echo '{line}'"),
            Self::Run(run) => write!(f, "{run}"),
            Self::RemoveDir(dir) => write!(f, "rm -rf {}", quote(&show(dir))),
            Self::CreateDir(dir) => write!(f, "mkdir -p {}", quote(&show(dir))),
            Self::Copy { from, to } => {
                write!(f, "cp {} {}", quote(&show(from)), quote(&show(to)))
            }
            Self::Sleep(duration) => write!(f, "sleep {}", duration.as_secs_f64()),
            Self::Probe {
                run,
                markers,
                failure,
            } => {
                write!(
                    f,
                    "out=$({run} 2>&1); printf '%s\\n' \"$out\"; unless exit 0"
                )?;
                for marker in *markers {
                    write!(f, " and '{marker}'")?;
                }
                write!(f, " in $out: echo '{failure}'; exit 1")
            }
            Self::Driver { run, announce } => {
                write!(f, "{run}")?;
                if let Some(Announce {
                    cannot_verify,
                    failed,
                }) = announce
                {
                    write!(
                        f,
                        "; if rc=2: echo '{cannot_verify}'; elif rc!=0: echo '{failed}'"
                    )?;
                }
                f.write_str("; exit $rc")
            }
            Self::Native {
                check: Native::WindowsA11y { probe },
                announce:
                    Announce {
                        cannot_verify,
                        failed,
                    },
            } => write!(
                f,
                "uia-client {}; if rc=2: echo '{cannot_verify}'; elif rc!=0: echo '{failed}'; exit $rc",
                quote(&show(probe))
            ),
            Self::RequireLog {
                log,
                patterns,
                failure,
                excerpt,
                tail,
            } => {
                let log = quote(&show(log)).into_owned();
                write!(f, "unless {log} has a line matching")?;
                for (index, pattern) in patterns.iter().enumerate() {
                    let joint = if index == 0 { "" } else { " and" };
                    write!(f, "{joint} '{pattern}'")?;
                }
                write!(
                    f,
                    ": echo '{failure}'; grep '{excerpt}' {log} | tail -{tail}; exit 1"
                )
            }
            Self::CountProcesses(pattern) => write!(f, "ALIVE=$(pgrep -f {pattern} | wc -l)"),
            Self::RequireSurvivor(failure) => write!(f, "if ALIVE < 1: echo '{failure}'; exit 1"),
            Self::RequireDiffer { a, b, failure } => write!(
                f,
                "if {} and {} are byte-identical: echo '{failure}'; exit 1",
                quote(&show(a)),
                quote(&show(b))
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A uniquely named directory under the system temp dir, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "xtask-device-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).expect("create scratch dir");
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn step(root: &Path, step: &Step) -> Flow {
        Executor::new(root, std::slice::from_ref(step))
            .expect("executor")
            .step(step)
            .expect("step runs")
    }

    #[test]
    fn a_probe_passes_only_on_exit_zero_with_every_marker() {
        let output = b"phase 1\nRESIZE_JITTER_PROBE_STALE=0\nRESIZE_JITTER_PROBE_RESULT=PASS\n";
        let markers = [
            "RESIZE_JITTER_PROBE_RESULT=PASS",
            "RESIZE_JITTER_PROBE_STALE=0",
        ];
        assert!(probe_passed(true, output, &markers));
        assert!(!probe_passed(false, output, &markers), "non-zero exit");
        assert!(
            !probe_passed(true, b"RESIZE_JITTER_PROBE_RESULT=PASS\n", &markers),
            "one marker missing"
        );
        assert!(!probe_passed(true, b"", &["CLOSE_PATH_PROBE_RESULT=PASS"]));
    }

    #[test]
    fn captured_output_is_echoed_like_printf_of_a_command_substitution() {
        assert_eq!(trim_trailing_newlines(b"a\nb\n\n"), b"a\nb");
        assert_eq!(trim_trailing_newlines(b"a\n\nb"), b"a\n\nb");
        assert_eq!(trim_trailing_newlines(b"\n\n"), b"");
        assert_eq!(trim_trailing_newlines(b""), b"");
    }

    #[test]
    fn a_driver_exit_code_is_announced_the_way_the_recipe_did() {
        let announce = Announce {
            cannot_verify: "CANNOT VERIFY",
            failed: "FAILED",
        };
        assert_eq!(announce.line_for(0), None);
        assert_eq!(announce.line_for(2), Some("CANNOT VERIFY"));
        assert_eq!(announce.line_for(1), Some("FAILED"));
        assert_eq!(announce.line_for(101), Some("FAILED"));
    }

    #[test]
    fn the_log_check_greps_each_pattern_on_its_own_line() {
        let log = "x [flui] Selected GPU: Apple M2 (Metal)\r\ny [flui] First frame rendered\n";
        let patterns = ["Selected GPU:.*Metal", "First frame rendered"];
        assert!(every_pattern_matches(log, &patterns));
        assert!(!every_pattern_matches("Selected GPU: none\n", &patterns));
        // `.` does not cross a line, as grep's does not.
        assert!(!every_pattern_matches(
            "Selected GPU:\nMetal\nFirst frame rendered\n",
            &patterns
        ));
    }

    #[test]
    fn the_failure_excerpt_is_the_last_matching_lines() {
        let log = (1..=30)
            .flat_map(|n| [format!("[flui] line {n}"), "other".to_owned()])
            .collect::<Vec<_>>()
            .join("\n");
        let tail = last_lines_containing(&log, "flui]", 20);
        assert_eq!(tail.len(), 20);
        assert_eq!(tail.first(), Some(&"[flui] line 11"));
        assert_eq!(tail.last(), Some(&"[flui] line 30"));
        assert_eq!(
            last_lines_containing("[flui] only\n", "flui]", 20),
            ["[flui] only"]
        );
    }

    #[test]
    fn process_counts_are_newline_counts() {
        assert_eq!(count_lines(b""), 0);
        assert_eq!(count_lines(b"4242\n"), 1);
        assert_eq!(count_lines(b"4242\n4243\n"), 2);
    }

    #[test]
    fn arguments_render_quoted_only_when_a_shell_would_need_it() {
        assert_eq!(quote("--features"), "--features");
        assert_eq!(quote("240,0,0"), "240,0,0");
        assert_eq!(quote("iPhone 17 Pro"), "'iPhone 17 Pro'");
        assert_eq!(quote(""), "''");
        assert_eq!(quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn staging_steps_behave_like_rm_mkdir_and_cp() {
        let scratch = Scratch::new();
        let root = scratch.0.as_path();
        std::fs::write(root.join("Info.plist"), "plist").expect("write");
        let app = PathBuf::from("staged/Probe.app");
        for staging in [
            Step::RemoveDir(app.clone()),
            Step::CreateDir(app.join("Contents/MacOS")),
            Step::Copy {
                from: "Info.plist".into(),
                to: app.join("Contents/Info.plist"),
            },
        ] {
            assert_eq!(step(root, &staging), Flow::Continue, "{staging}");
        }
        assert_eq!(
            std::fs::read_to_string(root.join(&app).join("Contents/Info.plist")).expect("read"),
            "plist"
        );
        // A second staging starts from nothing, as `rm -rf` guarantees.
        std::fs::write(root.join(&app).join("stale"), "").expect("write");
        assert_eq!(step(root, &Step::RemoveDir(app.clone())), Flow::Continue);
        assert!(!root.join(&app).exists());
        assert_eq!(
            step(root, &Step::RemoveDir(app)),
            Flow::Continue,
            "missing is fine"
        );
    }

    #[test]
    fn identical_screenshots_fail_and_differing_ones_pass() {
        let scratch = Scratch::new();
        let root = scratch.0.as_path();
        for (name, bytes) in [("a.png", "one"), ("b.png", "one"), ("c.png", "two")] {
            std::fs::write(root.join(name), bytes).expect("write");
        }
        let differ = |b: &str| Step::RequireDiffer {
            a: "a.png".into(),
            b: b.into(),
            failure: "IDENTICAL",
        };
        assert_eq!(step(root, &differ("b.png")), Flow::Exit(1));
        assert_eq!(step(root, &differ("c.png")), Flow::Continue);
    }

    #[test]
    fn a_log_missing_a_pattern_fails_the_plan() {
        let scratch = Scratch::new();
        let root = scratch.0.as_path();
        std::fs::write(root.join("app.log"), "[flui] Selected GPU: x (Metal)\n").expect("write");
        let require = |patterns: &'static [&'static str]| Step::RequireLog {
            log: "app.log".into(),
            patterns,
            failure: "FAIL",
            excerpt: "flui]",
            tail: 20,
        };
        assert_eq!(
            step(root, &require(&["Selected GPU:.*Metal"])),
            Flow::Continue
        );
        assert_eq!(
            step(
                root,
                &require(&["Selected GPU:.*Metal", "First frame rendered"])
            ),
            Flow::Exit(1)
        );
    }

    #[test]
    fn a_probe_is_captured_and_judged_on_its_real_output() {
        let scratch = Scratch::new();
        let version = || Run::new(Program::Cargo).arg("--version");
        let probe = |markers: &'static [&'static str]| Step::Probe {
            run: version(),
            markers,
            failure: "FAILED",
        };
        assert_eq!(step(&scratch.0, &probe(&["cargo "])), Flow::Continue);
        assert_eq!(
            step(&scratch.0, &probe(&["cargo ", "NO SUCH MARKER"])),
            Flow::Exit(1)
        );
        let failing = Step::Probe {
            run: Run::new(Program::Cargo).arg("--no-such-flag"),
            markers: &[],
            failure: "FAILED",
        };
        assert_eq!(step(&scratch.0, &failing), Flow::Exit(1), "non-zero exit");
        let missing = Step::Probe {
            run: Run::new(Program::Path("no/such/probe".into())),
            markers: &[],
            failure: "FAILED",
        };
        assert_eq!(step(&scratch.0, &missing), Flow::Exit(1), "cannot start");
    }

    #[test]
    fn a_driver_ends_the_plan_with_its_exit_code() {
        let scratch = Scratch::new();
        let driver = |arg: &str| Step::Driver {
            run: Run::new(Program::Cargo).arg(arg),
            announce: Some(Announce {
                cannot_verify: "CANNOT VERIFY",
                failed: "FAILED",
            }),
        };
        assert_eq!(step(&scratch.0, &driver("--version")), Flow::Exit(0));
        assert_eq!(step(&scratch.0, &driver("--no-such-flag")), Flow::Exit(1));
    }

    #[test]
    fn a_failing_command_stops_the_plan_unless_tolerated() {
        let scratch = Scratch::new();
        let failing = Run::new(Program::Cargo).arg("--no-such-flag").silent();
        assert_eq!(step(&scratch.0, &Step::Run(failing.clone())), Flow::Exit(1));
        assert_eq!(
            step(&scratch.0, &Step::Run(failing.or_true())),
            Flow::Continue
        );
        let absent = Run::new(Program::Tool("xtask-no-such-tool"));
        assert_eq!(
            step(&scratch.0, &Step::Run(absent.or_true())),
            Flow::Continue
        );
    }

    #[test]
    fn a_redirected_command_writes_its_file() {
        let scratch = Scratch::new();
        let run = Run::new(Program::Cargo)
            .arg("--version")
            .stdout(Sink::File("out.txt".into()));
        assert_eq!(step(&scratch.0, &Step::Run(run)), Flow::Continue);
        let written = std::fs::read_to_string(scratch.0.join("out.txt")).expect("read");
        assert!(written.starts_with("cargo "), "{written}");
    }
}
