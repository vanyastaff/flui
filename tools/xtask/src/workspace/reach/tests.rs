//! Synthetic workspaces go through the same `cargo metadata` document the
//! real run reads ([`Fixture::metadata`]); the last three tests read this
//! repository.

use std::collections::BTreeSet;
use std::process::Command;

use serde_json::json;

use super::fixture::{Dep, Fixture};
use super::resolve::{Graph, Selection, resolve};
use super::rules::Rules;
use super::{
    COMBOS, Expect, FACTS, Fact, Finding, Report, Staleness, check_metadata, evaluate,
    self_test_diff,
};
use crate::util;
use crate::workspace::{Members, Warrant, tiers};

/// The standard rules and a facade with nothing but its defaults, so the
/// only roots that matter are the members'.
fn base() -> Fixture {
    Fixture::standard()
        .member("flui", "H", &json!(null))
        .feature("flui", "default", &[])
}

fn report(fixture: &Fixture, facts: &[Fact]) -> anyhow::Result<Report> {
    check_metadata(&Fixture::root(), &fixture.metadata(), facts, &[""])
}

fn findings(fixture: &Fixture) -> Vec<Finding> {
    report(fixture, &[]).expect("the check runs").findings
}

/// `(package, forbidden)` of each `Reaches` finding.
fn reaches(fixture: &Fixture) -> Vec<(String, String)> {
    findings(fixture)
        .into_iter()
        .map(|finding| match finding {
            Finding::Reaches {
                package, forbidden, ..
            } => (package, forbidden),
            other => panic!("expected only reach findings, got {other}"),
        })
        .collect()
}

fn pair(package: &str, forbidden: &str) -> (String, String) {
    (package.to_owned(), forbidden.to_owned())
}

/// The package names `root` builds under `selection`.
fn names(fixture: &Fixture, root: &str, selection: &str) -> BTreeSet<String> {
    let graph = Graph::from_metadata(&fixture.metadata()).expect("the graph joins");
    let build = resolve(
        &graph,
        graph.member(root).expect("a member"),
        &Selection::parse(selection).expect("a selection"),
    )
    .expect("resolves");
    build
        .reached
        .iter()
        .map(|&at| graph.name(at).to_owned())
        .collect()
}

#[test]
fn a_k_crate_that_reaches_winit_is_reported() {
    let fixture = base()
        .member("k", "K", &json!(null))
        .external("p")
        .external("winit")
        .dep("k", "p", Dep::normal())
        .dep("p", "winit", Dep::normal());
    let found = findings(&fixture);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        found[0].to_string(),
        "k (tier K) reaches winit under `k`: k -> p -> winit (forbidden in tier K, ADR-0081 §2)"
    );
    // the same crate one tier up is a host, which may reach anything
    let fixture =
        base()
            .member("k", "H", &json!(null))
            .external("winit")
            .dep("k", "winit", Dep::normal());
    assert_eq!(findings(&fixture), []);
}

#[test]
fn a_dev_dependency_reaches_nothing() {
    let fixture =
        base()
            .member("k", "K", &json!(null))
            .external("winit")
            .dep("k", "winit", Dep::dev());
    assert_eq!(findings(&fixture), []);
    assert!(!names(&fixture, "k", "--all-features").contains("winit"));
}

#[test]
fn an_optional_dependency_reaches_only_under_a_feature_that_enables_it() {
    let fixture = base()
        .member("k", "K", &json!(null))
        .external("winit")
        .feature("k", "default", &[])
        .feature("k", "win", &["dep:winit"])
        .dep("k", "winit", Dep::normal().optional());
    assert!(!names(&fixture, "k", "").contains("winit"));
    assert!(names(&fixture, "k", "--features win").contains("winit"));
    // the check's own roots include `k --all-features`
    let found = findings(&fixture);
    assert!(
        matches!(&found[..], [Finding::Reaches { root, .. }] if root == "k --all-features"),
        "{found:#?}"
    );
}

