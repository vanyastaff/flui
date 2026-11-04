//! `flui record`, end to end against a stand-in agent.
//!
//! The stand-in binds the endpoint in the environment `flui record` reads, answers the
//! protocol's hello, and plays one recording: `record.start`'s answer, `record` events, and
//! `record.stop`'s. The real server's half is pinned by `flui-runtime`'s agent tests; what is
//! pinned here is the CLI's: the role it connects in, the file it writes, and how it ends.
#![cfg(unix)]

use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use flui_protocol::input_recording::{Header, Row};
use flui_protocol::version::PROTOCOL_VERSION;
use serde_json::{Value, json};
use tempfile::TempDir;

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

const HEADER: &str =
    r#"{"flui_recording":3,"clock":"virtual","windows":[{"window":1,"scale_factor":2.0}]}"#;

/// A data row at `t`.
fn deadline(t: u64) -> Value {
    json!({ "deadline": { "window": 1, "t": t, "member": 1 } })
}

/// The `record` event carrying `row` of recording `recording`.
fn row(recording: u64, row: &Value) -> String {
    json!({ "event": "record", "params": { "recording": recording, "window": "w1", "row": row } })
        .to_string()
}

/// One accepted conversation, past its hello.
struct Peer {
    lines: std::io::Lines<BufReader<UnixStream>>,
    out: UnixStream,
    /// The client's hello.
    hello: String,
}

impl Peer {
    fn accept(listener: &UnixListener) -> Self {
        let (stream, _) = listener.accept().expect("accept");
        let out = stream.try_clone().expect("clone");
        let mut lines = BufReader::new(stream).lines();
        let hello = lines.next().expect("a hello").expect("a line");
        assert!(hello.contains(TOKEN), "{hello}");
        let mut peer = Self { lines, out, hello };
        peer.send(&format!(
            r#"{{"hello":{{"protocol":"{PROTOCOL_VERSION}"}}}}"#
        ));
        peer
    }

    fn send(&mut self, line: &str) {
        writeln!(self.out, "{line}").expect("send");
    }

    /// The next request: its id and method.
    fn request(&mut self) -> (Value, String, Value) {
        let line = self.lines.next().expect("a request").expect("a line");
        let request: Value = serde_json::from_str(&line).expect("json");
        (
            request["id"].clone(),
            request["method"].as_str().expect("a method").to_owned(),
            request["params"].clone(),
        )
    }

    fn answer(&mut self, id: &Value, result: &Value) {
        self.send(&json!({ "id": id, "result": result }).to_string());
    }
}

/// A recording started as `flui record` asks for one: `record.start` answered with number 7.
fn started(peer: &mut Peer) -> Value {
    let (id, method, params) = peer.request();
    assert_eq!(method, "record.start");
    let header: Value = serde_json::from_str(HEADER).expect("header");
    peer.answer(&id, &json!({ "recording": 7, "header": header }));
    params
}

/// `record.stop` of recording 7, answered with `summary` after `rows`.
fn stopped(peer: &mut Peer, rows: &[Value], summary: &Value) {
    let (id, method, params) = peer.request();
    assert_eq!(method, "record.stop");
    assert_eq!(params, json!({ "recording": 7 }));
    for value in rows {
        peer.send(&row(7, value));
    }
    peer.answer(&id, summary);
}

fn flui_record(dir: &Path, args: &[&str]) -> Child {
    Command::new(env!("CARGO_BIN_EXE_flui"))
        .current_dir(dir)
        .env("FLUI_AGENT_ENDPOINT", dir.join("agent.sock"))
        .env("FLUI_AGENT_TOKEN", TOKEN)
        .arg("record")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn flui record")
}

/// What a recording file holds, read with the format's own types.
fn read(path: &Path) -> (Header, Vec<Row>) {
    let text = std::fs::read_to_string(path).expect("the recording");
    let mut lines = text.lines();
    let header = serde_json::from_str(lines.next().expect("a header")).expect("a header");
    let rows = lines
        .map(|line| serde_json::from_str::<Row>(line).expect("a row"))
        .collect();
    (header, rows)
}

