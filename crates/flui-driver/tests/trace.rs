//! Traces: the writer, the reader, replay and codegen, on the headless target and over a
//! scripted agent's socket.

use std::path::{Path, PathBuf};
use std::time::Duration;

use flui_driver::pixels::{CaptureSource, Image};
use flui_driver::protocol::input::{ActionSequence, InputSource};
use flui_driver::protocol::input_recording::REDACTED;
use flui_driver::testing::ScriptedLane;
use flui_driver::trace::{
    self, CodegenOptions, GapReason, Positions, Privacy, Row, Trace, TraceMode, TraceOptions,
};
use flui_driver::{Driver, Error, Isolation, Launch, Locator, Report, TextMatch, Verdict, expect};
use flui_view::prelude::*;
use flui_view::{Signal, StatefulView, ViewState};
use flui_widgets::{Column, RawButton, Text};

use crate::fixtures::{Counter, Probe};

/// A counter that counts by `step`: with step 2 it is the counter changed after a trace of it.
#[derive(Clone, StatefulView)]
struct Stepper {
    step: u32,
}

struct StepperState {
    count: Signal<u32>,
}

impl StatefulView for Stepper {
    type State = StepperState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        StepperState {
            count: ctx.signal(0),
        }
    }
}

impl ViewState<Stepper> for StepperState {
    fn build(&self, view: &Stepper, ctx: &dyn BuildContext) -> impl IntoView {
        let count = self.count;
        let step = view.step;
        Column::new((
            Text::new(format!("Count: {}", count.get(ctx))),
            RawButton::new(Text::new("Increment"))
                .identifier("increment")
                .on_pressed(move |ctx| count.update(ctx, |n| *n += step)),
        ))
    }
}

fn stepper(step: u32) -> Driver {
    Driver::launch(Launch::headless(move || Stepper { step }).name("stepper"))
        .expect("the headless app starts")
}

fn counter() -> Driver {
    Driver::launch(
        Launch::headless(|| Counter {
            probe: Probe::default(),
        })
        .name("counter"),
    )
    .expect("the headless app starts")
}

fn count() -> Locator {
    Locator::default().name_matching(TextMatch::prefix("Count:"))
}

/// Tap the increment button once and expect the count to read 1.
fn tap_once(driver: &mut Driver) -> flui_driver::Result<()> {
    driver
        .main_window()?
        .locate(Locator::named("Increment"))
        .tap()?;
    expect(driver.main_window()?.locate(count())).to_have_name("Count: 1")
}

/// Record `run` against `driver` into `dir`, finished with a passing report; the trace.
fn record(
    driver: &mut Driver,
    dir: &Path,
    options: TraceOptions,
    run: impl FnOnce(&mut Driver) -> flui_driver::Result<()>,
) -> Trace {
    driver.start_trace(options).expect("the trace starts");
    let mut report = Report::new();
    report
        .check("the run", || run(driver))
        .expect("the run passes");
    let summary = driver
        .finish_trace(&mut report)
        .expect("the trace finishes");
    assert_eq!(summary.dir.as_deref(), Some(dir));
    Trace::open(dir).expect("the trace reads back")
}

fn trace_dir(root: &tempfile::TempDir, name: &str) -> PathBuf {
    root.path().join(format!("{name}.flui-trace"))
}

/// `trace.jsonl` with what varies between hosts and runs set aside: the start time, the OS, and
/// the handles the app minted, numbered by first appearance as a replay pairs them.
fn normalized_rows(dir: &Path) -> Vec<serde_json::Value> {
    let text = std::fs::read_to_string(dir.join("trace.jsonl")).expect("the rows");
    let mut handles = std::collections::BTreeMap::new();
    text.lines()
        .map(|line| {
            let mut value: serde_json::Value = serde_json::from_str(line).expect("a JSON line");
            if value.get("flui_trace").is_some() {
                value["started_unix_ms"] = 0.into();
                value["target"]["os"] = "any".into();
            }
            renumber(&mut value, &mut handles);
            value
        })
        .collect()
}

/// Each handle in `value` (`w3`, `e12`) as the `n`th of its kind to appear.
fn renumber(
    value: &mut serde_json::Value,
    handles: &mut std::collections::BTreeMap<String, String>,
) {
    let mut one = |part: &str| -> Option<String> {
        let kind = part
            .chars()
            .next()
            .filter(|kind| matches!(kind, 'w' | 'e'))?;
        let digits = &part[1..];
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let count = handles.keys().filter(|seen| seen.starts_with(kind)).count();
        Some(
            handles
                .entry(part.to_owned())
                .or_insert_with(|| format!("{kind}{}", count + 1))
                .clone(),
        )
    };
    match value {
        serde_json::Value::String(text) => {
            let parts: Option<Vec<String>> = text.split('/').map(&mut one).collect();
            if let Some(parts) = parts {
                *text = parts.join("/");
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                renumber(item, handles);
            }
        }
        serde_json::Value::Object(fields) => {
            for item in fields.values_mut() {
                renumber(item, handles);
            }
        }
        _ => {}
    }
}

