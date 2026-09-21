use crate::error::{CliError, CliResult};
use crate::proc;
use crate::runner::{CargoCommand, OutputStyle};
use crate::ui;
use console::style;
use serde_json::json;
use std::process::Command;
use std::time::Duration;

/// The crate this CLI ships as (not `flui_cli` — that package does not
/// exist on crates.io; the underscore/hyphen mixup once made `--self`
/// install nothing).
const CRATE_NAME: &str = "flui-cli";

/// How long `--check` waits for `cargo search` before giving up.
const SEARCH_TIMEOUT: Duration = Duration::from_secs(30);

/// How long `--check --dependencies` waits for `cargo update --dry-run`
/// before giving up. A dry run still has to resolve the whole dependency
/// graph against the registry, so it gets a much longer budget than the
/// single `cargo search` lookup used for `--self`.
const DEPENDENCIES_CHECK_TIMEOUT: Duration = Duration::from_mins(5);

/// Execute the upgrade command.
///
/// # Arguments
///
/// * `self_update` - Update only the `flui-cli` binary itself
/// * `dependencies` - Update only project dependencies
/// * `check` - Report what would change without installing anything
///
/// `check` honours `self_update`/`dependencies`: `--check --dependencies`
/// runs `cargo update --dry-run` (bounded to
/// [`DEPENDENCIES_CHECK_TIMEOUT`]) and reports what would change in the
/// project's lockfile; `--check` alone or `--check --self` asks crates.io
/// whether a newer `flui-cli` has been published. `--self` and
/// `--dependencies` are mutually exclusive (enforced by clap), so `check`
/// never has to reconcile both at once.
///
/// # Errors
///
/// Returns `CliError::UpgradeFailed` if the CLI's own upgrade fails.
/// Returns `CliError::UpdateFailed` if the dependency update fails.
pub fn execute(self_update: bool, dependencies: bool, check: bool) -> CliResult<()> {
    ui::intro(style(" flui upgrade ").on_blue().white())?;

    if check {
        return if dependencies {
            report_check_dependencies()
        } else {
            report_check()
        };
    }

    // clap enforces `self` and `dependencies` as mutually exclusive
    // (`conflicts_with`), so at most one of the two is ever true here.
    // With neither set, a bare `flui upgrade` used to force-install over the
    // running binary as a side effect of updating dependencies — a footgun.
    // Now it only touches dependencies, and says so.
    if !self_update && !dependencies {
        ui::info(format!(
            "Only updating dependencies. Run {} to update the flui CLI itself.",
            style("flui upgrade --self").cyan()
        ))?;
    }

    let result = if self_update {
        self_install()
    } else {
        update_dependencies()
    };

    match result {
        Ok(()) => {
            ui::outro(style("Upgrade complete").green())?;
            Ok(())
        }
        Err(err) => {
            ui::outro_cancel("Upgrade failed")?;
            Err(err)
        }
    }
}

