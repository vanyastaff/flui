//! `flui doctor`: audit the toolchains FLUI can build against.
//!
//! Checks are data (see [`Check`]), not print statements, so the same pass
//! renders as Flutter-style `[✓]/[!]/[✗]` lines in human mode and as NDJSON
//! `doctor.check` / `doctor.summary` events in `--json` mode. `rustc`,
//! `cargo`, `rustup` and `git` are always required; Android/iOS/Web
//! toolchains only warn unless the caller asked for that section
//! explicitly (`--android`/`--ios`/`--web`), in which case a missing piece
//! of *that* section becomes an error.

use crate::error::{CliError, CliResult};
use crate::proc::{self, PROBE_TIMEOUT};
use crate::ui;
use console::style;
use serde::Serialize;
use std::fmt::Write as _;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::Duration;

/// The budget for `rustup target add`, which downloads a component.
const FIX_TIMEOUT: Duration = Duration::from_mins(10);

/// Options of `flui doctor`.
#[derive(Debug, Clone, Copy, Default)]
pub struct DoctorOptions {
    /// Show paths and full version strings.
    pub verbose: bool,
    /// Check only the Android toolchain.
    pub android: bool,
    /// Check only the iOS toolchain.
    pub ios: bool,
    /// Check only the Web toolchain.
    pub web: bool,
    /// Run automatable fixes (currently: missing `rustup` targets), then
    /// re-check.
    pub fix: bool,
}

/// Severity of one [`Check`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// The tool or capability is present and usable.
    Ok,
    /// Missing, but only a required piece if the caller opted into this
    /// section; otherwise cosmetic.
    Warn,
    /// Missing a piece that is required in this run.
    Error,
}

/// A fix for a failing [`Check`], carried structurally rather than as free
/// text describing it. `--fix` matches on the variant instead of re-parsing
/// the rendered hint (e.g. via `strip_prefix("rustup target add ")`), so the
/// automation can never drift from what is shown to the user.
#[derive(Debug, Clone)]
enum Fix {
    /// `rustup target add <targets...>` — the only shape `--fix` runs
    /// automatically.
    RustupTargets(Vec<String>),
    /// Anything else: shown as a hint in both modes, never executed.
    Manual(String),
}

impl Fix {
    fn manual(text: impl Into<String>) -> Self {
        Self::Manual(text.into())
    }

    /// The text shown in human mode and in the JSON `fix` field. The single
    /// place that turns a [`Fix`] into prose, so the rendered hint and the
    /// command `--fix` runs can never disagree.
    fn rendered(&self) -> String {
        match self {
            Self::RustupTargets(targets) => format!("rustup target add {}", targets.join(" ")),
            Self::Manual(text) => text.clone(),
        }
    }
}

/// One thing `doctor` looked at.
#[derive(Debug, Clone)]
struct Check {
    id: &'static str,
    section: &'static str,
    title: &'static str,
    status: Status,
    detail: String,
    /// A fix for this check, if one exists.
    fix: Option<Fix>,
    path: Option<PathBuf>,
}

impl Check {
    fn ok(id: &'static str, section: &'static str, title: &'static str, detail: String) -> Self {
        Self {
            id,
            section,
            title,
            status: Status::Ok,
            detail,
            fix: None,
            path: None,
        }
    }

    fn with_path(mut self, path: Option<PathBuf>) -> Self {
        self.path = path;
        self
    }

    fn failing(
        id: &'static str,
        section: &'static str,
        title: &'static str,
        status: Status,
        detail: String,
        fix: Option<Fix>,
    ) -> Self {
        debug_assert_ne!(status, Status::Ok, "use Check::ok for a passing check");
        Self {
            id,
            section,
            title,
            status,
            detail,
            fix,
            path: None,
        }
    }

    /// `Error` when `promote`, `Warn` otherwise — the required-vs-optional
    /// rule for a section the caller may or may not have selected.
    fn missing(promote: bool) -> Status {
        if promote { Status::Error } else { Status::Warn }
    }
}