#[test]
fn a_headless_trace_matches_its_golden_rows() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let dir = trace_dir(&root, "counter");
    let mut driver = counter();
    let trace = record(&mut driver, &dir, TraceOptions::new(&dir), tap_once);
    assert!(trace.is_complete());
    let header = trace.header();
    assert_eq!(header.flui_trace, trace::FORMAT_VERSION);
    assert_eq!(header.target.kind, "headless");
    assert_eq!(header.isolation, Isolation::InProcess);
    assert_eq!(header.fidelity.positions, Positions::Exact);
    assert_eq!(header.privacy, Privacy::Redact);
    let window = header.window.expect("the main window's size");
    assert!(window.width > 0.0 && window.scale > 0.0, "{window:?}");

    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/counter.trace.jsonl");
    let rows = normalized_rows(&dir);
    let written = rows.iter().fold(String::new(), |mut text, row| {
        text.push_str(&row.to_string());
        text.push('\n');
        text
    });
    if std::env::var_os("FLUI_OVERWRITE").is_some() {
        std::fs::create_dir_all(golden.parent().expect("a folder")).expect("the folder");
        std::fs::write(&golden, &written).expect("the golden file");
    }
    let expected =
        std::fs::read_to_string(&golden).expect("the golden file; FLUI_OVERWRITE=1 writes it");
    assert_eq!(written, expected, "the rows of {}", dir.display());
}

#[test]
fn a_trace_records_commands_as_written_with_their_results() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let dir = trace_dir(&root, "steps");
    let mut driver = counter();
    let trace = record(&mut driver, &dir, TraceOptions::new(&dir), tap_once);
    assert_eq!(
        trace
            .native_recordings()
            .expect("a trace without the native layer has none")
            .len(),
        0
    );
    let steps = trace.steps();
    let methods: Vec<&str> = steps
        .iter()
        .map(|step| step.command.method.as_str())
        .collect();
    assert_eq!(
        methods,
        ["windows.list", "element.tap", "windows.list", "wait.until"]
    );
    let tap = steps[1];
    assert_eq!(
        tap.command.params["target"]["locator"]["name"]["text"], "Increment",
        "the locator as written, never only the handle it resolved to"
    );
    let result = tap.result.expect("the tap answered");
    assert_eq!(
        result.ok.as_ref().expect("it succeeded")["via"],
        "synthetic"
    );
    assert!(steps.iter().all(|step| step.result.is_some()));
    let seqs: Vec<u64> = steps.iter().map(|step| step.command.seq).collect();
    assert_eq!(seqs, [1, 2, 3, 4]);
}

#[test]
fn a_trace_cut_short_reads_as_incomplete() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let dir = trace_dir(&root, "cut");
    {
        let mut driver = counter();
        driver
            .start_trace(TraceOptions::new(&dir))
            .expect("the trace starts");
        driver
            .main_window()
            .expect("a window")
            .locate(Locator::named("Increment"))
            .tap()
            .expect("the tap");
        // The session ends without finishing the trace, as a crash would end it.
    }
    let trace = Trace::open(&dir).expect("an incomplete trace still reads");
    assert!(!trace.is_complete());
    assert!(!trace.rows().iter().any(|row| matches!(row, Row::End(_))));

    // A writer stopped in the middle of a line: the rows before it still read, incomplete.
    let rows = dir.join("trace.jsonl");
    let mut text = std::fs::read_to_string(&rows).expect("the rows");
    text.push_str(r#"{"command":{"seq":9,"t_ms":4"#);
    std::fs::write(&rows, text).expect("the rows");
    let trace = Trace::open(&dir).expect("a torn last line ends the trace");
    assert!(!trace.is_complete());
    assert_eq!(trace.steps().len(), 2);
}

#[test]
fn a_reader_refuses_a_newer_format_an_unknown_row_and_a_row_after_the_end() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let dir = trace_dir(&root, "valid");
    let mut driver = counter();
    record(&mut driver, &dir, TraceOptions::new(&dir), |_| Ok(()));
    let rows = std::fs::read_to_string(dir.join("trace.jsonl")).expect("the rows");
    let (header, rest) = rows.split_once('\n').expect("a header line");

    let cases = [
        (
            "newer",
            format!(
                "{}\n{rest}",
                header.replace("\"flui_trace\":1", "\"flui_trace\":2")
            ),
            "format 2",
        ),
        (
            "unknown_row",
            format!("{header}\n{{\"bookmark\":{{\"t_ms\":0}}}}\n{rest}"),
            "not a row",
        ),
        (
            "after_end",
            format!("{rows}{{\"end\":{{}}}}\n"),
            "after the trace's `end`",
        ),
        (
            "orphan_result",
            format!("{header}\n{{\"result\":{{\"seq\":7,\"t_ms\":0,\"ok\":null}}}}\n{rest}"),
            "not open",
        ),
    ];
    for (name, text, says) in cases {
        let case = trace_dir(&root, name);
        std::fs::create_dir_all(&case).expect("the directory");
        std::fs::write(case.join("trace.jsonl"), text).expect("the rows");
        let refused = Trace::open(&case).expect_err(name);
        let Error::Unreadable { message, .. } = &refused else {
            panic!("{name}: {refused:?}");
        };
        assert!(message.contains(says), "{name}: {message}");
    }
    // Unknown fields are a newer writer's additions, and are passed over.
    let case = trace_dir(&root, "additions");
    std::fs::create_dir_all(&case).expect("the directory");
    std::fs::write(
        case.join("trace.jsonl"),
        format!(
            "{}\n{rest}",
            header.replacen('{', "{\"comment\":\"new\",", 1)
        ),
    )
    .expect("the rows");
    assert!(Trace::open(&case).expect("additions read").is_complete());
    // A packed trace is a file, which the reader names.
    let packed = root.path().join("packed.zip");
    std::fs::write(&packed, b"PK").expect("the file");
    let refused = Trace::open(&packed).expect_err("a file");
    assert_eq!(
        refused.missing_capability().as_deref(),
        Some("trace_archive")
    );
}

