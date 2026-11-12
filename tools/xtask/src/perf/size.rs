//! `cargo xtask perf --size`: what an application built on FLUI costs before
//! it does anything, compared with the checked-in baseline.
//!
//! Three kinds of counter, each one cost that has to stay at or under its
//! baseline:
//!
//! - `crates_<target>`: the packages `cargo tree -p flui` resolves for the
//!   facade with no feature on that target (normal edges, each package once).
//!   It is a function of `Cargo.lock` and the manifests, so it is compared
//!   exactly: a new package in the default graph fails until `--bless`
//!   records it as meant.
//! - `counter_release_bytes`: the release binary of a `flui create --template
//!   counter` project on the host, built with the release profile the
//!   template writes, as `flui build` ships it.
//! - `counter_wasm_bytes`: the same project built for
//!   `wasm32-unknown-unknown`, before `wasm-opt`.
//! - `hot_reload_host_release_bytes`: the host of a `flui create --hot-reload`
//!   project built the same way: its default build links the logic crate and
//!   installs no reload hook, so the reload machinery stays out of the binary.
//!
//! A binary's size moves with every change to the code it links, so the
//! byte counters fail only past [`BYTES_TOLERANCE_PERCENT`] over the baseline;
//! growth inside it prints and passes, and `--bless` on `main` moves the
//! baseline. A count or size that fell prints as an improvement and passes.
//!
//! The baseline is [`BASELINE`]; the host binary's size depends on the host,
//! so the baseline is Linux x86_64's, which is where CI runs this.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use anyhow::{Context, bail};
use cargo_metadata::Message;
use cargo_metadata::camino::Utf8PathBuf;

use super::{Counts, Finding, Mode};

/// The size baseline: one table, `[size]`.
const BASELINE: &str = "perf/size.toml";

/// The one scenario the baseline holds.
const SCENARIO: &str = "size";

/// The name of the `flui create --template counter` project that is measured.
const COUNTER_PROJECT: &str = "perf-counter";

/// The name of the `flui create --hot-reload` project whose host is measured.
const HOT_RELOAD_PROJECT: &str = "perf-reload";

/// The checkout's target subdirectory every generated project builds into.
const PROJECTS_TARGET_DIR: &str = "perf-size-projects";

/// How far a byte counter may exceed its baseline before it fails, in percent.
pub(super) const BYTES_TOLERANCE_PERCENT: u64 = 2;

/// The targets whose default graph is counted, by the name the counter uses.
const GRAPH_TARGETS: [(&str, &str); 5] = [
    ("linux", "x86_64-unknown-linux-gnu"),
    ("windows", "x86_64-pc-windows-msvc"),
    ("macos", "aarch64-apple-darwin"),
    ("android", "aarch64-linux-android"),
    ("wasm32", "wasm32-unknown-unknown"),
];

const WASM_TARGET: &str = "wasm32-unknown-unknown";

/// The first of the changed `files` (repo-relative, `/`-separated) that can
/// move a counter past what a code change does: a manifest or the lock (the
/// graph, the features), the toolchain or cargo's configuration, the templates
/// `flui create` writes (the measured projects and their release profile), the
/// baseline, or this check. A pull request whose diff holds one runs the
/// `size` job (ADR-0303 §9); `main` runs it on every push, which catches the
/// slow drift of code changes.
pub(crate) fn changed_input(files: &[String]) -> Option<&str> {
    files.iter().map(String::as_str).find(|f| {
        *f == "Cargo.lock"
            || *f == "Cargo.toml"
            || f.ends_with("/Cargo.toml")
            || *f == "rust-toolchain.toml"
            || f.starts_with(".cargo/")
            || *f == BASELINE
            || *f == "tools/xtask/src/perf/size.rs"
            || f.starts_with("crates/flui-cli/src/templates/")
    })
}

const BASELINE_HEADER: &str = "# Written by `cargo xtask perf --size --bless`; Linux x86_64.\n\
# Package counts of the facade's default graph and the release sizes of a\n\
# `flui create` counter (host and wasm32) and of a `flui create --hot-reload` host.\n";

/// Which counters a `--size` run measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Measure {
    /// The package counts and both binaries.
    All,
    /// The package counts alone: no build, so a pull request can afford it.
    Graph,
}

