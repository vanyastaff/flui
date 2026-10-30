//! Recordings of the streams beside a pointer registry's (ADR-0215 §4): a
//! loop's application delivery (system notices, session ends, reopens) and a
//! window's delivery (its input method's events), replayed on the headless
//! host.

use flui_native::journal::codec::{
    CodecError, NativeRecording, ReplayError, ReplayTarget, Row, Unsupported,
};
use flui_native::testing::app::{AppCall, AppLog, run_keeping_owner};
use flui_native::{HeadlessHostEvents, HeadlessPlatform, MockWindow};
use flui_platform::{
    ImeEvent, MemoryPressureLevel, PlatformInput, SessionEndPhase, SystemNotice, WindowOptions,
};
use flui_testing::window_handler::RecordingWindowHandler;

/// The checked-in recordings of the streams beside a registry's.
pub(super) const FAMILY_RECORDINGS: [(&str, &str); 2] = [
    (
        "headless_app_notices",
        include_str!("headless_app_notices.jsonl"),
    ),
    (
        "headless_window_ime",
        include_str!("headless_window_ime.jsonl"),
    ),
];

/// Replays an application stream into a headless host's events.
pub(super) struct HostEventsTarget(pub(super) HeadlessHostEvents);

impl ReplayTarget for HostEventsTarget {
    fn notice(&mut self, notice: SystemNotice) -> Result<(), Unsupported> {
        self.0.notice(notice);
        Ok(())
    }

    fn session_end(&mut self, phase: SessionEndPhase) -> Result<(), Unsupported> {
        let _answer = self.0.session_end(phase);
        Ok(())
    }

    fn reopen(&mut self) -> Result<(), Unsupported> {
        self.0.reopen();
        Ok(())
    }
}

/// Replays a window stream into a headless window.
pub(super) struct WindowTarget(pub(super) MockWindow);

impl ReplayTarget for WindowTarget {
    fn ime(&mut self, event: &ImeEvent) -> Result<(), Unsupported> {
        self.0.inject_event(PlatformInput::Ime(event.clone()));
        Ok(())
    }
}

/// A headless window, opened on `owner` with a recording handler.
pub(super) fn headless_window(
    owner: &flui_native::OwnerPlatform,
) -> (MockWindow, std::rc::Rc<RecordingWindowHandler>) {
    let handler = RecordingWindowHandler::new();
    let window = owner
        .open_window(WindowOptions::default(), handler.clone())
        .expect("a headless window opens")
        .try_ready()
        .expect("at once");
    let mock = window
        .as_any()
        .downcast_ref::<MockWindow>()
        .expect("a headless window")
        .clone();
    (mock, handler)
}

/// The input method events `handler` heard, in order.
pub(super) fn ime_events(handler: &RecordingWindowHandler) -> Vec<ImeEvent> {
    handler
        .inputs()
        .into_iter()
        .filter_map(|input| input.as_ime().cloned())
        .collect()
}

fn family(name: &str) -> NativeRecording {
    let (_, text) = FAMILY_RECORDINGS
        .iter()
        .find(|(found, _)| *found == name)
        .expect("a checked-in family recording");
    NativeRecording::read(text.as_bytes()).expect("a checked-in recording reads")
}

/// An application stream replays on the headless host to its handler, every
/// row in order: the notices, both phases of a cancelled session end and the
/// reopen.
fn app_recordings_replay_on_the_headless_host() {
    let platform = HeadlessPlatform::new();
    let events = platform.host_events();
    let log = AppLog::new();
    let _owner = run_keeping_owner(Box::new(platform), &log);
    family("headless_app_notices")
        .replay_into(&mut HostEventsTarget(events))
        .expect("an application stream replays on the headless host");
    assert_eq!(
        log.calls(),
        [
            AppCall::Ready,
            AppCall::Notice(SystemNotice::Sleep),
            AppCall::Notice(SystemNotice::Wake),
            AppCall::Notice(SystemNotice::MemoryPressure(MemoryPressureLevel::Critical)),
            AppCall::SessionEnd(SessionEndPhase::Query),
            AppCall::SessionEnd(SessionEndPhase::Cancelled),
            AppCall::Reopen,
            AppCall::Notice(SystemNotice::SessionLocked),
        ]
    );
}

