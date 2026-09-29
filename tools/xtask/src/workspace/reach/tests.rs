//! Synthetic workspaces go through the same `cargo metadata` document the
//! real run reads ([`Fixture::metadata`]); the last case reads this
//! repository.

use std::collections::BTreeSet;
use std::process::Command;

use serde_json::json;

use super::fixture::{Dep, Fixture};
use super::resolve::{Graph, Selection, resolve};
use super::rules::Rules;
use super::{Expect, FACTS, Fact, Finding, Report, check_metadata, evaluate};
use crate::util;
use crate::workspace::{Members, Warrant};

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

/// Parley's `std = [.., "peniko/std", ..]` names `peniko`, a dev-dependency
/// only: Cargo applies it where dev edges are built, and resolving a root
/// with the feature on is neither an error nor an edge.
fn a_feature_forwarded_to_a_dev_dependency_is_ignored() {
    let fixture = base()
        .member("k", "K", &json!(null))
        .external("winit")
        .feature("k", "std", &["winit/std"])
        .dep("k", "winit", Dep::dev());
    assert_eq!(findings(&fixture), []);
    assert!(!names(&fixture, "k", "--features std").contains("winit"));
}

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

fn an_unknown_feature_is_an_error_not_a_pass() {
    let fixture = base()
        .member("k", "K", &json!(null))
        .external("x")
        .feature("k", "f", &["x/missing"])
        .dep("k", "x", Dep::normal());
    let graph = Graph::from_metadata(&fixture.metadata()).expect("the graph joins");
    let k = graph.member("k").expect("a member");
    for selection in ["--features no-such-feature", "--features f"] {
        let error = resolve(
            &graph,
            k,
            &Selection::parse(selection).expect("a selection"),
        )
        .expect_err("refused");
        assert!(error.to_string().contains("has no feature"), "{error}");
    }
}

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

fn a_build_dependency_reaches() {
    let fixture =
        base()
            .member("k", "K", &json!(null))
            .external("winit")
            .dep("k", "winit", Dep::build());
    assert_eq!(reaches(&fixture), [pair("k", "winit")]);
}

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

fn rules(reach: serde_json::Value) -> anyhow::Result<Rules> {
    let tiers: Vec<String> = ["V", "C", "S", "R", "K", "H", "pkg"]
        .map(str::to_owned)
        .into();
    Rules::from_workspace_metadata(&json!({ "flui": { "reach": reach } }), &tiers)
}

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

fn the_r_tier_inherits_wgpu_and_the_v_tier_refuses_tokio() {
    let rules = rules(json!({
        "tier": {
            "K": { "forbid": ["winit", "wgpu"] },
            "S": { "extends": "K" },
            "V": { "extends": "S", "forbid": ["tokio"] },
            "R": { "extends": "K" },
        },
    }))
    .expect("rules parse");
    assert!(rules.forbidden("R", &[], "wgpu"));
    assert!(rules.forbidden("R", &[], "winit"));
    assert!(rules.forbidden("K", &[], "wgpu"));
    assert!(rules.forbidden("V", &[], "tokio"));
    assert!(rules.forbidden("V", &[], "wgpu"));
    assert!(!rules.forbidden("S", &[], "tokio"));
}

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
            // a tier drops no inherited name; a crate's `grant` admits one
            json!({ "tier": { "K": { "forbid": ["x"] }, "R": { "extends": "K", "except": ["x"] } } }),
            "unknown key `except`",
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

/// The train-guard facts' packages: `flui-sdk` and the facade each with or
/// without a normal edge to `flui-foundation`.
fn train_guard(sdk_has_it: bool, facade_has_it: bool) -> Fixture {
    let mut fixture =
        base()
            .member("flui-foundation", "V", &json!(null))
            .member("flui-sdk", "K", &json!(null));
    if sdk_has_it {
        fixture = fixture.dep("flui-sdk", "flui-foundation", Dep::normal());
    }
    if facade_has_it {
        fixture = fixture.dep("flui", "flui-foundation", Dep::normal());
    }
    fixture
}

