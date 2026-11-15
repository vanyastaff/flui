//! Recordings of the streams beside a pointer registry's (ADR-0215 §4): a
//! loop's application delivery (system notices, session ends, reopens), a
//! window's delivery (its input method's events), a platform's reads of the
//! system (its preferences and displays) and a window's published snapshots,
//! replayed on the headless host.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use flui_native::journal::codec::{
    CloseCause, CodecError, DisplayRead, FailureKind, NativeRecording, PreferenceRead, ReadFailure,
    RecordedWindowState, ReplayError, ReplayTarget, Row, Unsupported,
};
use flui_native::journal::store::{RecordedStore, StoreFocus};
use flui_native::testing::app::{AppCall, AppLog, run_keeping_owner};
use flui_native::{
    HeadlessDisplayControls, HeadlessHostEvents, HeadlessHostWindow, HeadlessPlatform,
    HeadlessPreferences, OwnerPlatform,
};
use flui_platform::text_store::TextStore;
use flui_platform::{
    CapabilityAbsence, DisplayId, DisplayKey, DisplayParts, DisplayRegistry, Headroom, ImeEvent,
    MemoryPressureLevel, PlatformInput, SessionEndPhase, SystemNotice, SystemPreferences,
    WindowOptions, WindowState, WindowStateParts,
};
use flui_platform::{CloseDecision, WindowCommand};
use flui_testing::text_store_kit::InMemoryTextStore;
use flui_testing::window_commands::AdmitAndApply as _;
use flui_testing::window_handler::{HandlerCall, RecordingWindowHandler};

