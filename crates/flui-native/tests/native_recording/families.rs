//! Recordings of the streams beside a pointer registry's (ADR-0215 §4): a
//! loop's application delivery (system notices, session ends, reopens), a
//! window's delivery (its input method's events), a platform's reads of the
//! system (its preferences and displays) and a window's published snapshots,
//! replayed on the headless host.

use std::cell::RefCell;
use std::collections::HashMap;

use flui_native::journal::codec::{
    CodecError, DisplayRead, FailureKind, NativeRecording, PreferenceRead, ReadFailure,
    RecordedWindowState, ReplayError, ReplayTarget, Row, Unsupported,
};
use flui_native::testing::app::{AppCall, AppLog, run_keeping_owner};
use flui_native::{
    HeadlessDisplayControls, HeadlessHostEvents, HeadlessPlatform, HeadlessPreferences, MockWindow,
    OwnerPlatform,
};
use flui_platform::{
    CapabilityAbsence, DisplayId, DisplayKey, DisplayParts, DisplayRegistry, Headroom, ImeEvent,
    MemoryPressureLevel, PlatformInput, SessionEndPhase, SystemNotice, SystemPreferences,
    WindowOptions, WindowState, WindowStateParts,
};
use flui_testing::window_handler::{HandlerCall, RecordingWindowHandler};

/// The checked-in recordings of the streams beside a registry's.
pub(super) const FAMILY_RECORDINGS: [(&str, &str); 4] = [
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
    (
        "headless_window_state",
        include_str!("headless_window_state.jsonl"),
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

/// The ids one replay issues for the display labels of its recordings, shared
/// by every stream it replays: a display a `platform` stream labelled is the
/// display a `window_state` stream of the same session names by that label.
#[derive(Default)]
pub(super) struct Labels {
    registry: DisplayRegistry,
    ids: RefCell<HashMap<DisplayId, u64>>,
}

impl Labels {
    /// The id this replay issued for `label`, issued now on first sight.
    pub(super) fn id(&self, label: u64) -> DisplayId {
        let id = self
            .registry
            .mint(&DisplayKey::Injected(label))
            .expect("a replay's registry has ids for its displays");
        self.ids.borrow_mut().insert(id, label);
        id
    }

    /// The label `id` was issued for.
    pub(super) fn label(&self, id: DisplayId) -> u64 {
        *self
            .ids
            .borrow()
            .get(&id)
            .expect("a replayed display has a label")
    }

    /// How many labels were issued an id.
    pub(super) fn len(&self) -> usize {
        self.ids.borrow().len()
    }
}

/// Replays a platform stream into a headless platform's sources, reading
/// the system back through `owner` after each row, as the application reads
/// it. A recorded display's label is issued an id of the replay's own.
pub(super) struct PlatformTarget<'a> {
    sources: Sources,
    owner: &'a OwnerPlatform,
    labels: &'a Labels,
    pub(super) reads: Vec<Read>,
}

impl<'a> PlatformTarget<'a> {
    pub(super) fn new(sources: Sources, owner: &'a OwnerPlatform, labels: &'a Labels) -> Self {
        Self {
            sources,
            owner,
            labels,
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
                    let id = self.labels.id(display.label);
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
        let labels = self.labels;
        self.reads
            .push(Read::Displays(read_displays(self.owner, |id| {
                labels.label(id)
            })));
        Ok(())
    }
}

/// Replays a `window_state` stream into a headless window: each snapshot is
/// published as the window system's report, on the display the replay
/// issued for its label, and what the window published is kept.
pub(super) struct WindowStateTarget<'a> {
    window: MockWindow,
    labels: &'a Labels,
    pub(super) published: Vec<WindowState>,
}

impl<'a> WindowStateTarget<'a> {
    pub(super) fn new(window: MockWindow, labels: &'a Labels) -> Self {
        Self {
            window,
            labels,
            published: Vec::new(),
        }
    }
}

impl ReplayTarget for WindowStateTarget<'_> {
    fn window_state(&mut self, state: &RecordedWindowState) -> Result<(), Unsupported> {
        let display = state.display().map(|label| self.labels.id(label));
        let sample = state.clone().into_parts(display).into_state();
        self.published.push(self.window.simulate_state(sample));
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
            Row::Displays {
                read: DisplayRead::Observed { displays, .. },
                ..
            } => Some(Read::Displays(Ok(displays
                .iter()
                .map(|display| (display.label, display.parts.clone()))
                .collect()))),
            Row::Displays {
                read: DisplayRead::Failed(failure),
                ..
            } => Some(Read::Displays(Err(failure.clone()))),
            _ => None,
        })
        .collect()
}

