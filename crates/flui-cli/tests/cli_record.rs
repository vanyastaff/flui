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
    r#"{"flui_recording":6,"clock":"virtual","windows":[{"window":1,"scale_factor":2.0}]}"#;

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

/// The `record_native` event carrying `line` of native `stream` of recording `recording`.
fn native(recording: u64, stream: u32, line: &Value) -> String {
    json!({ "event": "record_native", "params": {
        "recording": recording, "window": "w1", "stream": stream, "line": line,
    } })
    .to_string()
}

/// A registry reading of `device` read as `kind`.
fn reading(device: u64, kind: &str, contact: u64) -> Value {
    json!({ "row": "reading", "reading": {
        "surface": 1, "device": device, "contact": contact, "kind": kind, "action": "move",
        "time": { "tick_ms": { "tick": 10, "observed_ms": 10 } }, "position": [5.0, 4.0],
    } })
}

#[test]
fn a_native_recording_writes_each_stream_with_the_devices_its_rows_read() {
    use flui_protocol::native_recording::Row as NativeRow;

    let tmp = TempDir::new().unwrap();
    let listener = UnixListener::bind(tmp.path().join("agent.sock")).unwrap();
    let child = flui_record(tmp.path(), &["out.jsonl", "--native", "native"]);

    let mut peer = Peer::accept(&listener);
    assert_eq!(started(&mut peer), json!({ "layers": ["input", "native"] }));
    let registry = json!({ "flui_native_recording": 3, "backend": "x11" });
    let add = json!({ "row": "add_device", "device": 1, "kind": "mouse", "time_ns": 51 });
    let gap = json!({ "gap": { "rows": 3 } });
    peer.send(&native(7, 0, &json!({ "header": registry })));
    peer.send(&native(
        7,
        1,
        &json!({ "header": {
        "flui_native_recording": 3, "backend": "x11", "stream": "app",
    } }),
    ));
    peer.send(&native(7, 0, &json!({ "row": add })));
    peer.send(&native(7, 0, &json!({ "row": reading(1, "mouse", 9) })));
    // A line of another recording is not this one's.
    peer.send(&native(8, 0, &json!({ "row": reading(4, "touch", 9) })));
    peer.send(&native(7, 0, &gap));
    // A stream whose header never came has no file to place its rows in.
    peer.send(&native(7, 2, &json!({ "row": reading(3, "mouse", 9) })));
    peer.send(&row(7, &json!({ "end": { "lost_rows": 0 } })));
    // Admitted before the end, sent after it.
    peer.send(&native(7, 0, &json!({ "row": reading(2, "pen", 11) })));
    stopped(
        &mut peer,
        &[],
        &json!({ "rows": 0, "lost_rows": 0, "native_lost_rows": 4, "complete": true }),
    );

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let dir = tmp.path().join("native");
    let text = std::fs::read_to_string(dir.join("0.jsonl")).expect("stream 0");
    let lines: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).expect("json"))
        .collect();
    assert_eq!(lines.len(), 5, "{text}");
    assert_eq!(
        lines[0],
        json!({ "flui_native_recording": 3, "backend": "x11", "devices": [
            { "device": 1, "kinds": ["mouse"] },
            { "device": 2, "kinds": ["pen"] },
        ] })
    );
    let native_row = |value: &Value| serde_json::from_value::<NativeRow>(value.clone()).unwrap();
    assert_eq!(native_row(&lines[1]), native_row(&add));
    assert_eq!(native_row(&lines[2]), native_row(&reading(1, "mouse", 9)));
    assert_eq!(lines[3], gap, "the gap stays where the stream lost rows");
    assert_eq!(native_row(&lines[4]), native_row(&reading(2, "pen", 11)));
    let app = std::fs::read_to_string(dir.join("1.jsonl")).expect("stream 1");
    assert_eq!(app.lines().count(), 1, "{app}");

    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    assert_eq!(files, ["0.jsonl", "1.jsonl", "index.json"]);
    let index: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("index.json")).unwrap()).unwrap();
    assert_eq!(
        index,
        json!({ "streams": [
            { "stream": 0, "file": "0.jsonl", "backend": "x11", "journals": "registry",
              "rows": 3, "lost_rows": 3 },
            { "stream": 1, "file": "1.jsonl", "backend": "x11", "journals": "app",
              "rows": 0, "lost_rows": 0 },
            { "stream": 2, "file": null, "backend": null, "journals": null,
              "rows": 0, "lost_rows": 1 },
        ] })
    );
    let said = stderr(&output);
    assert!(said.contains("Wrote 2 native streams"), "{said}");
    assert!(said.contains("native stream 0 lost 3 rows"), "{said}");
    assert!(said.contains("native stream 2 sent no header"), "{said}");
}

