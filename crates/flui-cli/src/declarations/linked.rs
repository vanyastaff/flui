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
//!
//! The listing names a crate as cargo shows it to a person, by name, version
//! and source. Each line is resolved to the [`PackageId`] of the one package
//! of `cargo metadata`'s graph it names, so two crates of one name and
//! version from two sources (a registry's and a fork's) keep their own
//! features.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt::{self, Display, Formatter};
use std::path::Path;

use cargo_metadata::{Package, PackageId};

use crate::build::error::{BuildError, BuildResult};

/// The targets a build's graph is resolved for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target<'a> {
    /// One Rust target triple.
    Triple(&'a str),
    /// Every target: what any platform's build of the application links.
    Every,
}

impl Display for Target<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Triple(triple) => f.write_str(triple),
            Self::Every => f.write_str("every target"),
        }
    }
}

/// The dependency edges the built artifact links its crates by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Edges {
    /// Normal dependencies: a library's or a binary's build.
    Normal,
    /// Normal dependencies and the built packages' dev-dependencies: an
    /// example's build.
    NormalAndDev,
}

/// One build whose linked crates are read.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LinkedBuild<'a> {
    /// The directory cargo runs in.
    pub(crate) dir: &'a Path,
    pub(crate) target: Target<'a>,
    pub(crate) edges: Edges,
    /// The packages the build compiles. Cargo is asked for them by their
    /// ids, which no dependency of the same name can be mistaken for.
    pub(crate) roots: &'a [&'a Package],
    /// The features the build turns on beyond the defaults, each as
    /// `cargo --features` takes it.
    pub(crate) features: &'a [String],
}

/// The crates one build links, each with the features the build enables for
/// it.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct LinkedCrates {
    features: BTreeMap<PackageId, BTreeSet<String>>,
}

impl LinkedCrates {
    /// What `build` links. `graph` is every package `cargo metadata`
    /// resolves in the build's directory for at least the build's targets.
    ///
    /// # Errors
    ///
    /// The failure of `cargo tree`, a line of its listing that does not
    /// name one package of `graph`, or a listing without one of the build's
    /// roots.
    pub(crate) fn read(build: &LinkedBuild<'_>, graph: &[Package]) -> BuildResult<Self> {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
        let mut command = std::process::Command::new(cargo);
        command.current_dir(build.dir).args([
            "tree",
            "--target",
            match build.target {
                Target::Triple(triple) => triple,
                Target::Every => "all",
            },
            "--edges",
            match build.edges {
                Edges::Normal => "normal,no-proc-macro",
                Edges::NormalAndDev => "normal,dev,no-proc-macro",
            },
            "--prefix",
            "none",
            "--format",
            "{p}|{f}",
        ]);
        for root in build.roots {
            command.args(["--package", &root.id.repr]);
        }
        if !build.features.is_empty() {
            command.args(["--features", &build.features.join(",")]);
        }
        let failed = |reason: &dyn Display| {
            BuildError::Other(format!("cargo tree for {}: {reason}", build.target))
        };
        let output = command.output().map_err(|error| failed(&error))?;
        if !output.status.success() {
            return Err(failed(&String::from_utf8_lossy(&output.stderr).trim()));
        }
        Self::resolve(&String::from_utf8_lossy(&output.stdout), graph, build.roots)
            .map_err(|reason| failed(&reason))
    }

    /// The packages of `graph` that `listing` names, each with the features
    /// its lines give it.
    ///
    /// # Errors
    ///
    /// The first line that is not a crate with its features, or that names
    /// none or several of the packages of `graph`; then the first of
    /// `roots` the listing does not name, since a build links what it
    /// compiles and a listing without it would declare less than the build.
    fn resolve(listing: &str, graph: &[Package], roots: &[&Package]) -> Result<Self, String> {
        let mut linked = Self::default();
        for line in listing.lines().filter(|line| !line.trim().is_empty()) {
            let listed = Listed::parse(line).ok_or_else(|| {
                format!("the line `{line}` is not `<name> v<version>[ (<source>)]|<features>`")
            })?;
            linked
                .features
                .entry(listed.package_in(graph)?.id.clone())
                .or_default()
                .extend(
                    listed
                        .features
                        .split(',')
                        .map(str::trim)
                        .filter(|feature| !feature.is_empty())
                        .map(str::to_owned),
                );
        }
        match roots
            .iter()
            .find(|root| !linked.features.contains_key(&root.id))
        {
            Some(root) => Err(format!(
                "the listing does not name `{} v{}`, which the build compiles",
                root.name, root.version
            )),
            None => Ok(linked),
        }
    }

    /// The features the build enables for the package `id`, or `None` when
    /// the build does not link it.
    #[must_use]
    pub(crate) fn features_of(&self, id: &PackageId) -> Option<&BTreeSet<String>> {
        self.features.get(id)
    }
}

