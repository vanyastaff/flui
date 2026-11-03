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
