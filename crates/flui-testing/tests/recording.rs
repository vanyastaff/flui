//! Input recordings (ADR-0215): what a host applies is recorded with the
//! arena decisions it made, written as JSON Lines with text redacted by
//! default, read back, checked whole, and replayed to the same decisions.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::geometry::Offset;
use flui_interaction::GestureDisposition;
use flui_interaction::PointerId;
use flui_interaction::events::{PointerKind, make_down_event_for_id, make_up_event_for_id};
use flui_platform_api::{GesturePreferences, PlatformInput, SystemPreferences};
use flui_runtime::{DecisionRow, InputRecord};
use flui_testing::recording::{
    Privacy, RecordingError, RecordingReader, RecordingWriter, ReplayableRecording, record,
};
use flui_testing::{HeadlessHost, HeadlessWindow};

/// A box that taps and long-presses, logging which fired.
fn host(log: &Rc<RefCell<Vec<&'static str>>>) -> HeadlessHost {
    host_on(HeadlessWindow::new(200, 200), log)
}

/// [`host`] presenting into `window`.
fn host_on(window: HeadlessWindow, log: &Rc<RefCell<Vec<&'static str>>>) -> HeadlessHost {
    let mut host = HeadlessHost::new(window);
    let (taps, presses) = (Rc::clone(log), Rc::clone(log));
    host.attach(
        &flui_widgets::GestureDetector::new()
            .on_tap(move |_| taps.borrow_mut().push("tap"))
            .on_long_press(move |_| presses.borrow_mut().push("long_press"))
            .child(flui_widgets::ColoredBox::new(
                flui_painting::styling::Color::RED,
            )),
    )
    .expect("attach");
    let _ = host.pump(Duration::from_millis(16));
    host
}

fn touch(host: &HeadlessHost, id: u64, down: bool) {
    let id = PointerId::try_from(id).expect("nonzero pointer");
    let at = Offset::new(50.0, 50.0);
    let event = if down {
        make_down_event_for_id(id, at, PointerKind::Touch)
    } else {
        make_up_event_for_id(id, at, PointerKind::Touch)
    };
    host.dispatch(PlatformInput::Pointer(event.expect("finite position")));
}

/// Hold `id` for `frames` frames of 16 ms, then lift it.
fn press(host: &mut HeadlessHost, id: u64, frames: u32) {
    touch(host, id, true);
    for _ in 0..frames {
        let _ = host.pump(Duration::from_millis(16));
    }
    touch(host, id, false);
    let _ = host.pump(Duration::from_millis(16));
}

/// Record `session` on a fresh host, finish the recording and return its bytes.
fn recorded(
    privacy: Privacy,
    session: impl FnOnce(&mut HeadlessHost),
) -> (Vec<u8>, Vec<&'static str>) {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut host = host(&log);
    let writer = RecordingWriter::new(Vec::new(), &host.recording_header(privacy))
        .expect("the header writes");
    let recording = record(host.input_taps(), writer).expect("first tap registration");
    session(&mut host);
    let bytes = recording.finish().expect("a complete recording");
    let fired = log.borrow().clone();
    (bytes, fired)
}

fn replayable(bytes: &[u8]) -> Result<ReplayableRecording, RecordingError> {
    ReplayableRecording::read(RecordingReader::new(bytes)?)
}

fn accepted(decisions: &[DecisionRow]) -> Vec<(u64, String)> {
    decisions
        .iter()
        .filter(|decision| decision.disposition == GestureDisposition::Accepted)
        .map(|decision| {
            let member = decision.member_type.rsplit("::").next().unwrap_or_default();
            (decision.pointer.get().get(), member.to_owned())
        })
        .collect()
}