fn each_fact_reads_the_build_both_ways() {
    let [absent, present, enables, sdk_guard, facade_guard] = FACTS;
    assert!(matches!(absent.expect, Expect::Absent(_)));
    assert!(matches!(present.expect, Expect::Present(_)));
    assert!(matches!(enables.expect, Expect::Enables(..)));
    let holds = |fixture: &Fixture, fact: &Fact| {
        let graph = Graph::from_metadata(&fixture.metadata()).expect("the graph joins");
        evaluate(&graph, fact).expect("evaluates").is_none()
    };

    let good = hot_reload(true, true, true);
    for fact in [&absent, &present, &enables] {
        assert!(holds(&good, fact), "{}", fact.what);
    }

    // the train guard: in the SDK's and the facade's builds, or reported
    let guarded = train_guard(true, true);
    for fact in [&sdk_guard, &facade_guard] {
        assert!(matches!(fact.expect, Expect::Present("flui-foundation")));
        assert!(holds(&guarded, fact), "{}", fact.what);
    }
    assert!(!holds(&train_guard(false, true), &sdk_guard));
    assert!(!holds(&train_guard(true, false), &facade_guard));
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

/// The resolver builds exactly what `cargo tree` prints for the same root,
/// on every target, over normal and build edges.
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

#[test]
fn reach_gate_contract() {
    crate::table_test::run_table(
        "reach_gate_contract",
        &[
            (
                "a_feature_forwarded_to_a_dev_dependency_is_ignored",
                a_feature_forwarded_to_a_dev_dependency_is_ignored as fn(),
            ),
            (
                "an_optional_dependency_reaches_only_under_a_feature_that_enables_it",
                an_optional_dependency_reaches_only_under_a_feature_that_enables_it as fn(),
            ),
            (
                "a_weak_feature_does_not_activate_its_dependency",
                a_weak_feature_does_not_activate_its_dependency as fn(),
            ),
            (
                "a_strong_feature_enables_the_same_named_feature_whatever_it_lists",
                a_strong_feature_enables_the_same_named_feature_whatever_it_lists as fn(),
            ),
            (
                "an_unknown_feature_is_an_error_not_a_pass",
                an_unknown_feature_is_an_error_not_a_pass as fn(),
            ),
            (
                "a_dependency_is_matched_by_package_name_not_library_name",
                a_dependency_is_matched_by_package_name_not_library_name as fn(),
            ),
            (
                "a_target_specific_dependency_counts_on_every_target",
                a_target_specific_dependency_counts_on_every_target as fn(),
            ),
            (
                "a_build_dependency_reaches",
                a_build_dependency_reaches as fn(),
            ),
            (
                "features_combine_within_one_root_and_not_across_roots",
                features_combine_within_one_root_and_not_across_roots as fn(),
            ),
            (
                "a_generic_ffi_crate_matches_no_glob",
                a_generic_ffi_crate_matches_no_glob as fn(),
            ),
            (
                "an_exact_forbid_entry_naming_a_generic_ffi_crate_is_an_error",
                an_exact_forbid_entry_naming_a_generic_ffi_crate_is_an_error as fn(),
            ),
            (
                "the_r_tier_inherits_wgpu_and_the_v_tier_refuses_tokio",
                the_r_tier_inherits_wgpu_and_the_v_tier_refuses_tokio as fn(),
            ),
            (
                "an_extends_cycle_or_unknown_tier_is_an_error",
                an_extends_cycle_or_unknown_tier_is_an_error as fn(),
            ),
            (
                "a_reach_exception_needs_exactly_one_of_exit_or_grant",
                a_reach_exception_needs_exactly_one_of_exit_or_grant as fn(),
            ),
            (
                "each_fact_reads_the_build_both_ways",
                each_fact_reads_the_build_both_ways as fn(),
            ),
            (
                "the_resolver_agrees_with_cargo_tree",
                the_resolver_agrees_with_cargo_tree as fn(),
            ),
        ],
    );
}