/// A native recording of `send`'s lines, all of stream 0, ended by the app: the directory, the
/// index and what the command said.
fn native_session(send: impl FnOnce(&mut dyn FnMut(&Value))) -> (TempDir, Value, String) {
    let tmp = TempDir::new().unwrap();
    let listener = UnixListener::bind(tmp.path().join("agent.sock")).unwrap();
    let child = flui_record(tmp.path(), &["out.jsonl", "--native", "native"]);
    let mut peer = Peer::accept(&listener);
    started(&mut peer);
    send(&mut |line| peer.send(&native(7, 0, line)));
    peer.send(&row(7, &json!({ "end": { "lost_rows": 0 } })));
    stopped(
        &mut peer,
        &[],
        &json!({ "rows": 0, "lost_rows": 0, "complete": true }),
    );
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let index = std::fs::read_to_string(tmp.path().join("native/index.json")).unwrap();
    let index: Value = serde_json::from_str(&index).unwrap();
    (tmp, index, stderr(&output))
}

/// The lines of native stream 0's file, each with its newline.
fn native_lines(tmp: &TempDir) -> Vec<String> {
    let text = std::fs::read_to_string(tmp.path().join("native/0.jsonl")).expect("stream 0");
    text.split_inclusive('\n').map(str::to_owned).collect()
}

/// The bytes `value` takes as a line of a native file, its newline included.
fn line_bytes(value: &impl serde::Serialize) -> usize {
    serde_json::to_vec(value).unwrap().len() + 1
}

/// An input method's commit of `text`, as a native row.
fn commit(text: String) -> flui_protocol::native_recording::Row {
    use flui_protocol::native_recording::{Ime, ImeEvent, Row};
    Row::Ime {
        ime: Ime::Event(ImeEvent::Commit(text)),
        redacted: false,
    }
}

/// The native header line of an x11 stream journaling `stream`, and the bytes the file's header
/// takes before it declares a device.
fn native_header(stream: &str) -> (Value, usize) {
    use flui_protocol::native_recording::Header;
    let header = json!({ "flui_native_recording": 3, "backend": "x11", "stream": stream });
    let bytes = line_bytes(&serde_json::from_value::<Header>(header.clone()).unwrap());
    (json!({ "header": header }), bytes)
}

/// Commit rows that bring a file whose header takes `header` bytes to `left` bytes under the
/// recording's bound: rows as long as a line comfortably holds, then one that fills the rest.
fn commits_leaving(header: usize, left: usize) -> Vec<Value> {
    use flui_protocol::native_recording::{MAX_LINE_BYTES, MAX_RECORDING_BYTES};
    let empty = line_bytes(&commit(String::new()));
    let full_text = MAX_LINE_BYTES - 128;
    let full = empty + full_text;
    let body = MAX_RECORDING_BYTES - left - header;
    let fulls = (body - empty) / full;
    let filler = body - fulls * full - empty;
    let row = |text: String| json!({ "row": serde_json::to_value(commit(text)).unwrap() });
    let mut rows = vec![row("x".repeat(full_text)); fulls];
    rows.push(row("x".repeat(filler)));
    rows
}

/// A stream past the byte bound ends at a `cut` row, its file within what a reader takes. Each
/// row the stream sent is counted once: written, or dropped at the cut; a `gap` line is no row,
/// and its rows are lost, also where it is the line that met the bound.
#[test]
fn a_native_stream_past_the_byte_bound_ends_at_a_cut_and_counts_each_row_once() {
    use flui_protocol::native_recording::{CutReason, MAX_RECORDING_BYTES, Row};

    let (header, header_bytes) = native_header("window");
    // 70 bytes left: room for the cut a writer keeps after each row (64), not for a gap line
    // and that room.
    let rows = commits_leaving(header_bytes, 70);
    let written_rows = rows.len() as u64;
    let (tmp, index, said) = native_session(|send| {
        send(&header);
        for row in &rows {
            send(row);
        }
        send(&json!({ "gap": { "rows": 3 } }));
        send(&json!({ "row": serde_json::to_value(commit("after".to_owned())).unwrap() }));
        send(&json!({ "row": { "row": "reopen" } }));
    });

    let lines = native_lines(&tmp);
    let bytes: usize = lines.iter().map(String::len).sum();
    assert!(bytes <= MAX_RECORDING_BYTES, "{bytes} bytes");
    let last = lines.last().unwrap();
    assert_eq!(
        serde_json::from_str::<Row>(last).ok(),
        Some(Row::Cut {
            reason: CutReason::Bytes
        }),
        "the last line, {} bytes, is the cut",
        last.len()
    );
    assert_eq!(
        lines.len() as u64,
        written_rows + 2,
        "the header, the rows, the cut"
    );
    let stream = &index["streams"][0];
    assert_eq!(stream["rows"], json!(written_rows));
    assert_eq!(stream["lost_rows"], json!(3), "the gap's rows, once");
    assert_eq!(
        stream["cut"],
        json!({ "bound": "bytes", "dropped": 2 }),
        "the two rows after the cut; the gap that met it is no row"
    );
    assert!(said.contains("reached the 67108864 bytes"), "{said}");
    assert!(
        said.contains("2 rows from there were not written"),
        "{said}"
    );
}