/// `cargo install flui-cli --locked`, surfacing cargo's own diagnostics on
/// failure rather than swallowing them behind a generic error.
///
/// This runs captured rather than streamed: detecting the "not on
/// crates.io, this was a source install" case needs the text of cargo's
/// stderr, which a streamed (inherited) child would not give us. The
/// output still reaches the user — just after the (usually short) command
/// finishes, not line-by-line as it runs.
fn self_install() -> CliResult<()> {
    let spinner = ui::spinner();
    spinner.start(format!("Installing {CRATE_NAME}..."));

    let output = Command::new("cargo")
        .args(["install", CRATE_NAME, "--locked"])
        .output()
        .map_err(|error| CliError::context(error, "Failed to run cargo install"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    if output.status.success() {
        spinner.stop(format!("{} {CRATE_NAME} installed", style("✓").green()));
        if !stdout.trim().is_empty() {
            ui::info(stdout.trim())?;
        }
        return Ok(());
    }

    spinner.error(format!("Failed to install {CRATE_NAME}"));
    if !stderr.trim().is_empty() {
        ui::error(stderr.trim())?;
    }
    if stderr.contains("could not find") {
        ui::note(
            "Installed from source?",
            format!(
                "{CRATE_NAME} is not resolving on crates.io for this command. If this \
                 binary was built from a source checkout rather than published crate, \
                 update it with:\n  cargo install --path crates/flui-cli --locked"
            ),
        )?;
    }
    Err(CliError::UpgradeFailed)
}

/// `cargo update`.
fn update_dependencies() -> CliResult<()> {
    let spinner = ui::spinner();
    spinner.start("Updating project dependencies...");

    let _ = CargoCommand::update()
        .output_style(OutputStyle::Silent)
        .run()?;

    spinner.stop(format!("{} Dependencies updated", style("✓").green()));
    Ok(())
}

/// `--check`: ask crates.io what the latest published version is, without
/// installing anything.
fn report_check() -> CliResult<()> {
    let current = env!("CARGO_PKG_VERSION");
    let spinner = ui::spinner();
    spinner.start(format!("Checking crates.io for {CRATE_NAME}..."));

    let mut command = Command::new("cargo");
    command.args(["search", CRATE_NAME, "--limit", "1"]);

    let output = match proc::output_with_timeout(&mut command, SEARCH_TIMEOUT) {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            spinner.error("cargo search failed");
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            return Err(CliError::context(
                std::io::Error::other(stderr),
                "cargo search failed",
            ));
        }
        Err(error) => {
            spinner.error("cargo search failed");
            return Err(CliError::context(error, "Failed to run cargo search"));
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let latest = parse_search_version(&stdout, CRATE_NAME);

    let (message, update_available) = match &latest {
        Some(latest_version) => match is_newer(latest_version, current) {
            Some(true) => (format!("newer version {latest_version} available"), true),
            _ => ("up to date".to_string(), false),
        },
        None => ("not published yet".to_string(), false),
    };

    spinner.stop(message.clone());

    ui::emit(
        "upgrade.check",
        &json!({
            "current": current,
            "latest": latest,
            "update_available": update_available,
        }),
    );

    ui::outro(format!("{CRATE_NAME} {current}: {message}"))?;
    Ok(())
}

/// `--check --dependencies`: run `cargo update --dry-run` and report what
/// it would change, without touching `Cargo.lock`.
fn report_check_dependencies() -> CliResult<()> {
    let spinner = ui::spinner();
    spinner.start("Checking for dependency updates (cargo update --dry-run)...");

    let mut command = Command::new("cargo");
    command.args(["update", "--dry-run"]);

    let output = match proc::output_with_timeout(&mut command, DEPENDENCIES_CHECK_TIMEOUT) {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            spinner.error("cargo update --dry-run failed");
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            return Err(CliError::context(
                std::io::Error::other(stderr),
                "cargo update --dry-run failed",
            ));
        }
        Err(error) => {
            spinner.error("cargo update --dry-run failed");
            return Err(CliError::context(
                error,
                "Failed to run cargo update --dry-run",
            ));
        }
    };

    let report = String::from_utf8_lossy(&output.stderr).into_owned();
    let changes = planned_changes(&report);
    let update_available = !changes.is_empty();

    let message = if update_available {
        format!(
            "{} dependenc{} would change",
            changes.len(),
            if changes.len() == 1 { "y" } else { "ies" }
        )
    } else {
        "dependencies are up to date".to_string()
    };

    spinner.stop(message.clone());

    if update_available {
        ui::note("Dependencies that would change", changes.join("\n"))?;
    }

    ui::emit(
        "upgrade.check_dependencies",
        &json!({
            "update_available": update_available,
            "changes": changes,
        }),
    );

    ui::outro(message)?;
    Ok(())
}

/// Pull `<crate_name> = "<version>"` out of `cargo search`'s output.
fn parse_search_version(stdout: &str, crate_name: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        let rest = line.trim().strip_prefix(crate_name)?.trim_start();
        let rest = rest.strip_prefix('=')?.trim_start();
        let rest = rest.strip_prefix('"')?;
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    })
}

/// `(major, minor, patch)` plus an optional pre-release tag.
type ParsedVersion<'a> = ((u64, u64, u64), Option<&'a str>);

/// Minimal `major.minor.patch[-pre]` parse: no new dependency for something
/// used only to answer "is there a newer version". The pre-release tag is
/// kept as-is; [`compare_prerelease`] applies SemVer 2.0's real
/// dot-separated-identifier precedence to it rather than a literal string
/// comparison, so `beta.9` correctly precedes `beta.10`.
fn parse_version(version: &str) -> Option<ParsedVersion<'_>> {
    let version = version.trim();
    let (core, pre) = match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (version, None),
    };
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some(((major, minor, patch), pre))
}

/// Compare two SemVer 2.0 dot-separated pre-release identifiers (e.g.
/// `"beta"` and `"9"` in `beta.9`).
///
/// Numeric identifiers compare numerically; a numeric identifier always
/// has lower precedence than an alphanumeric one; alphanumeric identifiers
/// compare byte-wise (ASCII sort order, per the spec); and when one
/// identifier is a prefix of the other, the longer set of fields has
/// higher precedence.
fn compare_identifier(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a.parse::<u64>(), b.parse::<u64>()) {
        (Ok(a), Ok(b)) => a.cmp(&b),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => a.cmp(b),
    }
}

/// Compare two full pre-release strings (e.g. `"beta.9"` vs `"beta.10"`)
/// by SemVer 2.0 precedence: identifier-by-identifier via
/// [`compare_identifier`], shorter-is-lower when one is a prefix of the
/// other.
fn compare_prerelease(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let mut a_ids = a.split('.');
    let mut b_ids = b.split('.');
    loop {
        return match (a_ids.next(), b_ids.next()) {
            (Some(a_id), Some(b_id)) => match compare_identifier(a_id, b_id) {
                Ordering::Equal => continue,
                other => other,
            },
            (Some(_), None) => Ordering::Greater,
            (None, Some(_)) => Ordering::Less,
            (None, None) => Ordering::Equal,
        };
    }
}

