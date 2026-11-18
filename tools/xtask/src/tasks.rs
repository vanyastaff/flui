//! The tasks that choose packages, features or targets from the workspace:
//! `checks`, `feature-matrix`, `facade-combos`, `cross-typecheck` and the
//! `wasm-*` steps, and `clean-nested`.
//!
//! Each task prints every command before it runs it and stops at the first
//! failure, except `checks` and `feature-matrix`, which run every step and then
//! report all that failed. `--dry-run` prints the commands without running
//! them. The command lists are built by plain functions ([`feature_matrix_plan`],
//! [`feature_scope_plan`], ...) so the unit tests below pin them to the CI
//! steps they mirror.

mod checks;
mod exec;
pub(crate) mod facade;
mod fuzz;
mod native;
mod web;

use std::process::ExitCode;

use exec::{Cmd, Host, Runner, Step, target_dir};
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
fn deny_warnings(cmd: Cmd) -> Cmd {
    cmd.args(["--", "-D", "warnings"])
}

/// `flui-hot-reload` with every feature on wasm32: it has no web runner, but an
/// application that keeps the package in a cross-target build must still build
/// for the web rather than meet native-only imports.
fn wasm_hot_reload_check() -> Cmd {
    Cmd::cargo([
        "check",
        "-p",
        "flui-hot-reload",
        "--locked",
        "--target",
        WASM_TARGET,
        "--all-features",
    ])
}

/// cargo-hack's per-feature clippy over `packages` (`--workspace` or `-p ...`),
/// in two passes: libs/bins without dev targets (the library as its consumers
/// see it: under resolver v2 a dev-dependency's features unify only while dev
/// targets build, so a single `--all-targets` pass can hide a missing feature
/// edge), then the tests, benches and examples themselves.
fn hack_passes(packages: &str) -> [Cmd; 2] {
    let pass = || {
        Cmd::cargo(["hack", "clippy"]).split(packages).args([
            "--locked",
            "--each-feature",
            "--keep-going",
        ])
    };
    [
        deny_warnings(pass()),
        deny_warnings(pass().args(["--tests", "--benches", "--examples"])),
    ]
}

/// One slice of CI's `feature-matrix` job, or all of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slice {
    /// The combinations, then the per-feature passes over the whole workspace.
    All,
    /// Shard `index` of `count` of the per-feature passes. cargo-hack's
    /// `--partition` splits its command list into contiguous blocks, so the
    /// shards cover every package and feature exactly once by construction,
    /// as long as every shard runs the same cargo-hack version.
    Shard { index: usize, count: usize },
    /// Each supported facade feature combination, built on its own.
    Combinations,
}

impl Slice {
    fn parse(text: &str) -> Result<Self, String> {
        match text {
            "all" => return Ok(Self::All),
            "combinations" => return Ok(Self::Combinations),
            _ => {}
        }
        let shard = text.split_once('/').and_then(|(index, count)| {
            let index = index.parse().ok()?;
            let count = count.parse().ok()?;
            (1..=count)
                .contains(&index)
                .then_some(Self::Shard { index, count })
        });
        shard.ok_or_else(|| {
            format!("expected `all`, `combinations` or `k/N` with 1 <= k <= N, got `{text}`")
        })
    }
}

/// The per-feature passes of `slice`; none for the combinations.
///
/// flui-engine's wgpu backend features get no combination run of their own:
/// they only forward to wgpu's features and no FLUI code is behind them, so
/// every pair compiles the same FLUI code, and each backend already builds on
/// supported targets in local checks and the wasm-check CI job.
fn feature_matrix_plan(slice: Slice) -> Vec<Step> {
    let packages = match slice {
        Slice::All => "--workspace".to_owned(),
        Slice::Shard { index, count } => format!("--workspace --partition {index}/{count}"),
        Slice::Combinations => return Vec::new(),
    };
    hack_passes(&packages).map(Step::from).into()
}

/// The nested-cargo tests' build caches, relative to the target directory:
/// separate target roots that `cargo sweep` cannot see.
const NESTED_CACHES: [&[&str]; 3] = [
    &["cli-template-check"],
    &["facade-consumer-check"],
    &["tests", "trybuild"],
];

/// Runs every step of `slice` even after one fails, so one run reports each
/// broken configuration.
fn feature_matrix_stage(runner: Runner, slice: Slice) -> anyhow::Result<()> {
    let combos = match slice {
        Slice::Shard { .. } => Ok(()),
        Slice::All | Slice::Combinations => facade::run(runner),
    };
    let passes = runner.every(&feature_matrix_plan(slice));
    combos.and(passes)
}

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

/// Arguments for `cargo xtask feature-matrix`.
#[derive(Debug, clap::Args)]
pub(crate) struct FeatureMatrixArgs {
    /// `all`, `combinations` (the facade feature combinations) or `k/N`
    /// (shard k of N of the per-feature passes).
    #[arg(long, default_value = "all", value_parser = Slice::parse)]
    slice: Slice,
    /// Instead of the workspace, the per-feature passes over these packages
    /// only (`-p a -p b`): the per-feature clippy of the crates a change
    /// touches, run locally before a pull request; `--slice k/N` takes a share
    /// of them.
    #[arg(long, allow_hyphen_values = true)]
    packages: Option<String>,
    #[command(flatten)]
    pub(crate) run: RunOpts,
}

