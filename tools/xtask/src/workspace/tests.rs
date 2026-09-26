//! Each test builds a small workspace in a temporary directory, breaks one
//! rule, and runs the real `cargo metadata --no-deps` against it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::check;
use crate::util;

/// A throwaway workspace; removed on drop.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    /// Layers "Base" and "Top" (and an unused third), tiers "Low", "High" and
    /// "Top", crates `a` (layer 0, tier Low order 1) and `b` (layer 1, tier High
    /// order 1, depends on `a`), an example `ex`, and one ADR.
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "xtask-workspace-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        let fixture = Self { root };
        fixture.write(
            "Cargo.toml",
            r#"[workspace]
resolver = "3"
members = ["crates/a", "crates/b", "examples/ex"]

[workspace.metadata.flui]
layers = ["Base", "Top", "Design systems"]
tiers = ["Low", "High", "Top"]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
license = "MIT"
authors = ["x"]
repository = "https://example.com"

[workspace.lints.rust]
unsafe_code = "warn"
"#,
        );
        fixture.write_crate("a", 0, "Low", 1, "");
        fixture.write_crate("b", 1, "High", 1, "a = { path = \"../a\" }\n");
        fixture.write(
            "examples/ex/Cargo.toml",
            r#"[package]
name = "ex"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish = false

[dependencies]
b = { path = "../../crates/b" }

[lints]
workspace = true

[package.metadata.flui]
tier-kind = "tool"
"#,
        );
        fixture.write("examples/ex/src/main.rs", "fn main() {}\n");
        fixture.write("docs/adr/ADR-0001-first.md", "# ADR-0001\n");
        fixture
    }

    fn write_crate(&self, name: &str, layer: usize, tier: &str, order: u64, dependencies: &str) {
        self.write(
            &format!("crates/{name}/Cargo.toml"),
            &format!(
                r#"[package]
name = "{name}"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true

[dependencies]
{dependencies}
[lints]
workspace = true

[package.metadata.flui]
layer = {layer}
tier = "{tier}"
tier-kind = "internal"
order = {order}
"#
            ),
        );
        self.write(&format!("crates/{name}/src/lib.rs"), "");
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.root.join(rel);
        std::fs::create_dir_all(path.parent().expect("BUG: fixture paths have a parent"))
            .expect("create fixture directory");
        std::fs::write(path, text).expect("write fixture file");
    }

    fn edit(&self, rel: &str, from: &str, to: &str) {
        let path = self.root.join(rel);
        let text = std::fs::read_to_string(&path).expect("read fixture file");
        assert!(text.contains(from), "{rel} does not contain {from:?}");
        std::fs::write(path, text.replacen(from, to, 1)).expect("write fixture file");
    }

    fn findings(&self) -> Vec<String> {
        let metadata = util::metadata(&self.root).expect("cargo metadata on the fixture");
        check(&self.root, &metadata).expect("check runs").0
    }

    /// The error `check` stops with, for a manifest it cannot read.
    fn error(&self) -> String {
        let metadata = util::metadata(&self.root).expect("cargo metadata on the fixture");
        format!(
            "{:#}",
            check(&self.root, &metadata).expect_err("check refuses the manifest")
        )
    }

    fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn assert_one(findings: &[String], needle: &str) {
    assert!(
        findings.len() == 1 && findings[0].contains(needle),
        "expected one finding containing {needle:?}, got {findings:#?}"
    );
}

#[test]
fn a_well_formed_workspace_passes() {
    let fixture = Fixture::new();
    assert_eq!(fixture.findings(), Vec::<String>::new());
}