/// What a test compares of a published snapshot: the label of its display,
/// as `label` gives it, and its other facts. The revision is the publishing
/// window's own.
pub(super) fn facts(
    state: WindowState,
    label: impl Fn(DisplayId) -> u64,
) -> (Option<u64>, WindowStateParts) {
    let display = state.display.map(label);
    let parts = state.into_parts();
    (
        display,
        WindowStateParts {
            display: None,
            ..parts
        },
    )
}

/// The snapshots a `window_state` recording holds, as [`facts`] compares them.
fn recorded_states(recording: &NativeRecording) -> Vec<(Option<u64>, WindowStateParts)> {
    recording
        .rows
        .iter()
        .filter_map(|row| match row {
            Row::State { state, .. } => Some((state.display(), state.clone().into_parts(None))),
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
    let labels = Labels::default();
    let mut target = PlatformTarget::new(sources, &owner, &labels);
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

    assert_eq!(labels.len(), 2, "one id for each label, kept across reads");
}

/// A window state stream replays on a headless window: each snapshot is
/// published as recorded, every fact the schema carries included, a display
/// label is issued one id of the replay's own, and each change is heard by the
/// handler.
fn window_state_recordings_replay_on_the_headless_host() {
    let recording = family("headless_window_state");
    let owner = run_keeping_owner(Box::new(HeadlessPlatform::new()), &AppLog::new());
    let (window, handler) = headless_window(&owner);
    let labels = Labels::default();
    let mut target = WindowStateTarget::new(window, &labels);
    recording
        .replay_into(&mut target)
        .expect("a window state stream replays on a headless window");
    assert_eq!(
        target
            .published
            .iter()
            .cloned()
            .map(|state| facts(state, |id| labels.label(id)))
            .collect::<Vec<_>>(),
        recorded_states(&recording)
    );
    assert_eq!(labels.len(), 2, "one id for each label, kept across rows");
    let last = target.published.last().expect("snapshots were replayed");
    assert_eq!(last.title, None);
    assert_eq!(
        last.normal_geometry.and_then(|geometry| geometry.position),
        Some(flui_foundation::geometry::Point::new(10.0, 20.0))
    );
    assert_eq!(
        last.capabilities.into_parts().maximizes,
        Err(CapabilityAbsence::NotYetOnThisHost),
        "an absence survives the replay"
    );
    assert_eq!(
        handler.count(&HandlerCall::StateChanged),
        target.published.len(),
        "each snapshot differs from the window's own opening one"
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
    let labels = Labels::default();
    let mut target = PlatformTarget::new(sources, &owner, &labels);
    assert_eq!(
        deferred.replay_into(&mut target),
        Err(ReplayError::Unsupported {
            line: 3,
            row: "preferences"
        }),
        "the headless source cannot defer a read"
    );
    assert_eq!(target.reads.len(), 1, "the read before it was replayed");

    let mut retitled = family("headless_window_state");
    let Row::State { redacted, .. } = &mut retitled.rows[1] else {
        panic!("a window state stream holds snapshots");
    };
    *redacted = true;
    let (window, _) = headless_window(&owner);
    let labels = Labels::default();
    let mut target = WindowStateTarget::new(window, &labels);
    assert_eq!(
        retitled.replay_into(&mut target),
        Err(ReplayError::Redacted {
            line: 3,
            row: "state"
        }),
        "a snapshot whose title was redacted is refused"
    );
    assert_eq!(
        target.published.len(),
        1,
        "the snapshot before it was replayed"
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
        (
            "platform stream with a clock",
            r#"{"flui_native_recording":1,"backend":"web","stream":"platform","epoch_ns":0,"uptime_ns":0,"devices":[]}"#,
        ),
        (
            "window state stream with a clock",
            r#"{"flui_native_recording":1,"backend":"web","stream":"window_state","epoch_ns":0,"uptime_ns":0,"devices":[]}"#,
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
pub(super) const CASES: [(&str, fn()); 6] = [
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
        "window_state_recordings_replay_on_the_headless_host",
        window_state_recordings_replay_on_the_headless_host,
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
    /// What the window published, its first snapshot included.
    states: Vec<WindowState>,
    /// The session's recordings.
    recordings: Vec<NativeRecording>,
}

/// A headless session's application, window and platform streams, captured:
/// the notices, a session end and a reopen the host reported, one
/// composition the window delivered, and the application's reads of the
/// system preferences and the displays, one of each failed; then the
/// window's snapshots as it is resized, loses focus, changes scale and is
/// reported minimized.
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn captured_session(privacy: flui_native::journal::recorder::Privacy) -> Captured {
    use flui_native::DisplayChange;
    use flui_native::journal::codec::Backend;
    use flui_native::journal::recorder::Recorder;
    use flui_platform::{PlatformWindow, WindowMode};

    let recorder = Recorder::with_privacy(Backend::Headless, privacy);
    // Displays the OS identifies, by keys that name the hardware.
    let platform = HeadlessPlatform::new().with_displays(vec![
        flui_native::DisplaySpec::new("Built-in Retina", 1440, 900)
            .with_persistent_key("EDID-0610-A050-SERIAL-C02XL0"),
        flui_native::DisplaySpec::new("Studio Display", 2560, 1440)
            .with_persistent_key("7A1C2E44-5B6D-4F0E-9C3A-2D8E1F4B6A90"),
    ]);
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
    let mut states = vec![window.state()];
    window.simulate_resize(1024.0, 768.0);
    states.push(window.state());
    window.simulate_focus(false);
    states.push(window.state());
    window.simulate_scale_factor_change(2.0);
    states.push(window.state());
    // A report the successor rules rewrite: a minimized window is never
    // occluded, and a report with no drawable area keeps the metrics before it.
    let minimized = window
        .state()
        .with_mode(WindowMode::Minimized)
        .with_occluded(true)
        .with_metrics(None);
    states.push(window.simulate_state(minimized));
    Captured {
        calls: log.calls(),
        ime: ime_events(&handler),
        reads,
        states,
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
    use flui_platform::WindowMode;

    let session = captured_session(Privacy::KeepText);
    let app = only(&session.recordings, Stream::App);
    let window = only(&session.recordings, Stream::Window);
    let reads = only(&session.recordings, Stream::Platform);
    let states = only(&session.recordings, Stream::WindowState);
    for recording in [&app, &window, &reads, &states] {
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
    assert_eq!(
        recorded_states(&states),
        session
            .states
            .iter()
            .cloned()
            .map(|state| facts(state, DisplayId::get))
            .collect::<Vec<_>>(),
        "the window state stream journals every snapshot the window published, its first included"
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
    let labels = Labels::default();
    let mut target = PlatformTarget::new(sources, &owner, &labels);
    reads
        .replay_into(&mut target)
        .expect("the platform stream replays");
    assert_eq!(target.reads, session.reads);

    // The window's snapshots replay on a window of the replayed platform,
    // each through the successor rules the live window published it by, and
    // on the display the platform stream's replay issued for its label.
    let (mock, replayed) = headless_window(&owner);
    let mut target = WindowStateTarget::new(mock, &labels);
    states
        .replay_into(&mut target)
        .expect("the window state stream replays");
    assert_eq!(
        target
            .published
            .iter()
            .cloned()
            .map(|state| facts(state, |id| labels.label(id)))
            .collect::<Vec<_>>(),
        recorded_states(&states),
        "a replayed snapshot publishes what the live window published"
    );
    assert_eq!(
        target
            .published
            .iter()
            .map(|state| state.revision.get())
            .collect::<Vec<_>>(),
        states
            .rows
            .iter()
            .filter_map(|row| match row {
                Row::State { state, .. } => Some(state.revision()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        "the replaying window earns the revisions the live one did"
    );
    assert_eq!(
        replayed.count(&HandlerCall::StateChanged),
        target.published.len() - 1,
        "the handler heard each change; the first snapshot is the one the window opened with"
    );
    let primary = owner
        .display_snapshot()
        .expect("the replayed displays read")
        .primary()
        .map(|display| display.id);
    let last = target.published.last().expect("snapshots were replayed");
    assert_eq!(
        last.display, primary,
        "one label registry for the streams of a replay"
    );
    assert_eq!(last.mode, WindowMode::Minimized);
    assert!(!last.occluded, "a minimized window is not occluded");
    assert_eq!(
        last.metrics.map(|metrics| metrics.scale().get()),
        Some(2.0),
        "a report with no drawable area keeps the metrics before it"
    );

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
    let states = only(&redacted.recordings, Stream::WindowState);
    assert!(
        states
            .rows
            .iter()
            .all(|row| matches!(row, Row::State { redacted: true, state } if state.title() == Some("\u{fffd}"))),
        "every snapshot lost its title"
    );
    let (mock, _) = headless_window(&owner);
    assert_eq!(
        states.replay_into(&mut WindowStateTarget::new(mock, &labels)),
        Err(ReplayError::Redacted {
            line: 2,
            row: "state"
        })
    );

    // A redacting capture writes no display's name or persistent key: each
    // key becomes a pseudonym counted within the recording, the same for the
    // same display on every read, and the row still replays.
    let reads = only(&redacted.recordings, Stream::Platform);
    let mut written = Vec::new();
    reads
        .write(&mut written)
        .expect("a captured recording writes");
    let written = String::from_utf8(written).expect("JSON Lines are UTF-8");
    for secret in [
        "Built-in Retina",
        "Studio Display",
        "EDID-0610-A050-SERIAL-C02XL0",
        "7A1C2E44-5B6D-4F0E-9C3A-2D8E1F4B6A90",
    ] {
        assert!(!written.contains(secret), "{secret} reached the recording");
    }
    let observed: Vec<Vec<(String, Option<String>)>> = reads
        .rows
        .iter()
        .filter_map(|row| match row {
            Row::Displays {
                read: DisplayRead::Observed { displays, .. },
                redacted,
            } => {
                assert!(redacted, "a redacted read is marked");
                Some(
                    displays
                        .iter()
                        .map(|display| {
                            (
                                display.parts.name.clone(),
                                display.parts.persistent_key.clone(),
                            )
                        })
                        .collect(),
                )
            }
            _ => None,
        })
        .collect();
    let pseudonymous = vec![
        ("\u{fffd}".to_owned(), Some("display-1".to_owned())),
        ("\u{fffd}".to_owned(), Some("display-2".to_owned())),
    ];
    assert_eq!(observed, vec![pseudonymous; 3]);
    let platform = HeadlessPlatform::new();
    let sources = Sources::of(&platform);
    let owner = run_keeping_owner(Box::new(platform), &AppLog::new());
    let labels = Labels::default();
    let mut target = PlatformTarget::new(sources, &owner, &labels);
    reads
        .replay_into(&mut target)
        .expect("a redacted display read replays");
    assert_eq!(
        target.reads.len(),
        redacted.reads.len(),
        "every read replayed"
    );
}

/// The capture cases of the streams beside a registry's.
#[cfg(all(debug_assertions, feature = "native-capture"))]
pub(super) const CAPTURE_CASES: [(&str, fn()); 1] = [(
    "captured_streams_replay_to_what_the_session_delivered",
    captured_streams_replay_to_what_the_session_delivered,
)];