#[test]
fn a_recording_replays_to_the_decisions_it_recorded() {
    let (bytes, fired) = recorded(Privacy::Redact, |host| {
        press(host, 1, 3);
        press(host, 2, 40);
    });
    assert_eq!(fired, ["tap", "long_press"]);
    let recording = replayable(&bytes).expect("a whole recording");
    let recorded = recording.decisions();
    assert_eq!(
        accepted(&recorded),
        [
            (1, "TapArenaMember".to_owned()),
            (2, "LongPressGestureRecognizer".to_owned())
        ],
        "the recording holds each arena's winner, the deadline's included"
    );

    let log = Rc::new(RefCell::new(Vec::new()));
    let mut fresh = host(&log);
    let replayed = fresh.replay(&recording);
    assert_eq!(
        replayed, recorded,
        "the replay decided as the recording did"
    );
    assert_eq!(*log.borrow(), ["tap", "long_press"]);
}

/// A window moved to a monitor of another scale mid-recording: the change is
/// a `Metrics` row at its position, and the replay applies it there.
#[test]
fn a_scale_change_mid_recording_replays_to_the_same_decisions() {
    use flui_platform_api::PlatformWindow as _;
    use flui_runtime::{HostInput, owner::WindowObservation};

    let (bytes, fired) = recorded(Privacy::Redact, |host| {
        press(host, 1, 3);
        host.set_scale_factor(host.primary_window(), 1.5);
        press(host, 2, 40);
    });
    assert_eq!(fired, ["tap", "long_press"]);
    let recording = replayable(&bytes).expect("a whole recording");
    let metrics = recording
        .rows()
        .iter()
        .position(|row| {
            matches!(row, InputRecord::Input(row) if matches!(
                row.input,
                HostInput::Window(WindowObservation::Metrics { scale_factor, .. })
                    if scale_factor == 1.5
            ))
        })
        .expect("the scale change is a metrics row");
    let first_long_press = recording
        .rows()
        .iter()
        .position(|row| matches!(row, InputRecord::Decision(decision) if decision.pointer.get().get() == 2))
        .expect("the second sequence decided");
    assert!(
        metrics < first_long_press,
        "the row sits between the sequences"
    );

    let log = Rc::new(RefCell::new(Vec::new()));
    let mut fresh = host(&log);
    assert_eq!(fresh.replay(&recording), recording.decisions());
    assert_eq!(*log.borrow(), ["tap", "long_press"]);
    assert!(
        (fresh.window().scale_factor() - 1.5).abs() < f64::EPSILON,
        "the replay moved the window to the recorded scale"
    );
}

/// A decision row carries the time its arena committed the verdict, even
/// when the clock has moved on by the time the row is admitted: here the
/// tap's own callback steps the manual clock before the runtime admits it.
#[test]
fn a_decision_row_carries_its_commit_time() {
    let mut host = HeadlessHost::new(HeadlessWindow::new(200, 200));
    let clock = host.clock().clone();
    host.attach(
        &flui_widgets::GestureDetector::new()
            .on_tap(move |_| clock.advance(Duration::from_millis(7)))
            // A second member keeps the contest open until the Up decides it.
            .on_long_press(|_| {})
            .child(flui_widgets::ColoredBox::new(
                flui_painting::styling::Color::RED,
            )),
    )
    .expect("attach");
    let _ = host.pump(Duration::from_millis(16));
    let writer = RecordingWriter::new(Vec::new(), &host.recording_header(Privacy::Redact))
        .expect("the header writes");
    let recording = record(host.input_taps(), writer).expect("first tap registration");
    touch(&host, 1, true);
    host.clock().advance(Duration::from_millis(30));
    let committed = flui_platform_api::EventTime::from_nanos(
        u64::try_from(host.clock().elapsed().as_nanos()).expect("small"),
    );
    touch(&host, 1, false);
    let bytes = recording.finish().expect("a complete recording");
    let recording = replayable(&bytes).expect("a whole recording");
    let won = recording
        .decisions()
        .into_iter()
        .find(|decision| decision.disposition == GestureDisposition::Accepted)
        .expect("the tap won");
    assert_eq!(
        host.clock().elapsed().as_nanos() - u128::from(committed.as_nanos()),
        7_000_000,
        "the callback stepped the clock past the commit"
    );
    assert_eq!(won.t, committed, "stamped at commit, not at admission");
}