/// A screen of one colour, the window's size.
fn flat_screen(
    placement: &flui_driver::protocol::session::Placement,
) -> flui_driver::Result<Image> {
    let side =
        |logical: f64| u32::try_from((logical * placement.scale).round() as i64).unwrap_or(1);
    Image::filled(
        side(placement.size.width),
        side(placement.size.height),
        [40, 80, 120, 255],
    )
}

#[test]
fn evidence_is_kept_once_and_past_the_budget_becomes_a_gap() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let dir = trace_dir(&root, "evidence");
    let (lane, _) = ScriptedLane::new(Isolation::InProcess);
    let mut driver = counter()
        .with_lane(lane.screen(flat_screen))
        .expect("the lane");
    let trace = record(&mut driver, &dir, TraceOptions::new(&dir), |driver| {
        let mut window = driver.main_window()?;
        window.capture(CaptureSource::Os)?;
        window.capture(CaptureSource::Os)?;
        Ok(())
    });
    let captures: Vec<_> = trace
        .steps()
        .into_iter()
        .filter(|step| step.command.method == "capture.os")
        .collect();
    assert_eq!(captures.len(), 2);
    let names: Vec<&String> = captures
        .iter()
        .map(|step| &step.result.expect("an answer").evidence[0])
        .collect();
    assert_eq!(names[0], names[1], "the same capture is one file");
    let file = trace.evidence(names[0]).expect("an evidence name");
    let kept = flui_driver::pixels::read_png(&file).expect("the evidence reads back");
    assert_eq!(kept.pixel(0, 0), Some([40, 80, 120, 255]));
    let json_name = names[0].replace(".png", ".json");
    assert!(trace.evidence(&json_name).is_err(), "evidence is PNG only");
    // A replay sends the captures nowhere: their rows are not in the method table yet, so it
    // lists them, and a replay that skipped a command did not match.
    let replayed = trace::replay(&trace, &mut counter()).expect("the replay runs");
    assert_eq!(replayed.divergence, None, "{replayed:?}");
    assert_eq!(replayed.skipped.len(), 2);
    assert!(!replayed.matched());
    assert!(
        trace.evidence("evidence/../trace.jsonl").is_err(),
        "no path out of the trace"
    );

    let dir = trace_dir(&root, "budget");
    let (lane, _) = ScriptedLane::new(Isolation::InProcess);
    let mut driver = counter()
        .with_lane(lane.screen(flat_screen))
        .expect("the lane");
    let trace = record(
        &mut driver,
        &dir,
        TraceOptions::new(&dir).evidence_budget(16),
        |driver| driver.main_window()?.capture(CaptureSource::Os).map(drop),
    );
    let gap = trace
        .rows()
        .iter()
        .find_map(|row| match row {
            Row::Gap(gap) => Some(gap.clone()),
            _ => None,
        })
        .expect("a gap row");
    assert_eq!((gap.rows, gap.reason), (1, GapReason::EvidenceBudget));
    let capture = trace
        .steps()
        .into_iter()
        .find(|step| step.command.method == "capture.os")
        .expect("the capture");
    assert_eq!(
        capture.result.expect("an answer").evidence,
        Vec::<String>::new()
    );
    assert!(!dir.join("evidence").exists());
}

#[test]
fn text_is_redacted_unless_the_session_keeps_it() {
    let secret = "correct-horse-battery-staple";
    let typed = |driver: &mut Driver| {
        // The counter has no field: the button is focused by a tap, which counts, and the text
        // reaches nothing; the command is traced all the same.
        driver
            .main_window()?
            .locate(Locator::named("Increment"))
            .type_text(secret)?;
        driver.main_window()?.read().map(drop)
    };
    let root = tempfile::tempdir().expect("a temporary directory");
    let dir = trace_dir(&root, "redacted");
    let trace = record(&mut counter(), &dir, TraceOptions::new(&dir), typed);
    let rows = std::fs::read_to_string(dir.join("trace.jsonl")).expect("the rows");
    assert!(!rows.contains(secret), "typed text is redacted");
    assert!(!rows.contains("Count: "), "the app's text is redacted");
    assert!(
        rows.contains("Increment"),
        "the caller's locator is kept, so a replay resolves it"
    );
    let read = trace
        .steps()
        .into_iter()
        .find(|step| step.command.method == "semantics.read")
        .and_then(|step| step.result)
        .and_then(|result| result.ok.clone())
        .expect("the read's tree");
    assert!(read.to_string().contains(REDACTED), "{read}");
    let typing = trace
        .steps()
        .into_iter()
        .find(|step| step.command.method == "element.type_text")
        .expect("the typing");
    assert_eq!(typing.command.params["text"], REDACTED);

    let dir = trace_dir(&root, "kept");
    let trace = record(
        &mut counter(),
        &dir,
        TraceOptions::new(&dir).keep_text(true),
        typed,
    );
    assert_eq!(trace.header().privacy, Privacy::KeepText);
    let rows = std::fs::read_to_string(dir.join("trace.jsonl")).expect("the rows");
    assert!(rows.contains(secret), "typed text is kept: {rows}");
    assert!(rows.contains("Count: "), "the app's text is kept: {rows}");
}