#[test]
fn an_upward_dependency_is_refused() {
    let fixture = Fixture::new();
    fixture.edit(
        "crates/a/Cargo.toml",
        "[dependencies]\n",
        "[dependencies]\nb = { path = \"../b\" }\n",
    );
    fixture.edit("crates/b/Cargo.toml", "a = { path = \"../a\" }\n", "");
    // the tier rule admits the edge, so the layer rule reports it alone
    fixture.edit(
        "crates/b/Cargo.toml",
        "tier = \"High\"\ntier-kind = \"internal\"\norder = 1",
        "tier = \"Low\"\ntier-kind = \"internal\"\norder = 0",
    );
    assert_one(
        &fixture.findings(),
        "a (layer 0 (Base)) depends on b (layer 1 (Top))",
    );
}

#[test]
fn a_same_layer_dependency_is_allowed() {
    let fixture = Fixture::new();
    fixture.edit("crates/b/Cargo.toml", "layer = 1", "layer = 0");
    assert_eq!(fixture.findings(), Vec::<String>::new());
}

#[test]
fn a_dev_dependency_may_point_up() {
    let fixture = Fixture::new();
    fixture.edit(
        "crates/a/Cargo.toml",
        "[lints]",
        "[dev-dependencies]\nb = { path = \"../b\" }\n\n[lints]",
    );
    assert_eq!(fixture.findings(), Vec::<String>::new());
}

#[test]
fn allowed_dev_dependents_restrict_dev_dependencies() {
    let fixture = Fixture::new();
    fixture.edit(
        "crates/b/Cargo.toml",
        "layer = 1",
        "layer = 1\nallowed-dependents = [\"ex\"]\nallowed-dev-dependents = [\"ex\"]",
    );
    fixture.edit(
        "crates/a/Cargo.toml",
        "[lints]",
        "[dev-dependencies]\nb = { path = \"../b\" }\n\n[lints]",
    );
    assert_one(
        &fixture.findings(),
        "a has a dev-dependency on b, which allows only ex (its `allowed-dev-dependents`)",
    );
}

#[test]
fn allowed_dependents_leave_dev_dependencies_alone() {
    let fixture = Fixture::new();
    fixture.edit(
        "crates/b/Cargo.toml",
        "layer = 1",
        "layer = 1\nallowed-dependents = [\"ex\"]",
    );
    fixture.edit(
        "crates/a/Cargo.toml",
        "[lints]",
        "[dev-dependencies]\nb = { path = \"../b\" }\n\n[lints]",
    );
    assert_eq!(fixture.findings(), Vec::<String>::new());
}

#[test]
fn allowed_dependents_cover_build_dependencies() {
    let fixture = Fixture::new();
    fixture.edit(
        "crates/a/Cargo.toml",
        "layer = 0",
        "layer = 0\nallowed-dependents = [\"ex\"]",
    );
    fixture.edit(
        "crates/b/Cargo.toml",
        "[dependencies]\na = { path = \"../a\" }\n",
        "[build-dependencies]\na = { path = \"../a\" }\n",
    );
    assert_one(&fixture.findings(), "b depends on a, which allows only ex");
}

#[test]
fn examples_may_depend_on_a_restricted_crate() {
    // `ex` depends on `b`, which allows no crate at all
    let fixture = Fixture::new();
    fixture.edit(
        "crates/b/Cargo.toml",
        "layer = 1",
        "layer = 1\nallowed-dependents = []\nallowed-dev-dependents = []",
    );
    assert_eq!(fixture.findings(), Vec::<String>::new());
}

#[test]
fn a_crate_without_a_layer_is_reported() {
    let fixture = Fixture::new();
    fixture.edit("crates/a/Cargo.toml", "layer = 0\n", "");
    assert_one(
        &fixture.findings(),
        "crates/a/Cargo.toml has no `[package.metadata.flui] layer`",
    );
}

#[test]
fn a_layer_beyond_the_named_ones_is_reported() {
    let fixture = Fixture::new();
    fixture.edit("crates/b/Cargo.toml", "layer = 1", "layer = 5");
    assert_one(
        &fixture.findings(),
        "declares layer 5, but the root manifest names only 3",
    );
}