/// `true` when `candidate` is a newer version than `current`; `None` when
/// either string does not parse as `major.minor.patch[-pre]`.
///
/// Follows SemVer 2.0 precedence throughout, including for the
/// pre-release tag: a version with a pre-release is lower than the same
/// `major.minor.patch` without one (`0.3.0-rc.1 < 0.3.0`), and two
/// pre-releases are compared identifier-by-identifier
/// (`0.3.0-beta.9 < 0.3.0-beta.10 < 0.3.0-rc.1`).
fn is_newer(candidate: &str, current: &str) -> Option<bool> {
    use std::cmp::Ordering;
    let (candidate_core, candidate_pre) = parse_version(candidate)?;
    let (current_core, current_pre) = parse_version(current)?;
    Some(
        match candidate_core.cmp(&current_core) {
            Ordering::Equal => match (candidate_pre, current_pre) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => compare_prerelease(a, b),
            },
            other => other,
        } == Ordering::Greater,
    )
}

/// The dependency changes `cargo update --dry-run` reports on stderr.
///
/// Cargo prints housekeeping lines with the same verbs — `Updating crates.io
/// index`, `Updating git repository …`, `Locking N packages` — before the
/// real plan, so the verb alone is not evidence of a change. A change line
/// names a package and a version: `Updating serde v1.0.1 -> v1.0.2`,
/// `Adding foo v0.1.0`, `Removing bar v2.0.0`.
fn planned_changes(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .map(str::trim)
        .filter(|line| {
            let mut words = line.split_whitespace();
            let verb = words.next().unwrap_or_default();
            let name = words.next();
            let version = words.next();
            let has_version = version.is_some_and(|v| v.starts_with('v'));
            match verb {
                "Updating" => name.is_some() && has_version && line.contains(" -> "),
                "Adding" | "Removing" => name.is_some() && has_version,
                _ => false,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_search_line() {
        let stdout = "flui-cli = \"0.3.0\"    # Command-line interface for FLUI\n... and 8 crates more (use --limit N to see more)";
        assert_eq!(
            parse_search_version(stdout, "flui-cli"),
            Some("0.3.0".to_string())
        );
    }

    #[test]
    fn missing_line_is_none() {
        assert_eq!(parse_search_version("", "flui-cli"), None);
    }

    #[test]
    fn newer_patch_is_newer() {
        assert_eq!(is_newer("0.3.1", "0.3.0"), Some(true));
    }

    #[test]
    fn same_version_is_not_newer() {
        assert_eq!(is_newer("0.3.0", "0.3.0"), Some(false));
    }

    #[test]
    fn older_version_is_not_newer() {
        assert_eq!(is_newer("0.2.9", "0.3.0"), Some(false));
    }

    #[test]
    fn release_is_newer_than_its_own_prerelease() {
        assert_eq!(is_newer("0.3.0", "0.3.0-beta.1"), Some(true));
        assert_eq!(is_newer("0.3.0-beta.1", "0.3.0"), Some(false));
    }

    #[test]
    fn prerelease_precedence_follows_semver_2_0() {
        // 0.3.0-beta.9 < 0.3.0-beta.10 < 0.3.0-rc.1 < 0.3.0
        assert_eq!(is_newer("0.3.0-beta.10", "0.3.0-beta.9"), Some(true));
        assert_eq!(is_newer("0.3.0-beta.9", "0.3.0-beta.10"), Some(false));
        assert_eq!(is_newer("0.3.0-rc.1", "0.3.0-beta.10"), Some(true));
        assert_eq!(is_newer("0.3.0-beta.10", "0.3.0-rc.1"), Some(false));
        assert_eq!(is_newer("0.3.0", "0.3.0-rc.1"), Some(true));
        assert_eq!(is_newer("0.3.0-rc.1", "0.3.0"), Some(false));
    }

    #[test]
    fn compare_prerelease_orders_numeric_identifiers_numerically() {
        assert_eq!(
            compare_prerelease("beta.9", "beta.10"),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_prerelease("beta.10", "beta.9"),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn compare_identifier_ranks_numeric_below_alphanumeric() {
        assert_eq!(compare_identifier("9", "alpha"), std::cmp::Ordering::Less);
        assert_eq!(
            compare_identifier("alpha", "9"),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn compare_prerelease_shorter_is_lower_on_equal_prefix() {
        assert_eq!(
            compare_prerelease("alpha", "alpha.1"),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn unparseable_version_yields_none() {
        assert_eq!(is_newer("not-a-version", "0.3.0"), None);
    }

    #[test]
    fn planned_changes_ignore_cargo_housekeeping_lines() {
        let stderr = "    Updating crates.io index\n    Updating git repository `https://x/y`\n     Locking 2 packages to latest compatible versions\n    Updating serde v1.0.1 -> v1.0.2\n      Adding foo v0.1.0\n    Removing bar v2.0.0\n";
        assert_eq!(
            super::planned_changes(stderr),
            vec![
                "Updating serde v1.0.1 -> v1.0.2",
                "Adding foo v0.1.0",
                "Removing bar v2.0.0"
            ]
        );
        assert!(super::planned_changes("    Updating crates.io index\n").is_empty());
    }
}
