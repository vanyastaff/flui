//! `cargo xtask module-dag`: the import direction between a crate's top-level
//! modules.
//!
//! A crate opts in with `[package.metadata.flui.modules]`: `layers`, its
//! top-level modules bottom to top; `transparent`, modules of `use` items
//! only (a prelude, a seam module), through which a path counts as a path to
//! what they re-export; and `exceptions`, each a refused edge that stays legal
//! until the change in the ADR its `exit` names removes it. Non-test code of a
//! module names only modules in lower layers; modules in one layer name none
//! of each other, except that `"*"`, alone in a layer, stands for every
//! module not named elsewhere, unchecked among themselves. `#[cfg(test)]`
//! code is exempt, as dev-dependencies are between crates. An exception the
//! scan no longer needs is a finding, so the list only shrinks.
//!
//! Why a scan (ADR-0078 §4, condition 1): no type or stock lint states the
//! rule. Visibility (`pub(crate)`, `pub(in ...)`) says who may name an item,
//! not which way modules depend; the catalog's items are `pub` and re-exported
//! at the root, so every module can name every other. clippy's
//! `disallowed_types`/`disallowed_methods` are configured per crate and cannot
//! vary by the importing module. Splitting the crate so cargo enforces the
//! direction was ruled out (`design/architecture.md`, flui-widgets stays one
//! crate). The other conditions hold: it reads a syn AST and the manifest
//! TOML, `--self-test` runs it on built-in crates with planted violations, and
//! its allowlist is manifest data that names an ADR and only shrinks.
//!
//! What counts as a dependency: every path in non-test code that starts at
//! `crate`, `$crate`, `self`, `super` or an `extern crate self` alias (`::`
//! before the alias too) — in
//! `use` items, types, expressions, patterns, bounds, impl headers, a macro's
//! path and the tokens of macro invocations and `macro_rules!` bodies. A path
//! through a root re-export, a transparent module, a glob re-export or a
//! `#[macro_export]` macro is followed to the module that owns the name. Doc
//! comments, string literals and `pub(in ...)` restrictions are not
//! dependencies. What the scan cannot attribute is a finding, never skipped.
//!
//! Limit: code a proc-macro generates, or a macro from another crate expands
//! to, is not seen; only the tokens written in this crate are.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, bail};
use serde_json::Value as Json;

use crate::util;

mod source;
#[cfg(test)]
mod tests;

use source::{Resolved, Scan, Site, Sources};

/// Arguments for `cargo xtask module-dag`.
#[derive(Debug, clap::Args)]
pub(crate) struct ModuleDagArgs {
    /// Check only this package (repeatable); it must declare
    /// `[package.metadata.flui.modules]`. Default: every package that does.
    #[arg(short = 'p', long = "package", conflicts_with = "self_test")]
    packages: Vec<String>,
    /// Run the rule over built-in crates with planted violations instead of
    /// the workspace; exit 1 unless it reports exactly those.
    #[arg(long)]
    self_test: bool,
    /// Print each production edge between top-level modules and exit 0: the
    /// scan's own run, to seed a declaration or its exceptions from.
    #[arg(long, conflicts_with = "self_test")]
    print_edges: bool,
}

