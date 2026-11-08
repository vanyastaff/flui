//! Build an external package author against the SDK archive, not its checkout.
//!
//! The consumer has a version-only production dependency and opts into the test
//! kit through its dev-dependency, as an external package's own tests do. Every
//! direct SDK dependency is patched to this checkout so Cargo can resolve the
//! unpublished train offline; the SDK itself is patched to the unpacked archive.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use flate2::read::GzDecoder;

const SDK_VERSION: &str = "0.1.0-dev";
const TRAIN_CRATES: &[&str] = &[
    "flui-animation",
    "flui-foundation",
    "flui-interaction",
    "flui-objects",
    "flui-painting",
    "flui-platform",
    "flui-rendering",
    "flui-scheduler",
    "flui-testing",
    "flui-view",
    "flui-widgets",
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("BUG: flui-sdk must sit two levels below the workspace root")
        .to_path_buf()
}

/// The workspace target as Cargo resolves it, including a relocated target.
/// Each nested command gets its own child rather than the outer target itself,
/// whose build lock the enclosing test process still holds.
fn workspace_target_dir(root: &Path) -> PathBuf {
    cargo_metadata::MetadataCommand::new()
        .cargo_path(env!("CARGO"))
        .current_dir(root)
        .no_deps()
        .other_options(vec!["--offline".to_owned(), "--locked".to_owned()])
        .exec()
        .expect("run cargo metadata for the workspace target directory")
        .target_directory
        .into_std_path_buf()
}

fn utf8_path(path: &Path) -> &str {
    path.to_str()
        .expect("checkout and temporary paths must be UTF-8 for a TOML manifest")
}

fn path_entry(path: &Path) -> toml::Value {
    let mut entry = toml::Table::new();
    entry.insert("path".into(), utf8_path(path).into());
    entry.into()
}

fn write_consumer(root: &Path, project: &tempfile::TempDir, packaged_sdk: &Path) {
    let name = project
        .path()
        .file_name()
        .expect("BUG: a temporary directory has a name")
        .to_string_lossy()
        .trim_start_matches('.')
        .to_ascii_lowercase();
    let mut package = toml::Table::new();
    package.insert("name".into(), format!("sdk-package-parity-{name}").into());
    package.insert("version".into(), "0.0.0".into());
    package.insert("edition".into(), "2024".into());
    package.insert("publish".into(), false.into());

    let mut dev_sdk = toml::Table::new();
    dev_sdk.insert("version".into(), format!("={SDK_VERSION}").into());
    dev_sdk.insert("features".into(), vec!["testing"].into());
    let mut dev_dependencies = toml::Table::new();
    dev_dependencies.insert("flui-sdk".into(), dev_sdk.into());

    let mut dependencies = toml::Table::new();
    dependencies.insert("flui-sdk".into(), format!("={SDK_VERSION}").into());

    let mut patches = toml::Table::new();
    patches.insert("flui-sdk".into(), path_entry(packaged_sdk));
    for package in TRAIN_CRATES {
        patches.insert(
            (*package).into(),
            path_entry(&root.join("crates").join(package)),
        );
    }
    let mut patch = toml::Table::new();
    patch.insert("crates-io".into(), patches.into());

    let mut manifest = toml::Table::new();
    manifest.insert("package".into(), package.into());
    // Keep the consumer outside FLUI's workspace even when its temporary
    // directory is created below a checkout on another host.
    manifest.insert("workspace".into(), toml::Table::new().into());
    manifest.insert("dependencies".into(), dependencies.into());
    manifest.insert("dev-dependencies".into(), dev_dependencies.into());
    manifest.insert("patch".into(), patch.into());

    std::fs::create_dir(project.path().join("src")).expect("create consumer source directory");
    std::fs::write(
        project.path().join("Cargo.toml"),
        toml::to_string(&manifest).expect("serialize external consumer manifest"),
    )
    .expect("write external consumer manifest");
    std::fs::write(project.path().join("src/lib.rs"), EXTERNAL_SOURCE)
        .expect("write external consumer source");
    // Seed registry versions from this checkout, then let the first metadata
    // call add the external root and its patched path packages offline. The
    // actual test uses --locked after that resolution has completed.
    std::fs::copy(root.join("Cargo.lock"), project.path().join("Cargo.lock"))
        .expect("seed external consumer lockfile");
}