#[test]
fn allowed_dependents_are_enforced() {
    let fixture = Fixture::new();
    fixture.edit(
        "crates/a/Cargo.toml",
        "layer = 0",
        "layer = 0\nallowed-dependents = [\"ex\"]",
    );
    assert_one(&fixture.findings(), "b depends on a, which allows only ex");
}

#[test]
fn depending_on_an_example_is_refused() {
    let fixture = Fixture::new();
    fixture.edit(
        "examples/ex/Cargo.toml",
        "[dependencies]\nb = { path = \"../../crates/b\" }\n",
        "[lib]\npath = \"src/main.rs\"\n",
    );
    fixture.edit(
        "crates/b/Cargo.toml",
        "a = { path = \"../a\" }\n",
        "ex = { path = \"../../examples/ex\" }\n",
    );
    let findings = fixture.findings();
    assert_eq!(findings.len(), 2, "{findings:#?}");
    assert!(findings[0].contains("b depends on ex, whose `tier-kind` is \"tool\""));
    assert!(findings[1].contains("b depends on ex, an example or tool"));
}

#[test]
fn manifests_inherit_the_workspace_keys_and_lints() {
    let fixture = Fixture::new();
    fixture.edit(
        "crates/a/Cargo.toml",
        "license.workspace = true",
        "license = \"MIT\"",
    );
    fixture.edit("crates/b/Cargo.toml", "[lints]\nworkspace = true\n", "");
    fixture.edit("examples/ex/Cargo.toml", "publish = false\n", "");
    let findings = fixture.findings();
    assert_eq!(findings.len(), 3, "{findings:#?}");
    assert!(findings[0].contains("crates/a/Cargo.toml must inherit `license.workspace = true`"));
    assert!(findings[1].contains("crates/b/Cargo.toml must set `[lints] workspace = true`"));
    assert!(findings[2].contains("examples/ex/Cargo.toml must set `publish = false`"));
}

#[test]
fn a_test_file_no_target_reaches_is_reported() {
    let fixture = Fixture::new();
    fixture.edit(
        "crates/a/Cargo.toml",
        "repository.workspace = true\n",
        "repository.workspace = true\nautotests = false\n\n[[test]]\nname = \"a_it\"\npath = \"tests/main.rs\"\n",
    );
    fixture.write(
        "crates/a/tests/main.rs",
        "mod mounted;\n#[path = \"elsewhere.rs\"]\nmod renamed;\n",
    );
    fixture.write("crates/a/tests/mounted.rs", "");
    fixture.write("crates/a/tests/elsewhere.rs", "");
    fixture.write("crates/a/tests/orphan.rs", "");
    assert_one(&fixture.findings(), "crates/a/tests/orphan.rs never runs");
}

#[test]
fn an_undeclared_tests_main_is_reported() {
    let fixture = Fixture::new();
    fixture.edit(
        "crates/a/Cargo.toml",
        "repository.workspace = true\n",
        "repository.workspace = true\nautotests = false\n",
    );
    fixture.write("crates/a/tests/main.rs", "");
    assert_one(
        &fixture.findings(),
        "crates/a/tests/main.rs is not a declared `[[test]]` target",
    );
}

#[test]
fn duplicate_adr_numbers_are_reported() {
    let fixture = Fixture::new();
    fixture.write("docs/adr/ADR-0001-second.md", "# ADR-0001\n");
    assert_one(
        &fixture.findings(),
        "ADR-0001 is used by ADR-0001-first.md, ADR-0001-second.md",
    );
    assert!(fixture.root().join("docs/adr").is_dir());
}

#[test]
fn wasm_false_is_accepted() {
    let fixture = Fixture::new();
    fixture.edit(
        "crates/a/Cargo.toml",
        "layer = 0",
        "layer = 0
wasm = false",
    );
    assert_eq!(fixture.findings(), Vec::<String>::new());
}