/// `cargo xtask module-dag`. Exit code 1 lists every finding.
pub(crate) fn module_dag(args: &ModuleDagArgs) -> anyhow::Result<ExitCode> {
    if args.self_test {
        return Ok(self_test());
    }
    let root = util::repo_root();
    let metadata = util::metadata(&root)?;
    let packages = declaring_packages(&root, &metadata, &args.packages)?;
    if packages.is_empty() {
        println!("module-dag: no package declares `[package.metadata.flui.modules]`");
        return Ok(ExitCode::SUCCESS);
    }
    let adr_files = util::adr_files(&root);
    let mut failed = false;
    for package in packages {
        let declaration = Declaration::from_json(&package.modules, &package.manifest)?;
        let scan = source::scan(&Sources::on_disk(root.clone()), &package.lib)
            .with_context(|| format!("scanning {}", package.name))?;
        let outcome = check(&declaration, &scan, &adr_files);
        if args.print_edges {
            for ((from, to), sites) in &outcome.edges {
                println!(
                    "{}: {from} -> {to} ({} site(s), first {})",
                    package.name,
                    sites.len(),
                    sites[0]
                );
            }
            continue;
        }
        if outcome.findings.is_empty() {
            println!("module-dag: {}: {}", package.name, outcome.summary);
            continue;
        }
        failed = true;
        eprintln!(
            "module-dag: {}: {} problem(s)",
            package.name,
            outcome.findings.len()
        );
        for finding in &outcome.findings {
            eprintln!("  - {}: {finding}", package.name);
        }
    }
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// A workspace package with a `[package.metadata.flui.modules]` table.
struct Package {
    name: String,
    /// Manifest, repository-relative.
    manifest: String,
    /// The library root file, repository-relative.
    lib: String,
    modules: Json,
}

/// The packages to check: those named by `-p`, which must declare modules,
/// or every package that does.
fn declaring_packages(
    root: &Path,
    metadata: &cargo_metadata::Metadata,
    selected: &[String],
) -> anyhow::Result<Vec<Package>> {
    let workspace = metadata.workspace_packages();
    for name in selected {
        if !workspace
            .iter()
            .any(|package| package.name.as_str() == name)
        {
            bail!("-p {name}: no workspace package has that name");
        }
    }
    let mut packages = Vec::new();
    for package in workspace {
        let named = selected.iter().any(|name| name == package.name.as_str());
        if !selected.is_empty() && !named {
            continue;
        }
        let modules = &package.metadata["flui"]["modules"];
        if modules.is_null() {
            if named {
                bail!(
                    "-p {}: the package declares no `[package.metadata.flui.modules]`",
                    package.name
                );
            }
            continue;
        }
        let lib = package
            .targets
            .iter()
            .find(|target| {
                !(target.is_bin()
                    || target.is_example()
                    || target.is_test()
                    || target.is_bench()
                    || target.is_custom_build())
            })
            .with_context(|| {
                format!(
                    "{} declares `modules` but has no library target",
                    package.name
                )
            })?;
        packages.push(Package {
            name: package.name.to_string(),
            manifest: crate::workspace::relative(root, package.manifest_path.as_std_path())?,
            lib: crate::workspace::relative(root, lib.src_path.as_std_path())?,
            modules: modules.clone(),
        });
    }
    Ok(packages)
}

/// The keys of `[package.metadata.flui.modules]`; the set is closed.
const KEYS: [&str; 3] = ["layers", "transparent", "exceptions"];

/// The keys of one `exceptions` entry, all required.
const EXCEPTION_KEYS: [&str; 5] = ["from", "to", "exit", "since", "reason"];

/// The layer entry that stands for every module not named elsewhere.
const WILDCARD: &str = "*";

/// `[package.metadata.flui.modules]`.
#[derive(Debug, Default)]
struct Declaration {
    layers: Vec<Vec<String>>,
    transparent: Vec<String>,
    exceptions: Vec<Exception>,
}

/// A refused edge, legal until the change in the ADR named by `exit`.
#[derive(Debug, Clone)]
struct Exception {
    from: String,
    to: String,
    exit: String,
    since: String,
    /// Read by people, not by the check; required so no entry is unexplained.
    #[expect(dead_code, reason = "required in the manifest, read by reviewers")]
    reason: String,
}

impl Declaration {
    /// Reads the table; a shape the check cannot read is an error.
    fn from_json(modules: &Json, rel: &str) -> anyhow::Result<Self> {
        let table = modules
            .as_object()
            .with_context(|| format!("{rel}: `[package.metadata.flui] modules` must be a table"))?;
        if let Some(key) = table.keys().find(|key| !KEYS.contains(&key.as_str())) {
            bail!(
                "{rel}: unknown `[package.metadata.flui.modules]` key `{key}` (known: {})",
                KEYS.join(", ")
            );
        }
        let names = |value: &Json, what: &str| -> anyhow::Result<Vec<String>> {
            value
                .as_array()
                .and_then(|names| {
                    names
                        .iter()
                        .map(|name| name.as_str().map(str::to_owned))
                        .collect()
                })
                .with_context(|| format!("{rel}: {what} must be a list of module names"))
        };
        let layers = table
            .get("layers")
            .with_context(|| format!("{rel}: `[package.metadata.flui.modules]` needs `layers`"))?
            .as_array()
            .with_context(|| format!("{rel}: `layers` must be a list of lists of module names"))?
            .iter()
            .map(|layer| names(layer, "each layer"))
            .collect::<anyhow::Result<_>>()?;
        let transparent = match table.get("transparent") {
            None => Vec::new(),
            Some(value) => names(value, "`transparent`")?,
        };
        let malformed = || {
            format!(
                "{rel}: `exceptions` must be a list of `{{ from = \"<module>\", to = \
                 \"<module>\", exit = \"ADR-NNNN\", since = \"YYYY-MM-DD\", reason = \"<text>\" }}`"
            )
        };
        let exceptions = match table.get("exceptions") {
            None => Vec::new(),
            Some(value) => value
                .as_array()
                .with_context(malformed)?
                .iter()
                .map(|entry| {
                    let entry = entry.as_object().with_context(malformed)?;
                    if entry.len() != EXCEPTION_KEYS.len()
                        || !entry
                            .keys()
                            .all(|key| EXCEPTION_KEYS.contains(&key.as_str()))
                    {
                        bail!(malformed());
                    }
                    let field = |key: &str| {
                        entry
                            .get(key)
                            .and_then(Json::as_str)
                            .map(str::to_owned)
                            .with_context(malformed)
                    };
                    Ok(Exception {
                        from: field("from")?,
                        to: field("to")?,
                        exit: field("exit")?,
                        since: field("since")?,
                        reason: field("reason")?,
                    })
                })
                .collect::<anyhow::Result<_>>()?,
        };
        Ok(Self {
            layers,
            transparent,
            exceptions,
        })
    }
}

/// One problem with a crate's modules or their declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Finding {
    /// Non-test code names a module in the same or a higher layer, and no
    /// exception admits the edge.
    Refused {
        from: String,
        from_layer: usize,
        to: String,
        to_layer: usize,
        sites: Vec<Site>,
    },
    /// A `crate`-relative path the scan cannot attribute to a module.
    Unattributed {
        from: String,
        site: Site,
        why: String,
    },
    /// A top-level module neither `layers` nor `transparent` names, with no
    /// `"*"` layer to cover it.
    Undeclared { module: String },
    /// A declared name that is not a top-level module of the crate.
    Unknown { name: String },
    /// A name declared more than once across `layers` and `transparent`.
    Duplicate { name: String },
    /// A declared name with `::`: only top-level modules are nodes.
    Nested { name: String },
    /// `"*"` in a layer with other names, or in more than one layer.
    Wildcard { layer: usize },
    /// A transparent module holding an item other than a `use`.
    RelayItem { module: String, site: Site },
    /// An exception for an edge the layers admit, one the code does not
    /// have, or a repeated one.
    Stale { from: String, to: String },
    /// An exception whose `from` or `to` is not a module of the crate.
    ExceptionModule { from: String, to: String },
    /// An exception whose `exit` is not an `ADR-NNNN` number.
    BadExit {
        from: String,
        to: String,
        exit: String,
    },
    /// An exception whose `exit` names no file under docs/adr.
    MissingAdr {
        from: String,
        to: String,
        exit: String,
    },
    /// An exception whose `since` is not a `YYYY-MM-DD` date.
    BadDate {
        from: String,
        to: String,
        since: String,
    },
}

