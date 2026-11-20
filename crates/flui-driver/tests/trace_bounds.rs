//! A trace's edges: events lost on the way, a start that fails half way, what a replay refuses
//! and maps, and the bounds a reader holds a trace and its native recordings to.

use std::path::Path;
use std::time::Duration;

use flui_driver::protocol::input::{ActionSequence, InputSource};
use flui_driver::protocol::native_recording::{MAX_LINE_BYTES, MAX_RECORDING_BYTES, MAX_ROWS};
use flui_driver::trace::{self, GapReason, Row, Trace, TraceOptions, native};
use flui_driver::{Driver, Error, Launch, Locator, Report};

use crate::fixtures::{Counter, Probe};

fn counter() -> Driver {
    Driver::launch(
        Launch::headless(|| Counter {
            probe: Probe::default(),
        })
        .name("counter"),
    )
    .expect("the headless app starts")
}

/// The header of a trace that holds the native layer.
const NATIVE_HEADER: &str = r#"{"flui_trace":1,"protocol":"0.28","target":{"kind":"attached","os":"any"},"isolation":"shared","fidelity":{"positions":"exact"},"started_unix_ms":0,"privacy":"redact","layers":["input","native"]}"#;

/// A native recording of one row, as a trace's `native/<n>.jsonl` holds it.
const ONE_ROW: &str =
    "{\"flui_native_recording\":4,\"backend\":\"headless\"}\n{\"row\":\"click_settings\"}\n";

/// A trace directory holding the native layer, with `index` as its index and `files` beside it.
fn native_trace(
    root: &Path,
    name: &str,
    index: &str,
    files: &[(&str, &str)],
) -> std::path::PathBuf {
    let dir = root.join(format!("{name}.flui-trace"));
    std::fs::create_dir_all(dir.join("native")).expect("the directories");
    std::fs::write(
        dir.join("trace.jsonl"),
        format!("{NATIVE_HEADER}\n{{\"end\":{{}}}}\n"),
    )
    .expect("the rows");
    std::fs::write(dir.join("native").join("index.json"), index).expect("the index");
    for (file, text) in files {
        std::fs::write(dir.join("native").join(file), text).expect("a stream");
    }
    dir
}

fn refusal(result: flui_driver::Result<Vec<native::NativeRecording>>) -> String {
    match result {
        Err(Error::Unreadable { message, .. }) => message,
        other => panic!("refused as unreadable, not {other:?}"),
    }
}

