//! Compile actual downstream packages: this package's own tests can see its
//! implementation dependencies and cannot prove facade-only macro hygiene.

use std::path::Path;
use std::process::{Command, Output};

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
        .arg(root.join("target/facade-consumer-check"))
        .current_dir(project.path())
        .output()
        .expect("run consumer Cargo check")
}

fn compile_consumer(dependencies: toml::Table, source: &str) -> Output {
    run_consumer(dependencies, None, source, "check")
}

#[test]
fn ordinary_facade_graph_excludes_test_support() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
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
fn external_consumers_can_name_custom_paint_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut dependencies = toml::Table::new();
    dependencies.insert("flui".into(), dependency("flui", root, false));
    check_consumer(
        dependencies,
        "use flui::painting::{Canvas, CustomPainter};\nuse flui::types::Size;\n#[derive(Debug)] pub struct Painter;\nimpl CustomPainter for Painter { fn paint(&self, _: &mut Canvas, _: Size) {} fn should_repaint(&self, _: &dyn CustomPainter) -> bool { false } fn as_any(&self) -> &dyn std::any::Any { self } }",
        "custom painting without testing feature",
    );
}

#[test]
fn external_consumers_extend_and_test_through_the_facade() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
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

fn check_consumer(dependencies: toml::Table, source: &str, scenario: &str) {
    let output = compile_consumer(dependencies, source);
    assert!(
        output.status.success(),
        "{scenario}:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn missing_runtime_dependency_reports_how_to_fix_the_manifest() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut dependencies = toml::Table::new();
    dependencies.insert(
        "derive_support".into(),
        dependency("flui-macros", &root.join("crates/flui-macros"), false),
    );
    let output = compile_consumer(
        dependencies,
        "#[derive(Debug, derive_support::Diagnosticable)] pub struct Details { pub count: usize }",
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "missing runtime must be rejected");
    assert!(stderr.contains("add `flui` to Cargo.toml"), "{stderr}");
    assert!(!stderr.contains("proc-macro derive panicked"), "{stderr}");
}

#[test]
fn external_consumers_use_only_the_facade_including_when_renamed() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for alias in ["flui", "ui"] {
        for defaults in [false, true] {
            let mut dependencies = toml::Table::new();
            dependencies.insert(alias.into(), dependency("flui", root, defaults));
            let source = include_str!("fixtures/facade_consumer.rs")
                .replace("flui::", &format!("{alias}::"));
            check_consumer(
                dependencies,
                &source,
                &format!("facade alias={alias}, defaults={defaults}"),
            );
        }
    }
}

#[test]
fn internal_consumers_can_rename_direct_owning_crates() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut dependencies = toml::Table::new();
    for (alias, package) in [
        ("views", "flui-view"),
        ("motion", "flui-animation"),
        ("base", "flui-foundation"),
        ("catalog", "flui-widgets"),
        ("derive_support", "flui-macros"),
    ] {
        dependencies.insert(
            alias.into(),
            dependency(package, &root.join("crates").join(package), false),
        );
    }
    let source = include_str!("fixtures/facade_consumer.rs")
        .replace("flui::prelude", "catalog::prelude")
        .replace("flui::animation", "motion")
        .replace(
            "use flui::Diagnosticable;",
            "use base::Diagnosticable;\nuse derive_support::Diagnosticable;",
        );
    check_consumer(
        dependencies,
        &source,
        "renamed direct owners without facade",
    );
}

fn hot_reload_dependencies(include_layer: bool) -> toml::Table {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
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
    dependencies
}

const SCENE_PLUGIN_SOURCE: &str = "fn build(_: f32, _: f32) -> flui_layer::Scene { flui_layer::Scene::default() } flui::hot_reload::scene_plugin!(build);";
const APP_PLUGIN_SOURCE: &str =
    "flui::hot_reload::app_plugin!(flui::widgets::Text::new(\"hello\"));";