type Identity = (String, String, &'static str);

impl Finding {
    /// `(subject, object, kind)`: what the self-test compares.
    fn identity(&self) -> Identity {
        let pair = |a: &str, b: &str, kind| (a.to_owned(), b.to_owned(), kind);
        match self {
            Self::Refused { from, to, .. } => pair(from, to, "refused"),
            Self::Unattributed { from, site, .. } => pair(from, &site.text, "unattributed"),
            Self::Undeclared { module } => pair(module, "", "undeclared"),
            Self::Unknown { name } => pair(name, "", "unknown"),
            Self::Duplicate { name } => pair(name, "", "duplicate"),
            Self::Nested { name } => pair(name, "", "nested"),
            Self::Wildcard { layer } => pair(&format!("layer {layer}"), WILDCARD, "wildcard"),
            Self::RelayItem { module, .. } => pair(module, "", "relay item"),
            Self::Stale { from, to } => pair(from, to, "stale exception"),
            Self::ExceptionModule { from, to } => pair(from, to, "exception module"),
            Self::BadExit { from, to, .. } => pair(from, to, "bad exit"),
            Self::MissingAdr { from, to, .. } => pair(from, to, "missing adr"),
            Self::BadDate { from, to, .. } => pair(from, to, "bad date"),
        }
    }
}

/// How many sites a refused edge lists before the total.
const SITES_SHOWN: usize = 3;

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused {
                from,
                from_layer,
                to,
                to_layer,
                sites,
            } => {
                let shown: Vec<String> = sites
                    .iter()
                    .take(SITES_SHOWN)
                    .map(ToString::to_string)
                    .collect();
                write!(
                    f,
                    "{from} (layer {from_layer}) imports {to} (layer {to_layer}) at {} ({} \
                     site(s)); non-test code names only modules in lower layers: move the \
                     code, change `layers`, or add a dated `exceptions` entry with the ADR that \
                     removes the edge",
                    shown.join(", "),
                    sites.len()
                )
            }
            Self::Unattributed { from, site, why } => write!(
                f,
                "{from} names {site}, which the scan cannot attribute to a module: {why}"
            ),
            Self::Undeclared { module } => write!(
                f,
                "the top-level module `{module}` is in no layer and not `transparent`; declare \
                 it"
            ),
            Self::Unknown { name } => write!(
                f,
                "`{name}` is declared, but it is not a top-level module compiled outside tests"
            ),
            Self::Duplicate { name } => write!(f, "`{name}` is declared more than once"),
            Self::Nested { name } => write!(
                f,
                "`{name}` is a nested module; nested modules are not nodes yet, only top-level \
                 ones"
            ),
            Self::Wildcard { layer } => write!(
                f,
                "layer {layer} misuses \"*\": it stands alone in its layer, in one layer only"
            ),
            Self::RelayItem { module, site } => write!(
                f,
                "the transparent module `{module}` holds {site}; a transparent module holds \
                 only `use` items"
            ),
            Self::Stale { from, to } => write!(
                f,
                "the exception for {from} -> {to} admits nothing: the layers allow the edge, \
                 the code no longer has it, or it repeats an entry; remove it"
            ),
            Self::ExceptionModule { from, to } => write!(
                f,
                "the exception for {from} -> {to} names a module the crate does not have"
            ),
            Self::BadExit { from, to, exit } => write!(
                f,
                "the exception for {from} -> {to} names exit \"{exit}\", which is not an \
                 `ADR-NNNN` number"
            ),
            Self::MissingAdr { from, to, exit } => write!(
                f,
                "the exception for {from} -> {to} names {exit}, which has no file under docs/adr"
            ),
            Self::BadDate { from, to, since } => write!(
                f,
                "the exception for {from} -> {to} has `since = \"{since}\"`, which is not a \
                 YYYY-MM-DD date"
            ),
        }
    }
}