/// `cargo xtask perf --size [--graph] [--check | --bless]`.
pub(super) fn run(root: &Path, mode: Mode, measure: Measure) -> anyhow::Result<ExitCode> {
    let mut counters = BTreeMap::new();
    for (name, triple) in GRAPH_TARGETS {
        counters.insert(format!("crates_{name}"), graph_packages(root, triple)?);
    }
    if measure == Measure::All {
        let counter = GeneratedProject::counter(root, COUNTER_PROJECT, false)?;
        counters.insert(
            "counter_release_bytes".to_owned(),
            counter.release_size(root, COUNTER_PROJECT, None)?,
        );
        counters.insert(
            "counter_wasm_bytes".to_owned(),
            counter.release_size(root, COUNTER_PROJECT, Some(WASM_TARGET))?,
        );
        let reload = GeneratedProject::counter(root, HOT_RELOAD_PROJECT, true)?;
        let host = format!("{HOT_RELOAD_PROJECT}-host");
        counters.insert(
            "hot_reload_host_release_bytes".to_owned(),
            reload.release_size(root, &host, None)?,
        );
    }
    let current: Counts = [(SCENARIO.to_owned(), counters)].into_iter().collect();

    let path = root.join(BASELINE);
    if mode == Mode::Bless {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        std::fs::write(&path, render(&current)?)
            .with_context(|| format!("writing {}", path.display()))?;
        println!("perf --size: blessed {BASELINE}");
        return Ok(ExitCode::SUCCESS);
    }
    let mut baseline = load(&path)?;
    if measure == Measure::Graph {
        // The byte counters are not measured; leaving them in would report
        // each one missing.
        for counters in baseline.values_mut() {
            counters.retain(|counter, _| !is_bytes(counter));
        }
    }
    super::print_table(&baseline, &current);
    let findings = compare(&baseline, &current);
    for finding in &findings {
        println!("{finding}");
    }
    let failing = findings.iter().filter(|finding| finding.fails()).count();
    Ok(match mode {
        Mode::Check if failing > 0 => {
            println!("perf --size: {failing} failing findings against {BASELINE}");
            ExitCode::FAILURE
        }
        Mode::Check => {
            println!("perf --size: holds {BASELINE}");
            ExitCode::SUCCESS
        }
        Mode::Advisory | Mode::Bless => {
            println!("advisory: {failing} failing findings");
            ExitCode::SUCCESS
        }
    })
}

/// The distinct packages of the facade's default graph on `triple`.
fn graph_packages(root: &Path, triple: &str) -> anyhow::Result<u64> {
    let output = cargo(root)
        .args([
            "tree",
            "--locked",
            "--quiet",
            "--edges",
            "normal",
            "--prefix",
            "none",
            "--package",
            "flui",
            "--target",
            triple,
        ])
        .output()
        .context("running cargo tree")?;
    if !output.status.success() {
        bail!(
            "cargo tree --target {triple} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let count = distinct_packages(&String::from_utf8_lossy(&output.stdout));
    u64::try_from(count).context("a package count fits u64")
}

/// The number of distinct `name version` pairs in `cargo tree --prefix none`
/// output, where a repeated package ends in ` (*)`.
fn distinct_packages(tree: &str) -> usize {
    let mut packages: Vec<(&str, &str)> = tree
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            Some((words.next()?, words.next()?))
        })
        .collect();
    packages.sort_unstable();
    packages.dedup();
    packages.len()
}

/// A project `flui create --local` generated from this checkout, in a
/// temporary directory outside it, so its own profiles apply and the
/// checkout's `.cargo/` does not. Its lock starts from the checkout's, so it
/// resolves the versions the workspace does.
struct GeneratedProject {
    /// Removed on drop.
    _parent: tempfile::TempDir,
    dir: PathBuf,
}

impl GeneratedProject {
    /// `flui create <name> --template counter`, with `--hot-reload` when
    /// `hot_reload`.
    fn counter(root: &Path, name: &str, hot_reload: bool) -> anyhow::Result<Self> {
        let parent = tempfile::tempdir().context("creating the generated project's directory")?;
        let status = cargo(root)
            .args([
                "run",
                "--locked",
                "--quiet",
                "--package",
                "flui-cli",
                "--bin",
                "flui",
                "--",
                "create",
                name,
                "--template",
                "counter",
                "--org",
                "com.perf",
                "--local",
                "--no-check",
            ])
            .args(hot_reload.then_some("--hot-reload"))
            .arg("--path")
            .arg(parent.path())
            .status()
            .with_context(|| format!("running flui create {name}"))?;
        if !status.success() {
            bail!("flui create {name} failed ({status})");
        }
        let dir = parent.path().join(name);
        std::fs::copy(root.join("Cargo.lock"), dir.join("Cargo.lock"))
            .with_context(|| format!("seeding {name}'s lock"))?;
        // Records the project's own members in the copied lock and moves no
        // registry version, so the builds below can be `--locked`.
        let status = cargo(&dir)
            .args(["update", "--workspace", "--offline", "--quiet"])
            .status()
            .with_context(|| format!("adding {name}'s members to its lock"))?;
        if !status.success() {
            bail!("cargo update --workspace in {name} failed ({status})");
        }
        Ok(Self {
            _parent: parent,
            dir,
        })
    }

