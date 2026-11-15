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
//! The listing names a crate as cargo shows it to a person: by name, version
//! and source, a registry by the name cargo's configuration gives it, which
//! [`Registries`] reads back to its index. Each line is resolved to the
//! [`PackageId`] of one package of `cargo metadata`'s graph: the dependency
//! of the crate on the line above it that has the line's name and version,
//! and where that crate depends on two of them, the one whose id names the
//! line's source. Two crates of one name and version (a registry's and a
//! fork's) keep their own features.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt::Display;
use std::path::{Path, PathBuf};

use cargo_metadata::{Metadata, Node, Package, PackageId};

use crate::build::error::{BuildError, BuildResult};

/// The dependency edges the built artifact links its crates by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Edges {
    /// Normal dependencies: a library's or a binary's build.
    Normal,
    /// Normal dependencies and the built package's dev-dependencies: an
    /// example's build.
    NormalAndDev,
}

/// One build whose linked crates are read.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LinkedBuild<'a> {
    /// The directory cargo runs in.
    pub(crate) dir: &'a Path,
    /// The Rust target triple the build is for.
    pub(crate) target: &'a str,
    pub(crate) edges: Edges,
    /// The package the build compiles. Cargo is asked for it by its id,
    /// which no dependency of the same name can be mistaken for.
    pub(crate) root: &'a Package,
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
    /// What `build` links. `metadata` is what `cargo metadata` resolves in
    /// the build's directory for at least the build's target.
    ///
    /// # Errors
    ///
    /// The failure of `cargo tree`, a line of its listing that does not
    /// name one package of `metadata`'s graph, or a listing without the
    /// build's package.
    pub(crate) fn read(build: &LinkedBuild<'_>, metadata: &Metadata) -> BuildResult<Self> {
        let failed = |reason: &dyn Display| {
            BuildError::Other(format!("cargo tree for {}: {reason}", build.target))
        };
        let nodes = metadata
            .resolve
            .as_ref()
            .ok_or_else(|| failed(&"cargo metadata resolved no dependency graph"))?
            .nodes
            .as_slice();
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
        let mut command = std::process::Command::new(cargo);
        command.current_dir(build.dir).args([
            "tree",
            "--target",
            build.target,
            "--edges",
            match build.edges {
                Edges::Normal => "normal,no-proc-macro",
                Edges::NormalAndDev => "normal,dev,no-proc-macro",
            },
            "--prefix",
            "depth",
            "--color",
            "never",
            // The depth cargo prints first ends at the `|`, which no
            // package name holds; the features start after the last `|`,
            // which no feature name holds, whatever a source's path holds.
            "--format",
            "|{p}|{f}",
            "--package",
            &build.root.id.repr,
        ]);
        if !build.features.is_empty() {
            command.args(["--features", &build.features.join(",")]);
        }
        let output = command.output().map_err(|error| failed(&error))?;
        if !output.status.success() {
            return Err(failed(&String::from_utf8_lossy(&output.stderr).trim()));
        }
        Self::resolve(
            &String::from_utf8_lossy(&output.stdout),
            &metadata.packages,
            nodes,
            build.root,
            &Registries::configured_at(build.dir, CargoEnv::of_process()),
        )
        .map_err(|reason| failed(&reason))
    }

    /// The packages of the graph that `listing` names, each with the
    /// features its lines give it. `packages` and `nodes` are the graph,
    /// `root` the package the listing starts from, `registries` the indexes
    /// of the registries the listing names.
    ///
    /// # Errors
    ///
    /// The first line that is not a crate with its features, that is deeper
    /// than the line before it allows, or that names none or several of the
    /// dependencies of the crate it is listed under; then a listing that
    /// does not start from `root`, since a build links what it compiles and
    /// a listing without it would declare less than the build.
    fn resolve(
        listing: &str,
        packages: &[Package],
        nodes: &[Node],
        root: &Package,
        registries: &Registries,
    ) -> Result<Self, String> {
        let packages: BTreeMap<&PackageId, &Package> = packages
            .iter()
            .map(|package| (&package.id, package))
            .collect();
        let nodes: BTreeMap<&PackageId, &Node> =
            nodes.iter().map(|node| (&node.id, node)).collect();
        let mut linked = Self::default();
        // The package of each line above this one that is its ancestor, by
        // depth.
        let mut ancestors: Vec<&Package> = Vec::new();
        for line in listing.lines().filter(|line| !line.trim().is_empty()) {
            let (depth, listed) = Listed::parse(line).ok_or_else(|| {
                format!(
                    "the line `{line}` is not \
                     `<depth>|<name> v<version>[ (proc-macro)][ (<source>)]|<features>`"
                )
            })?;
            if depth > ancestors.len() {
                return Err(format!(
                    "the line `{line}` is deeper than the line before it allows"
                ));
            }
            ancestors.truncate(depth);
            let package = match ancestors.last() {
                None => listed.one_of(
                    std::iter::once(root),
                    "the package the build compiles",
                    registries,
                )?,
                Some(parent) => listed.one_of(
                    nodes
                        .get(&parent.id)
                        .into_iter()
                        .flat_map(|node| &node.deps)
                        .filter_map(|dependency| packages.get(&dependency.pkg).copied()),
                    &format!("the dependencies of `{} v{}`", parent.name, parent.version),
                    registries,
                )?,
            };
            ancestors.push(package);
            linked
                .features
                .entry(package.id.clone())
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
        if linked.features.contains_key(&root.id) {
            Ok(linked)
        } else {
            Err(format!(
                "the listing does not name `{} v{}`, which the build compiles",
                root.name, root.version
            ))
        }
    }

    /// The features the build enables for the package `id`, or `None` when
    /// the build does not link it.
    #[must_use]
    pub(crate) fn features_of(&self, id: &PackageId) -> Option<&BTreeSet<String>> {
        self.features.get(id)
    }
}

