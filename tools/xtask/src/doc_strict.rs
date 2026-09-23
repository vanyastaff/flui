//! rustdoc with warnings denied.
//!
//! The strict rustdoc gate: the whole workspace, private items included, with
//! every crate's `testing` feature ON.
//!
//! Why the feature list: a crate's test-support module is compiled only under
//! `#[cfg(any(test, feature = "testing"))]`, so whether `cargo doc` renders it
//! depends on which edge in the workspace happens to turn the feature on.
//! `flui-testing` activates `flui-interaction/testing` and
//! `flui-painting/testing` on NORMAL dependency edges, so those two are reached
//! by a plain `cargo doc --workspace`; `flui-rendering`, `flui-layer`, and
//! `flui-widgets` activate theirs only through self- and downstream
//! `[dev-dependencies]`, which `cargo doc` ignores, so those modules would
//! never be rendered and a broken intra-doc link in them would be invisible to
//! the gate. Passing the list explicitly makes coverage independent of which
//! edge exists; deriving it from `cargo metadata` rather than writing it here
//! means a crate that grows a `testing` feature is covered the day it does.
//!
//! `cargo xtask gate` and CI's `doc` job both run this command — one command,
//! one environment, no mirror to drift.

use std::process::{Command, ExitCode};

use anyhow::Context;

use crate::util::repo_root;

/// Arguments for `cargo xtask doc-strict`.
#[derive(Debug, clap::Args)]
pub(crate) struct DocStrictArgs {}

/// `cargo xtask doc-strict`: build the workspace docs with warnings denied.
pub(crate) fn doc_strict(_args: &DocStrictArgs) -> anyhow::Result<ExitCode> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .current_dir(repo_root())
        .no_deps()
        .other_options(vec!["--locked".to_owned()])
        .exec()
        .context("running `cargo metadata`")?;
    let features = testing_features(metadata.packages.iter().map(|package| {
        (
            package.name.as_str(),
            package.features.contains_key("testing"),
        )
    }));
    println!(
        "doc-strict: testing features on: {}",
        if features.is_empty() {
            "<none>"
        } else {
            &features
        }
    );

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .current_dir(repo_root())
        .env("RUSTDOCFLAGS", "-D warnings")
        .args([
            "doc",
            "--workspace",
            "--no-deps",
            "--locked",
            "--document-private-items",
            "--features",
            &features,
        ])
        .status()
        .context("spawning `cargo doc`")?;
    Ok(match status.code() {
        Some(0) => ExitCode::SUCCESS,
        Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        None => ExitCode::FAILURE,
    })
}

/// `name/testing` for every package that declares a `testing` feature, comma-joined in order.
fn testing_features<'a>(packages: impl IntoIterator<Item = (&'a str, bool)>) -> String {
    packages
        .into_iter()
        .filter(|(_, has_testing)| *has_testing)
        .map(|(name, _)| format!("{name}/testing"))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_packages_with_a_testing_feature_are_listed() {
        assert_eq!(
            testing_features([("a", true), ("b", false), ("c", true)]),
            "a/testing,c/testing"
        );
        assert_eq!(testing_features([("b", false)]), "");
    }
}