#[test]
fn a_weak_feature_does_not_activate_its_dependency() {
    let fixture = base()
        .member("k", "K", &json!(null))
        .external("x")
        .feature("x", "g", &[])
        .feature("k", "f", &["x?/g"])
        .feature("k", "use-x", &["dep:x"])
        .dep("k", "x", Dep::normal().optional());
    assert!(!names(&fixture, "k", "--features f").contains("x"));
    assert!(names(&fixture, "k", "--features f,use-x").contains("x"));

    // once activated, the weak feature is forwarded
    let graph = Graph::from_metadata(&fixture.metadata()).expect("the graph joins");
    let build = resolve(
        &graph,
        graph.member("k").expect("a member"),
        &Selection::parse("--features f,use-x").expect("a selection"),
    )
    .expect("resolves");
    let x = graph.named("x")[0];
    assert!(build.features[x].contains("g"), "{:?}", build.features[x]);

    // the strong form activates the dependency and forwards the feature
    let strong = fixture.feature("k", "s", &["x/g"]);
    let graph = Graph::from_metadata(&strong.metadata()).expect("the graph joins");
    let build = resolve(
        &graph,
        graph.member("k").expect("a member"),
        &Selection::parse("--features s").expect("a selection"),
    )
    .expect("resolves");
    let x = graph.named("x")[0];
    assert!(build.reached.contains(&x));
    assert!(build.features[x].contains("g"), "{:?}", build.features[x]);
}

#[test]
fn a_strong_feature_enables_the_same_named_feature_whatever_it_lists() {
    // `k/x` is explicit and forwards to `y`; `k/s = ["x/g"]` enables it, so
    // `y` joins with `f`, as cargo resolves it
    let fixture = base()
        .member("k", "K", &json!(null))
        .external("x")
        .external("y")
        .feature("x", "g", &[])
        .feature("y", "f", &[])
        .feature("k", "x", &["dep:x", "y/f"])
        .feature("k", "s", &["x/g"])
        .dep("k", "x", Dep::normal().optional())
        .dep("k", "y", Dep::normal().optional());
    let graph = Graph::from_metadata(&fixture.metadata()).expect("the graph joins");
    let build = resolve(
        &graph,
        graph.member("k").expect("a member"),
        &Selection::parse("--features s").expect("a selection"),
    )
    .expect("resolves");
    let y = graph.named("y")[0];
    assert!(build.reached.contains(&y), "y joins through k/x");
    assert!(build.features[y].contains("f"), "{:?}", build.features[y]);
    assert!(
        build.features[graph.member("k").expect("a member")].contains("x"),
        "k/x is enabled"
    );
}

#[test]
fn a_dependency_is_matched_by_package_name_not_library_name() {
    // the resolve graph names `xml-rs` by its library, `xml`
    let fixture = base()
        .member(
            "k",
            "K",
            &json!({ "reach-forbid": ["xml-rs", "renamed-rs"] }),
        )
        .external_version("xml-rs", "0.8.0", "xml")
        .external_version("renamed-rs", "1.0.0", "renamed")
        .feature("k", "default", &["dep:alias"])
        .dep("k", "xml-rs", Dep::normal())
        .dep("k", "renamed-rs", Dep::normal().optional().renamed("alias"));
    assert_eq!(
        reaches(&fixture),
        [pair("k", "renamed-rs"), pair("k", "xml-rs")]
    );
    // two versions of one package: each declaration joins its own
    let fixture = base()
        .member("k", "K", &json!(null))
        .member("h", "H", &json!(null))
        .external_version("windows", "0.58.0", "windows")
        .external_version("windows", "0.62.0", "windows")
        .external("inner")
        .dep("windows@0.58.0", "inner", Dep::normal())
        .dep("h", "windows@0.58.0", Dep::normal())
        .dep("h", "windows@0.62.0", Dep::normal().renamed("windows62"))
        .dep("k", "windows@0.62.0", Dep::normal());
    let graph = Graph::from_metadata(&fixture.metadata()).expect("the graph joins");
    let k = &graph.packages[graph.member("k").expect("a member")];
    assert_eq!(k.deps.len(), 1);
    assert!(graph.packages[k.deps[0].to].deps.is_empty(), "k joins 0.62");
    assert!(!names(&fixture, "k", "").contains("inner"));
    assert!(names(&fixture, "h", "").contains("inner"));
}