/// One line of the listing after its depth and its `|`:
/// `<name> v<version>[ (proc-macro)][ (<source>)]|<feature>,…`, which
/// `cargo tree` ends in ` (*)` when it repeats a crate.
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
    /// The line's depth in the tree and what it lists.
    fn parse(line: &'a str) -> Option<(usize, Self)> {
        // No package name holds a `|`, though one may start with a digit,
        // so the first `|` ends the depth; no feature name holds one either,
        // so the last starts the features, whatever a source's path holds.
        let (depth, listed) = line.split_once('|')?;
        let depth = depth.parse().ok()?;
        let (shown, features) = listed
            .strip_suffix(" (*)")
            .unwrap_or(listed)
            .rsplit_once('|')?;
        let (name, rest) = shown.split_once(' ')?;
        let rest = rest.strip_prefix('v')?;
        let (version, rest) = rest.split_once(' ').unwrap_or((rest, ""));
        let rest = rest
            .strip_prefix("(proc-macro)")
            .map_or(rest, str::trim_start);
        let source = match rest {
            "" => None,
            source => Some(source.strip_prefix('(')?.strip_suffix(')')?),
        };
        (!name.is_empty() && !version.is_empty()).then_some((
            depth,
            Self {
                name,
                version,
                source,
                features,
            },
        ))
    }

    /// The one of `candidates` this line names, `what` saying what they
    /// are. A name and version one candidate has are that package; where
    /// several have them, the source tells them apart.
    fn one_of<'p>(
        self,
        candidates: impl Iterator<Item = &'p Package>,
        what: &str,
        registries: &Registries,
    ) -> Result<&'p Package, String> {
        let same_version: Vec<&Package> = candidates
            .filter(|package| {
                package.name.as_str() == self.name && package.version.to_string() == self.version
            })
            .collect();
        let named: Vec<&Package> = match same_version.as_slice() {
            [package] => vec![*package],
            _ => same_version
                .iter()
                .copied()
                .filter(|package| self.is_from_source_of(package, registries))
                .collect(),
        };
        match named.as_slice() {
            [package] => Ok(package),
            _ => Err(format!(
                "`{} v{}`{} names {} of the {} such package(s) among {what}",
                self.name,
                self.version,
                self.source
                    .map_or_default(|source| format!(" from `{source}`")),
                if named.is_empty() { "none" } else { "several" },
                same_version.len(),
            )),
        }
    }

    /// Whether the source cargo printed on this line is `package`'s.
    ///
    /// The package's side is its id, the package id specification
    /// `<kind>+<url>[?<reference>]#[<name>@]<version>`, read up to the `#`:
    /// cargo writes a git reference into the id encoded as it writes it into
    /// the listing (`branch=fix%2Fa`), and into the metadata's `source`
    /// without that encoding.
    fn is_from_source_of(self, package: &Package, registries: &Registries) -> bool {
        let Some((source, _)) = package.id.repr.rsplit_once('#') else {
            return false;
        };
        let Some(shown) = self.source else {
            // crates.io is the one source cargo leaves out of the listing.
            return package
                .source
                .as_ref()
                .is_some_and(cargo_metadata::Source::is_crates_io);
        };
        // A registry is listed by the name the dependency gives it
        // (`registry = "<name>"`), or without one by its index, which a
        // sparse registry's id holds with its `sparse+`: cargo prints
        // ``registry `<SourceId::display_registry_name>` ``, the key of
        // `[registries.<name>]` when the source has one, else the index
        // (cargo `src/workspace/source_id.rs`, `display_registry_name` and
        // `impl Display for SourceId`). `cargo tree --format` has no key for
        // the package id, so a name is read back to its index through
        // cargo's configuration; a name no file or variable this reads
        // configures (a `--config` of the user's) stands for any registry's
        // crate.
        let same_index =
            |shown: &str, index: &str| shown.trim_end_matches('/') == index.trim_end_matches('/');
        let registry = |index: &str| {
            shown
                .strip_prefix("registry `")
                .and_then(|label| label.strip_suffix('`'))
                .is_some_and(|label| match registries.index_of(label) {
                    Some(configured) => same_index(configured, index),
                    None => same_index(label, index) || !label.contains("://"),
                })
        };
        match source.split_once('+') {
            // The listing has the directory.
            Some(("path", _)) => package
                .manifest_path
                .parent()
                .is_some_and(|parent| parent.as_std_path() == Path::new(shown)),
            // `<repository>[?<reference>]#<commit>`, the commit cut to its
            // first characters; one repository and reference resolve to one
            // commit.
            Some(("git", repository)) => shown
                .rsplit_once('#')
                .is_some_and(|(listed, _)| listed == repository),
            Some(("registry", index)) => {
                !package
                    .source
                    .as_ref()
                    .is_some_and(cargo_metadata::Source::is_crates_io)
                    && registry(index)
            }
            Some(("sparse", _)) => registry(source),
            _ => false,
        }
    }
}