#[test]
fn plugin_factory_requires_the_canonical_scene() {
    let output = compile_consumer(
        hot_reload_dependencies(true),
        "fn build(_: f32, _: f32) -> u8 { 1 } flui::hot_reload::scene_plugin!(build);",
    );
    let diagnostics = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "non-Scene factory compiled");
    assert!(diagnostics.contains("E0308"), "{diagnostics}");
}

fn reject_safe_teardown(source: &str, function: &str) {
    let source = format!(
        "{source}\npub fn invalid_safe_call() {{ {function}(std::ptr::dangling_mut::<u8>().cast()); }}"
    );
    let output = compile_consumer(hot_reload_dependencies(true), &source);
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
fn scene_drop_requires_unsafe() {
    reject_safe_teardown(SCENE_PLUGIN_SOURCE, "flui_scene_drop");
}
#[test]
fn scene_free_requires_unsafe() {
    reject_safe_teardown(SCENE_PLUGIN_SOURCE, "flui_scene_free");
}
#[test]
fn app_drop_requires_unsafe() {
    reject_safe_teardown(APP_PLUGIN_SOURCE, "flui_app_drop");
}
#[test]
fn app_free_requires_unsafe() {
    reject_safe_teardown(APP_PLUGIN_SOURCE, "flui_app_free");
}

#[test]
fn plugin_macros_use_only_the_facade_including_when_renamed() {
    for alias in ["flui", "ui"] {
        for source in [
            "fn build(_: f32, _: f32) -> flui::hot_reload::Scene { Default::default() } flui::hot_reload::scene_plugin!(build);",
            APP_PLUGIN_SOURCE,
        ] {
            let mut dependencies = hot_reload_dependencies(false);
            let framework = dependencies.remove("flui").expect("facade dependency");
            dependencies.insert(alias.into(), framework);
            check_consumer(
                dependencies,
                &source.replace("flui::", &format!("{alias}::")),
                "facade plugin macro",
            );
        }
    }
}

#[test]
fn external_consumer_names_presentation_lifecycle_capability() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut dependencies = toml::Table::new();
    dependencies.insert("flui".into(), dependency("flui", root, false));
    let output = compile_consumer(
        dependencies,
        r"
use flui::view::{BuildContext, LifecycleHandle, LifecycleSubscription, LifecycleClosed};
pub fn acquire(ctx: &dyn BuildContext) -> Option<LifecycleHandle> { ctx.lifecycle_handle() }
pub fn observe(handle: &LifecycleHandle) -> Result<(Option<flui::view::AppLifecycleState>, LifecycleSubscription), LifecycleClosed> {
    handle.subscribe(|_| {})
}
",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn external_consumer_names_resident_application_and_renamed_facade() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = r#"
use flui::app::{Application, AppHandle, AppRunError, AppControlError, AppWindowError, MainWindowRequest, StartupWindow, ExitPolicy, AppConfig};
use std::{cell::Cell, rc::Rc};
fn send_sync<T: Send + Sync>() {}
pub fn application() {
    send_sync::<AppHandle>();
    let model = Rc::new(Cell::new(0));
    let _app = Application::new(move |_handle: &AppHandle| {
        model.set(model.get() + 1);
        flui::widgets::Text::new("fresh root")
    }).with_startup_window(StartupWindow::None)
      .with_config(AppConfig::new().with_exit_policy(ExitPolicy::ExplicitQuit))
      .on_ready(|handle| { let _request = handle.request_show_main_window(); })
      .on_window_error(|error: &AppWindowError| { let _ = std::error::Error::source(error); });
}
pub fn request(handle: &AppHandle) -> Result<MainWindowRequest, AppControlError> { handle.request_show_main_window() }
pub async fn result(request: MainWindowRequest) -> Result<flui::foundation::PresentationAddress, AppWindowError> { request.await }
pub fn run_error(error: AppRunError) { let _ = error; }
"#;
    for alias in ["flui", "ui"] {
        let mut dependencies = toml::Table::new();
        dependencies.insert(alias.into(), dependency("flui", root, false));
        let output = compile_consumer(
            dependencies,
            &source.replace("flui::", &format!("{alias}::")),
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
