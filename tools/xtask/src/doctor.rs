//! Tools the local checks need, with install hints.
//!
//! `cargo xtask doctor` checks what the everyday local checks need (`checks`,
//! nextest, clippy); `cargo xtask doctor full` also what the heavier jobs need
//! (the cross-target and wasm checks, the feature matrix, cargo-deny, Miri and
//! the workflow linters). Exit 1 if any required tool is missing. It only
//! looks: it installs nothing and changes no configuration. The toolchain
//! itself is not a row: `rust-toolchain.toml` pins it, and rustup has already
//! resolved it by the time xtask runs.

use std::ffi::OsStr;
use std::io::Write as _;
use std::process::{Command, ExitCode, Stdio};

use crate::fonts::find_python;
use anyhow::Context as _;

use crate::util::{read, repo_root};

/// The lychee CI's `checks` job installs (its `LYCHEE_VERSION`), which the
/// install hint names.
const LYCHEE_VERSION: &str = "0.24.2";

/// Every version of the `[[package]]`s named `name` in a Cargo.lock, sorted
/// and without repeats. The wasm-bindgen CLI must match the locked
/// `wasm-bindgen` exactly, or `wasm-bindgen-test-runner` refuses to start, so
/// a lock with two of them has no version to install: CI's `wasm-check` job
/// fails on it, and doctor's row does too.
fn locked_package_versions(lock: &str, name: &str) -> anyhow::Result<Vec<String>> {
    let parsed: toml::Table = lock.parse().context("parsing Cargo.lock")?;
    let packages = parsed
        .get("package")
        .and_then(toml::Value::as_array)
        .context("Cargo.lock has no [[package]] array")?;
    let mut versions: Vec<String> = packages
        .iter()
        .filter(|pkg| pkg.get("name").and_then(toml::Value::as_str) == Some(name))
        .filter_map(|pkg| pkg.get("version")?.as_str())
        .map(str::to_owned)
        .collect();
    versions.sort();
    versions.dedup();
    Ok(versions)
}

/// The `wasm-bindgen` versions this repository's Cargo.lock pins, or why it
/// could not be read.
fn locked_wasm_bindgen() -> anyhow::Result<Vec<String>> {
    read("Cargo.lock").and_then(|lock| locked_package_versions(&lock, "wasm-bindgen"))
}

/// A doctor row's verdict: whether the tool is usable, what the row says, and
/// how to install it when it is not.
#[derive(Debug, PartialEq, Eq)]
struct Verdict {
    ok: bool,
    detail: String,
    install: String,
}

/// The wasm-bindgen CLI's row, from the versions Cargo.lock pins and the one
/// installed: usable only when the lock pins exactly one and it is installed.
/// A lock that cannot be read is its own failure, not a lock with no
/// wasm-bindgen.
fn wasm_bindgen_row(locked: &anyhow::Result<Vec<String>>, have: Option<&str>) -> Verdict {
    let locked = match locked {
        Ok(versions) => versions.as_slice(),
        Err(error) => {
            return Verdict {
                ok: false,
                detail: format!("{error:#}"),
                install: "repair Cargo.lock: cargo refuses it too".to_owned(),
            };
        }
    };
    match (locked, have) {
        ([want], Some(have)) if want == have => Verdict {
            ok: true,
            detail: format!("{have} (= Cargo.lock)"),
            install: String::new(),
        },
        ([want], have) => Verdict {
            ok: false,
            detail: format!(
                "have {}, Cargo.lock pins {want} (the runner refuses a mismatch)",
                have.unwrap_or("none")
            ),
            install: format!("cargo install --locked wasm-bindgen-cli --version {want}"),
        },
        ([], _) => Verdict {
            ok: false,
            detail: "Cargo.lock has no wasm-bindgen package to pin the CLI to".to_owned(),
            install: "cargo install --locked wasm-bindgen-cli --version <Cargo.lock's wasm-bindgen>"
                .to_owned(),
        },
        (several, _) => Verdict {
            ok: false,
            detail: format!(
                "Cargo.lock pins {} wasm-bindgen versions ({}); the runner matches one",
                several.len(),
                several.join(", ")
            ),
            install: "leave one wasm-bindgen in Cargo.lock (`cargo tree -i wasm-bindgen` shows who needs each)"
                .to_owned(),
        },
    }
}

