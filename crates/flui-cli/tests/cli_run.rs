//! Project admission uses real Cargo metadata; fixture binaries only print a marker.
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::path::Path;
use tempfile::TempDir;

fn package(dir: &Path, name: &str, extra: &str) {
    std::fs::create_dir_all(dir.join("src")).expect("source directory");
    std::fs::write(
        dir.join("Cargo.toml"),
        format!("[package]\nname = {name:?}\nversion = \"0.1.0\"\nedition = \"2024\"\n{extra}"),
    )
    .expect("manifest");
    std::fs::write(
        dir.join("src/main.rs"),
        "fn main() { println!(\"FLUI_ADMISSION_MARKER\"); }\n",
    )
    .expect("main");
}

fn run(dir: &Path) -> assert_cmd::Command {
    let mut command = cargo_bin_cmd!("flui");
    command
        .current_dir(dir)
        .env("CARGO_NET_OFFLINE", "true")
        .args(["run", "--release", "--device", "desktop"]);
    command
}

#[test]
fn sole_facade_project_runs_its_binary() {
    let tmp = TempDir::new().expect("temporary fixture");
    let dependency = tmp.path().join("facade");
    package(&dependency, "flui", "");
    std::fs::write(dependency.join("src/lib.rs"), "").expect("identity fixture library");
    let app = tmp.path().join("app");
    package(
        &app,
        "app",
        "[workspace]\n[dependencies]\nflui = { path = \"../facade\" }\n",
    );
    run(&app)
        .assert()
        .success()
        .stdout(predicate::str::contains("FLUI_ADMISSION_MARKER"));
}

#[test]
fn renamed_inherited_target_dependency_admits_only_its_application() {
    let tmp = TempDir::new().expect("workspace fixture");
    let root = tmp.path();
    std::fs::write(root.join("Cargo.toml"), "[workspace]\nresolver = '3'\nmembers = ['app', 'other', 'facade']\n[workspace.dependencies]\nui = { package = 'flui', path = 'facade' }\n").expect("workspace");
    package(&root.join("facade"), "flui", "");
    std::fs::write(root.join("facade/src/lib.rs"), "").expect("fixture library");
    package(
        &root.join("app"),
        "app",
        "[target.'cfg(all())'.dependencies]\nui.workspace = true\n",
    );
    package(&root.join("other"), "other", "");
    run(&root.join("app"))
        .assert()
        .success()
        .stdout(predicate::str::contains("FLUI_ADMISSION_MARKER"));
    run(&root.join("other"))
        .assert()
        .failure()
        .stderr(predicate::str::contains("normal dependency"));
    run(root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("virtual workspace root"));
}

#[test]
fn comments_prefixes_and_non_normal_dependencies_do_not_admit_projects() {
    for (name, kind) in [
        ("flui", "dev-dependencies"),
        ("flui", "build-dependencies"),
        ("flui-app-helper", "dependencies"),
    ] {
        let tmp = TempDir::new().expect("fixture");
        package(&tmp.path().join("dependency"), name, "");
        std::fs::write(tmp.path().join("dependency/src/lib.rs"), "").expect("fixture library");
        let app = tmp.path().join("app");
        package(
            &app,
            "app",
            &format!(
                "[workspace]\n# flui-app and flui-widgets are not dependencies\n[{kind}]\nui = {{ package = {name:?}, path = '../dependency' }}\n"
            ),
        );
        run(&app)
            .assert()
            .failure()
            .stderr(predicate::str::contains("normal dependency"));
    }
}

#[test]
fn cargo_metadata_failure_keeps_cargo_diagnostics() {
    let tmp = TempDir::new().expect("fixture");
    package(
        tmp.path(),
        "app",
        "[dependencies]\nui = { workspace = true }\n",
    );
    run(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("cargo metadata failed"))
        .stderr(predicate::str::contains("workspace"));
}

#[test]
fn legacy_dependency_alias_remains_supported() {
    let tmp = TempDir::new().expect("fixture");
    package(&tmp.path().join("dependency"), "flui-widgets", "");
    std::fs::write(tmp.path().join("dependency/src/lib.rs"), "").expect("fixture library");
    let app = tmp.path().join("app");
    package(
        &app,
        "app",
        "[workspace]\n[dependencies]\nwidgets = { package = 'flui-widgets', path = '../dependency' }\n",
    );
    run(&app)
        .assert()
        .success()
        .stdout(predicate::str::contains("FLUI_ADMISSION_MARKER"));
}

#[test]
fn unavailable_simulator_never_runs_the_host_application() {
    let tmp = TempDir::new().expect("simulator routing fixture");
    let dependency = tmp.path().join("facade");
    package(&dependency, "flui", "");
    std::fs::write(dependency.join("src/lib.rs"), "").expect("facade identity");
    let app = tmp.path().join("app");
    package(
        &app,
        "simulator-routing-app",
        "[workspace]\n[dependencies]\nflui = { path = \"../facade\" }\n",
    );
    let mut command = cargo_bin_cmd!("flui");
    command
        .current_dir(&app)
        .env("CARGO_NET_OFFLINE", "true")
        .timeout(std::time::Duration::from_secs(30))
        .args([
            "run",
            "--release",
            "--device",
            "00000000-0000-0000-0000-000000000000",
        ]);
    let output = command.output().expect("bounded CLI invocation");
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("FLUI_ADMISSION_MARKER"),
        "a simulator request executed the host binary: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(!output.status.success(), "unavailable simulator must fail");
}

#[test]
fn conflicting_ios_delivery_modes_fail_before_cargo() {
    let empty = TempDir::new().expect("empty directory");
    for arguments in [
        vec!["build", "ios", "--universal"],
        vec!["build", "ios", "--lib", "--example", "demo"],
        vec!["build", "ios", "--universal", "--simulator", "chosen"],
        vec!["build", "desktop", "--lib"],
        vec!["build", "desktop", "--simulator", "chosen"],
    ] {
        let output = cargo_bin_cmd!("flui")
            .current_dir(empty.path())
            .args(&arguments)
            .output()
            .expect("CLI");
        assert!(!output.status.success(), "{arguments:?}");
        let diagnostic = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !diagnostic.contains("could not find `Cargo.toml`"),
            "must validate selectors before Cargo: {diagnostic}"
        );
        assert!(
            diagnostic.contains("conflict")
                || diagnostic.contains("cannot be used")
                || diagnostic.contains("iOS-only"),
            "{diagnostic}"
        );
    }
}