    /// Builds the binary `package` in release with the project's own profile,
    /// for `target` (the host when `None`), and returns its size in bytes.
    ///
    /// Every generated project builds into one target directory of the
    /// checkout: they share the profile and the dependency graph, so the
    /// second build reuses the first one's dependencies.
    fn release_size(
        &self,
        root: &Path,
        package: &str,
        target: Option<&str>,
    ) -> anyhow::Result<u64> {
        let mut build = cargo(&self.dir);
        build
            .args(["build", "--locked", "--release", "--package", package])
            .args(["--bin", package])
            .arg("--target-dir")
            .arg(root.join("target").join(PROJECTS_TARGET_DIR))
            .args(["--message-format", "json-render-diagnostics"]);
        if let Some(target) = target {
            build.args(["--target", target]);
        }
        artifact_size(build, package, cargo_metadata::Target::is_bin)
    }
}

/// Runs `build` and returns the size of the artifact Cargo reports for the
/// target named `name` of the kind `is_kind` accepts.
///
/// The artifact's path is the one Cargo reports for this build, not one
/// derived from the target directory: a `CARGO_BUILD_TARGET` or `build.target`
/// setting moves the output under `target/<triple>/`, and a guessed path would
/// measure a stale binary from an earlier build.
fn artifact_size(
    mut build: Command,
    name: &str,
    is_kind: fn(&cargo_metadata::Target) -> bool,
) -> anyhow::Result<u64> {
    build.stdout(Stdio::piped()).stderr(Stdio::inherit());
    let mut child = build.spawn().with_context(|| format!("building {name}"))?;
    let stdout = child
        .stdout
        .take()
        .expect("BUG: the build's stdout is piped");
    let mut artifact: Option<PathBuf> = None;
    for message in Message::parse_stream(BufReader::new(stdout)) {
        let message = message.context("reading the build's messages")?;
        if let Message::CompilerArtifact(built) = message
            && built.target.name == name
            && is_kind(&built.target)
        {
            // Where Cargo reports no `executable` for the target, its module is
            // then the `.wasm` among its files.
            artifact = built
                .executable
                .or_else(|| {
                    built
                        .filenames
                        .into_iter()
                        .find(|file| file.extension() == Some("wasm"))
                })
                .map(Utf8PathBuf::into_std_path_buf);
        }
    }
    let status = child.wait().with_context(|| format!("building {name}"))?;
    if !status.success() {
        bail!("building {name} failed ({status})");
    }
    let artifact =
        artifact.with_context(|| format!("the build reported no executable for {name}"))?;
    let metadata =
        std::fs::metadata(&artifact).with_context(|| format!("reading {}", artifact.display()))?;
    Ok(metadata.len())
}

