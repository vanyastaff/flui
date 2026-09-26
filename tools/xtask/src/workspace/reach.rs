//! `cargo xtask reach`: no crate reaches what its tier forbids (ADR-0081 §2).
//!
//! - **Roots.** Each is resolved on its own, as `cargo build -p <root>
//!   <selection>` resolves it on every target ([`mod@resolve`]): the facade under
//!   each of [`COMBOS`] and under each of its features alone, and every
//!   member with a tier at its defaults and with `--all-features`.
//! - **Subjects.** Every workspace package with a tier that a root build
//!   reaches. Its closure is taken over that build's activated normal and
//!   build edges; dev edges reach nothing.
//! - **Forbidden.** The tier's set from the root manifest
//!   ([`rules`]), plus the package's own `reach-forbid`. A package in the
//!   closure whose name matches is a finding, witnessed by the first root that
//!   shows it and the path it takes.
//! - **Exceptions.** A `reach-exceptions` entry `{ to, exit | grant, reason }`
//!   on package E excuses every path that passes through E and then enters a
//!   package named `to`; a path that reaches `to` any other way is still
//!   reported. `exit` names the ADR that removes the path, `grant` the ADR
//!   that permits it for good. An entry is stale, and a finding, when E
//!   reaches no `to` in any root build, or when nothing behind `to` is
//!   forbidden to E or to a crate that reaches E in any root build: the list
//!   only shrinks.
//! - **Facts.** [`FACTS`]: statements about one root's build, such as hot
//!   reload's absence from an ordinary production graph.
//!
//! The graph is `cargo metadata --locked --all-features`, not its resolution:
//! Cargo resolves features once for the whole workspace, so its own graph
//! puts the facade's defaults and `flui-app`'s `hot-reload` into every root.
//!
//! `--self-test` runs the check over a built-in workspace with planted
//! violations and fails unless it reports exactly those (ADR-0078 §4).

mod fixture;
mod resolve;
mod rules;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fmt;
use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, ensure};
use cargo_metadata::Metadata;
use serde_json::json;

use self::fixture::{Dep, Fixture};
use self::resolve::{Build, Graph, Selection, resolve};
use self::rules::{Pattern, Rules};
use super::{Members, ReachException, tiers};
use crate::tasks::facade::COMBOS;
use crate::util;

/// Arguments for `cargo xtask reach`.
#[derive(Debug, clap::Args)]
pub(crate) struct ReachArgs {
    /// Run the check over a built-in workspace with planted violations
    /// instead of this one; exit 1 unless it reports exactly those.
    #[arg(long)]
    self_test: bool,
}

/// `cargo xtask reach`. Exit code 1 lists every finding.
pub(crate) fn reach(args: &ReachArgs) -> anyhow::Result<ExitCode> {
    if args.self_test {
        return Ok(self_test());
    }
    let root = util::repo_root();
    let metadata = util::resolved_metadata(&root)?;
    let report = check_metadata(&root, &metadata, &FACTS, &COMBOS)?;
    if report.findings.is_empty() {
        println!(
            "reach: {} root builds ({} distinct), {} facts, no crate reaches what its tier \
             forbids",
            report.roots,
            report.builds,
            FACTS.len()
        );
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!("reach: {} problem(s)", report.findings.len());
    for finding in &report.findings {
        eprintln!("  - {finding}");
    }
    Ok(ExitCode::FAILURE)
}

/// The facade: its roots are [`COMBOS`] and each of its features alone.
const FACADE: &str = "flui";

/// A root build: a package and the features asked of it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Root {
    package: String,
    selection: Selection,
}

impl fmt::Display for Root {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let selection = self.selection.to_string();
        if selection.is_empty() {
            f.write_str(&self.package)
        } else {
            write!(f, "{} {selection}", self.package)
        }
    }
}

