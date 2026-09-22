//! Project admission uses real Cargo metadata; fixture binaries only print a marker.
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::path::Path;
use tempfile::TempDir;

fn package(dir: &Path, name: &str, extra: &str) {
    std::fs::create_dir_all(dir.join("src")).expect("source directory");
    std::fs::write(
        dir.join("Cargo.toml"),
        format!("[package]\nname = {name:?}\nversion = \"0.1.0\"\nedition = \"2024\"\n{extra}"),
    )
    .expect("manifest");
    std::fs::write(
        dir.join("src/main.rs"),
        "fn main() { println!(\"FLUI_ADMISSION_MARKER\"); eprintln!(\"FLUI_STDERR_MARKER\"); }\n",
    )
    .expect("main");
}

fn run(dir: &Path) -> assert_cmd::Command {
    let mut command = cargo_bin_cmd!("flui");
    command
        .current_dir(dir)
        .env("CARGO_NET_OFFLINE", "true")
        .args(["run", "--release", "--device", "desktop"]);
    command
}

#[test]
fn sole_facade_project_runs_its_binary() {
    let tmp = TempDir::new().expect("temporary fixture");
    let dependency = tmp.path().join("facade");
    package(&dependency, "flui", "");
    std::fs::write(dependency.join("src/lib.rs"), "").expect("identity fixture library");
    let app = tmp.path().join("app");
    package(
        &app,
        "app",
        "[workspace]\n[dependencies]\nflui = { path = \"../facade\" }\n",
    );
    run(&app)
        .assert()
        .success()
        .stdout(predicate::str::contains("FLUI_ADMISSION_MARKER"));
}

#[test]
fn renamed_inherited_target_dependency_admits_only_its_application() {
    let tmp = TempDir::new().expect("workspace fixture");
    let root = tmp.path();
    std::fs::write(root.join("Cargo.toml"), "[workspace]\nresolver = '3'\nmembers = ['app', 'other', 'facade']\n[workspace.dependencies]\nui = { package = 'flui', path = 'facade' }\n").expect("workspace");
    package(&root.join("facade"), "flui", "");
    std::fs::write(root.join("facade/src/lib.rs"), "").expect("fixture library");
    package(
        &root.join("app"),
        "app",
        "[target.'cfg(all())'.dependencies]\nui.workspace = true\n",
    );
    package(&root.join("other"), "other", "");
    run(&root.join("app"))
        .assert()
        .success()
        .stdout(predicate::str::contains("FLUI_ADMISSION_MARKER"));
    run(&root.join("other"))
        .assert()
        .failure()
        .stderr(predicate::str::contains("normal dependency"));
    run(root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("virtual workspace root"));
}

#[test]
fn comments_prefixes_and_non_normal_dependencies_do_not_admit_projects() {
    for (name, kind) in [
        ("flui", "dev-dependencies"),
        ("flui", "build-dependencies"),
        ("flui-app-helper", "dependencies"),
    ] {
        let tmp = TempDir::new().expect("fixture");
        package(&tmp.path().join("dependency"), name, "");
        std::fs::write(tmp.path().join("dependency/src/lib.rs"), "").expect("fixture library");
        let app = tmp.path().join("app");
        package(
            &app,
            "app",
            &format!(
                "[workspace]\n# flui-app and flui-widgets are not dependencies\n[{kind}]\nui = {{ package = {name:?}, path = '../dependency' }}\n"
            ),
        );
        run(&app)
            .assert()
            .failure()
            .stderr(predicate::str::contains("normal dependency"));
    }
}

#[test]
fn cargo_metadata_failure_keeps_cargo_diagnostics() {
    let tmp = TempDir::new().expect("fixture");
    package(
        tmp.path(),
        "app",
        "[dependencies]\nui = { workspace = true }\n",
    );
    run(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("cargo metadata failed"))
        .stderr(predicate::str::contains("workspace"));
}

#[test]
fn legacy_dependency_alias_remains_supported() {
    let tmp = TempDir::new().expect("fixture");
    package(&tmp.path().join("dependency"), "flui-widgets", "");
    std::fs::write(tmp.path().join("dependency/src/lib.rs"), "").expect("fixture library");
    let app = tmp.path().join("app");
    package(
        &app,
        "app",
        "[workspace]\n[dependencies]\nwidgets = { package = 'flui-widgets', path = '../dependency' }\n",
    );
    run(&app)
        .assert()
        .success()
        .stdout(predicate::str::contains("FLUI_ADMISSION_MARKER"));
}

