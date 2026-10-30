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

#[test]
fn android_run_builds_the_selected_device_native_abi() {
    let fixture = TempDir::new().expect("Android runner fixture");
    let root = fixture.path();
    let facade = root.join("facade");
    package(&facade, "flui", "");
    std::fs::write(facade.join("src/lib.rs"), "").expect("facade identity");
    let app = root.join("app");
    package(
        &app,
        "app",
        "[workspace]\n[dependencies]\nflui = { path = \"../facade\" }\n",
    );
    std::fs::write(
        app.join("flui.toml"),
        "[app]\nname = \"app\"\nversion = \"0.1.0\"\norganization = \"com.example\"\n",
    )
    .expect("application config");
    let tools = root.join("tools");
    std::fs::create_dir(&tools).expect("tool directory");
    let source = tools.join("tool.rs");
    std::fs::write(&source, r#"
use std::{env, path::Path, process::{Command, exit}};
fn main() {
    let exe = env::current_exe().expect("tool executable");
    let name = Path::new(&exe).file_stem().expect("tool name").to_string_lossy();
    let args: Vec<_> = env::args_os().skip(1).collect();
    match name.as_ref() {
        "adb" if args.first().is_some_and(|arg| arg == "devices") => println!("List of devices attached\nemulator-5554 device product:sdk model:Fixture device:sdk"),
        "adb" => println!("x86_64,arm64-v8a"),
        "rustup" => println!("{}", env::var("FLUI_FIXTURE_INSTALLED_TARGETS").expect("installed targets")),
        "java" => println!("openjdk version 21"),
        "google-chrome" => {
            std::fs::write(env::var("FLUI_FIXTURE_BROWSER_STARTED").expect("browser mark"), "").expect("browser mark");
            println!("Google Chrome 1.0");
        }
        "cargo" if args.first().is_some_and(|arg| arg == "ndk") => {
            if args.iter().any(|arg| arg == "--version") { println!("cargo-ndk 4.1.2"); }
            else { eprintln!("ANDROID_NATIVE_BUILD_ARGS={args:?}"); exit(31); }
        }
        "cargo" => {
            let status = Command::new(env::var_os("FLUI_FIXTURE_REAL_CARGO").expect("real Cargo"))
                .args(args).status().expect("Cargo metadata");
            exit(status.code().unwrap_or(1));
        }
        _ => panic!("unknown fixture tool"),
    }
}
"#).expect("tool source");
    let shim = tools.join(format!("fixture-tool{}", std::env::consts::EXE_SUFFIX));
    let output = std::process::Command::new("rustc")
        .args(["--edition", "2024"])
        .arg(source)
        .arg("-o")
        .arg(&shim)
        .output()
        .expect("compile fixture tools");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // A browser on the PATH, which the Linux discovery would start to read its
    // version; it records that it was started.
    for tool in ["adb", "cargo", "rustup", "java", "google-chrome"] {
        std::fs::copy(
            &shim,
            tools.join(format!("{tool}{}", std::env::consts::EXE_SUFFIX)),
        )
        .expect("install fixture tool");
    }
    let sdk = root.join("sdk");
    let ndk = sdk.join("ndk").join("29.0.0");
    std::fs::create_dir_all(&ndk).expect("NDK fixture");
    let path = std::env::join_paths(std::iter::once(tools).chain(std::env::split_paths(
        &std::env::var_os("PATH").expect("PATH"),
    )))
    .expect("fixture PATH");
    let browser_started = root.join("browser-started");
    let mut cli = cargo_bin_cmd!("flui");
    cli.current_dir(&app)
        .env("PATH", path)
        .env("FLUI_FIXTURE_BROWSER_STARTED", &browser_started)
        // Metadata uses only local path dependencies. Keep its Cargo locks and
        // outputs independent of concurrent CLI fixtures on the CI runner.
        .env("CARGO_HOME", root.join("cargo-home"))
        .env("CARGO_NET_OFFLINE", "true")
        .env("CARGO_TARGET_DIR", app.join("target"))
        .env("ANDROID_HOME", sdk)
        .env("ANDROID_NDK_HOME", ndk)
        .env_remove("JAVA_HOME")
        .env("FLUI_FIXTURE_INSTALLED_TARGETS", "aarch64-linux-android\nx86_64-linux-android")
        .env(
            "FLUI_FIXTURE_REAL_CARGO",
            std::env::var_os("CARGO").expect("real Cargo"),
        )
        .args(["run", "--device", "emulator-5554", "--no-hot-reload"])
        .timeout(std::time::Duration::from_secs(30));
    let output = cli.output().expect("Android run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(4),
        "a failed build without hot reload must terminate: {stderr}"
    );
    let command = stderr
        .lines()
        .find(|line| line.contains("ANDROID_NATIVE_BUILD_ARGS="))
        .unwrap_or_else(|| panic!("the native build must reach cargo-ndk: {stderr}"));
    assert!(
        command.contains("\"-t\", \"x86_64\""),
        "the emulator's first native ABI must win over its ARM translation compatibility: {command}"
    );
    assert!(
        !browser_started.exists(),
        "a device named by its serial is found without starting the installed browsers"
    );

    let output = cli
        .env("FLUI_FIXTURE_INSTALLED_TARGETS", "aarch64-linux-android")
        .output()
        .expect("Android run without the selected compiler target");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(3),
        "missing compiler target: {stderr}"
    );
    assert!(
        stderr.contains("rustup target add x86_64-linux-android"),
        "installation hint: {stderr}"
    );
    assert!(
        !stderr.contains("ANDROID_NATIVE_BUILD_ARGS="),
        "must reject before cargo-ndk builds: {stderr}"
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

/// A worker hot-reload project builds and starts its host under the
/// application's floors, so a `[platforms]` table FLUI cannot honour stops
/// `flui run` before anything is built (ADR-0244 §10).
#[test]
fn a_worker_project_refuses_a_floor_flui_cannot_honour() {
    let tmp = TempDir::new().expect("fixture");
    let root = tmp.path();
    std::fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"host\", \"worker\"]\nresolver = \"3\"\n",
    )
    .expect("workspace manifest");
    let facade = root.join("facade");
    package(&facade, "flui", "");
    std::fs::write(facade.join("src/lib.rs"), "").expect("facade identity");
    package(
        &root.join("host"),
        "host",
        "[dependencies]\nflui = { path = \"../facade\" }\n",
    );
    package(&root.join("worker"), "worker", "");
    std::fs::write(
        root.join("flui.toml"),
        "[app]\nname = \"host\"\nversion = \"0.1.0\"\norganization = \"com.example\"\n\n\
         [hot_reload]\nhost_package = \"host\"\nworker_package = \"worker\"\n\
         worker_lib = \"worker\"\nlogic_watch = \"worker/src\"\n\n\
         [platforms.ios]\nmin = \"12.0\"\n",
    )
    .expect("application config");
    let output = cargo_bin_cmd!("flui")
        .current_dir(root)
        .env("CARGO_NET_OFFLINE", "true")
        .env("CARGO_TARGET_DIR", root.join("target"))
        .args(["run", "--device", "desktop"])
        .timeout(std::time::Duration::from_secs(60))
        .output()
        .expect("CLI");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success()
            && stderr
                .contains("[platforms.ios] min = \"12.0\" is below FLUI's floor for ios, 13.0"),
        "status {:?}: {stderr}",
        output.status
    );
    assert!(
        !root.join("target").exists(),
        "nothing is built before the refusal"
    );
}