fn cargo_package(root: &Path, target: &Path) -> Output {
    Command::new(env!("CARGO"))
        .args([
            "package",
            "--manifest-path",
            utf8_path(&root.join("crates/flui-sdk/Cargo.toml")),
            "--offline",
            "--locked",
            "--allow-dirty",
            "--no-verify",
            "--exclude-lockfile",
            "--target-dir",
        ])
        .arg(target)
        .current_dir(root)
        .output()
        .expect("run cargo package for flui-sdk")
}

fn report(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn external_package_tests_run_against_the_packaged_sdk() {
    let root = repo_root();
    let cache = workspace_target_dir(&root).join("sdk-package-parity-check");
    let package_target = cache.join("package");
    let archive = package_target.join(format!("package/flui-sdk-{SDK_VERSION}.crate"));
    if archive.exists() {
        std::fs::remove_file(&archive).expect("remove stale flui-sdk package archive");
    }
    let output = cargo_package(&root, &package_target);
    assert!(
        output.status.success(),
        "cargo package failed for flui-sdk:\n{}",
        report(&output)
    );

    let project = tempfile::tempdir().expect("create external SDK consumer directory");
    let unpacked = project.path().join("unpacked");
    let file = std::fs::File::open(&archive)
        .unwrap_or_else(|error| panic!("open {}: {error}", archive.display()));
    tar::Archive::new(GzDecoder::new(file))
        .unpack(&unpacked)
        .expect("unpack flui-sdk package archive");
    let packaged_sdk = unpacked.join(format!("flui-sdk-{SDK_VERSION}"));
    write_consumer(&root, &project, &packaged_sdk);

    let metadata = cargo_metadata::MetadataCommand::new()
        .cargo_path(env!("CARGO"))
        .current_dir(project.path())
        .other_options(vec!["--offline".to_owned()])
        .exec()
        .expect("resolve the external SDK consumer offline");
    let sdk = metadata
        .packages
        .iter()
        .find(|package| package.name.as_str() == "flui-sdk")
        .expect("external metadata contains flui-sdk");
    let resolved = sdk
        .manifest_path
        .as_std_path()
        .canonicalize()
        .expect("canonical resolved packaged SDK manifest");
    let expected = packaged_sdk
        .join("Cargo.toml")
        .canonicalize()
        .expect("canonical unpacked SDK manifest");
    assert_eq!(
        resolved, expected,
        "flui-sdk must resolve to the unpacked archive, not the checkout"
    );
    assert!(
        sdk.source.is_none(),
        "the patched packaged SDK must be a path package: {:?}",
        sdk.source
    );
    println!("packaged flui-sdk resolved to {}", resolved.display());

    let output = Command::new(env!("CARGO"))
        .args(["test", "--offline", "--locked", "--lib", "--target-dir"])
        .arg(cache.join("consumer"))
        .args(["--", "--nocapture"])
        .current_dir(project.path())
        .output()
        .expect("run external package tests against packaged flui-sdk");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "external package tests against packaged flui-sdk failed:\n{}",
        report(&output)
    );
    for test in [
        "custom_theme_extension_runs_through_packaged_sdk",
        "custom_plugin_registers_through_packaged_sdk",
    ] {
        assert!(
            stdout.contains(&format!("test {test} ... ok")),
            "external consumer did not execute {test}:\n{stdout}"
        );
    }
    assert!(
        stdout.contains("test result: ok. 2 passed; 0 failed"),
        "external consumer must run exactly its two behavioural tests:\n{stdout}"
    );
}

const EXTERNAL_SOURCE: &str = r#"
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use flui_sdk::geometry::Size;
use flui_sdk::painting::styling::Color;
use flui_sdk::platform::capability::{Capability, CapabilityRegistrar, Plugin, ProviderContext};
use flui_sdk::rendering::BoxConstraints;
use flui_sdk::testing::widgets::lay_out;
use flui_sdk::testing::{HeadlessHost, HeadlessWindow};
use flui_sdk::view::prelude::*;
use flui_sdk::view::{BoxedView, InheritedView, impl_inherited_view};
use flui_sdk::widgets::{InheritedTheme, SizedBox};