#[test]
fn native_recordings_are_read_from_the_index_under_their_one_name() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let read = |name: &str, index: &str, files: &[(&str, &str)]| {
        Trace::open(native_trace(root.path(), name, index, files))
            .expect("the trace reads")
            .native_recordings()
    };
    let one = read(
        "one",
        r#"{"streams":[{"stream":0,"file":"0.jsonl"}]}"#,
        &[("0.jsonl", ONE_ROW)],
    )
    .expect("the stream the index names");
    assert_eq!(one.len(), 1);
    let unindexed = read(
        "unindexed",
        r#"{"streams":[{"stream":0,"file":"0.jsonl"}]}"#,
        &[("0.jsonl", ONE_ROW), ("1.jsonl", "not even a recording")],
    )
    .expect("a file the index does not name is not read");
    assert_eq!(unindexed.len(), 1);
    let no_file = read("no_file", r#"{"streams":[{"stream":3}]}"#, &[])
        .expect("a stream with no file is passed over");
    assert_eq!(no_file.len(), 0, "{no_file:?}");

    let other_name = refusal(read(
        "other_name",
        r#"{"streams":[{"stream":0,"file":"00.jsonl"}]}"#,
        &[("00.jsonl", ONE_ROW)],
    ));
    assert!(
        other_name.contains("`0.jsonl`, not `00.jsonl`"),
        "{other_name}"
    );
    let outside = refusal(read(
        "outside",
        r#"{"streams":[{"stream":0,"file":"../../escape.jsonl"}]}"#,
        &[],
    ));
    assert!(outside.contains("not `../../escape.jsonl`"), "{outside}");
    let twice = refusal(read(
        "twice",
        r#"{"streams":[{"stream":0,"file":"0.jsonl"},{"stream":0,"file":"0.jsonl"}]}"#,
        &[("0.jsonl", ONE_ROW)],
    ));
    assert!(twice.contains("named twice"), "{twice}");
    // At the bounds, both are still read.
    let at_bound: Vec<String> = (0..trace::MAX_NATIVE_STREAMS)
        .map(|stream| format!("{{\"stream\":{stream}}}"))
        .collect();
    read(
        "at_bound",
        &format!("{{\"streams\":[{}]}}", at_bound.join(",")),
        &[],
    )
    .expect("as many streams as the bound");
    let empty = "{\"streams\":[],\"pad\":\"\"}";
    let full = format!(
        "{{\"streams\":[],\"pad\":\"{}\"}}",
        "x".repeat(usize::try_from(trace::MAX_NATIVE_INDEX_BYTES).expect("small") - empty.len())
    );
    assert_eq!(
        u64::try_from(full.len()).expect("small"),
        trace::MAX_NATIVE_INDEX_BYTES
    );
    read("full", &full, &[]).expect("an index as long as the bound");
    let many: Vec<String> = (0..=trace::MAX_NATIVE_STREAMS)
        .map(|stream| format!("{{\"stream\":{stream}}}"))
        .collect();
    let too_many = refusal(read(
        "many",
        &format!("{{\"streams\":[{}]}}", many.join(",")),
        &[],
    ));
    assert!(too_many.contains("past the 256"), "{too_many}");
    let long = format!(
        "{{\"streams\":[],\"pad\":\"{}\"}}",
        "x".repeat(usize::try_from(trace::MAX_NATIVE_INDEX_BYTES).expect("small"))
    );
    let too_long = refusal(read("long", &long, &[]));
    assert!(too_long.contains("bytes an index may have"), "{too_long}");

    let dir = native_trace(root.path(), "unfinished", "", &[]);
    std::fs::remove_file(dir.join("native").join("index.json")).expect("no index");
    let unfinished = refusal(
        Trace::open(&dir)
            .expect("the trace reads")
            .native_recordings(),
    );
    assert!(unfinished.contains("no index"), "{unfinished}");
}

#[cfg(unix)]
#[test]
fn a_native_stream_through_a_symbolic_link_is_refused() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let elsewhere = root.path().join("elsewhere.jsonl");
    std::fs::write(&elsewhere, ONE_ROW).expect("a file outside the trace");
    let index = r#"{"streams":[{"stream":0,"file":"0.jsonl"}]}"#;
    let dir = native_trace(root.path(), "linked_file", index, &[]);
    std::os::unix::fs::symlink(&elsewhere, dir.join("native").join("0.jsonl")).expect("a link");
    let linked = refusal(
        Trace::open(&dir)
            .expect("the trace reads")
            .native_recordings(),
    );
    assert!(linked.contains("symbolic link"), "{linked}");

    let outside = root.path().join("outside");
    std::fs::create_dir_all(&outside).expect("a directory outside the trace");
    std::fs::write(outside.join("index.json"), index).expect("an index");
    std::fs::write(outside.join("0.jsonl"), ONE_ROW).expect("a stream");
    let dir = native_trace(root.path(), "linked_dir", index, &[]);
    std::fs::remove_dir_all(dir.join("native")).expect("the native directory");
    std::os::unix::fs::symlink(&outside, dir.join("native")).expect("a link");
    let linked = refusal(
        Trace::open(&dir)
            .expect("the trace reads")
            .native_recordings(),
    );
    assert!(linked.contains("symbolic link"), "{linked}");
}