#[test]
fn a_target_specific_dependency_counts_on_every_target() {
    let fixture = base()
        .member("k", "K", &json!(null))
        .external("windows")
        .dep("k", "windows", Dep::normal().target("cfg(windows)"));
    assert_eq!(reaches(&fixture), [pair("k", "windows")]);
    // one key declared for two targets is one dependency: `dep:` activates
    // both declarations, and each brings its own features
    let fixture = base()
        .member("k", "K", &json!(null))
        .external("winit")
        .feature("winit", "x11", &[])
        .feature("k", "default", &["dep:winit"])
        .dep(
            "k",
            "winit",
            Dep::normal().optional().target("cfg(windows)"),
        )
        .dep(
            "k",
            "winit",
            Dep::normal()
                .optional()
                .target("cfg(unix)")
                .features(&["x11"]),
        );
    let graph = Graph::from_metadata(&fixture.metadata()).expect("the graph joins");
    let build = resolve(
        &graph,
        graph.member("k").expect("a member"),
        &Selection::parse("").expect("a selection"),
    )
    .expect("resolves");
    let winit = graph.named("winit")[0];
    assert!(build.reached.contains(&winit));
    assert!(
        build.features[winit].contains("x11"),
        "{:?}",
        build.features[winit]
    );
}

#[test]
fn a_build_dependency_reaches() {
    let fixture =
        base()
            .member("k", "K", &json!(null))
            .external("winit")
            .dep("k", "winit", Dep::build());
    assert_eq!(reaches(&fixture), [pair("k", "winit")]);
}

#[test]
fn features_combine_within_one_root_and_not_across_roots() {
    let fixture = base()
        .member("app", "H", &json!(null))
        .member("hr", "pkg", &json!(null))
        .example("host")
        .feature("app", "default", &[])
        .feature("app", "hot-reload", &["dep:hr"])
        .dep("app", "hr", Dep::normal().optional())
        .dep("host", "app", Dep::normal().features(&["hot-reload"]));
    // the host's selection of `hot-reload` does not leak into app's own root
    assert!(names(&fixture, "host", "").contains("hr"));
    assert!(!names(&fixture, "app", "").contains("hr"));
    // and a dependency with default features off keeps them off
    let fixture = base()
        .member("k", "K", &json!(null))
        .external("x")
        .external("winit")
        .feature("x", "default", &["dep:winit"])
        .dep("x", "winit", Dep::normal().optional())
        .dep("k", "x", Dep::normal().no_default_features());
    assert!(!names(&fixture, "k", "").contains("winit"));
}

#[test]
fn selection_parses_every_facade_combo() {
    for combo in COMBOS {
        let selection = Selection::parse(combo).expect("a facade combo parses");
        assert_eq!(selection.to_string(), combo, "round trip");
    }
    assert_eq!(
        Selection::parse("--features a,b").expect("parses"),
        Selection::Defaults(vec!["a".to_owned(), "b".to_owned()])
    );
    for bad in [
        "--no-such-flag",
        "--features",
        "--all-features --no-default-features",
    ] {
        assert!(Selection::parse(bad).is_err(), "{bad}");
    }
}

fn rules(reach: serde_json::Value) -> anyhow::Result<Rules> {
    let tiers: Vec<String> = ["V", "C", "S", "R", "K", "H", "pkg"]
        .map(str::to_owned)
        .into();
    Rules::from_workspace_metadata(&json!({ "flui": { "reach": reach } }), &tiers)
}

#[test]
fn a_generic_ffi_crate_matches_no_glob() {
    let rules = rules(json!({
        "generic-ffi": [{ "name": "windows-sys", "reason": "raw bindings" }],
        "tier": { "pkg": { "forbid": ["windows-*", "windows"] } },
    }))
    .expect("rules parse");
    assert!(rules.forbidden("pkg", &[], "windows-core"));
    assert!(rules.forbidden("pkg", &[], "windows"));
    assert!(!rules.forbidden("pkg", &[], "windows-sys"));
    assert!(!rules.forbidden("pkg", &[], "window"));
    assert!(!rules.forbidden("H", &[], "windows"));
}

