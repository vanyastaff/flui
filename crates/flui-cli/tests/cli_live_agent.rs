//! The counter scenario on a live device: `flui create` a counter app, run it
//! with `flui run --agent`, and through `flui mcp` read its semantics tree,
//! press Increment and read the count go from 0 to 1.
//!
//! One scenario for every device `flui run --device` takes: the desktop it
//! runs on, an Android device or emulator, a browser, an iOS Simulator. It is
//! gated behind `FLUI_CLI_LIVE_DEVICE` (`desktop` or a device id from
//! `flui devices`) because it builds and launches a real application; the
//! nightly platform workflow sets it on each runner. Where FLUI cannot yet
//! drive the device, the scenario fails as that device's entry of
//! `docs/gaps.md` ([`KNOWN_GAPS`]), and fails outright once the entry closes.
//!
//! On the desktop a second scenario drives the acceptance application
//! (`tests/fixtures/acceptance_app/`) the same way: from its home page to
//! Settings and back, through `flui mcp` alone.
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tempfile::TempDir;

/// The `docs/gaps.md` entry the scenario fails as on a device platform (as
/// `flui devices --json` names it) where FLUI cannot drive an app yet.
const KNOWN_GAPS: &[(&str, &str)] = &[(
    "ios",
    "the-driver-agent-cannot-reach-an-app-in-the-ios-simulator",
)];

/// How long the app may take to build and answer the agent: `run.app.start`
/// comes before the build on the desktop, and a cold build of the counter
/// takes minutes. The job's own timeout bounds a hung one.
const LAUNCH: Duration = Duration::from_mins(45);

/// How long the app may take to show a window with content once it answers,
/// and the count to change after the press.
const SETTLE: Duration = Duration::from_secs(120);

fn flui() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_flui"));
    command.env("NO_COLOR", "1");
    command
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("BUG: crates/flui-cli sits two levels under the workspace root")
        .to_path_buf()
}

/// The platform `flui devices --json` names `device` on, `desktop` for this
/// machine.
fn platform_of(device: &str) -> String {
    let output = flui()
        .args(["--json", "devices"])
        .output()
        .expect("run flui devices");
    let listing = String::from_utf8_lossy(&output.stdout);
    listing
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|event| event["event"] == "device" && event["id"] == device)
        .and_then(|event| event["platform"].as_str().map(str::to_owned))
        .unwrap_or_else(|| panic!("`flui devices` lists no device {device:?}:\n{listing}"))
}

/// A counter project generated against this checkout, with the workspace's
/// lockfile so it builds offline.
fn counter_project(workdir: &Path) -> PathBuf {
    let repo_root = repo_root();
    let name = "flui_live_agent";
    let status = flui()
        .current_dir(&repo_root)
        .env("CARGO_NET_OFFLINE", "true")
        .args(["create", name, "--org", "com.test", "--template", "counter"])
        .args(["--no-check"])
        .arg(format!("--local={}", repo_root.display()))
        .arg("--path")
        .arg(workdir)
        .status()
        .expect("run flui create");
    assert!(status.success(), "flui create failed");
    let project = workdir.join(name);
    std::fs::copy(repo_root.join("Cargo.lock"), project.join("Cargo.lock"))
        .expect("seed the project with the workspace's resolved versions");
    project
}

/// `flui mcp` in `project`, one JSON-RPC request and its reply at a time.
struct Mcp {
    child: Child,
    stdin: std::process::ChildStdin,
    replies: std::io::Lines<BufReader<std::process::ChildStdout>>,
    next_id: u64,
}

impl Mcp {
    fn start(project: &Path) -> Self {
        let mut child = flui()
            .current_dir(project)
            .env_remove("FLUI_AGENT_ENDPOINT")
            .env_remove("FLUI_AGENT_TOKEN")
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("start flui mcp");
        let stdin = child.stdin.take().expect("BUG: stdin is piped");
        let replies = BufReader::new(child.stdout.take().expect("BUG: stdout is piped")).lines();
        Self {
            child,
            stdin,
            replies,
            next_id: 1,
        }
    }