#[test]
fn a_native_recording_past_its_byte_or_row_budget_or_reading_an_undeclared_device_is_refused() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let path = root.path().join("0.jsonl");
    let header = "{\"flui_native_recording\":4,\"backend\":\"headless\"}\n";
    let refused = |text: String| {
        std::fs::write(&path, text).expect("the file");
        match native::read(&path) {
            Err(Error::Unreadable { message, line, .. }) => (message, line),
            other => panic!("refused, not {other:?}"),
        }
    };

    // A mouse move of device 9, which the header declares no kind of.
    let reading = r#"{"row":"reading","reading":{"device":9,"contact":1,"kind":"mouse","action":"move","time":{"epoch_ns":5}}}"#;
    std::fs::write(
        &path,
        format!(
            "{{\"flui_native_recording\":4,\"backend\":\"headless\",\"devices\":[{{\"device\":9,\"kinds\":[\"mouse\"]}}]}}\n{reading}\n"
        ),
    )
    .expect("the file");
    native::read(&path).expect("the same row with its device declared");
    let (message, line) = refused(format!("{header}{reading}\n"));
    assert!(message.contains("device 9"), "{message}");
    assert_eq!(line, Some(2));

    let row = "{\"row\":\"click_settings\"}\n";
    let rows = row.repeat(MAX_ROWS + 1);
    let (message, line) = refused(format!("{header}{rows}"));
    assert!(message.contains("rows"), "{message}");
    assert_eq!(line, Some(u64::try_from(MAX_ROWS).expect("small") + 2));

    // Lines just under the line bound, as many as take the recording past its byte bound.
    let pad = "x".repeat(MAX_LINE_BYTES - 64);
    let long_row = format!("{{\"row\":\"click_settings\",\"pad\":\"{pad}\"}}\n");
    let count = MAX_RECORDING_BYTES / long_row.len() + 1;
    let (message, _) = refused(format!("{header}{}", long_row.repeat(count)));
    assert!(message.contains("bytes"), "{message}");
}

#[test]
fn a_redacted_trace_replay_refuses_the_row_that_would_type_the_redaction() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let dir = root.path().join("typed.flui-trace");
    let mut driver = counter();
    driver
        .start_trace(TraceOptions::new(&dir))
        .expect("the trace starts");
    driver
        .main_window()
        .expect("a window")
        .locate(Locator::named("Increment"))
        .type_text("hunter2")
        .expect("typed");
    driver.windows().expect("a command after the typing");
    driver.finish_trace(&mut Report::new()).expect("finished");
    let trace = Trace::open(&dir).expect("it reads");
    let typed = trace
        .steps()
        .into_iter()
        .find(|step| step.command.method == "element.type_text")
        .expect("the typing")
        .command
        .seq;

    let report = trace::replay(&trace, &mut counter()).expect("the replay runs");
    let refused = report.refused.clone().expect("the typing is refused");
    assert_eq!(
        (refused.seq, refused.method.as_str()),
        (typed, "element.type_text")
    );
    assert!(refused.reason.contains("keep_text"), "{}", refused.reason);
    assert_eq!(
        report.replayed,
        typed - 1,
        "nothing after the refusal is sent"
    );
    assert!(!report.matched());

    // Text redacted inside a list (an input sequence's text source) is refused the same way.
    let dir = root.path().join("performed.flui-trace");
    let mut driver = counter();
    driver
        .start_trace(TraceOptions::new(&dir))
        .expect("the trace starts");
    driver
        .main_window()
        .expect("a window")
        .perform(ActionSequence::new(vec![InputSource::Text {
            text: "hunter2".into(),
        }]))
        .expect("typed");
    driver.finish_trace(&mut Report::new()).expect("finished");
    let report = trace::replay(&Trace::open(&dir).expect("it reads"), &mut counter())
        .expect("the replay runs");
    let refused = report.refused.expect("the input sequence is refused");
    assert_eq!(refused.method, "input.perform");
}

#[cfg(any(unix, windows))]
mod scripted {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use flui_driver::protocol::error::{ErrorCode, ErrorObject};
    use flui_driver::protocol::events::Event;
    use flui_driver::protocol::record::RecordLayer;
    use flui_driver::protocol::session::Capabilities;
    use flui_driver::testing::{AfterLines, Reply, ScriptedAgent};
    use serde_json::{Value, json};