/// The root builds, in the order findings are witnessed: the facade under
/// `combos`, then under each feature no single-feature combo already names,
/// then every member with a tier at its defaults and with `--all-features`.
fn roots(graph: &Graph, members: &Members, combos: &[&str]) -> anyhow::Result<Vec<Root>> {
    let mut roots: Vec<Root> = Vec::new();
    let mut push = |root: Root| {
        if !roots.contains(&root) {
            roots.push(root);
        }
    };
    for combo in combos {
        push(Root {
            package: FACADE.to_owned(),
            selection: Selection::parse(combo)?,
        });
    }
    let facade = &graph.packages[graph.member(FACADE)?];
    for feature in facade
        .features
        .keys()
        .filter(|feature| *feature != "default")
    {
        push(Root {
            package: FACADE.to_owned(),
            selection: Selection::NoDefaults(vec![feature.clone()]),
        });
    }
    let mut tiered: Vec<&str> = members
        .iter()
        .filter(|member| member.tier.is_some())
        .map(super::Member::name)
        .collect();
    tiered.sort_unstable();
    for name in tiered {
        for selection in [Selection::Defaults(Vec::new()), Selection::All] {
            push(Root {
                package: name.to_owned(),
                selection,
            });
        }
    }
    Ok(roots)
}

/// A statement about one root's build.
#[derive(Debug, Clone, Copy)]
struct Fact {
    /// What is being established.
    what: &'static str,
    /// The root package, a workspace member.
    root: &'static str,
    /// Its feature selection, as on a cargo command line.
    selection: &'static str,
    expect: Expect,
    /// The failure, when the build says otherwise.
    failure: &'static str,
}

#[derive(Debug, Clone, Copy)]
enum Expect {
    /// No package of this name is in the build.
    Absent(&'static str),
    /// A package of this name is in the build.
    Present(&'static str),
    /// A package of this name is in the build with this feature on.
    Enables(&'static str, &'static str),
}

/// Hot reload must be absent from an ordinary production graph, not merely
/// unused by it; the feature must bring it in; and the first-party host, the
/// executable contract for `flui run`, must enable flui-app's feature (a
/// direct dependency on flui-hot-reload does not). The train guard,
/// `flui-foundation`, must be in the SDK's build and in the facade's build
/// with no features, so a package on one and an application on the other
/// share it (ADR-0088 §5).
const FACTS: [Fact; 5] = [
    Fact {
        what: "flui-hot-reload must be absent from flui-app's default graph",
        root: "flui-app",
        selection: "",
        expect: Expect::Absent("flui-hot-reload"),
        failure: "flui-hot-reload is in flui-app's default normal dependency graph",
    },
    Fact {
        what: "the hot-reload feature must bring in flui-hot-reload",
        root: "flui-app",
        selection: "--features hot-reload",
        expect: Expect::Present("flui-hot-reload"),
        failure: "the hot-reload feature did not bring in flui-hot-reload",
    },
    Fact {
        what: "hot-reload-counter-host must enable flui-app/hot-reload",
        root: "hot-reload-counter-host",
        selection: "",
        expect: Expect::Enables("flui-app", "hot-reload"),
        failure: "hot-reload-counter-host does not enable flui-app/hot-reload",
    },
    Fact {
        what: "the train guard must be in flui-sdk's build",
        root: "flui-sdk",
        selection: "",
        expect: Expect::Present("flui-foundation"),
        failure: "flui-foundation is not in flui-sdk's normal dependency graph",
    },
    Fact {
        what: "the train guard must be in the facade's build without features",
        root: "flui",
        selection: "--no-default-features",
        expect: Expect::Present("flui-foundation"),
        failure: "flui-foundation is not in the facade's normal dependency graph",
    },
];

/// Why a `reach-exceptions` entry is stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Staleness {
    /// The declaring package reaches no package of that name.
    Absent,
    /// Nothing behind it is forbidden to anyone the entry could excuse.
    ExcusesNothing,
}

/// One problem `reach` reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Finding {
    /// A package reaches a name forbidden to it.
    Reaches {
        package: String,
        tier: String,
        forbidden: String,
        /// Forbidden by the package's own `reach-forbid`, not its tier.
        own: bool,
        /// The first root build that shows it.
        root: String,
        path: Vec<String>,
    },
    /// A `reach-exceptions` entry that excuses nothing.
    Stale {
        package: String,
        to: String,
        why: Staleness,
    },
    /// A fact that does not hold.
    Fact {
        what: &'static str,
        root: String,
        failure: &'static str,
    },
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reaches {
                package,
                tier,
                forbidden,
                own,
                root,
                path,
            } => {
                write!(
                    f,
                    "{package} (tier {tier}) reaches {forbidden} under `{root}`: {} ",
                    path.join(" -> ")
                )?;
                if *own {
                    write!(f, "(forbidden by its `reach-forbid`, ADR-0081 §2)")
                } else {
                    write!(f, "(forbidden in tier {tier}, ADR-0081 §2)")
                }
            }
            Self::Stale {
                package,
                to,
                why: Staleness::Absent,
            } => write!(
                f,
                "{package} lists `reach-exceptions` for {to}, but reaches no {to} in any root \
                 build; remove the entry"
            ),
            Self::Stale {
                package,
                to,
                why: Staleness::ExcusesNothing,
            } => write!(
                f,
                "{package} lists `reach-exceptions` for {to}, but nothing behind {to} is \
                 forbidden to {package} or to a crate that reaches it in any root build; remove \
                 the entry"
            ),
            Self::Fact {
                what,
                root,
                failure,
            } => write!(f, "{what}: {failure} (under `{root}`)"),
        }
    }
}

