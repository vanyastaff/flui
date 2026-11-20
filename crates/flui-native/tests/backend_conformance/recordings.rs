//! The replay behind the conformance table's recording rows (ADR-0215 §4, ADR-0390): a session
//! of one kind written as a backend's native journal records it, read back through the
//! format's reader and replayed on a fresh headless host, through the targets the checked-in
//! family recordings replay into (`native_recording::families`). What the headless host's
//! application, window and platform then deliver is what the row compares with the session.
//!
//! A [`Fault`] breaks the headless host's handling of one kind, so each row shows it can fail.

use std::collections::HashMap;

use flui_native::HeadlessPlatform;
use flui_native::journal::codec::{
    Backend, DisplayRead, Header, NativeRecording, PreferenceRead, RecordedDisplay,
    RecordedWindowState, ReplayTarget, Row, Stream, Unsupported,
};
use flui_native::testing::app::{AppCall, AppLog, run_keeping_owner};
use flui_platform::{ImeEvent, SessionEndPhase, SystemNotice};
use flui_testing::backend_conformance::{Recorded, RecordingReplay};

use super::Fault;
use crate::native_recording::families::{
    HostEventsTarget, Labels, PlatformTarget, Read, Sources, WindowStateTarget, WindowTarget,
    headless_window, ime_events, whole,
};

/// Write `session` as `backend`'s journal records it, one recording per stream in the order the
/// session first reaches each, read each back and replay it on a fresh headless host, broken by
/// `fault`: what the host delivered, stream by stream.
pub(super) fn replay_on_headless(
    backend: Backend,
    fault: Option<Fault>,
    session: &[Recorded],
) -> RecordingReplay {
    let mut streams: Vec<(Stream, Vec<Row>)> = Vec::new();
    let mut labels: HashMap<String, u64> = HashMap::new();
    for (index, entry) in session.iter().enumerate() {
        let (stream, row) = row_of(entry, index, &mut labels);
        match streams.iter_mut().find(|(open, _)| *open == stream) {
            Some((_, rows)) => rows.push(row),
            None => streams.push((stream, vec![row])),
        }
    }
    let platform = HeadlessPlatform::new();
    let mut events = Some(platform.host_events());
    let mut sources = Some(Sources::of(&platform));
    let log = AppLog::new();
    let owner = run_keeping_owner(Box::new(platform), &log);
    let labels = Labels::default();
    let mut heard = Vec::new();
    for (stream, rows) in streams {
        let recording = written_and_read(NativeRecording {
            header: Header::for_stream(backend, stream),
            rows,
        });
        match stream {
            Stream::App => {
                let events = events
                    .take()
                    .expect("BUG: a session's lifecycle is one application stream");
                let mut target = Broken::new(HostEventsTarget(events), fault);
                whole(
                    recording
                        .replay_into(&mut target)
                        .expect("an application stream replays on the headless host"),
                );
                heard.extend(log.calls().into_iter().filter_map(|call| match call {
                    AppCall::Notice(notice) => Some(Recorded::Notice(notice)),
                    AppCall::SessionEnd(phase) => Some(Recorded::SessionEnd(phase)),
                    AppCall::Reopen => Some(Recorded::Reopen),
                    _ => None,
                }));
            }
            Stream::Window => {
                let (window, handler) = headless_window(&owner);
                let mut target = Broken::new(WindowTarget::new(window), fault);
                whole(
                    recording
                        .replay_into(&mut target)
                        .expect("a window stream replays on a headless window"),
                );
                heard.extend(ime_events(&handler).into_iter().map(Recorded::Ime));
            }
            Stream::Platform => {
                let sources = sources
                    .take()
                    .expect("BUG: a session's reads are one platform stream");
                let mut target = Broken::new(PlatformTarget::new(sources, &owner, &labels), fault);
                whole(
                    recording
                        .replay_into(&mut target)
                        .expect("a platform stream replays on the headless host"),
                );
                heard.extend(target.inner.reads.into_iter().map(|read| match read {
                    Read::Preferences(Ok(values)) => Recorded::Preferences(Box::new(values)),
                    Read::Displays(Ok(displays)) => {
                        Recorded::Displays(displays.into_iter().map(|(_, parts)| parts).collect())
                    }
                    failed @ (Read::Preferences(Err(_)) | Read::Displays(Err(_))) => {
                        panic!("a replayed read the session observed failed: {failed:?}")
                    }
                }));
            }
            Stream::WindowState => {
                let (window, _handler) = headless_window(&owner);
                let mut target = Broken::new(WindowStateTarget::new(window, &labels), fault);
                whole(
                    recording
                        .replay_into(&mut target)
                        .expect("a window state stream replays on a headless window"),
                );
                heard.extend(
                    target
                        .inner
                        .published
                        .into_iter()
                        .map(|state| Recorded::WindowState(Box::new(state.into_parts()))),
                );
            }
            other => panic!("BUG: no recorded kind journals into a {other:?} stream"),
        }
    }
    RecordingReplay::Replayed(heard)
}