/// The indexes of the registries cargo's configuration names, as cargo reads
/// them for a command run in one directory: the variable
/// `CARGO_REGISTRIES_<NAME>_INDEX` first, then `[registries.<name>] index`
/// of `.cargo/config.toml` (or `.cargo/config`, which cargo prefers when
/// both are there) in that directory and in each above it, the nearest
/// first, then of the one in `$CARGO_HOME`.
#[derive(Debug, Default)]
struct Registries {
    /// Each variable's index, by the name part of the variable
    /// (`COMPANY_A`).
    variables: BTreeMap<String, String>,
    /// Each file's index, by registry name, the nearest file's.
    files: BTreeMap<String, String>,
}

/// What of a process's environment cargo reads its configuration by.
#[derive(Debug, Default)]
struct CargoEnv {
    /// `CARGO_HOME` as the process has it.
    cargo_home: Option<OsString>,
    /// The user's home directory.
    home: Option<PathBuf>,
    /// Each `CARGO_REGISTRIES_<NAME>_INDEX`, by its name part.
    registry_indexes: BTreeMap<String, String>,
}

impl CargoEnv {
    /// This process's environment, which the cargo it starts inherits.
    fn of_process() -> Self {
        Self {
            cargo_home: std::env::var_os("CARGO_HOME"),
            home: std::env::home_dir(),
            registry_indexes: std::env::vars_os()
                .filter_map(|(key, value)| {
                    let key = key.into_string().ok()?;
                    let name = key
                        .strip_prefix("CARGO_REGISTRIES_")?
                        .strip_suffix("_INDEX")?;
                    Some((name.to_owned(), value.into_string().ok()?))
                })
                .collect(),
        }
    }
}

