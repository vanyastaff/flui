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
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|event| event["event"] == "device" && event["id"] == device)
        .and_then(|event| event["platform"].as_str().map(str::to_owned))
        .unwrap_or_else(|| panic!("`flui devices` lists no device {device:?}"))
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

    let mut command = flui();
    command
        .current_dir(&project)
        .env("CARGO_NET_OFFLINE", "true")
        .args(["--json", "run", "--agent", "--no-hot-reload", "--device"])
        .arg(&device)
        .stdin(Stdio::null())
        .stdout(Stdio::piped());
    // Dropped last, after `mcp`: on every exit, a failed assertion included.
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

    let mut mcp = Mcp::start(&project);
    // The app builds and starts; it answers once it serves its agent.
    let deadline = Instant::now() + LAUNCH;
    while let Err(error) = mcp.call("flui_windows", json!({})) {
        assert_running(&received, "answer from the app's agent");
        assert!(
            Instant::now() < deadline,
            "the app's agent did not answer within {LAUNCH:?}: {error}"
        );
        std::thread::sleep(Duration::from_secs(2));
    }
    let button = mcp.read_until(&received, "Increment button with a count of 0", |tree| {
        let button = nodes(tree).into_iter().find(|node| {
            node["role"] == "button"
                && node["actions"]
                    .as_array()
                    .is_some_and(|actions| actions.iter().any(|action| action == "invoke"))
                && names(node, "Increment")
        })?;
        (count(tree) == Some(0)).then(|| button["id"].as_str().map(str::to_owned))?
    });
    mcp.call("flui_act", json!({ "element": button, "action": "invoke" }))
        .unwrap_or_else(|error| panic!("pressing Increment failed: {error}"));
    mcp.read_until(&received, "count of 1", |tree| {
        (count(tree) == Some(1)).then_some(())
    });
}
