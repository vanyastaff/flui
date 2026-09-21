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
    let mut command = cargo_bin_cmd!("flui");
    command.current_dir(repo_root());
    command
}

/// Workspace root — this crate lives at `<root>/crates/flui-cli`.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("BUG: flui-cli must sit two levels below the workspace root")
        .canonicalize()
        .expect("canonical workspace root")
}

/// Generate a project with `--local` and prove it actually compiles.
///
/// This is the real gate on the templates: the file-existence tests below pass
/// just as happily on a template that emits a long-deleted API. Anything that
/// drifts the generated `main.rs` or `Cargo.toml` off the current public
/// surface fails here.
///
/// The check gets its own `--target-dir`: reusing the workspace's would
/// deadlock, since the outer `cargo test` holds that directory's build lock for
/// the duration of the run.
fn assert_generated_project_compiles(template: &str) {
    let root = repo_root();
    let target = root.join("target");
    let name = format!("flui-tmpl-check-{template}");
    let output_dir = TempDir::new().expect("external output directory");
    let project = output_dir.path().join(&name);

    // The explicit check below is the compile oracle; skip the CLI's advisory
    // check so it cannot resolve a fresh graph or build into a private target.
    flui()
        .current_dir(&root)
        .env("CARGO_NET_OFFLINE", "true")
        .args([
            "create",
            &name,
            "--template",
            template,
            "--org",
            "com.test",
            "--local",
            "--no-check",
        ])
        .arg("--path")
        .arg(output_dir.path())
        .assert()
        .success();

    if template == "counter" {
        let source =
            std::fs::read_to_string(project.join("src/main.rs")).expect("generated source");
        for test in GENERATED_COUNTER_TESTS {
            let function = test
                .rsplit("::")
                .next()
                .expect("BUG: a test name always has a trailing segment");
            assert!(
                source.contains(&format!("fn {function}")),
                "the generated counter must ship `{function}`, its `{test}` regression test"
            );
        }
        let manifest: toml::Table = std::fs::read_to_string(project.join("Cargo.toml"))
            .expect("manifest")
            .parse()
            .expect("valid TOML");
        assert_eq!(
            manifest["dev-dependencies"]["flui"]["path"],
            manifest["dependencies"]["flui"]["path"]
        );
        assert_eq!(
            manifest["dev-dependencies"]["flui"]["features"]
                .as_array()
                .expect("features"),
            &[toml::Value::String("testing".into())]
        );
        assert!(manifest["dependencies"]["flui"].get("features").is_none());
    }

    // Reuse this checkout's resolved dependency graph and cached crates. The
    // generated package is added to the copied lock by Cargo; --offline prevents
    // this test from depending on registry availability or fresh releases.
    std::fs::copy(root.join("Cargo.lock"), project.join("Cargo.lock"))
        .expect("seed the generated project with the workspace's resolved versions");

    // Template checks share a separate target so the enclosing cargo test's
    // build lock cannot deadlock them. External output paths exercise the same
    // dependency resolution users get outside the framework checkout.
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = std::process::Command::new(&cargo)
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
    if template == "counter" {
        let output = std::process::Command::new(&cargo)
            .args([
                "tree",
                "--offline",
                "--edges",
                "normal,build",
                "--prefix",
                "none",
                "--format",
                "{p}",
            ])
            .current_dir(&project)
            .output()
            .expect("normal dependency graph");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let graph = String::from_utf8_lossy(&output.stdout);
        assert!(
            !graph.lines().any(|line| line.starts_with("flui-testing ")),
            "{graph}"
        );

        run_generated_counter_tests(&cargo, &project, &name, &target);
    }
}

/// Every test the generated counter is contracted to ship, by name.
///
/// The whole binary runs and each name must appear as passed, so a test that
/// exists in the template but is not listed here cannot hide: the run's own
/// `test result` line is asserted against this list's length. That count is why
/// the list has to be extended whenever the template gains a test — and why a
/// generated test that silently never executed, which is how a layout
/// regression once passed this harness, now fails it.
const GENERATED_COUNTER_TESTS: &[&str] = &[
    "tests::counter_responds_to_pointer_input",
    "tests::counter_content_is_centred",
];

