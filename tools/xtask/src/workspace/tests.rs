//! Each case builds a small workspace in a temporary directory, breaks one
//! rule, and runs the real `cargo metadata --no-deps` against it.

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

fn a_well_formed_workspace_passes() {
    let fixture = Fixture::new();
    assert_eq!(fixture.findings(), Vec::<String>::new());
}

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

fn a_same_layer_dependency_is_allowed() {
    let fixture = Fixture::new();
    fixture.edit("crates/b/Cargo.toml", "layer = 1", "layer = 0");
    assert_eq!(fixture.findings(), Vec::<String>::new());
}

fn a_dev_dependency_may_point_up() {
    let fixture = Fixture::new();
    fixture.edit(
        "crates/a/Cargo.toml",
        "[lints]",
        "[dev-dependencies]\nb = { path = \"../b\" }\n\n[lints]",
    );
    assert_eq!(fixture.findings(), Vec::<String>::new());
}

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

fn a_crate_without_a_layer_is_reported() {
    let fixture = Fixture::new();
    fixture.edit("crates/a/Cargo.toml", "layer = 0\n", "");
    assert_one(
        &fixture.findings(),
        "crates/a/Cargo.toml has no `[package.metadata.flui] layer`",
    );
}

fn a_layer_beyond_the_named_ones_is_reported() {
    let fixture = Fixture::new();
    fixture.edit("crates/b/Cargo.toml", "layer = 1", "layer = 5");
    assert_one(
        &fixture.findings(),
        "declares layer 5, but the root manifest names only 3",
    );
}

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

fn an_evolving_crate_sets_its_own_zero_major_version() {
    let fixture = Fixture::new();
    fixture.edit(
        "crates/a/Cargo.toml",
        "tier-kind = \"internal\"",
        "tier-kind = \"evolving\"",
    );
    // inheriting the train's version is refused
    assert_one(
        &fixture.findings(),
        "crates/a/Cargo.toml is evolving: it sets its own `version",
    );
    fixture.edit(
        "crates/a/Cargo.toml",
        "version.workspace = true",
        "version = \"1.0.0\"",
    );
    assert_one(
        &fixture.findings(),
        "crates/a/Cargo.toml is evolving: its version is `0.N` (ADR-0081 §3, ADR-0088 §4), not \
         `1.0.0`",
    );
    fixture.edit(
        "crates/a/Cargo.toml",
        "version = \"1.0.0\"",
        "version = \"0.1.0-dev\"",
    );
    assert_eq!(fixture.findings(), Vec::<String>::new());
}

fn an_internal_crate_with_its_own_version_is_still_reported() {
    let fixture = Fixture::new();
    fixture.edit(
        "crates/a/Cargo.toml",
        "version.workspace = true",
        "version = \"0.1.0-dev\"",
    );
    assert_one(
        &fixture.findings(),
        "crates/a/Cargo.toml must inherit `version.workspace = true`",
    );
}

/// Adds `crates/<name>` at tier Low with `links = "<links>"` and the build
/// script Cargo requires beside it.
fn write_linking_crate(fixture: &Fixture, name: &str, order: u64, links: &str) {
    fixture.write_crate(name, 0, "Low", order, "");
    fixture.edit(
        &format!("crates/{name}/Cargo.toml"),
        "repository.workspace = true\n",
        &format!("repository.workspace = true\nlinks = \"{links}\"\n"),
    );
    fixture.write(&format!("crates/{name}/build.rs"), "fn main() {}\n");
    fixture.edit(
        "Cargo.toml",
        "members = [",
        &format!("members = [\"crates/{name}\", "),
    );
}

