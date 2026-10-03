//! Which packages a change touches, and the guards that keep that classification honest.
//!
//! `cargo xtask affected` is the ONE computation behind CI's lanes (the `plan`
//! job) and `cargo xtask check-changed`, so the two cannot disagree:
//!
//! ```text
//! cargo xtask affected                         # vs origin/main, human summary
//! cargo xtask affected --worktree              # also uncommitted + untracked files
//! cargo xtask affected --event pull_request --base <sha> --format github >> "$GITHUB_OUTPUT"
//! eval "$(cargo xtask affected --worktree --format shell)"
//! ```
//!
//! [`classify`] decides the mode and the packages, [`lane_args`] the lane and
//! the cargo arguments; `paths-filter` guards the docs-only allowlist;
//! [`aggregator`] is the `ci` job's skip rule.

mod aggregator;
mod classify;
mod guards;
mod lane_args;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::process::ExitCode;

use anyhow::Context;

use crate::util::repo_root;
use classify::{Repo, Scope};
use lane_args::{Event, LaneArgs};

/// How `affected` prints its answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Format {
    /// A one-line summary for people.
    Human,
    /// `KEY='value'` lines for a shell's `eval`.
    Shell,
    /// `key=value` lines for `$GITHUB_OUTPUT` (CI's `plan` job).
    Github,
}

/// Arguments for `cargo xtask affected`.
#[derive(Debug, clap::Args)]
pub(crate) struct AffectedArgs {
    /// Diff against the merge base with this revision.
    #[arg(long, default_value = "origin/main")]
    base: String,
    /// Include uncommitted and untracked files.
    #[arg(long)]
    worktree: bool,
    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Human)]
    format: Format,
    /// Classify these paths instead of a git diff.
    #[arg(long, num_args = 0..)]
    files: Option<Vec<String>>,
    /// The whole workspace, no diff.
    #[arg(long)]
    full: bool,
    /// The GitHub event (`github.event_name`); without it, a pull request.
    /// Every event but `pull_request` takes the whole workspace, no diff.
    #[arg(long, value_enum)]
    event: Option<Event>,
}

/// `cargo xtask affected`: print the lane and the packages a change touches (CI's `plan`).
pub(crate) fn affected(args: &AffectedArgs) -> anyhow::Result<ExitCode> {
    let repo = Repo::open(repo_root());
    let event = args.event.unwrap_or(Event::PullRequest);
    let scope = if args.full || event != Event::PullRequest {
        Scope::whole_workspace()
    } else {
        let files = match &args.files {
            Some(files) => files.clone(),
            None => repo.changed_files(&args.base, args.worktree)?,
        };
        classify::classify(&repo, &files)?
    };
    let values = lane_args::lane_args(&repo, &scope, event)?;
    print!("{}", render(&values, scope.packages.len(), args.format));
    Ok(ExitCode::SUCCESS)
}

/// The scoped check for this checkout: its change against `base`, uncommitted
/// and untracked files included, as the `(key, value)` pairs `affected
/// --worktree --format shell` prints for a pull request (`cargo xtask
/// check-changed` reads them in-process).
pub(crate) fn worktree_lane(base: &str) -> anyhow::Result<Vec<(&'static str, String)>> {
    let repo = Repo::open(repo_root());
    let scope = classify::classify(&repo, &repo.changed_files(base, true)?)?;
    Ok(lane_args::lane_args(&repo, &scope, Event::PullRequest)?
        .fields()
        .into())
}

/// The packages that do not build for wasm32 (`[package.metadata.flui]
/// wasm = false`), which `check-changed`'s wasm step excludes too.
pub(crate) fn no_wasm_packages() -> anyhow::Result<BTreeSet<String>> {
    let repo = Repo::open(repo_root());
    Ok(lane_args::no_wasm_packages(repo.workspace()?))
}

/// POSIX shell quoting, byte-for-byte as Python's `shlex.quote`: a value of
/// only safe characters stays bare, anything else is single-quoted.
fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_owned();
    }
    let safe = |c: char| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c);
    if value.chars().all(safe) {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\'', r#"'"'"'"#))
}