impl Registries {
    /// The registries cargo configures for a command run in `dir` with
    /// `env`. A file that is not TOML is skipped: cargo refused it before it
    /// listed anything.
    fn configured_at(dir: &Path, env: CargoEnv) -> Self {
        // As cargo's `home::cargo_home_with_cwd_env`: an empty `CARGO_HOME` is
        // unset, and a relative one is read against the directory cargo runs
        // in.
        let cargo_home = env
            .cargo_home
            .filter(|home| !home.is_empty())
            .map(|home| dir.join(home))
            .or_else(|| env.home.map(|home| home.join(".cargo")));
        let variables = env.registry_indexes;
        let mut files = BTreeMap::new();
        for cargo_dir in dir
            .ancestors()
            .map(|dir| dir.join(".cargo"))
            .chain(cargo_home)
        {
            let Some(table) = ["config", "config.toml"].iter().find_map(|file| {
                std::fs::read_to_string(cargo_dir.join(file))
                    .ok()?
                    .parse::<toml::Table>()
                    .ok()
            }) else {
                continue;
            };
            let configured = table
                .get("registries")
                .and_then(toml::Value::as_table)
                .into_iter()
                .flatten();
            for (name, registry) in configured {
                if let Some(index) = registry.get("index").and_then(toml::Value::as_str) {
                    files
                        .entry(name.clone())
                        .or_insert_with(|| index.to_owned());
                }
            }
        }
        Self { variables, files }
    }

