//! The wasm32 tasks: `wasm-check`, `wasm-link` and `wasm-test`, CI's
//! `wasm-check` job in three steps.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use anyhow::{Context, bail, ensure};
use regex::Regex;

use super::exec::{Cmd, Runner, Step, parsed, target_dir};
use super::{WASM_TARGET, deny_warnings, wasm_facade_check};
use crate::util::repo_root;
use crate::{change_scope, wasm};

/// The two web demos, as cdylibs: `(package, artifact name)`.
const LINKED: [(&str, &str); 2] = [
    ("flui-web-demo", "flui_web_demo"),
    ("flui-painting-demo", "flui_painting_demo"),
];

static TEST_RESULT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^test result: ok\. ([0-9]+) passed").expect("BUG: static regex"));

/// Check and clippy of the wasm-capable workspace, then the facade's
/// hot-reload feature. `no_wasm` are the packages that cannot target wasm32.
/// The clippy pass is lib/bin targets only (test targets pull native-only
/// dev-dependencies such as tokio), and it is the only lint pass over
/// flui-platform's wasm32-only web backend.
fn check_plan(no_wasm: &BTreeSet<String>) -> Vec<Step> {
    let excludes: Vec<String> = no_wasm
        .iter()
        .flat_map(|name| ["--exclude".to_owned(), name.clone()])
        .collect();
    vec![
        Cmd::cargo(["check", "--workspace", "--locked", "--target", WASM_TARGET])
            .args(excludes.iter().cloned())
            .into(),
        deny_warnings(
            Cmd::cargo([
                "clippy",
                "--workspace",
                "--lib",
                "--bins",
                "--locked",
                "--target",
                WASM_TARGET,
            ])
            .args(excludes),
        )
        .into(),
        wasm_facade_check().into(),
    ]
}

/// `cargo xtask wasm-check`.
pub(super) fn check(runner: Runner) -> anyhow::Result<()> {
    let no_wasm = change_scope::no_wasm_packages()?;
    // an empty set would silently check packages that cannot build for wasm32
    ensure!(
        !no_wasm.is_empty(),
        "no package sets `[package.metadata.flui] wasm = false`; flui-cli and xtask cannot build for wasm32"
    );
    runner.steps(&check_plan(&no_wasm))
}

/// Where cargo puts `artifact`'s linked module for wasm32.
fn wasm_module(target_dir: &Path, artifact: &str) -> PathBuf {
    target_dir
        .join(WASM_TARGET)
        .join("debug")
        .join(format!("{artifact}.wasm"))
}

/// `cargo xtask wasm-link`: really link the two demos, then check their import
/// surface. `cargo check` does not link, and on wasm32 even a link does not
/// fail on an undefined symbol (rust-lld turns it into an import), hence the
/// committed import allowlist `wasm-imports` checks against.
pub(super) fn link(runner: Runner) -> anyhow::Result<()> {
    let mut build = Cmd::cargo(["build", "--locked", "--target", WASM_TARGET]);
    for (package, _) in LINKED {
        build = build.args(["-p", package]);
    }
    runner.run(&build)?;
    let target = target_dir()?;
    let modules: Vec<PathBuf> = LINKED
        .iter()
        .map(|(_, artifact)| wasm_module(&target, artifact))
        .collect();
    let shown: Vec<String> = modules.iter().map(|m| m.display().to_string()).collect();
    runner.in_process(&format!("wasm-imports {}", shown.join(" ")), || {
        wasm::wasm_imports(&parsed(modules.iter().map(OsString::from))?)
    })
}

/// The crates whose tests run on wasm32, from this binary's own
/// `wasm-test-crates` (the wasm module exposes that list only as the command's
/// output).
fn test_crates() -> anyhow::Result<Vec<String>> {
    let exe = std::env::current_exe().context("locating the xtask binary")?;
    let output = Command::new(&exe)
        .arg("wasm-test-crates")
        .current_dir(repo_root())
        .stderr(Stdio::inherit())
        .output()
        .context("running `cargo xtask wasm-test-crates`")?;
    if !output.status.success() {
        bail!("`cargo xtask wasm-test-crates` failed ({})", output.status);
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Which of a crate's test targets run on wasm32.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// The lib test: the only way to reach a `pub(crate)` seam (flui-app's
    /// `Backend::Sequential`, for one).
    Lib,
    /// `tests/wasm32.rs`, which sees only the public API.
    Integration,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Self::Lib => "lib",
            Self::Integration => "integration",
        }
    }
}

/// The wasm32 test run of `package`'s `kind` target.
///
/// `CARGO_BUILD_WARNINGS=warn`, as CI's step sets it: the wasm lib-test
/// configuration legitimately has dead code (its callers are the native-only
/// test modules), and a local green must mean what CI's does. The shipping
/// wasm configuration is still linted under deny by `wasm-check`.
fn test_cmd(package: &str, kind: Kind) -> Cmd {
    let target = match kind {
        Kind::Lib => vec!["--lib"],
        Kind::Integration => vec!["--test", "wasm32"],
    };
    Cmd::cargo(["test", "-p", package, "--locked", "--target", WASM_TARGET])
        .args(target)
        .env("CARGO_BUILD_WARNINGS", "warn")
}

/// The assertions a libtest-style run reports passing: the last
/// `test result: ok. N passed` line.
fn passed(output: &str) -> Option<u64> {
    output
        .lines()
        .filter_map(|line| TEST_RESULT.captures(line)?[1].parse().ok())
        .next_back()
}