/// Run the generated counter's own test binary and require every test in it.
///
/// The binary runs whole — no `--exact`, no test-name filter — so the harness
/// cannot pin itself to a subset of the template's tests and report a pass for
/// the rest.
fn run_generated_counter_tests(cargo: &str, project: &Path, name: &str, target: &Path) {
    let output = std::process::Command::new(cargo)
        .args(["test", "--offline", "--bin", name])
        .arg("--target-dir")
        .arg(target.join("cli-template-check"))
        .args(["--", "--nocapture"])
        .current_dir(project)
        .output()
        .expect("generated counter regression suite");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for test in GENERATED_COUNTER_TESTS {
        assert!(
            stdout.contains(&format!("test {test} ... ok")),
            "generated test `{test}` did not execute and pass: {stdout}"
        );
    }
    // The test binary is the only thing this invocation runs, so its result
    // line is the template's test count. A `#[test]` added to the template
    // without being added to GENERATED_COUNTER_TESTS lands here.
    let expected = format!(
        "test result: ok. {} passed; 0 failed",
        GENERATED_COUNTER_TESTS.len()
    );
    assert!(
        stdout.contains(&expected),
        "the generated counter did not run exactly the {} tests this harness \
         accounts for (`{expected}` absent): {stdout}",
        GENERATED_COUNTER_TESTS.len()
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

/// The hot-reload template emits a three-crate *workspace*, and the real gate
/// is that `cargo check --workspace` passes on it — the same "file existence is
/// not enough" rule the single-crate templates follow. The workspace layout is
/// what makes the host/worker split compile at all (each member resolves its
/// siblings by `path`), so a drift there is caught here, not in production.
#[test]
fn generated_hot_reload_workspace_compiles() {
    let root = repo_root();
    let target = root.join("target");
    let name = "flui-tmpl-check-hot-reload";
    let output_dir = TempDir::new().expect("external output directory");
    let project = output_dir.path().join(name);

    flui()
        .current_dir(&root)
        .env("CARGO_NET_OFFLINE", "true")
        .args([
            "create",
            name,
            "--template",
            "counter",
            "--org",
            "com.test",
            "--local",
            "--hot-reload",
            "--no-check",
        ])
        .arg("--path")
        .arg(output_dir.path())
        .assert()
        .success();

    // The workspace must be laid out exactly as the reload protocol expects.
    for member in [
        format!("{name}-types"),
        format!("{name}-logic"),
        format!("{name}-host"),
    ] {
        assert!(
            project.join(&member).join("Cargo.toml").exists(),
            "hot-reload workspace member {member} missing",
        );
    }
    let flui_toml = std::fs::read_to_string(project.join("flui.toml")).expect("read flui.toml");
    assert!(
        flui_toml.contains("[hot_reload]"),
        "flui.toml must carry the [hot_reload] section `flui run` reads",
    );

    std::fs::copy(root.join("Cargo.lock"), project.join("Cargo.lock"))
        .expect("seed the generated workspace with the workspace's resolved versions");

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = std::process::Command::new(cargo)
        .args(["check", "--workspace", "--offline"])
        .arg("--target-dir")
        .arg(target.join("cli-template-check"))
        .current_dir(&project)
        .output()
        .expect("run cargo check on the generated hot-reload workspace");

    assert!(
        output.status.success(),
        "`flui create --hot-reload` generated a workspace that does not compile:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// `git init` lands in the generated project, not in the directory the CLI
/// was run from. Before the fix, `flui create` initialised a repository in
/// the caller's working directory — a test run left an empty `.git` inside
/// `crates/flui-cli` itself, which made `cargo package` refuse every file
/// in this crate as uncommitted.
#[test]
fn create_project_initialises_git_in_the_project_not_the_caller_directory() {
    let tmp = TempDir::new().expect("temp dir");
    let caller = TempDir::new().expect("caller directory");
    let project_dir = tmp.path().join("test-git-placement");

    let mut command = cargo_bin_cmd!("flui");
    command
        .current_dir(caller.path())
        .args([
            "create",
            "test-git-placement",
            "--template",
            "basic",
            "--org",
            "com.test",
            "--no-check",
        ])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .success();

    assert!(
        project_dir.join(".git").is_dir(),
        "the generated project must be a git repository (or git is absent on this host)"
    );
    assert!(
        !caller.path().join(".git").exists(),
        "the caller's working directory must not become a repository"
    );
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
            "--no-check",
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
            "--no-check",
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
            "--no-check",
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
            "--no-check",
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
        .args(["create", "test-default", "--org", "com.test", "--no-check"])
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
        .args(["create", "test-dup", "--org", "com.test", "--no-check"])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .success();

    // Second creation with same name should fail
    flui()
        .args(["create", "test-dup", "--org", "com.test", "--no-check"])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn local_source_resolves_outside_checkout() {
    let tmp = TempDir::new().expect("temp dir");
    flui()
        .current_dir(repo_root())
        .args(["create", "external-app", "--local", "--no-check"])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .success();
    let manifest: toml::Table = std::fs::read_to_string(tmp.path().join("external-app/Cargo.toml"))
        .expect("manifest")
        .parse()
        .expect("valid TOML");
    let dependency = manifest["dependencies"]["flui"]["path"]
        .as_str()
        .expect("path dependency");
    assert_eq!(Path::new(dependency), repo_root());
}

fn assert_manifest_source(project: &Path, source: &Path) {
    let manifest: toml::Table = std::fs::read_to_string(project.join("Cargo.toml"))
        .expect("manifest")
        .parse()
        .expect("valid TOML");
    assert_eq!(
        Path::new(
            manifest["dependencies"]["flui"]["path"]
                .as_str()
                .expect("path")
        ),
        source
    );
}

#[test]
fn explicit_source_accepts_absolute_and_relative_paths() {
    let tmp = TempDir::new().expect("temp dir");
    let source = tmp.path().join("source with 'single' quotes");
    std::fs::create_dir(&source).expect("source dir");
    // A real directory (not a symlink) keeps unusual characters after canonicalization.
    std::fs::copy(repo_root().join("Cargo.toml"), source.join("Cargo.toml"))
        .expect("root manifest");
    for name in ["flui-app", "flui-view", "flui-widgets"] {
        let dir = source.join("crates").join(name);
        std::fs::create_dir_all(&dir).expect("crate dir");
        std::fs::copy(
            repo_root().join("crates").join(name).join("Cargo.toml"),
            dir.join("Cargo.toml"),
        )
        .expect("crate manifest");
    }
    for (name, path) in [
        ("absolute", source.clone()),
        ("relative", PathBuf::from("source with 'single' quotes")),
    ] {
        let mut flag = std::ffi::OsString::from("--local=");
        flag.push(&path);
        flui()
            .current_dir(tmp.path())
            .args(["create", name, "--no-check"])
            .arg(flag)
            .assert()
            .success();
        assert_manifest_source(
            &tmp.path().join(name),
            &source.canonicalize().expect("source root"),
        );
    }
}

#[test]
fn bare_local_before_project_name_uses_current_directory() {
    let tmp = TempDir::new().expect("temp dir");
    flui()
        .args(["create", "--local", "before", "--no-check", "--path"])
        .arg(tmp.path())
        .assert()
        .success();
    assert_manifest_source(&tmp.path().join("before"), &repo_root());
}

#[test]
fn invalid_local_source_does_not_create_output() {
    let tmp = TempDir::new().expect("temp dir");
    for source in [tmp.path().to_path_buf(), tmp.path().join("missing")] {
        let mut flag = std::ffi::OsString::from("--local=");
        flag.push(source);
        flui()
            .args(["create", "invalid", "--no-check", "--path"])
            .arg(tmp.path())
            .arg(flag)
            .assert()
            .failure()
            .stderr(predicate::str::contains("FLUI"));
        assert!(!tmp.path().join("invalid").exists());
    }
}

#[test]
fn registry_source_remains_versioned() {
    let tmp = TempDir::new().expect("temp dir");
    flui()
        .args(["create", "registry-app", "--no-check", "--path"])
        .arg(tmp.path())
        .assert()
        .success();
    let manifest: toml::Table = std::fs::read_to_string(tmp.path().join("registry-app/Cargo.toml"))
        .expect("manifest")
        .parse()
        .expect("TOML");
    assert_eq!(
        manifest["dependencies"]["flui"].as_str(),
        Some(env!("CARGO_PKG_VERSION"))
    );
}

#[cfg(unix)]
#[test]
fn local_source_preserves_quotes_and_backslashes() {
    let tmp = TempDir::new().expect("temp dir");
    let source = tmp.path().join("a \"quoted\" \\ source");
    std::fs::create_dir(&source).expect("source dir");
    std::fs::copy(repo_root().join("Cargo.toml"), source.join("Cargo.toml"))
        .expect("root manifest");
    for name in ["flui-app", "flui-view", "flui-widgets"] {
        let dir = source.join("crates").join(name);
        std::fs::create_dir_all(&dir).expect("crate dir");
        std::fs::copy(
            repo_root().join("crates").join(name).join("Cargo.toml"),
            dir.join("Cargo.toml"),
        )
        .expect("crate manifest");
    }
    let mut flag = std::ffi::OsString::from("--local=");
    flag.push(&source);
    flui()
        .current_dir(tmp.path())
        .args(["create", "escaped", "--no-check"])
        .arg(flag)
        .assert()
        .success();
    assert_manifest_source(
        &tmp.path().join("escaped"),
        &source.canonicalize().expect("canonical source"),
    );
}

#[cfg(unix)]
#[test]
fn non_utf8_source_is_rejected_without_output() {
    use std::os::unix::ffi::OsStringExt;
    let tmp = TempDir::new().expect("temp dir");
    let source = tmp
        .path()
        .join(std::ffi::OsString::from_vec(vec![b's', 0xff]));
    let mut flag = std::ffi::OsString::from("--local=");
    flag.push(source);
    flui()
        .args(["create", "invalid", "--no-check", "--path"])
        .arg(tmp.path())
        .arg(flag)
        .assert()
        .failure()
        .stderr(predicate::str::contains("UTF-8"));
    assert!(!tmp.path().join("invalid").exists());
}

#[test]
fn incomplete_local_checkout_is_rejected_without_output() {
    let tmp = TempDir::new().expect("temp dir");
    std::fs::copy(
        repo_root().join("Cargo.toml"),
        tmp.path().join("Cargo.toml"),
    )
    .expect("root manifest");
    flui()
        .current_dir(tmp.path())
        .args(["create", "incomplete", "--local", "--no-check"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("flui-app"));
    assert!(!tmp.path().join("incomplete").exists());
}

#[test]
fn hot_reload_requires_its_additional_source_manifests() {
    let tmp = TempDir::new().expect("temp dir");
    std::fs::copy(
        repo_root().join("Cargo.toml"),
        tmp.path().join("Cargo.toml"),
    )
    .expect("root manifest");
    for name in [
        "flui-app",
        "flui-view",
        "flui-widgets",
        "flui-hot-reload",
        "flui-types",
    ] {
        if matches!(name, "flui-hot-reload" | "flui-types") {
            flui()
                .current_dir(tmp.path())
                .args([
                    "create",
                    "incomplete",
                    "--local",
                    "--hot-reload",
                    "--no-check",
                ])
                .assert()
                .failure()
                .stderr(predicate::str::contains(name));
            assert!(!tmp.path().join("incomplete").exists());
        }
        let dir = tmp.path().join("crates").join(name);
        std::fs::create_dir_all(&dir).expect("crate directory");
        std::fs::copy(
            repo_root().join("crates").join(name).join("Cargo.toml"),
            dir.join("Cargo.toml"),
        )
        .expect("crate manifest");
    }
}

#[test]
fn all_templates_depend_on_the_public_facade_only() {
    let tmp = TempDir::new().expect("temp dir");
    for (name, template, hot_reload) in [
        ("basic", "basic", false),
        ("counter", "counter", false),
        ("reload", "counter", true),
        ("flui-reload", "counter", true),
    ] {
        let mut command = flui();
        command
            .args([
                "create",
                name,
                "--template",
                template,
                "--local",
                "--no-check",
                "--path",
            ])
            .arg(tmp.path());
        if hot_reload {
            command.arg("--hot-reload");
        }
        command.assert().success();
        let members = if hot_reload {
            vec![
                format!("{name}-types"),
                format!("{name}-logic"),
                format!("{name}-host"),
            ]
        } else {
            vec![String::new()]
        };
        for member in members {
            let path = tmp.path().join(name).join(&member).join("Cargo.toml");
            let manifest: toml::Table = std::fs::read_to_string(path)
                .expect("manifest")
                .parse()
                .expect("TOML");
            let dependencies = manifest["dependencies"].as_table().expect("dependencies");
            let types_package = format!("{name}-types");
            let mut expected = vec!["flui"];
            if hot_reload && member != types_package {
                expected.push(&types_package);
            }
            if hot_reload && member == format!("{name}-host") {
                expected.push("tracing");
            }
            expected.sort_unstable();
            assert_eq!(
                dependency_identities(dependencies),
                expected,
                "dependency packages for {name}/{member}"
            );
            let facade = &dependencies["flui"];
            assert_eq!(
                Path::new(facade["path"].as_str().expect("facade source")),
                repo_root()
            );
            if hot_reload && member != types_package {
                assert_eq!(
                    dependencies[&types_package]["path"].as_str(),
                    Some(format!("../{types_package}").as_str())
                );
            }
            if hot_reload {
                assert!(
                    facade["features"]
                        .as_array()
                        .expect("features")
                        .iter()
                        .any(|feature| feature.as_str() == Some("hot-reload"))
                );
            }
        }
    }
}

fn dependency_identities(dependencies: &toml::Table) -> Vec<&str> {
    let mut packages: Vec<_> = dependencies
        .iter()
        .map(|(name, value)| {
            value
                .as_table()
                .and_then(|table| table.get("package"))
                .map_or(name.as_str(), |package| {
                    package.as_str().expect("Cargo package name")
                })
        })
        .collect();
    packages.sort_unstable();
    packages
}

#[test]
fn dependency_guard_detects_renamed_internal_packages() {
    let mut dependencies: toml::Table = "flui = '0.2.0'".parse().expect("dependency table");
    assert_eq!(dependency_identities(&dependencies), ["flui"]);
    let mutation: toml::Table = "package = 'flui-view'\nversion = '0.2.0'"
        .parse()
        .expect("renamed dependency");
    dependencies.insert("views".into(), mutation.into());
    assert_eq!(dependency_identities(&dependencies), ["flui", "flui-view"]);
    assert_ne!(dependency_identities(&dependencies), ["flui"]);
}