#[test]
fn a_trace_kept_on_failure_goes_with_a_passing_run_and_stays_with_a_failing_one() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let dir = trace_dir(&root, "passing");
    let mut driver = counter();
    driver
        .start_trace(TraceOptions::new(&dir).mode(TraceMode::RetainOnFailure))
        .expect("the trace starts");
    let mut report = Report::new();
    report
        .check("the run", || tap_once(&mut driver))
        .expect("it passes");
    let summary = driver
        .finish_trace(&mut report)
        .expect("the trace finishes");
    assert_eq!(summary.dir, None);
    assert!(!dir.exists(), "a passing run leaves no trace");
    assert!(report.to_json()["trace"].is_null());

    let dir = trace_dir(&root, "failing");
    let mut driver = counter();
    driver
        .start_trace(TraceOptions::new(&dir).mode(TraceMode::RetainOnFailure))
        .expect("the trace starts");
    let mut report = Report::new();
    let _ = report.check("the run", || {
        expect(driver.main_window()?.locate(count()))
            .timeout(Duration::from_millis(100))
            .to_have_name("Count: 9")
    });
    assert_eq!(report.verdict(), Verdict::Fail);
    let summary = driver
        .finish_trace(&mut report)
        .expect("the trace finishes");
    assert_eq!(summary.dir.as_deref(), Some(dir.as_path()));
    assert_eq!(report.to_json()["trace"], dir.display().to_string());
    assert!(Trace::open(&dir).expect("kept").is_complete());

    // A run that could not be verified keeps its trace as a failing one does.
    let dir = trace_dir(&root, "unverified");
    let mut driver = counter();
    driver
        .start_trace(TraceOptions::new(&dir).mode(TraceMode::RetainOnFailure))
        .expect("the trace starts");
    let mut report = Report::new();
    report.not_run("a check the host cannot make", "no lane");
    assert_eq!(report.verdict(), Verdict::CannotVerify);
    let summary = driver
        .finish_trace(&mut report)
        .expect("the trace finishes");
    assert_eq!(summary.dir.as_deref(), Some(dir.as_path()));

    let refused = driver
        .finish_trace(&mut Report::new())
        .expect_err("no trace is being recorded");
    assert!(
        matches!(refused, Error::InvalidArgument { .. }),
        "{refused:?}"
    );
    let mut again = counter();
    again
        .start_trace(TraceOptions::new(&dir))
        .expect_err("a trace never writes over another");
}

#[test]
fn a_headless_replay_has_no_divergence_and_a_changed_app_diverges_at_its_wait() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let dir = trace_dir(&root, "stepper");
    let trace = record(&mut stepper(1), &dir, TraceOptions::new(&dir), tap_once);

    let same = trace::replay(&trace, &mut stepper(1)).expect("the replay runs");
    assert_eq!(same.divergence, None, "{same:?}");
    assert!(same.matched(), "{same:?}");
    assert_eq!(same.replayed, 4);

    let changed = trace::replay(&trace, &mut stepper(2)).expect("the replay runs");
    let divergence = changed.divergence.expect("the changed app diverges");
    assert_eq!(
        (divergence.seq, divergence.method.as_str()),
        (4, "wait.until")
    );
    assert_eq!(divergence.at, "/satisfied");
    assert_eq!(
        (divergence.expected, divergence.actual),
        (serde_json::json!(true), serde_json::json!(false))
    );
}

/// The trace in `dir` with `edit` applied to each line's JSON.
fn rewrite(dir: &Path, mut edit: impl FnMut(&mut serde_json::Value)) {
    let path = dir.join("trace.jsonl");
    let text = std::fs::read_to_string(&path).expect("the rows");
    let edited = text.lines().fold(String::new(), |mut out, line| {
        let mut value: serde_json::Value = serde_json::from_str(line).expect("a JSON line");
        edit(&mut value);
        out.push_str(&value.to_string());
        out.push('\n');
        out
    });
    std::fs::write(&path, edited).expect("the rows");
}

#[test]
fn a_replay_leaves_out_frames_and_compares_positions_only_at_exact_fidelity() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let dir = trace_dir(&root, "fidelity");
    record(&mut stepper(1), &dir, TraceOptions::new(&dir), tap_once);
    // The wait's answer recorded other frame counts and its element somewhere else.
    rewrite(&dir, |row| {
        if let Some(ok) = row.pointer_mut("/result/ok")
            && ok.get("satisfied").is_some()
        {
            ok["frames"] = 999.into();
            let x = ok["observation"]["matched"][0]["surface_rect"]["x"]
                .as_i64()
                .expect("the matched element's bounds");
            ok["observation"]["matched"][0]["surface_rect"]["x"] = (x + 7).into();
        }
    });
    let exact = trace::replay(&Trace::open(&dir).expect("it reads"), &mut stepper(1))
        .expect("the replay runs");
    let divergence = exact.divergence.expect("an exact lane compares positions");
    assert_eq!(
        divergence.at, "/observation/matched/0/surface_rect/x",
        "{divergence:?}"
    );

    rewrite(&dir, |row| {
        if row.get("flui_trace").is_some() {
            row["fidelity"]["positions"] = "order".into();
        }
    });
    let order = trace::replay(&Trace::open(&dir).expect("it reads"), &mut stepper(1))
        .expect("the replay runs");
    assert_eq!(
        order.divergence, None,
        "frames never compare, and positions not at order fidelity"
    );
}