#[test]
fn an_exact_forbid_entry_naming_a_generic_ffi_crate_is_an_error() {
    let error = rules(json!({
        "generic-ffi": [{ "name": "windows_*", "reason": "import libraries" }],
        "tier": { "K": { "forbid": ["windows_x86_64_msvc"] } },
    }))
    .expect_err("refused");
    assert!(error.to_string().contains("generic-ffi"), "{error}");
    let ok = rules(json!({
        "generic-ffi": [{ "name": "jni", "reason": "trust store" }],
    }))
    .expect("rules parse");
    let error = ok
        .patterns(&["jni".to_owned()], "k")
        .expect_err("a reach-forbid is refused too");
    assert!(error.to_string().contains("k's `reach-forbid`"), "{error}");
}

#[test]
fn the_r_tier_admits_wgpu_and_the_v_tier_refuses_tokio() {
    let rules = rules(json!({
        "tier": {
            "K": { "forbid": ["winit", "wgpu"] },
            "S": { "extends": "K" },
            "V": { "extends": "S", "forbid": ["tokio"] },
            "R": { "extends": "K", "except": ["wgpu"] },
        },
    }))
    .expect("rules parse");
    assert!(!rules.forbidden("R", &[], "wgpu"));
    assert!(rules.forbidden("R", &[], "winit"));
    assert!(rules.forbidden("K", &[], "wgpu"));
    assert!(rules.forbidden("V", &[], "tokio"));
    assert!(rules.forbidden("V", &[], "wgpu"));
    assert!(!rules.forbidden("S", &[], "tokio"));
}

#[test]
fn an_extends_cycle_or_unknown_tier_is_an_error() {
    let cases = [
        (
            json!({ "tier": { "K": { "extends": "S" }, "S": { "extends": "K" } } }),
            "cycle",
        ),
        (
            json!({ "tier": { "Q": { "forbid": ["x"] } } }),
            "names no tier",
        ),
        (
            json!({ "tier": { "K": { "extends": "H" } } }),
            "has no `reach.tier` table",
        ),
        (
            json!({ "tier": { "K": { "forbid": ["x"] }, "R": { "extends": "K", "except": ["y"] } } }),
            "does not inherit",
        ),
        (
            json!({ "tier": { "K": { "forbids": ["x"] } } }),
            "unknown key",
        ),
        (json!({ "generic_ffi": [] }), "unknown key"),
    ];
    for (reach, needle) in cases {
        let error = rules(reach.clone()).expect_err("refused");
        assert!(format!("{error:#}").contains(needle), "{reach}: {error:#}");
    }
}

#[test]
fn reach_forbid_adds_to_the_tier_set() {
    let fixture = base()
        .member("r", "R", &json!({ "reach-forbid": ["wgpu"] }))
        .member("r2", "R", &json!(null))
        .external("wgpu")
        .dep("r", "wgpu", Dep::normal())
        .dep("r2", "wgpu", Dep::normal());
    let found = findings(&fixture);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(
        found[0]
            .to_string()
            .ends_with("(forbidden by its `reach-forbid`, ADR-0081 §2)"),
        "{}",
        found[0]
    );
}

/// `k -> s -> flui-platform -> winit`, with `s` declaring `exception`.
fn through_s(exception: &serde_json::Value) -> Fixture {
    base()
        .member("k", "K", &json!(null))
        .member("s", "S", exception)
        .member("flui-platform", "H", &json!(null))
        .external("winit")
        .dep("k", "s", Dep::normal())
        .dep("s", "flui-platform", Dep::normal())
        .dep("flui-platform", "winit", Dep::normal())
}

fn exception(to: &str) -> serde_json::Value {
    json!({ "reach-exceptions": [{ "to": to, "exit": "ADR-0082", "reason": "test" }] })
}