/// The findings, and what was checked.
#[derive(Debug)]
struct Report {
    findings: Vec<Finding>,
    roots: usize,
    builds: usize,
}

/// Loads the members, rules and graph from `metadata` and runs [`check`].
fn check_metadata(
    root: &Path,
    metadata: &Metadata,
    facts: &[Fact],
    combos: &[&str],
) -> anyhow::Result<Report> {
    let members = Members::load(root, metadata)?;
    let tiers = tiers::names(&metadata.workspace_metadata)?;
    let rules = Rules::from_workspace_metadata(&metadata.workspace_metadata, &tiers)?;
    let graph = Graph::from_metadata(metadata)?;
    check(&graph, &members, &rules, facts, combos)
}

/// One `reach-exceptions` entry, with the package that declares it.
struct Exception<'m> {
    declared_by: &'m str,
    entry: &'m ReachException,
}

/// The most exceptions the path search tracks, one bit each.
const MAX_EXCEPTIONS: usize = 64;

/// Every finding over `graph`.
fn check(
    graph: &Graph,
    members: &Members,
    rules: &Rules,
    facts: &[Fact],
    combos: &[&str],
) -> anyhow::Result<Report> {
    let by_name = members.by_name();
    let tier_of = |at: usize| -> Option<&str> {
        let package = &graph.packages[at];
        if !package.member {
            return None;
        }
        by_name.get(package.name.as_str())?.tier.as_deref()
    };
    let extras: BTreeMap<&str, Vec<Pattern>> = members
        .iter()
        .map(|member| {
            Ok((
                member.name(),
                rules.patterns(&member.reach_forbid, member.name())?,
            ))
        })
        .collect::<anyhow::Result<_>>()?;
    let forbidden_to = |at: usize, name: &str| -> bool {
        tier_of(at).is_some_and(|tier| rules.forbidden(tier, &extras[graph.name(at)], name))
    };

    let exceptions: Vec<Exception<'_>> = members
        .iter()
        .flat_map(|member| {
            member.reach_exceptions.iter().map(|entry| Exception {
                declared_by: member.name(),
                entry,
            })
        })
        .collect();
    ensure!(
        exceptions.len() <= MAX_EXCEPTIONS,
        "{} `reach-exceptions` entries; the check tracks at most {MAX_EXCEPTIONS}",
        exceptions.len()
    );
    // bits armed on passing through a package, and bits that block entering
    // a package of a name
    let mut arms = vec![0_u64; graph.packages.len()];
    let mut blocks: HashMap<&str, u64> = HashMap::new();
    for (bit, exception) in exceptions.iter().enumerate() {
        let declared_by = graph.member(exception.declared_by)?;
        arms[declared_by] |= 1 << bit;
        *blocks.entry(exception.entry.to.as_str()).or_default() |= 1 << bit;
    }
    let blocked = |mask: u64, at: usize| {
        blocks
            .get(graph.name(at))
            .is_some_and(|bits| mask & bits != 0)
    };

    let roots = roots(graph, members, combos)?;
    let mut builds: Vec<Build> = Vec::new();
    let mut reported: BTreeMap<(String, String), Finding> = BTreeMap::new();
    let mut edge_seen = vec![false; exceptions.len()];
    let mut excuses = vec![false; exceptions.len()];
    for root in &roots {
        let build = resolve(graph, graph.member(&root.package)?, &root.selection)
            .with_context(|| format!("resolving `{root}`"))?;
        if builds.contains(&build) {
            continue;
        }
        let subjects: Vec<usize> = build
            .reached
            .iter()
            .copied()
            .filter(|&at| tier_of(at).is_some())
            .collect();
        for &subject in &subjects {
            let tier = tier_of(subject).expect("BUG: a subject has a tier");
            for (at, path) in unexcused_paths(&build, subject, &arms, &blocked) {
                let name = graph.name(at);
                if !forbidden_to(subject, name) {
                    continue;
                }
                reported
                    .entry((graph.name(subject).to_owned(), name.to_owned()))
                    .or_insert_with(|| Finding::Reaches {
                        package: graph.name(subject).to_owned(),
                        tier: tier.to_owned(),
                        forbidden: name.to_owned(),
                        own: !rules.tier_forbids(tier, name),
                        root: root.to_string(),
                        path: path.iter().map(|&at| graph.name(at).to_owned()).collect(),
                    });
            }
        }

        let closures: BTreeMap<usize, BTreeSet<usize>> = subjects
            .iter()
            .map(|&subject| (subject, build.closure([subject])))
            .collect();
        for (bit, exception) in exceptions.iter().enumerate() {
            let declared_by = graph.member(exception.declared_by)?;
            if !build.reached.contains(&declared_by) {
                continue;
            }
            let targets: Vec<usize> = build
                .closure([declared_by])
                .into_iter()
                .filter(|&at| at != declared_by && graph.name(at) == exception.entry.to)
                .collect();
            if targets.is_empty() {
                continue;
            }
            edge_seen[bit] = true;
            let behind = build.closure(targets);
            let excusable: Vec<usize> = std::iter::once(declared_by)
                .chain(
                    closures
                        .iter()
                        .filter(|(_, closure)| closure.contains(&declared_by))
                        .map(|(&subject, _)| subject),
                )
                .collect();
            if behind.iter().any(|&at| {
                excusable
                    .iter()
                    .any(|&whose| forbidden_to(whose, graph.name(at)))
            }) {
                excuses[bit] = true;
            }
        }
        builds.push(build);
    }

    let mut findings: Vec<Finding> = reported.into_values().collect();
    for (bit, exception) in exceptions.iter().enumerate() {
        let why = if !edge_seen[bit] {
            Staleness::Absent
        } else if !excuses[bit] {
            Staleness::ExcusesNothing
        } else {
            continue;
        };
        findings.push(Finding::Stale {
            package: exception.declared_by.to_owned(),
            to: exception.entry.to.clone(),
            why,
        });
    }
    for fact in facts {
        if let Some(finding) = evaluate(graph, fact)? {
            findings.push(finding);
        }
    }
    Ok(Report {
        findings,
        roots: roots.len(),
        builds: builds.len(),
    })
}