    /// The index of the registry configured as `name`.
    fn index_of(&self, name: &str) -> Option<&str> {
        self.variables
            .get(&name.to_uppercase().replace('-', "_"))
            .or_else(|| self.files.get(name))
            .map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::Path;
    use std::process::Command;

    use cargo_metadata::{Node, Package};

    use super::{CargoEnv, Edges, LinkedBuild, LinkedCrates, Registries};

    const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

    /// A package as `cargo metadata` lists it: its id, source and manifest.
    fn package(id: &str, name: &str, version: &str, source: Option<&str>, dir: &str) -> Package {
        serde_json::from_value::<Package>(serde_json::json!({
            "id": id,
            "name": name,
            "version": version,
            "source": source,
            "manifest_path": format!("{dir}/Cargo.toml"),
            "dependencies": [],
            "targets": [],
            "features": {},
        }))
        .expect("a package of cargo metadata")
    }

    /// The node of `id` in `cargo metadata`'s graph, depending on `deps`.
    fn node(id: &str, deps: &[&str]) -> Node {
        serde_json::from_value::<Node>(serde_json::json!({
            "id": id,
            "deps": deps
                .iter()
                .map(|dep| serde_json::json!({ "name": "dep", "pkg": dep, "dep_kinds": [] }))
                .collect::<Vec<_>>(),
            "dependencies": deps,
            "features": [],
        }))
        .expect("a node of cargo metadata")
    }

    /// An application depending on a library, on one name and version from
    /// crates.io, a fork on a branch whose name cargo encodes and a
    /// directory, on two versions of one crate, on a crate whose name starts
    /// with a digit, and on crates of other registries: twins of one name
    /// and version from two registries cargo lists by their names, the
    /// library on one of them, and twins of another version from a sparse
    /// and a git registry cargo lists by their indexes. The graph also holds
    /// the crates.io copy of the forked crate, which nothing here links.
    fn graph() -> (Vec<Package>, Vec<Node>) {
        let fork = "git+https://example.com/fork/util?branch=fix/a+b#e7d82e99cb09a39e1e53927bd992af259874bb9d";
        let packages = vec![
            package(
                "path+file:///work/my%20app%7Cx/app#0.1.0",
                "app",
                "0.1.0",
                None,
                "/work/my app|x/app",
            ),
            package(
                "path+file:///work/lib#0.1.0",
                "lib",
                "0.1.0",
                None,
                "/work/lib",
            ),
            package(
                &format!("{CRATES_IO}#yoke@0.8.3"),
                "yoke",
                "0.8.3",
                Some(CRATES_IO),
                "/registry/yoke-0.8.3",
            ),
            package(
                &format!("{CRATES_IO}#yoke@0.7.0"),
                "yoke",
                "0.7.0",
                Some(CRATES_IO),
                "/registry/yoke-0.7.0",
            ),
            package(
                "sparse+https://crates.example.com/index/#quiet@1.2.3-beta.1+build",
                "quiet",
                "1.2.3-beta.1+build",
                Some("sparse+https://crates.example.com/index/"),
                "/registry/quiet",
            ),
            package(
                &format!("{CRATES_IO}#util@0.1.0"),
                "util",
                "0.1.0",
                Some(CRATES_IO),
                "/registry/util-0.1.0",
            ),
            package(
                "git+https://example.com/fork/util?branch=fix%2Fa%2Bb#0.1.0",
                "util",
                "0.1.0",
                Some(fork),
                "/git/checkouts/util/e7d82e9",
            ),
            package(
                "path+file:///work/vendor/util#0.1.0",
                "util",
                "0.1.0",
                None,
                "/work/vendor/util",
            ),
            package(
                "registry+https://a.example.com/index#twin@1.0.0",
                "twin",
                "1.0.0",
                Some("registry+https://a.example.com/index"),
                "/registry/a/twin",
            ),
            package(
                "sparse+https://b.example.com/index/#twin@1.0.0",
                "twin",
                "1.0.0",
                Some("sparse+https://b.example.com/index/"),
                "/registry/b/twin",
            ),
            package(
                "sparse+https://c.example.com/index/#twin@2.0.0",
                "twin",
                "2.0.0",
                Some("sparse+https://c.example.com/index/"),
                "/registry/c/twin",
            ),
            package(
                "registry+https://d.example.com/index#twin@2.0.0",
                "twin",
                "2.0.0",
                Some("registry+https://d.example.com/index"),
                "/registry/d/twin",
            ),
            package(
                &format!("{CRATES_IO}#7z-rust@0.1.0"),
                "7z-rust",
                "0.1.0",
                Some(CRATES_IO),
                "/registry/7z-rust-0.1.0",
            ),
        ];
        let nodes = vec![
            node(
                &packages[0].id.repr,
                &[1, 2, 4, 6, 7, 8, 9, 10, 11, 12].map(|index| packages[index].id.repr.as_str()),
            ),
            node(
                &packages[1].id.repr,
                &[3, 9].map(|index| packages[index].id.repr.as_str()),
            ),
        ];
        (packages, nodes)
    }

    /// Each package `linked` holds, by its index in `packages`, with its
    /// features.
    fn read<'a>(linked: &'a LinkedCrates, packages: &[Package]) -> Vec<(usize, Vec<&'a str>)> {
        packages
            .iter()
            .enumerate()
            .filter_map(|(index, package)| {
                let features = linked.features_of(&package.id)?;
                Some((index, features.iter().map(String::as_str).collect()))
            })
            .collect()
    }

    /// Two registries configured by name: `company-a`'s index in a file,
    /// `company-b`'s in a variable, which a file's value for it does not
    /// override.
    fn registries() -> Registries {
        Registries {
            variables: BTreeMap::from([(
                "COMPANY_B".to_owned(),
                "sparse+https://b.example.com/index/".to_owned(),
            )]),
            files: BTreeMap::from([
                (
                    "company-a".to_owned(),
                    "https://a.example.com/index/".to_owned(),
                ),
                (
                    "company-b".to_owned(),
                    "https://elsewhere.example.com/index".to_owned(),
                ),
                (
                    "company-c".to_owned(),
                    "https://c.example.com/other-index".to_owned(),
                ),
            ]),
        }
    }

