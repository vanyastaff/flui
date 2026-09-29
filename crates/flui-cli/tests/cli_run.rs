//! Project admission uses real Cargo metadata; fixture binaries only print a marker.
//!
//! Each fixture builds into its own target directory, never an inherited
//! `CARGO_TARGET_DIR`: several fixtures are packages named `app`, and in a
//! shared target dir parallel tests would race to replace one `app` binary.
use assert_cmd::cargo::cargo_bin_cmd;
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
        .env("CARGO_TARGET_DIR", dir.join("target"))
        .args(["run", "--release", "--device", "desktop"]);
    command
}

/// Every line of `--json` stdout is one event; the app's own stdout arrives
/// as `run.app.log` instead of leaking into the machine stream.
pub fn json_mode_streams_the_app_lifecycle_as_ndjson() {
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
        .map(|e| {
            (
                e["stream"].as_str().expect("stream"),
                e["line"].as_str().expect("line"),
            )
        })
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