/// Every package `start`'s closure reaches by a path no exception excuses,
/// each with the first such path found (breadth first, so a shortest one).
///
/// The search runs over (package, armed exceptions): passing through a
/// package arms the exceptions it declares, and entering a package whose
/// name an armed exception names is not a step.
fn unexcused_paths(
    build: &Build,
    start: usize,
    arms: &[u64],
    blocked: &impl Fn(u64, usize) -> bool,
) -> Vec<(usize, Vec<usize>)> {
    type State = (usize, u64);
    let first: State = (start, arms[start]);
    let mut parent: HashMap<State, State> = HashMap::new();
    let mut found: BTreeMap<usize, State> = BTreeMap::new();
    let mut queue: VecDeque<State> = VecDeque::from([first]);
    parent.insert(first, first);
    while let Some((at, mask)) = queue.pop_front() {
        for &next in &build.edges[at] {
            if blocked(mask, next) {
                continue;
            }
            let state = (next, mask | arms[next]);
            if parent.contains_key(&state) {
                continue;
            }
            parent.insert(state, (at, mask));
            found.entry(next).or_insert(state);
            queue.push_back(state);
        }
    }
    found
        .into_iter()
        .filter(|&(at, _)| at != start)
        .map(|(at, mut state)| {
            let mut path = vec![at];
            while state != first {
                state = parent[&state];
                path.push(state.0);
            }
            path.reverse();
            (at, path)
        })
        .collect()
}