/// What a check of one crate found.
#[derive(Debug)]
struct Outcome {
    findings: Vec<Finding>,
    /// Every non-test edge between two nodes, with where it is named.
    edges: BTreeMap<(String, String), Vec<Site>>,
    /// The one-line summary of a green run.
    summary: String,
}

/// Where a declared name sits: a layer, or transparent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placement {
    Layer(usize),
    Transparent,
}

/// Checks `scan` against `declaration`; `adr_files` are the names under
/// docs/adr (see [`util::adr_files`]).
fn check(declaration: &Declaration, scan: &Scan, adr_files: &[String]) -> Outcome {
    let mut findings = Vec::new();

    let mut placed: BTreeMap<String, Placement> = BTreeMap::new();
    let mut wildcard = None;
    let mut place = |name: &str, at: Placement, findings: &mut Vec<Finding>| {
        if placed.contains_key(name) {
            // Reported once, however many repeats.
            if !findings
                .iter()
                .any(|finding| matches!(finding, Finding::Duplicate { name: seen } if seen == name))
            {
                findings.push(Finding::Duplicate {
                    name: name.to_owned(),
                });
            }
            return;
        }
        if name.contains("::") {
            findings.push(Finding::Nested {
                name: name.to_owned(),
            });
        } else if !scan.modules.contains_key(name) {
            findings.push(Finding::Unknown {
                name: name.to_owned(),
            });
        }
        placed.insert(name.to_owned(), at);
    };
    for (index, layer) in declaration.layers.iter().enumerate() {
        for name in layer {
            if name == WILDCARD {
                if wildcard.is_some() || layer.len() != 1 {
                    findings.push(Finding::Wildcard { layer: index });
                } else {
                    wildcard = Some(index);
                }
            } else {
                place(name, Placement::Layer(index), &mut findings);
            }
        }
    }
    for name in &declaration.transparent {
        place(name, Placement::Transparent, &mut findings);
    }

    let layer_of = |name: &str| match placed.get(name) {
        Some(Placement::Layer(layer)) => Some(*layer),
        Some(Placement::Transparent) => None,
        None => wildcard,
    };
    for module in scan.modules.keys() {
        if !placed.contains_key(module.as_str()) && wildcard.is_none() {
            findings.push(Finding::Undeclared {
                module: module.clone(),
            });
        }
    }

    let transparent: BTreeSet<String> = placed
        .iter()
        .filter(|(name, at)| **at == Placement::Transparent && scan.modules.contains_key(*name))
        .map(|(name, _)| name.clone())
        .collect();
    for module in &transparent {
        if let Some(site) = scan.first_item.get(module) {
            findings.push(Finding::RelayItem {
                module: module.clone(),
                site: site.clone(),
            });
        }
    }

    let mut edges: BTreeMap<(String, String), Vec<Site>> = BTreeMap::new();
    for reference in &scan.references {
        if transparent.contains(&reference.from) {
            continue; // a relay's own `use` items are not edges
        }
        let to = match scan.resolve(&reference.path, &transparent) {
            Resolved::Module(to) => to,
            // The root is no node: its own code is not scanned, so an edge
            // into it could hide one out of it.
            Resolved::Root(name) => {
                findings.push(Finding::Unattributed {
                    from: reference.from.clone(),
                    site: reference.site.clone(),
                    why: format!(
                        "it names `{name}`, an item of the crate root itself; move it into a \
                         module"
                    ),
                });
                continue;
            }
            Resolved::External => continue,
            Resolved::Unattributed(why) => {
                findings.push(Finding::Unattributed {
                    from: reference.from.clone(),
                    site: reference.site.clone(),
                    why,
                });
                continue;
            }
        };
        if to != reference.from {
            edges
                .entry((reference.from.clone(), to))
                .or_default()
                .push(reference.site.clone());
        }
    }
    for sites in edges.values_mut() {
        sites.sort();
        sites.dedup();
    }

    let mut refused = BTreeSet::new();
    for ((from, to), sites) in &edges {
        let (Some(from_layer), Some(to_layer)) = (layer_of(from), layer_of(to)) else {
            continue; // an undeclared module is reported above
        };
        if to_layer < from_layer || (to_layer == from_layer && Some(to_layer) == wildcard) {
            continue;
        }
        refused.insert((from.as_str(), to.as_str()));
        if declaration
            .exceptions
            .iter()
            .any(|entry| entry.from == *from && entry.to == *to)
        {
            continue;
        }
        findings.push(Finding::Refused {
            from: from.clone(),
            from_layer,
            to: to.clone(),
            to_layer,
            sites: sites.clone(),
        });
    }

    let mut seen = BTreeSet::new();
    for entry in &declaration.exceptions {
        let (from, to) = (entry.from.clone(), entry.to.clone());
        if !scan.modules.contains_key(&entry.from) || !scan.modules.contains_key(&entry.to) {
            findings.push(Finding::ExceptionModule { from, to });
            continue;
        }
        let first = seen.insert((entry.from.as_str(), entry.to.as_str()));
        if !first || !refused.contains(&(entry.from.as_str(), entry.to.as_str())) {
            findings.push(Finding::Stale {
                from: from.clone(),
                to: to.clone(),
            });
        }
        if !util::is_adr_number(&entry.exit) {
            findings.push(Finding::BadExit {
                from: from.clone(),
                to: to.clone(),
                exit: entry.exit.clone(),
            });
        } else if !util::adr_exists(adr_files, &entry.exit) {
            findings.push(Finding::MissingAdr {
                from: from.clone(),
                to: to.clone(),
                exit: entry.exit.clone(),
            });
        }
        if !is_date(&entry.since) {
            findings.push(Finding::BadDate {
                from,
                to,
                since: entry.since.clone(),
            });
        }
    }

    let layered = scan
        .modules
        .keys()
        .filter(|module| layer_of(module).is_some())
        .count();
    let summary = format!(
        "{} modules ({layered} in {} layers, {} transparent), {} edges, {} exceptions checked",
        scan.modules.len(),
        declaration.layers.len(),
        transparent.len(),
        edges.len(),
        declaration.exceptions.len()
    );
    Outcome {
        findings,
        edges,
        summary,
    }
}