#[test]
fn unavailable_simulator_never_runs_the_host_application() {
    let tmp = TempDir::new().expect("simulator routing fixture");
    let dependency = tmp.path().join("facade");
    package(&dependency, "flui", "");
    std::fs::write(dependency.join("src/lib.rs"), "").expect("facade identity");
    let app = tmp.path().join("app");
    package(
        &app,
        "simulator-routing-app",
        "[workspace]\n[dependencies]\nflui = { path = \"../facade\" }\n",
    );
    let mut command = cargo_bin_cmd!("flui");
    command
        .current_dir(&app)
        .env("CARGO_NET_OFFLINE", "true")
        .timeout(std::time::Duration::from_secs(30))
        .args([
            "run",
            "--release",
            "--device",
            "00000000-0000-0000-0000-000000000000",
        ]);
    let output = command.output().expect("bounded CLI invocation");
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("FLUI_ADMISSION_MARKER"),
        "a simulator request executed the host binary: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        output.status.code(),
        Some(5),
        "an unknown UDID is a device-not-found error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Every line of `--json` stdout is one event; the app's own stdout arrives
/// as `run.app.log` instead of leaking into the machine stream.
#[test]
fn json_mode_streams_the_app_lifecycle_as_ndjson() {
    let tmp = TempDir::new().expect("fixture");
    let dependency = tmp.path().join("facade");
    package(&dependency, "flui", "");
    std::fs::write(dependency.join("src/lib.rs"), "").expect("facade identity");
    let app = tmp.path().join("app");
    package(
        &app,
        "app",
        "[workspace]\n[dependencies]\nflui = { path = \"../facade\" }\n",
    );
    let output = run(&app).arg("--json").output().expect("CLI");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let events: Vec<serde_json::Value> = stdout
        .lines()
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|e| panic!("not JSON: {line:?}: {e}"))
        })
        .collect();
    let names: Vec<&str> = events
        .iter()
        .map(|e| e["event"].as_str().expect("event field"))
        .collect();
    for expected in [
        "run.start",
        "run.app.start",
        "run.app.log",
        "run.app.exit",
        "run.stop",
    ] {
        assert!(names.contains(&expected), "missing {expected} in {names:?}");
    }
    let logs: Vec<(&str, &str)> = events
        .iter()
        .filter(|e| e["event"] == "run.app.log")
        .map(|e| (e["stream"].as_str().unwrap(), e["line"].as_str().unwrap()))
        .collect();
    assert!(
        logs.contains(&("stdout", "FLUI_ADMISSION_MARKER")),
        "{logs:?}"
    );
    assert!(
        logs.contains(&("stderr", "FLUI_STDERR_MARKER")),
        "the app's stderr must be wrapped too, never raw in the stream: {logs:?}"
    );
    let stop = events
        .iter()
        .find(|e| e["event"] == "run.stop")
        .expect("run.stop closes run.start");
    assert_eq!(stop["interrupted"], false);
    let exit = events
        .iter()
        .find(|e| e["event"] == "run.app.exit")
        .expect("exit event");
    assert_eq!(exit["code"], 0);
}

/// A library crate cannot be run; the refusal names the fix (exit 6).
#[test]
fn library_package_is_refused_with_a_hint() {
    let tmp = TempDir::new().expect("fixture");
    let dependency = tmp.path().join("facade");
    package(&dependency, "flui", "");
    std::fs::write(dependency.join("src/lib.rs"), "").expect("facade identity");
    let lib = tmp.path().join("widgets");
    std::fs::create_dir_all(lib.join("src")).expect("lib dir");
    std::fs::write(
        lib.join("Cargo.toml"),
        "[package]\nname = \"widgets\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[workspace]\n[dependencies]\nflui = { path = \"../facade\" }\n",
    )
    .expect("manifest");
    std::fs::write(lib.join("src/lib.rs"), "").expect("lib");
    run(&lib)
        .assert()
        .failure()
        .code(6)
        .stderr(predicate::str::contains("library"))
        .stderr(predicate::str::contains("flui test"));
}

