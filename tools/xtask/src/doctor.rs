//! Tools the local gates need, with install hints.
//!
//! `cargo xtask doctor` checks what `cargo xtask ci` needs; `cargo xtask
//! doctor full` also what `cargo xtask ci-full` needs. Exit 1 if any required
//! tool is missing. It only looks: it installs nothing and changes no
//! configuration. The toolchain itself is not a row: `rust-toolchain.toml`
//! pins it, and rustup has already resolved it by the time xtask runs.

use std::process::{Command, ExitCode};

use crate::docs_links;
use crate::fonts::find_python;
use crate::tasks;
use crate::util::repo_root;
use crate::wasm::repo_locked_version;

/// Which command's needs are required.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Mode {
    /// What `cargo xtask ci` needs.
    Ci,
    /// Also what `cargo xtask ci-full` needs.
    Full,
}

impl Mode {
    /// The xtask command whose needs this mode checks.
    fn command(self) -> &'static str {
        match self {
            Mode::Ci => "ci",
            Mode::Full => "ci-full",
        }
    }
}

/// Arguments for `cargo xtask doctor`.
#[derive(Debug, clap::Args)]
pub(crate) struct DoctorArgs {
    /// `ci` checks what `cargo xtask ci` needs; `full` also what
    /// `cargo xtask ci-full` needs.
    #[arg(value_enum, default_value_t = Mode::Ci)]
    mode: Mode,
}

/// Who needs a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// `cargo xtask ci`.
    Ci,
    /// Only `cargo xtask ci-full`.
    Full,
    /// No command; reported for information.
    Info,
}

impl Scope {
    fn label(self) -> &'static str {
        match self {
            Scope::Ci => "ci",
            Scope::Full => "full",
            Scope::Info => "info",
        }
    }
}

/// The report being printed, and its tallies.
struct Doctor {
    mode: Mode,
    macos: bool,
    missing_required: usize,
    missing_optional: usize,
}

impl Doctor {
    fn row(&mut self, scope: Scope, name: &str, ok: bool, detail: &str, install: &str) {
        let status = if ok { "ok" } else { "MISSING" };
        println!("  {:<5} {name:<34} {status:<8} {detail}", scope.label());
        if ok {
            return;
        }
        if !install.is_empty() {
            println!("  {:<5} {:<34} {:<8}   install: {install}", "", "", "");
        }
        if scope == Scope::Ci || (scope == Scope::Full && self.mode == Mode::Full) {
            self.missing_required += 1;
        } else {
            self.missing_optional += 1;
        }
    }

    /// The install hint for this host: brew on macOS, `other` elsewhere.
    fn brew_or(&self, formula: &str, other: &str) -> String {
        if self.macos {
            format!("brew install {formula}")
        } else {
            other.to_owned()
        }
    }

    fn check_bin(&mut self, scope: Scope, bin: &str, install: &str, version_args: &[&str]) {
        match Command::new(bin).args(version_args).output() {
            Ok(out) => {
                let version = first_line(&out.stdout);
                let detail = if version.is_empty() {
                    "present"
                } else {
                    &version
                };
                self.row(scope, bin, true, detail, "");
            }
            Err(_) => self.row(scope, bin, false, "not on PATH", install),
        }
    }

    /// The closing line: what is missing, for which command.
    fn summary(&self) -> String {
        let command = self.mode.command();
        if self.missing_required > 0 {
            format!(
                "doctor: {} missing for `cargo xtask {command}` -- install commands above.",
                self.missing_required
            )
        } else if self.missing_optional > 0 {
            let full = if self.mode == Mode::Ci {
                " (`cargo xtask doctor full` also checks ci-full)"
            } else {
                ""
            };
            format!(
                "doctor: everything `cargo xtask {command}` needs is here; {} optional item(s) missing{full}.",
                self.missing_optional
            )
        } else {
            format!("doctor: everything `cargo xtask {command}` needs is here.")
        }
    }

    fn check_cargo_sub(&mut self, scope: Scope, sub: &str, install: &str) {
        let name = format!("cargo {sub}");
        match succeeded_first_line("cargo", &[sub, "--version"]) {
            Some(version) => self.row(scope, &name, true, &version, ""),
            None => self.row(scope, &name, false, "not installed", install),
        }
    }

    fn check_target(&mut self, scope: Scope, triple: &str, installed: &[String]) {
        let name = format!("target {triple}");
        if installed.iter().any(|t| t == triple) {
            self.row(scope, &name, true, "installed", "");
        } else {
            self.row(
                scope,
                &name,
                false,
                "rustup target not installed",
                &format!("rustup target add {triple}"),
            );
        }
    }
}