#[test]
fn codegen_marks_what_it_cannot_write_and_writes_the_main_window_as_the_tests_own() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let write = |name: &str, rows: &[serde_json::Value]| {
        let dir = trace_dir(&root, name);
        std::fs::create_dir_all(&dir).expect("the directory");
        let header = r#"{"flui_trace":1,"protocol":"0.28","target":{"kind":"headless","os":"any","app":"counter"},"isolation":"in_process","fidelity":{"positions":"exact"},"started_unix_ms":0,"privacy":"keep_text"}"#;
        let mut text = format!("{header}\n");
        for row in rows {
            text.push_str(&row.to_string());
            text.push('\n');
        }
        std::fs::write(dir.join("trace.jsonl"), text).expect("the rows");
        let trace = Trace::open(&dir).expect("it reads");
        trace::codegen(
            &trace,
            &CodegenOptions::new("generated", "Launch::headless(app)"),
        )
        .expect("a test")
    };
    let command = |seq: u64, method: &str, params: serde_json::Value| serde_json::json!({ "command": { "seq": seq, "t_ms": 0, "method": method, "params": params } });
    let ok = |seq: u64, value: serde_json::Value| serde_json::json!({ "result": { "seq": seq, "t_ms": 0, "ok": value } });

    let plain = write(
        "plain",
        &[
            command(1, "windows.list", serde_json::json!({})),
            ok(1, serde_json::json!({ "windows": ["w5"] })),
            command(
                2,
                "element.tap",
                serde_json::json!({ "target": { "locator": { "identifier": "increment", "window": "w5" } } }),
            ),
            ok(
                2,
                serde_json::json!({ "via": "synthetic", "effect": { "kind": "ran" } }),
            ),
        ],
    );
    assert!(
        plain.contains("driver.main_window()?.locate(Locator::identifier(\"increment\")).tap()?;"),
        "{plain}"
    );
    assert!(
        !plain.contains("windows.list"),
        "a read is left out: {plain}"
    );
    assert!(
        !plain.contains("serde_json"),
        "nothing sent as recorded: {plain}"
    );
    assert!(
        !plain.contains("Not generated"),
        "nothing left out: {plain}"
    );
    assert!(!plain.contains("let window"), "no window to name: {plain}");
    assert!(
        plain.contains("use flui_driver::{Driver, Launch, Locator};\n"),
        "only what the test uses is imported: {plain}"
    );

    let marked = write(
        "marked",
        &[
            command(1, "windows.list", serde_json::json!({})),
            ok(1, serde_json::json!({ "windows": ["w5"] })),
            command(
                2,
                "semantics.act",
                serde_json::json!({
                    "element": "w5/e3", "action": "focus", "window": "w5",
                    "notes": ["w5"], "other": "w5", "nothing": null,
                }),
            ),
            ok(2, serde_json::json!({})),
            command(3, "session.close", serde_json::json!({})),
            ok(3, serde_json::json!({})),
            command(
                4,
                "pixels.check",
                serde_json::json!({ "window": "w5", "source": "os" }),
            ),
            command(
                5,
                "input.release",
                serde_json::json!({ "note": "no window" }),
            ),
            ok(5, serde_json::json!({})),
        ],
    );
    assert!(
        marked.contains(
            "    driver.call_value(\"input.release\", serde_json::json!({\"note\": \"no window\"}))?;"
        ) && !marked.contains("id();\n    driver.call_value(\"input.release\""),
        "a command that names no window does not read one: {marked}"
    );
    assert!(
        marked.contains("session.close (seq 3): it begins or ends a session"),
        "{marked}"
    );
    assert!(
        marked.contains("let window = driver.main_window()?.id();"),
        "{marked}"
    );
    assert!(marked.contains("\"window\": window"), "{marked}");
    assert!(
        marked.contains("\"notes\": [\"w5\"]"),
        "a list is not the window field: {marked}"
    );
    assert!(marked.contains("\"other\": \"w5\""), "{marked}");
    assert!(marked.contains("\"nothing\": null"), "{marked}");
    assert!(
        marked.contains("// not generated: session.close (seq 3)"),
        "{marked}"
    );
    assert!(
        marked.contains("// not generated: pixels.check (seq 4)"),
        "{marked}"
    );
    assert!(
        marked.contains("//! Not generated: 2 command(s)"),
        "{marked}"
    );
    assert!(marked.contains("needs `serde_json`"), "{marked}");
}

#[test]
fn a_trace_records_the_apps_layers_only_when_asked() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let dir = trace_dir(&root, "no_layers");
    let trace = record(
        &mut counter(),
        &dir,
        TraceOptions::new(&dir).record_layers(false),
        |driver| driver.windows().map(drop),
    );
    assert_eq!(trace.header().layers, []);
    assert_eq!(trace.input_recording(), None);
    let dir = trace_dir(&root, "layers");
    let trace = record(&mut counter(), &dir, TraceOptions::new(&dir), |driver| {
        driver.windows().map(drop)
    });
    assert_eq!(
        trace.header().layers,
        [flui_driver::protocol::record::RecordLayer::Input]
    );
}

#[test]
fn codegen_writes_the_checked_in_test_from_a_trace() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let dir = trace_dir(&root, "generated");
    let (lane, _) = ScriptedLane::new(Isolation::InProcess);
    let mut driver = counter()
        .with_lane(lane.screen(flat_screen))
        .expect("the lane");
    let trace = record(
        &mut driver,
        &dir,
        TraceOptions::new(&dir).keep_text(true),
        |driver| {
            tap_once(driver)?;
            // A pixel command, which codegen marks rather than drops.
            driver.main_window()?.capture(CaptureSource::Os)?;
            // Text with a control character, sent as recorded: the generated literal compiles.
            driver
                .main_window()?
                .perform(ActionSequence::new(vec![InputSource::Text {
                    text: "\u{1}".into(),
                }]))
                .map(drop)
        },
    );
    let options = CodegenOptions::new(
        "a_generated_test_taps_the_counter",
        "Launch::headless(|| crate::fixtures::Counter { probe: crate::fixtures::Probe::default() })",
    );
    let written = trace::codegen(&trace, &options).expect("a test");
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/trace_generated.rs");
    if std::env::var_os("FLUI_OVERWRITE").is_some() {
        std::fs::write(&golden, &written).expect("the generated test");
    }
    let expected = std::fs::read_to_string(&golden).expect("the generated test");
    assert_eq!(
        written, expected,
        "tests/trace_generated.rs is this trace's codegen, compiled and run as a test of this \
         suite; FLUI_OVERWRITE=1 rewrites it"
    );
    let refused = trace::codegen(&trace, &CodegenOptions::new("not a name", "x"));
    assert!(
        matches!(refused, Err(Error::InvalidArgument { .. })),
        "{refused:?}"
    );
}