/// The checked-in recordings of the streams beside a registry's.
pub(super) const FAMILY_RECORDINGS: [(&str, &str); 5] = [
    (
        "headless_window_store_and_close",
        include_str!("headless_window_store_and_close.jsonl"),
    ),
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

/// Replays a window stream into a headless window: its input method's
/// events, its close requests and close, and the locks on each text store it
/// served, replayed on a store of the target's own per recorded store.
pub(super) struct WindowTarget {
    window: HeadlessHostWindow,
    pub(super) stores: HashMap<RecordedStore, Rc<InMemoryTextStore>>,
}

impl WindowTarget {
    pub(super) fn new(window: HeadlessHostWindow) -> Self {
        Self {
            window,
            stores: HashMap::new(),
        }
    }
}

impl ReplayTarget for WindowTarget {
    fn ime(&mut self, event: &ImeEvent) -> Result<(), Unsupported> {
        self.window.inject_event(PlatformInput::Ime(event.clone()));
        Ok(())
    }

    fn store_focus(&mut self, focus: Option<&StoreFocus>) -> Result<(), Unsupported> {
        if let Some(focus) = focus {
            let store = InMemoryTextStore::new("");
            store.set_protected(focus.status.protected);
            store.set_purpose(focus.status.purpose);
            self.stores.insert(focus.store, store);
        }
        Ok(())
    }

    fn text_store(&mut self, store: RecordedStore) -> Option<Rc<dyn TextStore>> {
        self.stores
            .get(&store)
            .map(|store| Rc::clone(store) as Rc<dyn TextStore>)
    }

    fn close_request(&mut self) -> Result<CloseDecision, Unsupported> {
        Ok(if self.window.simulate_close() {
            CloseDecision::Close
        } else {
            CloseDecision::KeepOpen
        })
    }

    fn closed(&mut self, cause: CloseCause) -> Result<(), Unsupported> {
        if cause == CloseCause::NoRequest {
            self.window
                .admit_and_apply(WindowCommand::Close)
                .map_err(|_| Unsupported)?;
        }
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
/// issued for its label, and what a reader of the window then sees is kept.
pub(super) struct WindowStateTarget<'a> {
    window: HeadlessHostWindow,
    labels: &'a Labels,
    /// The window's state as a consumer reads it after each replayed
    /// snapshot (`PlatformWindow::state`), which is what the application's
    /// `state_changed` route reads.
    pub(super) published: Vec<WindowState>,
}

impl<'a> WindowStateTarget<'a> {
    pub(super) fn new(window: HeadlessHostWindow, labels: &'a Labels) -> Self {
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
        let reported = self.window.simulate_state(sample);
        // A consumer is told of the change and reads the window: the read
        // answers the snapshot the replay reported, and replaces nothing.
        let read = flui_platform::PlatformWindow::state(&self.window);
        assert_eq!(
            read, reported,
            "a read of the window answers the snapshot its window system reported"
        );
        self.published.push(read);
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
) -> (HeadlessHostWindow, std::rc::Rc<RecordingWindowHandler>) {
    let handler = RecordingWindowHandler::new();
    let window = owner
        .open_window(WindowOptions::default(), handler.clone())
        .expect("a headless window opens")
        .try_ready()
        .expect("at once");
    let mock = window
        .as_any()
        .downcast_ref::<HeadlessHostWindow>()
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
        .replay_into(&mut WindowTarget::new(window))
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

/// A handler that keeps its window at the first close request and grants
/// every later one, and how many it was asked.
fn veto_once(handler: &RecordingWindowHandler) -> Rc<std::cell::Cell<usize>> {
    let asked = Rc::new(std::cell::Cell::new(0));
    let count = Rc::clone(&asked);
    handler.react_to_close_request(move || {
        count.set(count.get() + 1);
        if count.get() == 1 {
            CloseDecision::KeepOpen
        } else {
            CloseDecision::Close
        }
    });
    asked
}

/// The store a replay made for the first store its recording names.
fn first_store(target: &WindowTarget) -> Rc<InMemoryTextStore> {
    Rc::clone(&target.stores[&RecordedStore::from_raw(std::num::NonZeroU64::MIN)])
}

/// A window stream's text store locks replay on a store of the replay's
/// own, which starts from the document the first lock found and ends as the
/// input method left it, every read answering as recorded; its close requests
/// replay to the same decisions, a vetoed one keeping the window and a
/// granted one closing it.
fn window_store_and_close_recordings_replay_on_the_headless_host() {
    use flui_platform::text_store::{Selection, Utf16Offset};

    let owner = run_keeping_owner(Box::new(HeadlessPlatform::new()), &AppLog::new());
    let (window, handler) = headless_window(&owner);
    let asked = veto_once(&handler);
    let mut target = WindowTarget::new(window);
    family("headless_window_store_and_close")
        .replay_into(&mut target)
        .expect("a window stream with store locks and close requests replays");
    let store = first_store(&target);
    assert_eq!(store.text(), "ab漢");
    assert_eq!(store.selection(), Selection::collapsed(Utf16Offset::new(3)));
    assert_eq!(store.composition(), None);
    assert_eq!(asked.get(), 2, "both requests were asked");
    assert_eq!(
        handler.count(&HandlerCall::Closed),
        1,
        "the granted one closed"
    );
}

/// A row the replaying host reproduces otherwise is a typed divergence that
/// names the row: a close request its handler decides otherwise, and a store
/// lock whose edit answers otherwise; the rows after it are not replayed.
fn a_row_replayed_otherwise_is_a_divergence() {
    use flui_native::journal::codec::Divergence;
    use flui_native::journal::store::{StoreDivergence, StoreStep};

    let owner = run_keeping_owner(Box::new(HeadlessPlatform::new()), &AppLog::new());
    let (window, handler) = headless_window(&owner);
    handler.react_to_close_request(|| CloseDecision::Close);
    assert_eq!(
        family("headless_window_store_and_close").replay_into(&mut WindowTarget::new(window)),
        Err(ReplayError::Diverged {
            line: 7,
            row: "close_request",
            divergence: Divergence::CloseDecision {
                recorded: CloseDecision::KeepOpen,
                replayed: CloseDecision::Close,
            },
        })
    );
    assert_eq!(
        handler.count(&HandlerCall::CloseRequested),
        1,
        "the request after the divergent one was not asked"
    );

    let mut edited = family("headless_window_store_and_close");
    let Row::StoreLock { lock, .. } = &mut edited.rows[3] else {
        panic!("the fourth row is the composition's last lock");
    };
    let StoreStep::Replace { text, .. } = &mut lock.steps[0] else {
        panic!("the lock begins with its replacement");
    };
    // The recording claims the store answered for "漢" while the replay now
    // writes two units.
    "漢字".clone_into(text);
    let (window, _) = headless_window(&owner);
    assert_eq!(
        edited.replay_into(&mut WindowTarget::new(window)),
        Err(ReplayError::Diverged {
            line: 5,
            row: "store_lock",
            divergence: Divergence::Store(StoreDivergence::Answer {
                step: 0,
                call: "replace",
            }),
        })
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
        full.regional().hour_cycle(),
        Some(flui_platform::HourCycle::H11),
        "the regional formats survive the read"
    );
    assert_eq!(
        full.regional()
            .region()
            .map(|region| region.code().to_owned()),
        Some("GB".to_owned())
    );
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
        built_in.color.map(flui_platform::DisplayColor::headroom),
        Ok(Ok(Headroom::try_new(2.5).expect("finite, at least 1"))),
        "the headroom survives the read"
    );
    assert_eq!(
        displays[1].1.color,
        Err(CapabilityAbsence::NOT_YET_ON_THIS_HOST)
    );
    assert!(
        matches!(&target.reads[4], Read::Displays(Err(failure)) if failure.kind == FailureKind::Displays),
        "the failed display read fails as a display source does"
    );
    let Read::Displays(Ok(displays)) = &target.reads[5] else {
        panic!("the last display read observed the displays");
    };
    assert_eq!(
        displays[0]
            .1
            .color
            .map(|color| (color.color_space().dynamic_range, color.headroom())),
        Ok((
            flui_platform::DynamicRange::High,
            Err(CapabilityAbsence::NOT_ON_THIS_DEVICE)
        )),
        "a high-range display whose peak the system does not report"
    );
    assert_eq!(
        displays[1].1.color,
        Ok(flui_platform::DisplayColor::Standard(
            flui_platform::ColorGamut::Srgb
        )),
        "a standard-range display, written with SDR white"
    );

    assert_eq!(labels.len(), 2, "one id for each label, kept across reads");
}

/// A standard-range display has no headroom to lack or to exceed: a row that
/// says otherwise is refused at its line, where it is read, so no replay
/// hands an application a colour its type cannot hold.
fn a_display_colour_that_contradicts_itself_is_refused() {
    let (_, text) = FAMILY_RECORDINGS
        .iter()
        .find(|(name, _)| *name == "headless_platform_reads")
        .expect("a checked-in family recording");
    let high_range = r#""dynamic_range":"high","headroom":{"absent":"not_on_this_device"}"#;
    assert!(text.contains(high_range));
    for contradiction in [
        r#""dynamic_range":"standard","headroom":{"absent":"not_on_this_device"}"#,
        r#""dynamic_range":"standard","headroom":{"multiple":4.0}"#,
    ] {
        let malformed = text.replace(high_range, contradiction);
        assert!(
            matches!(
                NativeRecording::read(malformed.as_bytes()),
                Err(CodecError::Invalid { line: 7, .. })
            ),
            "{contradiction}"
        );
    }
    let sdr_white = r#""dynamic_range":"standard","headroom":{"multiple":1.0}"#;
    assert!(
        NativeRecording::read(text.replace(high_range, sdr_white).as_bytes()).is_ok(),
        "SDR white is the headroom of a standard range"
    );
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
        last.normal_geometry
            .and_then(flui_platform::WindowGeometry::position),
        Some(flui_foundation::geometry::Point::new(10.0, 20.0))
    );
    assert_eq!(
        last.capabilities.into_parts().maximizes,
        flui_platform::WindowCapability::Absent(CapabilityAbsence::NOT_YET_ON_THIS_HOST),
        "an absence survives the replay"
    );
    assert_eq!(
        handler.count(&HandlerCall::StateChanged),
        target.published.len(),
        "each snapshot differs from the window's own opening one"
    );
}

fn app_title_bar_snapshots_keep_their_wire_values_and_replay() {
    use flui_native::Platform as _;
    use flui_native::journal::codec::{Backend, Header, Stream};
    use flui_platform::{
        AppTitleBar, ControlsOffset, SystemControls, WindowCapabilities, WindowCommand, WindowFrame,
    };
    use flui_testing::window_commands::AdmitAndApply as _;

    let mut capabilities = WindowCapabilities::ALL.into_parts();
    capabilities.sets_frame =
        flui_platform::WindowCapability::Absent(CapabilityAbsence::NOT_YET_ON_THIS_HOST);
    let platform =
        HeadlessPlatform::new().with_window_capabilities(capabilities.into_capabilities());
    let window = platform
        .open_window(WindowOptions::default(), RecordingWindowHandler::new())
        .expect("a headless window opens");
    let cases = [
        (SystemControls::Shown, serde_json::json!("shown")),
        (SystemControls::Hidden, serde_json::json!("hidden")),
        (
            SystemControls::Offset(ControlsOffset::try_new(17.0, 9.0).expect("finite offset")),
            serde_json::json!({"offset": [17.0, 9.0]}),
        ),
    ];
    let mut recording = NativeRecording {
        header: Header::for_stream(Backend::Headless, Stream::WindowState),
        rows: Vec::new(),
    };
    for (controls, _) in &cases {
        let frame = WindowFrame::AppTitleBar(AppTitleBar::new().with_system_controls(*controls));
        window
            .admit_and_apply(WindowCommand::SetFrame(frame))
            .expect("app title bars are available independently of system frames");
        assert_eq!(window.state().frame, frame);
        recording.rows.push(Row::State {
            state: RecordedWindowState::of(window.state()),
            redacted: false,
        });
    }
    let mut written = Vec::new();
    recording.write(&mut written).expect("snapshots write");
    let written = String::from_utf8(written).expect("JSON Lines are UTF-8");
    let lines = written.lines().skip(1).collect::<Vec<_>>();
    assert_eq!(lines.len(), cases.len());
    for (line, (_, expected_controls)) in lines.iter().zip(&cases) {
        let row: serde_json::Value = serde_json::from_str(line).expect("a JSON row");
        assert_eq!(
            row["state"]["frame"],
            serde_json::json!({"app_title_bar": {"system_controls": expected_controls}})
        );
        assert_eq!(
            row["state"]["capabilities"]["sets_frame"],
            serde_json::json!({"unavailable": "not_yet_on_this_host"})
        );
        assert_eq!(
            row["state"]["capabilities"]["app_title_bar"],
            serde_json::json!("native")
        );
    }
    let decoded = NativeRecording::read(written.as_bytes()).expect("snapshots read");
    let owner = run_keeping_owner(Box::new(HeadlessPlatform::new()), &AppLog::new());
    let (replay_window, _) = headless_window(&owner);
    let labels = Labels::default();
    let mut target = WindowStateTarget::new(replay_window, &labels);
    decoded.replay_into(&mut target).expect("snapshots replay");
    assert_eq!(target.published.len(), cases.len());
    for (state, (controls, _)) in target.published.iter().zip(&cases) {
        assert_eq!(
            state.frame,
            WindowFrame::AppTitleBar(AppTitleBar::new().with_system_controls(*controls))
        );
        let capabilities = state.capabilities.into_parts();
        assert_eq!(
            capabilities.sets_frame,
            flui_platform::WindowCapability::Absent(CapabilityAbsence::NOT_YET_ON_THIS_HOST)
        );
        assert_eq!(
            capabilities.app_title_bar,
            flui_platform::WindowCapability::Native
        );
    }
    let valid_offset = r#""offset":[17.0,9.0]"#;
    assert!(written.contains(valid_offset));
    for malformed_offset in [r#""offset":[-1.0,9.0]"#, r#""offset":[17.0,-1.0]"#] {
        let malformed = written.replace(valid_offset, malformed_offset);
        assert!(matches!(
            NativeRecording::read(malformed.as_bytes()),
            Err(CodecError::Invalid { line: 4, .. })
        ));
    }
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
        redacted.replay_into(&mut WindowTarget::new(window)),
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
            r#"{"flui_native_recording":2,"backend":"web","devices":[]}"#,
        ),
        (
            "application stream with a clock",
            r#"{"flui_native_recording":2,"backend":"web","stream":"app","epoch_ns":0,"uptime_ns":0,"devices":[]}"#,
        ),
        (
            "platform stream with a clock",
            r#"{"flui_native_recording":2,"backend":"web","stream":"platform","epoch_ns":0,"uptime_ns":0,"devices":[]}"#,
        ),
        (
            "window state stream with a clock",
            r#"{"flui_native_recording":2,"backend":"web","stream":"window_state","epoch_ns":0,"uptime_ns":0,"devices":[]}"#,
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
pub(super) const CASES: [(&str, fn()); 10] = [
    (
        "window_store_and_close_recordings_replay_on_the_headless_host",
        window_store_and_close_recordings_replay_on_the_headless_host,
    ),
    (
        "a_row_replayed_otherwise_is_a_divergence",
        a_row_replayed_otherwise_is_a_divergence,
    ),
    (
        "app_title_bar_snapshots_keep_their_wire_values_and_replay",
        app_title_bar_snapshots_keep_their_wire_values_and_replay,
    ),
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
        "a_display_colour_that_contradicts_itself_is_refused",
        a_display_colour_that_contradicts_itself_is_refused,
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
            .persistent_key("EDID-0610-A050-SERIAL-C02XL0"),
        flui_native::DisplaySpec::new("Studio Display", 2560, 1440)
            .persistent_key("7A1C2E44-5B6D-4F0E-9C3A-2D8E1F4B6A90"),
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
        .replay_into(&mut WindowTarget::new(mock))
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
        window.replay_into(&mut WindowTarget::new(mock)),
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

#[cfg(all(debug_assertions, feature = "native-capture"))]
thread_local! {
    /// The store the headless window's [`ServedStore`] host was last told to
    /// serve, as the host's input method reaches it.
    static SERVED: RefCell<Option<Rc<dyn TextStore>>> = const { RefCell::new(None) };
}

/// A pull-model store host as a platform's is: it observes the store it is
/// told to serve, and hands it to the test's input method ([`SERVED`]).
#[cfg(all(debug_assertions, feature = "native-capture"))]
struct ServedStore;

#[cfg(all(debug_assertions, feature = "native-capture"))]
impl flui_platform::text_store::TextStoreHost for ServedStore {
    fn focus_store(&self, store: Option<Rc<dyn TextStore>>) {
        if let Some(store) = &store {
            store.set_observer(Some(Rc::new(QuietObserver)));
        }
        SERVED.with(|served| *served.borrow_mut() = store);
    }

    fn complete_composition(
        &self,
        _store: &Rc<dyn TextStore>,
    ) -> Result<
        flui_platform::text_store::CompositionEnd,
        flui_platform::text_store::TextStoreHostError,
    > {
        Err(flui_platform::text_store::TextStoreHostError::NotFocused)
    }
}

/// A platform observer that hears the store's changes and does nothing.
#[cfg(all(debug_assertions, feature = "native-capture"))]
struct QuietObserver;

#[cfg(all(debug_assertions, feature = "native-capture"))]
impl flui_platform::text_store::TextStoreObserver for QuietObserver {
    fn text_changed(&self, _change: flui_platform::text_store::TextChange) {}

    fn selection_changed(&self) {}

    fn layout_changed(&self) {}

    fn status_changed(&self) {}
}

/// The input method paths whose pure halves drive a store in a capture.
#[cfg(all(debug_assertions, feature = "native-capture"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StorePath {
    /// A TSF text service under the Windows host's document.
    Tsf,
    /// The browser's `EditContext` under the web host.
    WebEditContext,
    /// `FluiView`'s editor under the Android host's mirror.
    AndroidMirror,
}

/// A headless session whose window serves a field holding "ab" through a
/// journaled store host, and whose input method on `path` composes "漢"
/// after it ("か", "かん", then the conversion), with an application edit
/// between the first two steps on the paths that keep no snapshot of their
/// own: the field, and the window's recording.
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn captured_store_session(
    path: StorePath,
    privacy: flui_native::journal::recorder::Privacy,
) -> (Rc<InMemoryTextStore>, NativeRecording) {
    use flui_native::journal::codec::{Backend, Stream};
    use flui_native::journal::recorder::Recorder;
    use flui_native::text::conformance::{AndroidMirror, TsfDocument, WebEditContext};
    use flui_platform::text_store::{Utf16Offset, Utf16Range};

    let recorder = Recorder::with_privacy(Backend::Headless, privacy);
    let platform = HeadlessPlatform::new().with_text_store_host(|| Rc::new(ServedStore) as Rc<_>);
    let owner = run_keeping_owner(Box::new(platform), &AppLog::new());
    let window = owner
        .open_window(WindowOptions::default(), RecordingWindowHandler::new())
        .expect("a headless window opens")
        .try_ready()
        .expect("at once");
    let host = owner
        .text_store_host(&window)
        .expect("the headless window is pull-model with a store host");
    let field = InMemoryTextStore::new("ab");
    host.focus_store(Some(Rc::clone(&field) as Rc<dyn TextStore>));
    let served = SERVED
        .with(|served| served.borrow().clone())
        .expect("the host serves the focused store");
    assert!(
        !std::ptr::addr_eq(Rc::as_ptr(&served), Rc::as_ptr(&field)),
        "the host serves a journaled store in the field's place"
    );
    let app_edit = || {
        field.app_replace(
            Utf16Range::try_new(Utf16Offset::ZERO, Utf16Offset::new(1)).expect("ordered"),
            "A",
        );
    };
    match path {
        StorePath::Tsf => {
            let document = TsfDocument::focus(served);
            document.edit(|session| {
                session.set_text(2, 2, "か");
                session.update_composition(2, 3);
                session.set_caret(3);
            });
            app_edit();
            document.edit(|session| {
                session.set_text(2, 3, "かん");
                session.update_composition(2, 4);
                session.set_caret(4);
            });
            document.edit(|session| {
                session.set_text(2, 4, "漢");
                session.end_composition();
                session.set_caret(3);
            });
        }
        StorePath::WebEditContext => {
            let context = WebEditContext::focus(served);
            context.text_update(2..2, "か", 3..3, true);
            app_edit();
            context.text_update(2..3, "かん", 4..4, true);
            context.text_update(2..4, "漢", 3..3, true);
            context.composition_end();
        }
        StorePath::AndroidMirror => {
            let mirror = AndroidMirror::focus(served);
            mirror.report("abか", (3, 3), Some((2, 3)));
            mirror.report("abかん", (4, 4), Some((2, 4)));
            mirror.report("ab漢", (3, 3), None);
        }
    }
    host.focus_store(None);
    let window_stream = only(&recorder.finish(), Stream::Window);
    (field, window_stream)
}

/// What an input method left a store holding: its text, selection and
/// composition.
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn document_of(
    store: &InMemoryTextStore,
) -> (
    String,
    flui_platform::text_store::Selection,
    Option<flui_platform::text_store::Composition>,
) {
    (store.text(), store.selection(), store.composition())
}

/// The locks of a window stream whose row holds the document they found.
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn opened_documents(recording: &NativeRecording) -> Vec<String> {
    recording
        .rows
        .iter()
        .filter_map(|row| match row {
            Row::StoreLock { lock, .. } => lock.opened.as_ref().map(|opened| opened.text.clone()),
            _ => None,
        })
        .collect()
}

/// On each path whose pure half drives the shared store (TSF, the web's
/// `EditContext`, Android's mirror), a composition's store locks are
/// journaled into the window's stream, write and read back unchanged, and
/// replay on a fresh store of the headless host to the document the field
/// was left with: the document a lock found is recorded at the first lock
/// and again after the application changed the field. A redacting capture
/// writes none of the text and its replay refuses the first lock that lost
/// some.
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn captured_store_locks_replay_to_the_document_they_left() {
    use flui_native::journal::recorder::Privacy;

    for (path, text, opened) in [
        (StorePath::Tsf, "Ab漢", vec!["ab", "Abか"]),
        (StorePath::WebEditContext, "Ab漢", vec!["ab", "Abか"]),
        (StorePath::AndroidMirror, "ab漢", vec!["ab"]),
    ] {
        let (field, recording) = captured_store_session(path, Privacy::KeepText);
        assert_eq!(field.text(), text, "{path:?}: the input method composed");
        assert!(
            matches!(recording.rows.first(), Some(Row::StoreFocus(Some(_)))),
            "{path:?}: the focus is the stream's first row"
        );
        assert!(
            matches!(recording.rows.last(), Some(Row::StoreFocus(None))),
            "{path:?}: the blur is its last"
        );
        assert_eq!(opened_documents(&recording), opened, "{path:?}");
        let mut written = Vec::new();
        recording
            .write(&mut written)
            .expect("a captured recording writes");
        assert_eq!(
            NativeRecording::read(written.as_slice()).expect("it reads back"),
            recording,
            "{path:?}"
        );

        let owner = run_keeping_owner(Box::new(HeadlessPlatform::new()), &AppLog::new());
        let (window, _) = headless_window(&owner);
        let mut target = WindowTarget::new(window);
        recording
            .replay_into(&mut target)
            .unwrap_or_else(|error| panic!("{path:?}: the window stream replays: {error}"));
        assert_eq!(
            document_of(&first_store(&target)),
            document_of(&field),
            "{path:?}: the replayed store holds what the field was left with"
        );

        let (_, redacted) = captured_store_session(path, Privacy::Redact);
        let mut written = Vec::new();
        redacted
            .write(&mut written)
            .expect("a redacted recording writes");
        let written = String::from_utf8(written).expect("JSON Lines are UTF-8");
        assert!(
            !written.contains('か') && !written.contains("ab"),
            "{path:?}: no text reached the recording"
        );
        let (window, _) = headless_window(&owner);
        assert!(
            matches!(
                redacted.replay_into(&mut WindowTarget::new(window)),
                Err(ReplayError::Redacted {
                    row: "store_lock",
                    ..
                })
            ),
            "{path:?}"
        );
    }
}

/// A lock that makes more calls than a row holds is journaled with the
/// first [`MAX_STORE_STEPS`](flui_native::journal::store::MAX_STORE_STEPS)
/// and marked incomplete, and so is one whose body panicked after it edited;
/// a replay refuses either with a typed error naming the row, never applying
/// part of it.
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn a_store_lock_past_its_bounds_or_unwound_is_incomplete() {
    use flui_native::journal::codec::{Backend, Stream};
    use flui_native::journal::recorder::{Privacy, Recorder};
    use flui_native::journal::store::MAX_STORE_STEPS;
    use flui_platform::text_store::{LockGrant, LockTiming};

    let recorder = Recorder::with_privacy(Backend::Headless, Privacy::KeepText);
    let platform = HeadlessPlatform::new().with_text_store_host(|| Rc::new(ServedStore) as Rc<_>);
    let owner = run_keeping_owner(Box::new(platform), &AppLog::new());
    let window = owner
        .open_window(WindowOptions::default(), RecordingWindowHandler::new())
        .expect("a headless window opens")
        .try_ready()
        .expect("at once");
    let host = owner.text_store_host(&window).expect("a store host");
    let field = InMemoryTextStore::new("ab");
    host.focus_store(Some(Rc::clone(&field) as Rc<dyn TextStore>));
    let served = SERVED
        .with(|served| served.borrow().clone())
        .expect("the host serves the focused store");
    let _granted = served
        .request_lock(
            LockGrant::read(|session| {
                for _ in 0..MAX_STORE_STEPS + 10 {
                    let _ = session.selection();
                }
            }),
            LockTiming::Sync,
        )
        .expect("a free store grants a sync read");
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: the test unwinds an input method's lock body on purpose"
    )]
    let _unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        served.request_lock(
            LockGrant::read_write(|session| {
                let _ = session.insert_at_selection("x");
                panic!("an input method's code fails inside its lock");
            }),
            LockTiming::Sync,
        )
    }));
    let recording = only(&recorder.finish(), Stream::Window);
    let locks: Vec<(usize, bool)> = recording
        .rows
        .iter()
        .filter_map(|row| match row {
            Row::StoreLock { lock, .. } => Some((lock.steps.len(), lock.incomplete)),
            _ => None,
        })
        .collect();
    assert_eq!(locks, [(MAX_STORE_STEPS, true), (1, true)]);
    let (window, _) = headless_window(&owner);
    assert_eq!(
        recording.replay_into(&mut WindowTarget::new(window)),
        Err(ReplayError::Incomplete {
            line: 3,
            row: "store_lock"
        })
    );
}

