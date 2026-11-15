//! The crates a build links, each with the features the build turns on for
//! it.
//!
//! `cargo metadata` resolves one graph for everything a workspace can build,
//! so its features are a union the built artifact does not have: a feature
//! only a dev-dependency turns on counts, as does one only a build script's
//! or a procedural macro's own copy of the crate turns on, which the resolver
//! builds apart for the host. A manifest written from that graph would
//! declare a permission for code the application does not link.
//!
//! `cargo tree` over normal edges, without procedural macros, resolves as
//! `cargo build` does for the target: it lists the crates the artifact links
//! and the features each is compiled with. An example's build links its
//! package's dev-dependencies as well, so their edges count for it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use cargo_metadata::Package;

use crate::build::error::{BuildError, BuildResult};

/// The crates one build links, by name and version, each with the features
/// the build enables for it.
///
/// A name and a version name one crate of a graph unless the graph takes the
/// same version from two sources, which then read as one crate with the
/// features of both.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct LinkedCrates {
    features: BTreeMap<String, BTreeMap<String, BTreeSet<String>>>,
}

impl LinkedCrates {
    /// What the build of `packages` at `app_dir` for `target`, with
    /// `features` turned on beyond the defaults, links; with `dev`, the build
    /// is an example's and links the packages' dev-dependencies too.
    ///
    /// # Errors
    ///
    /// The failure of `cargo tree`, or a line of its listing that names no
    /// crate.
    pub(crate) fn read(
        app_dir: &Path,
        target: &str,
        features: &[String],
        packages: &[&str],
        dev: bool,
    ) -> BuildResult<Self> {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut command = std::process::Command::new(cargo);
        command.current_dir(app_dir).args([
            "tree",
            "--target",
            target,
            "--edges",
            if dev {
                "normal,dev,no-proc-macro"
            } else {
                "normal,no-proc-macro"
            },
            "--prefix",
            "none",
            "--format",
            "{p}|{f}",
        ]);
        for package in packages {
            command.args(["--package", package]);
        }
        if !features.is_empty() {
            command.args(["--features", &features.join(",")]);
        }
        let failed = |reason: &dyn std::fmt::Display| {
            BuildError::Other(format!("cargo tree for {target}: {reason}"))
        };
        let output = command.output().map_err(|error| failed(&error))?;
        if !output.status.success() {
            return Err(failed(&String::from_utf8_lossy(&output.stderr).trim()));
        }
        Self::parse(&String::from_utf8_lossy(&output.stdout)).map_err(|line| {
            failed(&format_args!(
                "the line `{line}` is not `<name> v<version>|<features>`"
            ))
        })
    }

    /// The crates `listing` names, one per line as
    /// `<name> v<version>[ (<source>)]|<feature>,…`, a line `cargo tree`
    /// repeats ending in ` (*)`.
    ///
    /// # Errors
    ///
    /// The first line that is not of that shape.
    fn parse(listing: &str) -> Result<Self, &str> {
        let mut linked = Self::default();
        for line in listing.lines().filter(|line| !line.trim().is_empty()) {
            // No feature name holds a `|`, so the last one ends the crate,
            // whatever its source's path holds.
            let (name, version, features) = line
                .strip_suffix(" (*)")
                .unwrap_or(line)
                .rsplit_once('|')
                .and_then(|(crate_shown, features)| {
                    let (name, rest) = crate_shown.split_once(' ')?;
                    let version = rest.strip_prefix('v')?.split(' ').next()?;
                    (!name.is_empty() && !version.is_empty()).then_some((name, version, features))
                })
                .ok_or(line)?;
            linked
                .features
                .entry(name.to_owned())
                .or_default()
                .entry(version.to_owned())
                .or_default()
                .extend(
                    features
                        .split(',')
                        .map(str::trim)
                        .filter(|feature| !feature.is_empty())
                        .map(str::to_owned),
                );
        }
        Ok(linked)
    }

    /// The features the build enables for `package`, or `None` when the
    /// build does not link it.
    #[must_use]
    pub(crate) fn features_of(&self, package: &Package) -> Option<&BTreeSet<String>> {
        self.features
            .get(package.name.as_str())?
            .get(&package.version.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::LinkedCrates;

    /// A listing as `cargo tree` prints it reads back as each crate's
    /// features: a path with a space and a `|`, a registry crate without a
    /// source, a repeated line, one crate at two versions, and the blank
    /// line between two packages' trees; a line that names no crate is
    /// refused.
    #[test]
    fn a_listing_reads_back_as_each_crates_features() {
        let listing = "app v0.1.0 (/work/my app|x/app)|\n\
                       lib v0.1.0 (/work/lib)|http,tls\n\
                       yoke v0.8.3|alloc,derive (*)\n\
                       lib v0.1.0 (/work/lib)|http,tls (*)\n\
                       \n\
                       yoke v0.7.0|alloc\n\
                       quiet v1.2.3-beta.1+build|\n";
        let linked = LinkedCrates::parse(listing).expect("a listing");
        let read: Vec<(&str, &str, Vec<&str>)> = linked
            .features
            .iter()
            .flat_map(|(name, versions)| {
                versions.iter().map(|(version, features)| {
                    (
                        name.as_str(),
                        version.as_str(),
                        features.iter().map(String::as_str).collect(),
                    )
                })
            })
            .collect();
        assert_eq!(
            read,
            [
                ("app", "0.1.0", vec![]),
                ("lib", "0.1.0", vec!["http", "tls"]),
                ("quiet", "1.2.3-beta.1+build", vec![]),
                ("yoke", "0.7.0", vec!["alloc"]),
                ("yoke", "0.8.3", vec!["alloc", "derive"]),
            ]
        );
        for malformed in ["lib|http", "lib 0.1.0|http", "lib v0.1.0", " v0.1.0|http"] {
            assert_eq!(
                LinkedCrates::parse(malformed),
                Err(malformed),
                "{malformed:?}"
            );
        }
    }
}
