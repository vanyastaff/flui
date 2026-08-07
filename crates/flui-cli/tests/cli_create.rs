//! Integration tests for `flui create` command.
//!
//! Tests project creation with TempDir, verifying directory structure,
//! template selection, and `--local` flag behavior.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Get a command for the `flui` binary.
fn flui() -> Command {
    cargo_bin_cmd!("flui")
}

/// Workspace root — this crate lives at `<root>/crates/flui-cli`.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("BUG: flui-cli must sit two levels below the workspace root")
        .to_path_buf()
}

/// Generate a project with `--local` and prove it actually compiles.
///
/// This is the real gate on the templates: the file-existence tests below pass
/// just as happily on a template that emits a long-deleted API. Anything that
/// drifts the generated `main.rs` or `Cargo.toml` off the current public
/// surface fails here.
///
/// `flui create --local` emits `path = "../../crates/flui-app"`, so a generated
/// project resolves its dependencies only from exactly one directory below the
/// workspace root — hence `<root>/target/<name>` (already gitignored) rather
/// than a `TempDir`.
///
/// The check gets its own `--target-dir`: reusing the workspace's would
/// deadlock, since the outer `cargo test` holds that directory's build lock for
/// the duration of the run.
fn assert_generated_project_compiles(template: &str) {
    let root = repo_root();
    let target = root.join("target");
    let name = format!("flui-tmpl-check-{template}");
    let project = target.join(&name);

    // `target/` may not exist yet when `CARGO_TARGET_DIR` points elsewhere.
    std::fs::create_dir_all(&target).expect("create the scratch directory");
    if project.exists() {
        std::fs::remove_dir_all(&project).expect("clear the previous generated project");
    }

    flui()
        .args([
            "create",
            &name,
            "--template",
            template,
            "--org",
            "com.test",
            "--local",
        ])
        .arg("--path")
        .arg(&target)
        .assert()
        .success();

    // Seed the generated project with this workspace's own resolved versions
    // before checking it, and forbid the network.
    //
    // Without this the generated project — which declares its own empty
    // `[workspace]` to detach from ours — resolves its whole transitive graph
    // fresh from the registry on every run. That makes this test, and
    // therefore the merge gate, non-hermetic: any crate anywhere in that graph
    // can turn CI red with nothing in this repository having changed. It has:
    // an upstream `zune-jpeg` release stopped compiling under the pinned
    // toolchain and took `main` down with it. Every other cargo invocation in
    // `ci.yml` runs `--locked` for exactly this reason; this one escaped the
    // discipline by construction, because it builds a *different* project.
    //
    // Copying the lock is sound here precisely because `--local` points the
    // generated project's `flui-*` dependencies at this tree, so its
    // third-party graph is a subset of ours. `cargo` still adds the new root
    // package to the copied lock; `--offline` is what guarantees nothing else
    // is re-resolved, and fails loudly rather than silently upgrading if a
    // crate is somehow absent from the cache.
    //
    // What this deliberately gives up: this test no longer notices that the
    // template compiles against the *current* published world. That check
    // belongs in `weekly.yml`, which already builds against a fresh
    // `cargo update` as early warning rather than as a merge gate.
    std::fs::copy(root.join("Cargo.lock"), project.join("Cargo.lock"))
        .expect("seed the generated project with the workspace's resolved versions");

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = std::process::Command::new(cargo)
        .arg("check")
        .arg("--offline")
        .arg("--target-dir")
        .arg(target.join("cli-template-check"))
        .current_dir(&project)
        .output()
        .expect("run cargo check on the generated project");

    assert!(
        output.status.success(),
        "`flui create --template {template}` generated a project that does not compile:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn generated_basic_project_compiles() {
    assert_generated_project_compiles("basic");
}

#[test]
fn generated_counter_project_compiles() {
    assert_generated_project_compiles("counter");
}

#[test]
fn create_project_with_basic_template() {
    let tmp = TempDir::new().expect("temp dir");
    let project_dir = tmp.path().join("test-basic");

    flui()
        .args([
            "create",
            "test-basic",
            "--template",
            "basic",
            "--org",
            "com.test",
        ])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .success();

    // Verify directory structure
    assert!(
        project_dir.join("Cargo.toml").exists(),
        "Cargo.toml missing"
    );
    assert!(
        project_dir.join("src").join("main.rs").exists(),
        "src/main.rs missing"
    );
    assert!(project_dir.join("flui.toml").exists(), "flui.toml missing");
    assert!(project_dir.join("assets").is_dir(), "assets/ missing");
}

#[test]
fn create_project_with_counter_template() {
    let tmp = TempDir::new().expect("temp dir");
    let project_dir = tmp.path().join("test-counter");

    flui()
        .args([
            "create",
            "test-counter",
            "--template",
            "counter",
            "--org",
            "com.test",
        ])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .success();

    assert!(
        project_dir.join("Cargo.toml").exists(),
        "Cargo.toml missing"
    );
    assert!(
        project_dir.join("src").join("main.rs").exists(),
        "src/main.rs missing"
    );
    assert!(project_dir.join("flui.toml").exists(), "flui.toml missing");
}

#[test]
fn create_project_with_local_flag() {
    let tmp = TempDir::new().expect("temp dir");
    let project_dir = tmp.path().join("test-local");

    flui()
        .args([
            "create",
            "test-local",
            "--template",
            "basic",
            "--org",
            "com.test",
            "--local",
        ])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .success();

    // Verify path dependencies in Cargo.toml
    let cargo_toml =
        std::fs::read_to_string(project_dir.join("Cargo.toml")).expect("read Cargo.toml");
    assert!(
        cargo_toml.contains("path ="),
        "Cargo.toml should contain path dependencies when --local is used"
    );
}

#[test]
fn create_project_with_platforms() {
    let tmp = TempDir::new().expect("temp dir");
    let project_dir = tmp.path().join("test-plats");

    flui()
        .args([
            "create",
            "test-plats",
            "--template",
            "basic",
            "--org",
            "com.test",
            "--platforms",
            "android,web",
        ])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .success();

    // Verify platform directories were scaffolded
    assert!(
        project_dir.join("platforms").join("android").is_dir(),
        "platforms/android/ missing"
    );
    assert!(
        project_dir.join("platforms").join("web").is_dir(),
        "platforms/web/ missing"
    );

    // Verify flui.toml contains the platforms
    let flui_toml = std::fs::read_to_string(project_dir.join("flui.toml")).expect("read flui.toml");
    assert!(
        flui_toml.contains("android"),
        "flui.toml should list android"
    );
    assert!(flui_toml.contains("web"), "flui.toml should list web");
}

#[test]
fn create_project_default_template_is_counter() {
    let tmp = TempDir::new().expect("temp dir");
    let project_dir = tmp.path().join("test-default");

    flui()
        .args(["create", "test-default", "--org", "com.test"])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .success();

    // Default template is counter — src/main.rs should exist
    assert!(project_dir.join("src").join("main.rs").exists());
}

#[test]
fn create_project_duplicate_name_fails() {
    let tmp = TempDir::new().expect("temp dir");

    // First creation should succeed
    flui()
        .args(["create", "test-dup", "--org", "com.test"])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .success();

    // Second creation with same name should fail
    flui()
        .args(["create", "test-dup", "--org", "com.test"])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}