    /// A listing as `cargo tree` prints it reads back as the features of
    /// each package it names: a path with a space and a `|`, a crate of
    /// crates.io without a source, a name that starts with a digit, a
    /// repeated line, one crate at two versions, crates of other registries
    /// named by their configuration's names or by their indexes, a sparse
    /// one with its `sparse+`. One name and version from crates.io, from a
    /// fork on an encoded branch and from a directory are three packages,
    /// and a feature of one is not the others'; twins of two registries
    /// cargo lists by name are told apart by the index each name configures.
    /// A line that is no crate, that skips a depth, or that names none or
    /// several of the dependencies of the crate above it is refused, and so
    /// is a listing that leaves out the package the build compiles.
    #[test]
    fn a_listing_reads_back_as_each_crates_features() {
        let (packages, nodes) = graph();
        let app = &packages[0];
        let registries = registries();
        let resolve =
            |listing: &str| LinkedCrates::resolve(listing, &packages, &nodes, app, &registries);
        let listing = "0|app v0.1.0 (/work/my app|x/app)|\n\
                       1|lib v0.1.0 (/work/lib)|http,tls\n\
                       2|yoke v0.7.0|alloc\n\
                       2|twin v1.0.0 (registry `company-b`)|std\n\
                       1|yoke v0.8.3|alloc,derive\n\
                       1|7z-rust v0.1.0|\n\
                       1|quiet v1.2.3-beta.1+build (registry `example`)|\n\
                       1|twin v1.0.0 (registry `company-a`)|\n\
                       1|twin v1.0.0 (registry `company-b`)|std\n\
                       1|twin v2.0.0 (registry `sparse+https://c.example.com/index/`)|c\n\
                       1|twin v2.0.0 (registry `https://d.example.com/index`)|d\n\
                       1|util v0.1.0 (https://example.com/fork/util?branch=fix%2Fa%2Bb#e7d82e99)|http\n\
                       1|util v0.1.0 (/work/vendor/util)|camera\n\
                       1|lib v0.1.0 (/work/lib)|http,tls (*)\n\
                       \n";
        let linked = resolve(listing).expect("a listing");
        assert_eq!(
            read(&linked, &packages),
            [
                (0, vec![]),
                (1, vec!["http", "tls"]),
                (2, vec!["alloc", "derive"]),
                (3, vec!["alloc"]),
                (4, vec![]),
                (6, vec!["http"]),
                (7, vec!["camera"]),
                (8, vec![]),
                (9, vec!["std"]),
                (10, vec!["c"]),
                (11, vec!["d"]),
                (12, vec![]),
            ],
            "the crates.io `util` the line without a source names is not linked: \
             the application depends on the fork and the directory"
        );
        let unlisted = resolve("").expect_err("a listing without the package the build compiles");
        assert!(
            unlisted.contains("does not name `app v0.1.0`, which the build compiles"),
            "{unlisted}"
        );
        let shape = "is not `<depth>|<name> v<version>";
        let app_line = "0|app v0.1.0 (/work/my app|x/app)|\n";
        for (refused, reason) in [
            ("0|app|http".to_owned(), shape),
            ("0|app 0.1.0|http".to_owned(), shape),
            ("0|app v0.1.0".to_owned(), shape),
            ("0| v0.1.0|http".to_owned(), shape),
            ("0|app v0.1.0 /work/app|http".to_owned(), shape),
            ("app v0.1.0 (/work/my app|x/app)|".to_owned(), shape),
            ("0app v0.1.0 (/work/my app|x/app)|".to_owned(), shape),
            (
                format!("{app_line}2|yoke v0.8.3|"),
                "is deeper than the line before it allows",
            ),
            (
                "0|lib v0.1.0 (/work/lib)|".to_owned(),
                "names none of the 0 such package(s) among the package the build compiles",
            ),
            (
                format!("{app_line}1|yoke v0.7.0|"),
                "names none of the 0 such package(s) among the dependencies of `app v0.1.0`",
            ),
            (
                format!("{app_line}1|util v0.1.0|"),
                "names none of the 2 such package(s)",
            ),
            (
                format!(
                    "{app_line}1|util v0.1.0 \
                     (https://example.com/other/util?branch=fix%2Fa%2Bb#e7d82e99)|"
                ),
                "names none of the 2 such package(s)",
            ),
            (
                format!(
                    "{app_line}1|util v0.1.0 \
                     (https://example.com/fork/util?branch=fix/a+b#e7d82e99)|"
                ),
                "names none of the 2 such package(s)",
            ),
            (
                format!("{app_line}1|util v0.1.0 (/work/elsewhere/util)|"),
                "names none of the 2",
            ),
            (
                format!("{app_line}1|twin v1.0.0 (registry `company-c`)|"),
                "names none of the 2 such package(s)",
            ),
            (
                format!("{app_line}1|twin v2.0.0 (registry `mirror`)|"),
                "names several of the 2 such package(s)",
            ),
        ] {
            let error = resolve(&refused).expect_err(&refused);
            assert!(error.contains(reason), "{refused:?}: {error}");
        }
    }