/// Whether `text` is a calendar date spelled `YYYY-MM-DD`.
fn is_date(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    let [year, month, day] = parts.as_slice() else {
        return false;
    };
    let digits = |part: &str, len: usize| {
        part.len() == len && part.bytes().all(|byte| byte.is_ascii_digit())
    };
    if !(digits(year, 4) && digits(month, 2) && digits(day, 2)) {
        return false;
    }
    let (Ok(year), Ok(month), Ok(day)) = (
        year.parse::<u32>(),
        month.parse::<u32>(),
        day.parse::<u32>(),
    ) else {
        return false;
    };
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => return false,
    };
    (1..=days).contains(&day)
}

/// `[package.metadata.flui.modules]` written as TOML, read as cargo metadata
/// hands it over.
fn declaration_from_toml(text: &str) -> anyhow::Result<Declaration> {
    let table: toml::Table = toml::from_str(text).context("parsing the declaration")?;
    Declaration::from_json(&serde_json::to_value(table)?, "<declaration>")
}

/// A self-test crate: its declaration and its files under `<name>/src/`.
struct Planted {
    name: &'static str,
    declaration: &'static str,
    files: &'static [(&'static str, &'static str)],
}

/// The self-test crates: every legal shape stays silent, and each planted
/// violation produces one finding of [`EXPECTED`].
const PLANTED: [Planted; 2] = [
    Planted {
        name: "a",
        declaration: r#"
layers = [
    ["base", "peer_a", "peer_b", "via_use", "via_root", "via_relay", "via_relay_plain",
     "via_expr", "via_super",
     "via_macro", "via_macro_rules", "via_macro_export", "via_alias", "via_feature",
     "via_unknown", "via_admitted", "via_bad_exit", "via_missing_adr", "via_bad_date",
     "quiet", "quiet", "phantom", "top::inner"],
    ["top"],
]
transparent = ["relay"]
exceptions = [
    { from = "via_admitted", to = "top", exit = "ADR-0001", since = "2026-01-31", reason = "admitted" },
    { from = "base", to = "top", exit = "ADR-0001", since = "2026-01-31", reason = "stale" },
    { from = "via_bad_exit", to = "top", exit = "0001", since = "2026-01-31", reason = "bad exit" },
    { from = "via_missing_adr", to = "top", exit = "ADR-0999", since = "2026-01-31", reason = "no ADR" },
    { from = "via_bad_date", to = "top", exit = "ADR-0001", since = "2026-02-30", reason = "bad date" },
    { from = "nowhere", to = "top", exit = "ADR-0001", since = "2026-01-31", reason = "no module" },
]
"#,
        files: &[
            (
                "lib.rs",
                "//! Planted crate.\n\
                 extern crate self as alias_a;\n\
                 pub mod base;\npub mod peer_a;\npub mod peer_b;\npub mod top;\n\
                 pub mod via_use;\npub mod via_root;\npub mod via_relay;\n\
                 pub mod via_relay_plain;\npub mod via_expr;\n\
                 pub mod via_super;\npub mod via_macro;\npub mod via_macro_rules;\n\
                 pub mod via_macro_export;\npub mod via_alias;\n\
                 #[cfg(any(test, feature = \"testing\"))]\npub mod via_feature;\n\
                 pub mod via_unknown;\npub mod via_admitted;\npub mod via_bad_exit;\n\
                 pub mod via_missing_adr;\npub mod via_bad_date;\npub mod quiet;\n\
                 pub mod ghost;\npub mod relay;\n\
                 #[cfg(test)]\nmod root_tests;\n\
                 pub use top::Top;\n",
            ),
            ("base.rs", "pub struct Base;\n"),
            (
                "peer_a.rs",
                "pub fn f() -> crate::peer_b::B { crate::peer_b::B }\n",
            ),
            ("peer_b.rs", "pub struct B;\n"),
            (
                "top.rs",
                "use crate::base::Base;\n\
                 pub struct Top;\npub struct Deep;\n\
                 impl Top { pub fn new() -> Self { let _ = Base; Top } }\n\
                 #[macro_export]\nmacro_rules! top_macro { () => {}; }\n",
            ),
            ("via_use.rs", "use crate::top::Top;\n"),
            ("via_root.rs", "use crate::Top;\n"),
            ("via_relay.rs", "use crate::relay::Deep;\n"),
            (
                "relay.rs",
                "pub use crate::top::Deep;\nuse crate::top;\npub use top::Top as Plain;\n\
                 pub fn stray() {}\n",
            ),
            ("via_relay_plain.rs", "use crate::relay::Plain;\n"),
            (
                "via_expr.rs",
                "pub fn f() { let _ = crate::top::Top::new(); }\n",
            ),
            ("via_super/mod.rs", "mod inner;\n"),
            ("via_super/inner.rs", "use super::super::top::Top;\n"),
            (
                "via_macro.rs",
                "pub fn f() { let _ = vec![crate::top::Top::new()]; }\n",
            ),
            (
                "via_macro_rules.rs",
                "macro_rules! make { () => { $crate::top::Top::new() }; }\n",
            ),
            (
                "via_macro_export.rs",
                "pub fn f() { crate::top_macro!(); }\n",
            ),
            (
                "via_alias.rs",
                "pub fn f() -> alias_a::top::Top { Top::new() }\n",
            ),
            (
                "via_feature.rs",
                "pub fn f() -> crate::top::Top { crate::top::Top::new() }\n",
            ),
            ("via_unknown.rs", "use crate::Nowhere;\n"),
            ("via_admitted.rs", "use crate::top::Top;\n"),
            ("via_bad_exit.rs", "use crate::top::Top;\n"),
            ("via_missing_adr.rs", "use crate::top::Top;\n"),
            ("via_bad_date.rs", "use crate::top::Top;\n"),
            (
                "quiet.rs",
                "//! Links [`crate::top::Top`] in a doc comment.\n\
                 /// Names [`crate::top::Top`] in a doc link.\n\
                 pub const NAME: &str = \"crate::top::Top\";\n\
                 pub(in crate::top) fn restricted() {}\n\
                 pub struct Quiet;\n\
                 impl Quiet {\n    #[cfg(test)]\n    fn fixture() -> crate::top::Top { crate::top::Top::new() }\n}\n\
                 #[cfg(all(test, feature = \"images\"))]\n\
                 fn only_in_tests() -> crate::top::Top { crate::top::Top::new() }\n\
                 #[cfg(test)]\nmod tests { use crate::top::Top; }\n\
                 #[cfg(test)]\nmod mounted;\n",
            ),
            ("quiet/mounted.rs", "use crate::top::Top;\n"),
            ("root_tests.rs", "use crate::top::Top;\n"),
            ("ghost.rs", "pub struct Ghost;\n"),
        ],
    },
    Planted {
        name: "b",
        declaration: r#"
layers = [["low"], ["*"]]
transparent = ["prelude"]
"#,
        files: &[
            (
                "lib.rs",
                "pub mod low;\npub mod wild_a;\npub mod wild_b;\n\
                 pub mod prelude { pub use crate::wild_a::A; }\n",
            ),
            ("low.rs", "pub struct Low;\nuse crate::prelude::A;\n"),
            (
                "wild_a.rs",
                "pub struct A;\npub fn f() -> crate::wild_b::B { crate::wild_b::B }\n",
            ),
            (
                "wild_b.rs",
                "pub struct B;\npub fn g() -> crate::wild_a::A { crate::wild_a::A }\n\
                 use crate::low::Low;\n",
            ),
        ],
    },
];