fn cargo(root: &Path) -> Command {
    let mut command =
        Command::new(std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo")));
    command.current_dir(root);
    command
}

fn load(path: &Path) -> anyhow::Result<Counts> {
    if !path.exists() {
        println!("perf --size: no baseline at {BASELINE}; run `cargo xtask perf --size --bless`");
        return Ok(Counts::new());
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let counts = super::parse(&text).with_context(|| format!("parsing {}", path.display()))?;
    if counts.len() != 1 || !counts.contains_key(SCENARIO) {
        bail!(
            "{} must hold exactly the table [{SCENARIO}]",
            path.display()
        );
    }
    Ok(counts)
}

fn render(counts: &Counts) -> anyhow::Result<String> {
    Ok(format!("{BASELINE_HEADER}\n{}", toml::to_string(counts)?))
}

/// Whether `counter` is a binary size, which may grow within the tolerance.
fn is_bytes(counter: &str) -> bool {
    counter.ends_with("_bytes")
}

/// Whether `current` exceeds `base` by more than the bytes tolerance.
/// Widened before multiplying, so no size overflows the comparison.
pub(super) fn past_tolerance(base: u64, current: u64) -> bool {
    u128::from(current) * 100 > u128::from(base) * u128::from(100 + BYTES_TOLERANCE_PERCENT)
}

/// Every difference that matters between `baseline` and `current`: a count
/// that rose, a size past the tolerance, a fall (which passes), and a counter
/// or scenario on one side only.
fn compare(baseline: &Counts, current: &Counts) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (scenario, base_counters) in baseline {
        let Some(current_counters) = current.get(scenario) else {
            findings.push(Finding::MissingScenario {
                scenario: scenario.clone(),
            });
            continue;
        };
        for (counter, &base) in base_counters {
            let Some(&value) = current_counters.get(counter) else {
                findings.push(Finding::MissingCounter {
                    scenario: scenario.clone(),
                    counter: counter.clone(),
                });
                continue;
            };
            let regressed = if is_bytes(counter) {
                past_tolerance(base, value)
            } else {
                value > base
            };
            if regressed {
                findings.push(Finding::Regressed {
                    scenario: scenario.clone(),
                    counter: counter.clone(),
                    base,
                    current: value,
                });
            } else if value < base {
                findings.push(Finding::Improved {
                    scenario: scenario.clone(),
                    counter: counter.clone(),
                    base,
                    current: value,
                });
            }
        }
        for counter in current_counters.keys() {
            if !base_counters.contains_key(counter) {
                findings.push(Finding::NewCounter {
                    scenario: scenario.clone(),
                    counter: counter.clone(),
                });
            }
        }
    }
    for scenario in current.keys() {
        if !baseline.contains_key(scenario) {
            findings.push(Finding::NewScenario {
                scenario: scenario.clone(),
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(pairs: &[(&str, u64)]) -> Counts {
        [(
            SCENARIO.to_owned(),
            pairs
                .iter()
                .map(|(name, value)| ((*name).to_owned(), *value))
                .collect(),
        )]
        .into_iter()
        .collect()
    }

    #[test]
    fn size_comparison_table() {
        let base = size(&[("crates_linux", 269), ("counter_release_bytes", 1000)]);
        for (case, current, expected) in [
            (
                "equal holds",
                size(&[("crates_linux", 269), ("counter_release_bytes", 1000)]),
                vec![],
            ),
            (
                "one more package fails",
                size(&[("crates_linux", 270), ("counter_release_bytes", 1000)]),
                vec![Finding::Regressed {
                    scenario: SCENARIO.to_owned(),
                    counter: "crates_linux".to_owned(),
                    base: 269,
                    current: 270,
                }],
            ),
            (
                "bytes within the tolerance hold",
                size(&[("crates_linux", 269), ("counter_release_bytes", 1020)]),
                vec![],
            ),
            (
                "bytes past the tolerance fail",
                size(&[("crates_linux", 269), ("counter_release_bytes", 1021)]),
                vec![Finding::Regressed {
                    scenario: SCENARIO.to_owned(),
                    counter: "counter_release_bytes".to_owned(),
                    base: 1000,
                    current: 1021,
                }],
            ),
            (
                "a fall is an improvement",
                size(&[("crates_linux", 250), ("counter_release_bytes", 1000)]),
                vec![Finding::Improved {
                    scenario: SCENARIO.to_owned(),
                    counter: "crates_linux".to_owned(),
                    base: 269,
                    current: 250,
                }],
            ),
            (
                "a counter left out is missing",
                size(&[("crates_linux", 269)]),
                vec![Finding::MissingCounter {
                    scenario: SCENARIO.to_owned(),
                    counter: "counter_release_bytes".to_owned(),
                }],
            ),
        ] {
            assert_eq!(compare(&base, &current), expected, "{case}");
        }
        assert!(!past_tolerance(u64::MAX, u64::MAX));
    }

    #[test]
    fn size_inputs_table() {
        let cases: [(&[&str], Option<&str>); 10] = [
            (&["Cargo.lock"], Some("Cargo.lock")),
            (&["Cargo.toml"], Some("Cargo.toml")),
            (
                &["src/lib.rs", "crates/flui-app/Cargo.toml"],
                Some("crates/flui-app/Cargo.toml"),
            ),
            (&["rust-toolchain.toml"], Some("rust-toolchain.toml")),
            (&[".cargo/config.toml"], Some(".cargo/config.toml")),
            (&["perf/size.toml"], Some("perf/size.toml")),
            (&["examples/counter.rs"], None),
            (
                &["crates/flui-cli/src/templates/hot_reload.rs"],
                Some("crates/flui-cli/src/templates/hot_reload.rs"),
            ),
            (&["docs/Cargo.toml.md"], None),
            (&["crates/flui-view/src/lib.rs"], None),
        ];
        for (files, expected) in cases {
            let files: Vec<String> = files.iter().map(|f| (*f).to_owned()).collect();
            assert_eq!(changed_input(&files), expected, "{files:?}");
        }
    }

    #[test]
    fn repeated_packages_count_once() {
        let tree = "flui v0.2.0-dev (/x)\nflui-app v0.2.0-dev (/x/a)\nzbus v5.19.0\nzbus v6.1.0\n\
                    flui-app v0.2.0-dev (/x/a) (*)\n";
        assert_eq!(distinct_packages(tree), 4);
    }

    #[test]
    fn the_checked_in_baseline_parses() {
        let path = crate::util::repo_root().join(BASELINE);
        let counts = load(&path).expect("the size baseline parses");
        let counters = &counts[SCENARIO];
        for (name, _) in GRAPH_TARGETS {
            assert!(counters.contains_key(&format!("crates_{name}")), "{name}");
        }
        assert!(counters.contains_key("counter_release_bytes"));
        assert!(counters.contains_key("counter_wasm_bytes"));
        assert_eq!(
            render(&counts)
                .ok()
                .and_then(|text| super::super::parse(&text).ok()),
            Some(counts)
        );
    }
}