/// NDJSON payload for one `doctor.check` event.
#[derive(Serialize)]
struct CheckEvent<'a> {
    id: &'a str,
    section: &'a str,
    title: &'a str,
    status: Status,
    detail: &'a str,
    fix: Option<String>,
    path: &'a Option<PathBuf>,
}

impl<'a> From<&'a Check> for CheckEvent<'a> {
    fn from(check: &'a Check) -> Self {
        Self {
            id: check.id,
            section: check.section,
            title: check.title,
            status: check.status,
            detail: &check.detail,
            fix: check.fix.as_ref().map(Fix::rendered),
            path: &check.path,
        }
    }
}

/// NDJSON payload for the closing `doctor.summary` event.
#[derive(Serialize)]
struct Summary {
    ok: usize,
    warn: usize,
    error: usize,
    total: usize,
    exit_code: i32,
}

fn summarize(checks: &[Check]) -> Summary {
    let ok = checks.iter().filter(|c| c.status == Status::Ok).count();
    let warn = checks.iter().filter(|c| c.status == Status::Warn).count();
    let error = checks.iter().filter(|c| c.status == Status::Error).count();
    Summary {
        ok,
        warn,
        error,
        total: checks.len(),
        exit_code: if error > 0 {
            crate::error::exit_code::ENVIRONMENT
        } else {
            crate::error::exit_code::SUCCESS
        },
    }
}

/// Execute `flui doctor`.
pub fn execute(options: DoctorOptions) -> CliResult<()> {
    let _ = ui::intro(style(" flui doctor ").on_cyan().black());

    let mut checks = run_checks(&options);

    if options.fix {
        run_autofix(&checks);
        // Re-check: a fix may have changed the answer.
        checks = run_checks(&options);
    }

    // The report box carries every line in normal human mode; the bare
    // warning/error lines exist for `--quiet`, where the box is suppressed
    // but a failure must still be visible. JSON consumers read the events.
    let bare_lines = ui::is_quiet() && !ui::is_json();
    for check in &checks {
        ui::emit("doctor.check", &CheckEvent::from(check));
        if !bare_lines {
            continue;
        }
        match check.status {
            Status::Warn => {
                let _ = ui::warning(format!("{}: {}", check.title, check.detail));
            }
            Status::Error => {
                let _ = ui::error(format!("{}: {}", check.title, check.detail));
            }
            Status::Ok => {}
        }
    }

    let _ = ui::note("Environment Check", render_report(&checks));

    let summary = summarize(&checks);
    ui::emit("doctor.summary", &summary);

    if summary.error > 0 {
        let _ = ui::outro_cancel(format!(
            "{} of {} checks failed. Please fix the issues above.",
            summary.error, summary.total
        ));
        Err(CliError::EnvironmentCheckFailed {
            failed: summary.error,
            total: summary.total,
        })
    } else {
        let _ = ui::outro(style("All required checks passed!").green());
        Ok(())
    }
}

/// Build the full set of checks for this invocation.
fn run_checks(options: &DoctorOptions) -> Vec<Check> {
    let DoctorOptions {
        verbose,
        android,
        ios,
        web,
        fix: _,
    } = *options;

    let installed_targets = rustup_installed_targets();

    let mut checks = core_checks(verbose, installed_targets.as_deref());

    let check_all = !android && !ios && !web;

    if android || check_all {
        checks.extend(android_checks(
            verbose,
            android,
            installed_targets.as_deref(),
        ));
    }

    if ios || check_all {
        checks.extend(ios_checks(verbose, ios, installed_targets.as_deref()));
    }

    if web || check_all {
        checks.extend(web_checks(verbose, web, installed_targets.as_deref()));
    }

    checks
}

// ============================================================================
// Process probing
// ============================================================================

/// The outcome of running a probe command, distinguishing the reasons a
/// check can fail so the detail text is specific rather than a generic "no".
enum Probe {
    Success(Output),
    Failed(Output),
    NotFound,
    TimedOut,
    Error(std::io::Error),
}

fn probe(command: &mut Command) -> Probe {
    match proc::output_with_timeout(command, PROBE_TIMEOUT) {
        Ok(output) if output.status.success() => Probe::Success(output),
        Ok(output) => Probe::Failed(output),
        Err(error) if error.kind() == ErrorKind::TimedOut => Probe::TimedOut,
        Err(error) if error.kind() == ErrorKind::NotFound => Probe::NotFound,
        Err(error) => Probe::Error(error),
    }
}