    /// The registries a command run in a directory reads: a variable above
    /// every file, the nearest `.cargo/config` above `.cargo/config.toml`
    /// beside it, a nearer directory's above a farther one's, then the file
    /// in `CARGO_HOME`, which a relative value names under the directory
    /// cargo runs in and an empty one leaves to the home directory's
    /// `.cargo`.
    #[test]
    fn registries_are_read_from_the_nearest_configuration() {
        let project = tempfile::tempdir().expect("a temporary directory");
        let write = |path: &str, contents: &str| {
            let path = project.path().join(path);
            std::fs::create_dir_all(path.parent().expect("a parent directory"))
                .expect("the directories");
            std::fs::write(path, contents).expect("a configuration file");
        };
        write(
            ".cargo/config.toml",
            "[registries.flui-outer]\nindex = \"https://outer.example.com/index\"\n\
             [registries.flui-near]\nindex = \"https://far.example.com/index\"\n",
        );
        write(
            "app/.cargo/config",
            "[registries.flui-near]\nindex = \"sparse+https://near.example.com/index/\"\n",
        );
        write(
            "app/.cargo/config.toml",
            "[registries.flui-near]\nindex = \"https://unread.example.com/index\"\n",
        );
        write(
            "app/src/cargo-home/config.toml",
            "[registries.flui-home]\nindex = \"https://home.example.com/index\"\n\
             [registries.flui-outer]\nindex = \"https://unread.example.com/index\"\n",
        );
        write(
            "user/.cargo/config.toml",
            "[registries.flui-user]\nindex = \"https://user.example.com/index\"\n",
        );
        let dir = project.path().join("app/src");
        let registries = Registries::configured_at(
            &dir,
            CargoEnv {
                cargo_home: Some("cargo-home".into()),
                home: Some(project.path().join("user")),
                registry_indexes: BTreeMap::from([(
                    "FLUI_OUTER".to_owned(),
                    "https://variable.example.com/index".to_owned(),
                )]),
            },
        );
        assert_eq!(
            registries.index_of("flui-near"),
            Some("sparse+https://near.example.com/index/")
        );
        assert_eq!(
            registries.index_of("flui-outer"),
            Some("https://variable.example.com/index")
        );
        assert_eq!(
            registries.index_of("flui-home"),
            Some("https://home.example.com/index"),
            "a relative CARGO_HOME under the directory cargo runs in"
        );
        assert_eq!(registries.index_of("flui-user"), None);
        assert_eq!(registries.index_of("flui-unconfigured"), None);

        let unset = Registries::configured_at(
            &dir,
            CargoEnv {
                cargo_home: Some("".into()),
                home: Some(project.path().join("user")),
                registry_indexes: BTreeMap::new(),
            },
        );
        assert_eq!(
            unset.index_of("flui-user"),
            Some("https://user.example.com/index"),
            "an empty CARGO_HOME leaves the home directory's"
        );
        assert_eq!(
            unset.index_of("flui-outer"),
            Some("https://outer.example.com/index")
        );
        assert_eq!(unset.index_of("flui-home"), None);
    }

    /// A procedural macro the build compiles is listed with its kind before
    /// its source.
    #[test]
    fn a_procedural_macro_is_listed_with_its_kind() {
        let macros = package(
            "git+https://example.com/macros#0.2.0",
            "macros",
            "0.2.0",
            Some("git+https://example.com/macros#0badc0de"),
            "/git/checkouts/macros/0badc0d",
        );
        let linked = LinkedCrates::resolve(
            "0|macros v0.2.0 (proc-macro) (https://example.com/macros#0badc0de)|extra\n",
            std::slice::from_ref(&macros),
            &[],
            &macros,
            &Registries::default(),
        )
        .expect("a procedural macro's listing");
        assert_eq!(
            linked.features_of(&macros.id),
            Some(&BTreeSet::from(["extra".to_owned()]))
        );
    }