#[test]
fn a_reach_exception_excuses_only_paths_through_its_crate() {
    let without = through_s(&json!(null));
    assert_eq!(
        reaches(&without),
        [
            pair("k", "flui-platform"),
            pair("k", "winit"),
            pair("s", "flui-platform"),
            pair("s", "winit"),
        ]
    );
    let with = through_s(&exception("flui-platform"));
    assert_eq!(findings(&with), []);
    // a direct edge is still reported, and so is a second way around
    let direct = through_s(&exception("flui-platform"))
        .member("k2", "K", &json!(null))
        .dep("k2", "flui-platform", Dep::normal())
        .dep("k", "flui-platform", Dep::normal());
    let found = findings(&direct);
    assert_eq!(
        found
            .iter()
            .map(|finding| match finding {
                Finding::Reaches {
                    package,
                    forbidden,
                    path,
                    ..
                } => format!("{package}:{forbidden}:{}", path.join(">")),
                other => other.to_string(),
            })
            .collect::<Vec<_>>(),
        [
            "k:flui-platform:k>flui-platform",
            "k:winit:k>flui-platform>winit",
            "k2:flui-platform:k2>flui-platform",
            "k2:winit:k2>flui-platform>winit",
        ]
    );
}

#[test]
fn a_reach_exception_whose_edge_is_gone_is_stale() {
    let fixture = base().member("s", "S", &exception("flui-platform"));
    assert_eq!(
        findings(&fixture),
        [Finding::Stale {
            package: "s".to_owned(),
            to: "flui-platform".to_owned(),
            why: Staleness::Absent,
        }]
    );
    assert!(
        findings(&fixture)[0]
            .to_string()
            .contains("reaches no flui-platform in any root build")
    );
}

#[test]
fn a_reach_exception_that_excuses_nothing_is_stale() {
    let fixture = base()
        .member("s", "S", &exception("harmless"))
        .external("harmless")
        .dep("s", "harmless", Dep::normal());
    assert_eq!(
        findings(&fixture),
        [Finding::Stale {
            package: "s".to_owned(),
            to: "harmless".to_owned(),
            why: Staleness::ExcusesNothing,
        }]
    );
    // what lies behind it counts for the crates that reach the declaring
    // one: a host's exception excuses the K crate above it
    let fixture = base()
        .member("k", "K", &json!(null))
        .member("h", "H", &exception("winit"))
        .external("winit")
        .dep("k", "h", Dep::normal())
        .dep("h", "winit", Dep::normal());
    assert_eq!(findings(&fixture), []);
}

#[test]
fn a_reach_exception_needs_exactly_one_of_exit_or_grant() {
    for entry in [
        json!({ "to": "x", "reason": "neither" }),
        json!({ "to": "x", "exit": "ADR-0082", "grant": "ADR-0081", "reason": "both" }),
        json!({ "to": "x", "exit": "ADR-0082" }),
    ] {
        let fixture = base().member("s", "S", &json!({ "reach-exceptions": [entry] }));
        let error = report(&fixture, &[]).expect_err("refused");
        assert!(
            format!("{error:#}").contains("`reach-exceptions` must be a list of"),
            "{error:#}"
        );
    }
    let twice = json!({ "reach-exceptions": [
        { "to": "x", "exit": "ADR-0082", "reason": "one" },
        { "to": "x", "grant": "ADR-0081", "reason": "two" },
    ] });
    let error = report(&base().member("s", "S", &twice), &[]).expect_err("refused");
    assert!(format!("{error:#}").contains("names x twice"), "{error:#}");
    // a grant parses as a grant
    let fixture = base().member(
        "r",
        "R",
        &json!({ "reach-exceptions": [{ "to": "wgpu", "grant": "ADR-0081", "reason": "r" }] }),
    );
    let members = Members::load(&Fixture::root(), &fixture.metadata()).expect("loads");
    let r = members
        .iter()
        .find(|member| member.name() == "r")
        .expect("r");
    assert_eq!(
        r.reach_exceptions[0].warrant,
        Warrant::Grant("ADR-0081".to_owned())
    );
}

