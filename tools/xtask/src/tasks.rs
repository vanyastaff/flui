//! The tasks that choose packages, targets or tools from the workspace and
//! the host: `checks`, `cross-typecheck`, `wasm-link` and `wasm-test`, and
//! `clean-nested`.
//!
//! Each task prints every command before it runs it and stops at the first
//! failure, except `checks`, which runs every step and then reports all that
//! failed. `--dry-run` prints the commands without running them.

mod checks;
mod exec;
mod fuzz;
mod native;
mod web;

use std::process::ExitCode;

use exec::{Host, Runner, target_dir};
pub(crate) use fuzz::{FuzzArgs, fuzz};

use crate::util::{human_size, tree_size};

const WINDOWS_TARGET: &str = "x86_64-pc-windows-msvc";
const MACOS_TARGET: &str = "aarch64-apple-darwin";
const ANDROID_TARGET: &str = "aarch64-linux-android";
/// The device triple, not `-sim`: the two differ in the slice, not in the API
/// surface a lint sees.
const IOS_TARGET: &str = "aarch64-apple-ios";
const WASM_TARGET: &str = "wasm32-unknown-unknown";

/// flui-native's backends as they ship: MSVC, not
/// the GNU ABI, and aarch64 for macOS, Android and iOS.
const PLATFORM_TARGETS: [&str; 4] = [WINDOWS_TARGET, MACOS_TARGET, ANDROID_TARGET, IOS_TARGET];

/// Options every task takes.
#[derive(Debug, Clone, Copy, clap::Args)]
pub(crate) struct RunOpts {
    /// Print the commands instead of running them.
    #[arg(long)]
    pub(crate) dry_run: bool,
}

impl RunOpts {
    fn runner(self) -> Runner {
        Runner {
            dry_run: self.dry_run,
        }
    }
}

/// `-- -D warnings`: every lint step denies warnings, as CI does.
fn deny_warnings(cmd: exec::Cmd) -> exec::Cmd {
    cmd.args(["--", "-D", "warnings"])
}

/// The nested-cargo tests' build caches, relative to the target directory:
/// separate target roots that `cargo sweep` cannot see.
const NESTED_CACHES: [&[&str]; 3] = [
    &["cli-template-check"],
    &["facade-consumer-check"],
    &["tests", "trybuild"],
];

/// `Ok(ExitCode::SUCCESS)` once `stage` succeeded.
fn done(stage: anyhow::Result<()>) -> anyhow::Result<ExitCode> {
    stage.map(|()| ExitCode::SUCCESS)
}
/// Arguments for `cargo xtask checks`.
#[derive(Debug, clap::Args)]
pub(crate) struct ChecksArgs {
    /// Fail when typos, taplo or lychee is not installed instead of skipping it (CI).
    #[arg(long)]
    strict: bool,
    #[command(flatten)]
    pub(crate) run: RunOpts,
}

/// `cargo xtask checks`: run the source checks that need no workspace build (the CI `checks` job).
///
/// `cargo fmt --check`, typos, taplo, and this crate's own checks in-process:
/// `docs-links` (lychee), `toolchain`, `unmounted-tests`, `adr-numbers`,
/// `changelog --check` and the generated files against their sources
/// (`font-assets --package-list` and the rest). All of them run; exit 1 if any failed.
pub(crate) fn checks(args: &ChecksArgs) -> anyhow::Result<ExitCode> {
    done(checks::run(args.run.runner(), args.strict))
}

/// Arguments for `cargo xtask cross-typecheck`.
#[derive(Debug, clap::Args)]
pub(crate) struct CrossTypecheckArgs {
    #[command(flatten)]
    pub(crate) run: RunOpts,
    /// Check only this native triple (CI runs the triples in parallel).
    #[arg(long, value_parser = PLATFORM_TARGETS)]
    pub(crate) target: Option<String>,
    /// Enable every feature of the selected packages (Apple targets need a genuine SDK).
    #[arg(long)]
    pub(crate) all_features: bool,
}

/// `cargo xtask cross-typecheck`: lint discovered native workspace targets without linking.
///
/// Needs `rustup target add x86_64-pc-windows-msvc aarch64-apple-darwin
/// aarch64-linux-android aarch64-apple-ios`.
pub(crate) fn cross_typecheck(args: &CrossTypecheckArgs) -> anyhow::Result<ExitCode> {
    done(args.run.runner().steps(&native::plan(
        &crate::util::repo_root(),
        "",
        Host::current(),
        args.target.as_deref(),
        if args.all_features {
            native::FeatureSet::All
        } else {
            native::FeatureSet::RequiredTargets
        },
    )?))
}

/// Arguments for `cargo xtask wasm-link`.
#[derive(Debug, clap::Args)]
pub(crate) struct WasmLinkArgs {
    #[command(flatten)]
    pub(crate) run: RunOpts,
}

/// `cargo xtask wasm-link`: link the wasm demos and check their imports.
pub(crate) fn wasm_link(args: &WasmLinkArgs) -> anyhow::Result<ExitCode> {
    done(web::link(args.run.runner()))
}

/// Arguments for `cargo xtask wasm-test`.
#[derive(Debug, clap::Args)]
pub(crate) struct WasmTestArgs {
    #[command(flatten)]
    pub(crate) run: RunOpts,
}

/// `cargo xtask wasm-test`: run the wasm32 tests under wasm-bindgen-test-runner.
///
/// Installs the wasm-bindgen-cli matching Cargo.lock when it is missing.
pub(crate) fn wasm_test(args: &WasmTestArgs) -> anyhow::Result<ExitCode> {
    done(web::test(args.run.runner()))
}

/// Arguments for `cargo xtask clean-nested`.
#[derive(Debug, clap::Args)]
pub(crate) struct CleanNestedArgs {
    #[command(flatten)]
    pub(crate) run: RunOpts,
}

/// `cargo xtask clean-nested`: remove the nested-cargo test caches under the target directory.
///
/// `cli-template-check/`, `facade-consumer-check/` and `tests/trybuild/`; the
/// next nextest run that needs them rebuilds them (~9 min cold). A test run using
/// them must not overlap it: run as `cargo heli xtask clean-nested`, which the
/// repository's `.config/heli.toml` makes wait for every build and test heli
/// runs in this checkout. Run directly, it says so before deleting anything.
pub(crate) fn clean_nested(args: &CleanNestedArgs) -> anyhow::Result<ExitCode> {
    let target = target_dir()?;
    if !args.run.dry_run && std::env::var_os("HELI_RUN").is_none_or(|run| run.is_empty()) {
        println!(
            "clean-nested: not run through heli, so nothing waits for a test using these caches; \
             `cargo heli xtask clean-nested` waits for this checkout's builds first"
        );
    }
    let mut found = 0;
    for rel in NESTED_CACHES {
        let mut dir = target.clone();
        dir.extend(rel);
        if !dir.is_dir() {
            continue;
        }
        found += 1;
        let size = human_size(tree_size(&dir));
        if args.run.dry_run {
            println!("would remove {size} {}", dir.display());
            continue;
        }
        println!("removing {size} {}", dir.display());
        std::fs::remove_dir_all(&dir)
            .map_err(|error| anyhow::anyhow!("removing {}: {error}", dir.display()))?;
    }
    if found == 0 {
        println!(
            "clean-nested: no nested-cargo cache under {}",
            target.display()
        );
    }
    Ok(ExitCode::SUCCESS)
}
