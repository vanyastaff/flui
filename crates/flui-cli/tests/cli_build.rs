//! Integration tests for `flui build`.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
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
fn desktop_build_outside_a_project_exits_not_a_flui_project() {
    let tmp = TempDir::new().expect("empty temp dir");

    flui()
        .current_dir(tmp.path())
        .args(["build", "desktop"])
        .assert()
        .failure()
        .code(6)
        .stderr(predicate::str::contains("Cargo.toml not found"));
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

/// `--lib` and `--example` conflict via clap's own `conflicts_with`
/// (`validate_options` no longer duplicates this check); clap's own usage
/// errors exit 2, same as `CliError::Usage`.
#[test]
fn ios_lib_and_example_are_mutually_exclusive() {
    let tmp = TempDir::new().expect("empty temp dir");

    flui()
        .current_dir(tmp.path())
        .args(["build", "ios", "--lib", "--example", "x"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("--lib").and(predicate::str::contains("--example")));
}

/// `--universal` and `--simulator` conflict via clap's own `conflicts_with`
/// (`validate_options` no longer duplicates this check either).
#[test]
fn ios_universal_and_simulator_are_mutually_exclusive() {
    let tmp = TempDir::new().expect("empty temp dir");

    flui()
        .current_dir(tmp.path())
        .args(["build", "ios", "--universal", "--simulator", "chosen"])
        .assert()
        .failure()
        .code(2)
        .stderr(
            predicate::str::contains("--universal").and(predicate::str::contains("--simulator")),
        );
}

#[test]
fn android_simulator_flag_is_rejected_as_ios_only() {
    let tmp = TempDir::new().expect("empty temp dir");

    flui()
        .current_dir(tmp.path())
        .args(["build", "android", "--simulator", "X"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("--simulator").and(predicate::str::contains("iOS-only")));
}

#[test]
fn desktop_lib_flag_is_rejected_as_ios_only() {
    let tmp = TempDir::new().expect("empty temp dir");

    flui()
        .current_dir(tmp.path())
        .args(["build", "desktop", "--lib"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("--lib").and(predicate::str::contains("iOS-only")));
}

#[test]
fn example_and_package_are_mutually_exclusive() {
    let tmp = TempDir::new().expect("empty temp dir");

    flui()
        .current_dir(tmp.path())
        .args(["build", "desktop", "--example", "demo", "--package", "app"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("--example").and(predicate::str::contains("--package")));
}

#[test]
fn ios_universal_without_lib_is_rejected() {
    let tmp = TempDir::new().expect("empty temp dir");

    flui()
        .current_dir(tmp.path())
        .args(["build", "ios", "--universal"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("--universal").and(predicate::str::contains("--lib")));
}

/// Selector validation runs before any project lookup: each of these
/// argument sets is rejected on the flag conflict itself (exit code 2, a
/// usage error), in an empty directory, without ever reaching (or
/// mentioning) the `Cargo.toml` check.
#[test]
fn conflicting_ios_delivery_modes_fail_before_cargo() {
    let tmp = TempDir::new().expect("empty temp dir");

    for args in [
        vec!["build", "ios", "--universal"],
        vec!["build", "ios", "--lib", "--example", "demo"],
        vec!["build", "ios", "--universal", "--simulator", "chosen"],
        vec!["build", "desktop", "--lib"],
        vec!["build", "desktop", "--simulator", "chosen"],
    ] {
        let assertion = flui()
            .current_dir(tmp.path())
            .args(&args)
            .assert()
            .failure()
            .code(2);
        let output = assertion.get_output();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("Cargo.toml not found")
                && !stderr.contains("could not find `Cargo.toml`"),
            "{args:?} must fail on the selector conflict itself, before any Cargo.toml lookup; got: {stderr}"
        );
    }
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

/// Real end-to-end web path: scaffold against this checkout, `flui build
/// web --json` must leave `pkg/app.js` + `pkg/app_bg.wasm` beside
/// `index.html`, then `flui run --device browser:… --no-open --json` must
/// serve that directory (the page carries the reload script) and close the
/// session with `run.stop` on SIGINT. Gated behind `FLUI_CLI_LIVE_WEB=1`:
/// it compiles the framework for wasm32 and needs `wasm-bindgen-cli`.
#[cfg(unix)]
#[test]
fn live_web_build_and_run_serve_the_generated_project() {
    use std::io::{BufRead, BufReader, Read, Write};
    if std::env::var_os("FLUI_CLI_LIVE_WEB").is_none() {
        eprintln!(
            "skipping live_web_build_and_run_serve_the_generated_project: set FLUI_CLI_LIVE_WEB=1"
        );
        return;
    }
    let repo_root = repo_root();
    let workdir = TempDir::new().expect("scaffold workdir");
    let name = "flui-live-web-check";
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
            "counter",
            "--no-check",
        ])
        .arg(format!("--local={}", repo_root.display()))
        .arg("--path")
        .arg(workdir.path())
        .assert()
        .success();
    std::fs::copy(repo_root.join("Cargo.lock"), project.join("Cargo.lock"))
        .expect("seed the generated project with the workspace's resolved versions");
    flui()
        .current_dir(&project)
        .args(["platform", "add", "web", "--json"])
        .assert()
        .success();

    let output = flui()
        .current_dir(&project)
        .env("CARGO_NET_OFFLINE", "true")
        .args(["build", "web", "--json"])
        .output()
        .expect("run flui build web --json");
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
    let dir = Path::new(
        done["artifacts"][0]["path"]
            .as_str()
            .expect("artifact path"),
    );
    for file in ["index.html", "pkg/app.js", "pkg/app_bg.wasm"] {
        assert!(
            dir.join(file).is_file(),
            "missing {file} in {}",
            dir.display()
        );
    }

    // The first browser flui devices lists on this machine; none means the
    // serve half cannot be exercised here, and the build half already passed.
    let devices = flui()
        .current_dir(&project)
        .args(["devices", "--json"])
        .output()
        .expect("flui devices --json");
    let Some(browser) = ndjson_events(&devices.stdout)
        .into_iter()
        .find(|event| event["event"] == "device" && event["platform"] == "web")
        .and_then(|event| event["id"].as_str().map(str::to_string))
    else {
        eprintln!("no browser installed; the serve half is not exercised here");
        return;
    };

    let mut run = std::process::Command::new(assert_cmd::cargo::cargo_bin("flui"))
        .current_dir(&project)
        .env("CARGO_NET_OFFLINE", "true")
        .args([
            "run",
            "--device",
            &browser,
            "--no-open",
            "--no-hot-reload",
            "--json",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .expect("spawn flui run");
    let mut lines = BufReader::new(run.stdout.take().expect("piped stdout")).lines();
    let mut seen = Vec::new();
    let url = loop {
        let line = lines
            .next()
            .expect("flui run ended before serving")
            .expect("stdout line");
        let event: serde_json::Value = serde_json::from_str(&line).expect("NDJSON");
        seen.push(event.clone());
        if event["event"] == "run.web.serve" {
            break event["url"].as_str().expect("url").to_string();
        }
    };

    let addr = url
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string();
    let mut stream = std::net::TcpStream::connect(&addr).expect("connect to the dev server");
    write!(stream, "GET / HTTP/1.1\r\nHost: {addr}\r\n\r\n").expect("request");
    let mut page = String::new();
    stream.read_to_string(&mut page).expect("response");
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    assert!(page.contains("/__flui/reload.js"), "{page}");
    assert!(page.contains("./pkg/app.js"), "{page}");

    assert!(
        std::process::Command::new("kill")
            .args(["-INT", &run.id().to_string()])
            .status()
            .expect("kill")
            .success()
    );
    let rest: Vec<String> = lines.map_while(Result::ok).collect();
    let status = run.wait().expect("flui run exit status");
    assert_eq!(status.code(), Some(130), "{rest:?}");
    assert!(
        rest.iter()
            .any(|line| line.contains("\"event\":\"run.stop\"")),
        "{rest:?}"
    );
}
