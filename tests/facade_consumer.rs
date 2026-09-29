//! Compile actual downstream packages: this package's own tests can see its
//! implementation dependencies and cannot prove facade-only macro hygiene.
//!
//! Every test here points its consumer at the workspace checkout's `crates/`
//! by path. The published `flui` archive ships this file (its `include`
//! list carries `/tests/**`) but not `crates/`, so from an unpacked archive
//! each test reports itself skipped through [`checkout_root`] rather than
//! failing on a path that was never meant to exist there.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The workspace checkout these consumer projects depend on by path, or
/// `None` — with the reason on stderr — when this test runs from somewhere
/// without one (an unpacked crates.io archive).
fn checkout_root() -> Option<&'static Path> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    if root.join("crates").is_dir() {
        Some(root)
    } else {
        eprintln!(
            "skipped: {} has no `crates/` directory, so there is no workspace checkout for \
             the consumer project to depend on (running from a packaged archive?)",
            root.display()
        );
        None
    }
}

/// The workspace's target directory as Cargo itself resolves it —
/// `CARGO_TARGET_DIR`, a configured `build.target-dir`, or `<root>/target` —
/// so the consumer checks' separate cache (a subdirectory of it, never the
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

fn dependency(package: &str, path: &Path, defaults: bool) -> toml::Value {
    let mut value = toml::Table::new();
    value.insert("package".into(), package.into());
    value.insert("path".into(), path.to_str().expect("UTF-8 checkout").into());
    value.insert("default-features".into(), defaults.into());
    value.into()
}

fn consumer_project(
    dependencies: toml::Table,
    dev_dependencies: Option<toml::Table>,
    source: &str,
) -> tempfile::TempDir {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let project = tempfile::tempdir().expect("create external consumer directory");
    std::fs::create_dir(project.path().join("src")).expect("create source directory");
    let mut package = toml::Table::new();
    package.insert("name".into(), "facade-consumer".into());
    package.insert("version".into(), "0.1.0".into());
    package.insert("edition".into(), "2024".into());
    let mut manifest = toml::Table::new();
    manifest.insert("package".into(), package.into());
    manifest.insert("workspace".into(), toml::Table::new().into());
    manifest.insert("dependencies".into(), dependencies.into());
    if let Some(dev_dependencies) = dev_dependencies {
        manifest.insert("dev-dependencies".into(), dev_dependencies.into());
    }
    std::fs::write(
        project.path().join("Cargo.toml"),
        toml::to_string(&manifest).expect("serialize consumer manifest"),
    )
    .expect("write consumer manifest");
    std::fs::write(project.path().join("src/lib.rs"), source).expect("write consumer source");
    // Resolve the new root package offline against versions already locked by
    // FLUI, rather than accidentally testing newer registry releases.
    std::fs::copy(root.join("Cargo.lock"), project.path().join("Cargo.lock"))
        .expect("seed consumer lockfile");
    project
}

fn run_consumer(
    dependencies: toml::Table,
    dev_dependencies: Option<toml::Table>,
    source: &str,
    command: &str,
) -> Output {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let project = consumer_project(dependencies, dev_dependencies, source);
    // Never reuse the outer Cargo target: its build lock remains held during
    // integration tests. Both cases share this separate cache, serialized by Cargo.
    Command::new(env!("CARGO"))
        .args([command, "--offline", "--all-targets", "--manifest-path"])
        .arg(project.path().join("Cargo.toml"))
        .arg("--target-dir")
        .arg(workspace_target_dir(root).join("facade-consumer-check"))
        .current_dir(project.path())
        .output()
        .expect("run consumer Cargo check")
}

fn compile_consumer(dependencies: toml::Table, source: &str) -> Output {
    run_consumer(dependencies, None, source, "check")
}

#[test]
fn ordinary_facade_graph_excludes_test_support() {
    let Some(root) = checkout_root() else { return };
    for defaults in [false, true] {
        let mut dependencies = toml::Table::new();
        dependencies.insert("flui".into(), dependency("flui", root, defaults));
        let project = consumer_project(dependencies, None, "");
        let output = Command::new(env!("CARGO"))
            .args([
                "tree",
                "--offline",
                "--edges",
                "normal",
                "--prefix",
                "none",
                "--format",
                "{p}",
            ])
            .current_dir(project.path())
            .output()
            .expect("inspect normal consumer dependency graph");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let graph = String::from_utf8_lossy(&output.stdout);
        assert!(
            graph.lines().any(|line| line.starts_with("flui v")),
            "{graph}"
        );
        assert!(
            !graph.lines().any(|line| line.starts_with("flui-testing ")),
            "test driver in default={defaults} normal graph:\n{graph}"
        );
        assert!(
            !graph
                .lines()
                .any(|line| line.starts_with("flui-hot-reload ")),
            "hot reload in default={defaults} normal graph:\n{graph}"
        );
    }
}

