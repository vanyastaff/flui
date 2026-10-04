//! Integration tests for `flui build`.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
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
        .canonicalize()
        .expect("canonical workspace root")
}

/// Parse a captured stdout stream as NDJSON: one `serde_json::Value` per
/// non-empty line, panicking (with the offending line) on any line that
/// is not valid JSON — a `--json` run must never interleave plain text.
fn ndjson_events(stdout: &[u8]) -> Vec<serde_json::Value> {
    let text = String::from_utf8(stdout.to_vec()).expect("stdout is UTF-8");
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("not one JSON object per line ({e}): {line:?}"))
        })
        .collect()
}

#[test]
fn desktop_build_outside_a_project_in_json_mode_emits_a_pure_error_event() {
    let tmp = TempDir::new().expect("empty temp dir");

    let output = flui()
        .current_dir(tmp.path())
        .args(["build", "desktop", "--json"])
        .output()
        .expect("run flui build desktop --json");

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(6));

    // The stdout stream is pure NDJSON: every line parses, and the error
    // event carries the exit code and a message naming the missing file.
    let events = ndjson_events(&output.stdout);
    let error_event = events
        .iter()
        .find(|event| event["event"] == "error")
        .unwrap_or_else(|| panic!("no `error` event in {events:?}"));
    assert_eq!(error_event["code"], 6);
    assert!(
        error_event["message"]
            .as_str()
            .expect("message is a string")
            .contains("Cargo.toml not found")
    );
}

/// Real end-to-end build: scaffolds a project against this checkout and
/// builds it for desktop, gated behind `FLUI_CLI_LIVE_BUILD=1` because it
/// runs a genuine `cargo build` (slow, and needs a working toolchain).
#[test]
fn live_desktop_build_produces_an_artifact_on_disk() {
    if std::env::var_os("FLUI_CLI_LIVE_BUILD").is_none() {
        eprintln!(
            "skipping live_desktop_build_produces_an_artifact_on_disk: set FLUI_CLI_LIVE_BUILD=1 to run a real `flui build desktop`"
        );
        return;
    }

    let repo_root = repo_root();
    let workdir = TempDir::new().expect("scaffold workdir");
    let name = "flui-live-build-check";
    let project = workdir.path().join(name);

    flui()
        .current_dir(&repo_root)
        .env("CARGO_NET_OFFLINE", "true")
        .args([
            "create",
            name,
            "--org",
            "com.test",
            "--template",
            "empty",
            "--no-check",
        ])
        .arg(format!("--local={}", repo_root.display()))
        .arg("--path")
        .arg(workdir.path())
        .assert()
        .success();

    // Seed the resolved versions so the build does not need network access.
    std::fs::copy(repo_root.join("Cargo.lock"), project.join("Cargo.lock"))
        .expect("seed generated project with the workspace's resolved versions");

    let output = flui()
        .current_dir(&project)
        .env("CARGO_NET_OFFLINE", "true")
        .args(["build", "desktop", "--json"])
        .output()
        .expect("run flui build desktop --json");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let events = ndjson_events(&output.stdout);
    let done = events
        .iter()
        .find(|event| event["event"] == "build.done")
        .unwrap_or_else(|| panic!("no `build.done` event in {events:?}"));
    assert_eq!(done["ok"], true);
    let artifacts = done["artifacts"].as_array().expect("artifacts array");
    assert_eq!(
        artifacts.len(),
        1,
        "expected exactly one artifact: {artifacts:?}"
    );
    let kind = artifacts[0]["kind"].as_str().expect("artifact kind");
    assert!(
        matches!(kind, "binary" | "app-bundle"),
        "unexpected desktop artifact kind: {kind}"
    );
    let path = artifacts[0]["path"].as_str().expect("artifact path");
    assert!(
        Path::new(path).exists(),
        "reported artifact path does not exist: {path}"
    );
}

