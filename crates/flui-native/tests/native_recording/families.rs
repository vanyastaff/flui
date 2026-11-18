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
    RecordedWindowState, ReplayError, ReplayTarget, Replayed, Row, Unsupported,
};
use flui_native::journal::store::{RecordedStore, StoreFocus};
use flui_native::testing::app::{AppCall, AppLog, run_keeping_owner};
use flui_native::{
    HeadlessDisplayControls, HeadlessHostEvents, HeadlessHostWindow, HeadlessPlatform,
    HeadlessPreferences, OwnerPlatform,
};
use flui_platform::CloseDecision;
use flui_platform::text_store::TextStore;
use flui_platform::{
    CapabilityAbsence, DisplayId, DisplayKey, DisplayParts, DisplayRegistry, Headroom, ImeEvent,
    MemoryPressureLevel, PlatformInput, SessionEndPhase, SystemNotice, SystemPreferences,
    WindowOptions, WindowState, WindowStateParts,
};
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

/// Asserts a replay reproduced every row: a cut recording is not a match.
#[track_caller]
pub(super) fn whole(replayed: Replayed) {
    assert_eq!(replayed, Replayed::Whole, "the recording replayed whole");
}

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
            store.set_single_line(focus.status.single_line);
            store.set_text_input(focus.status.text_input);
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

    fn closed(&mut self, _cause: CloseCause) -> Result<(), Unsupported> {
        // A window the replayed request it granted closed already is closed.
        self.window.close_now();
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
                    infos.push(display.parts.clone().into_description(id));
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
        let sample = state
            .clone()
            .into_parts(|label| Some(self.labels.id(label)))
            .into_state();
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

/// The snapshots a `window_state` recording holds, as [`facts`] compares them,
/// each label on the display `resolve` gives it.
fn recorded_states(
    recording: &NativeRecording,
    mut resolve: impl FnMut(u64) -> Option<DisplayId>,
) -> Vec<(Option<u64>, WindowStateParts)> {
    recording
        .rows
        .iter()
        .filter_map(|row| match row {
            Row::State { state, .. } => {
                let parts = state.clone().into_parts(&mut resolve);
                // [`facts`] carries the display as its label.
                Some((
                    state.display(),
                    WindowStateParts {
                        display: None,
                        ..parts
                    },
                ))
            }
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
    whole(
        family("headless_app_notices")
            .replay_into(&mut HostEventsTarget(events))
            .expect("an application stream replays on the headless host"),
    );
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
    whole(
        family("headless_window_ime")
            .replay_into(&mut WindowTarget::new(window))
            .expect("a window stream replays on a headless window"),
    );
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
    whole(
        family("headless_window_store_and_close")
            .replay_into(&mut target)
            .expect("a window stream with store locks and close requests replays"),
    );
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

/// A lock that names a store the recording never saw focused, or whose
/// document it never held, is malformed in a recording that began with its
/// stream and refused as it is read, naming the line; in one whose header
/// says it joined a live stream late, it reads, and its replay is refused at
/// that lock with `JoinedLate`. A recording that focuses one store twice is
/// refused as it is read.
fn a_store_lock_the_recording_joined_after_is_refused() {
    let owner = run_keeping_owner(Box::new(HeadlessPlatform::new()), &AppLog::new());
    // The golden's rows: the focus, the lock that found "ab", a read lock, ….
    let written = |recording: &NativeRecording| {
        let mut bytes = Vec::new();
        recording.write(&mut bytes).expect("a recording writes");
        bytes
    };
    // Missing the document of a store focused in the recording is malformed whether it joined
    // late or not: the first lock after a focus records it.
    let mut no_document = family("headless_window_store_and_close");
    no_document.rows.remove(1);
    for joined_late in [false, true] {
        no_document.header.joined_late = joined_late;
        assert!(
            matches!(
                NativeRecording::read(written(&no_document).as_slice()),
                Err(CodecError::Invalid { line: 3, .. })
            ),
            "a lock after its store's focus and without its document, joined late: {joined_late}"
        );
    }
    {
        let (case, missed, line) = ("missed the focus", 0, 2);
        let mut joined = family("headless_window_store_and_close");
        joined.rows.remove(missed);
        assert!(
            matches!(
                NativeRecording::read(written(&joined).as_slice()),
                Err(CodecError::Invalid { line: found, .. }) if found == line
            ),
            "{case}: a recording from its stream's start that misses it is malformed"
        );
        joined.header.joined_late = true;
        let joined = NativeRecording::read(written(&joined).as_slice())
            .unwrap_or_else(|error| panic!("{case}: a late join reads: {error}"));
        let (window, _) = headless_window(&owner);
        assert_eq!(
            joined.replay_into(&mut WindowTarget::new(window)),
            Err(ReplayError::JoinedLate {
                line,
                row: "store_lock"
            }),
            "{case}"
        );
    }

    // Each rule a late join relaxes, before the join point (the first row of its kind, with the
    // context it rests on before the join) and after it (the context seen in the recording):
    // relaxed before, held in full after, and held in full in a recording that did not join.
    let header = |joined: bool| {
        format!(
            r#"{{"flui_native_recording":4,"backend":"headless","stream":"window"{}}}"#,
            if joined { r#","joined_late":true"# } else { "" }
        )
    };
    let focus = r#"{"row":"store_focus","focus":{"store":1,"status":{}}}"#;
    let opened = r#"{"row":"store_lock","lock":{"store":1,"kind":"read","opened":{"text":"","selection":[0,0]}}}"#;
    let bare = r#"{"row":"store_lock","lock":{"store":1,"kind":"read","steps":[{"call":"selection","selection":[0,0]}]}}"#;
    let request = |answer: &str| format!(r#"{{"row":"close_request","answer":"{answer}"}}"#);
    let closed = |cause: &str| format!(r#"{{"row":"closed","cause":"{cause}"}}"#);
    for (case, rows, before_join, after_join) in [
        (
            "a lock on a store focused before the join",
            vec![bare.to_owned()],
            true,
            false,
        ),
        (
            "a lock without its document on a store focused after the join",
            vec![focus.to_owned(), bare.to_owned()],
            false,
            false,
        ),
        (
            "a document then a bare lock on a store focused after the join",
            vec![focus.to_owned(), opened.to_owned(), bare.to_owned()],
            true,
            true,
        ),
        (
            "a first `closing` after a grant before the join",
            vec![request("closing"), closed("granted_request")],
            true,
            false,
        ),
        (
            "a first close for a grant before the join",
            vec![closed("granted_request")],
            true,
            false,
        ),
        (
            "`closing` after a refusal the recording saw",
            vec![request("keep_open"), request("closing")],
            false,
            false,
        ),
        (
            "a close for a grant after a refusal the recording saw",
            vec![request("keep_open"), closed("granted_request")],
            false,
            false,
        ),
    ] {
        for (joined, reads) in [(true, before_join), (false, after_join)] {
            let text = format!("{}\n{}\n", header(joined), rows.join("\n"));
            assert_eq!(
                NativeRecording::read(text.as_bytes()).is_ok(),
                reads,
                "{case}, joined late: {joined}"
            );
        }
    }

    let (_, text) = FAMILY_RECORDINGS[0];
    let focus = r#"{"row":"store_focus","focus":{"store":1,"status":{"single_line":true}}}"#;
    let twice = format!("{text}{focus}\n");
    assert!(
        matches!(
            NativeRecording::read(twice.as_bytes()),
            Err(CodecError::Invalid { line: 10, .. })
        ),
        "a store focused twice is refused at its second focus"
    );
}

/// Close rows in an order no window's delivery writes are refused as they
/// are read, naming the line: a second close, a request after the close, a
/// `closing` with no granted request before it, a request asked after one was
/// granted, and a close whose cause contradicts the requests before it.
fn close_rows_out_of_order_are_refused() {
    let header =
        r#"{"flui_native_recording":4,"backend":"headless","stream":"window","devices":[]}"#;
    let request = |answer: &str| format!(r#"{{"row":"close_request","answer":"{answer}"}}"#);
    let closed = |cause: &str| format!(r#"{{"row":"closed","cause":"{cause}"}}"#);
    for (case, rows, line) in [
        (
            "a second close",
            vec![closed("no_request"), closed("no_request")],
            3,
        ),
        (
            "a request after the close",
            vec![closed("no_request"), request("close")],
            3,
        ),
        ("closing with nothing granted", vec![request("closing")], 2),
        (
            "a request asked after a grant",
            vec![request("close"), request("keep_open")],
            3,
        ),
        (
            "a granted close with no grant",
            vec![request("keep_open"), closed("granted_request")],
            3,
        ),
        (
            "a close that asked nothing after a grant",
            vec![request("close"), closed("no_request")],
            3,
        ),
    ] {
        let text = format!("{header}\n{}\n", rows.join("\n"));
        assert!(
            matches!(
                NativeRecording::read(text.as_bytes()),
                Err(CodecError::Invalid { line: found, .. }) if found == line
            ),
            "{case}"
        );
    }
    let live = format!(
        "{header}\n{}\n",
        [
            request("keep_open"),
            request("close"),
            request("closing"),
            closed("granted_request")
        ]
        .join("\n")
    );
    assert!(
        NativeRecording::read(live.as_bytes()).is_ok(),
        "the order a delivery writes reads"
    );
}

/// A writer never writes what its reader refuses: a recording whose rows
/// would pass the byte bound is written up to the last row that fits and a
/// `cut` row saying why, which reads as a whole recording; a row after a cut
/// is refused as it is read.
fn a_recording_past_its_bound_is_written_cut() {
    use flui_native::journal::codec::{CutReason, Header};
    use flui_protocol::native_recording::{MAX_LINE_BYTES, MAX_RECORDING_BYTES, MAX_ROWS};

    let commit = "x".repeat(MAX_LINE_BYTES - 128);
    let rows = MAX_RECORDING_BYTES / commit.len() + 2;
    let long = NativeRecording {
        header: Header::for_stream(
            flui_native::journal::codec::Backend::Headless,
            flui_native::journal::codec::Stream::Window,
        ),
        rows: vec![
            Row::Ime {
                event: ImeEvent::Commit(commit),
                redacted: false,
            };
            rows
        ],
    };
    let mut written = Vec::new();
    long.write(&mut written).expect("a long recording writes");
    assert!(
        written.len() <= MAX_RECORDING_BYTES,
        "what the writer writes fits"
    );
    let read = NativeRecording::read(written.as_slice()).expect("and its reader reads it");
    assert_eq!(read.rows.last(), Some(&Row::Cut(CutReason::Bytes)));
    assert!(
        read.rows.len() < rows,
        "the rows past the bound were not written"
    );
    // Its replay reproduces every row before the cut and says it was cut, and where.
    let owner = run_keeping_owner(Box::new(HeadlessPlatform::new()), &AppLog::new());
    let (window, handler) = headless_window(&owner);
    let cut_line = read.rows.len() as u64 + 1;
    assert_eq!(
        read.replay_into(&mut WindowTarget::new(window)),
        Ok(Replayed::Cut {
            line: cut_line,
            reason: CutReason::Bytes
        })
    );
    assert_eq!(
        ime_events(&handler).len(),
        read.rows.len() - 1,
        "every row before the cut replayed"
    );

    let header =
        r#"{"flui_native_recording":4,"backend":"headless","stream":"window","devices":[]}"#;
    let after =
        format!("{header}\n{{\"row\":\"cut\",\"reason\":\"bytes\"}}\n{{\"row\":\"reopen\"}}\n");
    assert!(matches!(
        NativeRecording::read(after.as_bytes()),
        Err(CodecError::Invalid { line: 3, .. })
    ));

    // The row bound: a recording of exactly `MAX_ROWS` rows is written whole, with no room
    // kept after its last row; one row more is cut where the cut still fits.
    let reopens = |count: usize| NativeRecording {
        header: Header::for_stream(
            flui_native::journal::codec::Backend::Headless,
            flui_native::journal::codec::Stream::App,
        ),
        rows: vec![Row::Reopen; count],
    };
    for (count, last) in [
        (MAX_ROWS, Row::Reopen),
        (MAX_ROWS + 1, Row::Cut(CutReason::Rows)),
    ] {
        let mut written = Vec::new();
        reopens(count).write(&mut written).expect("it writes");
        let read = NativeRecording::read(written.as_slice()).expect("its reader reads it");
        assert_eq!(read.rows.len(), MAX_ROWS, "{count} rows");
        assert_eq!(read.rows.last(), Some(&last), "{count} rows");
    }
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
    whole(
        recording
            .replay_into(&mut target)
            .expect("a platform stream replays on the headless host"),
    );
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
        full.accent_color(),
        Some(flui_platform::SrgbColor::from_rgba8(0, 120, 215, 255)),
        "the accent survives the read"
    );
    assert_eq!(
        (
            full.scroll_bars().visibility(),
            full.scroll_bars().track_click()
        ),
        (
            Some(flui_platform::ScrollBarVisibility::Always),
            Some(flui_platform::ScrollTrackClick::JumpToSpot)
        ),
        "the scroll-bar preferences survive the read, each in its own spelling"
    );
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
    whole(
        recording
            .replay_into(&mut target)
            .expect("a window state stream replays on a headless window"),
    );
    assert_eq!(
        target
            .published
            .iter()
            .cloned()
            .map(|state| facts(state, |id| labels.label(id)))
            .collect::<Vec<_>>(),
        recorded_states(&recording, |label| Some(labels.id(label)))
    );
    assert_eq!(labels.len(), 2, "one id for each label, kept across rows");
    let last = target.published.last().expect("snapshots were replayed");
    assert_eq!(last.title, None);
    assert_eq!(
        last.normal_geometry
            .and_then(flui_platform::WindowGeometry::position)
            .map(flui_platform::WindowPosition::at),
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
    whole(decoded.replay_into(&mut target).expect("snapshots replay"));
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

/// A snapshot's keyboard is written with the inset a docked one covers or the
/// frame a floating one has, reads back as the placement it was, moving or
/// at rest, and replays; a row that names both an inset and a floating frame
/// is refused, since a floating keyboard covers no edge.
fn keyboard_placements_keep_their_wire_values_and_replay() {
    use std::time::Duration;

    use flui_foundation::geometry::{EdgeInsets, Rect};
    use flui_native::journal::codec::{Backend, Header, Stream};
    use flui_platform::{
        EventTime, InsetCurve, InsetMotion, KeyboardPlacement, PlatformWindow as _, SoftKeyboard,
        WindowInsets,
    };

    let docked = KeyboardPlacement::Docked {
        inset: EdgeInsets::new(0.0, 0.0, 336.0, 0.0),
    };
    let floating = KeyboardPlacement::Floating {
        // Corners that the frame's width and height do not add back to: the
        // row holds the corners themselves, and reads back to the bit.
        frame: Rect::from_ltrb(220.1, 24.9, 493.8, 105.8),
    };
    let undocking = InsetMotion::try_new(
        EdgeInsets::new(0.0, 0.0, 336.0, 0.0),
        EventTime::from_nanos(1_000_000),
        Duration::from_millis(250),
        InsetCurve::SystemKeyboard,
    )
    .expect("a valid motion");
    let cases = [
        (
            SoftKeyboard::try_at_rest(docked).expect("a docked keyboard"),
            serde_json::json!({"target": [0.0, 0.0, 336.0, 0.0]}),
        ),
        (
            SoftKeyboard::try_at_rest(floating).expect("a floating keyboard"),
            serde_json::json!({
                "target": [0.0, 0.0, 0.0, 0.0],
                "floating": [220.1, 24.9, 493.8, 105.8],
            }),
        ),
        (
            SoftKeyboard::try_moving(floating, undocking).expect("a keyboard that undocks"),
            serde_json::json!({
                "target": [0.0, 0.0, 0.0, 0.0],
                "motion": {
                    "from": [0.0, 0.0, 336.0, 0.0],
                    "start_ns": 1_000_000,
                    "duration_ns": 250_000_000,
                    "curve": {"curve": "system_keyboard"},
                },
                "floating": [220.1, 24.9, 493.8, 105.8],
            }),
        ),
    ];
    let recorded = run_keeping_owner(Box::new(HeadlessPlatform::new()), &AppLog::new());
    let (window, _) = headless_window(&recorded);
    let mut recording = NativeRecording {
        header: Header::for_stream(Backend::Headless, Stream::WindowState),
        rows: Vec::new(),
    };
    for (keyboard, _) in &cases {
        window.simulate_insets(WindowInsets::ZERO.with_keyboard(*keyboard));
        assert_eq!(window.state().insets.keyboard(), *keyboard);
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
    for (line, (_, expected)) in lines.iter().zip(&cases) {
        let row: serde_json::Value = serde_json::from_str(line).expect("a JSON row");
        assert_eq!(&row["state"]["keyboard"], expected);
    }

    let decoded = NativeRecording::read(written.as_bytes()).expect("snapshots read");
    let owner = run_keeping_owner(Box::new(HeadlessPlatform::new()), &AppLog::new());
    let (replay_window, _) = headless_window(&owner);
    let labels = Labels::default();
    let mut target = WindowStateTarget::new(replay_window, &labels);
    whole(decoded.replay_into(&mut target).expect("snapshots replay"));
    assert_eq!(
        target
            .published
            .iter()
            .map(|state| state.insets.keyboard())
            .collect::<Vec<_>>(),
        cases.map(|(keyboard, _)| keyboard)
    );

    let at_rest = r#""target":[0.0,0.0,0.0,0.0],"floating""#;
    assert!(written.contains(at_rest));
    let contradictory = written.replacen(at_rest, r#""target":[0.0,0.0,336.0,0.0],"floating""#, 1);
    assert!(matches!(
        NativeRecording::read(contradictory.as_bytes()),
        Err(CodecError::Invalid { line: 3, .. })
    ));
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
            r#"{"flui_native_recording":4,"backend":"web","devices":[]}"#,
        ),
        (
            "application stream with a clock",
            r#"{"flui_native_recording":4,"backend":"web","stream":"app","epoch_ns":0,"uptime_ns":0,"devices":[]}"#,
        ),
        (
            "platform stream with a clock",
            r#"{"flui_native_recording":4,"backend":"web","stream":"platform","epoch_ns":0,"uptime_ns":0,"devices":[]}"#,
        ),
        (
            "window state stream with a clock",
            r#"{"flui_native_recording":4,"backend":"web","stream":"window_state","epoch_ns":0,"uptime_ns":0,"devices":[]}"#,
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

/// A store's focus keeps every field of its status: what the field asks of
/// the input method is written in full and reads back as it was written; an
/// older recording, with no Enter action or correction, reads as the hosts
/// then behaved; a sign or a separator on a purpose that is not a number is
/// refused.
fn a_store_focus_keeps_every_field_of_its_status() {
    use flui_platform::text_store::{
        Autofill, Capitalization, Correction, EnterAction, NumberFormat, TextInputOptions,
        TextInputPurpose, TextStoreStatus,
    };

    let header =
        r#"{"flui_native_recording":4,"backend":"headless","stream":"window","devices":[]}"#;
    let focused = |status: &str| {
        format!(r#"{{"row":"store_focus","focus":{{"store":1,"status":{status}}}}}"#)
    };
    let plain = TextStoreStatus::EDITABLE_SINGLE_LINE;
    let signed_decimal = TextInputPurpose::Number(NumberFormat {
        signed: true,
        decimal: true,
    });
    // (the status read, what it means, the status written back)
    for (status, expected, rewritten) in [
        (
            r#"{"single_line":true}"#,
            plain,
            r#"{"single_line":true,"enter_action":"default","correction":"autocorrect"}"#,
        ),
        (
            r#"{"single_line":true,"purpose":"search"}"#,
            plain.with_text_input(TextInputOptions::new(TextInputPurpose::Search)),
            r#"{"single_line":true,"purpose":"search","enter_action":"search","correction":"autocorrect"}"#,
        ),
        (
            r#"{"purpose":"email"}"#,
            plain
                .with_single_line(false)
                .with_text_input(TextInputOptions::new(TextInputPurpose::Email)),
            r#"{"purpose":"email","enter_action":"default","correction":"off"}"#,
        ),
        (
            r#"{"protected":true,"purpose":"number","signed":true,"decimal":true,"enter_action":"next","capitalization":"words","correction":"suggestions","autofill":"one_time_code"}"#,
            plain
                .with_protected(true)
                .with_single_line(false)
                .with_text_input(
                    TextInputOptions::new(signed_decimal)
                        .enter_action(EnterAction::Next)
                        .capitalization(Capitalization::Words)
                        .correction(Correction::Suggestions)
                        .autofill(Autofill::OneTimeCode),
                ),
            r#"{"protected":true,"purpose":"number","signed":true,"decimal":true,"enter_action":"next","capitalization":"words","correction":"suggestions","autofill":"one_time_code"}"#,
        ),
        (
            r#"{"single_line":true,"purpose":"search","enter_action":"default","correction":"off","autofill":"new_password"}"#,
            plain.with_text_input(
                TextInputOptions::new(TextInputPurpose::Search)
                    .enter_action(EnterAction::Default)
                    .correction(Correction::Off)
                    .autofill(Autofill::NewPassword),
            ),
            r#"{"single_line":true,"purpose":"search","enter_action":"default","correction":"off","autofill":"new_password"}"#,
        ),
    ] {
        let text = format!("{header}\n{}\n", focused(status));
        let recording = NativeRecording::read(text.as_bytes()).expect("a status it reads");
        let Some(Row::StoreFocus(Some(focus))) = recording.rows.first() else {
            panic!("one focus: {:?}", recording.rows);
        };
        assert_eq!(focus.status, expected, "{status}");
        let mut written = Vec::new();
        recording.write(&mut written).expect("written");
        let written = String::from_utf8(written).expect("UTF-8");
        assert_eq!(
            written.lines().nth(1),
            Some(focused(rewritten).as_str()),
            "written back in full"
        );
    }
    let signed_email = format!(
        "{header}\n{}\n",
        focused(r#"{"purpose":"email","signed":true}"#)
    );
    assert!(
        matches!(
            NativeRecording::read(signed_email.as_bytes()),
            Err(CodecError::Invalid { line: 2, .. })
        ),
        "a sign on an address is refused"
    );
}

/// The cases of the streams beside a registry's.
pub(super) const CASES: [(&str, fn()); 15] = [
    (
        "a_store_focus_keeps_every_field_of_its_status",
        a_store_focus_keeps_every_field_of_its_status,
    ),
    (
        "close_rows_out_of_order_are_refused",
        close_rows_out_of_order_are_refused,
    ),
    (
        "a_recording_past_its_bound_is_written_cut",
        a_recording_past_its_bound_is_written_cut,
    ),
    (
        "a_store_lock_the_recording_joined_after_is_refused",
        a_store_lock_the_recording_joined_after_is_refused,
    ),
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
        "keyboard_placements_keep_their_wire_values_and_replay",
        keyboard_placements_keep_their_wire_values_and_replay,
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
            .expect("in range")
            .with_accent_color(Some(flui_platform::SrgbColor::from_rgba8(255, 45, 85, 255)))
            .with_scroll_bars(
                flui_platform::ScrollBarPreferences::default()
                    .with_visibility(flui_platform::ScrollBarVisibility::WhenScrolling)
                    .with_track_click(flui_platform::ScrollTrackClick::Page),
            ),
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
        .simulate_display_change(
            primary,
            DisplayChange::ScaleFactor(
                flui_foundation::geometry::DevicePixelRatio::try_new(2.0)
                    .expect("a positive ratio"),
            ),
        )
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
    // The journal labels a display by its issued number, so each label the
    // session's snapshots name resolves to the id the window published.
    let issued = session
        .states
        .iter()
        .flat_map(|state| {
            state
                .display
                .into_iter()
                .chain(
                    state
                        .outer
                        .position()
                        .map(flui_platform::WindowPosition::display),
                )
                .chain(
                    state
                        .normal_geometry
                        .and_then(flui_platform::WindowGeometry::position)
                        .map(flui_platform::WindowPosition::display),
                )
        })
        .map(|id| (id.get(), id))
        .collect::<HashMap<_, _>>();
    assert_eq!(
        recorded_states(&states, |label| issued.get(&label).copied()),
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
    whole(
        app.replay_into(&mut HostEventsTarget(events))
            .expect("the application stream replays"),
    );
    assert_eq!(log.calls(), session.calls);
    let (mock, handler) = headless_window(&owner);
    whole(
        window
            .replay_into(&mut WindowTarget::new(mock))
            .expect("the window stream replays"),
    );
    assert_eq!(ime_events(&handler), session.ime);
    let labels = Labels::default();
    let mut target = PlatformTarget::new(sources, &owner, &labels);
    whole(
        reads
            .replay_into(&mut target)
            .expect("the platform stream replays"),
    );
    assert_eq!(target.reads, session.reads);

    // The window's snapshots replay on a window of the replayed platform,
    // each through the successor rules the live window published it by, and
    // on the display the platform stream's replay issued for its label.
    let (mock, replayed) = headless_window(&owner);
    let mut target = WindowStateTarget::new(mock, &labels);
    whole(
        states
            .replay_into(&mut target)
            .expect("the window state stream replays"),
    );
    assert_eq!(
        target
            .published
            .iter()
            .cloned()
            .map(|state| facts(state, |id| labels.label(id)))
            .collect::<Vec<_>>(),
        recorded_states(&states, |label| Some(labels.id(label))),
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
    assert!(
        matches!(
            &window.rows[1],
            Row::Ime { event: ImeEvent::Preedit { text, cursor: Some((start, end)) }, .. }
                if start == end && *end == text.len()
        ),
        "a redacted preedit selects no clause, its caret after the replacement: {:?}",
        window.rows[1]
    );
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
    whole(
        reads
            .replay_into(&mut target)
            .expect("a redacted display read replays"),
    );
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

    fn show_keyboard(&self, _store: &Rc<dyn TextStore>) {}
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
/// own: the field, and the window's recording. The field has `status`.
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn captured_store_session(
    path: StorePath,
    privacy: flui_native::journal::recorder::Privacy,
    status: flui_platform::text_store::TextStoreStatus,
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
    field.set_protected(status.protected);
    field.set_text_input(status.text_input);
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
        let (field, recording) = captured_store_session(
            path,
            Privacy::KeepText,
            flui_platform::text_store::TextStoreStatus::EDITABLE_SINGLE_LINE,
        );
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
        whole(
            recording
                .replay_into(&mut target)
                .unwrap_or_else(|error| panic!("{path:?}: the window stream replays: {error}")),
        );
        assert_eq!(
            document_of(&first_store(&target)),
            document_of(&field),
            "{path:?}: the replayed store holds what the field was left with"
        );

        let (_, redacted) = captured_store_session(
            path,
            Privacy::Redact,
            flui_platform::text_store::TextStoreStatus::EDITABLE_SINGLE_LINE,
        );
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

/// Every answer a close request's row can hold and every cause of a close,
/// each recorded from a live headless window and replayed on a fresh one: a
/// veto and a grant are asked again and decided the same; a request queued
/// behind the granted one (`closing`) is replayed without asking, as it was
/// answered; a handler that panicked is refused as incomplete; a close by the
/// application's command asks nothing. `no_handler` cannot be recorded from a
/// live window (the slot is vacant only once the window let go of its
/// handler, after `closed`), so its row is written by hand and replayed
/// without asking.
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn every_close_answer_and_cause_replays() {
    use flui_native::journal::codec::{Backend, CloseAnswer, Stream};
    use flui_native::journal::recorder::Recorder;

    type Drive = fn(&HeadlessHostWindow, &RecordingWindowHandler);
    let request = |answer, queued| Row::CloseRequest { answer, queued };
    let cases: [(&str, Drive, Vec<Row>); 4] = [
        (
            "veto then grant",
            |window, handler| {
                let _ = veto_once(handler);
                assert!(!window.simulate_close());
                assert!(window.simulate_close());
            },
            vec![
                request(CloseAnswer::KeepOpen, false),
                request(CloseAnswer::Close, false),
                Row::Closed(CloseCause::GrantedRequest),
            ],
        ),
        (
            "a request queued behind the granted one",
            |window, handler| {
                let again = window.clone();
                handler.react_to_close_request(move || {
                    assert!(!again.simulate_close(), "a nested request is queued");
                    CloseDecision::Close
                });
                assert!(window.simulate_close());
            },
            vec![
                request(CloseAnswer::Close, false),
                request(CloseAnswer::Closing, true),
                Row::Closed(CloseCause::GrantedRequest),
            ],
        ),
        (
            "a handler that panics",
            |window, handler| {
                handler.react_to_close_request(|| panic!("the close guard fails"));
                #[expect(
                    clippy::disallowed_methods,
                    reason = "ADR-0287: the test lets a handler's panic reach it on purpose"
                )]
                let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    window.simulate_close()
                }));
                assert!(unwound.is_err(), "the handler's panic goes on");
            },
            vec![request(CloseAnswer::Panicked, false)],
        ),
        (
            "the application's command",
            |window, _| window.close_now(),
            vec![Row::Closed(CloseCause::NoRequest)],
        ),
    ];
    let owner = run_keeping_owner(Box::new(HeadlessPlatform::new()), &AppLog::new());
    let mut covered: Vec<Row> = Vec::new();
    for (case, drive, rows) in cases {
        covered.extend(rows.iter().cloned());
        let recorder = Recorder::start(Backend::Headless);
        let (window, handler) = headless_window(&owner);
        drive(&window, &handler);
        let recording = only(&recorder.finish(), Stream::Window);
        assert_eq!(recording.rows, rows, "{case}: what the capture journals");
        // The replaying handler decides each request it is asked as the live
        // one decided the requests it was asked, in order.
        let (window, replaying) = headless_window(&owner);
        let decisions: std::collections::VecDeque<CloseDecision> = rows
            .iter()
            .filter_map(|row| match row {
                Row::CloseRequest { answer, .. } => asked_decision(*answer),
                _ => None,
            })
            .collect();
        let decisions = RefCell::new(decisions);
        let asked = Rc::new(std::cell::Cell::new(0));
        let count = Rc::clone(&asked);
        replaying.react_to_close_request(move || {
            count.set(count.get() + 1);
            decisions
                .borrow_mut()
                .pop_front()
                .unwrap_or(CloseDecision::KeepOpen)
        });
        let replayed = recording.replay_into(&mut WindowTarget::new(window));
        if rows.contains(&request(CloseAnswer::Panicked, false)) {
            assert_eq!(
                replayed,
                Err(ReplayError::Incomplete {
                    line: 2,
                    row: "close_request"
                }),
                "{case}"
            );
            continue;
        }
        whole(replayed.unwrap_or_else(|error| panic!("{case}: the stream replays: {error}")));
        let recorded_asks = rows
            .iter()
            .filter(|row| {
                matches!(
                    row,
                    Row::CloseRequest {
                        answer: CloseAnswer::Close | CloseAnswer::KeepOpen,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(asked.get(), recorded_asks, "{case}: asked as often as live");
        assert_eq!(
            replaying.count(&HandlerCall::Closed),
            1,
            "{case}: closed once"
        );
    }

    let mut no_handler = NativeRecording::read(
        r#"{"flui_native_recording":4,"backend":"headless","stream":"window","devices":[]}"#
            .as_bytes(),
    )
    .expect("a window header reads");
    no_handler.rows = vec![
        request(CloseAnswer::NoHandler, false),
        Row::Closed(CloseCause::GrantedRequest),
    ];
    let (window, replaying) = headless_window(&owner);
    whole(
        no_handler
            .replay_into(&mut WindowTarget::new(window))
            .expect("a request no handler answered replays without asking"),
    );
    assert_eq!(replaying.count(&HandlerCall::CloseRequested), 0);
    assert_eq!(replaying.count(&HandlerCall::Closed), 1);

    // Every answer and every cause the vocabulary has has a case above: a new
    // one fails here until it is recorded and replayed.
    covered.extend(no_handler.rows);
    for answer in CloseAnswer::ALL {
        assert!(
            covered.iter().any(
                |row| matches!(row, Row::CloseRequest { answer: found, .. } if found == answer)
            ),
            "the close answer {answer:?} has no replay case"
        );
        let _ = asked_decision(*answer);
    }
    for cause in CloseCause::ALL {
        assert!(
            covered.contains(&Row::Closed(*cause)),
            "the close cause {cause:?} has no replay case"
        );
    }
}

/// What a replaying window's handler is asked to decide for a recorded
/// answer: the live decision where the live handler was asked, `None` where
/// it was not. Every answer is named: a new one panics here.
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn asked_decision(answer: flui_native::journal::codec::CloseAnswer) -> Option<CloseDecision> {
    use flui_native::journal::codec::CloseAnswer;

    match answer {
        CloseAnswer::Close => Some(CloseDecision::Close),
        CloseAnswer::KeepOpen => Some(CloseDecision::KeepOpen),
        CloseAnswer::Closing | CloseAnswer::NoHandler | CloseAnswer::Panicked => None,
        other => panic!("a close answer this table does not know: {other:?}"),
    }
}

/// A lock on a store that holds a secret (a protected field, or a one-time
/// code shown in clear), on each model, is journaled as a lock that happened
/// on a protected store and nothing derived from the field's content, with
/// text kept or not: no text, no range, no offset, no length; its replay is
/// refused as withheld.
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn a_protected_store_journals_nothing_of_its_content() {
    use flui_native::journal::recorder::Privacy;
    use flui_platform::text_store::{
        Autofill, NumberFormat, TextInputOptions, TextInputPurpose, TextStoreStatus,
    };

    let plain = TextStoreStatus::EDITABLE_SINGLE_LINE;
    let one_time_code = plain.with_text_input(
        TextInputOptions::new(TextInputPurpose::Number(NumberFormat::WHOLE))
            .autofill(Autofill::OneTimeCode),
    );
    for path in [
        StorePath::Tsf,
        StorePath::WebEditContext,
        StorePath::AndroidMirror,
    ] {
        for (privacy, status) in [
            (Privacy::KeepText, plain.with_protected(true)),
            (Privacy::Redact, plain.with_protected(true)),
            (Privacy::KeepText, one_time_code),
        ] {
            let (field, recording) = captured_store_session(path, privacy, status);
            assert!(
                field.text().contains('漢'),
                "{path:?}: the field took the text"
            );
            let mut written = Vec::new();
            recording.write(&mut written).expect("it writes");
            let written = String::from_utf8(written).expect("JSON Lines are UTF-8");
            let locks: Vec<&str> = written
                .lines()
                .filter(|line| line.contains("\"store_lock\""))
                .collect();
            assert!(!locks.is_empty(), "{path:?}: the locks are journaled");
            for lock in locks {
                assert!(
                    [
                        r#"{"row":"store_lock","lock":{"store":1,"kind":"read_write","protected":true}}"#,
                        r#"{"row":"store_lock","lock":{"store":1,"kind":"read","protected":true}}"#,
                    ]
                    .contains(&lock),
                    "{path:?} {privacy:?}: a protected lock holds nothing of the field: {lock}"
                );
            }
            let owner = run_keeping_owner(Box::new(HeadlessPlatform::new()), &AppLog::new());
            let (window, _) = headless_window(&owner);
            assert!(
                matches!(
                    recording.replay_into(&mut WindowTarget::new(window)),
                    Err(ReplayError::Redacted {
                        row: "store_lock",
                        ..
                    })
                ),
                "{path:?}"
            );
        }
    }
}

/// A store whose document is longer than a row holds is never read whole by
/// the journal, and a capture whose sink wants no rows reads nothing and
/// journals no lock.
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn the_journal_reads_no_document_it_cannot_hold_or_nobody_wants() {
    use flui_native::journal::codec::{Backend, Stream};
    use flui_native::journal::lines::{Forms, LineSink, StreamLine, capture_lines};
    use flui_native::journal::recorder::Recorder;
    use flui_native::journal::store::MAX_STORE_TEXT_BYTES;
    use flui_platform::text_store::{LockGrant, LockTiming, Selection, Utf16Offset};

    /// Opens a headless window, focuses `field` through its journaled host
    /// counting the reads of its text, and sets the caret in one lock.
    fn set_caret(field: Rc<InMemoryTextStore>) -> usize {
        let platform =
            HeadlessPlatform::new().with_text_store_host(|| Rc::new(ServedStore) as Rc<_>);
        let owner = run_keeping_owner(Box::new(platform), &AppLog::new());
        let window = owner
            .open_window(WindowOptions::default(), RecordingWindowHandler::new())
            .expect("a headless window opens")
            .try_ready()
            .expect("at once");
        let host = owner.text_store_host(&window).expect("a store host");
        let counted = Rc::new(CountedReads::of(field));
        host.focus_store(Some(Rc::clone(&counted) as Rc<dyn TextStore>));
        let served = SERVED
            .with(|served| served.borrow().clone())
            .expect("the host serves the focused store");
        let _granted = served
            .request_lock(
                LockGrant::read_write(|session| {
                    let _ = session.set_selection(Selection::collapsed(Utf16Offset::ZERO));
                }),
                LockTiming::Sync,
            )
            .expect("a free store grants a sync lock");
        counted.reads.get()
    }

    let recorder = Recorder::start(Backend::Headless);
    let large = InMemoryTextStore::new("x".repeat(MAX_STORE_TEXT_BYTES + 1));
    assert_eq!(set_caret(large), 0, "a document past the bound is not read");
    let recording = only(&recorder.finish(), Stream::Window);
    assert!(
        recording
            .rows
            .iter()
            .any(|row| matches!(row, Row::StoreLock { lock, .. } if lock.incomplete && lock.opened.is_none())),
        "the lock is journaled incomplete"
    );

    /// A sink that wants no row, and counts the lines it is handed anyway.
    struct Unwanted(std::sync::Arc<std::sync::atomic::AtomicUsize>);

    impl LineSink for Unwanted {
        fn wants(&self) -> Forms {
            Forms::default()
        }

        fn line(&mut self, line: StreamLine<'_>) {
            if !line.setup {
                self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }

        fn lost(&mut self, _stream: u32) {}
    }
    let rows = std::sync::Arc::default();
    let capture = capture_lines(Backend::Headless, Unwanted(std::sync::Arc::clone(&rows)));
    assert_eq!(
        set_caret(InMemoryTextStore::new("ab")),
        0,
        "nobody wants the lock: the document is not read"
    );
    drop(capture);
    assert_eq!(
        rows.load(std::sync::atomic::Ordering::Relaxed),
        0,
        "and no row was made of it"
    );
}

/// A text store that counts how often a session reads its text, standing
/// for the field an input method's lock reaches.
#[cfg(all(debug_assertions, feature = "native-capture"))]
struct CountedReads {
    inner: Rc<InMemoryTextStore>,
    reads: Rc<std::cell::Cell<usize>>,
    /// Its status panics while set, as application code may.
    panic_status: std::cell::Cell<bool>,
}

#[cfg(all(debug_assertions, feature = "native-capture"))]
impl CountedReads {
    fn of(inner: Rc<InMemoryTextStore>) -> Self {
        Self {
            inner,
            reads: Rc::default(),
            panic_status: std::cell::Cell::new(false),
        }
    }
}

/// A session of a [`CountedReads`] store.
#[cfg(all(debug_assertions, feature = "native-capture"))]
struct CountedSession<'a, S: ?Sized> {
    session: &'a mut S,
    reads: &'a std::cell::Cell<usize>,
}

#[cfg(all(debug_assertions, feature = "native-capture"))]
impl<S: flui_platform::text_store::TextStoreRead + ?Sized> flui_platform::text_store::TextStoreRead
    for CountedSession<'_, S>
{
    fn document_len(&self) -> flui_platform::text_store::Utf16Offset {
        self.session.document_len()
    }

    fn text(
        &self,
        range: flui_platform::text_store::Utf16Range,
    ) -> Result<String, flui_platform::text_store::TextStoreError> {
        self.reads.set(self.reads.get() + 1);
        self.session.text(range)
    }

    fn selection(&self) -> flui_platform::text_store::Selection {
        self.session.selection()
    }

    fn composition(&self) -> Option<flui_platform::text_store::Composition> {
        self.session.composition()
    }

    fn rect_for_range(
        &self,
        range: flui_platform::text_store::Utf16Range,
    ) -> Result<flui_platform::text_store::RangeRect, flui_platform::text_store::TextStoreError>
    {
        self.session.rect_for_range(range)
    }

    fn document_bounds(
        &self,
    ) -> Result<flui_foundation::geometry::Bounds<f64>, flui_platform::text_store::TextStoreError>
    {
        self.session.document_bounds()
    }

    fn index_at_point(
        &self,
        point: flui_foundation::geometry::Point<f64>,
        mode: flui_platform::text_store::PointMode,
    ) -> Result<flui_platform::text_store::Utf16Offset, flui_platform::text_store::TextStoreError>
    {
        self.session.index_at_point(point, mode)
    }
}

#[cfg(all(debug_assertions, feature = "native-capture"))]
impl<S: flui_platform::text_store::TextStoreEdit + ?Sized> flui_platform::text_store::TextStoreEdit
    for CountedSession<'_, S>
{
    fn replace(
        &mut self,
        range: flui_platform::text_store::Utf16Range,
        text: &str,
    ) -> Result<flui_platform::text_store::TextChange, flui_platform::text_store::TextStoreError>
    {
        self.session.replace(range, text)
    }

    fn insert_at_selection(
        &mut self,
        text: &str,
    ) -> Result<flui_platform::text_store::TextChange, flui_platform::text_store::TextStoreError>
    {
        self.session.insert_at_selection(text)
    }

    fn set_selection(
        &mut self,
        selection: flui_platform::text_store::Selection,
    ) -> Result<(), flui_platform::text_store::TextStoreError> {
        self.session.set_selection(selection)
    }

    fn set_composition(
        &mut self,
        composition: Option<flui_platform::text_store::Composition>,
    ) -> Result<(), flui_platform::text_store::TextStoreError> {
        self.session.set_composition(composition)
    }
}

#[cfg(all(debug_assertions, feature = "native-capture"))]
impl TextStore for CountedReads {
    fn status(&self) -> flui_platform::text_store::TextStoreStatus {
        assert!(!self.panic_status.get(), "the application's status fails");
        self.inner.status()
    }

    fn request_lock(
        &self,
        grant: flui_platform::text_store::LockGrant,
        timing: flui_platform::text_store::LockTiming,
    ) -> Result<flui_platform::text_store::LockOutcome, flui_platform::text_store::TextStoreError>
    {
        use flui_platform::text_store::LockGrant;

        let reads = Rc::clone(&self.reads);
        // The test's locks are read-write: a read lock is passed on uncounted.
        let grant = match grant {
            LockGrant::Read(body) => LockGrant::Read(body),
            LockGrant::ReadWrite(body) => LockGrant::read_write(move |session| {
                body(&mut CountedSession {
                    session,
                    reads: &reads,
                });
            }),
        };
        self.inner.request_lock(grant, timing)
    }

    fn run_deferred_grants(&self) -> usize {
        self.inner.run_deferred_grants()
    }

    fn set_commit_gate(&self, gate: flui_platform::text_store::CommitGate) {
        self.inner.set_commit_gate(gate);
    }

    fn set_observer(&self, observer: Option<Rc<dyn flui_platform::text_store::TextStoreObserver>>) {
        self.inner.set_observer(observer);
    }
}

/// A protected field journals one row for as long as it stays protected,
/// whatever number of locks its input method was granted: a recording of one
/// keystroke and one of twenty are the same bytes.
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn a_protected_store_journals_one_row_whatever_was_typed() {
    use flui_native::journal::codec::{Backend, Stream};
    use flui_native::journal::recorder::{Privacy, Recorder};
    use flui_native::text::conformance::TsfDocument;

    let typed = |keys: usize, privacy| {
        let recorder = Recorder::with_privacy(Backend::Headless, privacy);
        let (_owner, _window, served) = served_store(InMemoryTextStore::new(""), true);
        let document = TsfDocument::focus(served);
        for key in 0..keys {
            document.edit(move |session| session.set_text(key, key, "x"));
        }
        let mut written = Vec::new();
        only(&recorder.finish(), Stream::Window)
            .write(&mut written)
            .expect("it writes");
        written
    };
    for privacy in [Privacy::KeepText, Privacy::Redact] {
        assert_eq!(
            String::from_utf8(typed(1, privacy)),
            String::from_utf8(typed(20, privacy)),
            "{privacy:?}: the recording does not count the keys typed"
        );
    }
}

/// A headless window serving `field` through its journaled host, the field
/// protected or not, and the store the host's input method reaches.
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn served_store(
    field: Rc<InMemoryTextStore>,
    protected: bool,
) -> (
    flui_native::OwnerPlatform,
    Rc<dyn flui_native::HostWindow>,
    Rc<dyn TextStore>,
) {
    field.set_protected(protected);
    let platform = HeadlessPlatform::new().with_text_store_host(|| Rc::new(ServedStore) as Rc<_>);
    let owner = run_keeping_owner(Box::new(platform), &AppLog::new());
    let window = owner
        .open_window(WindowOptions::default(), RecordingWindowHandler::new())
        .expect("a headless window opens")
        .try_ready()
        .expect("at once");
    let host = owner.text_store_host(&window).expect("a store host");
    host.focus_store(Some(field as Rc<dyn TextStore>));
    let served = SERVED
        .with(|served| served.borrow().clone())
        .expect("the host serves the focused store");
    (owner, window, served)
}

/// A store whose status panics while a lock is granted on it: the journal
/// contains the panic, the input method's body still runs, and the panic goes
/// on after it, the first failure (the store then treats the lock as it treats any that unwound).
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn the_granted_body_runs_when_the_store_status_panics() {
    use flui_native::journal::codec::Backend;
    use flui_native::journal::recorder::Recorder;
    use flui_platform::text_store::{LockGrant, LockTiming, Utf16Offset, Utf16Range};

    let recorder = Recorder::start(Backend::Headless);
    let field = InMemoryTextStore::new("ab");
    let counted = Rc::new(CountedReads::of(Rc::clone(&field)));
    let (_owner, _window, served) = served_store_of(Rc::clone(&counted) as Rc<dyn TextStore>);
    let read = || {
        let _granted = served
            .request_lock(
                LockGrant::read(|session| {
                    let _ = session.selection();
                }),
                LockTiming::Sync,
            )
            .expect("a free store grants a sync read");
    };
    // The recording holds the document after this lock.
    read();
    counted.panic_status.set(true);
    let ran = Rc::new(std::cell::Cell::new(false));
    let body_ran = Rc::clone(&ran);
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: the test lets the store's panic reach it on purpose"
    )]
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        served.request_lock(
            LockGrant::read_write(move |session| {
                let end = Utf16Range::collapsed(Utf16Offset::new(2));
                let _ = session.replace(end, "c");
                body_ran.set(true);
            }),
            LockTiming::Sync,
        )
    }));
    counted.panic_status.set(false);
    assert!(ran.get(), "the granted body ran");
    assert!(
        unwound.is_err(),
        "the status's panic went on after the body"
    );
    // The lock ran unrecorded, so the recording no longer holds the document:
    // the next lock records it again.
    read();
    let opened: Vec<bool> = only(
        &recorder.finish(),
        flui_native::journal::codec::Stream::Window,
    )
    .rows
    .iter()
    .filter_map(|row| match row {
        Row::StoreLock { lock, .. } => Some(lock.opened.is_some()),
        _ => None,
    })
    .collect();
    assert_eq!(
        opened,
        [true, true],
        "the lock after an unrecorded one records the document"
    );
}

/// A store whose status panics as the window is told to serve it: the host
/// still serves it, unjournaled, and the panic goes on after the host was
/// told.
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn the_host_is_told_of_a_focus_when_the_store_status_panics() {
    use flui_native::journal::codec::Backend;
    use flui_native::journal::recorder::Recorder;

    let _recorder = Recorder::start(Backend::Headless);
    let counted = Rc::new(CountedReads::of(InMemoryTextStore::new("ab")));
    counted.panic_status.set(true);
    SERVED.with(|served| served.replace(None));
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: the test lets the store's panic reach it on purpose"
    )]
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        served_store_of(Rc::clone(&counted) as Rc<dyn TextStore>)
    }));
    counted.panic_status.set(false);
    assert!(unwound.is_err(), "the status's panic went on");
    let served = SERVED
        .with(|served| served.borrow().clone())
        .expect("the host was told to serve the store");
    assert!(
        std::ptr::addr_eq(Rc::as_ptr(&served), Rc::as_ptr(&counted)),
        "it serves the store itself, unjournaled, as the focus row could not be written"
    );
}

/// [`served_store`] for a store that is not an [`InMemoryTextStore`].
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn served_store_of(
    store: Rc<dyn TextStore>,
) -> (
    flui_native::OwnerPlatform,
    Rc<dyn flui_native::HostWindow>,
    Rc<dyn TextStore>,
) {
    let platform = HeadlessPlatform::new().with_text_store_host(|| Rc::new(ServedStore) as Rc<_>);
    let owner = run_keeping_owner(Box::new(platform), &AppLog::new());
    let window = owner
        .open_window(WindowOptions::default(), RecordingWindowHandler::new())
        .expect("a headless window opens")
        .try_ready()
        .expect("at once");
    let host = owner.text_store_host(&window).expect("a store host");
    host.focus_store(Some(store));
    let served = SERVED
        .with(|served| served.borrow().clone())
        .expect("the host serves the focused store");
    (owner, window, served)
}

/// A store nobody observes has its document recorded at a lock only when it
/// changed since the recording last held it: read-only locks write it once,
/// an edit makes the next lock write it again.
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn an_unchanged_document_is_not_written_again() {
    use flui_native::journal::codec::{Backend, Stream};
    use flui_native::journal::recorder::{Privacy, Recorder};
    use flui_platform::text_store::{LockGrant, LockTiming, Utf16Offset, Utf16Range};

    let recorder = Recorder::with_privacy(Backend::Headless, Privacy::KeepText);
    let (_owner, _window, served) = served_store(InMemoryTextStore::new("ab"), false);
    served.set_observer(None);
    let read = || {
        let _granted = served
            .request_lock(
                LockGrant::read(|session| {
                    let _ = session.selection();
                }),
                LockTiming::Sync,
            )
            .expect("a free store grants a sync read");
    };
    read();
    read();
    let _granted = served
        .request_lock(
            LockGrant::read_write(|session| {
                let _ = session.replace(Utf16Range::collapsed(Utf16Offset::new(2)), "c");
            }),
            LockTiming::Sync,
        )
        .expect("a free store grants a sync lock");
    read();
    let opened: Vec<Option<String>> = only(&recorder.finish(), Stream::Window)
        .rows
        .iter()
        .filter_map(|row| match row {
            Row::StoreLock { lock, .. } => {
                Some(lock.opened.as_ref().map(|opened| opened.text.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        opened,
        [Some("ab".to_owned()), None, None, Some("abc".to_owned())],
        "found once, held while unchanged, found again after the edit"
    );
}

/// A live recording holds rows only while they fit the bounds its reader
/// holds it to: the first row past them is replaced by one `cut` row, and
/// nothing the window delivers after it is kept, so what a capture holds
/// stays bounded however long it runs.
#[cfg(all(debug_assertions, feature = "native-capture"))]
fn a_live_recording_past_its_bound_holds_one_cut_and_nothing_after() {
    use flui_native::journal::codec::{Backend, CutReason, Stream};
    use flui_native::journal::recorder::{Privacy, Recorder};
    use flui_protocol::native_recording::{MAX_LINE_BYTES, MAX_RECORDING_BYTES};

    let recorder = Recorder::with_privacy(Backend::Headless, Privacy::KeepText);
    let owner = run_keeping_owner(Box::new(HeadlessPlatform::new()), &AppLog::new());
    let (window, _handler) = headless_window(&owner);
    let commit = "x".repeat(MAX_LINE_BYTES - 128);
    let past_the_bound = MAX_RECORDING_BYTES / commit.len() + 2;
    let commit_all = |count: usize| {
        for _ in 0..count {
            window.inject_event(PlatformInput::Ime(ImeEvent::Commit(commit.clone())));
        }
    };
    let held = |recorder: &Recorder| {
        let journals = recorder.journals().snapshot();
        let rows = &only(&journals, Stream::Window).rows;
        let cuts = rows.iter().filter(|row| matches!(row, Row::Cut(_))).count();
        (rows.len(), cuts, rows.last().cloned())
    };
    commit_all(past_the_bound);
    let (rows, cuts, last) = held(&recorder);
    assert!(
        rows < past_the_bound,
        "the rows past the bound were not kept"
    );
    assert_eq!(cuts, 1, "one cut stands for them");
    assert_eq!(last, Some(Row::Cut(CutReason::Bytes)));
    commit_all(4);
    assert_eq!(
        held(&recorder),
        (rows, 1, last),
        "nothing is kept after the cut"
    );

    let recordings = recorder.finish();
    let recording = only(&recordings, Stream::Window);
    let mut written = Vec::new();
    recording
        .write(&mut written)
        .expect("a cut recording writes");
    assert!(written.len() <= MAX_RECORDING_BYTES, "what it holds fits");
    let read = NativeRecording::read(written.as_slice()).expect("and its reader reads it");
    assert_eq!(read.rows, recording.rows, "written as it was held");
}

/// The capture cases of the streams beside a registry's.
#[cfg(all(debug_assertions, feature = "native-capture"))]
pub(super) const CAPTURE_CASES: [(&str, fn()); 11] = [
    (
        "a_live_recording_past_its_bound_holds_one_cut_and_nothing_after",
        a_live_recording_past_its_bound_holds_one_cut_and_nothing_after,
    ),
    (
        "the_host_is_told_of_a_focus_when_the_store_status_panics",
        the_host_is_told_of_a_focus_when_the_store_status_panics,
    ),
    (
        "a_protected_store_journals_one_row_whatever_was_typed",
        a_protected_store_journals_one_row_whatever_was_typed,
    ),
    (
        "the_granted_body_runs_when_the_store_status_panics",
        the_granted_body_runs_when_the_store_status_panics,
    ),
    (
        "an_unchanged_document_is_not_written_again",
        an_unchanged_document_is_not_written_again,
    ),
    (
        "a_protected_store_journals_nothing_of_its_content",
        a_protected_store_journals_nothing_of_its_content,
    ),
    (
        "the_journal_reads_no_document_it_cannot_hold_or_nobody_wants",
        the_journal_reads_no_document_it_cannot_hold_or_nobody_wants,
    ),
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
        "every_close_answer_and_cause_replays",
        every_close_answer_and_cause_replays,
    ),
];