#[test]
fn a_mistyped_or_unknown_flui_key_is_an_error() {
    let fixture = Fixture::new();
    fixture.edit(
        "crates/a/Cargo.toml",
        "layer = 0",
        "layer = 0
wasm = \"false\"",
    );
    assert!(
        fixture.error().contains("`wasm` must be `true` or `false`"),
        "{}",
        fixture.error()
    );
    fixture.edit("crates/a/Cargo.toml", "wasm = \"false\"", "wasm32 = false");
    assert!(
        fixture
            .error()
            .contains("unknown `[package.metadata.flui]` key `wasm32`"),
        "{}",
        fixture.error()
    );
    fixture.edit("crates/a/Cargo.toml", "wasm32 = false\n", "");
    fixture.edit("crates/a/Cargo.toml", "order = 1", "order = \"1\"");
    assert!(
        fixture
            .error()
            .contains("`order` must be a non-negative integer"),
        "{}",
        fixture.error()
    );
    fixture.edit(
        "crates/a/Cargo.toml",
        "order = \"1\"",
        "order = 1\nedge-exceptions = [\"b\"]",
    );
    assert!(
        fixture
            .error()
            .contains("`edge-exceptions` must be a list of"),
        "{}",
        fixture.error()
    );
}

/// Sets `crate`'s tier and order, replacing the fixture's.
fn place(
    fixture: &Fixture,
    name: &str,
    (tier, order): (&str, u64),
    (to_tier, to_order): (&str, u64),
) {
    fixture.edit(
        &format!("crates/{name}/Cargo.toml"),
        &format!("tier = \"{tier}\"\ntier-kind = \"internal\"\norder = {order}"),
        &format!("tier = \"{to_tier}\"\ntier-kind = \"internal\"\norder = {to_order}"),
    );
}

/// Replaces `b -> a` with `a -> b`, both on layer 0, so only the tier rule
/// can speak.
fn reverse_the_edge(fixture: &Fixture) {
    fixture.edit("crates/b/Cargo.toml", "a = { path = \"../a\" }\n", "");
    fixture.edit("crates/b/Cargo.toml", "layer = 1", "layer = 0");
    fixture.edit(
        "crates/a/Cargo.toml",
        "[dependencies]\n",
        "[dependencies]\nb = { path = \"../b\" }\n",
    );
}

#[test]
fn an_upward_tier_edge_is_refused() {
    let fixture = Fixture::new();
    reverse_the_edge(&fixture);
    assert_one(
        &fixture.findings(),
        "a (tier Low, order 1) depends on b (tier High, order 1): a dependency points to a \
         lower tier, or to a smaller order in the same tier",
    );
}

#[test]
fn an_in_tier_edge_to_a_larger_order_is_refused() {
    let fixture = Fixture::new();
    place(&fixture, "a", ("Low", 1), ("Low", 2));
    place(&fixture, "b", ("High", 1), ("Low", 1));
    assert_one(
        &fixture.findings(),
        "b (tier Low, order 1) depends on a (tier Low, order 2)",
    );
}

#[test]
fn an_in_tier_edge_to_a_smaller_order_is_allowed() {
    let fixture = Fixture::new();
    place(&fixture, "b", ("High", 1), ("Low", 2));
    assert_eq!(fixture.findings(), Vec::<String>::new());
}

#[test]
fn a_dev_edge_may_point_up_a_tier() {
    let fixture = Fixture::new();
    fixture.edit("crates/b/Cargo.toml", "a = { path = \"../a\" }\n", "");
    fixture.edit(
        "crates/a/Cargo.toml",
        "[lints]",
        "[dev-dependencies]\nb = { path = \"../b\" }\n\n[lints]",
    );
    assert_eq!(fixture.findings(), Vec::<String>::new());
}

#[test]
fn a_dev_cycle_inside_a_tier_is_allowed() {
    // the shape of flui-view <-> flui-testing
    let fixture = Fixture::new();
    place(&fixture, "b", ("High", 1), ("Low", 2));
    fixture.edit(
        "crates/a/Cargo.toml",
        "[lints]",
        "[dev-dependencies]\nb = { path = \"../b\" }\n\n[lints]",
    );
    assert_eq!(fixture.findings(), Vec::<String>::new());
}