fn only_flui_foundation_carries_the_train_guard() {
    let fixture = Fixture::new();
    write_linking_crate(&fixture, "flui-foundation", 2, "flui_train");
    assert_eq!(fixture.findings(), Vec::<String>::new());
    write_linking_crate(&fixture, "c", 3, "flui_train");
    assert_one(
        &fixture.findings(),
        "crates/c/Cargo.toml declares `links = \"flui_train\"`, which only flui-foundation \
         carries",
    );

    let fixture = Fixture::new();
    write_linking_crate(&fixture, "flui-foundation", 2, "something_else");
    assert_one(
        &fixture.findings(),
        "crates/flui-foundation/Cargo.toml must declare `links = \"flui_train\"`",
    );
}

/// Cargo's error for a second package with the guard's `links` value.
const TWO_TRAINS: &str = "links to the native library `flui_train`";

/// A fixture outside any workspace: an application that depends on an SDK
/// built on `flui-foundation` 0.2.0 and a facade built on 0.3.0, each copy
/// declaring `links` when `links` is given. Returns the `cargo metadata
/// --offline` result, which resolves the graph.
fn resolve_two_trains(dir: &Path, links: Option<&str>) -> std::process::Output {
    let write = |rel: &str, text: &str| {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().expect("BUG: fixture paths have a parent"))
            .expect("create fixture directory");
        std::fs::write(path, text).expect("write fixture file");
    };
    let links_line = links.map_or_else(String::new, |links| format!("links = \"{links}\"\n"));
    for (dir, version) in [("foundation-2", "0.2.0"), ("foundation-3", "0.3.0")] {
        write(
            &format!("{dir}/Cargo.toml"),
            &format!(
                "[package]\nname = \"flui-foundation\"\nversion = \"{version}\"\n\
                 edition = \"2024\"\n{links_line}"
            ),
        );
        write(&format!("{dir}/build.rs"), "fn main() {}\n");
        write(&format!("{dir}/src/lib.rs"), "");
    }
    for (name, foundation) in [("sdk", "foundation-2"), ("facade", "foundation-3")] {
        write(
            &format!("{name}/Cargo.toml"),
            &format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
                 [dependencies]\nflui-foundation = {{ path = \"../{foundation}\" }}\n"
            ),
        );
        write(&format!("{name}/src/lib.rs"), "");
    }
    // The application is its own workspace; the packages it names sit beside
    // it, outside the workspace directory, so none of them is a member.
    write(
        "app/Cargo.toml",
        "[workspace]\n\n[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
         [dependencies]\nsdk = { path = \"../sdk\" }\nfacade = { path = \"../facade\" }\n",
    );
    write("app/src/main.rs", "fn main() {}\n");
    std::process::Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args(["metadata", "--offline", "--format-version", "1"])
        .current_dir(dir.join("app"))
        .output()
        .expect("run cargo metadata")
}

/// ADR-0088 §5 with this repository's own `links` value: two trains in one
/// graph fail in the resolver. The control without `links` resolves both
/// copies side by side, which is the precondition for E0308 at the first
/// type that crosses between them.
fn two_trains_refuse_to_resolve() {
    let metadata = util::metadata(&util::repo_root()).expect("cargo metadata on this repository");
    let links = metadata
        .workspace_packages()
        .into_iter()
        .find(|package| package.name.as_str() == "flui-foundation")
        .expect("flui-foundation is a member")
        .links
        .clone();
    assert_eq!(links.as_deref(), Some("flui_train"));

    // A scratch directory with no workspace manifest above the packages.
    let fixture = Fixture::new();
    std::fs::remove_file(fixture.root().join("Cargo.toml")).expect("remove the root manifest");
    let dir = fixture.root().join("trains");
    let refused = resolve_two_trains(&dir.join("guarded"), links.as_deref());
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(!refused.status.success(), "two trains resolved: {stderr}");
    assert!(stderr.contains(TWO_TRAINS), "{stderr}");

    let control = resolve_two_trains(&dir.join("unguarded"), None);
    let stderr = String::from_utf8_lossy(&control.stderr);
    assert!(control.status.success(), "{stderr}");
    let resolved: serde_json::Value =
        serde_json::from_slice(&control.stdout).expect("cargo metadata prints JSON");
    let copies = resolved["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .filter(|package| package["name"] == "flui-foundation")
        .count();
    assert_eq!(copies, 2, "the control holds both trains");
}

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

fn block_commented_modules_do_not_mount_tests() {
    assert_commented_module_is_unreachable("/*\nmod orphan;\n*/\n");
}

fn line_commented_path_modules_do_not_mount_tests() {
    assert_commented_module_is_unreachable("// #[path = \"orphan.rs\"] mod omitted;\n");
}

fn assert_commented_module_is_unreachable(source: &str) {
    let fixture = Fixture::new();
    fixture.edit(
        "crates/a/Cargo.toml",
        "repository.workspace = true\n",
        "repository.workspace = true\nautotests = false\n\n[[test]]\nname = \"a_it\"\npath = \"tests/main.rs\"\n",
    );
    fixture.write("crates/a/tests/main.rs", source);
    fixture.write("crates/a/tests/orphan.rs", "");
    assert_one(&fixture.findings(), "crates/a/tests/orphan.rs never runs");
}

fn string_literals_do_not_mount_tests() {
    let fixture = Fixture::new();
    fixture.edit(
        "crates/a/Cargo.toml",
        "repository.workspace = true\n",
        "repository.workspace = true\nautotests = false\n\n[[test]]\nname = \"a_it\"\npath = \"tests/main.rs\"\n",
    );
    fixture.write(
        "crates/a/tests/main.rs",
        "const EXAMPLE: &str = r#\"#[path = \"orphan.rs\"] mod omitted;\"#;\n",
    );
    fixture.write("crates/a/tests/orphan.rs", "");
    assert_one(&fixture.findings(), "crates/a/tests/orphan.rs never runs");
}

fn implicit_cargo_main_test_path_mounts_tests() {
    assert_implicit_cargo_test_path_mounts_tests("main");
}

fn implicit_cargo_named_test_path_mounts_tests() {
    assert_implicit_cargo_test_path_mounts_tests("contract");
}

fn assert_implicit_cargo_test_path_mounts_tests(name: &str) {
    let fixture = Fixture::new();
    fixture.edit(
        "crates/a/Cargo.toml",
        "repository.workspace = true\n",
        &format!("repository.workspace = true\nautotests = false\n\n[[test]]\nname = \"{name}\"\n"),
    );
    fixture.write(
        &format!("crates/a/tests/{name}.rs"),
        "#[path = \"mounted.rs\"]\npub(crate) mod mounted;\n",
    );
    fixture.write("crates/a/tests/mounted.rs", "");
    assert_eq!(fixture.findings(), Vec::<String>::new());
}

fn duplicate_adr_numbers_are_reported() {
    let fixture = Fixture::new();
    fixture.write("docs/adr/ADR-0001-second.md", "# ADR-0001\n");
    assert_one(
        &fixture.findings(),
        "ADR-0001 is used by ADR-0001-first.md, ADR-0001-second.md",
    );
    assert!(fixture.root().join("docs/adr").is_dir());
}

fn a_modules_table_is_accepted_and_a_non_table_is_an_error() {
    let fixture = Fixture::new();
    fixture.edit(
        "crates/a/Cargo.toml",
        "order = 1",
        "order = 1\n\n[package.metadata.flui.modules]\nlayers = [[\"*\"]]",
    );
    assert_eq!(fixture.findings(), Vec::<String>::new());
    fixture.edit(
        "crates/a/Cargo.toml",
        "\n[package.metadata.flui.modules]\nlayers = [[\"*\"]]",
        "modules = [\"*\"]",
    );
    assert!(
        fixture.error().contains("`modules` must be a table"),
        "{}",
        fixture.error()
    );
}

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

fn a_reach_exception_citing_a_missing_adr_is_reported() {
    let fixture = Fixture::new();
    fixture.edit(
        "crates/a/Cargo.toml",
        "order = 1",
        "order = 1\nreach-exceptions = [{ to = \"winit\", exit = \"ADR-0999\", reason = \"test\" }]",
    );
    assert_one(
        &fixture.findings(),
        "a's `reach-exceptions` entry for winit names ADR-0999, which has no file under docs/adr",
    );
    fixture.edit(
        "crates/a/Cargo.toml",
        "exit = \"ADR-0999\"",
        "grant = \"0001\"",
    );
    assert_one(
        &fixture.findings(),
        "a's `reach-exceptions` entry for winit names grant \"0001\", which is not an `ADR-NNNN` \
         number",
    );
    fixture.edit("crates/a/Cargo.toml", "\"0001\"", "\"ADR-0001\"");
    assert_eq!(fixture.findings(), Vec::<String>::new());
    fixture.edit(
        "crates/a/Cargo.toml",
        "reach-exceptions = [{",
        "reach-forbid = [\"tokio\"]\nreach-exceptions = [{",
    );
    assert_eq!(fixture.findings(), Vec::<String>::new());
}

/// The facade turns no catalog, tool or capability on by default (ADR-0088
/// §6): an application names the features it uses.
fn the_facade_turns_no_feature_on_by_default() {
    let metadata = util::metadata(&util::repo_root()).expect("cargo metadata on the repository");
    let facade = metadata
        .workspace_packages()
        .into_iter()
        .find(|package| package.name.as_str() == "flui")
        .expect("the facade is a workspace member");
    let defaults = facade
        .features
        .get("default")
        .map_or(&[][..], Vec::as_slice);
    assert!(
        defaults.is_empty(),
        "the `flui` facade must have `default = []` (ADR-0088 §6), found {defaults:?}"
    );
}

/// Cargo reports canonical manifest paths; a root spelled through a symlink
/// (macOS's `/var` -> `/private/var`) must still own them. Where the host
/// refuses to create a symlink (Windows without the privilege), the root's
/// plain spelling against the canonical `\\?\` one is the same mismatch.
fn a_root_spelled_through_a_symlink_owns_canonical_paths() {
    let base = std::env::temp_dir().join(format!("xtask-relative-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let real = base.join("real");
    std::fs::create_dir_all(real.join("crates/a")).expect("create fixture directory");
    std::fs::write(real.join("crates/a/Cargo.toml"), "").expect("write fixture file");
    let link = base.join("link");
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&real, &link);
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_dir(&real, &link);
    let root = if linked.is_ok() { link } else { real.clone() };
    let manifest = real
        .canonicalize()
        .expect("canonicalize fixture")
        .join("crates/a/Cargo.toml");
    assert!(
        !manifest.starts_with(&root),
        "{} and {} must spell the directory differently for this test to mean anything",
        root.display(),
        manifest.display()
    );

    let rel = super::relative(&root, &manifest);
    let _ = std::fs::remove_dir_all(&base);
    assert_eq!(
        rel.expect("the manifest lies under the root"),
        "crates/a/Cargo.toml"
    );
}

#[test]
fn workspace_gate_contract() {
    crate::table_test::run_table(
        "workspace_gate_contract",
        &[
            (
                "a_well_formed_workspace_passes",
                a_well_formed_workspace_passes as fn(),
            ),
            (
                "the_facade_turns_no_feature_on_by_default",
                the_facade_turns_no_feature_on_by_default as fn(),
            ),
            (
                "an_upward_dependency_is_refused",
                an_upward_dependency_is_refused as fn(),
            ),
            (
                "a_same_layer_dependency_is_allowed",
                a_same_layer_dependency_is_allowed as fn(),
            ),
            (
                "a_dev_dependency_may_point_up",
                a_dev_dependency_may_point_up as fn(),
            ),
            (
                "allowed_dev_dependents_restrict_dev_dependencies",
                allowed_dev_dependents_restrict_dev_dependencies as fn(),
            ),
            (
                "allowed_dependents_leave_dev_dependencies_alone",
                allowed_dependents_leave_dev_dependencies_alone as fn(),
            ),
            (
                "allowed_dependents_cover_build_dependencies",
                allowed_dependents_cover_build_dependencies as fn(),
            ),
            (
                "examples_may_depend_on_a_restricted_crate",
                examples_may_depend_on_a_restricted_crate as fn(),
            ),
            (
                "a_crate_without_a_layer_is_reported",
                a_crate_without_a_layer_is_reported as fn(),
            ),
            (
                "a_layer_beyond_the_named_ones_is_reported",
                a_layer_beyond_the_named_ones_is_reported as fn(),
            ),
            (
                "depending_on_an_example_is_refused",
                depending_on_an_example_is_refused as fn(),
            ),
            (
                "manifests_inherit_the_workspace_keys_and_lints",
                manifests_inherit_the_workspace_keys_and_lints as fn(),
            ),
            (
                "an_evolving_crate_sets_its_own_zero_major_version",
                an_evolving_crate_sets_its_own_zero_major_version as fn(),
            ),
            (
                "an_internal_crate_with_its_own_version_is_still_reported",
                an_internal_crate_with_its_own_version_is_still_reported as fn(),
            ),
            (
                "only_flui_foundation_carries_the_train_guard",
                only_flui_foundation_carries_the_train_guard as fn(),
            ),
            (
                "two_trains_refuse_to_resolve",
                two_trains_refuse_to_resolve as fn(),
            ),
            (
                "a_test_file_no_target_reaches_is_reported",
                a_test_file_no_target_reaches_is_reported as fn(),
            ),
            (
                "an_undeclared_tests_main_is_reported",
                an_undeclared_tests_main_is_reported as fn(),
            ),
            (
                "block_commented_modules_do_not_mount_tests",
                block_commented_modules_do_not_mount_tests as fn(),
            ),
            (
                "line_commented_path_modules_do_not_mount_tests",
                line_commented_path_modules_do_not_mount_tests as fn(),
            ),
            (
                "string_literals_do_not_mount_tests",
                string_literals_do_not_mount_tests as fn(),
            ),
            (
                "implicit_cargo_main_test_path_mounts_tests",
                implicit_cargo_main_test_path_mounts_tests as fn(),
            ),
            (
                "implicit_cargo_named_test_path_mounts_tests",
                implicit_cargo_named_test_path_mounts_tests as fn(),
            ),
            (
                "duplicate_adr_numbers_are_reported",
                duplicate_adr_numbers_are_reported as fn(),
            ),
            (
                "a_modules_table_is_accepted_and_a_non_table_is_an_error",
                a_modules_table_is_accepted_and_a_non_table_is_an_error as fn(),
            ),
            (
                "a_mistyped_or_unknown_flui_key_is_an_error",
                a_mistyped_or_unknown_flui_key_is_an_error as fn(),
            ),
            (
                "a_dev_cycle_inside_a_tier_is_allowed",
                a_dev_cycle_inside_a_tier_is_allowed as fn(),
            ),
            (
                "a_crate_without_tier_order_or_kind_is_reported",
                a_crate_without_tier_order_or_kind_is_reported as fn(),
            ),
            (
                "an_unknown_tier_or_kind_is_reported",
                an_unknown_tier_or_kind_is_reported as fn(),
            ),
            (
                "an_example_declares_only_the_tool_kind",
                an_example_declares_only_the_tool_kind as fn(),
            ),
            (
                "a_stale_edge_exception_is_reported",
                a_stale_edge_exception_is_reported as fn(),
            ),
            (
                "an_edge_exception_citing_a_missing_adr_is_reported",
                an_edge_exception_citing_a_missing_adr_is_reported as fn(),
            ),
            (
                "a_reach_exception_citing_a_missing_adr_is_reported",
                a_reach_exception_citing_a_missing_adr_is_reported as fn(),
            ),
            (
                "a_root_spelled_through_a_symlink_owns_canonical_paths",
                a_root_spelled_through_a_symlink_owns_canonical_paths as fn(),
            ),
        ],
    );
}