/// Ctrl-C in the dev loop stops the app, reports it, and exits 130.
#[cfg(unix)]
#[test]
fn sigint_stops_the_dev_loop_and_exits_130() {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    let tmp = TempDir::new().expect("fixture");
    let dependency = tmp.path().join("facade");
    package(&dependency, "flui", "");
    std::fs::write(dependency.join("src/lib.rs"), "").expect("facade identity");
    let app = tmp.path().join("app");
    package(
        &app,
        "app",
        "[workspace]\n[dependencies]\nflui = { path = \"../facade\" }\n",
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_flui"))
        .current_dir(&app)
        .env("CARGO_NET_OFFLINE", "true")
        .args(["run", "--device", "desktop", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn flui run");
    let stdout = child.stdout.take().expect("piped stdout");
    let mut lines = BufReader::new(stdout).lines();
    // The fixture app exits at once; the dev loop keeps watching after that.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "no run.app.exit within 120 s"
        );
        let line = lines.next().expect("stream open").expect("line");
        let event: serde_json::Value = serde_json::from_str(&line).expect("ndjson");
        if event["event"] == "run.app.exit" {
            break;
        }
    }
    let status = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("kill");
    assert!(status.success());
    let rest: Vec<String> = lines.map_while(Result::ok).collect();
    let status = child.wait().expect("flui exits");
    assert_eq!(status.code(), Some(130), "events after SIGINT: {rest:?}");
    // The app had already exited, so no `run.app.stop` is owed — only the
    // terminal error event with the interrupt code.
    assert!(
        !rest.iter().any(|l| l.contains("\"run.app.stop\"")),
        "no app was running, nothing to stop: {rest:?}"
    );
    assert!(
        rest.iter().any(|l| l.contains("\"code\":130")),
        "the error event must carry 130: {rest:?}"
    );
    assert!(
        rest.iter()
            .any(|l| l.contains("\"event\":\"run.stop\"") && l.contains("\"interrupted\":true")),
        "run.stop must close the session on interrupt too: {rest:?}"
    );
}

/// Ctrl-C while the app is still running (no hot reload) stops it, reports
/// `run.app.stop`, and exits 130 — not "application failed".
#[cfg(unix)]
#[test]
fn sigint_during_run_once_stops_the_app_and_exits_130() {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    let tmp = TempDir::new().expect("fixture");
    let dependency = tmp.path().join("facade");
    package(&dependency, "flui", "");
    std::fs::write(dependency.join("src/lib.rs"), "").expect("facade identity");
    let app = tmp.path().join("app");
    package(
        &app,
        "app",
        "[workspace]\n[dependencies]\nflui = { path = \"../facade\" }\n",
    );
    // An app that keeps running until told otherwise.
    std::fs::write(
        app.join("src/main.rs"),
        "fn main() { println!(\"FLUI_ADMISSION_MARKER\"); std::thread::sleep(std::time::Duration::from_secs(600)); }\n",
    )
    .expect("long-running main");
    let mut child = Command::new(env!("CARGO_BIN_EXE_flui"))
        .current_dir(&app)
        .env("CARGO_NET_OFFLINE", "true")
        .args(["run", "--no-hot-reload", "--device", "desktop", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn flui run");
    let stdout = child.stdout.take().expect("piped stdout");
    let mut lines = BufReader::new(stdout).lines();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "no run.app.log within 120 s"
        );
        let line = lines.next().expect("stream open").expect("line");
        if line.contains("FLUI_ADMISSION_MARKER") {
            break;
        }
    }
    let status = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("kill");
    assert!(status.success());
    let rest: Vec<String> = lines.map_while(Result::ok).collect();
    let status = child.wait().expect("flui exits");
    assert_eq!(status.code(), Some(130), "events after SIGINT: {rest:?}");
    assert!(
        rest.iter().any(|l| l.contains("\"run.app.stop\"")),
        "the running app must be reported stopped: {rest:?}"
    );
}

/// An Android serial is a real device id `flui devices` prints, but `run`
/// cannot drive it yet: say so (exit 2), never fall through to iOS.
#[test]
fn unsupported_device_platform_is_refused_honestly() {
    let tmp = TempDir::new().expect("fixture");
    let dependency = tmp.path().join("facade");
    package(&dependency, "flui", "");
    std::fs::write(dependency.join("src/lib.rs"), "").expect("facade identity");
    let app = tmp.path().join("app");
    package(
        &app,
        "app",
        "[workspace]\n[dependencies]\nflui = { path = \"../facade\" }\n",
    );
    // Only cargo on PATH (no adb): the serial is unknown → device not found (5).
    let cargo_dir = std::path::PathBuf::from(env!("CARGO"))
        .parent()
        .expect("cargo lives in a directory")
        .to_path_buf();
    let output = cargo_bin_cmd!("flui")
        .current_dir(&app)
        .env("CARGO_NET_OFFLINE", "true")
        .env("PATH", &cargo_dir)
        .args(["run", "--release", "--device", "emulator-5554"])
        .output()
        .expect("CLI");
    assert_eq!(
        output.status.code(),
        Some(5),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("FLUI_ADMISSION_MARKER"),
        "must not run the host binary"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("flui devices"));
}

/// A browser id that `flui devices` does not list is a device-not-found
/// error (exit 5), the same as any other unknown `--device`.
#[test]
fn unknown_browser_device_exits_device_not_found() {
    let tmp = TempDir::new().expect("temporary fixture");
    let dependency = tmp.path().join("facade");
    package(&dependency, "flui", "");
    std::fs::write(dependency.join("src/lib.rs"), "").expect("identity fixture library");
    let app = tmp.path().join("app");
    package(
        &app,
        "app",
        "[workspace]\n[dependencies]\nflui = { path = \"../facade\" }\n",
    );
    cargo_bin_cmd!("flui")
        .current_dir(&app)
        .env("CARGO_NET_OFFLINE", "true")
        .args([
            "run",
            "--device",
            "browser:definitely-not-installed",
            "--no-open",
        ])
        .assert()
        .failure()
        .code(5)
        .stderr(predicate::str::contains("browser:definitely-not-installed"));
}