#[test]
fn every_text_field_of_the_wire_is_redacted_or_named_as_kept() {
    // Strings of the wire that carry no text the app shows or the user typed.
    const KEPT: &[&str] = &[
        // Handles, ids the author or the toolkit assigns (ADR-0240), and paths the caller gave.
        "artifact",
        "automation_id",
        "class_name",
        "endpoint",
        "handle",
        "id",
        "identifier",
        "persistent_key",
        "root",
        "token_file",
        "tree",
        // Names from a vocabulary: methods, capabilities, actions, kinds, roles, media types,
        // the driver's own reasons and details.
        "action",
        "capability",
        "client",
        "detail",
        "kind",
        "kind_args",
        "links",
        "media",
        "member_type",
        "method",
        "methods",
        "mime",
        "native_role",
        "provenance",
        "reason",
        "reasons",
        "tags",
        "target",
        "ty",
        // Key identities, which ADR-0215 §5 keeps: a key's name, code, modifiers and dead key;
        // the character a key types is redacted.
        "code",
        "dead",
        "key",
        "keys",
        "modifiers",
        "named",
        // A launch's arguments, as the caller wrote them.
        "args",
        // The regional preferences an app reads, which name a locale, not what anyone typed.
        "first_weekday",
        "hour_cycle",
        "locales",
        "measurement",
        "numbering",
        "region",
    ];
    let schema_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../flui-protocol/tests/schema/protocol-{}.json",
        flui_driver::protocol::version::PROTOCOL_VERSION
    ));
    let schema: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&schema_path).expect("the schema golden"))
            .expect("JSON");
    let mut free = std::collections::BTreeSet::new();
    free_strings(&schema, &mut free);
    let unclassified: Vec<&String> = free
        .iter()
        .filter(|name| {
            !trace::TEXT_FIELDS.contains(&name.as_str()) && !KEPT.contains(&name.as_str())
        })
        .collect();
    assert!(
        unclassified.is_empty(),
        "string fields neither redacted (trace::TEXT_FIELDS) nor kept: {unclassified:?}"
    );
}

#[test]
fn no_alternative_types_a_field_two_ways() {
    // A replay follows every alternative of a choice (an optional value, a tagged enum) when it
    // looks for handles, which finds the right positions only while no two alternatives of one
    // choice type a field of the same name as a handle in one and as something else in another.
    let schema_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../flui-protocol/tests/schema/protocol-{}.json",
        flui_driver::protocol::version::PROTOCOL_VERSION
    ));
    let schema: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&schema_path).expect("the schema golden"))
            .expect("JSON");
    let definitions = schema["$defs"].as_object().expect("definitions").clone();
    let resolve = |start: &serde_json::Value| -> serde_json::Value {
        let mut node = start.clone();
        for _ in 0..64 {
            let Some(name) = node
                .get("$ref")
                .and_then(serde_json::Value::as_str)
                .and_then(|reference| reference.strip_prefix("#/$defs/"))
            else {
                break;
            };
            if ["WindowId", "ElementId", "ElementRef"].contains(&name) {
                break;
            }
            match definitions.get(name) {
                Some(definition) => node = definition.clone(),
                None => break,
            }
        }
        node
    };
    let mut conflicts = Vec::new();
    let mut choices = Vec::new();
    collect_choices(&schema, &mut choices);
    for options in choices {
        let mut typed: std::collections::BTreeMap<String, bool> = std::collections::BTreeMap::new();
        for option in &options {
            let option = resolve(option);
            let Some(properties) = option
                .get("properties")
                .and_then(serde_json::Value::as_object)
            else {
                continue;
            };
            for (name, property) in properties {
                let handle = is_handle_schema(property);
                if let Some(earlier) = typed.insert(name.clone(), handle)
                    && earlier != handle
                {
                    conflicts.push(name.clone());
                }
            }
        }
    }
    assert!(
        conflicts.is_empty(),
        "fields a handle in one alternative and not in another: {conflicts:?}"
    );
}

/// Every `anyOf` and `oneOf` list anywhere in `schema`.
fn collect_choices(schema: &serde_json::Value, found: &mut Vec<Vec<serde_json::Value>>) {
    match schema {
        serde_json::Value::Object(fields) => {
            for key in ["anyOf", "oneOf"] {
                if let Some(serde_json::Value::Array(options)) = fields.get(key) {
                    found.push(options.clone());
                }
            }
            for value in fields.values() {
                collect_choices(value, found);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_choices(item, found);
            }
        }
        _ => {}
    }
}