/// The finding each planted violation must produce, and no other.
const EXPECTED: [(&str, &str, &str); 24] = [
    ("via_use", "top", "refused"),
    ("peer_a", "peer_b", "refused"),
    ("via_root", "top", "refused"),
    ("via_relay", "top", "refused"),
    ("via_relay_plain", "top", "refused"),
    ("via_expr", "top", "refused"),
    ("via_super", "top", "refused"),
    ("via_macro", "top", "refused"),
    ("via_macro_rules", "top", "refused"),
    ("via_macro_export", "top", "refused"),
    ("via_alias", "top", "refused"),
    ("via_feature", "top", "refused"),
    ("via_unknown", "crate::Nowhere", "unattributed"),
    ("ghost", "", "undeclared"),
    ("phantom", "", "unknown"),
    ("quiet", "", "duplicate"),
    ("top::inner", "", "nested"),
    ("relay", "", "relay item"),
    ("base", "top", "stale exception"),
    ("via_bad_exit", "top", "bad exit"),
    ("via_missing_adr", "top", "missing adr"),
    ("via_bad_date", "top", "bad date"),
    ("nowhere", "top", "exception module"),
    ("low", "wild_a", "refused"),
];

/// The ADR files the self-test's exceptions may cite.
const SELF_TEST_ADRS: [&str; 1] = ["ADR-0001-self-test.md"];