/// The close rows a capture journals: a request the handler vetoed, one it
/// granted, and the close that followed it; a window closed by the
/// application's command, asked nothing. Both replay on fresh headless
/// windows to the same decisions and closes.
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn captured_close_requests_replay_to_the_same_decisions() {
    use flui_native::journal::codec::{Backend, CloseAnswer, Stream};
    use flui_native::journal::recorder::Recorder;

    let recorder = Recorder::start(Backend::Headless);
    let owner = run_keeping_owner(Box::new(HeadlessPlatform::new()), &AppLog::new());
    let (asked, handler) = headless_window(&owner);
    let _ = veto_once(&handler);
    assert!(!asked.simulate_close(), "the first request is vetoed");
    assert!(asked.simulate_close(), "the second is granted");
    let (commanded, _) = headless_window(&owner);
    commanded
        .admit_and_apply(WindowCommand::Close)
        .expect("the window closes on the application's command");
    let windows: Vec<NativeRecording> = recorder
        .finish()
        .into_iter()
        .filter(|recording| recording.header.stream == Stream::Window)
        .collect();
    let [asked, commanded] = windows.as_slice() else {
        panic!("one window stream per window, in the order they opened");
    };
    assert_eq!(
        asked.rows,
        [
            Row::CloseRequest {
                answer: CloseAnswer::KeepOpen,
                queued: false
            },
            Row::CloseRequest {
                answer: CloseAnswer::Close,
                queued: false
            },
            Row::Closed(CloseCause::GrantedRequest),
        ]
    );
    assert_eq!(commanded.rows, [Row::Closed(CloseCause::NoRequest)]);

    let (window, handler) = headless_window(&owner);
    let replayed = veto_once(&handler);
    asked
        .replay_into(&mut WindowTarget::new(window))
        .expect("the asked window's stream replays");
    assert_eq!(replayed.get(), 2);
    assert_eq!(handler.count(&HandlerCall::Closed), 1);
    let (window, handler) = headless_window(&owner);
    commanded
        .replay_into(&mut WindowTarget::new(window))
        .expect("the commanded window's stream replays");
    assert_eq!(handler.count(&HandlerCall::CloseRequested), 0);
    assert_eq!(handler.count(&HandlerCall::Closed), 1);
}

/// The capture cases of the streams beside a registry's.
#[cfg(all(debug_assertions, feature = "native-capture"))]
pub(super) const CAPTURE_CASES: [(&str, fn()); 4] = [
    (
        "captured_streams_replay_to_what_the_session_delivered",
        captured_streams_replay_to_what_the_session_delivered,
    ),
    (
        "captured_store_locks_replay_to_the_document_they_left",
        captured_store_locks_replay_to_the_document_they_left,
    ),
    (
        "a_store_lock_past_its_bounds_or_unwound_is_incomplete",
        a_store_lock_past_its_bounds_or_unwound_is_incomplete,
    ),
    (
        "captured_close_requests_replay_to_the_same_decisions",
        captured_close_requests_replay_to_the_same_decisions,
    ),
];