/// Whether a property's schema is a handle, alone, optional or in a list.
fn is_handle_schema(schema: &serde_json::Value) -> bool {
    let reference = |schema: &serde_json::Value| {
        schema
            .get("$ref")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|reference| {
                [
                    "#/$defs/WindowId",
                    "#/$defs/ElementId",
                    "#/$defs/ElementRef",
                ]
                .contains(&reference)
            })
    };
    reference(schema)
        || schema.get("items").is_some_and(reference)
        || ["anyOf", "oneOf"].iter().any(|key| {
            schema
                .get(*key)
                .and_then(serde_json::Value::as_array)
                .is_some_and(|options| options.iter().any(&reference))
        })
}

#[test]
fn no_field_the_trace_redacts_holds_a_handle() {
    let schema_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../flui-protocol/tests/schema/protocol-{}.json",
        flui_driver::protocol::version::PROTOCOL_VERSION
    ));
    let schema: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&schema_path).expect("the schema golden"))
            .expect("JSON");
    let mut handles = std::collections::BTreeSet::new();
    handle_properties(&schema, &mut handles);
    // A replay finds handles by each method's schema, so a name that is a handle in one type and
    // text in another (`id`) is mapped where it is a handle and left alone where it is text. The
    // redaction goes by name: a name it redacts must be text in every type, or it would destroy
    // the handles a replay pairs.
    let redacted: Vec<&String> = handles
        .iter()
        .filter(|name| trace::TEXT_FIELDS.contains(&name.as_str()))
        .collect();
    assert!(
        redacted.is_empty(),
        "fields both redacted and holding a handle somewhere: {redacted:?}"
    );
    assert!(
        handles.contains("id") && handles.contains("window"),
        "{handles:?}"
    );
}

/// The names of the properties anywhere in `schema` whose value is a handle, alone, optional or
/// in a list.
fn handle_properties(schema: &serde_json::Value, found: &mut std::collections::BTreeSet<String>) {
    fn is_handle(schema: &serde_json::Value) -> bool {
        let reference = |schema: &serde_json::Value| {
            schema
                .get("$ref")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|reference| {
                    [
                        "#/$defs/WindowId",
                        "#/$defs/ElementId",
                        "#/$defs/ElementRef",
                    ]
                    .contains(&reference)
                })
        };
        reference(schema)
            || schema.get("items").is_some_and(reference)
            || ["anyOf", "oneOf"].iter().any(|key| {
                schema
                    .get(*key)
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|options| options.iter().any(&reference))
            })
    }
    match schema {
        serde_json::Value::Object(fields) => {
            if let Some(serde_json::Value::Object(properties)) = fields.get("properties") {
                for (name, property) in properties {
                    if is_handle(property) {
                        found.insert(name.clone());
                    }
                }
            }
            for value in fields.values() {
                handle_properties(value, found);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                handle_properties(item, found);
            }
        }
        _ => {}
    }
}

/// The names of the properties anywhere in `schema` that hold a free string: a string with no
/// `const`, `enum` or `pattern`, alone or beside `null`.
fn free_strings(schema: &serde_json::Value, found: &mut std::collections::BTreeSet<String>) {
    match schema {
        serde_json::Value::Object(fields) => {
            if let Some(serde_json::Value::Object(properties)) = fields.get("properties") {
                for (name, property) in properties {
                    if is_free_string(property) {
                        found.insert(name.clone());
                    }
                }
            }
            for value in fields.values() {
                free_strings(value, found);
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|item| free_strings(item, found)),
        _ => {}
    }
}

fn is_free_string(property: &serde_json::Value) -> bool {
    let plain = |schema: &serde_json::Value| {
        let typed = match schema.get("type") {
            Some(serde_json::Value::String(kind)) => kind == "string",
            Some(serde_json::Value::Array(kinds)) => kinds.iter().any(|kind| kind == "string"),
            _ => false,
        };
        typed
            && ["const", "enum", "pattern", "format"]
                .iter()
                .all(|key| schema.get(*key).is_none())
    };
    if plain(property) {
        return true;
    }
    if let Some(items) = property.get("items")
        && plain(items)
    {
        return true;
    }
    ["anyOf", "oneOf"].iter().any(|key| {
        property
            .get(*key)
            .and_then(serde_json::Value::as_array)
            .is_some_and(|options| options.iter().any(&plain))
    })
}

/// A `record_native` event line of stream 0 of recording 1.
#[cfg(any(unix, windows))]
fn native_line(line: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "event": "record_native",
        "params": { "recording": 1, "window": "w1", "stream": 0, "line": line },
    })
}

/// A `record` event line of recording 1.
#[cfg(any(unix, windows))]
fn input_row(row: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "event": "record",
        "params": { "recording": 1, "window": "w1", "row": row },
    })
}

