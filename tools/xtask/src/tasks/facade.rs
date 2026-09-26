//! `cargo xtask facade-combos`: every supported feature combination of the
//! `flui` facade, compiled on its own.

use super::deny_warnings;
use super::exec::{Cmd, Runner};

/// The facade's supported feature combinations; `""` is its defaults.
///
/// Each is its own clippy of the `flui` package alone: a `--workspace` build
/// proves nothing here, since feature unification would enable `material`
/// from a sibling and turn a broken combination green. `--all-targets` is
/// deliberate: a missing `required-features` on an example or test is exactly
/// the wiring these builds exist to catch. `cargo xtask reach` resolves the
/// facade under the same selections.
pub(crate) const COMBOS: [&str; 13] = [
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
    "--all-features",
    "",
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

/// `cargo xtask facade-combos`, stopping at the first failure.
pub(super) fn run(runner: Runner) -> anyhow::Result<()> {
    for cmd in clippy_plan() {
        runner.run(&cmd)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_combination_is_its_own_flui_clippy() {
        let lines: Vec<String> = clippy_plan().iter().map(ToString::to_string).collect();
        assert_eq!(lines.len(), 13);
        assert_eq!(
            lines[0],
            "cargo clippy -p flui --locked --all-targets --no-default-features -- -D warnings"
        );
        assert_eq!(
            lines[7],
            "cargo clippy -p flui --locked --all-targets --no-default-features --features material,cupertino,localizations -- -D warnings"
        );
        assert_eq!(
            lines[11],
            "cargo clippy -p flui --locked --all-targets --all-features -- -D warnings"
        );
        // the defaults: no feature flag at all
        assert_eq!(
            lines[12],
            "cargo clippy -p flui --locked --all-targets -- -D warnings"
        );
        let distinct: std::collections::BTreeSet<&str> = COMBOS.into_iter().collect();
        assert_eq!(distinct.len(), COMBOS.len());
    }
}