/// A window stream replays on a headless window to its handler: one
/// composition, its text and cursor as recorded.
fn window_recordings_replay_on_the_headless_host() {
    let log = AppLog::new();
    let owner = run_keeping_owner(Box::new(HeadlessPlatform::new()), &log);
    let (window, handler) = headless_window(&owner);
    family("headless_window_ime")
        .replay_into(&mut WindowTarget(window))
        .expect("a window stream replays on a headless window");
    assert_eq!(
        ime_events(&handler),
        [
            ImeEvent::Enabled,
            ImeEvent::Preedit {
                text: "に".to_owned(),
                cursor: Some((0, 3)),
            },
            ImeEvent::Commit("日本".to_owned()),
            ImeEvent::Disabled,
        ]
    );
}

/// A row the replay does not reproduce stops it with an error naming the
/// row and its line, never a skipped row: an application stream replayed as
/// a registry's, a registry's into a host's events, and a redacted input
/// method row.
fn a_row_the_replay_does_not_reproduce_is_a_typed_error() {
    assert_eq!(
        family("headless_app_notices").replay().err(),
        Some(ReplayError::Unsupported {
            line: 2,
            row: "notice",
        })
    );
    let pointer = NativeRecording::read(super::RECORDINGS[0].1.as_bytes())
        .expect("a checked-in recording reads");
    let first = pointer.rows[0].name();
    assert_eq!(
        pointer.replay_into(&mut HostEventsTarget(HeadlessPlatform::new().host_events())),
        Err(ReplayError::Unsupported {
            line: 2,
            row: first,
        })
    );
    let mut redacted = family("headless_window_ime");
    redacted.rows[2] = Row::Ime {
        event: ImeEvent::Commit("\u{fffd}".to_owned()),
        redacted: true,
    };
    let log = AppLog::new();
    let owner = run_keeping_owner(Box::new(HeadlessPlatform::new()), &log);
    let (window, handler) = headless_window(&owner);
    assert_eq!(
        redacted.replay_into(&mut WindowTarget(window)),
        Err(ReplayError::Redacted {
            line: 4,
            row: "ime"
        })
    );
    assert_eq!(
        ime_events(&handler).len(),
        2,
        "the rows before the redacted one were replayed, none after it"
    );
}

/// A registry's recording carries its clock and no other stream does: a
/// header that breaks either is refused at line 1.
fn a_header_out_of_its_stream_is_refused() {
    for (case, header) in [
        (
            "registry without a clock",
            r#"{"flui_native_recording":1,"backend":"web","devices":[]}"#,
        ),
        (
            "application stream with a clock",
            r#"{"flui_native_recording":1,"backend":"web","stream":"app","epoch_ns":0,"uptime_ns":0,"devices":[]}"#,
        ),
    ] {
        assert!(
            matches!(
                NativeRecording::read(format!("{header}\n").as_bytes()),
                Err(CodecError::Invalid { line: 1, .. })
            ),
            "{case}"
        );
    }
}

/// The cases of the streams beside a registry's.
pub(super) const CASES: [(&str, fn()); 4] = [
    (
        "app_recordings_replay_on_the_headless_host",
        app_recordings_replay_on_the_headless_host,
    ),
    (
        "window_recordings_replay_on_the_headless_host",
        window_recordings_replay_on_the_headless_host,
    ),
    (
        "a_row_the_replay_does_not_reproduce_is_a_typed_error",
        a_row_the_replay_does_not_reproduce_is_a_typed_error,
    ),
    (
        "a_header_out_of_its_stream_is_refused",
        a_header_out_of_its_stream_is_refused,
    ),
];