#[derive(Clone, Copy, Debug, PartialEq)]
struct PackageThemeData {
    accent: Color,
}

#[derive(Clone)]
struct PackageTheme {
    data: PackageThemeData,
    child: BoxedView,
}

impl PackageTheme {
    fn new(data: PackageThemeData, child: impl IntoView) -> Self {
        Self {
            data,
            child: child.into_view().boxed(),
        }
    }

    fn of(cx: &dyn BuildContext) -> PackageThemeData {
        cx.depend_on::<Self, _>(|theme| theme.data)
            .expect("PackageTheme ancestor")
    }
}

impl std::fmt::Debug for PackageTheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PackageTheme")
            .field("data", &self.data)
            .finish_non_exhaustive()
    }
}

impl InheritedView for PackageTheme {
    type Data = PackageThemeData;

    fn data(&self) -> &Self::Data {
        &self.data
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        self.data != old.data
    }
}

impl_inherited_view!(PackageTheme);

impl InheritedTheme for PackageTheme {
    fn wrap(&self, _cx: &dyn BuildContext, child: BoxedView) -> BoxedView {
        Self::new(self.data, child).boxed()
    }
}

#[derive(Clone, StatelessView)]
struct ThemedLeaf {
    observed: Rc<Cell<Option<Color>>>,
}

impl StatelessView for ThemedLeaf {
    fn build(&self, cx: &dyn BuildContext) -> impl IntoView {
        self.observed.set(Some(PackageTheme::of(cx).accent));
        SizedBox::new(1.0, 1.0)
    }
}

#[test]
fn custom_theme_extension_runs_through_packaged_sdk() {
    let accent = Color::rgb(12, 34, 56);
    let observed = Rc::new(Cell::new(None));
    let mut laid = lay_out(
        PackageTheme::new(
            PackageThemeData { accent },
            ThemedLeaf {
                observed: Rc::clone(&observed),
            },
        ),
        BoxConstraints::tight(Size::new(1.0, 1.0)),
    );
    assert_eq!(observed.get(), Some(accent));
    assert_eq!(laid.inherited_dependent_count::<PackageTheme>(), 1);
}

enum Battery {}

#[derive(Clone)]
struct BatteryHandle {
    level: u8,
}

impl Capability for Battery {
    type Handle = BatteryHandle;
    const NAME: &'static str = "external-package-battery";
}

struct BatteryPlugin;

impl Plugin for BatteryPlugin {
    fn register(&self, registrar: &mut CapabilityRegistrar) {
        registrar.provide::<Battery>(|_: &ProviderContext<'_>| Ok(BatteryHandle { level: 80 }));
    }
}

#[derive(Clone)]
struct Gauge {
    level: Rc<Cell<Option<u8>>>,
}

struct GaugeState {
    level: Rc<Cell<Option<u8>>>,
}

impl StatefulView for Gauge {
    type State = GaugeState;

    fn create_state(&self) -> Self::State {
        GaugeState {
            level: Rc::clone(&self.level),
        }
    }
}

impl View for Gauge {
    fn create_element(&self) -> flui_sdk::view::element::ElementKind {
        flui_sdk::view::element::ElementKind::stateful(self)
    }
}

impl ViewState<Gauge> for GaugeState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        let battery = cx.capability::<Battery>().expect("BatteryPlugin provides Battery");
        self.level.set(Some(battery.level));
    }

    fn build(&self, _view: &Gauge, _cx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(10.0, 10.0)
    }
}

#[test]
fn custom_plugin_registers_through_packaged_sdk() {
    let mut host = HeadlessHost::with_plugin(HeadlessWindow::new(10, 10), &BatteryPlugin);
    let level = Rc::new(Cell::new(None));
    host.attach(&Gauge {
        level: Rc::clone(&level),
    })
    .expect("external gauge attaches");
    let _ = host.pump(Duration::ZERO);
    assert_eq!(level.get(), Some(80));
}
"#;