fn web_selector_reaches_cargo(selector: &str, selected: &str) {
    let fixture = TempDir::new().expect("web selector fixture");
    let root = fixture.path();
    std::fs::create_dir_all(root.join("src")).expect("source directory");
    std::fs::create_dir_all(root.join("examples")).expect("example directory");
    std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = [\"members/selected\"]\nresolver = \"3\"\n[package]\nname = \"default-web-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n").expect("manifest");
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("main");
    std::fs::write(root.join("examples/selected-demo.rs"), "fn main() {}\n").expect("example");
    let member = root.join("members/selected");
    std::fs::create_dir_all(member.join("src")).expect("selected package directory");
    std::fs::write(
        member.join("Cargo.toml"),
        "[package]\nname = \"selected-web-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("selected package manifest");
    std::fs::write(member.join("src/main.rs"), "fn main() {}\n").expect("selected package main");
    let tools = root.join("tools");
    std::fs::create_dir(&tools).expect("tool directory");
    // Metadata and target selection remain real Cargo operations. Only native
    // SDK discovery and compilation are replaced: refusal exposes the exact
    // selected build command without requiring an installed wasm toolchain.
    let source = tools.join("tool.rs");
    std::fs::write(
        &source,
        r#"
use std::{env, path::Path, process::{Command, exit}};
fn main() {
    let exe = env::current_exe().expect("tool executable");
    let name = Path::new(&exe).file_stem().expect("tool name").to_string_lossy();
    let args: Vec<_> = env::args_os().skip(1).collect();
    match name.as_ref() {
        "rustup" => println!("wasm32-unknown-unknown"),
        "wasm-bindgen" => println!("wasm-bindgen 0.2.100"),
        "cargo" if args.first().is_some_and(|arg| arg == "build") => {
            eprintln!("selected fixture Cargo args: {args:?}");
            exit(31);
        }
        "cargo" => {
            let status = Command::new(env::var_os("FLUI_FIXTURE_REAL_CARGO").expect("real Cargo"))
                .args(args).status().expect("Cargo metadata");
            exit(status.code().unwrap_or(1));
        }
        _ => panic!("unknown fixture tool"),
    }
}
"#,
    )
    .expect("tool source");
    let shim = tools.join(format!("fixture-tool{}", std::env::consts::EXE_SUFFIX));
    let output = std::process::Command::new("rustc")
        .args(["--edition", "2024"])
        .arg(&source)
        .arg("-o")
        .arg(&shim)
        .output()
        .expect("compile fixture tool");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for name in ["rustup", "wasm-bindgen", "cargo"] {
        std::fs::copy(
            &shim,
            tools.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)),
        )
        .expect("install tool alias");
    }
    let real_cargo = which::which("cargo").expect("real Cargo executable");
    let mut paths = vec![tools];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let output = flui()
        .timeout(std::time::Duration::from_secs(30))
        .current_dir(root)
        .env("PATH", std::env::join_paths(paths).expect("fixture PATH"))
        .env("FLUI_FIXTURE_REAL_CARGO", real_cargo)
        .env("CARGO_NET_OFFLINE", "true")
        .env("CARGO_TARGET_DIR", root.join("target"))
        .args(["build", "web", selector, selected, "--json"])
        .output()
        .expect("run web build selector");
    assert!(!output.status.success(), "fixture compilation must refuse");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("selected fixture Cargo args:"), "{stderr}");
    let cargo_selector = if selector == "--example" {
        "--example"
    } else {
        "--package"
    };
    assert!(
        stderr.contains(&format!("\"{cargo_selector}\", \"{selected}\"")),
        "{stderr}"
    );
}

fn web_example_selector_reaches_the_requested_example() {
    web_selector_reaches_cargo("--example", "selected-demo");
}

fn web_package_selector_reaches_the_requested_package() {
    web_selector_reaches_cargo("--package", "selected-web-app");
}

fn android_example_selection_is_rejected_before_sdk_discovery() {
    flui()
        .args(["build", "android", "--example", "demo", "--json"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicates::str::contains(
            "--example and --package are unsupported",
        ));
}

fn android_package_selection_is_rejected_before_sdk_discovery() {
    flui()
        .args(["build", "android", "--package", "app", "--json"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicates::str::contains(
            "--example and --package are unsupported",
        ));
}

#[test]
fn platform_build_selectors_reach_the_selected_cargo_unit_or_refuse() {
    crate::test_cases::run_cases(&[
        (
            "web_example_selector_reaches_the_requested_example",
            web_example_selector_reaches_the_requested_example,
        ),
        (
            "web_package_selector_reaches_the_requested_package",
            web_package_selector_reaches_the_requested_package,
        ),
        (
            "android_example_selection_is_rejected_before_sdk_discovery",
            android_example_selection_is_rejected_before_sdk_discovery,
        ),
        (
            "android_package_selection_is_rejected_before_sdk_discovery",
            android_package_selection_is_rejected_before_sdk_discovery,
        ),
    ]);
}