#[cfg(any(unix, windows))]
#[test]
fn a_trace_holds_the_apps_native_recording_and_reads_it_back_without_the_core() {
    use flui_driver::protocol::native_recording::{Backend, Row as NativeRow};
    use flui_driver::protocol::record::RecordLayer;
    use flui_driver::protocol::session::Capabilities;
    use flui_driver::testing::{AfterLines, Reply, ScriptedAgent};
    use serde_json::json;

    let mut capabilities = Capabilities::default();
    capabilities.methods = [
        "windows.list",
        "window.placement",
        "record.start",
        "record.stop",
    ]
    .map(str::to_owned)
    .to_vec();
    capabilities.record = vec![RecordLayer::Input, RecordLayer::Native];
    let mut asked_native = false;
    let agent = ScriptedAgent::start_with(
        capabilities,
        Box::new(move |method, params| match method {
            "windows.list" => Reply::Result(json!({ "windows": ["w1"] })),
            "window.placement" => Reply::Result(json!({
                "window": "w1", "size": { "width": 320.0, "height": 200.0 }, "scale": 2.0,
                "visible": true, "focused": true,
            })),
            "record.start" => {
                asked_native = params["layers"] == json!(["input", "native"]);
                Reply::ResultThenLines {
                    result: json!({
                        "recording": 1,
                        "header": { "flui_recording": 6, "clock": "monotonic", "windows": [{ "window": 1, "scale_factor": 2.0 }] },
                    }),
                    lines: vec![
                        native_line(json!({ "header": { "flui_native_recording": 4, "backend": "win32" } })),
                        native_line(json!({ "row": { "row": "click_settings" } })),
                        native_line(json!({ "gap": { "rows": 2 } })),
                        input_row(json!({ "deadline": { "window": 1, "t": 5, "member": 1 } })),
                        json!({ "event": "gap", "params": { "missed": 3 } }),
                    ],
                    then: AfterLines::Serve,
                }
            }
            // The app sends the recording's `end` row before it answers the stop, as the agent
            // does (ADR-0288 §11).
            "record.stop" => Reply::LinesThenResult {
                lines: vec![input_row(json!({ "end": { "lost_rows": 0 } }))],
                result: json!({ "rows": 1, "lost_rows": 0, "native_lost_rows": 2, "complete": asked_native }),
            },
            _ => Reply::Close,
        }),
    )
    .expect("the stand-in starts");
    let mut driver =
        Driver::attach(&agent.endpoint(), agent.token(), Duration::from_secs(5)).expect("attached");
    let root = tempfile::tempdir().expect("a temporary directory");
    let dir = trace_dir(&root, "native");
    driver
        .start_trace(TraceOptions::new(&dir))
        .expect("the trace starts");
    driver
        .windows()
        .expect("a command between the start and the stop");
    let mut report = Report::new();
    report.push(flui_driver::Check::new(
        "recorded",
        flui_driver::Outcome::Passed,
    ));
    let summary = driver
        .finish_trace(&mut report)
        .expect("the trace finishes");
    let stopped = summary
        .recording
        .expect("the app's recording came to an end");
    assert!(
        stopped.complete,
        "record.start asked for the input and native layers"
    );
    assert_eq!(
        (
            summary.native.len(),
            summary.native[0].rows,
            summary.native[0].lost_rows
        ),
        (1, 1, 2)
    );

    let trace = Trace::open(&dir).expect("the trace reads back");
    assert_eq!(
        trace.header().layers,
        [RecordLayer::Input, RecordLayer::Native]
    );
    let input =
        std::fs::read_to_string(trace.input_recording().expect("input.jsonl")).expect("it reads");
    let lines: Vec<&str> = input.lines().collect();
    assert_eq!(lines.len(), 3, "the header, the row and the end: {input}");
    assert!(lines[2].contains("\"end\""), "{input}");
    let native = trace
        .native_recordings()
        .expect("the native layer reads through the protocol");
    assert_eq!(native.len(), 1);
    assert_eq!(native[0].header.backend, Backend::Win32);
    let rows: Vec<&NativeRow> = native[0].rows().collect();
    assert!(
        matches!(
            rows.as_slice(),
            [NativeRow::ClickSettings { settings: None, .. }]
        ),
        "{rows:?}"
    );
    assert_eq!(
        native[0].lost_rows(),
        2,
        "the gap stays where the stream lost rows"
    );
    let (events, dropped) = driver.take_events();
    assert_eq!(dropped, 0);
    assert!(
        matches!(events.as_slice(), [flui_driver::protocol::events::Event::Gap(gap)] if gap.missed == 3),
        "the recording's rows were the trace's, and every other event is still the caller's: \
         {events:?}"
    );
    let event = trace
        .rows()
        .iter()
        .find_map(|row| match row {
            Row::Event(event) => Some(event.clone()),
            _ => None,
        })
        .expect("the event is in the trace too");
    assert_eq!(
        (event.name.as_str(), &event.params),
        ("gap", &json!({ "missed": 3 }))
    );
    drop(driver);
    agent
        .join(Duration::from_secs(5))
        .expect("the stand-in ends");
}

#[test]
fn a_native_recording_past_its_bounds_or_of_a_newer_version_is_refused() {
    use flui_driver::protocol::native_recording::MAX_LINE_BYTES;
    use flui_driver::trace::native;

    let root = tempfile::tempdir().expect("a temporary directory");
    let path = root.path().join("0.jsonl");
    let header = r#"{"flui_native_recording":4,"backend":"headless"}"#;
    std::fs::write(&path, format!("{header}\n{{\"row\":\"click_settings\"}}\n")).expect("the file");
    let read = native::read(&path).expect("a recording within its bounds");
    assert_eq!(read.rows().count(), 1);

    let cases = [
        (
            "newer",
            r#"{"flui_native_recording":99,"backend":"headless"}"#.to_owned() + "\n",
            "version 99",
        ),
        (
            "long",
            format!(
                "{header}\n{{\"row\":\"click_settings\",\"pad\":\"{}\"}}\n",
                "x".repeat(MAX_LINE_BYTES)
            ),
            "longer than",
        ),
        (
            "unknown",
            format!("{header}\n{{\"row\":\"teleport\"}}\n"),
            "not a row",
        ),
    ];
    for (name, text, says) in cases {
        std::fs::write(&path, text).expect("the file");
        let refused = native::read(&path).expect_err(name);
        let Error::Unreadable { message, line, .. } = &refused else {
            panic!("{name}: {refused:?}");
        };
        assert!(message.contains(says), "{name}: {message}");
        assert!(line.is_some(), "{name}: the line is named");
    }
}