#[test]
fn external_consumers_extend_and_test_through_the_facade() {
    let Some(root) = checkout_root() else { return };
    for alias in ["flui", "ui"] {
        let mut dependencies = toml::Table::new();
        dependencies.insert(alias.into(), dependency("flui", root, false));
        let mut dev_dependencies = toml::Table::new();
        let mut framework = dependency("flui", root, false);
        framework
            .as_table_mut()
            .expect("framework dependency table")
            .insert(
                "features".into(),
                toml::Value::Array(vec!["testing".into()]),
            );
        dev_dependencies.insert(alias.into(), framework);
        let source =
            include_str!("fixtures/facade_extensions.rs").replace("flui::", &format!("{alias}::"));
        let output = run_consumer(dependencies, Some(dev_dependencies), &source, "test");
        assert!(
            output.status.success(),
            "facade extension alias={alias}:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let report = String::from_utf8_lossy(&output.stdout);
        for case in [
            "custom_painter_records_a_rectangle",
            "custom_render_view_mounts_lays_out_and_paints",
            "proxy_macros_forward_live_and_dry_layout",
            "gesture_recognizer_uses_headless_virtual_time",
            "pointer_input_schedules_a_widget_rebuild",
            "typed_drag_down_callback_is_available_from_the_widget_surface",
            "downstream_custom_recognizer_competes_in_the_arena",
            "lifecycle_capabilities_are_named_and_run_through_the_facade",
            "interaction_callback_vocabulary_is_nameable_through_the_facade",
            "retained_focus_node_uses_context_capabilities_through_the_facade",
            "presentation_lifecycle_subscription_runs_through_the_facade",
            "presentation_lifecycle_capability_is_absent_when_not_installed",
            "secondary_window_entry_point_requires_a_running_application",
        ] {
            assert!(
                report.contains(&format!("{case} ... ok")),
                "{alias}: missing executed acceptance case {case}:\n{report}"
            );
        }
    }
}

/// `None` when there is no checkout to depend on — see [`checkout_root`].
fn hot_reload_dependencies(include_layer: bool) -> Option<toml::Table> {
    let root = checkout_root()?;
    let mut framework = dependency("flui", root, false);
    framework.as_table_mut().expect("dependency table").insert(
        "features".into(),
        toml::Value::Array(vec!["hot-reload".into()]),
    );
    let mut dependencies = toml::Table::new();
    dependencies.insert("flui".into(), framework);
    if include_layer {
        dependencies.insert(
            "flui-layer".into(),
            dependency("flui-layer", &root.join("crates/flui-layer"), false),
        );
    }
    Some(dependencies)
}

const SCENE_PLUGIN_SOURCE: &str = "fn build(_: f64, _: f64) -> flui_layer::Scene { flui_layer::Scene::default() } flui::hot_reload::scene_plugin!(build);";
const APP_PLUGIN_SOURCE: &str =
    "flui::hot_reload::app_plugin!(flui::widgets::Text::new(\"hello\"));";

#[test]
fn plugin_factory_requires_the_canonical_scene() {
    let Some(dependencies) = hot_reload_dependencies(true) else {
        return;
    };
    let output = compile_consumer(
        dependencies,
        "fn build(_: f64, _: f64) -> u8 { 1 } flui::hot_reload::scene_plugin!(build);",
    );
    let diagnostics = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "non-Scene factory compiled");
    assert!(diagnostics.contains("E0308"), "{diagnostics}");
}

fn reject_safe_teardown(source: &str, function: &str) {
    let source = format!(
        "{source}\npub fn invalid_safe_call() {{ {function}(std::ptr::dangling_mut::<u8>().cast()); }}"
    );
    let Some(dependencies) = hot_reload_dependencies(true) else {
        return;
    };
    let output = compile_consumer(dependencies, &source);
    let diagnostics = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "{function} accepted a raw pointer in safe code"
    );
    assert!(
        diagnostics.contains("E0133") && diagnostics.contains(function),
        "{diagnostics}"
    );
}

#[test]
fn plugin_teardown_requires_unsafe() {
    for (source, function) in [
        (SCENE_PLUGIN_SOURCE, "flui_scene_drop"),
        (SCENE_PLUGIN_SOURCE, "flui_scene_free"),
        (APP_PLUGIN_SOURCE, "flui_app_drop"),
        (APP_PLUGIN_SOURCE, "flui_app_free"),
    ] {
        reject_safe_teardown(source, function);
    }
}
