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
    // A library with no declared crate types, as `flui create` writes it.
    std::fs::write(app.join("src/lib.rs"), "").expect("application library");
    std::fs::write(
        app.join("flui.toml"),
        "[app]\nname = \"app\"\nversion = \"0.1.0\"\nid = \"com.example.app\"\n",
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
        "rustc" if args.iter().any(|arg| arg == "target-libdir") => {
            let target = args.iter().skip_while(|arg| *arg != "--target").nth(1).expect("a target");
            let libraries = Path::new(&env::var_os("FLUI_FIXTURE_TARGET_LIBS").expect("target libraries")).join(target);
            println!("{}", libraries.display());
        }
        "rustc" => {
            let status = Command::new(env::var_os("FLUI_FIXTURE_REAL_RUSTC").expect("real rustc"))
                .args(args).status().expect("rustc");
            exit(status.code().unwrap_or(1));
        }
        "java" => println!("openjdk version 21"),
        "google-chrome" => {
            std::fs::write(env::var("FLUI_FIXTURE_BROWSER_STARTED").expect("browser mark"), "").expect("browser mark");
            println!("Google Chrome 1.0");
        }
        "cargo" if args.first().is_some_and(|arg| arg == "ndk") => {
            if args.iter().any(|arg| arg == "--version") { println!("cargo-ndk 4.1.2"); }
            else {
                eprintln!("ANDROID_NATIVE_BUILD_ARGS={args:?}");
                eprintln!("ANDROID_NATIVE_BUILD_NDK={}", env::var("ANDROID_NDK_HOME").unwrap_or_default());
                eprintln!("ANDROID_NATIVE_BUILD_SDK={}", env::var("ANDROID_HOME").unwrap_or_default());
                exit(31);
            }
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
    for tool in ["adb", "cargo", "rustc", "java", "google-chrome"] {
        std::fs::copy(
            &shim,
            tools.join(format!("{tool}{}", std::env::consts::EXE_SUFFIX)),
        )
        .expect("install fixture tool");
    }
    // The toolchain has the libraries of both Android targets.
    let target_libs = root.join("target-libs");
    for target in ["aarch64-linux-android", "x86_64-linux-android"] {
        std::fs::create_dir_all(target_libs.join(target)).expect("target libraries");
    }
    let real_rustc = which::which("rustc").expect("real rustc executable");
    // Two NDK versions under the SDK and no ANDROID_NDK_HOME: the build resolves the
    // newer by number (29 above 9, which sorts after it as text) and hands it
    // to cargo-ndk, which would otherwise pick one itself.
    let sdk = root.join("sdk");
    let ndk = sdk.join("ndk").join("29.0.0");
    std::fs::create_dir_all(&ndk).expect("NDK fixture");
    std::fs::create_dir_all(sdk.join("ndk").join("9.0.0")).expect("older NDK fixture");
    // The packager without Gradle finds its build-tools and a platform at or
    // above FLUI's targetSdk before anything compiles.
    let build_tools = sdk.join("build-tools").join("99.0.0");
    std::fs::create_dir_all(&build_tools).expect("build-tools fixture");
    for tool in ["aapt2", "zipalign", "apksigner"] {
        std::fs::write(
            build_tools.join(format!("{tool}{}", std::env::consts::EXE_SUFFIX)),
            b"",
        )
        .expect("build-tool fixture");
    }
    let platform = sdk.join("platforms").join("android-99");
    std::fs::create_dir_all(&platform).expect("platform fixture");
    std::fs::write(platform.join("android.jar"), b"").expect("android.jar fixture");
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
        .env("ANDROID_HOME", &sdk)
        .env_remove("ANDROID_NDK_HOME")
        .env_remove("RUSTC")
        .env_remove("JAVA_HOME")
        .env("FLUI_FIXTURE_TARGET_LIBS", &target_libs)
        .env("FLUI_FIXTURE_REAL_RUSTC", real_rustc)
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
        command.contains("\"rustc\", \"-p\", \"app\", \"--lib\", \"--crate-type\", \"cdylib\""),
        "the application's library is compiled as the cdylib NativeActivity loads: {command}"
    );
    assert!(
        !browser_started.exists(),
        "a device named by its serial is found without starting the installed browsers"
    );
    let handed = |name: &str| {
        stderr
            .lines()
            .find_map(|line| line.strip_prefix(&format!("ANDROID_NATIVE_BUILD_{name}=")))
            .map(std::path::PathBuf::from)
    };
    assert_eq!(
        handed("NDK"),
        Some(ndk),
        "cargo-ndk links with the NDK the build resolved: {stderr}"
    );
    assert_eq!(handed("SDK"), Some(sdk), "{stderr}");

    std::fs::remove_dir_all(target_libs.join("x86_64-linux-android")).expect("uninstall target");
    let output = cli
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
    // The app names the facade it depends on: cargo's `unused_dependencies`
    // lint would otherwise warn, and CI denies cargo's warnings.
    std::fs::write(
        app.join("src/main.rs"),
        "use flui as _;\n\nfn main() { println!(\"FLUI_ADMISSION_MARKER\"); eprintln!(\"FLUI_STDERR_MARKER\"); }\n",
    )
    .expect("main naming the facade");
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

/// A restart rebuilds beside the running application and replaces it only
/// with a build that succeeded: after a compile error the previous build is
/// still running, its window still up while the author fixes the error; the
/// fix replaces it; and an app that exits while a rebuild runs is reported as
/// that exit before its successor starts. Windows cannot overwrite a running
/// executable and stops the app first.
#[cfg(not(windows))]
#[test]
fn a_restart_replaces_the_running_build_only_with_one_that_succeeded() {
    use std::io::BufRead as _;
    use std::time::{Duration, Instant};

    /// `flui run` and its app, stopped when the test ends on any path: the
    /// app runs until a `stop` file appears in its directory, which a test
    /// that panicked would otherwise never write.
    struct Session {
        cli: std::process::Child,
        stop: std::path::PathBuf,
    }
    impl Drop for Session {
        /// Ctrl-C, on which `flui run` stops the app it started; the `stop`
        /// file is the app's own way out should `flui run` be gone.
        fn drop(&mut self) {
            let _ = std::fs::write(&self.stop, "");
            let _ = std::process::Command::new("kill")
                .args(["-INT", &self.cli.id().to_string()])
                .status();
            let deadline = Instant::now() + Duration::from_secs(10);
            while matches!(self.cli.try_wait(), Ok(None)) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            let _ = self.cli.kill();
            let _ = self.cli.wait();
        }
    }

    /// The agent the app serves at the endpoint `flui run` hands it, as far as `flui run`'s
    /// build reports need: the hello, one window, and each `build.report` printed as
    /// `REPORT <state>`.
    const AGENT: &str = r#"
fn serve_agent() {
    use std::io::{BufRead as _, Write as _};
    let Ok(path) = std::env::var("FLUI_AGENT_ENDPOINT") else { return };
    // The build it replaced left its socket behind.
    let _ = std::fs::remove_file(&path);
    let listener = std::os::unix::net::UnixListener::bind(&path).expect("bind the endpoint");
    std::thread::spawn(move || {
        for stream in listener.incoming().map_while(Result::ok) {
            let mut out = stream.try_clone().expect("clone");
            let mut lines = std::io::BufReader::new(stream).lines().map_while(Result::ok);
            if lines.next().is_none() {
                continue;
            }
            let _ = writeln!(out, "{{\"hello\":{{\"protocol\":\"PROTOCOL\"}}}}");
            for line in lines {
                let id = line
                    .split("\"id\":")
                    .nth(1)
                    .and_then(|rest| rest.split([',', '}']).next())
                    .unwrap_or("0")
                    .trim()
                    .to_owned();
                let result = if line.contains("\"windows.list\"") {
                    "{\"windows\":[\"w1\"]}"
                } else {
                    if let Some(state) = ["failed", "succeeded", "building"]
                        .into_iter()
                        .find(|state| line.contains(&format!("\"{state}\"")))
                    {
                        println!("REPORT {state}");
                    }
                    "{}"
                };
                let _ = writeln!(out, "{{\"id\":{id},\"result\":{result}}}");
            }
        }
    });
}
"#;

    /// An app that serves [`AGENT`], prints `marker` and runs until the `stop` file exists.
    fn app_main(marker: &str) -> String {
        let agent = AGENT.replace(
            "PROTOCOL",
            &flui_protocol::version::PROTOCOL_VERSION.to_string(),
        );
        format!(
            "use flui as _;\n{agent}\nfn main() {{\n    serve_agent();\n    \
             println!(\"{marker}\");\n    \
             while !std::path::Path::new(\"stop\").exists() {{\n        \
             std::thread::sleep(std::time::Duration::from_millis(20));\n    }}\n}}\n"
        )
    }
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
    // `cargo run` starts the app in the package directory.
    let stop = app.join("stop");
    std::fs::write(app.join("src/main.rs"), app_main("FLUI_APP_FIRST")).expect("first build");

    let mut cli = std::process::Command::new(assert_cmd::cargo::cargo_bin!("flui"));
    cli.current_dir(&app)
        .env("CARGO_NET_OFFLINE", "true")
        .env("CARGO_TARGET_DIR", app.join("target"))
        .args(["run", "--device", "desktop", "--json"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut session = Session {
        cli: cli.spawn().expect("flui run"),
        stop: stop.clone(),
    };
    let stdout = session.cli.stdout.take().expect("piped stdout");
    let (sender, events) = std::sync::mpsc::channel::<serde_json::Value>();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
        {
            if let Ok(event) = serde_json::from_str::<serde_json::Value>(&line)
                && sender.send(event).is_err()
            {
                break;
            }
        }
    });
    let mut seen = Vec::new();
    // Waits for the next event `wanted` accepts. Every event received stays
    // in `seen`, for the assertions and for the failure messages.
    fn wait_for(
        events: &std::sync::mpsc::Receiver<serde_json::Value>,
        seen: &mut Vec<serde_json::Value>,
        wanted: impl Fn(&serde_json::Value) -> bool,
        timeout: Duration,
    ) -> serde_json::Value {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = events.recv_timeout(left).unwrap_or_else(|_| {
                panic!("nothing expected within {timeout:?}; events: {seen:?}")
            });
            let found = wanted(&event);
            seen.push(event.clone());
            if found {
                return event;
            }
        }
    }
    let named = |name: &'static str| move |event: &serde_json::Value| event["event"] == name;
    let says = |marker: &'static str| {
        move |event: &serde_json::Value| event["event"] == "run.app.log" && event["line"] == marker
    };
    let names = |seen: &[serde_json::Value]| -> Vec<String> {
        seen.iter()
            .filter_map(|event| event["event"].as_str())
            .filter(|name| name.starts_with("run.app.") && *name != "run.app.log")
            .map(str::to_owned)
            .collect()
    };

    // An edit counts once the sources are watched and the app is up: `cargo
    // run` checks the sources again before it starts the binary.
    wait_for(
        &events,
        &mut seen,
        named("run.watch"),
        Duration::from_secs(300),
    );
    if !seen.iter().any(says("FLUI_APP_FIRST")) {
        wait_for(
            &events,
            &mut seen,
            says("FLUI_APP_FIRST"),
            Duration::from_secs(60),
        );
    }
    // The run opened its session only to report its builds: `flui mcp` finds no app through
    // it, and a session file a `flui run --agent` wrote would be left alone.
    assert!(
        !app.join("target").join("flui-agent.json").exists(),
        "a run without --agent wrote the session file"
    );

    std::fs::write(app.join("src/main.rs"), "fn main() { this is not rust }\n")
        .expect("a compile error");
    let mark = seen.len();
    let reload = wait_for(
        &events,
        &mut seen,
        named("run.reload"),
        Duration::from_secs(300),
    );
    assert_eq!(reload["ok"], false, "the broken build failed: {seen:?}");
    assert_eq!(
        names(&seen[mark..]),
        Vec::<String>::new(),
        "the previous build was stopped for a build that failed: {seen:?}"
    );
    // The running build shows the rebuild, then its failure (ADR-0395).
    if !seen[mark..].iter().any(says("REPORT failed")) {
        wait_for(
            &events,
            &mut seen,
            says("REPORT failed"),
            Duration::from_secs(30),
        );
    }
    assert!(
        seen[mark..].iter().any(says("REPORT building")),
        "the build was shown starting: {seen:?}"
    );

    std::fs::write(app.join("src/main.rs"), app_main("FLUI_APP_SECOND")).expect("the fix");
    let mark = seen.len();
    let reload = wait_for(
        &events,
        &mut seen,
        named("run.reload"),
        Duration::from_secs(300),
    );
    assert_eq!(reload["ok"], true, "the fixed build succeeded: {seen:?}");
    wait_for(
        &events,
        &mut seen,
        says("FLUI_APP_SECOND"),
        Duration::from_secs(60),
    );
    assert_eq!(
        names(&seen[mark..]),
        ["run.app.stop", "run.app.start"],
        "the fixed build replaced the previous one: {seen:?}"
    );
    // The success clears the failure: shown to the build it replaced or, once that one is
    // stopped, to its successor.
    if !seen[mark..].iter().any(says("REPORT succeeded")) {
        wait_for(
            &events,
            &mut seen,
            says("REPORT succeeded"),
            Duration::from_secs(30),
        );
    }

    // The app exits on its own while the next rebuild runs beside it.
    std::fs::write(app.join("src/main.rs"), app_main("FLUI_APP_THIRD")).expect("another edit");
    let mark = seen.len();
    wait_for(
        &events,
        &mut seen,
        named("run.build.start"),
        Duration::from_secs(60),
    );
    std::fs::write(&stop, "").expect("stop the running app");
    let reload = wait_for(
        &events,
        &mut seen,
        named("run.reload"),
        Duration::from_secs(300),
    );
    assert_eq!(reload["ok"], true, "{seen:?}");
    let reported = names(&seen[mark..]);
    assert_eq!(
        reported.first().map(String::as_str),
        Some("run.app.exit"),
        "the app that exited during the build is reported before its successor: {seen:?}"
    );
    assert_eq!(
        reported.get(1).map(String::as_str),
        Some("run.app.start"),
        "{seen:?}"
    );
    let exit = seen[mark..]
        .iter()
        .find(|event| event["event"] == "run.app.exit")
        .expect("the exit");
    assert_eq!(exit["code"], 0, "it exited as asked: {seen:?}");
    drop(session);
}

