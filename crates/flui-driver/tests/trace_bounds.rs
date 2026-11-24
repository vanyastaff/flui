//! A trace's edges: events lost on the way, a start that fails half way, what a replay refuses
//! and maps, and the bounds a reader holds a trace and its native recordings to.

use std::path::Path;
use std::time::Duration;

use flui_driver::protocol::input::{ActionSequence, InputSource};
use flui_driver::protocol::native_recording::{MAX_LINE_BYTES, MAX_RECORDING_BYTES, MAX_ROWS};
use flui_driver::protocol::report::Report;
use flui_driver::trace::{self, GapReason, Row, Trace, TraceOptions, native};
use flui_driver::{Driver, Error, Launch, Locator};

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

/// A link at `link` to the directory `target`: a symbolic link on Unix, a junction on Windows,
/// which needs no privilege to make (std makes none, so `mklink /J` does).
fn link_dir(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).expect("a link");
    #[cfg(windows)]
    {
        let made = std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(link)
            .arg(target)
            .output()
            .expect("cmd runs");
        assert!(made.status.success(), "a junction: {made:?}");
    }
}

#[test]
fn a_native_stream_through_a_link_is_refused() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let index = r#"{"streams":[{"stream":0,"file":"0.jsonl"}]}"#;
    let outside = root.path().join("outside");
    std::fs::create_dir_all(&outside).expect("a directory outside the trace");
    std::fs::write(outside.join("index.json"), index).expect("an index");
    std::fs::write(outside.join("0.jsonl"), ONE_ROW).expect("a stream");
    let dir = native_trace(root.path(), "linked_dir", index, &[]);
    std::fs::remove_dir_all(dir.join("native")).expect("the native directory");
    link_dir(&outside, &dir.join("native"));
    let linked = refusal(
        Trace::open(&dir)
            .expect("the trace reads")
            .native_recordings(),
    );
    assert!(linked.contains("symbolic link or a junction"), "{linked}");

    // A file link needs a privilege on Windows; on Unix any user makes one.
    #[cfg(unix)]
    {
        let elsewhere = root.path().join("elsewhere.jsonl");
        std::fs::write(&elsewhere, ONE_ROW).expect("a file outside the trace");
        let dir = native_trace(root.path(), "linked_file", index, &[]);
        std::os::unix::fs::symlink(&elsewhere, dir.join("native").join("0.jsonl")).expect("a link");
        let linked = refusal(
            Trace::open(&dir)
                .expect("the trace reads")
                .native_recordings(),
        );
        assert!(linked.contains("symbolic link"), "{linked}");
    }
}