fn rows(values: &[Value]) -> Vec<Row> {
    values
        .iter()
        .map(|value| serde_json::from_value::<Row>(value.clone()).expect("a row"))
        .collect()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Waits until `path` holds `lines` lines.
fn wait_for_lines(path: &Path, lines: usize) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while std::fs::read_to_string(path).map_or(0, |text| text.lines().count()) < lines {
        assert!(
            Instant::now() < deadline,
            "{} never held {lines} lines",
            path.display()
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_recording_the_app_ends_is_written_whole_and_counted() {
    let tmp = TempDir::new().unwrap();
    let listener = UnixListener::bind(tmp.path().join("agent.sock")).unwrap();
    let child = flui_record(tmp.path(), &["out.jsonl", "--window", "w1", "--keep-text"]);

    let mut peer = Peer::accept(&listener);
    // An observer: it records beside a driver.
    assert!(
        peer.hello.contains(r#""role":"observer""#),
        "{}",
        peer.hello
    );
    let params = started(&mut peer);
    assert_eq!(
        params,
        json!({ "window": "w1", "layers": ["input"], "keep_text": true })
    );
    let played = [
        deadline(1),
        json!({ "gap": { "rows": 2 } }),
        deadline(4),
        // The window closed.
        json!({ "end": { "lost_rows": 2 } }),
    ];
    peer.send(&row(7, &played[0]));
    // A row of another recording is not this one's.
    peer.send(&row(8, &deadline(99)));
    for value in &played[1..] {
        peer.send(&row(7, value));
    }
    stopped(
        &mut peer,
        &[],
        &json!({ "rows": 2, "lost_rows": 2, "complete": true }),
    );

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let (header, written) = read(&tmp.path().join("out.jsonl"));
    assert_eq!(header, serde_json::from_str::<Header>(HEADER).unwrap());
    assert_eq!(written, rows(&played));
    let said = stderr(&output);
    assert!(said.contains("Recorded 2 rows"), "{said}");
    assert!(said.contains("the window closed"), "{said}");
    assert!(said.contains("2 rows were lost"), "{said}");
}

#[test]
fn ctrl_c_stops_a_recording_after_the_rows_admitted_before_it() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("out.jsonl");
    let listener = UnixListener::bind(tmp.path().join("agent.sock")).unwrap();
    let child = flui_record(tmp.path(), &["out.jsonl"]);

    let mut peer = Peer::accept(&listener);
    assert_eq!(
        started(&mut peer),
        json!({ "layers": ["input"] }),
        "text is redacted unless asked for"
    );
    peer.send(&row(7, &deadline(1)));
    wait_for_lines(&out, 2);
    // The Ctrl-C listener is up once the header is written; a signal before it would kill.
    thread::sleep(Duration::from_millis(300));
    let interrupted = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(interrupted.success());
    // The rows admitted before the stop and the end row come before its answer.
    let tail = [deadline(2), json!({ "end": { "lost_rows": 0 } })];
    stopped(
        &mut peer,
        &tail,
        &json!({ "rows": 2, "lost_rows": 0, "complete": true }),
    );

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let (_, written) = read(&out);
    assert_eq!(written, rows(&[deadline(1), deadline(2), tail[1].clone()]));
    let said = stderr(&output);
    assert!(said.contains("Recorded 2 rows"), "{said}");
    assert!(!said.contains("the window closed"), "{said}");
}

#[test]
fn a_recording_cut_off_keeps_its_rows_and_fails() {
    let tmp = TempDir::new().unwrap();
    let listener = UnixListener::bind(tmp.path().join("agent.sock")).unwrap();
    let child = flui_record(tmp.path(), &["out.jsonl"]);

    let mut peer = Peer::accept(&listener);
    started(&mut peer);
    peer.send(&row(7, &deadline(1)));
    // The app goes.
    drop(peer);

    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let (_, written) = read(&tmp.path().join("out.jsonl"));
    assert_eq!(written, rows(&[deadline(1)]));
    let said = stderr(&output);
    assert!(said.contains("broke off after 1 rows"), "{said}");
}

#[test]
fn a_recording_the_app_refuses_says_why_and_writes_nothing() {
    let tmp = TempDir::new().unwrap();
    let listener = UnixListener::bind(tmp.path().join("agent.sock")).unwrap();
    let child = flui_record(tmp.path(), &["out.jsonl"]);

    let mut peer = Peer::accept(&listener);
    let (id, method, _) = peer.request();
    assert_eq!(method, "record.start");
    peer.send(
        &json!({ "id": id, "error": {
            "code": "busy",
            "message": "this runtime holds 8 recordings",
            "retry": "soon",
        } })
        .to_string(),
    );

    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let said = stderr(&output);
    assert!(said.contains("this runtime holds 8 recordings"), "{said}");
    let written = std::fs::read_to_string(tmp.path().join("out.jsonl")).unwrap_or_default();
    assert!(written.is_empty(), "{written}");
}