fn first_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned()
}

/// The first stdout line of a command that ran and succeeded with output.
fn succeeded_first_line(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    let line = first_line(&out.stdout);
    (out.status.success() && !line.is_empty()).then_some(line)
}

/// Every stdout line of a successful command; empty when it cannot run.
fn output_lines(program: &str, args: &[&str]) -> Vec<String> {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(|line| line.trim().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// The cargo subcommands only `cargo xtask ci-full` runs, each with its
/// install command: cargo-hack for the feature matrix, and every tool
/// `cargo xtask deps` runs.
fn ci_full_cargo_subs() -> Vec<(&'static str, String)> {
    let mut subs = vec![("hack", "cargo install --locked cargo-hack".to_owned())];
    subs.extend(
        tasks::deps_tools()
            .map(|(sub, install)| (sub, format!("cargo install --locked {install}"))),
    );
    subs
}

/// `cargo xtask doctor`: list missing tools for `cargo xtask ci` / `ci-full`.
#[expect(
    clippy::unnecessary_wraps,
    reason = "every command shares the dispatch signature in main.rs"
)]
pub(crate) fn doctor(args: &DoctorArgs) -> anyhow::Result<ExitCode> {
    let os = std::env::consts::OS;
    let mut doctor = Doctor {
        mode: args.mode,
        macos: os == "macos",
        missing_required: 0,
        missing_optional: 0,
    };
    let mode_name = match args.mode {
        Mode::Ci => "ci",
        Mode::Full => "full",
    };
    println!(
        "flui doctor ({mode_name}) -- {os}, repo {}",
        repo_root().display()
    );
    println!("  {:<5} {:<34} {:<8} detail", "scope", "name", "status");

    // Interpreters: the gates are Rust now, so bash is no longer needed; Python
    // still is, for the font generator that `font-assets` replays.
    if let Some(python) = find_python() {
        let version = succeeded_first_line(&python, &["--version"]).unwrap_or_default();
        doctor.row(
            Scope::Ci,
            "python >= 3.10",
            true,
            &format!("{python} ({version})"),
            "",
        );
    } else {
        let hint = doctor.brew_or("python@3.12", "apt-get install python3");
        doctor.row(
            Scope::Ci,
            "python >= 3.10",
            false,
            "none on PATH (tools/decoy-face/generate.py, run by `cargo xtask font-assets`)",
            &hint,
        );
    }

    // `cargo xtask ci`
    doctor.check_bin(
        Scope::Ci,
        "cargo",
        "https://rustup.rs (then: rustup show, in this repo, installs the pinned toolchain)",
        &["--version"],
    );
    doctor.check_cargo_sub(Scope::Ci, "nextest", "cargo install --locked cargo-nextest");
    doctor.check_bin(
        Scope::Ci,
        "typos",
        "cargo install --locked typos-cli",
        &["--version"],
    );
    doctor.check_bin(
        Scope::Ci,
        "taplo",
        "cargo install --locked taplo-cli",
        &["--version"],
    );
    doctor.check_bin(
        Scope::Ci,
        "lychee",
        &docs_links::install_hint(),
        &["--version"],
    );

    // `cargo xtask ci-full`
    for (sub, install) in ci_full_cargo_subs() {
        doctor.check_cargo_sub(Scope::Full, sub, &install);
    }
    let want = repo_locked_version("wasm-bindgen").ok().flatten();
    let have = succeeded_first_line("wasm-bindgen", &["--version"])
        .and_then(|line| line.split(' ').nth(1).map(str::to_owned));
    match (&want, &have) {
        (Some(want), Some(have)) if want == have => {
            doctor.row(
                Scope::Full,
                "wasm-bindgen-cli",
                true,
                &format!("{have} (= Cargo.lock)"),
                "",
            );
        }
        (Some(want), have) => doctor.row(
            Scope::Full,
            "wasm-bindgen-cli",
            false,
            &format!(
                "have {}, Cargo.lock pins {want} (the runner refuses a mismatch)",
                have.as_deref().unwrap_or("none")
            ),
            &format!("cargo install --locked wasm-bindgen-cli --version {want}"),
        ),
        (None, _) => doctor.row(
            Scope::Full,
            "wasm-bindgen-cli",
            false,
            "Cargo.lock has no wasm-bindgen package to pin the CLI to",
            "cargo install --locked wasm-bindgen-cli --version <Cargo.lock's wasm-bindgen>",
        ),
    }
    let actionlint = doctor.brew_or(
        "actionlint",
        "go install github.com/rhysd/actionlint/cmd/actionlint@latest",
    );
    doctor.check_bin(Scope::Full, "actionlint", &actionlint, &["--version"]);
    let zizmor = doctor.brew_or("zizmor", "cargo install --locked zizmor");
    doctor.check_bin(Scope::Full, "zizmor", &zizmor, &["--version"]);
    let installed = output_lines("rustup", &["target", "list", "--installed"]);
    for triple in [
        "wasm32-unknown-unknown",
        "x86_64-pc-windows-msvc",
        "aarch64-apple-darwin",
        "aarch64-linux-android",
        "aarch64-apple-ios",
    ] {
        doctor.check_target(Scope::Full, triple, &installed);
    }
    match succeeded_first_line("rustup", &["run", "nightly", "cargo", "miri", "--version"]) {
        Some(version) => doctor.row(Scope::Full, "nightly + miri", true, &version, ""),
        None => doctor.row(
            Scope::Full,
            "nightly + miri",
            false,
            "no nightly toolchain with miri",
            "rustup toolchain install nightly --component miri",
        ),
    }

    // Informational: useful on this host, required by no command.
    if os == "linux" {
        doctor.check_bin(
            Scope::Info,
            "xvfb-run",
            "apt-get install xvfb   (flui-platform tests, live-smoke)",
            &["-h"],
        );
    }
    doctor.check_cargo_sub(
        Scope::Info,
        "sweep",
        "cargo install --locked cargo-sweep   (bounds the target directory, docs/testing.md)",
    );

    println!();
    println!("{}", doctor.summary());
    Ok(if doctor.missing_required > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doctor(mode: Mode) -> Doctor {
        Doctor {
            mode,
            macos: false,
            missing_required: 0,
            missing_optional: 0,
        }
    }

    #[test]
    fn full_rows_are_required_only_in_full_mode() {
        let mut ci = doctor(Mode::Ci);
        ci.row(Scope::Ci, "a", false, "", "");
        ci.row(Scope::Full, "b", false, "", "");
        ci.row(Scope::Info, "c", false, "", "");
        ci.row(Scope::Ci, "d", true, "", "");
        assert_eq!((ci.missing_required, ci.missing_optional), (1, 2));

        let mut full = doctor(Mode::Full);
        full.row(Scope::Full, "b", false, "", "");
        full.row(Scope::Info, "c", false, "", "");
        assert_eq!((full.missing_required, full.missing_optional), (1, 1));
    }

    #[test]
    fn full_mode_checks_every_tool_the_deps_gate_runs() {
        let subs = ci_full_cargo_subs();
        for (sub, install) in tasks::deps_tools() {
            assert!(
                subs.contains(&(sub, format!("cargo install --locked {install}"))),
                "`cargo xtask doctor full` does not check cargo {sub}: {subs:?}"
            );
        }
    }

    #[test]
    fn install_hint_prefers_brew_on_macos() {
        let mut host = doctor(Mode::Ci);
        assert_eq!(host.brew_or("x", "apt-get install x"), "apt-get install x");
        host.macos = true;
        assert_eq!(host.brew_or("x", "apt-get install x"), "brew install x");
    }

    #[test]
    fn a_missing_binary_is_reported_missing() {
        let mut host = doctor(Mode::Ci);
        host.check_bin(Scope::Ci, "flui-xtask-no-such-binary", "", &["--version"]);
        assert_eq!(host.missing_required, 1);
    }

    #[test]
    fn the_summary_names_the_xtask_command_it_checked() {
        let mut ci = doctor(Mode::Ci);
        assert_eq!(
            ci.summary(),
            "doctor: everything `cargo xtask ci` needs is here."
        );
        ci.row(Scope::Full, "b", false, "", "");
        assert_eq!(
            ci.summary(),
            "doctor: everything `cargo xtask ci` needs is here; 1 optional item(s) missing \
             (`cargo xtask doctor full` also checks ci-full)."
        );

        let mut full = doctor(Mode::Full);
        full.row(Scope::Info, "c", false, "", "");
        assert_eq!(
            full.summary(),
            "doctor: everything `cargo xtask ci-full` needs is here; 1 optional item(s) missing."
        );
        full.row(Scope::Full, "b", false, "", "");
        assert_eq!(
            full.summary(),
            "doctor: 1 missing for `cargo xtask ci-full` -- install commands above."
        );
    }
}