/// The findings over one planted crate.
fn planted_findings(planted: &Planted) -> anyhow::Result<Vec<Finding>> {
    let sources = planted
        .files
        .iter()
        .fold(Sources::default(), |sources, (rel, text)| {
            sources.with(&format!("{}/src/{rel}", planted.name), text)
        });
    let scan = source::scan(&sources, &format!("{}/src/lib.rs", planted.name))?;
    let declaration = declaration_from_toml(planted.declaration)?;
    let adrs = SELF_TEST_ADRS.map(str::to_owned);
    Ok(check(&declaration, &scan, &adrs).findings)
}

/// `(missed, false positives)` of the rule over the self-test crates.
fn self_test_diff() -> anyhow::Result<(Vec<Identity>, Vec<Identity>)> {
    let mut seen = Vec::new();
    for planted in &PLANTED {
        seen.extend(planted_findings(planted)?.iter().map(Finding::identity));
    }
    let expected = EXPECTED
        .iter()
        .map(|&(from, to, kind)| (from.to_owned(), to.to_owned(), kind))
        .collect();
    Ok(multiset_diff(expected, seen))
}

/// `(expected - seen, seen - expected)` counting repeats: a finding reported
/// twice where it is planted once is a false positive.
fn multiset_diff(expected: Vec<Identity>, seen: Vec<Identity>) -> (Vec<Identity>, Vec<Identity>) {
    let mut balance: BTreeMap<Identity, isize> = BTreeMap::new();
    for identity in expected {
        *balance.entry(identity).or_default() += 1;
    }
    for identity in seen {
        *balance.entry(identity).or_default() -= 1;
    }
    let (mut missed, mut extra) = (Vec::new(), Vec::new());
    for (identity, count) in balance {
        let side = if count > 0 { &mut missed } else { &mut extra };
        side.extend(std::iter::repeat_n(identity, count.unsigned_abs()));
    }
    (missed, extra)
}

/// `cargo xtask module-dag --self-test`.
fn self_test() -> ExitCode {
    let (missed, extra) = match self_test_diff() {
        Ok(diff) => diff,
        Err(error) => {
            println!("self-test: the planted crates do not scan: {error:#}");
            return ExitCode::FAILURE;
        }
    };
    for (from, to, kind) in &missed {
        println!("self-test: MISSED ({from}, {to}, {kind})");
    }
    for (from, to, kind) in &extra {
        println!("self-test: FALSE POSITIVE ({from}, {to}, {kind})");
    }
    if !missed.is_empty() || !extra.is_empty() {
        return ExitCode::FAILURE;
    }
    println!(
        "module-dag: self-test ok ({} expected findings, no others)",
        EXPECTED.len()
    );
    ExitCode::SUCCESS
}