    /// The text of the tool's result, or the error it reported.
    fn call(&mut self, name: &str, arguments: Value) -> Result<String, String> {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call",
                              "params": { "name": name, "arguments": arguments } });
        writeln!(self.stdin, "{request}").expect("write to flui mcp");
        let line = self
            .replies
            .next()
            .expect("flui mcp replied")
            .expect("read from flui mcp");
        let reply: Value = serde_json::from_str(&line).expect("flui mcp replies in JSON");
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .map_or_else(|| reply.to_string(), str::to_owned);
        if reply["result"]["isError"] == false {
            Ok(text)
        } else {
            Err(text)
        }
    }

    /// The window's semantics tree, retried until `found` picks something
    /// out of it or [`SETTLE`] passes, failing at once when `flui run`
    /// reports the app ended.
    fn read_until<T>(
        &mut self,
        run: &mpsc::Receiver<Value>,
        what: &str,
        found: impl Fn(&Value) -> Option<T>,
    ) -> T {
        let deadline = Instant::now() + SETTLE;
        loop {
            assert_running(run, what);
            let last = match self.call("flui_read", json!({})) {
                Ok(text) => {
                    let tree: Value = serde_json::from_str(&text).expect("the tree is JSON");
                    if let Some(found) = found(&tree) {
                        return found;
                    }
                    text
                }
                Err(error) => error,
            };
            assert!(
                Instant::now() < deadline,
                "no {what} within {SETTLE:?}; the last read: {last}"
            );
            std::thread::sleep(Duration::from_millis(500));
        }
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `flui run` and everything it launched: the `cargo run` under it and the
/// app under that. Dropping it, on success or on a failed assertion, stops
/// the whole tree: Ctrl-C first, so `flui run` stops the app the way a
/// developer's Ctrl-C does (an Android session force-stops its package),
/// then a kill of whatever is left.
struct Session {
    child: Child,
}

impl Session {
    fn spawn(mut command: Command) -> Self {
        // Its own process group, so the interrupt reaches the tree and not
        // the test runner.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        Self {
            child: command.spawn().expect("start flui run"),
        }
    }

    /// Waits up to `within` for `flui run` to end.
    #[cfg(unix)]
    fn wait_up_to(&mut self, within: Duration) {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if !matches!(self.child.try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use rustix::process::{Pid, Signal, kill_process_group};
            let group = Pid::from_child(&self.child);
            let _ = kill_process_group(group, Signal::INT);
            self.wait_up_to(Duration::from_secs(15));
            // The app and `cargo run` share the group; whatever outlived
            // the interrupt goes now.
            let _ = kill_process_group(group, Signal::KILL);
        }
        // No console interrupt reaches one process tree on Windows; the
        // tree is killed whole.
        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .args(["/T", "/F", "/PID"])
                .arg(self.child.id().to_string())
                .output();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Fails the scenario if `flui run` reported, since the last look, that the
/// app ended or the run failed: waiting longer for `what` cannot help.
fn assert_running(run: &mpsc::Receiver<Value>, what: &str) {
    while let Ok(event) = run.try_recv() {
        if matches!(event["event"].as_str(), Some("run.app.exit" | "error")) {
            panic!("the app ended before the {what}: {event}");
        }
    }
}

/// Every node of `tree`, depth first.
fn nodes(tree: &Value) -> Vec<&Value> {
    fn walk<'a>(node: &'a Value, into: &mut Vec<&'a Value>) {
        into.push(node);
        for child in node["children"].as_array().into_iter().flatten() {
            walk(child, into);
        }
    }
    let mut all = Vec::new();
    for root in tree["roots"].as_array().into_iter().flatten() {
        walk(root, &mut all);
    }
    all
}

/// Whether `node` or a node under it is named `name`.
fn names(node: &Value, name: &str) -> bool {
    node["name"] == name
        || node["value"] == name
        || node["children"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|child| names(child, name))
}

/// The count the counter shows: the one node named with a number alone.
fn count(tree: &Value) -> Option<u64> {
    nodes(tree).into_iter().find_map(|node| {
        let label = node["name"].as_str().or(node["value"].as_str())?;
        label.parse::<u64>().ok()
    })
}

/// `flui run --agent --device <device>` in `project`, its JSON events on the
/// returned channel, and whether the app started: an `Err` carries the
/// message `flui run` reported instead. `env` is added to the run's
/// environment.
fn launch(
    project: &Path,
    device: &str,
    env: &[(&str, &Path)],
) -> (Session, mpsc::Receiver<Value>, Result<(), String>) {
    let mut command = flui();
    command
        .current_dir(project)
        .env("CARGO_NET_OFFLINE", "true")
        .args(["--json", "run", "--agent", "--no-hot-reload", "--device"])
        .arg(device)
        .stdin(Stdio::null())
        .stdout(Stdio::piped());
    for (key, value) in env {
        command.env(key, value);
    }
    let mut run = Session::spawn(command);
    let (events, received) = mpsc::channel::<Value>();
    let stdout = run.child.stdout.take().expect("BUG: stdout is piped");
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            eprintln!("flui run: {line}");
            if let Ok(event) = serde_json::from_str::<Value>(&line)
                && events.send(event).is_err()
            {
                break;
            }
        }
    });
    let started = loop {
        let Ok(event) = received.recv() else {
            break Err("flui run ended before the app started".to_owned());
        };
        match event["event"].as_str() {
            Some("run.app.start" | "run.android.start" | "run.web.open") => break Ok(()),
            Some("error") => break Err(event["message"].to_string()),
            _ => {}
        }
    };
    (run, received, started)
}

