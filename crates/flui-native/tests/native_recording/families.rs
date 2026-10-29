//! Recordings of the streams beside a pointer registry's (ADR-0215 §4): a
//! loop's application delivery (system notices, session ends, reopens), a
//! window's delivery (its input method's events) and a platform's reads of the
//! system (its preferences and displays), replayed on the headless host.

use std::collections::HashMap;

use flui_native::journal::codec::{
    CodecError, DisplayRead, FailureKind, NativeRecording, PreferenceRead, ReadFailure,
    ReplayError, ReplayTarget, Row, Unsupported,
};
use flui_native::testing::app::{AppCall, AppLog, run_keeping_owner};
use flui_native::{
    HeadlessDisplayControls, HeadlessHostEvents, HeadlessPlatform, HeadlessPreferences, MockWindow,
    OwnerPlatform,
};
use flui_platform::{
    CapabilityAbsence, DisplayId, DisplayKey, DisplayParts, DisplayRegistry, Headroom, ImeEvent,
    MemoryPressureLevel, PlatformInput, SessionEndPhase, SystemNotice, SystemPreferences,
    WindowOptions,
};
use flui_testing::window_handler::RecordingWindowHandler;

/// The checked-in recordings of the streams beside a registry's.
pub(super) const FAMILY_RECORDINGS: [(&str, &str); 3] = [
    (
        "headless_app_notices",
        include_str!("headless_app_notices.jsonl"),
    ),
    (
        "headless_window_ime",
        include_str!("headless_window_ime.jsonl"),
    ),
    (
        "headless_platform_reads",
        include_str!("headless_platform_reads.jsonl"),
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

/// One read of the system the application made, as a test compares it: a
/// display by its label within the recording and its fields, a failure by
/// the kind of error it was.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Read {
    /// A read of the system preferences.
    Preferences(Result<SystemPreferences, ReadFailure>),
    /// A read of the displays.
    Displays(Result<Vec<(u64, DisplayParts)>, ReadFailure>),
}

/// The headless platform's system sources, taken before `run` consumes it.
pub(super) struct Sources {
    preferences: HeadlessPreferences,
    displays: HeadlessDisplayControls,
}

impl Sources {
    pub(super) fn of(platform: &HeadlessPlatform) -> Self {
        Self {
            preferences: platform.preference_source(),
            displays: platform.display_controls(),
        }
    }
}

/// Replays a platform stream into a headless platform's sources, reading
/// the system back through `owner` after each row, as the application reads
/// it. A recorded display's label is issued an id of this replay's own.
pub(super) struct PlatformTarget<'a> {
    sources: Sources,
    owner: &'a OwnerPlatform,
    registry: DisplayRegistry,
    labels: HashMap<DisplayId, u64>,
    pub(super) reads: Vec<Read>,
}

impl<'a> PlatformTarget<'a> {
    pub(super) fn new(sources: Sources, owner: &'a OwnerPlatform) -> Self {
        Self {
            sources,
            owner,
            registry: DisplayRegistry::new(),
            labels: HashMap::new(),
            reads: Vec::new(),
        }
    }
}

impl ReplayTarget for PlatformTarget<'_> {
    fn preferences(&mut self, read: &PreferenceRead) -> Result<(), Unsupported> {
        match read {
            PreferenceRead::Observed(values) => self.sources.preferences.set(values.clone()),
            PreferenceRead::Failed(ReadFailure {
                kind: FailureKind::Preferences,
                message,
            }) => self
                .sources
                .preferences
                .fail_next_read(message.clone().unwrap_or_default()),
            // The headless source fails a read only as a preference source does.
            _ => return Err(Unsupported),
        }
        self.reads
            .push(Read::Preferences(read_preferences(self.owner)));
        Ok(())
    }

    fn displays(&mut self, read: &DisplayRead) -> Result<(), Unsupported> {
        match read {
            DisplayRead::Observed { displays, .. } => {
                let mut infos = Vec::with_capacity(displays.len());
                for display in displays {
                    let id = self
                        .registry
                        .mint(&DisplayKey::Injected(display.label))
                        .expect("a replay's registry has ids for its displays");
                    self.labels.insert(id, display.label);
                    infos.push(display.parts.clone().into_info(id));
                }
                self.sources
                    .displays
                    .replace(infos)
                    .expect("the headless snapshot advances");
            }
            DisplayRead::Failed(ReadFailure {
                kind: FailureKind::Displays,
                message,
            }) => self
                .sources
                .displays
                .fail_next_read(message.clone().unwrap_or_default()),
            // The headless source fails a read only as a display source does.
            _ => return Err(Unsupported),
        }
        let labels = &self.labels;
        self.reads
            .push(Read::Displays(read_displays(self.owner, |id| {
                *labels.get(&id).expect("a replayed display has a label")
            })));
        Ok(())
    }
}

