//! Cargo artifact acceptance without Xcode app packaging. Host fixtures are portable.
use flui_build::{
    BuildUnit, BuilderContextBuilder, IOSBuilder, Platform, PlatformBuilder, Profile,
};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

const ROOT: &str = "FLUI_IOS_ARTIFACT_FIXTURE_ROOT";
const CASE: &str = "FLUI_IOS_ARTIFACT_FIXTURE_CASE";
const TARGETS: &str = "FLUI_IOS_ARTIFACT_FIXTURE_TARGETS";
fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    std::fs::write(path, text).expect("fixture file");
}
fn manifest(name: &str, library: &str) -> String {
    format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[lib]\nname = \"{library}\"\ncrate-type = [\"rlib\", \"staticlib\"]\n"
    )
}
fn fixture(case: &str, targets: &str) {
    let fixture = tempfile::tempdir().expect("fixture");
    let root = fixture.path().join("app");
    let external = fixture.path().join("external target with spaces");
    write(
        &root,
        "Cargo.toml",
        &format!(
            "[workspace]\n{}",
            manifest("ios-artifact-probe", "custom_bridge")
        ),
    );
    write(
        &root,
        "src/lib.rs",
        "pub fn fixture_value() -> u32 { 42 }\n",
    );
    write(&root, "examples/demo.rs", "fn main() {}\n");
    write(
        &root,
        "build.rs",
        "fn main() { println!(\"cargo:warning=artifact fixture\"); }\n",
    );
    write(
        &root,
        "platforms/ios/Frameworks/keep.txt",
        "unrelated framework asset",
    );
    if case.starts_with("workspace") || case.starts_with("virtual") {
        let defaults = if case == "virtual-ambiguous" {
            ""
        } else {
            "default-members = [\"apps/selected\"]\n"
        };
        write(
            &root,
            "Cargo.toml",
            &format!("[workspace]\nmembers = [\"apps/*\"]\nresolver = \"3\"\n{defaults}"),
        );
        for name in ["selected", "other"] {
            write(
                &root,
                &format!("apps/{name}/Cargo.toml"),
                &manifest(&format!("{name}-app"), &format!("{name}_bridge")),
            );
            write(
                &root,
                &format!("apps/{name}/src/lib.rs"),
                "pub fn value() {}\n",
            );
        }
    }
    if case == "missing-staticlib" || case == "staticlib-example-only" {
        write(
            &root,
            "Cargo.toml",
            "[workspace]\n[package]\nname=\"fixture\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[[example]]\nname=\"demo\"\ncrate-type=[\"staticlib\"]\n",
        );
        if case == "missing-staticlib" {
            write(
                &root,
                "Cargo.toml",
                "[workspace]\n[package]\nname=\"fixture\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
            );
        }
    }
    if case == "default-run" {
        write(
            &root,
            "Cargo.toml",
            &format!(
                "[workspace]\n{}",
                manifest("ios-artifact-probe", "custom_bridge")
                    .replace("[lib]", "default-run=\"other\"\n[lib]")
            ),
        );
        write(&root, "src/bin/other.rs", "fn main() {}\n");
    }
    if case == "config" {
        let config = toml::Table::from_iter([(
            "build".into(),
            toml::Value::Table(toml::Table::from_iter([(
                "target-dir".into(),
                toml::Value::String(external.to_str().expect("UTF8 fixture").into()),
            )])),
        )]);
        write(
            &root,
            ".cargo/config.toml",
            &toml::to_string(&config).expect("config"),
        );
    }
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args(["--exact", "ios_cargo_artifact_fixtures", "--nocapture"])
        .env(ROOT, &root)
        .env(CASE, case)
        .env(TARGETS, targets)
        .env("FLUI_IOS_EXPECTED_TARGET", &external)
        .env("CARGO_NET_OFFLINE", "true");
    if case == "config" {
        command.env_remove("CARGO_TARGET_DIR");
    } else {
        command.env("CARGO_TARGET_DIR", &external);
    }
    let output = command.output().expect("fixture child");
    assert!(
        output.status.success(),
        "{case}: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
fn worker(root: PathBuf, case: &str) {
    let targets: Vec<String> = std::env::var(TARGETS)
        .expect("triples")
        .split(',')
        .map(str::to_owned)
        .collect();
    let selected_targets = if case == "empty" {
        vec![]
    } else if case == "multi-example" {
        vec![targets[0].clone(), targets[0].clone()]
    } else {
        targets.clone()
    };
    let unit = match case {
        "workspace-package" => BuildUnit::Package("other-app".into()),
        "example" | "multi-example" => BuildUnit::Example("demo".into()),
        _ => BuildUnit::DefaultBinary,
    };
    let cwd = if case == "workspace-current" {
        root.join("apps/other")
    } else {
        root.clone()
    };
    let ctx = BuilderContextBuilder::new(cwd)
        .with_platform(Platform::IOS {
            targets: selected_targets,
        })
        .with_profile(Profile::Debug)
        .with_target(unit)
        .with_output_dir(root.join("staged"))
        .build();
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let builder = IOSBuilder::new();
    let result = runtime.block_on(builder.build_rust(&ctx));
    let sentinel = || {
        assert_eq!(
            std::fs::read(root.join("platforms/ios/Frameworks/keep.txt"))
                .expect("preserved Frameworks"),
            b"unrelated framework asset"
        );
    };
    sentinel();
    if matches!(
        case,
        "empty"
            | "multi-example"
            | "missing-staticlib"
            | "staticlib-example-only"
            | "virtual-ambiguous"
    ) {
        let error = result.expect_err("invalid selection").to_string();
        assert!(
            error.contains(if case == "empty" {
                "at least one"
            } else if case == "multi-example" {
                "exactly one target"
            } else {
                "expected one static library"
            }),
            "{error}"
        );
        assert!(
            !PathBuf::from(std::env::var_os("FLUI_IOS_EXPECTED_TARGET").expect("target")).exists(),
            "invalid selection must not compile"
        );
        return;
    }
    let artifacts = result.expect("Cargo artifact");
    let external = PathBuf::from(std::env::var_os("FLUI_IOS_EXPECTED_TARGET").expect("target"));
    if case == "example" {
        assert!(artifacts.rust_libs.is_empty());
        let path = artifacts.executable.expect("example executable");
        assert!(path.is_file() && path.starts_with(&external));
    } else {
        assert!(artifacts.executable.is_none());
        assert_eq!(artifacts.rust_libs.len(), targets.len());
        let library = if matches!(case, "workspace-package" | "workspace-current") {
            "other_bridge"
        } else if case == "virtual-default" {
            "selected_bridge"
        } else {
            "custom_bridge"
        };
        for (path, triple) in artifacts.rust_libs.iter().zip(&targets) {
            assert!(
                path.is_file() && path.starts_with(external.join(triple)),
                "{}",
                path.display()
            );
            let expected = if triple.ends_with("windows-msvc") {
                format!("{library}.lib")
            } else {
                format!("lib{library}.a")
            };
            assert_eq!(path.file_name().expect("filename"), expected.as_str());
        }
        if case == "cached-failure" {
            let cached = runtime
                .block_on(builder.build_rust(&ctx))
                .expect("cached build");
            assert_eq!(cached.rust_libs, artifacts.rust_libs);
            write(
                &root,
                "src/lib.rs",
                "pub fn broken() { missing_fixture_symbol(); }\n",
            );
            assert!(
                runtime.block_on(builder.build_rust(&ctx)).is_err(),
                "stale archive is not successful compilation"
            );
            assert!(
                artifacts.rust_libs[0].is_file(),
                "stale archive remains present"
            );
            sentinel();
        }
    }
}
#[test]
fn ios_cargo_artifact_fixtures() {
    if let Some(root) = std::env::var_os(ROOT) {
        worker(PathBuf::from(root), &std::env::var(CASE).expect("case"));
        return;
    }
    let output = Command::new("rustc").arg("-vV").output().expect("rustc");
    let version = String::from_utf8(output.stdout).expect("version");
    let host = version
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .expect("host");
    for case in [
        "external",
        "config",
        "cached-failure",
        "default-run",
        "workspace-package",
        "workspace-current",
        "virtual-default",
        "virtual-ambiguous",
        "missing-staticlib",
        "staticlib-example-only",
        "example",
        "empty",
        "multi-example",
    ] {
        fixture(case, host);
    }
}
#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires Xcode SDK and installed aarch64-apple-ios/aarch64-apple-ios-sim targets; run explicitly"]
fn ios_device_and_simulator_static_libraries() {
    fixture("external", "aarch64-apple-ios,aarch64-apple-ios-sim");
}