impl Probe {
    /// A one-line reason this probe did not succeed, for a check's detail.
    fn failure_detail(&self, tool: &str) -> String {
        match self {
            Self::Success(_) => unreachable!("caller checks for Success first"),
            Self::Failed(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let line = stderr
                    .lines()
                    .next()
                    .unwrap_or("exited with an error")
                    .trim();
                format!("{tool} exited unsuccessfully: {line}")
            }
            Self::NotFound => format!("{tool} not found on PATH"),
            Self::TimedOut => format!("{tool} did not answer within {PROBE_TIMEOUT:?}"),
            Self::Error(error) => format!("{tool} could not be run: {error}"),
        }
    }
}

fn rustup_installed_targets() -> Option<Vec<String>> {
    let output = proc::probe_stdout(
        Command::new("rustup").args(["target", "list", "--installed"]),
        PROBE_TIMEOUT,
    )?;
    Some(output.lines().map(str::trim).map(str::to_owned).collect())
}

fn host_triple() -> Option<String> {
    let output = proc::probe_stdout(Command::new("rustc").arg("-vV"), PROBE_TIMEOUT)?;
    output
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::trim)
        .map(str::to_owned)
}

/// The first whitespace-separated token that looks like a version number
/// (starts with an ASCII digit), e.g. `"1.90.0"` out of `"rustc 1.90.0
/// (abcdef 2024-01-01)"`.
fn version_token(text: &str) -> Option<&str> {
    text.split_whitespace()
        .find(|token| token.starts_with(|c: char| c.is_ascii_digit()))
}