/// What the application reads of the system preferences through `owner`.
pub(super) fn read_preferences(owner: &OwnerPlatform) -> Result<SystemPreferences, ReadFailure> {
    owner.preferences().map_err(|error| ReadFailure::of(&error))
}

/// What the application reads of the displays through `owner`, each display
/// under the label `label` gives its id.
pub(super) fn read_displays(
    owner: &OwnerPlatform,
    label: impl Fn(DisplayId) -> u64,
) -> Result<Vec<(u64, DisplayParts)>, ReadFailure> {
    let snapshot = owner
        .display_snapshot()
        .map_err(|error| ReadFailure::of(&error))?;
    Ok(snapshot
        .displays()
        .iter()
        .cloned()
        .map(|display| {
            let (id, parts) = display.into_parts();
            (label(id), parts)
        })
        .collect())
}

/// What each read row of `recording` says the read returned.
fn recorded_reads(recording: &NativeRecording) -> Vec<Read> {
    recording
        .rows
        .iter()
        .filter_map(|row| match row {
            Row::Preferences(PreferenceRead::Observed(values)) => {
                Some(Read::Preferences(Ok(values.clone())))
            }
            Row::Preferences(PreferenceRead::Failed(failure)) => {
                Some(Read::Preferences(Err(failure.clone())))
            }
            Row::Displays(DisplayRead::Observed { displays, .. }) => {
                Some(Read::Displays(Ok(displays
                    .iter()
                    .map(|display| (display.label, display.parts.clone()))
                    .collect())))
            }
            Row::Displays(DisplayRead::Failed(failure)) => {
                Some(Read::Displays(Err(failure.clone())))
            }
            _ => None,
        })
        .collect()
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

/// A platform stream replays on the headless host: after each row the
/// application reads what the recorded read returned, a failure included,
/// with every preference field the full observation carries and each display
/// under one id of the replay's own for its label.
fn platform_recordings_replay_on_the_headless_host() {
    let recording = family("headless_platform_reads");
    let platform = HeadlessPlatform::new();
    let sources = Sources::of(&platform);
    let log = AppLog::new();
    let owner = run_keeping_owner(Box::new(platform), &log);
    let mut target = PlatformTarget::new(sources, &owner);
    recording
        .replay_into(&mut target)
        .expect("a platform stream replays on the headless host");
    let expected = recorded_reads(&recording);
    assert_eq!(expected.len(), 6, "every row is a read");
    assert_eq!(target.reads, expected);
    let Read::Preferences(Ok(full)) = &target.reads[0] else {
        panic!("the first read observed the preferences");
    };
    assert_eq!(full.text_scale(), Some(1.25));
    assert_eq!(
        full.locales().map(<[flui_platform::Locale]>::len),
        Some(2),
        "the locales survive the read"
    );
    assert_eq!(full.wheel().horizontal_characters(), Some(2));
    assert_eq!(
        target.reads[1],
        Read::Preferences(Err(ReadFailure {
            kind: FailureKind::Preferences,
            message: Some("the settings portal did not answer".to_owned()),
        }))
    );
    let Read::Displays(Ok(displays)) = &target.reads[3] else {
        panic!("the first display read observed the displays");
    };
    let built_in = &displays[0].1;
    assert!(built_in.is_primary);
    assert_eq!(
        built_in.color.map(|color| color.headroom),
        Ok(Ok(Headroom::try_new(2.5).expect("finite, at least 1"))),
        "the headroom survives the read"
    );
    assert_eq!(
        displays[1].1.color,
        Err(CapabilityAbsence::NotYetOnThisHost)
    );
    assert!(
        matches!(&target.reads[4], Read::Displays(Err(failure)) if failure.kind == FailureKind::Displays),
        "the failed display read fails as a display source does"
    );

    assert_eq!(
        target.labels.len(),
        2,
        "one id for each label, kept across reads"
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

    let mut deferred = family("headless_platform_reads");
    deferred.rows[1] = Row::Preferences(PreferenceRead::Failed(ReadFailure {
        kind: FailureKind::Deferred,
        message: None,
    }));
    let platform = HeadlessPlatform::new();
    let sources = Sources::of(&platform);
    let owner = run_keeping_owner(Box::new(platform), &AppLog::new());
    let mut target = PlatformTarget::new(sources, &owner);
    assert_eq!(
        deferred.replay_into(&mut target),
        Err(ReplayError::Unsupported {
            line: 3,
            row: "preferences"
        }),
        "the headless source cannot defer a read"
    );
    assert_eq!(target.reads.len(), 1, "the read before it was replayed");
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
        (
            "platform stream with a clock",
            r#"{"flui_native_recording":1,"backend":"web","stream":"platform","epoch_ns":0,"uptime_ns":0,"devices":[]}"#,
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
pub(super) const CASES: [(&str, fn()); 5] = [
    (
        "app_recordings_replay_on_the_headless_host",
        app_recordings_replay_on_the_headless_host,
    ),
    (
        "window_recordings_replay_on_the_headless_host",
        window_recordings_replay_on_the_headless_host,
    ),
    (
        "platform_recordings_replay_on_the_headless_host",
        platform_recordings_replay_on_the_headless_host,
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

/// What a captured headless session delivered, and its recordings.
#[cfg(all(debug_assertions, feature = "native-capture"))]
struct Captured {
    /// The application's calls.
    calls: Vec<AppCall>,
    /// The window's input method events.
    ime: Vec<ImeEvent>,
    /// What the application read of the system.
    reads: Vec<Read>,
    /// The session's recordings.
    recordings: Vec<NativeRecording>,
}

/// A headless session's application, window and platform streams, captured:
/// the notices, a session end and a reopen the host reported, one
/// composition the window delivered, and the application's reads of the
/// system preferences and the displays, one of each failed.
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn captured_session(privacy: flui_native::journal::recorder::Privacy) -> Captured {
    use flui_native::DisplayChange;
    use flui_native::journal::codec::Backend;
    use flui_native::journal::recorder::Recorder;

    let recorder = Recorder::with_privacy(Backend::Headless, privacy);
    let platform = HeadlessPlatform::new();
    let events = platform.host_events();
    let sources = Sources::of(&platform);
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
    let preferences = || Read::Preferences(read_preferences(&owner));
    let displays = || Read::Displays(read_displays(&owner, DisplayId::get));
    let mut reads = vec![preferences(), displays()];
    sources.preferences.set(
        SystemPreferences::default()
            .with_high_contrast(true)
            .with_text_scale(2.0)
            .expect("in range"),
    );
    reads.push(preferences());
    sources
        .preferences
        .fail_next_read("the settings service is gone");
    reads.push(preferences());
    reads.push(preferences());
    let primary = sources
        .displays
        .snapshot()
        .and_then(|snapshot| snapshot.primary().map(|display| display.id))
        .expect("the headless platform has a primary display");
    sources
        .displays
        .simulate_display_change(primary, DisplayChange::ScaleFactor(2.0))
        .expect("the primary display changes");
    reads.push(displays());
    sources
        .displays
        .fail_next_read("the display configuration changed while it was read");
    reads.push(displays());
    reads.push(displays());
    Captured {
        calls: log.calls(),
        ime: ime_events(&handler),
        reads,
        recordings: recorder.finish(),
    }
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

/// Captured application, window and platform streams write and read back
/// unchanged, and replay on a fresh headless host to exactly what the session
/// delivered and read; a capture that redacts text marks the rows that lost
/// it, which their replay refuses.
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn captured_streams_replay_to_what_the_session_delivered() {
    use flui_native::journal::codec::Stream;
    use flui_native::journal::recorder::Privacy;

    let session = captured_session(Privacy::KeepText);
    let app = only(&session.recordings, Stream::App);
    let window = only(&session.recordings, Stream::Window);
    let reads = only(&session.recordings, Stream::Platform);
    for recording in [&app, &window, &reads] {
        let mut written = Vec::new();
        recording
            .write(&mut written)
            .expect("a captured recording writes");
        assert_eq!(
            &NativeRecording::read(written.as_slice()).expect("it reads back"),
            recording
        );
    }
    assert_eq!(
        recorded_reads(&reads),
        session.reads,
        "the platform stream journals every read the application made, and only those"
    );

    let platform = HeadlessPlatform::new();
    let events = platform.host_events();
    let sources = Sources::of(&platform);
    let log = AppLog::new();
    let owner = run_keeping_owner(Box::new(platform), &log);
    app.replay_into(&mut HostEventsTarget(events))
        .expect("the application stream replays");
    assert_eq!(log.calls(), session.calls);
    let (mock, handler) = headless_window(&owner);
    window
        .replay_into(&mut WindowTarget(mock))
        .expect("the window stream replays");
    assert_eq!(ime_events(&handler), session.ime);
    let mut target = PlatformTarget::new(sources, &owner);
    reads
        .replay_into(&mut target)
        .expect("the platform stream replays");
    assert_eq!(target.reads, session.reads);

    let redacted = captured_session(Privacy::Redact);
    let window = only(&redacted.recordings, Stream::Window);
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