/// One line of the listing: `<name> v<version>[ (<source>)]|<feature>,…`,
/// which `cargo tree` ends in ` (*)` when it repeats a crate.
#[derive(Debug, Clone, Copy)]
struct Listed<'a> {
    name: &'a str,
    version: &'a str,
    /// What cargo prints of where the crate comes from; nothing for a crate
    /// of crates.io.
    source: Option<&'a str>,
    features: &'a str,
}

impl<'a> Listed<'a> {
    fn parse(line: &'a str) -> Option<Self> {
        // No feature name holds a `|`, so the last one ends the crate,
        // whatever its source's path holds.
        let (shown, features) = line.strip_suffix(" (*)").unwrap_or(line).rsplit_once('|')?;
        let (name, rest) = shown.split_once(' ')?;
        let rest = rest.strip_prefix('v')?;
        let (version, source) = match rest.split_once(' ') {
            Some((version, source)) => {
                (version, Some(source.strip_prefix('(')?.strip_suffix(')')?))
            }
            None => (rest, None),
        };
        (!name.is_empty() && !version.is_empty()).then_some(Self {
            name,
            version,
            source,
            features,
        })
    }

    /// The one package of `graph` this line names. A name and version one
    /// package of the graph has are that package, whatever its source; where
    /// several have them, the source tells them apart.
    fn package_in(self, graph: &'a [Package]) -> Result<&'a Package, String> {
        let same_version: Vec<&Package> = graph
            .iter()
            .filter(|package| {
                package.name.as_str() == self.name && package.version.to_string() == self.version
            })
            .collect();
        let named = match same_version.as_slice() {
            [package] => vec![*package],
            _ => same_version
                .iter()
                .copied()
                .filter(|package| self.is_from_source_of(package))
                .collect(),
        };
        match named.as_slice() {
            [package] => Ok(package),
            _ => Err(format!(
                "`{} v{}`{} names {} of the {} such package(s) cargo metadata resolves",
                self.name,
                self.version,
                self.source
                    .map_or_default(|source| format!(" from `{source}`")),
                if named.is_empty() { "none" } else { "several" },
                same_version.len(),
            )),
        }
    }

    /// Whether the source cargo printed on this line is `package`'s, as
    /// `cargo metadata` states it.
    fn is_from_source_of(self, package: &Package) -> bool {
        match (self.source, &package.source) {
            // crates.io is the one source cargo leaves out of the listing.
            (None, Some(source)) => source.is_crates_io(),
            // A path dependency has no source in the metadata; the listing
            // has its directory.
            (Some(directory), None) => package
                .manifest_path
                .parent()
                .is_some_and(|parent| parent.as_std_path() == Path::new(directory)),
            (Some(shown), Some(source)) => match source.repr.split_once('+') {
                // `<repository>[?<reference>]#<commit>`, which the listing
                // cuts to the commit's first characters.
                Some(("git", locked)) => match (locked.rsplit_once('#'), shown.rsplit_once('#')) {
                    (Some((repository, commit)), Some((shown_repository, short))) => {
                        repository == shown_repository
                            && !short.is_empty()
                            && commit.starts_with(short)
                    }
                    (None, None) => locked == shown,
                    _ => false,
                },
                // Another registry, listed as its index's URL or as the name
                // cargo's configuration gives it, which only that
                // configuration can tell from another's.
                Some(("registry" | "sparse", index)) if !source.is_crates_io() => shown
                    .strip_prefix("registry `")
                    .and_then(|label| label.strip_suffix('`'))
                    .is_some_and(|label| label == index || !label.contains("://")),
                _ => false,
            },
            (None, None) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use cargo_metadata::Package;

    use super::LinkedCrates;

    const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";
    const FORK: &str =
        "git+https://example.com/fork/util?branch=main#e7d82e99cb09a39e1e53927bd992af259874bb9d";

    /// A package as `cargo metadata` lists it: its source and its manifest.
    fn package(
        id: &str,
        name: &str,
        version: &str,
        source: Option<&str>,
        manifest: &str,
    ) -> Package {
        serde_json::from_value::<Package>(serde_json::json!({
            "id": id,
            "name": name,
            "version": version,
            "source": source,
            "manifest_path": manifest,
            "dependencies": [],
            "targets": [],
            "features": {},
        }))
        .expect("a package of cargo metadata")
    }

    fn graph() -> Vec<Package> {
        vec![
            package("app", "app", "0.1.0", None, "/work/my app|x/app/Cargo.toml"),
            package("lib", "lib", "0.1.0", None, "/work/lib/Cargo.toml"),
            package(
                "yoke-8",
                "yoke",
                "0.8.3",
                Some(CRATES_IO),
                "/registry/yoke-0.8.3/Cargo.toml",
            ),
            package(
                "yoke-7",
                "yoke",
                "0.7.0",
                Some(CRATES_IO),
                "/registry/yoke-0.7.0/Cargo.toml",
            ),
            package(
                "quiet",
                "quiet",
                "1.2.3-beta.1+build",
                Some("sparse+https://crates.example.com/index/"),
                "/registry/quiet/Cargo.toml",
            ),
            package(
                "util-registry",
                "util",
                "0.1.0",
                Some(CRATES_IO),
                "/registry/util-0.1.0/Cargo.toml",
            ),
            package(
                "util-fork",
                "util",
                "0.1.0",
                Some(FORK),
                "/git/checkouts/util/e7d82e9/Cargo.toml",
            ),
            package(
                "util-path",
                "util",
                "0.1.0",
                None,
                "/work/vendor/util/Cargo.toml",
            ),
            package(
                "twin-a",
                "twin",
                "1.0.0",
                Some("registry+https://a.example.com/index"),
                "/registry/a/twin/Cargo.toml",
            ),
            package(
                "twin-b",
                "twin",
                "1.0.0",
                Some("sparse+https://b.example.com/index/"),
                "/registry/b/twin/Cargo.toml",
            ),
        ]
    }

    fn read(linked: &LinkedCrates) -> Vec<(&str, Vec<&str>)> {
        linked
            .features
            .iter()
            .map(|(id, features)| {
                (
                    id.repr.as_str(),
                    features.iter().map(String::as_str).collect(),
                )
            })
            .collect()
    }

    /// A listing as `cargo tree` prints it reads back as the features of
    /// each package it names: a path with a space and a `|`, a registry
    /// crate without a source, a repeated line, one crate at two versions,
    /// a crate of another registry, and the blank line between two packages'
    /// trees. One name and version from crates.io, from a fork and from a
    /// directory are three packages, and a feature of one is not the
    /// others'. A line that is no crate, or that names none or several of
    /// the graph's packages, is refused, and so is a listing that leaves out
    /// a package the build compiles.
    #[test]
    fn a_listing_reads_back_as_each_crates_features() {
        let graph = graph();
        let listing = "app v0.1.0 (/work/my app|x/app)|\n\
                       lib v0.1.0 (/work/lib)|http,tls\n\
                       yoke v0.8.3|alloc,derive (*)\n\
                       lib v0.1.0 (/work/lib)|http,tls (*)\n\
                       \n\
                       yoke v0.7.0|alloc\n\
                       quiet v1.2.3-beta.1+build (registry `example`)|\n\
                       util v0.1.0|\n\
                       util v0.1.0 (https://example.com/fork/util?branch=main#e7d82e99)|http\n\
                       util v0.1.0 (/work/vendor/util)|camera\n\
                       twin v1.0.0 (registry `https://a.example.com/index`)|std\n";
        let app = &graph[0];
        let linked = LinkedCrates::resolve(listing, &graph, &[app]).expect("a listing");
        assert_eq!(
            read(&linked),
            [
                ("app", vec![]),
                ("lib", vec!["http", "tls"]),
                ("quiet", vec![]),
                ("twin-a", vec!["std"]),
                ("util-fork", vec!["http"]),
                ("util-path", vec!["camera"]),
                ("util-registry", vec![]),
                ("yoke-7", vec!["alloc"]),
                ("yoke-8", vec!["alloc", "derive"]),
            ]
        );
        assert_eq!(
            linked
                .features_of(&graph[1].id)
                .map(std::collections::BTreeSet::len),
            Some(2)
        );
        let only_lib = "lib v0.1.0 (/work/lib)|\n";
        assert_eq!(
            LinkedCrates::resolve(only_lib, &graph, &[])
                .expect("a listing")
                .features_of(&app.id),
            None,
            "a package the listing does not name is not linked"
        );
        let unlisted = LinkedCrates::resolve(only_lib, &graph, &[&graph[1], app])
            .expect_err("a listing without a package the build compiles");
        assert!(
            unlisted.contains("does not name `app v0.1.0`, which the build compiles"),
            "{unlisted}"
        );
        for (refused, reason) in [
            ("lib|http", "is not `<name> v<version>"),
            ("lib 0.1.0|http", "is not `<name> v<version>"),
            ("lib v0.1.0", "is not `<name> v<version>"),
            (" v0.1.0|http", "is not `<name> v<version>"),
            ("lib v0.1.0 /work/lib|http", "is not `<name> v<version>"),
            ("absent v1.0.0|", "names none of the 0 such package(s)"),
            (
                "util v0.1.0 (https://example.com/other/util?branch=main#e7d82e99)|",
                "names none of the 3 such package(s)",
            ),
            (
                "util v0.1.0 (https://example.com/fork/util?branch=main#0badc0de)|",
                "names none of the 3 such package(s)",
            ),
            ("util v0.1.0 (/work/elsewhere/util)|", "names none of the 3"),
            (
                "twin v1.0.0 (registry `mirror`)|",
                "names several of the 2 such package(s)",
            ),
        ] {
            let error = LinkedCrates::resolve(refused, &graph, &[]).expect_err(refused);
            assert!(error.contains(reason), "{refused:?}: {error}");
        }
    }
}