#[test]
fn a_crate_without_tier_order_or_kind_is_reported() {
    let fixture = Fixture::new();
    fixture.edit(
        "crates/a/Cargo.toml",
        "tier = \"Low\"\ntier-kind = \"internal\"\norder = 1\n",
        "",
    );
    let findings = fixture.findings();
    assert_eq!(findings.len(), 3, "{findings:#?}");
    for (finding, key) in findings.iter().zip(["tier", "tier-kind", "order"]) {
        assert!(
            finding.contains(&format!(
                "crates/a/Cargo.toml has no `[package.metadata.flui] {key}`"
            )),
            "{finding}"
        );
    }
}

#[test]
fn an_unknown_tier_or_kind_is_reported() {
    let fixture = Fixture::new();
    fixture.edit(
        "crates/a/Cargo.toml",
        "tier = \"Low\"\ntier-kind = \"internal\"",
        "tier = \"Q\"\ntier-kind = \"beta\"",
    );
    let findings = fixture.findings();
    assert_eq!(findings.len(), 2, "{findings:#?}");
    assert!(findings[0].contains("crates/a/Cargo.toml declares tier \"Q\", but the root"));
    assert!(findings[1].contains("crates/a/Cargo.toml declares tier-kind \"beta\""));
}

#[test]
fn two_crates_sharing_an_order_in_a_tier_are_reported() {
    let fixture = Fixture::new();
    fixture.edit("crates/b/Cargo.toml", "a = { path = \"../a\" }\n", "");
    place(&fixture, "b", ("High", 1), ("Low", 1));
    assert_one(
        &fixture.findings(),
        "a and b share order 1 in tier Low; an order is unique within its tier",
    );
}

#[test]
fn an_example_declares_only_the_tool_kind() {
    let fixture = Fixture::new();
    fixture.edit(
        "examples/ex/Cargo.toml",
        "tier-kind = \"tool\"",
        "tier-kind = \"tool\"\ntier = \"Low\"",
    );
    assert_one(
        &fixture.findings(),
        "examples/ex/Cargo.toml is an example or tool: it declares only `tier-kind = \"tool\"`, \
         not `tier`",
    );
    fixture.edit(
        "examples/ex/Cargo.toml",
        "tier-kind = \"tool\"\ntier = \"Low\"",
        "tier-kind = \"internal\"",
    );
    assert_one(
        &fixture.findings(),
        "examples/ex/Cargo.toml is an example or tool: its `tier-kind` is \"internal\"",
    );
    fixture.edit("examples/ex/Cargo.toml", "tier-kind = \"internal\"", "");
    assert_one(
        &fixture.findings(),
        "examples/ex/Cargo.toml has no `[package.metadata.flui] tier-kind`",
    );
}

#[test]
fn nothing_depends_on_a_tool_kind_crate() {
    let fixture = Fixture::new();
    fixture.edit(
        "crates/a/Cargo.toml",
        "tier-kind = \"internal\"",
        "tier-kind = \"tool\"",
    );
    assert_one(
        &fixture.findings(),
        "b depends on a, whose `tier-kind` is \"tool\"",
    );
}

#[test]
fn an_edge_exception_admits_one_upward_edge() {
    let fixture = Fixture::new();
    reverse_the_edge(&fixture);
    fixture.edit(
        "crates/a/Cargo.toml",
        "order = 1",
        "order = 1\nedge-exceptions = [{ to = \"b\", exit = \"ADR-0001\", reason = \"test\" }]",
    );
    assert_eq!(fixture.findings(), Vec::<String>::new());
}

