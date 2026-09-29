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

/// The workspace's target directory as Cargo itself resolves it —
/// `CARGO_TARGET_DIR`, a configured `build.target-dir`, or `<root>/target` —
/// so the template checks' separate cache (a subdirectory of it, never the
/// directory itself) follows a relocated target instead of rebuilding from
/// cold inside every checkout.
fn workspace_target_dir(root: &Path) -> PathBuf {
    cargo_metadata::MetadataCommand::new()
        .current_dir(root)
        .no_deps()
        .other_options(vec!["--offline".to_string()])
        .exec()
        .expect("run cargo metadata for the workspace target directory")
        .target_directory
        .into_std_path_buf()
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
    let target = workspace_target_dir(&root);
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
        // The app and its tests live in the library; main.rs only mounts it.
        let source = std::fs::read_to_string(project.join("src/lib.rs")).expect("generated source");
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
    // A template names exactly the packages and features its source uses
    // (ADR-0088 §6). None uses a design system today, so none may pull one in
    // -- through a facade default included -- nor test support.
    {
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
        for unused in ["flui-testing ", "flui-material ", "flui-cupertino "] {
            assert!(
                !graph.lines().any(|line| line.starts_with(unused)),
                "`flui create --template {template}` must not depend on `{}`, \
                 which its source does not use:\n{graph}",
                unused.trim_end()
            );
        }
    }
    if template == "counter" {
        run_generated_counter_tests(&cargo, &project, &target);
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
fn run_generated_counter_tests(cargo: &str, project: &Path, target: &Path) {
    // The tests live in the library (so does the app; main.rs only mounts
    // it), and `--lib` is exactly that one target.
    let output = std::process::Command::new(cargo)
        .args(["test", "--offline", "--lib"])
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

#[test]
fn generated_empty_project_compiles() {
    assert_generated_project_compiles("empty");
}

#[test]
fn generated_widget_project_compiles() {
    assert_generated_project_compiles("widget");
}

/// The widget template ships a widget test (`greeting_renders_its_name`); a
/// bare `cargo check` would let that test rot silently — compiled once,
/// never executed. This runs the generated library's own test binary and
/// requires that test to actually execute and pass, the same way
/// `run_generated_counter_tests` does for the counter template.
#[test]
fn generated_widget_project_test_passes() {
    let root = repo_root();
    let target = workspace_target_dir(&root);
    let name = "flui-tmpl-check-widget-test";
    let output_dir = TempDir::new().expect("external output directory");
    let project = output_dir.path().join(name);

    flui()
        .current_dir(&root)
        .env("CARGO_NET_OFFLINE", "true")
        .args([
            "create",
            name,
            "--template",
            "widget",
            "--org",
            "com.test",
            "--local",
            "--no-check",
        ])
        .arg("--path")
        .arg(output_dir.path())
        .assert()
        .success();

    assert!(
        !project.join("src/main.rs").exists(),
        "a widget template must not ship a main.rs"
    );

    std::fs::copy(root.join("Cargo.lock"), project.join("Cargo.lock"))
        .expect("seed the generated project with the workspace's resolved versions");

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = std::process::Command::new(&cargo)
        .args(["test", "--offline"])
        .arg("--target-dir")
        .arg(target.join("cli-template-check"))
        .args(["--", "--nocapture"])
        .current_dir(&project)
        .output()
        .expect("run the generated widget's test binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("test tests::greeting_renders_its_name ... ok"),
        "the generated widget's own test did not execute and pass: {stdout}"
    );
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
fn dry_run_writes_nothing_and_lists_key_files() {
    let tmp = TempDir::new().expect("temp dir");
    let project_dir = tmp.path().join("dry-app");

    flui()
        .args([
            "create",
            "dry-app",
            "--org",
            "com.test",
            "--template",
            "basic",
            "--dry-run",
        ])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Cargo.toml"))
        .stderr(
            predicate::str::contains("src/main.rs").or(predicate::str::contains("src\\main.rs")),
        );

    assert!(
        !project_dir.exists(),
        "--dry-run must not create the project directory"
    );
}

#[test]
fn json_create_stdout_is_pure_ndjson_ending_in_create_done() {
    let tmp = TempDir::new().expect("temp dir");

    let output = flui()
        .args([
            "create",
            "json-app",
            "--org",
            "com.test",
            "--no-check",
            "--json",
        ])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).expect("utf8 stdout");

    let events: Vec<serde_json::Value> = stdout
        .lines()
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|e| panic!("not NDJSON: {line}: {e}"))
        })
        .collect();
    assert!(!events.is_empty(), "no JSON events emitted");
    assert_eq!(
        events.last().expect("at least one event")["event"],
        "create.done",
        "stdout must end in create.done: {stdout}"
    );
    assert_eq!(
        events.first().expect("at least one event")["event"],
        "create.start"
    );
}
