//! Real Cargo fixtures exercise artifact discovery without building FLUI apps.
use std::path::{Path, PathBuf};
use std::process::Command;

use flui_build::desktop::DesktopBuilder;
use flui_build::{BuildUnit, BuilderContextBuilder, Platform, PlatformBuilder, Profile};

const FIXTURE_ROOT: &str = "FLUI_DESKTOP_FIXTURE_ROOT";
const FIXTURE_CASE: &str = "FLUI_DESKTOP_FIXTURE_CASE";

fn write(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("file parent")).expect("directory");
    std::fs::write(path, contents).expect("fixture file");
}

fn fixture(case: &str) {
    let temp = tempfile::tempdir().expect("fixture");
    let root = temp.path().join("app");
    let target = temp.path().join("external target with spaces");
    write(
        &root,
        "Cargo.toml",
        "[workspace]\n[package]\nname = \"artifact-probe\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    write(&root, "src/main.rs", "fn main() {}\n");
    write(&root, "examples/demo.rs", "fn main() {}\n");
    write(
        &root,
        "build.rs",
        "fn main() { println!(\"cargo:warning=fixture build script diagnostic\"); }\n",
    );
    if case == "default-run" || case == "ambiguous" {
        write(&root, "src/bin/other.rs", "fn main() {}\n");
        if case == "default-run" {
            write(
                &root,
                "Cargo.toml",
                "[workspace]\n[package]\nname = \"artifact-probe\"\nversion = \"0.1.0\"\nedition = \"2024\"\ndefault-run = \"other\"\n",
            );
        }
    }
    if case.starts_with("workspace") || case.starts_with("virtual") {
        write(
            &root,
            "Cargo.toml",
            if case == "virtual-ambiguous" || case == "virtual-examples" {
                "[workspace]\nmembers = [\"apps/*\"]\nresolver = \"3\"\n"
            } else {
                "[workspace]\nmembers = [\"apps/*\"]\ndefault-members = [\"apps/selected\"]\nresolver = \"3\"\n"
            },
        );
        for (directory, name) in [("selected", "selected-app"), ("other", "other-app")] {
            write(
                &root,
                &format!("apps/{directory}/Cargo.toml"),
                &format!(
                    "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[[bin]]\nname = \"custom-name\"\npath = \"src/main.rs\"\n"
                ),
            );
            write(
                &root,
                &format!("apps/{directory}/src/main.rs"),
                &format!("fn main() {{ println!(\"{name}\"); }}\n"),
            );
            write(
                &root,
                &format!("apps/{directory}/examples/demo.rs"),
                "fn main() {}\n",
            );
        }
    }
    if case == "library-example" {
        write(
            &root,
            "Cargo.toml",
            "[workspace]\n[package]\nname = \"artifact-probe\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[[example]]\nname = \"demo\"\ncrate-type = [\"lib\"]\n",
        );
        write(&root, "examples/demo.rs", "pub fn helper() {}\n");
    }
    if case == "config" {
        let document = toml::Table::from_iter([(
            "build".into(),
            toml::Value::Table(toml::Table::from_iter([(
                "target-dir".into(),
                toml::Value::String(target.to_string_lossy().into_owned()),
            )])),
        )]);
        write(
            &root,
            ".cargo/config.toml",
            &toml::to_string(&document).expect("config TOML"),
        );
    }
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args([
            "--exact",
            "desktop_uses_cargos_external_target_directory",
            "--nocapture",
        ])
        .env(FIXTURE_ROOT, &root)
        .env(FIXTURE_CASE, case)
        .env("CARGO_NET_OFFLINE", "true");
    if case == "config" {
        command.env_remove("CARGO_TARGET_DIR");
    } else {
        command.env("CARGO_TARGET_DIR", &target);
    }
    let output = command.output().expect("fixture subprocess");
    assert!(
        output.status.success(),
        "{case}: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if case == "cached-failure" {
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("cannot find value `missing_fixture_symbol`"),
            "compiler cause must remain visible: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn worker(root: PathBuf, case: &str) {
    let runtime = tokio::runtime::Runtime::new().expect("test runtime");
    let unit = match case {
        "external" | "virtual-examples" | "library-example" => BuildUnit::Example("demo".into()),
        "workspace" => BuildUnit::Package("other-app".into()),
        _ => BuildUnit::DefaultBinary,
    };
    // A current member must win over the virtual root's default members.
    let cwd = if case == "workspace-current" {
        root.join("apps/other")
    } else {
        root.clone()
    };
    let ctx = BuilderContextBuilder::new(cwd.clone())
        .with_platform(Platform::Desktop { target: None })
        .with_profile(Profile::Debug)
        .with_target(unit)
        .with_output_dir(root.join("staged"))
        .build();
    let builder = DesktopBuilder::new();
    let result = runtime.block_on(builder.build_rust(&ctx));
    if matches!(
        case,
        "ambiguous" | "virtual-ambiguous" | "virtual-examples" | "library-example"
    ) {
        assert!(
            result
                .expect_err("ambiguous/nonexecutable selection must fail")
                .to_string()
                .contains("expected one executable")
        );
        return;
    }
    let artifacts = result.expect("discover Cargo executable");
    let executable = artifacts.executable.as_ref().expect("binary artifact");
    let external = root
        .parent()
        .expect("fixture parent")
        .join("external target with spaces")
        .canonicalize()
        .expect("target path");
    assert!(
        executable
            .canonicalize()
            .expect("artifact path")
            .starts_with(external)
    );
    let expected_name = match case {
        "default-run" => "other",
        name if name.starts_with("workspace") || name == "virtual" => "custom-name",
        "external" => "demo",
        _ => "artifact-probe",
    };
    assert_eq!(executable.file_stem().expect("stem"), expected_name);
    let expected_package = match case {
        "workspace" | "workspace-current" => Some("other-app"),
        "virtual" => Some("selected-app"),
        _ => None,
    };
    if let Some(expected_package) = expected_package {
        let output = Command::new(executable)
            .output()
            .expect("run selected host executable");
        assert!(output.status.success(), "selected executable must succeed");
        assert_eq!(
            String::from_utf8(output.stdout)
                .expect("package marker")
                .trim(),
            expected_package,
            "equal binary names must not hide the selected package's identity"
        );
    }
    let staged = runtime
        .block_on(builder.build_platform(&ctx, &artifacts))
        .expect("stage executable");
    assert_eq!(
        std::fs::read(staged.app_binary).expect("staged bytes"),
        std::fs::read(executable).expect("built bytes")
    );
    if case == "cached-failure" {
        let cached = runtime
            .block_on(builder.build_rust(&ctx))
            .expect("cached build returns artifact");
        assert_eq!(cached.executable, artifacts.executable);
        write(
            &root,
            "src/main.rs",
            "fn main() { missing_fixture_symbol; }\n",
        );
        assert!(
            runtime.block_on(builder.build_rust(&ctx)).is_err(),
            "failed Cargo must not return the existing stale executable"
        );
        assert!(
            executable.is_file(),
            "stale executable exists to challenge selection"
        );
    }
}

#[test]
fn desktop_uses_cargos_external_target_directory() {
    if let Some(root) = std::env::var_os(FIXTURE_ROOT) {
        worker(
            PathBuf::from(root),
            &std::env::var(FIXTURE_CASE).expect("case"),
        );
    } else {
        fixture("external");
    }
}
#[test]
fn configured_target_directory_is_respected() {
    fixture("config");
}
#[test]
fn arbitrary_workspace_package_and_current_member_are_selected() {
    fixture("workspace");
    fixture("workspace-current");
}
#[test]
fn default_run_and_ambiguous_bins_are_distinguished() {
    fixture("default-run");
    fixture("ambiguous");
}
#[test]
fn virtual_default_members_and_ambiguous_examples_are_distinguished() {
    fixture("virtual");
    fixture("virtual-ambiguous");
    fixture("virtual-examples");
}
#[test]
fn library_examples_are_rejected() {
    fixture("library-example");
}
#[test]
fn cached_success_and_failed_compile_do_not_confuse_artifact_selection() {
    fixture("cached-failure");
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn bundle_names_cannot_escape_the_output_directory() {
    for name in [
        "../escape".to_string(),
        "/absolute".to_string(),
        "..".into(),
        ".".into(),
        String::new(),
        "parent\\escape".into(),
        "bad\0name".into(),
    ] {
        let temp = tempfile::tempdir().expect("fixture");
        let external = temp.path().join("escape.app");
        write(&external, "sentinel", "must survive");
        let name = if name == "/absolute" {
            temp.path().join("escape").to_string_lossy().into_owned()
        } else {
            name
        };
        let output = temp.path().join("output");
        let executable = temp.path().join("input");
        std::fs::write(&executable, "fixture executable").expect("input");
        let ctx = BuilderContextBuilder::new(temp.path().to_path_buf())
            .with_platform(Platform::Desktop { target: None })
            .with_profile(Profile::Debug)
            .with_output_dir(output.clone())
            .with_bundle(flui_build::AppBundle {
                name,
                identifier: "org.example.fixture".into(),
                version: "0.1.0".into(),
            })
            .build();
        let artifacts = flui_build::BuildArtifacts {
            rust_libs: Vec::new(),
            executable: Some(executable),
            metadata: serde_json::json!({}),
        };
        let builder = DesktopBuilder::new();
        assert!(
            builder.build_platform(&ctx, &artifacts).await.is_err(),
            "unsafe bundle component must be rejected"
        );
        assert_eq!(
            std::fs::read_to_string(external.join("sentinel"))
                .expect("external sentinel preserved"),
            "must survive"
        );
        assert!(
            !output.exists(),
            "validation precedes any directory creation"
        );
    }
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn unicode_bundle_names_work_and_existing_symlinks_are_not_followed() {
    let temp = tempfile::tempdir().expect("fixture");
    let output = temp.path().join("output");
    let executable = temp.path().join("input");
    std::fs::write(&executable, "fixture executable").expect("input");
    let name = "Интерфейс & App";
    let ctx = BuilderContextBuilder::new(temp.path().to_path_buf())
        .with_platform(Platform::Desktop { target: None })
        .with_profile(Profile::Debug)
        .with_output_dir(output.clone())
        .with_bundle(flui_build::AppBundle {
            name: name.into(),
            identifier: "org.example.fixture".into(),
            version: "0.1.0".into(),
        })
        .build();
    let artifacts = flui_build::BuildArtifacts {
        rust_libs: Vec::new(),
        executable: Some(executable),
        metadata: serde_json::json!({}),
    };
    let builder = DesktopBuilder::new();
    let staged = builder
        .build_platform(&ctx, &artifacts)
        .await
        .expect("unicode and spaces are valid");
    assert_eq!(staged.app_binary, output.join(format!("{name}.app")));
    let plist =
        std::fs::read_to_string(staged.app_binary.join("Contents/Info.plist")).expect("plist");
    assert!(plist.contains("Интерфейс &amp; App"));
    std::fs::remove_dir_all(&staged.app_binary).expect("remove owned fixture bundle");
    let external = temp.path().join("external");
    write(&external, "sentinel", "must survive");
    std::os::unix::fs::symlink(&external, &staged.app_binary).expect("fixture link");
    assert!(builder.build_platform(&ctx, &artifacts).await.is_err());
    assert_eq!(
        std::fs::read_to_string(external.join("sentinel")).expect("target preserved"),
        "must survive"
    );
    std::fs::remove_file(&staged.app_binary).expect("remove fixture link");
    std::os::unix::fs::symlink(temp.path().join("missing"), &staged.app_binary)
        .expect("dangling link");
    assert!(builder.build_platform(&ctx, &artifacts).await.is_err());
    assert!(
        std::fs::symlink_metadata(&staged.app_binary)
            .expect("link retained")
            .file_type()
            .is_symlink()
    );
}