/// The version `wasm-bindgen --version` reports, when it is installed.
fn installed_wasm_bindgen() -> Option<String> {
    let output = Command::new("wasm-bindgen")
        .arg("--version")
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output.status.success().then_some(())?;
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .nth(1)
        .map(str::to_owned)
}

/// `cargo xtask wasm-test`: execute the wasm32 tests on node, through the
/// runner `.cargo/config.toml` names. Compiling for wasm32 is not running on
/// it (issue #985): until this ran, every "works on the web" claim rested on
/// the linker succeeding.
pub(super) fn test(runner: Runner) -> anyhow::Result<()> {
    // The runner must match the LOCKED wasm-bindgen exactly or it refuses to
    // start, so the version comes from Cargo.lock: a dependabot bump moves the
    // tool with the lock instead of failing like a toolchain fault.
    let want =
        wasm::repo_locked_version("wasm-bindgen")?.context("wasm-bindgen is not in Cargo.lock")?;
    let have = installed_wasm_bindgen();
    if have.as_deref() != Some(want.as_str()) {
        eprintln!(
            "wasm-bindgen-cli {want} required (have: {}); installing",
            have.as_deref().unwrap_or("none")
        );
        runner.run(&Cmd::cargo([
            "install",
            "wasm-bindgen-cli",
            "--version",
            &want,
            "--locked",
        ]))?;
    }
    // Discovered, not listed: a list here would silently never run the next
    // crate to add wasm tests. The opt-in is the crate's own wasm32
    // dev-dependency on wasm-bindgen-test.
    let crates = test_crates()?;
    // a loop over nothing exits 0, which looks exactly like success
    ensure!(
        !crates.is_empty(),
        "wasm-test: no crate declares a wasm32 wasm-bindgen-test dev-dependency"
    );
    let mut total = 0;
    for name in &crates {
        let mut kinds = vec![Kind::Lib];
        if repo_root()
            .join("crates")
            .join(name)
            .join("tests")
            .join("wasm32.rs")
            .is_file()
        {
            kinds.push(Kind::Integration);
        }
        let mut crate_total = 0;
        for kind in kinds {
            let cmd = test_cmd(name, kind);
            println!("$ {cmd}");
            if runner.dry_run {
                continue;
            }
            // the output is echoed as it arrives, so a failure's panic and
            // the runner's own diagnostics are always shown
            let (ok, output) = cmd.merged()?;
            if !ok {
                bail!(
                    "wasm-test: {name} ({} target) failed -- see the output above",
                    kind.label()
                );
            }
            crate_total += passed(&output).unwrap_or(0);
        }
        // Per crate, not only in aggregate: a newly opted-in crate with no
        // executing test adds 0 while a healthy sibling keeps the total
        // non-zero. Not per target: a crate legitimately has only one kind.
        if !runner.dry_run && crate_total == 0 {
            bail!("wasm-test: {name} opted in but executed no wasm32 assertions -- inert");
        }
        total += crate_total;
    }
    if !runner.dry_run {
        println!(
            "wasm-test: {total} wasm32 assertions executed across {} crate(s)",
            crates.len()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasm_check_excludes_what_ci_excludes() {
        let no_wasm = change_scope::no_wasm_packages().expect("cargo metadata");
        assert!(no_wasm.contains("flui-cli"), "{no_wasm:?}");
        let lines: Vec<String> = check_plan(&no_wasm)
            .iter()
            .map(ToString::to_string)
            .collect();
        let excludes = "--exclude flui-assets --exclude flui-cli --exclude flui-desktop-mcp --exclude flui-web-server --exclude hot-reload-counter-host --exclude hot-reload-counter-logic --exclude hot-reload-counter-types --exclude xtask";
        assert_eq!(
            lines,
            [
                format!(
                    "$ cargo check --workspace --locked --target wasm32-unknown-unknown {excludes}"
                ),
                format!(
                    "$ cargo clippy --workspace --lib --bins --locked --target wasm32-unknown-unknown {excludes} -- -D warnings"
                ),
                "$ cargo check -p flui --locked --target wasm32-unknown-unknown --no-default-features --features hot-reload".to_owned(),
            ]
        );
    }

    #[test]
    fn linked_modules_live_under_the_resolved_target_dir() {
        let target = Path::new("D:/elsewhere/target-x");
        assert_eq!(
            wasm_module(target, "flui_web_demo"),
            target
                .join("wasm32-unknown-unknown")
                .join("debug")
                .join("flui_web_demo.wasm")
        );
    }

    #[test]
    fn test_commands_per_kind() {
        assert_eq!(
            test_cmd("flui-app", Kind::Lib).to_string(),
            "CARGO_BUILD_WARNINGS=warn cargo test -p flui-app --locked --target wasm32-unknown-unknown --lib"
        );
        assert_eq!(
            test_cmd("flui-foundation", Kind::Integration).to_string(),
            "CARGO_BUILD_WARNINGS=warn cargo test -p flui-foundation --locked --target wasm32-unknown-unknown --test wasm32"
        );
    }

    #[test]
    fn the_last_ok_result_line_counts() {
        let run = "running 3 tests\r\ntest result: ok. 3 passed; 0 failed; 0 ignored\r\n\
                   Running tests/wasm32.rs\ntest result: ok. 12 passed; 0 failed\n";
        assert_eq!(passed(run), Some(12));
        assert_eq!(passed("test result: FAILED. 1 passed; 2 failed\n"), None);
        assert_eq!(passed("no tests to run\n"), None);
        assert_eq!(passed("  test result: ok. 4 passed\n"), None);
    }
}