/// `flui mcp` in `project`, once the app it runs answers through its agent.
fn connect(project: &Path, run: &mpsc::Receiver<Value>) -> Mcp {
    let mut mcp = Mcp::start(project);
    // The app builds and starts; it answers once it serves its agent.
    let deadline = Instant::now() + LAUNCH;
    while let Err(error) = mcp.call("flui_windows", json!({})) {
        assert_running(run, "answer from the app's agent");
        assert!(
            Instant::now() < deadline,
            "the app's agent did not answer within {LAUNCH:?}: {error}"
        );
        std::thread::sleep(Duration::from_secs(2));
    }
    mcp
}

/// The handle of the button named `name` that accepts `invoke`.
fn button(tree: &Value, name: &str) -> Option<String> {
    nodes(tree)
        .into_iter()
        .find(|node| {
            node["role"] == "button"
                && node["actions"]
                    .as_array()
                    .is_some_and(|actions| actions.iter().any(|action| action == "invoke"))
                && names(node, name)
        })
        .and_then(|node| node["id"].as_str().map(str::to_owned))
}

#[test]
fn an_agent_presses_the_counter_on_a_live_device() {
    let Ok(device) = std::env::var("FLUI_CLI_LIVE_DEVICE") else {
        eprintln!(
            "skipping an_agent_presses_the_counter_on_a_live_device: set FLUI_CLI_LIVE_DEVICE \
             to `desktop` or a device id from `flui devices` to run it on that device"
        );
        return;
    };
    let platform = platform_of(&device);
    let known_gap = KNOWN_GAPS
        .iter()
        .find(|(on, _)| *on == platform)
        .map(|&(_, gap)| gap);
    let workdir = TempDir::new().expect("scaffold workdir");
    let project = counter_project(workdir.path());

    // Dropped last, after `mcp`: on every exit, a failed assertion included.
    let (_run, received, started) = launch(&project, &device, &[]);

    if let Some(gap) = known_gap {
        match started {
            Err(message) if message.contains(&format!("docs/gaps.md#{gap}")) => {
                eprintln!("expected failure: docs/gaps.md#{gap} is open on {platform}");
                return;
            }
            Err(message) => panic!("the scenario failed as {message}, expected docs/gaps.md#{gap}"),
            Ok(()) => panic!(
                "docs/gaps.md#{gap} is closed on {platform}: remove it from KNOWN_GAPS and \
                 update the entry"
            ),
        }
    }
    started.unwrap_or_else(|message| panic!("{message}"));

    let mut mcp = connect(&project, &received);
    let increment = mcp.read_until(&received, "Increment button with a count of 0", |tree| {
        (count(tree) == Some(0)).then(|| button(tree, "Increment"))?
    });
    mcp.call(
        "flui_act",
        json!({ "element": increment, "action": "invoke" }),
    )
    .unwrap_or_else(|error| panic!("pressing Increment failed: {error}"));
    mcp.read_until(&received, "count of 1", |tree| {
        (count(tree) == Some(1)).then_some(())
    });
}

/// The acceptance application (`tests/fixtures/acceptance_app/`) as a
/// project of its own: the counter's manifest with the facade's `material`
/// and `serde` features and the application's codec, and the fixture's
/// `app.rs` as its library, launched with its documents under the storage
/// directory `storage` instead of the fixture's own.
fn acceptance_project(workdir: &Path, storage: &str) -> PathBuf {
    let project = counter_project(workdir);
    let manifest_path = project.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest_path).expect("read the project manifest");
    let mut manifest = text
        .parse::<toml::Table>()
        .expect("the generated manifest is TOML");
    let dependencies = manifest
        .get_mut("dependencies")
        .and_then(toml::Value::as_table_mut)
        .expect("BUG: the counter template declares dependencies");
    let facade = dependencies
        .get_mut("flui")
        .and_then(toml::Value::as_table_mut)
        .expect("BUG: a local project names the facade by a table");
    facade.insert(
        "features".to_owned(),
        toml::Value::Array(vec!["material".into(), "serde".into()]),
    );
    let mut serde = toml::Table::new();
    serde.insert("version".to_owned(), "1".into());
    serde.insert(
        "features".to_owned(),
        toml::Value::Array(vec!["derive".into()]),
    );
    dependencies.insert("serde".to_owned(), toml::Value::Table(serde));
    dependencies.insert("serde_json".to_owned(), "1".into());
    std::fs::write(
        &manifest_path,
        toml::to_string(&manifest).expect("serialize the project manifest"),
    )
    .expect("write the project manifest");

    let source = project.join("src");
    std::fs::copy(
        repo_root().join("tests/fixtures/acceptance_app/app.rs"),
        source.join("app.rs"),
    )
    .expect("copy the acceptance application");
    std::fs::write(
        source.join("lib.rs"),
        "//! The acceptance application, run by `src/main.rs`.\n\n\
         #[allow(dead_code, reason = \"the headless rows use the rest\")]\npub mod app;\n",
    )
    .expect("write the library root");
    std::fs::write(
        source.join("main.rs"),
        format!(
            "use flui_live_agent::app;\n\n\
             fn main() -> Result<(), flui::LaunchError> {{\n    \
             let dir = flui::platform::AppDataDir::try_new({storage:?})\n        \
             .expect(\"BUG: the scenario's storage directory name is valid\");\n    \
             let config = flui::AppConfig::new().with_storage_dir(dir);\n    \
             flui::run_app(\n        \
             app::AcceptanceApp::new(app::Faults::NONE),\n        \
             flui::AppLaunch::new().config(config),\n    \
             )\n}}\n"
        ),
    )
    .expect("write the launcher");
    project
}