#[test]
fn a_stale_edge_exception_is_reported() {
    let fixture = Fixture::new();
    // `b -> a` exists and the rule admits it
    fixture.edit(
        "crates/b/Cargo.toml",
        "order = 1",
        "order = 1\nedge-exceptions = [{ to = \"a\", exit = \"ADR-0001\", reason = \"test\" }]",
    );
    assert_one(
        &fixture.findings(),
        "b lists `edge-exceptions` for a, but has no normal or build dependency on it that the \
         tier rule refuses",
    );
    // `a -> b` does not exist
    fixture.edit(
        "crates/b/Cargo.toml",
        "edge-exceptions",
        "# edge-exceptions",
    );
    fixture.edit(
        "crates/a/Cargo.toml",
        "order = 1",
        "order = 1\nedge-exceptions = [{ to = \"b\", exit = \"ADR-0001\", reason = \"test\" }]",
    );
    assert_one(&fixture.findings(), "a lists `edge-exceptions` for b");
    // a second entry for an edge an entry already admits
    reverse_the_edge(&fixture);
    fixture.edit(
        "crates/a/Cargo.toml",
        "reason = \"test\" }]",
        "reason = \"test\" }, { to = \"b\", exit = \"ADR-0001\", reason = \"again\" }]",
    );
    assert_one(&fixture.findings(), "a lists `edge-exceptions` for b");
}

#[test]
fn an_edge_exception_citing_a_missing_adr_is_reported() {
    let fixture = Fixture::new();
    reverse_the_edge(&fixture);
    fixture.edit(
        "crates/a/Cargo.toml",
        "order = 1",
        "order = 1\nedge-exceptions = [{ to = \"b\", exit = \"ADR-0999\", reason = \"test\" }]",
    );
    assert_one(
        &fixture.findings(),
        "a's `edge-exceptions` entry for b names ADR-0999, which has no file under docs/adr",
    );
    fixture.edit("crates/a/Cargo.toml", "ADR-0999", "0001");
    assert_one(
        &fixture.findings(),
        "a's `edge-exceptions` entry for b names exit \"0001\", which is not an `ADR-NNNN` number",
    );
}

#[test]
fn the_self_test_reports_exactly_the_planted_findings() {
    let (missed, extra) = super::tiers::self_test_diff();
    assert!(
        missed.is_empty() && extra.is_empty(),
        "missed {missed:#?}, false positives {extra:#?}"
    );
}

#[test]
fn the_design_systems_admit_only_the_adr_0028_dependents() {
    let metadata = util::metadata(&util::repo_root()).expect("cargo metadata on the repository");
    let members = super::Members::load(&util::repo_root(), &metadata).expect("manifests load");
    let by_name = members.by_name();
    let expected: BTreeSet<String> = ["flui-localizations", "flui-app", "flui"]
        .map(str::to_owned)
        .into();
    for design_system in ["flui-material", "flui-cupertino"] {
        let member = by_name[design_system];
        assert_eq!(
            member.allowed_dependents.as_ref(),
            Some(&expected),
            "{design_system}"
        );
        assert_eq!(
            member.allowed_dev_dependents.as_ref(),
            Some(&expected),
            "{design_system}"
        );
    }
}

/// The two crates ADR-0081 deletes keep their dependents frozen: each list
/// names exactly the crates that depend on it now, and never a crate outside
/// the set it had when the record was accepted. Adding a dependent means
/// editing this test; dropping an edge means dropping its entry.
#[test]
fn the_deleted_crates_admit_only_their_frozen_dependents() {
    let metadata = util::metadata(&util::repo_root()).expect("cargo metadata on the repository");
    let members = super::Members::load(&util::repo_root(), &metadata).expect("manifests load");
    let by_name = members.by_name();
    let frozen: [(&str, &[&str]); 2] = [
        (
            "flui-tree",
            &[
                "flui",
                "flui-layer",
                "flui-objects",
                "flui-rendering",
                "flui-semantics",
                "flui-view",
            ],
        ),
        ("flui-localizations", &["flui"]),
    ];
    for (target, admitted) in frozen {
        let admitted: BTreeSet<String> = admitted.iter().map(|&name| name.to_owned()).collect();
        let member = by_name[target];
        let listed = member
            .allowed_dependents
            .clone()
            .expect("a deleted crate lists its allowed-dependents");
        assert!(
            listed.is_subset(&admitted),
            "{target}: the frozen list only shrinks, but it also names {:?}",
            listed.difference(&admitted).collect::<Vec<_>>()
        );
        let dependents = |dev: bool| -> BTreeSet<String> {
            members
                .iter()
                .filter(|member| !member.is_example_or_tool())
                .filter(|member| {
                    member.repo_deps().any(|dep| {
                        dep.name == target
                            && (dep.kind == super::DependencyKind::Development) == dev
                    })
                })
                .map(|member| member.name().to_owned())
                .collect()
        };
        assert_eq!(listed, dependents(false), "{target}: allowed-dependents");
        assert_eq!(
            member.allowed_dev_dependents.clone(),
            Some(dependents(true)),
            "{target}: allowed-dev-dependents"
        );
    }
}