    use super::*;

    fn native_line(line: Value) -> Value {
        json!({
            "event": "record_native",
            "params": { "recording": 1, "window": "w1", "stream": 0, "line": line },
        })
    }

    fn capabilities() -> Capabilities {
        let mut capabilities = Capabilities::default();
        capabilities.methods = [
            "windows.list",
            "window.placement",
            "record.start",
            "record.stop",
            "semantics.locate",
        ]
        .map(str::to_owned)
        .to_vec();
        capabilities.record = vec![RecordLayer::Input, RecordLayer::Native];
        capabilities
    }

    fn placement() -> Value {
        json!({
            "window": "w1", "size": { "width": 320.0, "height": 200.0 }, "scale": 1.0,
            "visible": true, "focused": true,
        })
    }

    fn recording() -> Value {
        json!({
            "recording": 1,
            "header": { "flui_recording": 6, "clock": "monotonic", "windows": [{ "window": 1, "scale_factor": 1.0 }] },
        })
    }

    fn attach(agent: &ScriptedAgent) -> Driver {
        Driver::attach(&agent.endpoint(), agent.token(), Duration::from_secs(5)).expect("attached")
    }

    #[test]
    fn events_lost_mid_trace_mark_every_stream_and_the_callers_count() {
        let lost = 6;
        let mut listed = 0;
        let agent = ScriptedAgent::start_with(
            capabilities(),
            Box::new(move |method, _| match method {
                "windows.list" => {
                    listed += 1;
                    // The first is the trace's own read of the main window, the second takes
                    // the stream's first lines.
                    if listed == 3 {
                        // More events than the connection keeps arrive before the answer: the
                        // oldest are dropped, and which they were is not known.
                        let events = (0..1024 + lost)
                            .map(|_| json!({ "event": "gap", "params": { "missed": 0 } }))
                            .collect();
                        Reply::LinesThenResult {
                            lines: events,
                            result: json!({ "windows": ["w1"] }),
                        }
                    } else {
                        Reply::Result(json!({ "windows": ["w1"] }))
                    }
                }
                "window.placement" => Reply::Result(placement()),
                "record.start" => Reply::ResultThenLines {
                    result: recording(),
                    lines: vec![
                        native_line(
                            json!({ "header": { "flui_native_recording": 4, "backend": "win32" } }),
                        ),
                        native_line(json!({ "row": { "row": "click_settings" } })),
                    ],
                    then: AfterLines::Serve,
                },
                "record.stop" => {
                    Reply::Result(json!({ "rows": 0, "lost_rows": 0, "complete": false }))
                }
                _ => Reply::Close,
            }),
        )
        .expect("the stand-in starts");
        let mut driver = attach(&agent);
        let root = tempfile::tempdir().expect("a temporary directory");
        let dir = root.path().join("lost.flui-trace");
        driver
            .start_trace(TraceOptions::new(&dir))
            .expect("the trace starts");
        driver.windows().expect("the stream's rows are taken");
        driver.windows().expect("the events that overflow");
        let (events, dropped) = driver.take_events();
        assert_eq!(dropped, lost, "the caller's count has the loss");
        assert_eq!(events.len(), 1024);
        assert!(events.iter().all(|event| matches!(event, Event::Gap(_))));
        driver.finish_trace(&mut Report::new()).expect("finished");

        let trace = Trace::open(&dir).expect("it reads");
        let gap = trace
            .rows()
            .iter()
            .find_map(|row| match row {
                Row::Gap(gap) => Some(gap.clone()),
                _ => None,
            })
            .expect("a gap row");
        assert_eq!((gap.rows, gap.reason), (lost, GapReason::EventsDropped));
        let input = std::fs::read_to_string(trace.input_recording().expect("input.jsonl"))
            .expect("it reads");
        assert!(
            input.contains(&format!("{{\"gap\":{{\"rows\":{lost}}}}}")),
            "{input}"
        );
        let native = trace.native_recordings().expect("the native layer reads");
        assert_eq!(native.len(), 1);
        assert_eq!(native[0].rows().count(), 1);
        assert_eq!(
            native[0].lost_rows(),
            lost,
            "the native stream is not read as whole"
        );
        drop(driver);
        agent
            .join(Duration::from_secs(5))
            .expect("the stand-in ends");
    }