    /// Run `git` with `args` in `dir`.
    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(["-c", "user.name=flui", "-c", "user.email=flui@example.com"])
            .args(args)
            .current_dir(dir)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?}");
    }

    /// Set in the process [`a_fork_on_an_encoded_branch_keeps_its_features`]
    /// runs its fixture in.
    const FORK_WORKER: &str = "FLUI_LINKED_FORK_WORKER";

    /// Cargo's own listing and graph of an application that depends on one
    /// name and version from a directory and from a git branch whose name
    /// cargo encodes (`fix/a+b`) read back as each crate's features. The
    /// fixture runs in a second process of this test binary whose
    /// `CARGO_HOME` is a temporary directory, so the git database and
    /// checkout cargo makes leave nothing behind.
    #[test]
    fn a_fork_on_an_encoded_branch_keeps_its_features() {
        if std::env::var_os(FORK_WORKER).is_some() {
            fork_fixture();
            return;
        }
        let cargo_home = tempfile::tempdir().expect("a temporary CARGO_HOME");
        let output = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "declarations::linked::tests::a_fork_on_an_encoded_branch_keeps_its_features",
                "--nocapture",
            ])
            .env(FORK_WORKER, "1")
            .env("CARGO_HOME", cargo_home.path())
            .env("CARGO_NET_OFFLINE", "false")
            .output()
            .expect("the fixture's process");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("1 passed"),
            "{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// The fixture of [`a_fork_on_an_encoded_branch_keeps_its_features`].
    fn fork_fixture() {
        let project = tempfile::tempdir().expect("a temporary directory");
        let write = |path: &str, contents: &str| {
            let path = project.path().join(path);
            std::fs::create_dir_all(path.parent().expect("a parent directory"))
                .expect("the directories");
            std::fs::write(path, contents).expect("a fixture file");
        };
        let util = |feature: &str| {
            format!(
                "[package]\nname = \"util\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
                 [features]\n{feature} = []\n"
            )
        };
        write("fork/Cargo.toml", &util("http"));
        write("fork/src/lib.rs", "");
        let fork = project.path().join("fork");
        git(&fork, &["init", "--quiet"]);
        git(&fork, &["add", "--all"]);
        git(&fork, &["commit", "--quiet", "--message", "util"]);
        git(&fork, &["branch", "fix/a+b"]);
        write("app/vendor/util/Cargo.toml", &util("camera"));
        write("app/vendor/util/src/lib.rs", "");
        write("app/src/lib.rs", "");
        // A file URL of the fork: `file:///C:/…` on Windows, `file:///tmp/…`
        // elsewhere.
        let fork_path = fork
            .to_str()
            .expect("a UTF-8 temporary path")
            .replace('\\', "/");
        let url = format!(
            "file://{}{fork_path}",
            if fork_path.starts_with('/') { "" } else { "/" }
        );
        write(
            "app/Cargo.toml",
            &format!(
                "[workspace]\n[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
                 [dependencies]\n\
                 util = {{ path = \"vendor/util\", features = [\"camera\"] }}\n\
                 forked = {{ package = \"util\", git = \"{url}\", branch = \"fix/a+b\", \
                 features = [\"http\"] }}\n"
            ),
        );
        let app = project.path().join("app");
        let metadata = cargo_metadata::MetadataCommand::new()
            .current_dir(&app)
            .env("CARGO_NET_OFFLINE", "false")
            .exec()
            .expect("cargo metadata of the application");
        let root = metadata.root_package().expect("the application");
        let linked = LinkedCrates::read(
            &LinkedBuild {
                dir: &app,
                target: "x86_64-unknown-linux-gnu",
                edges: Edges::Normal,
                root,
                features: &[],
            },
            &metadata,
        )
        .expect("the application's linked crates");
        let features = |from_git: bool| {
            let package = metadata
                .packages
                .iter()
                .find(|package| {
                    package.name.as_str() == "util" && package.source.is_some() == from_git
                })
                .expect("each copy of util");
            linked.features_of(&package.id).cloned()
        };
        assert_eq!(features(true), Some(BTreeSet::from(["http".to_owned()])));
        assert_eq!(features(false), Some(BTreeSet::from(["camera".to_owned()])));
    }
}