/// A worker hot-reload project builds and starts its host under the
/// application's floors, so a `[platforms]` table FLUI cannot honour stops
/// `flui run` before anything is built, wherever in the project it runs
/// (ADR-0244 §10).
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
        "[app]\nname = \"host\"\nversion = \"0.1.0\"\nid = \"com.example.host\"\n\n\
         [hot_reload]\nhost_package = \"host\"\nworker_package = \"worker\"\n\
         worker_lib = \"worker\"\nlogic_watch = \"worker/src\"\n\n\
         [platforms.ios]\nmin = \"12.0\"\n",
    )
    .expect("application config");
    // From the workspace root with hot reload, and from the host package
    // below `flui.toml` without it: both paths find the same floors.
    let mut failures = Vec::new();
    for (dir, extra) in [
        (root.to_path_buf(), None),
        (root.join("host"), Some("--no-hot-reload")),
    ] {
        let output = cargo_bin_cmd!("flui")
            .current_dir(&dir)
            .env("CARGO_NET_OFFLINE", "true")
            .env("CARGO_TARGET_DIR", root.join("target"))
            .args(["run", "--device", "desktop"])
            .args(extra)
            .timeout(std::time::Duration::from_secs(60))
            .output()
            .expect("CLI");
        let stderr = String::from_utf8_lossy(&output.stderr);
        if output.status.success()
            || !stderr
                .contains("[platforms.ios] min = \"12.0\" is below FLUI's floor for ios, 13.0")
        {
            failures.push(format!(
                "{} {extra:?}: {:?}: {stderr}",
                dir.display(),
                output.status
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
    assert!(
        !root.join("target").exists(),
        "nothing is built before the refusal"
    );
}

/// On a Linux or Windows desktop an application that opens links or files
/// is refused before it is built unless flui keeps one instance of it; with
/// the feature on it builds and runs.
#[cfg(any(target_os = "linux", windows))]
#[test]
fn a_desktop_run_that_opens_links_needs_one_instance() {
    let tmp = TempDir::new().expect("fixture");
    let root = tmp.path();
    package(
        &root.join("flui-native"),
        "flui-native",
        "[features]\nsingle-instance = []\n",
    );
    std::fs::write(root.join("flui-native/src/lib.rs"), "").expect("native library");
    package(
        &root.join("facade"),
        "flui",
        "[features]\nsingle-instance = [\"flui-native/single-instance\"]\n\
         [dependencies]\nflui-native = { path = \"../flui-native\" }\n",
    );
    std::fs::write(root.join("facade/src/lib.rs"), "").expect("facade identity");
    let app = root.join("app");
    std::fs::create_dir_all(&app).expect("application directory");
    std::fs::write(
        app.join("flui.toml"),
        "[app]\nname = \"app\"\nversion = \"0.1.0\"\nid = \"com.example.app\"\n",
    )
    .expect("application config");
    let mut failures = Vec::new();
    for (features, runs) in [("", false), (", features = [\"single-instance\"]", true)] {
        package(
            &app,
            "app",
            &format!(
                "[workspace]\n[dependencies]\nflui = {{ path = \"../facade\"{features} }}\n\
                 [package.metadata.flui.declares]\nschema = 3\nurl-schemes = [\"notes\"]\n"
            ),
        );
        // CI denies warnings, and an application that names no item of
        // `flui` warns of an unused dependency.
        std::fs::write(app.join("src/main.rs"), "use flui as _;\nfn main() {}\n")
            .expect("application main");
        let output = run(&app)
            .timeout(std::time::Duration::from_secs(120))
            .output()
            .expect("CLI");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let built = app
            .join("target/release")
            .join(format!("app{}", std::env::consts::EXE_SUFFIX))
            .exists();
        let refused = stderr.contains("`notes:` links")
            && stderr.contains("features = [\"single-instance\"]");
        if output.status.success() != runs || built != runs || refused == runs {
            failures.push(format!(
                "features `{features}`: {:?}\nstdout: {stdout}\nstderr: {stderr}",
                output.status
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