/// `(package, tier, tier-kind)`.
type Placement = (String, Option<String>, String);

#[test]
fn the_tiers_match_the_adr_0081_table() {
    let metadata = util::metadata(&util::repo_root()).expect("cargo metadata on the repository");
    let members = super::Members::load(&util::repo_root(), &metadata).expect("manifests load");
    let table: [(&str, &str, &[&str]); 9] = [
        (
            "V",
            "internal",
            &[
                "flui-geometry",
                "flui-types",
                "flui-macros",
                "flui-foundation",
                "flui-tree",
            ],
        ),
        ("C", "stable", &["flui-platform-api"]),
        (
            "S",
            "internal",
            &[
                "flui-log",
                "flui-scheduler",
                "flui-painting",
                "flui-interaction",
                "flui-semantics",
                "flui-animation",
                "flui-assets",
            ],
        ),
        (
            "R",
            "internal",
            &[
                "flui-layer",
                "flui-rendering",
                "flui-objects",
                "flui-engine",
            ],
        ),
        (
            "K",
            "internal",
            &[
                "flui-view",
                "flui-testing",
                "flui-widgets",
                "flui-localizations",
            ],
        ),
        ("H", "internal", &["flui-platform", "flui-app"]),
        ("H", "tool", &["flui-cli"]),
        ("H", "stable", &["flui"]),
        (
            "pkg",
            "official",
            &[
                "flui-material",
                "flui-cupertino",
                "flui-devtools",
                "flui-hot-reload",
            ],
        ),
    ];
    let mut expected: BTreeSet<Placement> = table
        .iter()
        .flat_map(|(tier, kind, names)| {
            names.iter().map(|name| {
                (
                    (*name).to_owned(),
                    Some((*tier).to_owned()),
                    (*kind).to_owned(),
                )
            })
        })
        .collect();
    let applications: Vec<&str> = members
        .iter()
        .filter(|member| member.is_example_or_tool())
        .map(super::Member::name)
        .collect();
    assert_eq!(applications.len(), 12, "{applications:?}");
    expected.extend(
        applications
            .iter()
            .map(|name| ((*name).to_owned(), None, "tool".to_owned())),
    );

    let actual: BTreeSet<Placement> = members
        .iter()
        .map(|member| {
            (
                member.name.clone(),
                member.tier.clone(),
                member.tier_kind.clone().unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(actual, expected);

    let exceptions: BTreeSet<(&str, &str, &str)> = members
        .iter()
        .flat_map(|member| {
            member
                .edge_exceptions
                .iter()
                .map(move |entry| (member.name(), entry.to.as_str(), entry.exit.as_str()))
        })
        .collect();
    let seeded: BTreeSet<(&str, &str, &str)> = [
        ("flui-app", "flui-hot-reload", "ADR-0094"),
        ("flui", "flui-hot-reload", "ADR-0094"),
        ("flui", "flui-material", "ADR-0088"),
        ("flui", "flui-cupertino", "ADR-0088"),
    ]
    .into();
    assert_eq!(exceptions, seeded);
}
