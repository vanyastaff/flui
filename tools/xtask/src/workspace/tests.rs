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
    /// Layers "Base" and "Top" (and an unused third), crates `a` (layer 0) and `b` (layer 1,
    /// depends on `a`), an example `ex`, and one ADR.
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
        fixture.write_crate("a", 0, "");
        fixture.write_crate("b", 1, "a = { path = \"../a\" }\n");
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
"#,
        );
        fixture.write("examples/ex/src/main.rs", "fn main() {}\n");
        fixture.write("docs/adr/ADR-0001-first.md", "# ADR-0001\n");
        fixture
    }

    fn write_crate(&self, name: &str, layer: usize, dependencies: &str) {
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
    fixture.edit(
        "crates/a/Cargo.toml",
        "\n[package.metadata.flui]\nlayer = 0\n",
        "\n",
    );
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
    assert_one(&fixture.findings(), "b depends on ex, an example or tool");
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