fn digits_prefix(text: &str) -> Option<u64> {
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

/// `"1.97.0"` -> `Some((1, 97))`; ignores anything past the minor component.
fn major_minor(version: &str) -> Option<(u64, u64)> {
    let mut parts = version.split('.');
    let major = digits_prefix(parts.next()?)?;
    let minor = parts.next().and_then(digits_prefix).unwrap_or(0);
    Some((major, minor))
}

// ============================================================================
// Rustup targets (shared shape for Android/iOS/Web)
// ============================================================================

/// A check over a set of `rustup` targets a section needs, e.g. the four
/// Android ABIs. Reused by Android, iOS and Web since the shape — "which of
/// these targets are installed" — is identical.
fn targets_check(
    id: &'static str,
    section: &'static str,
    title: &'static str,
    wanted: &[&'static str],
    installed: Option<&[String]>,
    promote: bool,
) -> Check {
    let Some(installed) = installed else {
        return Check::failing(
            id,
            section,
            title,
            Status::Warn,
            "cannot verify: rustup not found".to_string(),
            None,
        );
    };

    let missing: Vec<String> = wanted
        .iter()
        .filter(|target| !installed.iter().any(|t| t == *target))
        .map(ToString::to_string)
        .collect();

    if missing.is_empty() {
        Check::ok(id, section, title, "all installed".to_string())
    } else {
        let detail = format!("missing {}", missing.join(", "));
        Check::failing(
            id,
            section,
            title,
            Check::missing(promote),
            detail,
            Some(Fix::RustupTargets(missing)),
        )
    }
}

// ============================================================================
// Core: rustc, host target, cargo, rustup, git — always required
// ============================================================================

fn core_checks(verbose: bool, installed_targets: Option<&[String]>) -> Vec<Check> {
    vec![
        Check::ok(
            "core.flui-cli",
            "core",
            "FLUI CLI",
            format!("v{}", env!("CARGO_PKG_VERSION")),
        ),
        check_rustc(verbose),
        check_host_target(installed_targets),
        check_tool_version("core.cargo", "Cargo", "cargo", &["--version"], verbose),
        check_tool_version("core.rustup", "rustup", "rustup", &["--version"], verbose),
        check_tool_version("core.git", "Git", "git", &["--version"], verbose),
    ]
}

fn check_rustc(verbose: bool) -> Check {
    const ID: &str = "core.rustc";
    const SECTION: &str = "core";
    const TITLE: &str = "Rust";

    match probe(Command::new("rustc").arg("--version")) {
        Probe::Success(output) => {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let msrv = env!("CARGO_PKG_RUST_VERSION");
            let msrv_pair = major_minor(msrv);
            let found_pair = version_token(&text).and_then(major_minor);

            match (found_pair, msrv_pair) {
                (Some(found), Some(msrv_pair)) if found < msrv_pair => Check::failing(
                    ID,
                    SECTION,
                    TITLE,
                    Status::Error,
                    format!("{text} is below the toolchain's MSRV ({msrv})"),
                    Some(Fix::manual("rustup update")),
                ),
                _ => {
                    let path = verbose.then(|| which::which("rustc").ok()).flatten();
                    Check::ok(ID, SECTION, TITLE, text).with_path(path)
                }
            }
        }
        probe => Check::failing(
            ID,
            SECTION,
            TITLE,
            Status::Error,
            probe.failure_detail("rustc"),
            Some(Fix::manual("Install from https://rustup.rs/")),
        ),
    }
}

fn check_host_target(installed_targets: Option<&[String]>) -> Check {
    const ID: &str = "core.host-target";
    const SECTION: &str = "core";
    const TITLE: &str = "Host target";

    let Some(triple) = host_triple() else {
        return Check::failing(
            ID,
            SECTION,
            TITLE,
            Status::Error,
            "could not determine the host target (rustc -vV failed)".to_string(),
            None,
        );
    };

    let Some(installed) = installed_targets else {
        return Check::failing(
            ID,
            SECTION,
            TITLE,
            Status::Warn,
            format!("cannot verify {triple}: rustup not found"),
            None,
        );
    };

    if installed.iter().any(|t| t == &triple) {
        Check::ok(ID, SECTION, TITLE, triple)
    } else {
        Check::failing(
            ID,
            SECTION,
            TITLE,
            Status::Error,
            format!("{triple} is not installed"),
            Some(Fix::RustupTargets(vec![triple])),
        )
    }
}

/// A tool whose presence is proven by running it with `args` and requiring
/// success; used for `cargo --version`, `rustup --version`, `git
/// --version`.
fn check_tool_version(
    id: &'static str,
    title: &'static str,
    program: &str,
    args: &[&str],
    verbose: bool,
) -> Check {
    match probe(Command::new(program).args(args)) {
        Probe::Success(output) => {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let path = verbose.then(|| which::which(program).ok()).flatten();
            Check::ok(id, "core", title, text).with_path(path)
        }
        probe => Check::failing(
            id,
            "core",
            title,
            Status::Error,
            probe.failure_detail(program),
            Some(Fix::manual(format!(
                "Install {program} and ensure it is on PATH"
            ))),
        ),
    }
}

// ============================================================================
// Android
// ============================================================================

const ANDROID_TARGETS: [&str; 4] = [
    "aarch64-linux-android",
    "armv7-linux-androideabi",
    "i686-linux-android",
    "x86_64-linux-android",
];

fn android_checks(
    verbose: bool,
    promote: bool,
    installed_targets: Option<&[String]>,
) -> Vec<Check> {
    let sdk_home = std::env::var("ANDROID_HOME")
        .or_else(|_| std::env::var("ANDROID_SDK_ROOT"))
        .ok();

    let sdk_check = check_android_sdk(sdk_home.as_deref(), promote);
    let sdk_path = sdk_home
        .as_deref()
        .filter(|_| sdk_check.status == Status::Ok)
        .map(PathBuf::from);

    let mut checks = vec![sdk_check, check_adb(verbose, promote)];
    // Without an SDK home the NDK row could only say "cannot check" — the
    // SDK row already carries that failure, so do not repeat it.
    if let Some(sdk_path) = sdk_path.as_deref() {
        checks.push(check_android_ndk(Some(sdk_path), promote));
    }
    checks.push(check_java(verbose, promote));
    checks.push(targets_check(
        "android.targets",
        "android",
        "Android targets",
        &ANDROID_TARGETS,
        installed_targets,
        promote,
    ));
    checks
}

fn check_android_sdk(sdk_home: Option<&str>, promote: bool) -> Check {
    const ID: &str = "android.sdk";
    const SECTION: &str = "android";
    const TITLE: &str = "Android SDK";

    let Some(sdk_home) = sdk_home else {
        return Check::failing(
            ID,
            SECTION,
            TITLE,
            Check::missing(promote),
            "ANDROID_HOME / ANDROID_SDK_ROOT not set".to_string(),
            Some(Fix::manual("export ANDROID_HOME=<path to Android SDK>")),
        );
    };

    if std::path::Path::new(sdk_home).is_dir() {
        Check::ok(ID, SECTION, TITLE, sdk_home.to_string())
    } else {
        Check::failing(
            ID,
            SECTION,
            TITLE,
            Check::missing(promote),
            format!("{sdk_home} does not exist"),
            None,
        )
    }
}

fn check_adb(verbose: bool, promote: bool) -> Check {
    const ID: &str = "android.adb";
    const SECTION: &str = "android";
    const TITLE: &str = "adb";

    match which::which("adb") {
        Ok(path) => Check::ok(ID, SECTION, TITLE, "found on PATH".to_string())
            .with_path(verbose.then_some(path)),
        Err(_) => Check::failing(
            ID,
            SECTION,
            TITLE,
            Check::missing(promote),
            "adb not found on PATH".to_string(),
            Some(Fix::manual("install the Android SDK Platform Tools")),
        ),
    }
}

fn check_android_ndk(sdk_home: Option<&std::path::Path>, promote: bool) -> Check {
    const ID: &str = "android.ndk";
    const SECTION: &str = "android";
    const TITLE: &str = "Android NDK";

    let Some(sdk_home) = sdk_home else {
        return Check::failing(
            ID,
            SECTION,
            TITLE,
            Check::missing(promote),
            "cannot check: Android SDK home is unknown".to_string(),
            None,
        );
    };

    let ndk_path = sdk_home.join("ndk");
    if ndk_path.is_dir() {
        Check::ok(ID, SECTION, TITLE, ndk_path.display().to_string())
    } else {
        Check::failing(
            ID,
            SECTION,
            TITLE,
            Check::missing(promote),
            format!("{} not found", ndk_path.display()),
            Some(Fix::manual(
                "install the NDK via Android Studio's SDK Manager",
            )),
        )
    }
}

/// `java -version` needs `status.success()`, not just a spawn: on macOS
/// without a JDK installed, Apple's `/usr/bin/java` stub *runs* and prints
/// "Unable to locate a Java Runtime" to stderr, then exits non-zero — a
/// bare "did it spawn" check reports that as success.
fn check_java(verbose: bool, promote: bool) -> Check {
    const ID: &str = "android.java";
    const SECTION: &str = "android";
    const TITLE: &str = "Java";

    match probe(Command::new("java").arg("-version")) {
        Probe::Success(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let line = stderr
                .lines()
                .next()
                .unwrap_or("version unknown")
                .trim()
                .to_string();
            let path = verbose.then(|| which::which("java").ok()).flatten();
            Check::ok(ID, SECTION, TITLE, line).with_path(path)
        }
        probe => Check::failing(
            ID,
            SECTION,
            TITLE,
            Check::missing(promote),
            probe.failure_detail("java"),
            Some(Fix::manual("Download a JDK from https://adoptium.net/")),
        ),
    }
}

// ============================================================================
// iOS
// ============================================================================

#[cfg(target_os = "macos")]
const IOS_TARGETS: [&str; 3] = [
    "aarch64-apple-ios",
    "aarch64-apple-ios-sim",
    "x86_64-apple-ios",
];

#[cfg(target_os = "macos")]
fn ios_checks(verbose: bool, promote: bool, installed_targets: Option<&[String]>) -> Vec<Check> {
    vec![
        check_xcode(verbose, promote),
        check_simctl(promote),
        targets_check(
            "ios.targets",
            "ios",
            "iOS targets",
            &IOS_TARGETS,
            installed_targets,
            promote,
        ),
    ]
}

#[cfg(not(target_os = "macos"))]
fn ios_checks(_verbose: bool, promote: bool, _installed_targets: Option<&[String]>) -> Vec<Check> {
    if promote {
        // `--ios` was selected explicitly on a non-macOS host: the
        // required-vs-optional rule promotes a missing piece of the
        // selected section to `Error`, same as every other check here.
        vec![Check::failing(
            "ios.platform",
            "ios",
            "iOS",
            Check::missing(promote),
            "iOS builds need macOS".to_string(),
            None,
        )]
    } else {
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
fn check_xcode(verbose: bool, promote: bool) -> Check {
    const ID: &str = "ios.xcode";
    const SECTION: &str = "ios";
    const TITLE: &str = "Xcode";

    match probe(Command::new("xcode-select").arg("-p")) {
        Probe::Success(output) => {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let check = Check::ok(
                ID,
                SECTION,
                TITLE,
                "command line tools installed".to_string(),
            );
            if verbose {
                check.with_path(Some(PathBuf::from(path)))
            } else {
                check
            }
        }
        probe => Check::failing(
            ID,
            SECTION,
            TITLE,
            Check::missing(promote),
            probe.failure_detail("xcode-select"),
            Some(Fix::manual("xcode-select --install")),
        ),
    }
}

#[cfg(target_os = "macos")]
fn check_simctl(promote: bool) -> Check {
    const ID: &str = "ios.simctl";
    const SECTION: &str = "ios";
    const TITLE: &str = "iOS simulators";

    match probe(Command::new("xcrun").args(["simctl", "list", "-j", "devices", "available"])) {
        Probe::Success(_) => Check::ok(ID, SECTION, TITLE, "simctl responded".to_string()),
        probe => Check::failing(
            ID,
            SECTION,
            TITLE,
            Check::missing(promote),
            probe.failure_detail("xcrun simctl"),
            Some(Fix::manual("open Xcode once to finish its first-run setup")),
        ),
    }
}

// ============================================================================
// Web
// ============================================================================

fn web_checks(_verbose: bool, promote: bool, installed_targets: Option<&[String]>) -> Vec<Check> {
    vec![
        targets_check(
            "web.wasm-target",
            "web",
            "WASM target",
            &["wasm32-unknown-unknown"],
            installed_targets,
            promote,
        ),
        check_wasm_tooling(),
    ]
}

/// `wasm-pack` or `wasm-bindgen` is convenient but not required — either
/// one, or neither, only ever warns.
fn check_wasm_tooling() -> Check {
    const ID: &str = "web.tooling";
    const SECTION: &str = "web";
    const TITLE: &str = "WASM tooling";

    if which::which("wasm-pack").is_ok() {
        Check::ok(ID, SECTION, TITLE, "wasm-pack found".to_string())
    } else if which::which("wasm-bindgen").is_ok() {
        Check::ok(ID, SECTION, TITLE, "wasm-bindgen found".to_string())
    } else {
        Check::failing(
            ID,
            SECTION,
            TITLE,
            Status::Warn,
            "neither wasm-pack nor wasm-bindgen found (optional)".to_string(),
            Some(Fix::manual("cargo install wasm-pack")),
        )
    }
}

// ============================================================================
// --fix
// ============================================================================

/// Run every automatable fix among the current `Error` checks, then let the
/// caller re-run [`run_checks`] to see whether it worked. Only `rustup
/// target add ...` hints are automated; anything else (Xcode, an SDK) is
/// reported and left alone.
fn run_autofix(checks: &[Check]) {
    let mut targets: Vec<String> = Vec::new();
    let mut unfixable: Vec<&Check> = Vec::new();

    for check in checks {
        if check.status != Status::Error {
            continue;
        }
        match &check.fix {
            Some(Fix::RustupTargets(wanted)) => targets.extend(wanted.iter().cloned()),
            Some(Fix::Manual(_)) => unfixable.push(check),
            None => {}
        }
    }
    targets.sort_unstable();
    targets.dedup();

    for check in unfixable {
        let fix = check
            .fix
            .as_ref()
            .map_or_else(|| "no suggested fix".to_string(), Fix::rendered);
        let _ = ui::warning(format!("cannot automatically fix {}: {fix}", check.title));
    }

    if targets.is_empty() {
        return;
    }

    let command_text = format!("rustup target add {}", targets.join(" "));
    let _ = ui::step(format!("Running: {command_text}"));

    let ok = matches!(
        proc::output_with_timeout(Command::new("rustup").arg("target").arg("add").args(&targets), FIX_TIMEOUT),
        Ok(output) if output.status.success()
    );

    ui::emit(
        "doctor.fix",
        &serde_json::json!({ "command": command_text, "ok": ok }),
    );

    if ok {
        let _ = ui::success(format!("Installed: {}", targets.join(", ")));
    } else {
        let _ = ui::warning(format!("Failed to run: {command_text}"));
    }
}

// ============================================================================
// Human-mode rendering
// ============================================================================

fn status_icon(status: Status) -> console::StyledObject<&'static str> {
    match status {
        Status::Ok => style("✓").green(),
        Status::Warn => style("!").yellow(),
        Status::Error => style("✗").red(),
    }
}

fn render_report(checks: &[Check]) -> String {
    let mut report = String::new();
    let mut current_section: Option<&str> = None;

    for check in checks {
        if current_section != Some(check.section) {
            if current_section.is_some() {
                report.push('\n');
            }
            let _ = writeln!(report, "{}", style(section_heading(check.section)).bold());
            current_section = Some(check.section);
        }

        let _ = write!(
            report,
            "[{}] {}: {}",
            status_icon(check.status),
            check.title,
            check.detail
        );
        if let Some(path) = &check.path {
            let _ = write!(report, " ({})", path.display());
        }
        report.push('\n');

        if let Some(fix) = &check.fix {
            let _ = writeln!(report, "    {}", style(fix.rendered()).dim());
        }
    }

    report.trim_end().to_string()
}

fn section_heading(section: &str) -> &'static str {
    match section {
        "core" => "Core",
        "android" => "Android",
        "ios" => "iOS",
        "web" => "Web",
        _ => "Other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_token_finds_the_number() {
        assert_eq!(
            version_token("rustc 1.90.0 (abc 2024-01-01)"),
            Some("1.90.0")
        );
        assert_eq!(version_token("no digits here"), None);
    }

    #[test]
    fn major_minor_parses_two_or_three_part_versions() {
        assert_eq!(major_minor("1.97"), Some((1, 97)));
        assert_eq!(major_minor("1.97.0"), Some((1, 97)));
        assert_eq!(major_minor("1.97.0-nightly"), Some((1, 97)));
    }

    #[test]
    fn check_missing_promotes_by_flag() {
        assert_eq!(Check::missing(true), Status::Error);
        assert_eq!(Check::missing(false), Status::Warn);
    }

    #[test]
    fn targets_check_reports_missing_and_a_fix() {
        let installed = vec!["aarch64-linux-android".to_string()];
        let check = targets_check(
            "t",
            "android",
            "Android targets",
            &ANDROID_TARGETS,
            Some(&installed),
            true,
        );
        assert_eq!(check.status, Status::Error);
        match &check.fix {
            Some(Fix::RustupTargets(targets)) => {
                assert!(!targets.iter().any(|t| t == "aarch64-linux-android"));
            }
            other => panic!("expected a RustupTargets fix, got {other:?}"),
        }
    }

    #[test]
    fn targets_check_without_rustup_warns_regardless_of_promote() {
        let check = targets_check(
            "t",
            "web",
            "WASM target",
            &["wasm32-unknown-unknown"],
            None,
            true,
        );
        assert_eq!(check.status, Status::Warn);
    }

    #[test]
    fn summary_exit_code_matches_error_count() {
        let checks = vec![
            Check::ok("a", "core", "A", "fine".into()),
            Check::failing("b", "core", "B", Status::Warn, "meh".into(), None),
        ];
        let summary = summarize(&checks);
        assert_eq!(summary.exit_code, crate::error::exit_code::SUCCESS);

        let checks = vec![Check::failing(
            "c",
            "core",
            "C",
            Status::Error,
            "broken".into(),
            None,
        )];
        let summary = summarize(&checks);
        assert_eq!(summary.exit_code, crate::error::exit_code::ENVIRONMENT);
    }
}
