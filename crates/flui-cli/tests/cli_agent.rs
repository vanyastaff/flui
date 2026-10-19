//! `flui run --agent` and `flui mcp`, end to end on a fixture app.
//!
//! The fixture stands in for an application with the `dev-agent` feature: it
//! binds the endpoint it is given, checks the launch token and answers the
//! framing `flui_devtools::agent` documents. The real server's behaviour is
//! pinned by `flui-devtools`' `agent_endpoint` tests; what is pinned here is
//! the CLI's half: the feature and environment it launches the app with, the
//! session file it writes, and the MCP calls that reach the endpoint.
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};

use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::TempDir;

const FIXTURE_MAIN: &str = r##"
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;

fn field(line: &str, key: &str) -> Option<String> {
    let at = line.find(key)? + key.len();
    let rest = &line[at..];
    let rest = rest.trim_start_matches('"');
    Some(rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect())
}

fn main() {
    let endpoint = std::env::var("FLUI_AGENT_ENDPOINT").expect("endpoint");
    let token = std::env::var("FLUI_AGENT_TOKEN").expect("token");
    assert!(cfg!(feature = "dev-agent"), "launched without the feature");
    let listener = UnixListener::bind(&endpoint).expect("bind");
    for stream in listener.incoming() {
        let stream = stream.expect("accept");
        let mut writer = stream.try_clone().expect("clone");
        let mut lines = BufReader::new(stream).lines();
        let hello = lines.next().expect("hello").expect("line");
        if !hello.contains(&token) {
            continue; // the real server closes unread on a wrong token
        }
        writeln!(writer, r#"{{"hello":{{"protocol":"0.4"}}}}"#).unwrap();
        for line in lines {
            let line = line.expect("line");
            let id = field(&line, r#""id":"#).expect("id");
            let op = field(&line, r#""op":"#).expect("op");
            let reply = match op.as_str() {
                "windows" => format!(r#"{{"id":{id},"result":{{"windows":["w1"]}}}}"#),
                "read" => format!(
                    r#"{{"id":{id},"result":{{"protocol":"0.4","nodes":[{{"id":"e1","role":"button"}}]}}}}"#
                ),
                "act" => {
                    writeln!(writer, r#"{{"id":{id},"result":{{}}}}"#).unwrap();
                    println!("FIXTURE_ACTED {line}");
                    return;
                }
                _ => format!(r#"{{"id":{id},"error":{{"code":"invalid_argument","message":"unknown op","retry":"never"}}}}"#),
            };
            writeln!(writer, "{reply}").unwrap();
        }
    }
}
"##;

fn fixture(root: &Path, with_feature: bool) -> std::path::PathBuf {
    let facade = root.join("facade");
    std::fs::create_dir_all(facade.join("src")).expect("fixture");
    std::fs::write(
        facade.join("Cargo.toml"),
        "[package]\nname = \"flui\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("fixture");
    std::fs::write(facade.join("src/lib.rs"), "").expect("fixture");
    let app = root.join("app");
    std::fs::create_dir_all(app.join("src")).expect("fixture");
    let features = if with_feature {
        "[features]\ndev-agent = []\n"
    } else {
        ""
    };
    std::fs::write(
        app.join("Cargo.toml"),
        format!(
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
             [workspace]\n{features}[dependencies]\nflui = {{ path = \"../facade\" }}\n"
        ),
    )
    .expect("fixture");
    std::fs::write(app.join("src/main.rs"), FIXTURE_MAIN).expect("fixture");
    app
}

fn flui() -> Command {
    Command::new(env!("CARGO_BIN_EXE_flui"))
}

#[test]
fn agent_is_refused_in_release_and_without_the_apps_feature() {
    let tmp = TempDir::new().unwrap();
    let app = fixture(tmp.path(), false);

    let release = cargo_bin_cmd!("flui")
        .current_dir(&app)
        .args(["run", "--agent", "--release"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&release.get_output().stderr).into_owned();
    assert!(stderr.contains("cannot be used with"), "{stderr}");

    let profile = cargo_bin_cmd!("flui")
        .current_dir(&app)
        .args(["run", "--agent", "--profile", "release"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&profile.get_output().stderr).into_owned();
    assert!(stderr.contains("debug builds only"), "{stderr}");

    let missing = cargo_bin_cmd!("flui")
        .current_dir(&app)
        .args(["run", "--agent"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&missing.get_output().stderr).into_owned();
    assert!(stderr.contains("dev-agent"), "{stderr}");
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
    let app = fixture(tmp.path(), true);
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
    assert!(
        acted.contains(r#""element":"e1""#) && acted.contains(r#""window":"w1""#),
        "{acted}"
    );
    assert!(!session.exists(), "the session file goes with the run");
}