/// Which set of tools is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Mode {
    /// What the everyday local checks need.
    Local,
    /// Also what the heavier jobs need.
    Full,
}

impl Mode {
    /// What the summary names as the reason a tool is needed.
    fn needed_for(self) -> &'static str {
        match self {
            Mode::Local => "the local checks",
            Mode::Full => "the full local checks",
        }
    }
}

/// Arguments for `cargo xtask doctor`.
#[derive(Debug, clap::Args)]
pub(crate) struct DoctorArgs {
    /// `local` checks what the everyday local checks need; `full` also what the
    /// heavier jobs need.
    #[arg(value_enum, default_value_t = Mode::Local)]
    mode: Mode,
}

/// Who needs a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// The everyday local checks.
    Local,
    /// Only the heavier jobs.
    Full,
    /// No check; reported for information.
    Info,
}

impl Scope {
    fn label(self) -> &'static str {
        match self {
            Scope::Local => "local",
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
    /// `None` for a tool no variable names, which fails its row.
    fn check_android_ndk(&mut self, compiler: Option<&OsStr>, archiver: Option<&OsStr>) {
        // The floor in platform-support.toml (ADR-0244); an unreadable file fails the row.
        let api = crate::platform_floor::android_api();
        let compiler_ok = compiler.is_some_and(|compiler| {
            api.as_ref()
                .is_ok_and(|api| android_compiler_available(compiler, *api))
        });
        let archiver_ok = archiver.is_some_and(android_archiver_available);
        let detail = |tool: Option<&OsStr>, variable: &str| {
            tool.map_or_else(
                || format!("{variable} not set"),
                |tool| tool.to_string_lossy().into_owned(),
            )
        };
        let hint = match &api {
            Ok(api) => format!(
                "install the Android NDK; set CC_aarch64_linux_android to its API-{api} clang wrapper and AR_aarch64_linux_android to its llvm-ar"
            ),
            Err(error) => format!("{error:#}"),
        };
        self.row(
            Scope::Full,
            "Android NDK compiler",
            compiler_ok,
            &detail(compiler, "CC_aarch64_linux_android"),
            &hint,
        );
        self.row(
            Scope::Full,
            "Android NDK archiver",
            archiver_ok,
            &detail(archiver, "AR_aarch64_linux_android"),
            &hint,
        );
    }

    fn row(&mut self, scope: Scope, name: &str, ok: bool, detail: &str, install: &str) {
        let status = if ok { "ok" } else { "MISSING" };
        println!("  {:<5} {name:<34} {status:<8} {detail}", scope.label());
        if ok {
            return;
        }
        if !install.is_empty() {
            println!("  {:<5} {:<34} {:<8}   install: {install}", "", "", "");
        }
        if scope == Scope::Local || (scope == Scope::Full && self.mode == Mode::Full) {
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
            Ok(out) if out.status.success() => {
                let version = first_line(&out.stdout);
                let detail = if version.is_empty() {
                    "present"
                } else {
                    &version
                };
                self.row(scope, bin, true, detail, "");
            }
            Ok(out) => self.row(
                scope,
                bin,
                false,
                &format!("version probe failed ({})", out.status),
                install,
            ),
            Err(_) => self.row(scope, bin, false, "not on PATH", install),
        }
    }

    /// A shared library a process loads at run time (`dlopen`), found by its
    /// `soname` in the dynamic linker's cache (`ldconfig -p`).
    fn check_library(&mut self, scope: Scope, soname: &str, install: &str) {
        let cache = ["ldconfig", "/sbin/ldconfig", "/usr/sbin/ldconfig"]
            .into_iter()
            .find_map(|ldconfig| {
                let out = Command::new(ldconfig).arg("-p").output().ok()?;
                out.status.success().then_some(out.stdout)
            });
        match cache {
            Some(cache) if String::from_utf8_lossy(&cache).contains(soname) => {
                self.row(scope, soname, true, "in the linker cache", "");
            }
            Some(_) => self.row(scope, soname, false, "not in the linker cache", install),
            None => self.row(scope, soname, false, "ldconfig did not answer", install),
        }
    }

    /// The closing line: what is missing, for which command.
    fn summary(&self) -> String {
        let needed_for = self.mode.needed_for();
        if self.missing_required > 0 {
            format!(
                "doctor: {} missing for {needed_for} -- install commands above.",
                self.missing_required
            )
        } else if self.missing_optional > 0 {
            let full = if self.mode == Mode::Local {
                " (`cargo xtask doctor full` also checks the heavier jobs)"
            } else {
                ""
            };
            format!(
                "doctor: everything {needed_for} need is here; {} optional item(s) missing{full}.",
                self.missing_optional
            )
        } else {
            format!("doctor: everything {needed_for} need is here.")
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

    fn check_full_targets(&mut self, installed: &[String]) {
        for triple in [
            "wasm32-unknown-unknown",
            "x86_64-pc-windows-msvc",
            "aarch64-apple-darwin",
            "aarch64-linux-android",
            "aarch64-apple-ios",
        ] {
            if triple == "aarch64-apple-ios" && !self.macos {
                continue;
            }
            self.check_target(Scope::Full, triple, installed);
        }
    }
}

/// Probe the configured compiler's Android target and sysroot for API `api` or later, without an
/// object file.
pub(crate) fn android_compiler_available(compiler: &OsStr, api: u32) -> bool {
    let Ok(mut child) = Command::new(compiler)
        .args(["-x", "c", "-fsyntax-only", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let input_ok = child.stdin.take().is_some_and(|mut input| {
        let probe = format!(
            "#include <android/api-level.h>\n#if !defined(__ANDROID__) || !defined(__ANDROID_API__) || __ANDROID_API__ < {api}\n#error Android API {api} or later required\n#endif\n"
        );
        input.write_all(probe.as_bytes()).is_ok()
    });
    let status = child.wait();
    input_ok && status.is_ok_and(|status| status.success())
}

/// Probe whether the selected Android archiver executes successfully.
pub(crate) fn android_archiver_available(archiver: &OsStr) -> bool {
    Command::new(archiver)
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

/// The Android compiler or archiver (`prefix` `CC` or `AR`) a cross build takes from the
/// environment, read from the variables cc-rs reads, in its order. `None` when none is set: cc-rs
/// then guesses a wrapper name, which doctor does not probe.
pub(crate) fn android_tool(prefix: &str) -> Option<std::ffi::OsString> {
    [
        format!("{prefix}_aarch64-linux-android"),
        format!("{prefix}_aarch64_linux_android"),
        format!("TARGET_{prefix}"),
        prefix.to_owned(),
    ]
    .into_iter()
    .find_map(std::env::var_os)
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
        .map_or_default(|out| {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(|line| line.trim().to_owned())
                .collect()
        })
}

/// The cargo subcommands only the heavier jobs run, each with the crate that
/// installs it: cargo-hack for the feature matrix and cargo-deny for the
/// dependency graph (`cargo deny --workspace --locked check`).
const HEAVY_CARGO_SUBS: [(&str, &str); 2] = [("hack", "cargo-hack"), ("deny", "cargo-deny")];

/// `cargo xtask doctor`: list missing tools for the local checks.
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
        Mode::Local => "local",
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
            Scope::Local,
            "python >= 3.10",
            true,
            &format!("{python} ({version})"),
            "",
        );
    } else {
        let hint = doctor.brew_or("python@3.12", "apt-get install python3");
        doctor.row(
            Scope::Local,
            "python >= 3.10",
            false,
            "none on PATH (tools/decoy-face/generate.py, run by `cargo xtask font-assets`)",
            &hint,
        );
    }

    // The everyday local checks.
    doctor.check_bin(
        Scope::Local,
        "cargo",
        "https://rustup.rs (then: rustup show, in this repo, installs the pinned toolchain)",
        &["--version"],
    );
    doctor.check_cargo_sub(
        Scope::Local,
        "nextest",
        "cargo install --locked cargo-nextest",
    );
    doctor.check_bin(
        Scope::Local,
        "typos",
        "cargo install --locked typos-cli",
        &["--version"],
    );
    doctor.check_bin(
        Scope::Local,
        "taplo",
        "cargo install --locked taplo-cli",
        &["--version"],
    );
    doctor.check_bin(
        Scope::Local,
        "lychee",
        &format!("cargo install --locked lychee --version {LYCHEE_VERSION}"),
        &["--version"],
    );

    // The heavier jobs.
    doctor.check_android_ndk(android_tool("CC").as_deref(), android_tool("AR").as_deref());
    if os != "windows" {
        doctor.check_cargo_sub(Scope::Full, "xwin", "cargo install --locked cargo-xwin --version 0.23.1 (also needs clang/LLVM and MSVC SDK/CRT provisioning)");
    }
    if os == "macos" {
        doctor.check_bin(
            Scope::Full,
            "xcrun",
            "install Xcode with the iOS SDK",
            &["--sdk", "iphoneos", "--show-sdk-path"],
        );
    } else {
        doctor.check_bin(
            Scope::Full,
            "cargo-zigbuild",
            "cargo install --locked cargo-zigbuild --version 0.23.4",
            &["--version"],
        );
        doctor.check_bin(Scope::Full, "zig", "install Zig 0.17.0 (macOS cross C headers); iOS checks require a genuine Apple SDK on macOS", &["version"]);
    }
    for (sub, krate) in HEAVY_CARGO_SUBS {
        doctor.check_cargo_sub(Scope::Full, sub, &format!("cargo install --locked {krate}"));
    }
    let have = succeeded_first_line("wasm-bindgen", &["--version"])
        .and_then(|line| line.split(' ').nth(1).map(str::to_owned));
    let row = wasm_bindgen_row(&locked_wasm_bindgen(), have.as_deref());
    doctor.row(
        Scope::Full,
        "wasm-bindgen-cli",
        row.ok,
        &row.detail,
        &row.install,
    );
    let actionlint = doctor.brew_or(
        "actionlint",
        "go install github.com/rhysd/actionlint/cmd/actionlint@latest",
    );
    doctor.check_bin(Scope::Full, "actionlint", &actionlint, &["--version"]);
    let zizmor = doctor.brew_or("zizmor", "cargo install --locked zizmor");
    doctor.check_bin(Scope::Full, "zizmor", &zizmor, &["--version"]);
    let installed = output_lines("rustup", &["target", "list", "--installed"]);
    doctor.check_full_targets(&installed);
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
            "apt-get install xvfb   (flui-native tests)",
            &["-h"],
        );
        doctor.check_bin(
            Scope::Info,
            "openbox",
            "apt-get install openbox   (the X11 host's window conformance, tests/x11_window.rs)",
            &["--version"],
        );
        doctor.check_library(
            Scope::Info,
            "libxcb.so.1",
            "apt-get install libxcb1   (the X11 host loads it at run time)",
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

    fn full_rows_are_required_only_in_full_mode() {
        let mut local = doctor(Mode::Local);
        local.row(Scope::Local, "a", false, "", "");
        local.row(Scope::Full, "b", false, "", "");
        local.row(Scope::Info, "c", false, "", "");
        local.row(Scope::Local, "d", true, "", "");
        assert_eq!((local.missing_required, local.missing_optional), (1, 2));

        let mut full = doctor(Mode::Full);
        full.row(Scope::Full, "b", false, "", "");
        full.row(Scope::Info, "c", false, "", "");
        assert_eq!((full.missing_required, full.missing_optional), (1, 1));
    }

    fn a_failing_binary_probe_is_not_an_available_tool() {
        let mut report = doctor(Mode::Local);
        report.check_bin(
            Scope::Local,
            "cargo",
            "install cargo",
            &["--no-such-flag-xtask"],
        );
        assert!(report.summary().contains("1 missing for the local checks"));
        let mut healthy = doctor(Mode::Local);
        healthy.check_bin(Scope::Local, "cargo", "install cargo", &["--version"]);
        assert!(
            healthy
                .summary()
                .contains("everything the local checks need is here")
        );
    }

    fn a_version_only_compiler_does_not_supply_android_headers() {
        let mut full = doctor(Mode::Full);
        full.check_android_ndk(Some(OsStr::new("cargo")), Some(OsStr::new("cargo")));
        assert!(
            full.summary()
                .contains("1 missing for the full local checks")
        );
        // No variable names a tool: cc-rs would guess one, so neither row passes.
        let mut unset = doctor(Mode::Full);
        unset.check_android_ndk(None, None);
        assert!(
            unset
                .summary()
                .contains("2 missing for the full local checks")
        );
    }

    /// The install hint names the lychee CI installs.
    fn the_lychee_hint_names_the_version_ci_installs() {
        let ci =
            std::fs::read_to_string(repo_root().join(".github/workflows/ci.yml")).expect("ci.yml");
        assert!(
            ci.lines()
                .any(|line| line.trim() == format!("LYCHEE_VERSION: {LYCHEE_VERSION}")),
            "ci.yml's LYCHEE_VERSION is not {LYCHEE_VERSION}"
        );
    }

    /// Every locked version of the named package, and only of that one: a
    /// lock with two `wasm-bindgen`s yields both, which doctor reports as a
    /// failed row as CI's `wasm-check` fails the job.
    fn the_locked_versions_are_the_named_packages() {
        let package = |name: &str, version: &str| {
            format!("[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n\n")
        };
        let one = format!(
            "version = 4\n\n{}{}",
            package("wasm-bindgen-macro", "9.9.9"),
            package("wasm-bindgen", "0.2.1")
        );
        assert_eq!(
            locked_package_versions(&one, "wasm-bindgen").expect("parses"),
            ["0.2.1"]
        );
        assert_eq!(
            locked_package_versions(&one, "absent").expect("parses"),
            Vec::<String>::new()
        );
        let two = format!(
            "{one}{}{}",
            package("wasm-bindgen", "0.1.9"),
            package("wasm-bindgen", "0.2.1")
        );
        assert_eq!(
            locked_package_versions(&two, "wasm-bindgen").expect("parses"),
            ["0.1.9", "0.2.1"]
        );
        let malformed = locked_package_versions("[[package]\n", "wasm-bindgen")
            .expect_err("a lock that is not TOML");
        assert_eq!(malformed.to_string(), "parsing Cargo.lock");
        let empty = locked_package_versions("version = 4\n", "wasm-bindgen")
            .expect_err("a lock with no packages");
        assert_eq!(empty.to_string(), "Cargo.lock has no [[package]] array");
    }

    /// This repository's lock pins one wasm-bindgen, the version CI's
    /// `wasm-check` installs the CLI at.
    fn this_lock_pins_one_wasm_bindgen() {
        let locked = locked_wasm_bindgen().expect("Cargo.lock reads");
        let [version] = locked.as_slice() else {
            panic!(
                "Cargo.lock pins {} wasm-bindgen versions: {locked:?}",
                locked.len()
            );
        };
        let lock = read("Cargo.lock").expect("Cargo.lock reads");
        assert!(
            lock.contains(&format!(
                "name = \"wasm-bindgen\"\nversion = \"{version}\"\n"
            )),
            "{version} is not the wasm-bindgen entry of Cargo.lock"
        );
    }

    /// The row is usable only for one locked version equal to the installed
    /// one, and each other case names what to do.
    fn the_wasm_bindgen_row_needs_one_locked_version_installed() {
        let one = Ok(vec!["0.2.1".to_owned()]);
        assert_eq!(
            wasm_bindgen_row(&one, Some("0.2.1")),
            Verdict {
                ok: true,
                detail: "0.2.1 (= Cargo.lock)".to_owned(),
                install: String::new(),
            }
        );
        for have in [Some("0.2.0"), None] {
            let row = wasm_bindgen_row(&one, have);
            assert!(!row.ok, "{have:?}");
            assert_eq!(
                row.install,
                "cargo install --locked wasm-bindgen-cli --version 0.2.1"
            );
            assert!(row.detail.contains(have.unwrap_or("none")), "{row:?}");
        }
        let none = wasm_bindgen_row(&Ok(Vec::new()), Some("0.2.1"));
        assert!(!none.ok);
        assert!(none.detail.contains("no wasm-bindgen"), "{none:?}");
        assert!(
            none.install
                .starts_with("cargo install --locked wasm-bindgen-cli"),
            "{none:?}"
        );
        let unreadable = wasm_bindgen_row(
            &locked_package_versions("[[package]\n", "wasm-bindgen"),
            Some("0.2.1"),
        );
        assert!(!unreadable.ok);
        assert!(
            unreadable.detail.starts_with("parsing Cargo.lock"),
            "an unreadable lock names why: {unreadable:?}"
        );
        assert!(
            unreadable.install.starts_with("repair Cargo.lock"),
            "{unreadable:?}"
        );
        let two = wasm_bindgen_row(
            &Ok(vec!["0.1.9".to_owned(), "0.2.1".to_owned()]),
            Some("0.2.1"),
        );
        assert!(!two.ok, "a lock with two versions has none to install");
        assert!(
            two.detail
                .contains("pins 2 wasm-bindgen versions (0.1.9, 0.2.1)"),
            "{two:?}"
        );
        assert!(two.install.starts_with("leave one wasm-bindgen"), "{two:?}");
    }

    fn only_apple_hosts_require_the_ios_target() {
        let installed = [
            "wasm32-unknown-unknown",
            "x86_64-pc-windows-msvc",
            "aarch64-apple-darwin",
            "aarch64-linux-android",
        ]
        .map(str::to_owned);
        for macos in [false, true] {
            let mut full = doctor(Mode::Full);
            full.macos = macos;
            full.check_full_targets(&installed);
            assert_eq!(full.missing_required, usize::from(macos));
        }
    }

    #[test]
    fn doctor_contract() {
        crate::table_test::run_table(
            "doctor_contract",
            &[
                (
                    "the_lychee_hint_names_the_version_ci_installs",
                    the_lychee_hint_names_the_version_ci_installs as fn(),
                ),
                (
                    "the_locked_versions_are_the_named_packages",
                    the_locked_versions_are_the_named_packages as fn(),
                ),
                (
                    "this_lock_pins_one_wasm_bindgen",
                    this_lock_pins_one_wasm_bindgen as fn(),
                ),
                (
                    "the_wasm_bindgen_row_needs_one_locked_version_installed",
                    the_wasm_bindgen_row_needs_one_locked_version_installed as fn(),
                ),
                (
                    "only_apple_hosts_require_the_ios_target",
                    only_apple_hosts_require_the_ios_target as fn(),
                ),
                (
                    "a_version_only_compiler_does_not_supply_android_headers",
                    a_version_only_compiler_does_not_supply_android_headers as fn(),
                ),
                (
                    "a_failing_binary_probe_is_not_an_available_tool",
                    a_failing_binary_probe_is_not_an_available_tool as fn(),
                ),
                (
                    "full_rows_are_required_only_in_full_mode",
                    full_rows_are_required_only_in_full_mode as fn(),
                ),
            ],
        );
    }
}