/// The finding when `fact` does not hold; an unknown root, package or
/// feature is an error, not a pass.
fn evaluate(graph: &Graph, fact: &Fact) -> anyhow::Result<Option<Finding>> {
    let root = Root {
        package: fact.root.to_owned(),
        selection: Selection::parse(fact.selection)?,
    };
    let build = resolve(graph, graph.member(fact.root)?, &root.selection)
        .with_context(|| format!("resolving `{root}` for a fact"))?;
    let known = |name: &str| -> anyhow::Result<&[usize]> {
        let named = graph.named(name);
        ensure!(
            !named.is_empty(),
            "the fact \"{}\" names {name}, which is in no build",
            fact.what
        );
        Ok(named)
    };
    let holds = match fact.expect {
        Expect::Absent(name) => !known(name)?.iter().any(|at| build.reached.contains(at)),
        Expect::Present(name) => known(name)?.iter().any(|at| build.reached.contains(at)),
        Expect::Enables(name, feature) => {
            let named = known(name)?;
            ensure!(
                named
                    .iter()
                    .any(|&at| graph.packages[at].features.contains_key(feature)),
                "the fact \"{}\" names feature `{feature}`, which {name} does not have",
                fact.what
            );
            named
                .iter()
                .any(|&at| build.reached.contains(&at) && build.features[at].contains(feature))
        }
    };
    Ok((!holds).then(|| Finding::Fact {
        what: fact.what,
        root: root.to_string(),
        failure: fact.failure,
    }))
}

/// `(subject, object, kind)`: what the self-test compares.
type Identity = (String, String, String);

impl Finding {
    fn identity(&self) -> Identity {
        match self {
            Self::Reaches {
                package,
                forbidden,
                root,
                ..
            } => (
                package.clone(),
                forbidden.clone(),
                format!("reaches under `{root}`"),
            ),
            Self::Stale { package, to, why } => {
                (package.clone(), to.clone(), format!("stale: {why:?}"))
            }
            Self::Fact { what, root, .. } => ((*what).to_owned(), root.clone(), "fact".to_owned()),
        }
    }
}

/// The self-test's facade selections.
const SELF_TEST_COMBOS: [&str; 2] = ["--no-default-features", ""];

/// The self-test's facts: one planted to fail, two that hold.
const SELF_TEST_FACTS: [Fact; 3] = [
    Fact {
        what: "winit must be absent from k-plain",
        root: "k-plain",
        selection: "",
        expect: Expect::Absent("winit"),
        failure: "winit is in k-plain's graph",
    },
    Fact {
        what: "k-feat's win feature must bring in winit",
        root: "k-feat",
        selection: "--features win",
        expect: Expect::Present("winit"),
        failure: "the win feature did not bring in winit",
    },
    Fact {
        what: "the facade's harness feature must enable k-feat/win",
        root: "flui",
        selection: "--no-default-features --features harness",
        expect: Expect::Enables("k-feat", "win"),
        failure: "harness does not enable k-feat/win",
    },
];

/// The self-test workspace: the real rule shape, silent paths of every
/// legal kind, and one planted violation per rule.
fn self_test_fixture() -> Fixture {
    let exception = |to: &str| json!({ "reach-exceptions": [{ "to": to, "exit": "ADR-0082", "reason": "self-test" }] });
    Fixture::standard()
        .member("flui", "H", &json!(null))
        .member("flui-platform", "H", &json!(null))
        .member("k-plain", "K", &json!(null))
        .member("k-feat", "K", &json!(null))
        .member("k-dev", "K", &json!(null))
        .member("k-ex", "K", &json!(null))
        .member("k-direct", "K", &json!(null))
        .member("s-int", "S", &exception("flui-platform"))
        // planted: an exception whose edge is gone
        .member("s-gone", "S", &exception("gone"))
        .member("s-idle", "S", &exception("harmless"))
        .member("v1", "V", &json!(null))
        .member("r1", "R", &json!({ "reach-forbid": ["tokio"] }))
        .member("r-layer", "R", &json!(null))
        .member(
            "r-engine",
            "R",
            &json!({ "reach-exceptions": [{ "to": "wgpu", "grant": "ADR-0081", "reason": "self-test" }] }),
        )
        .member("pkg1", "pkg", &json!(null))
        .external("winit")
        .external("tokio")
        .external("wgpu")
        .external("windows-core")
        .external("windows-sys")
        .external("harmless")
        .feature("flui", "default", &[])
        .feature("flui", "harness", &["k-feat/win"])
        .feature("k-feat", "win", &["dep:winit"])
        .dep("flui", "k-plain", Dep::normal())
        .dep("flui", "k-feat", Dep::normal())
        .dep("flui", "k-dev", Dep::normal())
        .dep("flui", "k-ex", Dep::normal())
        .dep("flui", "k-direct", Dep::normal())
        .dep("flui", "s-gone", Dep::normal())
        .dep("flui", "s-idle", Dep::normal())
        .dep("flui", "v1", Dep::normal())
        .dep("flui", "r1", Dep::normal())
        .dep("flui", "r-layer", Dep::normal())
        .dep("flui", "r-engine", Dep::normal())
        .dep("flui", "pkg1", Dep::normal())
        // planted: K -> winit
        .dep("k-plain", "winit", Dep::normal())
        // planted: K -> winit behind a feature only the facade's `harness`
        // selection turns on
        .dep("k-feat", "winit", Dep::normal().optional())
        // silent: a dev edge reaches nothing
        .dep("k-dev", "winit", Dep::dev())
        // silent: the path runs through s-int, whose exception excuses it
        .dep("k-ex", "s-int", Dep::normal())
        .dep("s-int", "flui-platform", Dep::normal())
        // planted: a direct edge, which s-int's exception does not cover
        .dep("k-direct", "flui-platform", Dep::normal())
        // silent: a host reaches winit
        .dep("flui-platform", "winit", Dep::normal())
        // planted: an exception with nothing forbidden behind it
        .dep("s-idle", "harmless", Dep::normal())
        // planted: V -> tokio
        .dep("v1", "tokio", Dep::normal())
        // planted: R admits tokio, but r1's `reach-forbid` adds it
        .dep("r1", "tokio", Dep::normal())
        // planted: an R crate without the grant reaches wgpu
        .dep("r-layer", "wgpu", Dep::normal())
        // silent: r-engine's grant admits wgpu
        .dep("r-engine", "wgpu", Dep::normal())
        // planted: pkg -> a windows crate; silent: pkg -> generic FFI
        .dep("pkg1", "windows-core", Dep::normal())
        .dep("pkg1", "windows-sys", Dep::normal())
}