/// The stream `entry` is journaled into and its row there, the `index`th of its session. A
/// display is labelled by its name's first sight within the session, as a journal labels it by
/// the id its registry issued.
fn row_of(entry: &Recorded, index: usize, labels: &mut HashMap<String, u64>) -> (Stream, Row) {
    match entry {
        Recorded::Ime(event) => (
            Stream::Window,
            Row::Ime {
                event: event.clone(),
                redacted: false,
            },
        ),
        Recorded::Notice(notice) => (Stream::App, Row::Notice(*notice)),
        Recorded::SessionEnd(phase) => (Stream::App, Row::SessionEnd(*phase)),
        Recorded::Reopen => (Stream::App, Row::Reopen),
        Recorded::WindowState(parts) => (
            Stream::WindowState,
            Row::State {
                state: RecordedWindowState::of((**parts).clone().into_state()),
                redacted: false,
            },
        ),
        Recorded::Preferences(values) => (
            Stream::Platform,
            Row::Preferences(PreferenceRead::Observed((**values).clone())),
        ),
        Recorded::Displays(displays) => {
            let displays = displays
                .iter()
                .map(|parts| {
                    let next = labels.len() as u64 + 1;
                    let label = *labels.entry(parts.name.clone()).or_insert(next);
                    RecordedDisplay {
                        label,
                        parts: parts.clone(),
                    }
                })
                .collect();
            (
                Stream::Platform,
                Row::Displays {
                    read: DisplayRead::Observed {
                        revision: index as u64 + 1,
                        displays,
                    },
                    redacted: false,
                },
            )
        }
        other => panic!("BUG: the table records no {other:?}"),
    }
}

/// `recording` written and read back, as a file of it is.
fn written_and_read(recording: NativeRecording) -> NativeRecording {
    let mut written = Vec::new();
    recording
        .write(&mut written)
        .expect("a recording of the session writes");
    let read = NativeRecording::read(written.as_slice()).expect("the recording reads back");
    assert_eq!(
        read, recording,
        "the recording reads back as it was written"
    );
    read
}

/// A replay target whose headless host handles one kind as `fault` breaks it; without a fault,
/// the target it wraps.
struct Broken<T> {
    inner: T,
    fault: Option<Fault>,
    /// The window snapshots handed on so far.
    snapshots: usize,
    /// The first preference read handed on.
    first_preferences: Option<PreferenceRead>,
}

impl<T> Broken<T> {
    const fn new(inner: T, fault: Option<Fault>) -> Self {
        Self {
            inner,
            fault,
            snapshots: 0,
            first_preferences: None,
        }
    }
}

impl<T: ReplayTarget> ReplayTarget for Broken<T> {
    fn notice(&mut self, notice: SystemNotice) -> Result<(), Unsupported> {
        self.inner.notice(notice)
    }

    fn session_end(&mut self, phase: SessionEndPhase) -> Result<(), Unsupported> {
        self.inner.session_end(phase)
    }

    fn reopen(&mut self) -> Result<(), Unsupported> {
        if self.fault == Some(Fault::ReplayedReopenLost) {
            return Ok(());
        }
        self.inner.reopen()
    }

    fn ime(&mut self, event: &ImeEvent) -> Result<(), Unsupported> {
        if self.fault == Some(Fault::ReplayedCommitLost) && matches!(event, ImeEvent::Commit(_)) {
            return Ok(());
        }
        self.inner.ime(event)
    }

    fn preferences(&mut self, read: &PreferenceRead) -> Result<(), Unsupported> {
        if self.fault == Some(Fault::ReplayedPreferencesStale) {
            // The source keeps what the first read set: every later read answers it again.
            let first = self
                .first_preferences
                .get_or_insert_with(|| read.clone())
                .clone();
            return self.inner.preferences(&first);
        }
        self.inner.preferences(read)
    }

    fn displays(&mut self, read: &DisplayRead) -> Result<(), Unsupported> {
        match (self.fault, read) {
            (
                Some(Fault::ReplayedDisplaysReversed),
                DisplayRead::Observed { revision, displays },
            ) => {
                let reversed = DisplayRead::Observed {
                    revision: *revision,
                    displays: displays.iter().rev().cloned().collect(),
                };
                self.inner.displays(&reversed)
            }
            _ => self.inner.displays(read),
        }
    }

    fn window_state(&mut self, state: &RecordedWindowState) -> Result<(), Unsupported> {
        self.snapshots += 1;
        if self.fault == Some(Fault::ReplayedSnapshotUnpublished) && self.snapshots == 2 {
            return Ok(());
        }
        self.inner.window_state(state)
    }
}