/// The real facts' packages: flui-app's edge to flui-hot-reload optional or
/// not, its `hot-reload` feature bringing it in or not, and the host
/// enabling that feature or not.
fn hot_reload(optional: bool, feature_brings_it: bool, host_enables: bool) -> Fixture {
    let hot_reload = if feature_brings_it {
        &["dep:flui-hot-reload"][..]
    } else {
        &[]
    };
    let host = if host_enables {
        Dep::normal().features(&["hot-reload"])
    } else {
        Dep::normal()
    };
    let edge = if optional {
        Dep::normal().optional()
    } else {
        Dep::normal()
    };
    base()
        .member("flui-app", "H", &json!(null))
        .member("flui-hot-reload", "pkg", &json!(null))
        .example("hot-reload-counter-host")
        .feature("flui-app", "default", &[])
        .feature("flui-app", "hot-reload", hot_reload)
        .dep("flui-app", "flui-hot-reload", edge)
        .dep("hot-reload-counter-host", "flui-app", host)
}

#[test]
fn each_fact_reads_the_build_both_ways() {
    let [absent, present, enables] = FACTS;
    assert!(matches!(absent.expect, Expect::Absent(_)));
    assert!(matches!(present.expect, Expect::Present(_)));
    assert!(matches!(enables.expect, Expect::Enables(..)));
    let holds = |fixture: &Fixture, fact: &Fact| {
        let graph = Graph::from_metadata(&fixture.metadata()).expect("the graph joins");
        evaluate(&graph, fact).expect("evaluates").is_none()
    };

    let good = hot_reload(true, true, true);
    for fact in &FACTS {
        assert!(holds(&good, fact), "{}", fact.what);
    }
    assert!(!holds(&hot_reload(false, true, true), &absent));
    assert!(!holds(&hot_reload(true, false, true), &present));
    assert!(!holds(&hot_reload(true, true, false), &enables));
    let failed = evaluate(
        &Graph::from_metadata(&hot_reload(false, true, true).metadata()).expect("joins"),
        &absent,
    )
    .expect("evaluates")
    .expect("fails");
    assert_eq!(
        failed.to_string(),
        "flui-hot-reload must be absent from flui-app's default graph: flui-hot-reload is in \
         flui-app's default normal dependency graph (under `flui-app`)"
    );

    // an unknown root, package or feature is an error, not a pass
    let graph = Graph::from_metadata(&good.metadata()).expect("joins");
    for fact in [
        Fact {
            root: "no-such-root",
            ..absent
        },
        Fact {
            expect: Expect::Absent("no-such-package"),
            ..absent
        },
        Fact {
            expect: Expect::Enables("flui-app", "no-such-feature"),
            ..enables
        },
    ] {
        assert!(evaluate(&graph, &fact).is_err(), "{fact:?}");
    }
}

#[test]
fn the_self_test_reports_exactly_the_planted_findings() {
    let (missed, extra) = self_test_diff().expect("the self-test workspace checks");
    assert!(
        missed.is_empty() && extra.is_empty(),
        "missed {missed:#?}, false positives {extra:#?}"
    );
}

/// `(tier, forbidden set)` of the root manifest, and the rules.
fn real_rules() -> (Vec<String>, Rules) {
    let metadata = util::metadata(&util::repo_root()).expect("cargo metadata on the repository");
    let tiers = tiers::names(&metadata.workspace_metadata).expect("tier names");
    let rules =
        Rules::from_workspace_metadata(&metadata.workspace_metadata, &tiers).expect("rules parse");
    (tiers, rules)
}