    /// A stand-in that serves a recording and counts the starts and stops it was sent.
    /// A stand-in that serves a recording (number 1), refusing to start one when `refuse`, and
    /// counts the starts it was sent and the stops of recording 1.
    fn counting(refuse: bool) -> (ScriptedAgent, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let starts = Arc::new(AtomicUsize::new(0));
        let stops = Arc::new(AtomicUsize::new(0));
        let (started, stopped) = (Arc::clone(&starts), Arc::clone(&stops));
        let agent = ScriptedAgent::start_with(
            capabilities(),
            Box::new(move |method, params| match method {
                "windows.list" => Reply::Result(json!({ "windows": ["w1"] })),
                "window.placement" => Reply::Result(placement()),
                "record.start" => {
                    started.fetch_add(1, Ordering::SeqCst);
                    if refuse {
                        Reply::Error(ErrorObject::new(ErrorCode::Busy, "a recording is running"))
                    } else {
                        Reply::Result(recording())
                    }
                }
                "record.stop" => {
                    if params["recording"] == 1 {
                        stopped.fetch_add(1, Ordering::SeqCst);
                    }
                    Reply::Result(json!({ "rows": 0, "lost_rows": 0, "complete": true }))
                }
                _ => Reply::Close,
            }),
        )
        .expect("the stand-in starts");
        (agent, starts, stops)
    }

    #[test]
    fn a_trace_that_cannot_start_leaves_the_app_recording_nothing() {
        let root = tempfile::tempdir().expect("a temporary directory");

        // The trace's own file cannot be made: the app is never asked to record.
        let (agent, starts, stops) = counting(false);
        let mut driver = attach(&agent);
        let taken = root.path().join("taken.flui-trace");
        std::fs::create_dir_all(&taken).expect("a directory");
        std::fs::write(taken.join("trace.jsonl"), "another trace").expect("a trace there");
        driver
            .start_trace(TraceOptions::new(&taken))
            .expect_err("a trace is there");
        assert_eq!(starts.load(Ordering::SeqCst), 0);
        drop(driver);
        agent
            .join(Duration::from_secs(5))
            .expect("the stand-in ends");
        assert_eq!(stops.load(Ordering::SeqCst), 0);

        // The app refuses to record: the trace's files go, and there is nothing to stop.
        let (agent, starts, stops) = counting(true);
        let mut driver = attach(&agent);
        let refused = root.path().join("refused.flui-trace");
        let error = driver
            .start_trace(TraceOptions::new(&refused))
            .expect_err("the app refuses");
        assert!(matches!(error, Error::Busy { .. }), "{error:?}");
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        assert!(!refused.join("trace.jsonl").exists(), "no trace is left");
        drop(driver);
        agent
            .join(Duration::from_secs(5))
            .expect("the stand-in ends");
        assert_eq!(stops.load(Ordering::SeqCst), 0);

        // The recording's files cannot be made once the app records: it is stopped. A file of
        // that name is never written over.
        let (agent, starts, stops) = counting(false);
        let mut driver = attach(&agent);
        let kept = root.path().join("kept.flui-trace");
        std::fs::create_dir_all(&kept).expect("a directory");
        std::fs::write(kept.join("input.jsonl"), "someone's recording").expect("a file there");
        driver
            .start_trace(TraceOptions::new(&kept))
            .expect_err("the recording's file is there");
        assert_eq!(
            std::fs::read_to_string(kept.join("input.jsonl")).expect("still there"),
            "someone's recording"
        );
        let held = root.path().join("held.flui-trace");
        std::fs::create_dir_all(held.join("input.jsonl")).expect("something in the way");
        driver
            .start_trace(TraceOptions::new(&held))
            .expect_err("the recording's file is in the way");
        assert_eq!(starts.load(Ordering::SeqCst), 2);
        assert_eq!(
            stops.load(Ordering::SeqCst),
            2,
            "each recording was stopped"
        );
        assert!(
            !held.join("trace.jsonl").exists(),
            "the trace that never started is gone"
        );
        assert!(
            held.join("input.jsonl").is_dir(),
            "what was there is left alone"
        );
        // The session goes on, and a trace starts there once the way is clear.
        std::fs::remove_dir(held.join("input.jsonl")).expect("cleared");
        driver
            .start_trace(TraceOptions::new(&held))
            .expect("the trace starts");
        driver.finish_trace(&mut Report::new()).expect("finished");
        assert_eq!(stops.load(Ordering::SeqCst), 3);
        drop(driver);
        agent
            .join(Duration::from_secs(5))
            .expect("the stand-in ends");
    }

