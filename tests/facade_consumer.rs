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

/// The facade at `root`, without its defaults, with `features` on.
fn facade_with_features(root: &Path, features: &[&str]) -> toml::Value {
    let mut framework = dependency("flui", root, false);
    framework
        .as_table_mut()
        .expect("framework dependency table")
        .insert(
            "features".into(),
            toml::Value::Array(features.iter().map(|&feature| feature.into()).collect()),
        );
    framework
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
    let project = consumer_project(dependencies, dev_dependencies, source);
    run_project(&project, command)
}

fn run_project(project: &tempfile::TempDir, command: &str) -> Output {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
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
        // The facade turns no design system on by default (ADR-0088 §6): a
        // consumer that names no feature gets neither catalog.
        for catalog in ["flui-material ", "flui-cupertino "] {
            assert!(
                !graph.lines().any(|line| line.starts_with(catalog)),
                "`{}` in default={defaults} normal graph:\n{graph}",
                catalog.trim_end()
            );
        }
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
        // Persisted documents bring no serialization crate: an application
        // encodes its documents with its own.
        assert!(
            !graph.lines().any(|line| line.starts_with("serde_json ")),
            "`serde_json` in default={defaults} normal graph:\n{graph}"
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
        dev_dependencies.insert(alias.into(), facade_with_features(root, &["testing"]));
        let source = rename_facade(include_str!("fixtures/facade_extensions.rs"), alias);
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
            "nested_custom_motion_repaints_and_retargets_through_the_facade",
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

/// The acceptance application, `tests/fixtures/acceptance_app/`, runs its rows
/// as a crate that depends on the facade alone, with `material` for the
/// application and `testing` for its rows. Every `#[test]` in the directory
/// must report `... ok`, so a row added there is required here without a
/// second list.
#[test]
fn acceptance_app_runs_through_the_facade() {
    let Some(root) = checkout_root() else { return };
    let table = |features: &[&str]| {
        let mut table = toml::Table::new();
        table.insert("flui".into(), facade_with_features(root, features));
        table
    };
    let project = consumer_project(
        table(&["material"]),
        Some(table(&["material", "testing"])),
        "",
    );
    let fixture = root.join("tests/fixtures/acceptance_app");
    copy_tree(&fixture, &project.path().join("src"));
    let rows = test_names(&fixture);
    assert!(rows.len() > 1, "no acceptance rows found in {fixture:?}");

    let output = run_project(&project, "test");
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "acceptance application:\n{report}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for row in rows {
        assert!(
            report.contains(&format!("{row} ... ok")),
            "missing executed acceptance row {row}:\n{report}"
        );
    }
}

/// Copy every file under `from` to the same place under `to`.
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("create a consumer source directory");
    for entry in std::fs::read_dir(from).expect("read a fixture directory") {
        let entry = entry.expect("fixture entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("fixture entry type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("copy a fixture file");
        }
    }
}

/// The name of each `#[test]` function in the `.rs` files under `directory`:
/// the `fn` that follows a `#[test]` line, past other attributes.
fn test_names(directory: &Path) -> Vec<String> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(directory).expect("read a fixture directory") {
        let path = entry.expect("fixture entry").path();
        if path.is_dir() {
            names.extend(test_names(&path));
            continue;
        }
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("read a fixture source");
        let mut after_test = false;
        for line in source.lines().map(str::trim) {
            if line == "#[test]" {
                after_test = true;
            } else if after_test && let Some(signature) = line.strip_prefix("fn ") {
                let name = signature
                    .split(['(', '<'])
                    .next()
                    .expect("BUG: split yields at least one piece");
                names.push(name.to_owned());
                after_test = false;
            }
        }
    }
    names
}

/// `source` as a consumer that names the facade `alias` writes it.
///
/// Rewrites each `flui::` that starts a path: one not preceded by an
/// identifier character or `:`, so `my_flui::x` and `a::flui::x` are left
/// alone. A comment or string literal naming `flui::` would be rewritten
/// too; the fixtures name the facade only in code.
fn rename_facade(source: &str, alias: &str) -> String {
    let mut renamed = String::with_capacity(source.len());
    let mut copied = 0;
    for (at, _) in source.match_indices("flui::") {
        let starts_path = !source[..at]
            .chars()
            .next_back()
            .is_some_and(|before| before.is_alphanumeric() || before == '_' || before == ':');
        if starts_path {
            renamed.push_str(&source[copied..at]);
            renamed.push_str(alias);
            renamed.push_str("::");
            copied = at + "flui::".len();
        }
    }
    renamed.push_str(&source[copied..]);
    renamed
}

/// `None` when there is no checkout to depend on — see [`checkout_root`].
fn hot_reload_dependencies(include_layer: bool) -> Option<toml::Table> {
    let root = checkout_root()?;
    let mut dependencies = toml::Table::new();
    dependencies.insert("flui".into(), facade_with_features(root, &["hot-reload"]));
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