#[test]
fn a_preference_revision_between_sequences_replays_to_the_same_decisions() {
    let shorter = SystemPreferences::default().with_gestures(
        GesturePreferences::default().with_long_press_timeout(Duration::from_millis(200)),
    );
    let (bytes, fired) = recorded(Privacy::Redact, |host| {
        // 304 ms: a tap under the default timeout.
        press(host, 1, 19);
        host.set_system_preferences(shorter);
        // The same hold, now past the host's shorter timeout.
        press(host, 2, 19);
    });
    assert_eq!(
        fired,
        ["tap", "long_press"],
        "the revision applied between them"
    );
    let recording = replayable(&bytes).expect("a whole recording");
    assert!(
        recording
            .rows()
            .iter()
            .any(|row| matches!(row, InputRecord::Preferences(_))),
        "the revision is a row at its position"
    );

    let log = Rc::new(RefCell::new(Vec::new()));
    let mut fresh = host(&log);
    assert_eq!(fresh.replay(&recording), recording.decisions());
    assert_eq!(*log.borrow(), ["tap", "long_press"]);
}

/// A recording whose text was redacted, then the line `index` (0-based,
/// header first) replaced with `line`.
fn with_line(bytes: &[u8], index: usize, line: &str) -> Vec<u8> {
    let text = std::str::from_utf8(bytes).expect("utf-8 recording");
    let mut lines: Vec<&str> = text.lines().collect();
    lines[index] = line;
    let mut out = lines.join("\n");
    out.push('\n');
    out.into_bytes()
}

