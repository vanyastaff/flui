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
pub(crate) const COMBOS: [&str; 9] = [
    "--no-default-features",
    "--no-default-features --features material",
    "--no-default-features --features cupertino",
    "--no-default-features --features material,cupertino",
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