/// `cargo xtask feature-matrix`: clippy every feature on its own (cargo-hack).
///
/// `facade-combos`, then the per-feature passes; `--slice` picks the part one
/// CI runner does, `--packages` narrows the passes to the packages a change
/// touches. Needs cargo-hack.
pub(crate) fn feature_matrix(args: &FeatureMatrixArgs) -> anyhow::Result<ExitCode> {
    match &args.packages {
        Some(packages) => done(
            args.run
                .runner()
                .every(&feature_scope_plan(packages, args.slice)?),
        ),
        None => done(feature_matrix_stage(args.run.runner(), args.slice)),
    }
}

/// The per-feature passes over `packages` alone (`-p a -p b`), or `slice`'s
/// share of them.
fn feature_scope_plan(packages: &str, slice: Slice) -> anyhow::Result<Vec<Step>> {
    let packages = match slice {
        Slice::All => packages.to_owned(),
        Slice::Shard { index, count } => format!("{packages} --partition {index}/{count}"),
        Slice::Combinations => {
            anyhow::bail!("--packages narrows the per-feature passes, not the facade combinations")
        }
    };
    Ok(hack_passes(&packages).map(Step::from).into())
}

/// Arguments for `cargo xtask facade-combos`.
#[derive(Debug, clap::Args)]
pub(crate) struct FacadeCombosArgs {
    #[command(flatten)]
    pub(crate) run: RunOpts,
}

/// `cargo xtask facade-combos`: build each supported facade feature combination on its own.
pub(crate) fn facade_combos(args: &FacadeCombosArgs) -> anyhow::Result<ExitCode> {
    done(facade::run(args.run.runner()))
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

/// Arguments for `cargo xtask wasm-check`.
#[derive(Debug, clap::Args)]
pub(crate) struct WasmCheckArgs {
    #[command(flatten)]
    pub(crate) run: RunOpts,
}

/// `cargo xtask wasm-check`: check and clippy the workspace for wasm32.
pub(crate) fn wasm_check(args: &WasmCheckArgs) -> anyhow::Result<ExitCode> {
    done(web::check(args.run.runner()))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(steps: &[Step]) -> Vec<String> {
        steps.iter().map(ToString::to_string).collect()
    }

    fn feature_matrix_slices_parse() {
        assert_eq!(Slice::parse("all"), Ok(Slice::All));
        assert_eq!(Slice::parse("combinations"), Ok(Slice::Combinations));
        assert_eq!(Slice::parse("3/3"), Ok(Slice::Shard { index: 3, count: 3 }));
        for bad in ["0/3", "4/3", "1/0", "3", "a/b", "1/3/2"] {
            assert!(Slice::parse(bad).is_err(), "{bad} parsed");
        }
    }

    /// `--packages` runs the two per-feature passes over the packages given,
    /// no combination and no shard of the workspace.
    fn feature_scope_lints_each_feature_of_the_given_packages() {
        let packages = "-p flui-runtime -p flui-app";
        let pass = |rest: &str| {
            format!("$ cargo hack clippy {packages}{rest} --locked --each-feature --keep-going")
        };
        let both = |rest: &str| {
            [
                format!("{} -- -D warnings", pass(rest)),
                format!("{} --tests --benches --examples -- -D warnings", pass(rest)),
            ]
        };
        let plan = |slice| lines(&feature_scope_plan(packages, slice).expect("a pass slice"));
        assert_eq!(plan(Slice::All), both(""));
        // a runner's share of the same commands, as main's shards split the workspace
        assert_eq!(
            plan(Slice::Shard { index: 2, count: 3 }),
            both(" --partition 2/3")
        );
        assert!(feature_scope_plan(packages, Slice::Combinations).is_err());
        // the invocation a contributor writes: a value that starts
        // with `-p`, beside a slice
        #[derive(clap::Parser)]
        struct FeatureMatrixCli {
            #[command(flatten)]
            args: FeatureMatrixArgs,
        }
        use clap::Parser as _;
        let parsed = FeatureMatrixCli::try_parse_from([
            "feature-matrix",
            "--packages",
            packages,
            "--slice",
            "2/3",
        ])
        .expect("the job's arguments parse");
        assert_eq!(parsed.args.packages.as_deref(), Some(packages));
        assert_eq!(parsed.args.slice, Slice::Shard { index: 2, count: 3 });
    }

    #[test]
    fn task_plans_contract() {
        crate::table_test::run_table(
            "task_plans_contract",
            &[
                (
                    "feature_matrix_slices_parse",
                    feature_matrix_slices_parse as fn(),
                ),
                (
                    "feature_scope_lints_each_feature_of_the_given_packages",
                    feature_scope_lints_each_feature_of_the_given_packages as fn(),
                ),
            ],
        );
    }
}
