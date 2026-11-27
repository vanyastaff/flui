//! `flui run --agent` and `flui mcp`, end to end on a fixture app.
//!
//! The fixture stands in for a FLUI app's driver agent (ADR-0239): it binds
//! the endpoint it is given, checks the launch token and answers the
//! protocol's hello and the four methods `flui mcp` calls. The real server's
//! behaviour is pinned by `flui-app`'s agent tests; what is pinned here is
//! the CLI's half: the feature and environment it launches the app with, the
//! session file it writes, and the MCP calls that reach the endpoint.
use std::io::Write as _;
#[cfg(unix)]
use std::io::{BufRead as _, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

use assert_cmd::cargo::cargo_bin_cmd;
use flui_protocol::version::PROTOCOL_VERSION;
use tempfile::TempDir;

/// The fixture app. It prints whether its facade was built with the
/// `driver` feature, and with `FIXTURE_EXIT` set exits right after.
const FIXTURE_MAIN: &str = r##"
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;

fn field(line: &str, key: &str) -> Option<String> {
    let at = line.find(key)? + key.len();
    let rest = &line[at..];
    let rest = rest.trim_start_matches('"');
    Some(rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.').collect())
}

fn main() {
    println!("FIXTURE_DRIVER {}", flui::DRIVER);
    // What started the process: `flui` when the run starts the built executable, `cargo` under
    // `cargo run`. Empty where `/proc` is not.
    let parent = std::fs::read_to_string(format!("/proc/{}/comm", std::os::unix::process::parent_id()))
        .unwrap_or_default();
    println!("FIXTURE_PARENT {}", parent.trim());
    println!("FIXTURE_LIBRARY_PATH {}", std::env::var("LD_LIBRARY_PATH").unwrap_or_default());
    if std::env::var_os("FIXTURE_EXIT").is_some() {
        return;
    }
    let endpoint = std::env::var("FLUI_AGENT_ENDPOINT").expect("endpoint");
    let token = std::env::var("FLUI_AGENT_TOKEN").expect("token");
    let listener = UnixListener::bind(&endpoint).expect("bind");
    for stream in listener.incoming() {
        let stream = stream.expect("accept");
        let mut writer = stream.try_clone().expect("clone");
        let mut lines = BufReader::new(stream).lines();
        let hello = lines.next().expect("hello").expect("line");
        if !hello.contains(&token) || !hello.contains(r#""protocol":"__PROTOCOL__""#) {
            continue; // the real server closes unread on a wrong token
        }
        writeln!(writer, r#"{{"hello":{{"protocol":"__PROTOCOL__"}}}}"#).unwrap();
        for line in lines {
            let line = line.expect("line");
            let id = field(&line, r#""id":"#).expect("id");
            let method = field(&line, r#""method":"#).expect("method");
            let reply = match method.as_str() {
                "windows.list" => {
                    // An event and a late answer to some earlier request come first: the
                    // client takes the line that answers its own id.
                    writeln!(writer, r#"{{"event":"gap","params":{{"missed":1}}}}"#).unwrap();
                    writeln!(writer, r#"{{"id":999,"result":{{"windows":[]}}}}"#).unwrap();
                    format!(r#"{{"id":{id},"result":{{"windows":["w1"]}}}}"#)
                }
                "semantics.read" => format!(
                    r#"{{"id":{id},"result":{{"protocol":"__PROTOCOL__","nodes":[{{"id":"e1","role":"button"}}]}}}}"#
                ),
                "semantics.act" => {
                    writeln!(writer, r#"{{"id":{id},"result":{{"via":"semantic"}}}}"#).unwrap();
                    println!("FIXTURE_ACTED {line}");
                    return;
                }
                _ => format!(r#"{{"id":{id},"error":{{"code":"invalid_argument","message":"no such method","retry":"never"}}}}"#),
            };
            writeln!(writer, "{reply}").unwrap();
        }
    }
}
"##;

/// A fixture app depending on `dependency`, a stand-in facade with a
/// `driver` feature.
fn fixture(root: &Path, dependency: &str) -> std::path::PathBuf {
    fixture_as(root, dependency, dependency)
}

/// A fixture app naming `dependency` under the key `key` (`package =` when
/// they differ), as an app that renames the facade does.
fn fixture_as(root: &Path, dependency: &str, key: &str) -> std::path::PathBuf {
    let facade = root.join("facade");
    std::fs::create_dir_all(facade.join("src")).expect("fixture");
    std::fs::write(
        facade.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{dependency}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
             [lib]\nname = \"flui\"\n[features]\ndriver = []\n"
        ),
    )
    .expect("fixture");
    std::fs::write(
        facade.join("src/lib.rs"),
        "pub const DRIVER: bool = cfg!(feature = \"driver\");\n",
    )
    .expect("fixture");
    let app = root.join("app");
    std::fs::create_dir_all(app.join("src")).expect("fixture");
    // A renamed dependency is reached by its key, not its library name.
    let (declaration, krate) = if key == dependency {
        (
            format!("{key} = {{ path = \"../facade\" }}"),
            "flui".to_owned(),
        )
    } else {
        (
            format!("{key} = {{ package = \"{dependency}\", path = \"../facade\" }}"),
            key.replace('-', "_"),
        )
    };
    std::fs::write(
        app.join("Cargo.toml"),
        format!(
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
             [workspace]\n[dependencies]\n{declaration}\n"
        ),
    )
    .expect("fixture");
    std::fs::write(
        app.join("src/main.rs"),
        FIXTURE_MAIN
            .replace("__PROTOCOL__", &PROTOCOL_VERSION.to_string())
            .replace("flui::DRIVER", &format!("{krate}::DRIVER")),
    )
    .expect("fixture");
    app
}

fn flui() -> Command {
    Command::new(env!("CARGO_BIN_EXE_flui"))
}

#[test]
fn an_optimized_agent_build_is_refused_where_no_crate_keeps_the_agent() {
    let tmp = TempDir::new().unwrap();
    let app = fixture(tmp.path(), "flui-widgets");

    let optimized = cargo_bin_cmd!("flui")
        .current_dir(&app)
        .args(["run", "--agent", "--release", "--no-hot-reload"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&optimized.get_output().stderr).into_owned();
    assert!(stderr.contains("names neither"), "{stderr}");
}

/// The lines the app printed, from `flui run --json`'s events.
#[cfg(unix)]
fn app_lines(events: impl std::io::BufRead) -> Vec<String> {
    events
        .lines()
        .filter_map(|line| {
            let event: serde_json::Value =
                serde_json::from_str(&line.expect("an event line")).ok()?;
            (event["event"] == "run.app.log")
                .then(|| event["line"].as_str().expect("a log line").to_owned())
        })
        .collect()
}

#[cfg(unix)]
#[test]
fn an_optimized_agent_build_turns_on_the_driver_feature_and_a_debug_one_needs_none() {
    for (args, key, driver) in [
        (&["--release"][..], "flui", true),
        // The feature goes through the key an app renamed the facade to.
        (&["--release"][..], "ui", true),
        (&["--profile", "release"][..], "flui", true),
        (&[][..], "flui", false),
    ] {
        let tmp = TempDir::new().unwrap();
        let app = fixture_as(tmp.path(), "flui", key);
        let mut run = flui()
            .current_dir(&app)
            .env("CARGO_TARGET_DIR", tmp.path().join("target"))
            .env("CARGO_NET_OFFLINE", "true")
            .env("FIXTURE_EXIT", "1")
            .args(["--json", "run", "--agent", "--no-hot-reload"])
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let lines = app_lines(BufReader::new(run.stdout.take().unwrap()));
        run.wait().unwrap();
        assert!(
            lines.contains(&format!("FIXTURE_DRIVER {driver}")),
            "{args:?} as `{key}`: {lines:?}"
        );
    }
}

/// `flui_reload` reaches the `flui run --agent` that started the app, at the run endpoint its
/// session file names (ADR-0401): a reload of the build the app runs leaves it running, a save
/// restarts it once on the executable its build wrote, and a reload of a broken build
/// answers the compiler's errors. The answer is the report the run writes as
/// its `run.reload` event.
#[cfg(unix)]
#[test]
fn an_agent_reloads_the_app_through_the_run_that_started_it() {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let tmp = TempDir::new().unwrap();
    let app = fixture(tmp.path(), "flui");
    let target = tmp.path().join("target");
    let mut run = flui()
        .current_dir(&app)
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_NET_OFFLINE", "true")
        .args(["--json", "run", "--agent", "--no-build-overlay"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = run.stdout.take().unwrap();
    let (sender, events) = mpsc::channel::<serde_json::Value>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(event) = serde_json::from_str::<serde_json::Value>(&line)
                && sender.send(event).is_err()
            {
                break;
            }
        }
    });
    let next = |name: &str| -> serde_json::Value {
        let deadline = Instant::now() + Duration::from_secs(300);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = events
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("no `{name}` in time"));
            if event["event"] == name {
                return event;
            }
        }
    };
    let ready = next("run.agent.ready");
    next("run.watch");
    let session = std::path::PathBuf::from(ready["session"].as_str().unwrap());
    let listed: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&session).unwrap()).unwrap();
    assert!(
        listed["run"].is_string(),
        "the session file names the run endpoint: {listed}"
    );
    assert_eq!(
        ready["run"], listed["run"],
        "`run.agent.ready` reports the run endpoint the file names"
    );

    let mut mcp = flui()
        .current_dir(&app)
        .env("CARGO_TARGET_DIR", &target)
        .env_remove("FLUI_AGENT_ENDPOINT")
        .env_remove("FLUI_AGENT_TOKEN")
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = mcp.stdin.take().unwrap();
    let mut replies = BufReader::new(mcp.stdout.take().unwrap()).lines();
    let mut tool =
        |id: u64, name: &str, arguments: serde_json::Value| -> (bool, serde_json::Value) {
            let call = serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": name, "arguments": arguments } });
            writeln!(stdin, "{call}").unwrap();
            let reply: serde_json::Value =
                serde_json::from_str(&replies.next().expect("a reply").unwrap()).unwrap();
            let text = reply["result"]["content"][0]["text"].as_str().unwrap();
            (
                reply["result"]["isError"] == true,
                serde_json::from_str(text).unwrap_or_else(|_| serde_json::json!(text)),
            )
        };

    // The events up to the next `run.reload`, that report among them.
    let until_reload = || -> (serde_json::Value, Vec<serde_json::Value>) {
        let deadline = Instant::now() + Duration::from_secs(300);
        let mut seen = Vec::new();
        loop {
            let event = events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("a `run.reload` in time");
            if event["event"] == "run.reload" {
                return (event, seen);
            }
            seen.push(event);
        }
    };
    let stopped = |seen: &[serde_json::Value]| seen.iter().any(|e| e["event"] == "run.app.stop");
    let unchanged = serde_json::json!("unchanged");
    let restarted = serde_json::json!({ "applied": { "tier": "process_restart" } });

    // The app's connection opens before the restart, so the call after it has a dead one to
    // replace.
    let (failed, windows) = tool(1, "flui_windows", serde_json::json!({}));
    assert!(!failed, "{windows}");
    // The run idles before it is asked: a report counts from the call, not from the last event.
    std::thread::sleep(Duration::from_millis(1500));
    let asked = Instant::now();
    let (failed, report) = tool(2, "flui_reload", serde_json::json!({}));
    let took = u64::try_from(asked.elapsed().as_millis()).unwrap();
    assert!(!failed, "{report}");
    assert!(
        report["elapsed_ms"].as_u64().is_some_and(|ms| ms <= took),
        "the reload took at most the {took} ms of its call: {report}"
    );
    // Nothing was edited since the app started: the build is the one it runs.
    assert_eq!(report["outcome"], unchanged, "{report}");
    let (reported, seen) = until_reload();
    assert_eq!(reported["outcome"], report["outcome"], "{reported}");
    assert_eq!(reported["elapsed_ms"], report["elapsed_ms"], "{reported}");
    assert!(!stopped(&seen), "an unchanged build leaves the app running");

    // A save rebuilds and restarts the app, which runs the executable the build wrote, started
    // by the run itself rather than by a `cargo run` that would build again.
    let source = app.join("src/main.rs");
    let mut edited = std::fs::read_to_string(&source).unwrap();
    edited.push_str("\n// edited\n");
    std::fs::write(&source, edited).unwrap();
    let (saved, seen) = until_reload();
    assert_eq!(saved["outcome"], restarted, "{saved}");
    assert!(stopped(&seen), "the save's build replaced the app");
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut parent = None;
    let library_path = loop {
        let event = events
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("the restarted app");
        let line = event["line"].as_str().unwrap_or_default();
        if let Some(name) = line.strip_prefix("FIXTURE_PARENT ") {
            parent = Some(name.to_owned());
        } else if let Some(path) = line.strip_prefix("FIXTURE_LIBRARY_PATH ") {
            break path.to_owned();
        }
    };
    if cfg!(target_os = "linux") {
        assert_eq!(
            parent.as_deref(),
            Some("flui"),
            "the run starts the built executable itself"
        );
        // The loader searches the build's own libraries, as under `cargo run`.
        let deps = target.join("debug/deps");
        assert!(
            std::env::split_paths(&library_path).any(|dir| dir == deps),
            "{library_path}"
        );
    }
    // The save and a reload of the same edit restart the app once.
    let (failed, report) = tool(3, "flui_reload", serde_json::json!({}));
    assert!(!failed, "{report}");
    assert_eq!(report["outcome"], unchanged, "{report}");
    let (_, seen) = until_reload();
    assert!(!stopped(&seen), "the reload after the save kept the app");

    // An executable written since the app started, by a build cargo did not report to this run,
    // is a change: the reload starts it.
    std::fs::File::open(target.join("debug/app"))
        .unwrap()
        .set_modified(std::time::SystemTime::now() + Duration::from_secs(5))
        .unwrap();
    let (failed, report) = tool(7, "flui_reload", serde_json::json!({}));
    assert!(!failed, "{report}");
    assert_eq!(report["outcome"], restarted, "{report}");
    let (_, seen) = until_reload();
    assert!(stopped(&seen), "the reload replaced the app");
    // An app that is gone runs no build: the reload starts the one cargo did not redo.
    let started = seen
        .iter()
        .find(|event| event["event"] == "run.app.start")
        .and_then(|event| event["pid"].as_u64())
        .expect("the replaced app's start");
    assert!(
        Command::new("kill")
            .arg(started.to_string())
            .status()
            .unwrap()
            .success()
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    while events
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .expect("the killed app's exit")["event"]
        != "run.app.exit"
    {}
    let (failed, report) = tool(8, "flui_reload", serde_json::json!({}));
    assert!(!failed, "{report}");
    assert_eq!(report["outcome"], restarted, "{report}");
    let (_, seen) = until_reload();
    assert!(
        seen.iter().any(|event| event["event"] == "run.app.start"),
        "the reload started the app again"
    );

    // A call that acts, which is never sent twice, reaches the restarted app, not the dead
    // connection to the one the reload stopped: the fixture answers `motion.set` with its own
    // error. A named window keeps the call from reading the windows first.
    let (_, motion) = tool(4, "flui_motion", serde_json::json!({ "window": "w1" }));
    assert_eq!(
        motion["error"]["code"], "invalid_argument",
        "the restarted app answers: {motion}"
    );
    std::fs::write(app.join("src/main.rs"), "fn main() { this is not rust }\n").unwrap();
    let (failed, report) = tool(
        5,
        "flui_reload",
        serde_json::json!({ "at_least": "restart" }),
    );
    assert!(
        !failed,
        "a failed build is an outcome, not an error: {report}"
    );
    let errors = &report["outcome"]["failed"]["diagnostics"];
    assert!(
        errors[0]["message"].as_str().is_some_and(|m| !m.is_empty()),
        "the compiler's error comes back: {report}"
    );
    // A restart stops the app before it builds, as `R` does; the rebuild a save starts keeps
    // it running through a failed build.
    let mut stopped_first = false;
    loop {
        let event = events
            .recv_timeout(Duration::from_secs(60))
            .expect("the restart's report");
        stopped_first |= event["event"] == "run.app.stop";
        if event["event"] == "run.reload" && event["elapsed_ms"] == report["elapsed_ms"] {
            break;
        }
    }
    assert!(stopped_first, "the restart stopped the app first");

    let (failed, refused) = tool(6, "flui_reload", serde_json::json!({ "at_least": "hot" }));
    assert!(failed, "an unknown tier is refused: {refused}");
    assert_eq!(refused["error"]["code"], "invalid_argument", "{refused}");

    drop(stdin);
    mcp.wait().unwrap();
    run.kill().unwrap();
    run.wait().unwrap();
}

#[test]
fn mcp_without_a_running_app_says_how_to_start_one() {
    let tmp = TempDir::new().unwrap();
    let mut child = flui()
        .current_dir(tmp.path())
        .env_remove("FLUI_AGENT_ENDPOINT")
        .env_remove("FLUI_AGENT_TOKEN")
        .env("CARGO_TARGET_DIR", tmp.path().join("target"))
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"flui_windows"}}}}"#
    )
    .unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    let reply: serde_json::Value =
        serde_json::from_slice(output.stdout.split(|b| *b == b'\n').next().unwrap()).unwrap();
    assert_eq!(reply["result"]["isError"], true);
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("flui run --agent"), "{text}");
}