/// A headless session's application and window streams, captured: the
/// notices, a session end and a reopen the host reported, and one
/// composition the window delivered. Returns the application's calls, the
/// window's input method events and the recordings.
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn captured_session(
    privacy: flui_native::journal::recorder::Privacy,
) -> (Vec<AppCall>, Vec<ImeEvent>, Vec<NativeRecording>) {
    use flui_native::journal::codec::Backend;
    use flui_native::journal::recorder::Recorder;

    let recorder = Recorder::with_privacy(Backend::Headless, privacy);
    let platform = HeadlessPlatform::new();
    let events = platform.host_events();
    let log = AppLog::new();
    let owner = run_keeping_owner(Box::new(platform), &log);
    let (window, handler) = headless_window(&owner);
    events.notice(SystemNotice::Sleep);
    events.notice(SystemNotice::Wake);
    let _answer = events.session_end(SessionEndPhase::Query);
    events.reopen();
    for event in [
        ImeEvent::Enabled,
        ImeEvent::Preedit {
            text: "か".to_owned(),
            cursor: Some((3, 3)),
        },
        ImeEvent::Commit("漢".to_owned()),
        ImeEvent::Disabled,
    ] {
        window.inject_event(PlatformInput::Ime(event));
    }
    let recordings = recorder.finish();
    (log.calls(), ime_events(&handler), recordings)
}

/// The one recording of `stream` among `recordings`.
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn only(
    recordings: &[NativeRecording],
    stream: flui_native::journal::codec::Stream,
) -> NativeRecording {
    let mut found = recordings
        .iter()
        .filter(|recording| recording.header.stream == stream);
    let recording = found.next().expect("the session journals the stream");
    assert!(found.next().is_none(), "the session opened one {stream:?}");
    recording.clone()
}

/// A captured application stream and window stream write and read back
/// unchanged, and replay on a fresh headless host to exactly what the
/// session delivered; a capture that redacts text marks the rows that lost
/// it, which their replay refuses.
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn captured_streams_replay_to_what_the_session_delivered() {
    use flui_native::journal::codec::Stream;
    use flui_native::journal::recorder::Privacy;

    let (calls, ime, recordings) = captured_session(Privacy::KeepText);
    let app = only(&recordings, Stream::App);
    let window = only(&recordings, Stream::Window);
    for recording in [&app, &window] {
        let mut written = Vec::new();
        recording
            .write(&mut written)
            .expect("a captured recording writes");
        assert_eq!(
            &NativeRecording::read(written.as_slice()).expect("it reads back"),
            recording
        );
    }

    let platform = HeadlessPlatform::new();
    let events = platform.host_events();
    let log = AppLog::new();
    let owner = run_keeping_owner(Box::new(platform), &log);
    app.replay_into(&mut HostEventsTarget(events))
        .expect("the application stream replays");
    assert_eq!(log.calls(), calls);
    let (mock, handler) = headless_window(&owner);
    window
        .replay_into(&mut WindowTarget(mock))
        .expect("the window stream replays");
    assert_eq!(ime_events(&handler), ime);

    let (_, _, redacted) = captured_session(Privacy::Redact);
    let window = only(&redacted, Stream::Window);
    let lost: Vec<usize> = window
        .rows
        .iter()
        .enumerate()
        .filter(|(_, row)| matches!(row, Row::Ime { redacted: true, .. }))
        .map(|(index, _)| index)
        .collect();
    assert_eq!(lost, [1, 2], "the preedit and the commit lost their text");
    let (mock, _) = headless_window(&owner);
    assert_eq!(
        window.replay_into(&mut WindowTarget(mock)),
        Err(ReplayError::Redacted {
            line: 3,
            row: "ime"
        })
    );
}

/// The capture cases of the streams beside a registry's.
#[cfg(all(debug_assertions, feature = "native-capture"))]
pub(super) const CAPTURE_CASES: [(&str, fn()); 1] = [(
    "captured_streams_replay_to_what_the_session_delivered",
    captured_streams_replay_to_what_the_session_delivered,
)];