/// The acceptance application on the desktop, driven through `flui mcp` as
/// an agent drives it: read the tree, press Settings, see the Settings page
/// (a Back button, no Settings button), press Back, and read the home page
/// again; the animation clock answers in between. Only on the desktop: the
/// counter scenario covers the other devices.
#[test]
fn an_agent_drives_the_acceptance_app_on_the_desktop() {
    if std::env::var("FLUI_CLI_LIVE_DEVICE").as_deref() != Ok("desktop") {
        eprintln!(
            "skipping an_agent_drives_the_acceptance_app_on_the_desktop: set \
             FLUI_CLI_LIVE_DEVICE=desktop to run it on this machine"
        );
        return;
    }
    let workdir = TempDir::new().expect("scaffold workdir");
    // A storage directory of the run's own: the user's acceptance
    // application keeps its session, and a page it left open cannot start
    // this run on it. Linux takes it under the work directory; the other
    // desktops resolve it in the user's data roots, which the guard empties.
    let storage = format!(
        "dev.flui.acceptance-app.live-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("BUG: the clock is past the epoch")
            .as_nanos()
    );
    let _documents = RunDocuments::of(&storage);
    let project = acceptance_project(workdir.path(), &storage);
    let data = workdir.path().join("data");
    let (_run, received, started) = launch(&project, "desktop", &[("XDG_DATA_HOME", &data)]);
    started.unwrap_or_else(|message| panic!("{message}"));

    let mut mcp = connect(&project, &received);
    let settings = mcp.read_until(&received, "home page with a Settings button", |tree| {
        button(tree, "Settings")
    });
    mcp.call("flui_motion", json!({}))
        .unwrap_or_else(|error| panic!("reading the animation clock failed: {error}"));
    mcp.call(
        "flui_act",
        json!({ "element": settings, "action": "invoke" }),
    )
    .unwrap_or_else(|error| panic!("pressing Settings failed: {error}"));
    let back = mcp.read_until(&received, "Settings page with a Back button", |tree| {
        button(tree, "Settings")
            .is_none()
            .then(|| button(tree, "Back"))?
    });
    mcp.call("flui_act", json!({ "element": back, "action": "invoke" }))
        .unwrap_or_else(|error| panic!("pressing Back failed: {error}"));
    mcp.read_until(&received, "home page again", |tree| {
        button(tree, "Back")
            .is_none()
            .then(|| button(tree, "Settings"))?
    });
}

/// The directories a desktop outside Linux keeps the scenario's documents
/// in (`flui_native::storage::data_dirs`), removed when the scenario ends.
struct RunDocuments {
    dirs: Vec<PathBuf>,
}

impl RunDocuments {
    fn of(storage: &str) -> Self {
        let roots: &[&str] = if cfg!(windows) {
            &["APPDATA", "LOCALAPPDATA"]
        } else if cfg!(target_os = "macos") {
            &["HOME"]
        } else {
            &[]
        };
        let dirs = roots
            .iter()
            .filter_map(std::env::var_os)
            .map(|root| {
                let root = PathBuf::from(root);
                if cfg!(target_os = "macos") {
                    root.join("Library/Application Support").join(storage)
                } else {
                    root.join(storage)
                }
            })
            .collect::<Vec<PathBuf>>();
        Self { dirs }
    }
}

impl Drop for RunDocuments {
    fn drop(&mut self) {
        for dir in &self.dirs {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