#[cfg(unix)]
#[test]
fn an_agent_reads_and_taps_the_app_run_with_agent_through_mcp() {
    let tmp = TempDir::new().unwrap();
    let app = fixture(tmp.path(), "flui");
    let target = tmp.path().join("target");

    let mut run = flui()
        .current_dir(&app)
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_NET_OFFLINE", "true")
        .args(["--json", "run", "--agent", "--no-hot-reload"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let events = BufReader::new(run.stdout.take().unwrap());
    let mut ready = None;
    let mut acted = None;

    // The session is announced before the app builds; the MCP client then
    // launches `flui mcp` in the same project, which waits for the endpoint.
    let mut lines = events.lines();
    for line in lines.by_ref() {
        let event: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        if event["event"] == "run.agent.ready" {
            ready = Some(event);
            break;
        }
    }
    let ready = ready.expect("run.agent.ready");
    let session = std::path::PathBuf::from(ready["session"].as_str().unwrap());
    let session_text = std::fs::read_to_string(&session).unwrap();
    // A run that does not rebuild on saves serves no reload, and names no run endpoint.
    assert!(ready.get("run").is_none(), "{ready}");
    assert!(!session_text.contains("\"run\""), "{session_text}");
    assert!(
        !ready.to_string().contains(
            session_text
                .split("\"token\": \"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap()
        ),
        "the token stays out of the event stream"
    );

    let mut mcp = flui()
        .current_dir(&app)
        .env("CARGO_TARGET_DIR", &target)
        .env_remove("FLUI_AGENT_ENDPOINT")
        .env_remove("FLUI_AGENT_TOKEN")
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = mcp.stdin.take().unwrap();
    let mut replies = BufReader::new(mcp.stdout.take().unwrap()).lines();
    let mut ask = |message: serde_json::Value| -> serde_json::Value {
        writeln!(stdin, "{message}").unwrap();
        serde_json::from_str(&replies.next().expect("a reply").unwrap()).unwrap()
    };
    let call = |id: u64, name: &str, arguments: serde_json::Value| {
        serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call",
                            "params": { "name": name, "arguments": arguments } })
    };

    let windows = ask(call(1, "flui_windows", serde_json::json!({})));
    assert_eq!(windows["result"]["isError"], false, "{windows}");
    assert!(
        windows["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("w1")
    );

    // No `window`: the app has exactly one.
    let tree = ask(call(2, "flui_read", serde_json::json!({ "max_depth": 3 })));
    assert!(
        tree["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("\"button\"")
    );

    let tap = ask(call(
        3,
        "flui_act",
        serde_json::json!({ "element": "e1", "action": "invoke" }),
    ));
    assert_eq!(tap["result"]["isError"], false, "{tap}");
    drop(stdin);
    mcp.wait().unwrap();

    for line in lines {
        let event: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        if event["event"] == "run.app.log"
            && event["line"].as_str().unwrap().starts_with("FIXTURE_ACTED")
        {
            acted = Some(event["line"].as_str().unwrap().to_owned());
        }
    }
    run.wait().unwrap();
    let acted = acted.expect("the app saw the act");
    assert!(acted.contains(r#""target":{"element":"w1/e1"}"#), "{acted}");
    assert!(!session.exists(), "the session file goes with the run");
}

/// `flui mcp` reaches an Android app through the loopback port `adb forward` holds: the session
/// file names the port, and the conversation is the same as over a local endpoint. The test's
/// listener stands in for `adb`, which carries each connection to the app's abstract socket.
#[test]
fn mcp_reaches_an_android_app_through_its_forwarded_port() {
    use std::io::{BufRead as _, BufReader};
    use std::net::TcpListener;

    let tmp = TempDir::new().unwrap();
    let forwarded = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = forwarded.local_addr().unwrap().port();
    let token = "a5".repeat(32);
    let session = tmp.path().join("flui-agent.json");
    std::fs::write(
        &session,
        serde_json::json!({
            "endpoint": { "forwarded": format!("127.0.0.1:{port}") },
            "token": token,
            "pid": 1,
        })
        .to_string(),
    )
    .unwrap();

    let protocol = PROTOCOL_VERSION.to_string();
    let app = std::thread::spawn(move || {
        let (stream, _) = forwarded.accept().unwrap();
        let mut writer = stream.try_clone().unwrap();
        let mut lines = BufReader::new(stream).lines();
        let hello = lines.next().unwrap().unwrap();
        assert!(hello.contains(&token), "the hello carries the token");
        writeln!(writer, r#"{{"hello":{{"protocol":"{protocol}"}}}}"#).unwrap();
        let request: serde_json::Value =
            serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
        assert_eq!(request["method"], "windows.list");
        writeln!(
            writer,
            r#"{{"id":{},"result":{{"windows":["w1"]}}}}"#,
            request["id"]
        )
        .unwrap();
    });

    let mut mcp = flui()
        .current_dir(tmp.path())
        .env_remove("FLUI_AGENT_ENDPOINT")
        .env_remove("FLUI_AGENT_TOKEN")
        .arg("mcp")
        .arg("--session")
        .arg(&session)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = mcp.stdin.take().unwrap();
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"flui_windows"}}}}"#
    )
    .unwrap();
    drop(stdin);
    let output = mcp.wait_with_output().unwrap();
    app.join().expect("the app answered");
    let reply: serde_json::Value =
        serde_json::from_slice(output.stdout.split(|b| *b == b'\n').next().unwrap()).unwrap();
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("w1"), "{text}");
}