#[test]
fn evidence_through_a_link_is_refused() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let name = format!("evidence/{}.png", "a".repeat(64));
    let outside = root.path().join("outside");
    std::fs::create_dir_all(&outside).expect("a directory outside the trace");
    std::fs::write(outside.join(format!("{}.png", "a".repeat(64))), b"png").expect("a file");
    let dir = native_trace(root.path(), "evidence", r#"{"streams":[]}"#, &[]);
    link_dir(&outside, &dir.join("evidence"));
    let trace = Trace::open(&dir).expect("the trace reads");
    let refused = trace
        .evidence(&name)
        .expect_err("a linked evidence directory");
    let Error::Unreadable { message, .. } = &refused else {
        panic!("{refused:?}");
    };
    assert!(message.contains("junction"), "{message}");

    let dir = native_trace(root.path(), "evidence_kept", r#"{"streams":[]}"#, &[]);
    std::fs::create_dir_all(dir.join("evidence")).expect("the evidence directory");
    std::fs::write(
        dir.join("evidence").join(format!("{}.png", "a".repeat(64))),
        b"png",
    )
    .expect("a file");
    let trace = Trace::open(&dir).expect("the trace reads");
    assert!(trace.evidence(&name).expect("its own file").is_file());
}

#[test]
fn native_streams_past_their_total_budget_are_refused_before_any_is_read() {
    let root = tempfile::tempdir().expect("a temporary directory");
    // Each file under the format's own bound, together past the trace's: their lengths are read
    // from the file system, so they hold nothing a reader would take.
    let each = flui_driver::protocol::native_recording::MAX_RECORDING_BYTES as u64 - 1;
    let count = trace::MAX_NATIVE_TOTAL_BYTES / each + 1;
    let streams: Vec<String> = (0..count)
        .map(|stream| format!("{{\"stream\":{stream},\"file\":\"{stream}.jsonl\"}}"))
        .collect();
    let dir = native_trace(
        root.path(),
        "heavy",
        &format!("{{\"streams\":[{}]}}", streams.join(",")),
        &[],
    );
    for stream in 0..count {
        let file = std::fs::File::create(dir.join("native").join(format!("{stream}.jsonl")))
            .expect("a stream");
        file.set_len(each).expect("its length");
    }
    let refused = refusal(
        Trace::open(&dir)
            .expect("the trace reads")
            .native_recordings(),
    );
    assert!(
        refused.contains("bytes a trace's native layer may"),
        "{refused}"
    );

    // Streams adding up to the budget exactly pass it, and are then read (and refused for what
    // they hold, which is no recording).
    let streams: Vec<String> = (0..4)
        .map(|stream| format!("{{\"stream\":{stream},\"file\":\"{stream}.jsonl\"}}"))
        .collect();
    let dir = native_trace(
        root.path(),
        "at_budget",
        &format!("{{\"streams\":[{}]}}", streams.join(",")),
        &[],
    );
    for stream in 0..4 {
        let file = std::fs::File::create(dir.join("native").join(format!("{stream}.jsonl")))
            .expect("a stream");
        file.set_len(trace::MAX_NATIVE_TOTAL_BYTES / 4)
            .expect("its length");
    }
    let read = refusal(
        Trace::open(&dir)
            .expect("the trace reads")
            .native_recordings(),
    );
    assert!(!read.contains("native layer may"), "{read}");
}

#[test]
fn a_command_numbered_twice_is_refused() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let dir = root.path().join("twice.flui-trace");
    std::fs::create_dir_all(&dir).expect("the directory");
    let command = r#"{"command":{"seq":1,"t_ms":0,"method":"windows.list","params":{}}}"#;
    std::fs::write(
        dir.join("trace.jsonl"),
        format!("{NATIVE_HEADER}\n{command}\n{command}\n"),
    )
    .expect("the rows");
    let refused = Trace::open(&dir).expect_err("two commands numbered 1");
    let Error::Unreadable { message, line, .. } = &refused else {
        panic!("{refused:?}");
    };
    assert!(message.contains("second command numbered 1"), "{message}");
    assert_eq!(*line, Some(3));
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

    // One redacted text among sources that hold none is enough; the refusal comes before
    // anything is sent, so the trace need not have come from this app.
    let dir = root.path().join("written.flui-trace");
    std::fs::create_dir_all(&dir).expect("the directory");
    let header = r#"{"flui_trace":1,"protocol":"0.28","target":{"kind":"headless","os":"any"},"isolation":"in_process","fidelity":{"positions":"exact"},"started_unix_ms":0,"privacy":"redact"}"#;
    let perform = serde_json::json!({ "command": { "seq": 1, "t_ms": 0, "method": "input.perform", "params": {
        "actions": { "sources": [
            { "type": "pointer", "id": "p", "device": "mouse", "actions": [{ "type": "pause", "duration_ms": 1 }] },
            { "type": "text", "text": flui_driver::protocol::input_recording::REDACTED },
        ] },
    } } });
    std::fs::write(dir.join("trace.jsonl"), format!("{header}\n{perform}\n")).expect("the rows");
    let report = trace::replay(&Trace::open(&dir).expect("it reads"), &mut counter())
        .expect("the replay runs");
    assert_eq!(report.replayed, 0);
    assert_eq!(report.refused.map(|refused| refused.seq), Some(1));
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
        // More events than the connection keeps, arriving before an answer: the oldest are
        // dropped, and which they were is not known.
        let overflow = move || -> Vec<Value> {
            (0..1024 + lost)
                .map(|_| json!({ "event": "gap", "params": { "missed": 0 } }))
                .collect()
        };
        let agent = ScriptedAgent::start_with(
            capabilities(),
            Box::new(move |method, _| match method {
                "windows.list" => {
                    listed += 1;
                    // The first is the trace's own read of the main window, the second takes
                    // the stream's first lines; then a loss, the app ending its recording
                    // (its window closed), and a loss after that end.
                    let lines = match listed {
                        3 | 5 => overflow(),
                        4 => vec![json!({
                            "event": "record",
                            "params": { "recording": 1, "window": "w1", "row": { "end": { "lost_rows": 0 } } },
                        })],
                        _ => Vec::new(),
                    };
                    Reply::LinesThenResult {
                        lines,
                        result: json!({ "windows": ["w1"] }),
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
        driver.windows().expect("the recording's end");
        driver.windows().expect("a loss after the end");
        driver.finish_trace(&mut Report::new()).expect("finished");

        let trace = Trace::open(&dir).expect("it reads");
        let gaps: Vec<_> = trace
            .rows()
            .iter()
            .filter_map(|row| match row {
                Row::Gap(gap) => Some((gap.rows, gap.reason)),
                _ => None,
            })
            .collect();
        assert_eq!(gaps, [(lost, GapReason::EventsDropped); 2]);
        let input = std::fs::read_to_string(trace.input_recording().expect("input.jsonl"))
            .expect("it reads");
        let lines: Vec<&str> = input.lines().collect();
        assert_eq!(
            lines[1..],
            [
                format!("{{\"gap\":{{\"rows\":{lost}}}}}").as_str(),
                "{\"end\":{\"lost_rows\":0}}"
            ],
            "the loss before the end is marked, and nothing follows the end: {input}"
        );
        let native = trace.native_recordings().expect("the native layer reads");
        assert_eq!(native.len(), 1);
        assert_eq!(native[0].rows().count(), 1);
        assert_eq!(
            native[0].lost_rows(),
            lost,
            "the native stream is marked at the loss before the recording ended, and not after: \
             no line of an ended recording is left to lose"
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
        let error = driver
            .start_trace(TraceOptions::new(&taken))
            .expect_err("a trace is there");
        assert!(matches!(error, Error::InvalidArgument { .. }), "{error:?}");
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
        assert!(
            !refused.exists(),
            "the directory the trace made goes with it"
        );
        // A directory the caller made, empty, stays.
        let given = root.path().join("given.flui-trace");
        std::fs::create_dir_all(&given).expect("the caller's directory");
        driver
            .start_trace(TraceOptions::new(&given))
            .expect_err("the app refuses");
        assert!(!given.join("trace.jsonl").exists(), "no trace is left");
        assert!(given.is_dir(), "the caller's directory stays");
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
        let error = driver
            .start_trace(TraceOptions::new(&kept))
            .expect_err("the recording's file is there");
        assert!(matches!(error, Error::InvalidArgument { .. }), "{error:?}");
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
    fn a_replay_maps_handles_where_the_schema_has_them_only() {
        let root = tempfile::tempdir().expect("a temporary directory");
        let dir = root.path().join("handles.flui-trace");
        std::fs::create_dir_all(&dir).expect("the directory");
        let header = r#"{"flui_trace":1,"protocol":"0.28","target":{"kind":"attached","os":"any"},"isolation":"shared","fidelity":{"positions":"exact"},"started_unix_ms":0,"privacy":"keep_text"}"#;
        let found = |element: &str, id: &str| {
            json!({ "matches": [{ "element": element, "revision": 1, "node": {
                "id": id, "role": "button", "native_role": "Button",
            } }] })
        };
        let inject = json!({ "window": "w1", "actions": { "sources": [{
            "type": "pointer", "id": "e1", "device": "mouse",
            "actions": [{ "type": "pause", "duration_ms": 1 }],
        }] } });
        let rows = [
            header.to_owned(),
            json!({ "command": { "seq": 1, "t_ms": 0, "method": "windows.list", "params": {} } })
                .to_string(),
            json!({ "result": { "seq": 1, "t_ms": 0, "ok": { "windows": ["w1"] } } }).to_string(),
            json!({ "command": { "seq": 2, "t_ms": 0, "method": "semantics.locate", "params": {
                "locator": { "window": "w1", "name": { "match": "exact", "text": "w1" } },
            } } })
            .to_string(),
            json!({ "result": { "seq": 2, "t_ms": 0, "ok": found("w1/e1", "e1") } }).to_string(),
            json!({ "command": { "seq": 3, "t_ms": 0, "method": "input.inject", "params": inject } })
                .to_string(),
            json!({ "result": { "seq": 3, "t_ms": 0, "ok": { "done": true } } }).to_string(),
            // An element no result named, in a window one did: its window is the replay's.
            json!({ "command": { "seq": 4, "t_ms": 0, "method": "semantics.act", "params": {
                "target": { "element": "w1/e3" }, "action": "focus",
            } } })
            .to_string(),
            json!({ "result": { "seq": 4, "t_ms": 0, "ok": { "done": true } } }).to_string(),
            json!({ "command": { "seq": 5, "t_ms": 0, "method": "semantics.locate", "params": {
                "locator": { "window": "w1" },
            } } })
            .to_string(),
            json!({ "result": { "seq": 5, "t_ms": 0, "ok": { "matches": [], "note": "w1" } } })
                .to_string(),
            json!({ "end": {} }).to_string(),
        ];
        std::fs::write(dir.join("trace.jsonl"), rows.join("\n") + "\n").expect("the rows");

        let sent = Arc::new(Mutex::new(Vec::<Value>::new()));
        let seen = Arc::clone(&sent);
        let mut located = 0;
        let agent = ScriptedAgent::start(
            &[
                "windows.list",
                "semantics.locate",
                "input.inject",
                "semantics.act",
            ],
            Box::new(move |method, params| {
                seen.lock().expect("the record").push(params.clone());
                match method {
                    "windows.list" => Reply::Result(json!({ "windows": ["w7"] })),
                    "semantics.locate" => {
                        located += 1;
                        if located == 1 {
                            Reply::Result(found("w7/e9", "e9"))
                        } else {
                            // A text that differs, shaped like a handle the replay paired: text.
                            Reply::Result(json!({ "matches": [], "note": "w7" }))
                        }
                    }
                    "input.inject" | "semantics.act" => Reply::Result(json!({ "done": true })),
                    _ => Reply::Close,
                }
            }),
        )
        .expect("the stand-in starts");
        let mut driver = attach(&agent);
        let report = trace::replay(&Trace::open(&dir).expect("it reads"), &mut driver)
            .expect("the replay runs");
        let sent = sent.lock().expect("the record").clone();
        let locate = &sent[1];
        assert_eq!(
            locate["locator"]["window"], "w7",
            "the window handle is the replay's"
        );
        assert_eq!(
            locate["locator"]["name"]["text"], "w1",
            "a name that looks like a handle is text"
        );
        let injected = &sent[2];
        assert_eq!(injected["window"], "w7");
        assert_eq!(
            injected["actions"]["sources"][0]["id"], "e1",
            "an input source's id is the caller's name for it, though `e1` is paired with `e9`"
        );
        assert_eq!(sent[3]["target"]["element"], "w7/e3");
        let divergence = report.divergence.expect("a text that differs diverges");
        assert_eq!((divergence.seq, divergence.at.as_str()), (5, "/note"));
        drop(driver);
        agent
            .join(Duration::from_secs(5))
            .expect("the stand-in ends");
    }
}