/// A stream with more rows than a native recording holds ends at a `cut` row that keeps the
/// file within the row bound.
#[test]
fn a_native_stream_past_the_row_bound_ends_at_a_cut() {
    use flui_protocol::native_recording::{CutReason, MAX_ROWS, Row};

    let (header, _) = native_header("app");
    let reopen = json!({ "row": { "row": "reopen" } });
    let (tmp, index, _) = native_session(|send| {
        send(&header);
        for _ in 0..=MAX_ROWS {
            send(&reopen);
        }
    });

    let lines = native_lines(&tmp);
    assert_eq!(lines.len() - 1, MAX_ROWS, "the rows, the cut last");
    assert_eq!(
        serde_json::from_str::<Row>(lines.last().unwrap()).ok(),
        Some(Row::Cut {
            reason: CutReason::Rows
        })
    );
    let stream = &index["streams"][0];
    assert_eq!(stream["rows"], json!(MAX_ROWS - 1));
    assert_eq!(stream["cut"], json!({ "bound": "rows", "dropped": 2 }));
}

/// A row longer than a line may be is lost in place of a `gap`, and the rows after it are
/// written; a header longer than a line may be opens no file, and its rows are lost.
#[test]
fn a_native_line_past_the_line_bound_is_not_written() {
    use flui_protocol::native_recording::MAX_LINE_BYTES;

    let (header, _) = native_header("window");
    let (tmp, index, _) = native_session(|send| {
        send(&header);
        send(&json!({ "row": serde_json::to_value(commit("x".repeat(MAX_LINE_BYTES))).unwrap() }));
        send(&json!({ "row": serde_json::to_value(commit("after".to_owned())).unwrap() }));
    });
    let lines = native_lines(&tmp);
    let sizes: Vec<usize> = lines.iter().map(String::len).collect();
    assert_eq!(sizes.len(), 3, "line sizes {sizes:?}");
    assert_eq!(
        lines[1], "{\"gap\":{\"rows\":1}}\n",
        "a gap of the lost row"
    );
    let stream = &index["streams"][0];
    assert_eq!(
        (&stream["rows"], &stream["lost_rows"]),
        (&json!(1), &json!(1))
    );

    let (tmp, index, said) = native_session(|send| {
        send(&json!({ "header": {
            "flui_native_recording": 3, "backend": "x11", "provenance": "x".repeat(MAX_LINE_BYTES),
        } }));
        send(&json!({ "row": { "row": "reopen" } }));
        send(&json!({ "gap": { "rows": 2 } }));
    });
    assert!(!tmp.path().join("native/0.jsonl").exists(), "no file");
    let stream = &index["streams"][0];
    assert_eq!(stream["header_too_long"], json!(true));
    assert_eq!(stream["file"], Value::Null);
    assert_eq!(stream["lost_rows"], json!(3), "the row and the gap's two");
    assert!(said.contains("header is longer than"), "{said}");
}

/// Near the byte bound, each row is admitted with the bytes the header grows by to declare the
/// device it reads first, so the file, its header filled in, stays within the bound.
#[test]
fn a_native_file_counts_the_header_its_rows_declare() {
    use flui_protocol::native_recording::{CutReason, Header, MAX_RECORDING_BYTES, Row};

    let (header, header_bytes) = native_header("registry");
    let filler = commits_leaving(header_bytes, 3000);
    let (tmp, index, _) = native_session(|send| {
        send(&header);
        for row in &filler {
            send(row);
        }
        for device in 1..=60 {
            send(&json!({ "row": reading(device, "mouse", 9) }));
        }
    });

    let lines = native_lines(&tmp);
    let bytes: usize = lines.iter().map(String::len).sum();
    assert!(bytes <= MAX_RECORDING_BYTES, "{bytes} bytes");
    assert_eq!(
        serde_json::from_str::<Row>(lines.last().unwrap()).ok(),
        Some(Row::Cut {
            reason: CutReason::Bytes
        })
    );
    let readings = lines.len() - 2 - filler.len();
    assert!(readings > 0, "some readings were written before the cut");
    let header: Header = serde_json::from_str(&lines[0]).unwrap();
    assert_eq!(
        header.devices.len(),
        readings,
        "each written reading's device is declared"
    );
    assert_eq!(index["streams"][0]["cut"]["bound"], json!("bytes"));
}