    #[test]
    fn a_replay_maps_handles_in_handle_fields_only() {
        let root = tempfile::tempdir().expect("a temporary directory");
        let dir = root.path().join("handles.flui-trace");
        std::fs::create_dir_all(&dir).expect("the directory");
        let header = r#"{"flui_trace":1,"protocol":"0.28","target":{"kind":"attached","os":"any"},"isolation":"shared","fidelity":{"positions":"exact"},"started_unix_ms":0,"privacy":"keep_text"}"#;
        let rows = [
            header.to_owned(),
            json!({ "command": { "seq": 1, "t_ms": 0, "method": "windows.list", "params": {} } })
                .to_string(),
            json!({ "result": { "seq": 1, "t_ms": 0, "ok": { "windows": ["w1"] } } }).to_string(),
            json!({ "command": { "seq": 2, "t_ms": 0, "method": "semantics.locate", "params": {
                "locator": { "window": "w1", "name": { "match": "exact", "text": "w1" } },
                "windows": ["w1"],
            } } })
            .to_string(),
            json!({ "result": { "seq": 2, "t_ms": 0, "ok": { "matches": [] } } }).to_string(),
            json!({ "command": { "seq": 3, "t_ms": 0, "method": "semantics.locate", "params": {
                "locator": { "window": "w1" },
            } } })
            .to_string(),
            json!({ "result": { "seq": 3, "t_ms": 0, "ok": { "matches": [], "note": "w1" } } })
                .to_string(),
            json!({ "end": {} }).to_string(),
        ];
        std::fs::write(dir.join("trace.jsonl"), rows.join("\n") + "\n").expect("the rows");

        let sent = Arc::new(Mutex::new(Value::Null));
        let seen = Arc::clone(&sent);
        let mut located = 0;
        let agent = ScriptedAgent::start(
            &["windows.list", "semantics.locate"],
            Box::new(move |method, params| match method {
                "windows.list" => Reply::Result(json!({ "windows": ["w7"] })),
                "semantics.locate" => {
                    located += 1;
                    if located == 1 {
                        *seen.lock().expect("the record") = params.clone();
                        Reply::Result(json!({ "matches": [] }))
                    } else {
                        // A text that differs, shaped like the handle the replay paired: text.
                        Reply::Result(json!({ "matches": [], "note": "w7" }))
                    }
                }
                _ => Reply::Close,
            }),
        )
        .expect("the stand-in starts");
        let mut driver = attach(&agent);
        let report = trace::replay(&Trace::open(&dir).expect("it reads"), &mut driver)
            .expect("the replay runs");
        let params = sent.lock().expect("the record").clone();
        assert_eq!(
            params["locator"]["window"], "w7",
            "the window handle is the replay's"
        );
        assert_eq!(params["windows"], json!(["w7"]), "a list of handles too");
        assert_eq!(
            params["locator"]["name"]["text"], "w1",
            "a name that looks like a handle is text"
        );
        let divergence = report.divergence.expect("a text that differs diverges");
        assert_eq!((divergence.seq, divergence.at.as_str()), (3, "/note"));
        drop(driver);
        agent
            .join(Duration::from_secs(5))
            .expect("the stand-in ends");
    }
}
