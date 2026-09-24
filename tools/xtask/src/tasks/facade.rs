//! `cargo xtask facade-combos`: every supported feature combination of the
//! `flui` facade, compiled on its own, and hot reload's absence from an
//! ordinary production graph.

use anyhow::bail;

use super::deny_warnings;
use super::exec::{Cmd, Runner};

/// The facade's supported feature combinations; `""` is its defaults.
///
/// Each is its own clippy of the `flui` package alone: a `--workspace` build
/// proves nothing here, since feature unification would enable `material`
/// from a sibling and turn a broken combination green. `--all-targets` is
/// deliberate: a missing `required-features` on an example or test is exactly
/// the wiring these builds exist to catch.
const COMBOS: [&str; 15] = [
    "--no-default-features",
    "--no-default-features --features material",
    "--no-default-features --features cupertino",
    "--no-default-features --features material,cupertino",
    "--no-default-features --features localizations",
    "--no-default-features --features material,localizations",
    "--no-default-features --features cupertino,localizations",
    "--no-default-features --features material,cupertino,localizations",
    "--no-default-features --features hot-reload",
    "--no-default-features --features serde",
    "--no-default-features --features a11y",
    "--no-default-features --features signals",
    "--no-default-features --features material,signals",
    "--all-features",
    "",
];

/// A fact about a dependency graph, read from `cargo tree`.
#[derive(Debug, Clone, Copy)]
struct TreeFact {
    /// What is being established.
    what: &'static str,
    /// The `cargo tree` arguments.
    args: &'static [&'static str],
    /// The text the output must contain, or must not.
    needle: &'static str,
    present: bool,
    /// The failure, when the output says otherwise.
    failure: &'static str,
}

/// Hot reload must be absent from an ordinary production graph, not merely
/// unused by it; the feature must bring it in; and the first-party host, the
/// executable contract for `flui run`, must enable flui-app's feature (a
/// direct dependency on flui-hot-reload does not).
const TREE_FACTS: [TreeFact; 3] = [
    TreeFact {
        what: "flui-hot-reload must be absent from flui-app's default graph",
        args: &["tree", "-p", "flui-app", "--locked", "-e", "normal"],
        needle: "flui-hot-reload",
        present: false,
        failure: "flui-hot-reload is in flui-app's default normal dependency graph",
    },
    TreeFact {
        what: "the hot-reload feature must bring in flui-hot-reload",
        args: &[
            "tree",
            "-p",
            "flui-app",
            "--locked",
            "-e",
            "normal",
            "--features",
            "hot-reload",
        ],
        needle: "flui-hot-reload",
        present: true,
        failure: "the hot-reload feature did not bring in flui-hot-reload",
    },
    TreeFact {
        what: "hot-reload-counter-host must enable flui-app/hot-reload",
        args: &[
            "tree",
            "-p",
            "hot-reload-counter-host",
            "--locked",
            "-e",
            "features",
            "-i",
            "flui-app",
        ],
        needle: r#"flui-app feature "hot-reload""#,
        present: true,
        failure: "hot-reload-counter-host does not enable flui-app/hot-reload",
    },
];

/// One clippy per entry of [`COMBOS`].
fn clippy_plan() -> Vec<Cmd> {
    COMBOS
        .into_iter()
        .map(|combo| {
            deny_warnings(
                Cmd::cargo(["clippy", "-p", "flui", "--locked", "--all-targets"]).split(combo),
            )
        })
        .collect()
}

/// Whether `output` bears `fact` out.
fn holds(fact: &TreeFact, output: &str) -> bool {
    output.contains(fact.needle) == fact.present
}

/// `cargo xtask facade-combos`, stopping at the first failure.
pub(super) fn run(runner: Runner) -> anyhow::Result<()> {
    for cmd in clippy_plan() {
        runner.run(&cmd)?;
    }
    for fact in &TREE_FACTS {
        println!("facade-combos: {}", fact.what);
        let cmd = Cmd::cargo(fact.args);
        println!("$ {cmd}");
        if runner.dry_run {
            continue;
        }
        // a failed `cargo tree` establishes nothing either way
        if !holds(fact, &cmd.stdout()?) {
            bail!("{}", fact.failure);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_combination_is_its_own_flui_clippy() {
        let lines: Vec<String> = clippy_plan().iter().map(ToString::to_string).collect();
        assert_eq!(lines.len(), 15);
        assert_eq!(
            lines[0],
            "cargo clippy -p flui --locked --all-targets --no-default-features -- -D warnings"
        );
        assert_eq!(
            lines[7],
            "cargo clippy -p flui --locked --all-targets --no-default-features --features material,cupertino,localizations -- -D warnings"
        );
        assert_eq!(
            lines[13],
            "cargo clippy -p flui --locked --all-targets --all-features -- -D warnings"
        );
        // the defaults: no feature flag at all
        assert_eq!(
            lines[14],
            "cargo clippy -p flui --locked --all-targets -- -D warnings"
        );
        let distinct: std::collections::BTreeSet<&str> = COMBOS.into_iter().collect();
        assert_eq!(distinct.len(), COMBOS.len());
    }

    #[test]
    fn tree_facts_read_the_output_both_ways() {
        let [absent, brought_in, host] = TREE_FACTS;
        let with = "flui-app v0.1.0\n└── flui-hot-reload v0.1.0\n";
        let without = "flui-app v0.1.0\n└── flui-view v0.1.0\n";
        assert!(holds(&absent, without));
        assert!(!holds(&absent, with));
        assert!(holds(&brought_in, with));
        assert!(!holds(&brought_in, without));
        assert!(holds(
            &host,
            "flui-app feature \"hot-reload\"\n└── hot-reload-counter-host v0.1.0\n"
        ));
        assert!(!holds(&host, "flui-app feature \"default\"\n"));
        assert_eq!(
            Cmd::cargo(host.args).to_string(),
            "cargo tree -p hot-reload-counter-host --locked -e features -i flui-app"
        );
    }
}