/// The finding each planted violation must produce, and no other.
const EXPECTED: [(&str, &str, &str); 11] = [
    (
        "k-direct",
        "flui-platform",
        "reaches under `flui --no-default-features`",
    ),
    (
        "k-direct",
        "winit",
        "reaches under `flui --no-default-features`",
    ),
    (
        "k-plain",
        "winit",
        "reaches under `flui --no-default-features`",
    ),
    (
        "k-feat",
        "winit",
        "reaches under `flui --no-default-features --features harness`",
    ),
    ("v1", "tokio", "reaches under `flui --no-default-features`"),
    ("r1", "tokio", "reaches under `flui --no-default-features`"),
    (
        "r-layer",
        "wgpu",
        "reaches under `flui --no-default-features`",
    ),
    (
        "pkg1",
        "windows-core",
        "reaches under `flui --no-default-features`",
    ),
    ("s-gone", "gone", "stale: Absent"),
    ("s-idle", "harmless", "stale: ExcusesNothing"),
    ("winit must be absent from k-plain", "k-plain", "fact"),
];

/// `(missed, false positives)` of the check over the self-test workspace.
pub(super) fn self_test_diff() -> anyhow::Result<(Vec<Identity>, Vec<Identity>)> {
    let report = check_metadata(
        &Fixture::root(),
        &self_test_fixture().metadata(),
        &SELF_TEST_FACTS,
        &SELF_TEST_COMBOS,
    )?;
    let seen: BTreeSet<Identity> = report.findings.iter().map(Finding::identity).collect();
    let expected: BTreeSet<Identity> = EXPECTED
        .iter()
        .map(|&(subject, object, kind)| (subject.to_owned(), object.to_owned(), kind.to_owned()))
        .collect();
    Ok((
        expected.difference(&seen).cloned().collect(),
        seen.difference(&expected).cloned().collect(),
    ))
}

/// `cargo xtask reach --self-test`.
fn self_test() -> ExitCode {
    let (missed, extra) = match self_test_diff() {
        Ok(diff) => diff,
        Err(error) => {
            println!("self-test: the check failed to run: {error:#}");
            return ExitCode::FAILURE;
        }
    };
    for (subject, object, kind) in &missed {
        println!("self-test: MISSED ({subject}, {object}, {kind})");
    }
    for (subject, object, kind) in &extra {
        println!("self-test: FALSE POSITIVE ({subject}, {object}, {kind})");
    }
    if !missed.is_empty() || !extra.is_empty() {
        return ExitCode::FAILURE;
    }
    println!(
        "reach: self-test ok ({} expected findings, no others)",
        EXPECTED.len()
    );
    ExitCode::SUCCESS
}