#[test]
fn an_unknown_variant_is_a_typed_error_not_a_skipped_row() {
    let (bytes, _) = recorded(Privacy::Redact, |host| press(host, 1, 1));
    let text = std::str::from_utf8(&bytes).expect("utf-8 recording");
    let pointer_row = text
        .lines()
        .position(|line| line.contains("\"down\""))
        .expect("a pointer-down row");
    for (row, tag) in [
        (
            text.lines()
                .nth(pointer_row)
                .expect("row")
                .replace("\"down\"", "\"pinch\""),
            "pinch",
        ),
        (r#"{"teleport":{"rows":1}}"#.to_owned(), "teleport"),
    ] {
        let damaged = with_line(&bytes, pointer_row, &row);
        let rows: Vec<_> = RecordingReader::new(&damaged[..])
            .expect("header")
            .collect();
        let line = u64::try_from(pointer_row + 1).expect("small");
        assert!(
            rows.iter().any(|row| matches!(
                row,
                Err(RecordingError::UnknownVariant { line: at, tag: found })
                    if *at == line && found == tag
            )),
            "`{tag}` on line {line} is a typed error"
        );
        assert_eq!(
            rows.len(),
            text.lines().count() - 1,
            "the reader neither skips nor stops at the unknown row"
        );
        assert!(matches!(
            replayable(&damaged),
            Err(RecordingError::UnknownVariant { .. })
        ));
    }

    // An unknown field is skipped within the major version.
    let extended = with_line(
        &bytes,
        pointer_row,
        &text.lines().nth(pointer_row).expect("row").replacen(
            r#"{"input":{"#,
            r#"{"input":{"added_later":1,"#,
            1,
        ),
    );
    replayable(&extended).expect("unknown fields are skipped");
}

#[test]
fn replay_stops_at_a_gap_with_non_replayable() {
    let (bytes, _) = recorded(Privacy::Redact, |host| press(host, 1, 1));
    let gapped = with_line(&bytes, 2, r#"{"gap":{"rows":1}}"#);
    assert!(matches!(
        replayable(&gapped),
        Err(RecordingError::NonReplayable { line: 3 })
    ));
}

#[test]
fn a_truncated_recording_dispatches_nothing() {
    let (bytes, _) = recorded(Privacy::Redact, |host| press(host, 1, 3));
    let text = std::str::from_utf8(&bytes).expect("utf-8 recording");
    let lines: Vec<&str> = text.lines().collect();
    // Everything but the `End` row: a tap that stopped, a crash, a cut file.
    let truncated = format!("{}\n", lines[..lines.len() - 1].join("\n"));
    let refused = replayable(truncated.as_bytes());
    assert!(
        matches!(refused, Err(RecordingError::NonReplayable { line }) if line == lines.len() as u64),
        "{refused:?}"
    );
    // Validation precedes dispatch: nothing was handed to a host, so the
    // rows that were whole cannot have run against a missing tail.
}

/// Rows carrying text in each place a recording redacts it.
fn text_rows() -> Vec<InputRecord> {
    use flui_platform_api::keyboard::{Code, Key, KeyEvent, KeyState};
    use flui_platform_api::{EventTime, ImeEvent, WindowId};
    use flui_runtime::owner::InputOutcome;
    use flui_runtime::{HostInput, InputRow};

    let at = EventTime::from_nanos(1_000);
    let request = |action, text: &str| accesskit::ActionRequest {
        action,
        target_tree: accesskit::TreeId::ROOT,
        target_node: accesskit::NodeId(7),
        data: Some(accesskit::ActionData::Value(text.into())),
    };
    [
        HostInput::Platform(PlatformInput::Keyboard(KeyEvent::new(
            KeyState::Down,
            Key::character("q"),
            Code::KeyQ,
            at,
        ))),
        HostInput::Platform(PlatformInput::Ime(ImeEvent::Preedit {
            text: "passw".to_owned(),
            cursor: Some((5, 5)),
        })),
        HostInput::Platform(PlatformInput::Ime(ImeEvent::Commit("password".to_owned()))),
        HostInput::Accessibility(request(accesskit::Action::SetValue, "hunter2")),
        HostInput::Accessibility(request(accesskit::Action::ReplaceSelectedText, "s3cret")),
    ]
    .into_iter()
    .map(|input| {
        InputRecord::Input(InputRow::new(
            WindowId(1),
            at,
            at,
            input,
            InputOutcome::Handled,
        ))
    })
    .chain([InputRecord::End { lost_rows: 0 }])
    .collect()
}

const SECRETS: [&str; 7] = [
    "\"q\"",
    "passw",
    "password",
    "hunter2",
    "s3cret",
    "Private window",
    "Secret pen",
];

fn written(privacy: Privacy) -> Vec<u8> {
    use flui_platform_api::WindowId;
    use flui_platform_api::pointer::{DeviceId, PenTool};
    use flui_testing::recording::{RecordedWindow, RecordingClock, RecordingHeader};

    let mut header = RecordingHeader::new(RecordingClock::Virtual)
        .with_window(RecordedWindow::new(WindowId(1), 1.5).with_title("Private window"))
        .with_device(
            DeviceId::try_from(3_u64).expect("nonzero"),
            PointerKind::Pen { tool: PenTool::Tip },
            Some("Secret pen".to_owned()),
        );
    header.privacy = privacy;
    let mut writer = RecordingWriter::new(Vec::new(), &header).expect("header");
    for row in text_rows() {
        writer.write(&row).expect("row");
    }
    writer.finish().expect("flush")
}

#[test]
fn recordings_redact_text_titles_and_device_names_by_default() {
    let redacted = written(Privacy::default());
    let text = String::from_utf8(redacted.clone()).expect("utf-8");
    for secret in SECRETS {
        assert!(!text.contains(secret), "{secret} reached the file:\n{text}");
    }
    let reader = RecordingReader::new(&redacted[..]).expect("header");
    assert_eq!(reader.header().privacy, Privacy::Redact);
    assert_ne!(
        reader.header().windows[0].title.as_deref(),
        Some("Private window")
    );
    let rows: Vec<_> = reader.map(|row| row.expect("row")).collect();
    assert!(
        rows[..5].iter().all(|row| row.redacted),
        "every row that lost text says so"
    );
    assert!(!rows[5].redacted, "the end row carried no text");

    // `--keep-text`: the one opt-in, covering all of them.
    let kept = written(Privacy::KeepText);
    let text = String::from_utf8(kept.clone()).expect("utf-8");
    for secret in SECRETS {
        assert!(
            text.contains(secret),
            "{secret} was dropped under keep-text"
        );
    }
    let read: Vec<String> = RecordingReader::new(&kept[..])
        .expect("header")
        .map(|row| {
            let row = row.expect("row");
            assert!(!row.redacted);
            format!("{:?}", row.record)
        })
        .collect();
    let original: Vec<String> = text_rows().iter().map(|row| format!("{row:?}")).collect();
    assert_eq!(read, original, "kept text round-trips");
}

/// A password field, recorded while assistive technology sets its value.
fn password_session(privacy: Privacy) -> (Vec<u8>, Vec<String>) {
    let (host, changes) = password_host();
    let mut host = host;
    let writer = RecordingWriter::new(Vec::new(), &host.recording_header(privacy))
        .expect("the header writes");
    let recording = record(host.input_taps(), writer).expect("first tap registration");
    set_password(&mut host, "hunter2");
    let bytes = recording.finish().expect("a complete recording");
    let changed = changes.borrow().clone();
    (bytes, changed)
}

fn password_host() -> (HeadlessHost, Rc<RefCell<Vec<String>>>) {
    use flui_interaction::FocusNode;
    use flui_widgets::{EditableText, TextEditingController};

    let changes = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&changes);
    let mut host = HeadlessHost::new(HeadlessWindow::new(400, 100).with_text_input());
    host.attach(
        &EditableText::new(
            TextEditingController::new(),
            FocusNode::with_debug_label("password"),
        )
        .obscure_text(true)
        .on_changed(move |_cx, text| sink.borrow_mut().push(text.to_owned())),
    )
    .expect("attach");
    host.enable_semantics();
    let _ = host.pump(Duration::ZERO);
    (host, changes)
}

fn set_password(host: &mut HeadlessHost, text: &str) {
    let tree = host
        .published_a11y_tree(host.primary_window())
        .expect("published semantics");
    let field = tree
        .nodes()
        .find(|node| node.supports_action(accesskit::Action::SetValue))
        .expect("the password field takes SetValue")
        .id();
    host.accessibility_action_listener()
        .expect("platform listener")(accesskit::ActionRequest {
        action: accesskit::Action::SetValue,
        target_tree: accesskit::TreeId::ROOT,
        target_node: field,
        data: Some(accesskit::ActionData::Value(text.into())),
    });
    let _ = host.pump(Duration::ZERO);
}

#[test]
fn an_assistive_set_value_on_a_password_field_records_no_plaintext() {
    let (bytes, changed) = password_session(Privacy::default());
    assert_eq!(changed, ["hunter2"], "the field took the value");
    let text = String::from_utf8(bytes.clone()).expect("utf-8");
    assert!(text.contains("set_value"), "the action is recorded");
    assert!(!text.contains("hunter2"), "no plaintext reached the file");
    // The redacted row cannot claim to reproduce what depended on its text.
    assert!(matches!(
        replayable(&bytes),
        Err(RecordingError::NonReplayable { .. })
    ));

    // The same recording made with `--keep-text` replays to its decisions
    // and to the same value.
    let (bytes, _) = password_session(Privacy::KeepText);
    let recording = replayable(&bytes).expect("a whole, unredacted recording");
    let (mut fresh, changes) = password_host();
    assert_eq!(fresh.replay(&recording), recording.decisions());
    assert_eq!(*changes.borrow(), ["hunter2"]);
}

/// A sink that hands its bytes over when the recording worker lets go of it.
struct Handover {
    bytes: Vec<u8>,
    done: std::sync::mpsc::Sender<Vec<u8>>,
}

impl std::io::Write for Handover {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for Handover {
    fn drop(&mut self) {
        let _ = self.done.send(std::mem::take(&mut self.bytes));
    }
}

/// An application's tap, installed for its runtime's life, still ends its
/// recording whole when the runtime goes away.
#[test]
fn a_factory_tap_ends_its_recording_when_the_runtime_goes_away() {
    let (done, handed_over) = std::sync::mpsc::channel();
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut host = host(&log);
    let header = host.recording_header(Privacy::Redact);
    let factory = flui_testing::recording::tap_factory(move || {
        RecordingWriter::new(
            Handover {
                bytes: Vec::new(),
                done: done.clone(),
            },
            &header,
        )
        .expect("the header writes")
    });
    factory
        .install_on(host.input_taps())
        .expect("first tap registration");
    press(&mut host, 1, 3);
    drop(host);
    let bytes = handed_over
        .recv_timeout(Duration::from_secs(10))
        .expect("the worker hands the sink over once the runtime is gone");
    let recording = replayable(&bytes).expect("the recording ends with its end row");
    assert_eq!(
        accepted(&recording.decisions()),
        [(1, "TapArenaMember".to_owned())]
    );
}

/// Two contacts reusing one pointer ID are two sequences in a recording: each
/// pointer row names the sequence its dispatch placed it in, and each verdict
/// the sequence its contest decided.
#[test]
fn a_reused_pointer_id_records_two_sequences() {
    use flui_runtime::HostInput;

    let (bytes, fired) = recorded(Privacy::Redact, |host| {
        press(host, 1, 3);
        press(host, 1, 3);
    });
    assert_eq!(fired, ["tap", "tap"]);
    let recording = replayable(&bytes).expect("a whole recording");
    let won: Vec<_> = recording
        .decisions()
        .into_iter()
        .filter(|decision| decision.disposition == GestureDisposition::Accepted)
        .map(|decision| (decision.pointer.get().get(), decision.sequence))
        .collect();
    assert_eq!(won.len(), 2, "each contact's contest decided");
    assert!(
        won.iter()
            .all(|(pointer, sequence)| *pointer == 1 && sequence.is_some())
    );
    assert_ne!(won[0].1, won[1].1, "a reused pointer ID is a new sequence");
    let downs: Vec<_> = recording
        .rows()
        .iter()
        .filter_map(|row| match row {
            InputRecord::Input(row)
                if matches!(
                    row.input,
                    HostInput::Platform(PlatformInput::Pointer(
                        flui_interaction::PointerEvent::Down(_)
                    ))
                ) =>
            {
                Some(row.sequence)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        downs,
        won.iter()
            .map(|(_, sequence)| *sequence)
            .collect::<Vec<_>>(),
        "each Down row names the sequence its contest decided"
    );
}

/// Two members of one type compete for one contact: a recording names each
/// by its own number, so a replay that let the other one win would not match.
#[test]
fn two_members_of_one_type_replay_to_the_right_winner() {
    fn nested(log: &Rc<RefCell<Vec<&'static str>>>) -> HeadlessHost {
        let mut host = HeadlessHost::new(HeadlessWindow::new(200, 200));
        let (outer, inner) = (Rc::clone(log), Rc::clone(log));
        host.attach(
            &flui_widgets::GestureDetector::new()
                .on_tap(move |_| outer.borrow_mut().push("outer"))
                .child(
                    flui_widgets::GestureDetector::new()
                        .on_tap(move |_| inner.borrow_mut().push("inner"))
                        .child(flui_widgets::ColoredBox::new(
                            flui_painting::styling::Color::RED,
                        )),
                ),
        )
        .expect("attach");
        let _ = host.pump(Duration::from_millis(16));
        host
    }

    let log = Rc::new(RefCell::new(Vec::new()));
    let mut host = nested(&log);
    let writer = RecordingWriter::new(Vec::new(), &host.recording_header(Privacy::Redact))
        .expect("the header writes");
    let recording = record(host.input_taps(), writer).expect("first tap registration");
    press(&mut host, 1, 1);
    let bytes = recording.finish().expect("a complete recording");
    assert_eq!(*log.borrow(), ["inner"]);
    let recording = replayable(&bytes).expect("a whole recording");
    let recorded = recording.decisions();
    let verdict = |disposition| {
        recorded
            .iter()
            .find(|decision| decision.disposition == disposition)
            .expect("both members were decided")
            .clone()
    };
    let (won, lost) = (
        verdict(GestureDisposition::Accepted),
        verdict(GestureDisposition::Rejected),
    );
    assert_eq!(
        won.member_type, lost.member_type,
        "the members share a type"
    );
    assert_ne!(won.member, lost.member, "but not a number");

    let replay_log = Rc::new(RefCell::new(Vec::new()));
    assert_eq!(nested(&replay_log).replay(&recording), recorded);
    assert_eq!(*replay_log.borrow(), ["inner"]);

    // What a replay that crowned the other member would report.
    let swapped: Vec<_> = recorded
        .iter()
        .cloned()
        .map(|mut decision| {
            decision.member = if decision.member == won.member {
                lost.member
            } else {
                won.member
            };
            decision
        })
        .collect();
    assert_ne!(
        swapped, recorded,
        "the other member's win is a different decision"
    );
}

/// Input an embedded surface reported is recorded with that surface, and a
/// replay routes it to the surface the replaying host embedded in its place.
#[test]
fn input_to_an_embedded_surface_records_and_replays_to_the_same_decisions() {
    use flui_runtime::{HostInput, RecordedSurface};

    fn touch_on(host: &HeadlessHost, surface: flui_interaction::InputSurface, down: bool) {
        let id = PointerId::try_from(4_u64).expect("nonzero pointer");
        let at = Offset::new(50.0, 50.0);
        let event = if down {
            make_down_event_for_id(id, at, PointerKind::Touch)
        } else {
            make_up_event_for_id(id, at, PointerKind::Touch)
        };
        host.dispatch_on(surface, PlatformInput::Pointer(event.expect("finite")));
    }

    let (bytes, fired) = recorded(Privacy::Redact, |host| {
        let surface = host.embed_surface().expect("a surface identity");
        touch_on(host, surface, true);
        let _ = host.pump(Duration::from_millis(16));
        touch_on(host, surface, false);
        let _ = host.pump(Duration::from_millis(16));
        // One wheel, two surfaces: each surface latches a sequence of its own.
        let wheel = || {
            PlatformInput::Pointer(
                flui_interaction::events::make_scroll_event(
                    Offset::new(50.0, 50.0),
                    Offset::new(0.0, 10.0),
                )
                .expect("finite"),
            )
        };
        host.dispatch_on(surface, wheel());
        host.dispatch(wheel());
    });
    assert_eq!(fired, ["tap"]);
    let recording = replayable(&bytes).expect("a whole recording");
    let surfaces: Vec<_> = recording
        .rows()
        .iter()
        .filter_map(|row| match row {
            InputRecord::Input(row) if matches!(row.input, HostInput::Platform(_)) => {
                Some(row.surface)
            }
            _ => None,
        })
        .collect();
    assert_eq!(surfaces.len(), 4);
    assert!(
        surfaces[..3]
            .iter()
            .all(|surface| matches!(surface, RecordedSurface::Embedded(_))),
        "{surfaces:?}"
    );
    assert_eq!(surfaces[3], RecordedSurface::Window);
    let wheels: Vec<_> = recording
        .rows()
        .iter()
        .filter_map(|row| match row {
            InputRecord::Input(row)
                if matches!(
                    row.input,
                    HostInput::Platform(PlatformInput::Pointer(
                        flui_interaction::PointerEvent::Scroll(_)
                    ))
                ) =>
            {
                Some(row.sequence)
            }
            _ => None,
        })
        .collect();
    assert!(wheels.iter().all(Option::is_some), "{wheels:?}");
    assert_ne!(
        wheels[0], wheels[1],
        "the embedded surface's wheel latch is apart from the window's"
    );

    let log = Rc::new(RefCell::new(Vec::new()));
    let mut fresh = host(&log);
    fresh.embed_surface().expect("a surface identity");
    assert_eq!(fresh.replay(&recording), recording.decisions());
    assert_eq!(*log.borrow(), ["tap"]);

    // Replay resolves the recorded surface against the host's own: a host
    // that embedded none cannot replay the recording onto its window.
    let mut bare = host(&Rc::new(RefCell::new(Vec::new())));
    let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = bare.replay(&recording);
    }));
    assert!(
        refused.is_err(),
        "an unembedded surface is not replayed as the window"
    );
}

/// The header holds each window's gesture values, not a preset's name, and a
/// replay restores them before its first row: a hold that was a tap under
/// the recorded long-press timeout stays a tap on a host configured with a
/// shorter one.
#[test]
fn a_recording_replays_under_its_recorded_gesture_values() {
    let (bytes, fired) = recorded(Privacy::Redact, |host| press(host, 1, 19));
    assert_eq!(fired, ["tap"], "304 ms is under the default timeout");
    let recording = replayable(&bytes).expect("a whole recording");
    let recorded_values = recording.header().windows[0]
        .gesture_profiles
        .clone()
        .expect("the header holds the window's gesture values");
    assert_eq!(
        recorded_values
            .for_kind(PointerKind::Touch)
            .long_press_timeout(),
        Duration::from_millis(500)
    );

    let shorter = flui_interaction::GestureSettings::default()
        .with_long_press_timeout(Duration::from_millis(200));
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut other = host_on(
        HeadlessWindow::new(200, 200).with_gesture_preset(shorter.clone()),
        &log,
    );
    // Unrestored, the same hold is a long press on this host.
    press(&mut other, 7, 19);
    assert_eq!(*log.borrow(), ["long_press"]);
    log.borrow_mut().clear();

    let mut fresh = host_on(
        HeadlessWindow::new(200, 200).with_gesture_preset(shorter),
        &log,
    );
    assert_eq!(fresh.replay(&recording), recording.decisions());
    assert_eq!(*log.borrow(), ["tap"]);
}

/// Every gesture value a header stores reads back exactly, with each
/// estimator and remainder policy.
#[test]
fn header_gesture_values_read_back_exactly() {
    use flui_interaction::gesture::{OneEuroTuning, VelocityEstimator};
    use flui_interaction::{GestureProfiles, GestureSettings, RemainderHandoff};
    use flui_platform_api::{TargetPlatform, WindowId};
    use flui_testing::recording::{RecordedWindow, RecordingClock, RecordingHeader};

    let estimators = [
        VelocityEstimator::LeastSquares,
        VelocityEstimator::Impulse,
        VelocityEstimator::Ios,
        VelocityEstimator::Macos,
    ];
    let handoffs = [
        RemainderHandoff::NextSequence,
        RemainderHandoff::AtEdgeWithinSequence,
    ];
    for (index, estimator) in estimators.into_iter().enumerate() {
        let base = GestureProfiles::preset(TargetPlatform::Android);
        let mut values = base.for_kind(PointerKind::Mouse).values();
        values.velocity_estimator = estimator;
        values.remainder_handoff = handoffs[index % handoffs.len()];
        values.position_smoothing = Some(OneEuroTuning::new(1.5, 0.25, 2.0).expect("valid tuning"));
        values.max_resolution_delay = Duration::from_nanos(1_234_567_891);
        values.min_target = 31.5;
        let mouse = GestureSettings::try_from_values(values.clone()).expect("valid values");
        let profiles = base.with_kind(PointerKind::Mouse, mouse);
        let header = RecordingHeader::new(RecordingClock::Virtual).with_window(
            RecordedWindow::new(WindowId(3), 1.25).with_gesture_profiles(profiles.clone()),
        );
        let bytes = RecordingWriter::new(Vec::new(), &header)
            .expect("the header writes")
            .finish()
            .expect("flush");
        let read = RecordingReader::new(&bytes[..]).expect("the header reads");
        let read = read.header().windows[0]
            .gesture_profiles
            .as_ref()
            .expect("the window keeps its gesture values");
        assert_eq!(read, &profiles, "{estimator:?}");
        // Against the authored values themselves, not a second conversion of
        // them, so a value the conversion drops cannot drop out of both sides.
        assert_eq!(
            read.for_kind(PointerKind::Mouse).values(),
            values,
            "{estimator:?}"
        );
    }
}