#[test]
fn the_forbid_sets_match_the_adr_0081_table() {
    let (tiers, rules) = real_rules();
    let k = [
        "flui-platform",
        "winit",
        "android-activity",
        "ndk",
        "windows",
        "objc2-app-kit",
        "objc2-ui-kit",
        "wgpu",
        "flui-engine",
        "flui-app",
    ];
    let os = [
        "windows-*",
        "objc2-*",
        "core-foundation*",
        "core-graphics*",
        "jni-*",
        "ndk-*",
        "android-*",
        "android_*",
    ];
    let set = |names: &[&str]| -> BTreeSet<String> {
        names.iter().map(|&name| name.to_owned()).collect()
    };
    let without_wgpu: Vec<&str> = k.into_iter().filter(|&name| name != "wgpu").collect();
    let expected = [
        ("V", set(&[&k[..], &["tokio"]].concat())),
        ("C", set(&k)),
        ("S", set(&k)),
        ("R", set(&without_wgpu)),
        ("K", set(&k)),
        ("H", set(&[])),
        ("pkg", set(&[&k[..], &os[..]].concat())),
    ];
    assert_eq!(
        tiers,
        expected
            .iter()
            .map(|(tier, _)| (*tier).to_owned())
            .collect::<Vec<_>>()
    );
    for (tier, names) in expected {
        let actual: BTreeSet<String> = rules
            .tier_set(tier)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(actual, names, "tier {tier}");
    }
    // the generic FFI crates ADR-0081 §2 names, each with its reason
    for name in ["windows-sys", "jni", "objc2", "core-foundation"] {
        let reason = rules
            .generic_ffi()
            .iter()
            .find(|(pattern, _)| pattern.to_string() == name)
            .map(|(_, reason)| reason.clone())
            .unwrap_or_default();
        assert!(
            !reason.trim().is_empty(),
            "{name} is allowlisted with a reason"
        );
    }
}

#[test]
fn the_seeded_reach_exceptions_are_the_known_debt() {
    let metadata = util::metadata(&util::repo_root()).expect("cargo metadata on the repository");
    let members = Members::load(&util::repo_root(), &metadata).expect("manifests load");
    let seeded: BTreeSet<(String, String, Warrant)> = members
        .iter()
        .flat_map(|member| {
            member.reach_exceptions.iter().map(move |entry| {
                (
                    member.name().to_owned(),
                    entry.to.clone(),
                    entry.warrant.clone(),
                )
            })
        })
        .collect();
    let exit = |adr: &str| Warrant::Exit(adr.to_owned());
    let expected: BTreeSet<(String, String, Warrant)> = [
        ("flui-interaction", "flui-platform", exit("ADR-0082")),
        ("flui-widgets", "flui-platform", exit("ADR-0082")),
        ("flui-engine", "wgpu", Warrant::Grant("ADR-0081".to_owned())),
        ("flui-hot-reload", "windows", exit("ADR-0094")),
        ("flui-hot-reload", "android_log-sys", exit("ADR-0094")),
    ]
    .into_iter()
    .map(|(package, to, warrant)| (package.to_owned(), to.to_owned(), warrant))
    .collect();
    assert_eq!(seeded, expected);
}

/// The resolver builds exactly what `cargo tree` prints for the same root,
/// on every target, over normal and build edges.
#[test]
fn the_resolver_agrees_with_cargo_tree() {
    let root = util::repo_root();
    let metadata = util::resolved_metadata(&root).expect("cargo metadata --all-features");
    let graph = Graph::from_metadata(&metadata).expect("the graph joins");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    for (package, selection) in [
        ("flui", "--no-default-features"),
        ("flui", ""),
        ("flui", "--all-features"),
        ("flui-widgets", "--all-features"),
        ("flui-app", ""),
    ] {
        let output = Command::new(&cargo)
            .current_dir(&root)
            .args(["tree", "-p", package])
            .args(selection.split_whitespace())
            .args(["-e", "normal,build", "--target", "all"])
            .args(["--prefix", "none", "-f", "{p}", "--locked"])
            .output()
            .expect("cargo tree runs");
        assert!(
            output.status.success(),
            "cargo tree: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let tree: BTreeSet<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .map(str::to_owned)
            .collect();
        let build = resolve(
            &graph,
            graph.member(package).expect("a member"),
            &Selection::parse(selection).expect("a selection"),
        )
        .expect("resolves");
        let ours: BTreeSet<String> = build
            .reached
            .iter()
            .map(|&at| graph.name(at).to_owned())
            .collect();
        assert_eq!(
            ours.symmetric_difference(&tree).collect::<Vec<_>>(),
            Vec::<&String>::new(),
            "`{package} {selection}`: {} ours, {} cargo tree",
            ours.len(),
            tree.len()
        );
    }
}