/// The `affected` output for `values`; `package_count` is the scope's size.
fn render(values: &LaneArgs, package_count: usize, format: Format) -> String {
    let mut out = String::new();
    match format {
        Format::Github => {
            // GITHUB_OUTPUT: one line per key; values carry no newlines
            for (key, value) in values.fields() {
                let _ = writeln!(out, "{key}={}", value.replace('\n', " "));
            }
        }
        Format::Shell => {
            // quoted: `reason` quotes file names, which are untrusted
            for (key, value) in values.fields() {
                let _ = writeln!(out, "{}={}", key.to_uppercase(), shell_quote(&value));
            }
        }
        Format::Human => {
            let heavy = if values.heavy_required {
                "  [wide lane required]"
            } else {
                ""
            };
            let _ = writeln!(
                out,
                "lane: {}, mode: {}{heavy}  ({})",
                values.lane.as_str(),
                values.mode,
                values.reason
            );
            if !values.packages.is_empty() {
                let _ = writeln!(out, "packages ({package_count}): {}", values.packages);
            }
        }
    }
    out
}

/// Arguments for `cargo xtask paths-filter`.
#[derive(Debug, clap::Args)]
pub(crate) struct PathsFilterArgs {}

/// `cargo xtask paths-filter`: check that no include_str! target is classified as docs-only.
///
/// Re-derives, from source, every path an `include_str!(...)` in the workspace
/// resolves to, and fails if any falls inside the docs-only allowlist
/// (`DOCS_ONLY`, the patterns `affected` itself uses, so the two cannot drift
/// without this failing first). No compilation.
pub(crate) fn paths_filter(_args: &PathsFilterArgs) -> anyhow::Result<ExitCode> {
    let offenders = guards::docs_only_include_targets(&repo_root())?;
    if offenders.is_empty() {
        println!("paths-filter: docs-only allowlist matches no compiled include_str!() target");
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!(
        "paths-filter: the following include_str!() targets fall inside the docs-only allowlist \
         (tools/xtask/src/change_scope/classify.rs DOCS_ONLY), so a PR touching only them would \
         wrongly skip every compiling job:"
    );
    for o in &offenders {
        eprintln!("  {} includes {}", o.rs_file, o.target);
    }
    eprintln!(
        "Fix: narrow DOCS_ONLY in tools/xtask/src/change_scope/classify.rs to exclude these paths."
    );
    Ok(ExitCode::FAILURE)
}

/// Arguments for `cargo xtask ci-verify`, the `ci` aggregator job's check.
///
/// Its inputs come from the environment the job sets: `NEEDS` (the
/// `toJSON(needs)` of every gated job), `LANE`, `STANDALONE`,
/// `EVENT`, and the lane lists `HEAVY_JOBS` (the `wide` lane's jobs),
/// `FULL_JOBS` and `EXTENDED_JOBS`.
#[derive(Debug, clap::Args)]
pub(crate) struct CiVerifyArgs {}

/// `cargo xtask ci-verify`: every ci.yml job is gated by the `ci` aggregator,
/// and exactly the jobs `plan`'s lane skips skipped (see [`aggregator`]).
pub(crate) fn ci_verify(_args: &CiVerifyArgs) -> anyhow::Result<ExitCode> {
    #[derive(serde::Deserialize)]
    struct Need {
        result: String,
    }
    let env =
        |name: &str| std::env::var(name).with_context(|| format!("environment variable {name}"));
    let needs: BTreeMap<String, Need> =
        serde_json::from_str(&env("NEEDS")?).context("parsing NEEDS")?;
    let needs: BTreeMap<String, String> =
        needs.into_iter().map(|(job, n)| (job, n.result)).collect();
    let list = |name: &str| -> anyhow::Result<BTreeSet<String>> {
        Ok(env(name)?.split_whitespace().map(str::to_owned).collect())
    };
    let lanes = aggregator::LaneJobs {
        wide: list("HEAVY_JOBS")?,
        full: list("FULL_JOBS")?,
        extended: list("EXTENDED_JOBS")?,
    };
    let (lane, event) = (env("LANE")?, env("EVENT")?);
    let plan = aggregator::Plan {
        result: needs.get("plan").map(String::as_str),
        lane: &lane,
        standalone: std::env::var("STANDALONE").is_ok_and(|v| !v.trim().is_empty()),
    };
    let ci_yml = classify::read_normalised(&repo_root().join(".github/workflows/ci.yml"))
        .context("reading .github/workflows/ci.yml")?;
    let declared = aggregator::gated_jobs(&aggregator::parse_jobs(&ci_yml)?);
    let (ok, log) = aggregator::verify(&declared, &needs, &lanes, &plan, &event);
    print!("{log}");
    Ok(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> LaneArgs {
        LaneArgs {
            lane: lane_args::Lane::Wide,
            mode: "packages".to_owned(),
            heavy_required: false,
            reason: "changed: flui-material; plus 2 dependents".to_owned(),
            packages: "flui flui-material flui-web-counter".to_owned(),
            pkg_args: "-p flui -p flui-material -p flui-web-counter".to_owned(),
            test_args: "-p flui -p flui-material -p flui-web-counter".to_owned(),
            features: "--features flui/cupertino".to_owned(),
            platform: false,
            cross_platform: false,
            cross_app: true,
            cross_cli: false,
            cross_desktop_mcp: false,
            cross_ios: true,
            wasm_args: "-p flui -p flui-material -p flui-web-counter".to_owned(),
            wasm_facade: true,
            hack_args: String::new(),
            doc_args: "-p flui -p flui-material -p flui-web-counter --features flui/testing"
                .to_owned(),
            doctest_args: "-p flui -p flui-material".to_owned(),
            standalone: String::new(),
        }
    }

    fn github_format_is_one_line_per_key() {
        let out = render(&sample(), 3, Format::Github);
        let expected = "lane=wide\nmode=packages\nheavy_required=false\nreason=changed: flui-material; plus 2 dependents\n\
             packages=flui flui-material flui-web-counter\npkg_args=-p flui -p flui-material -p flui-web-counter\n\
             test_args=-p flui -p flui-material -p flui-web-counter\nfeatures=--features flui/cupertino\n\
             platform=false\ncross_platform=false\ncross_app=true\ncross_cli=false\ncross_desktop_mcp=false\ncross_ios=true\n\
             wasm_args=-p flui -p flui-material -p flui-web-counter\nwasm_facade=true\nhack_args=\n\
             doc_args=-p flui -p flui-material -p flui-web-counter --features flui/testing\n\
             doctest_args=-p flui -p flui-material\nstandalone=\n";
        assert_eq!(out, expected);
        let mut multi = sample();
        multi.reason = "a\nb".to_owned();
        assert!(render(&multi, 3, Format::Github).contains("reason=a b\n"));
    }

    fn shell_format_quotes_like_shlex() {
        let out = render(&sample(), 3, Format::Shell);
        assert!(
            out.starts_with("LANE=wide\nMODE=packages\nHEAVY_REQUIRED=false\nREASON='changed: flui-material; plus 2 dependents'\n"),
            "{out}"
        );
        assert!(out.ends_with("\nSTANDALONE=''\n"), "{out}");
        assert!(out.contains("\nHACK_ARGS=''\n"), "{out}");
        assert!(
            out.contains("\nFEATURES='--features flui/cupertino'\n"),
            "{out}"
        );
        assert_eq!(shell_quote("--workspace"), "--workspace");
        assert_eq!(
            shell_quote("a@b%c+d=e:f,g.h/i-j_k"),
            "a@b%c+d=e:f,g.h/i-j_k"
        );
        assert_eq!(
            shell_quote("x';touch${IFS}PWNED;#"),
            r#"'x'"'"';touch${IFS}PWNED;#'"#
        );
        assert_eq!(shell_quote("é"), "'é'");
    }

    #[test]
    fn render_contract() {
        crate::table_test::run_table(
            "render_contract",
            &[
                (
                    "github_format_is_one_line_per_key",
                    github_format_is_one_line_per_key as fn(),
                ),
                (
                    "shell_format_quotes_like_shlex",
                    shell_format_quotes_like_shlex as fn(),
                ),
            ],
        );
    }

    /// A shell `eval`s the shell output; an unowned file's name
    /// lands in `reason`, so a hostile file name must stay data.
    #[cfg(unix)]
    #[test]
    fn shell_format_is_safe_to_eval() {
        let payload = "x';touch${IFS}PWNED;#";
        let repo = classify::tests::repo();
        let scope = classify::classify(repo, &[payload.to_owned()]).expect("classify");
        let out = render(
            &lane_args::lane_args(repo, &scope, Event::PullRequest).expect("args"),
            scope.packages.len(),
            Format::Shell,
        );
        let dir = std::env::temp_dir().join(format!("xtask-eval-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let run = std::process::Command::new("bash")
            .args(["-c", r#"eval "$1"; printf %s "$REASON""#, "_", &out])
            .current_dir(&dir)
            .output()
            .expect("bash");
        let pwned = dir.join("PWNED").exists();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(run.status.success());
        assert!(!pwned);
        assert_eq!(String::from_utf8_lossy(&run.stdout), scope.reason);
    }
}
