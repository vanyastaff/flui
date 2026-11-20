//! The recording rows' backend side (ADR-0215 §4, ADR-0390): a session of one kind delivered
//! through the delivery code that journals it, with a capture of the backend installed, then
//! the recordings that capture made written, read back and replayed on a fresh headless host.
//!
//! The decoders of the table have no OS delivery of their own: on every native host the same
//! code opens and feeds the journaled streams (`owner/app_delivery.rs` for notices, session ends
//! and reopens; `owner/window_delivery.rs` for the input method's events; `platform.rs` for the
//! reads of the preferences and the displays; `window/state.rs` for the window's snapshots), and
//! the headless host runs that code. So each backend's session goes through the headless host's
//! delivery with a capture of that backend installed before the platform starts, as its runner
//! installs it. A [`Fault`] breaks what the backend does with its journal or its delivery: a
//! capture installed after the platform, a journal that redacts, a reopen it never delivers.

use flui_native::journal::codec::{Backend, NativeRecording, Stream};
use flui_native::journal::recorder::{Privacy, Recorder};
use flui_native::testing::app::{AppCall, AppLog, run_keeping_owner};
use flui_native::{HeadlessPlatform, OwnerPlatform};
use flui_platform::{DisplayKey, DisplayRegistry, PlatformInput};
use flui_testing::backend_conformance::{Recorded, RecordingReplay};

use super::Fault;
use crate::native_recording::families::{
    HostEventsTarget, Labels, PlatformTarget, Read, Sources, WindowStateTarget, WindowTarget,
    headless_window, ime_events, read_displays, read_preferences,
};

/// Deliver `session` through the journaled delivery code as `backend` does, broken by `fault`,
/// with a capture of `backend` installed; then replay what the capture recorded on a fresh
/// headless host.
pub(super) fn journal_and_replay(
    backend: Backend,
    fault: Option<Fault>,
    session: &[Recorded],
) -> RecordingReplay {
    let privacy = if fault == Some(Fault::RedactingJournal) {
        Privacy::Redact
    } else {
        Privacy::KeepText
    };
    let late = fault == Some(Fault::JournalInstalledLate);
    // Before the platform, as a runner installs it: a stream reports only to the capture present
    // when it opened.
    let early = (!late).then(|| Recorder::with_privacy(backend, privacy));
    let platform = HeadlessPlatform::new();
    let events = platform.host_events();
    let sources = Sources::of(&platform);
    let log = AppLog::new();
    let owner = run_keeping_owner(Box::new(platform), &log);
    let (window, handler) = headless_window(&owner);
    let recorder = early.unwrap_or_else(|| Recorder::with_privacy(backend, privacy));

    let displays = DisplayRegistry::default();
    let mut reads = Vec::new();
    let mut states = Vec::new();
    for entry in session {
        match entry {
            Recorded::Ime(event) => {
                let _delivered = window.inject_event(PlatformInput::Ime(event.clone()));
            }
            Recorded::Notice(notice) => events.notice(*notice),
            Recorded::SessionEnd(phase) => {
                let _answer = events.session_end(*phase);
            }
            Recorded::Reopen if fault == Some(Fault::ReopenUntranslated) => {}
            Recorded::Reopen => events.reopen(),
            Recorded::WindowState(parts) => {
                states.push(window.simulate_state((**parts).clone().into_state()));
            }
            Recorded::Preferences(values) => {
                sources.preferences.set((**values).clone());
                reads.push(Read::Preferences(read_preferences(&owner)));
            }
            Recorded::Displays(parts) => {
                // The OS's identity of each display, which the session numbers.
                let described = parts
                    .iter()
                    .map(|(seen, parts)| {
                        let id = displays
                            .mint(&DisplayKey::Injected(*seen as u64 + 1))
                            .expect("a test registry has ids for its displays");
                        parts.clone().into_description(id)
                    })
                    .collect();
                sources
                    .displays
                    .replace(described)
                    .expect("the headless snapshot advances");
                reads.push(Read::Displays(read_displays(
                    &owner,
                    flui_platform::DisplayId::get,
                )));
            }
            other => panic!("BUG: the table delivers no {other:?}"),
        }
    }
    let live = heard(
        session,
        app_entries(&log.calls()),
        ime_entries(&handler),
        states,
        reads,
    );
    replay(recorder.finish(), session, live)
}

/// The input method events `handler` heard, as entries.
fn ime_entries(handler: &flui_testing::window_handler::RecordingWindowHandler) -> Vec<Recorded> {
    ime_events(handler).into_iter().map(Recorded::Ime).collect()
}

/// The notices, session end phases and reopens among an application's `calls`, as entries.
fn app_entries(calls: &[AppCall]) -> Vec<Recorded> {
    calls
        .iter()
        .filter_map(|call| match call {
            AppCall::Notice(notice) => Some(Recorded::Notice(*notice)),
            AppCall::SessionEnd(phase) => Some(Recorded::SessionEnd(*phase)),
            AppCall::Reopen => Some(Recorded::Reopen),
            _ => None,
        })
        .collect()
}

/// What the application heard, in the order of the streams `session` reaches: its calls, the
/// window's input method events, the snapshots it published and its reads.
fn heard(
    session: &[Recorded],
    mut app: Vec<Recorded>,
    mut ime: Vec<Recorded>,
    states: Vec<flui_platform::WindowState>,
    reads: Vec<Read>,
) -> Vec<Recorded> {
    let mut states: Vec<Recorded> = states
        .into_iter()
        .map(|state| Recorded::WindowState(Box::new(state.into_parts())))
        .collect();
    let mut reads = entries_of(reads);
    let mut heard = Vec::new();
    for stream in streams_of(session) {
        heard.append(match stream {
            Stream::App => &mut app,
            Stream::Window => &mut ime,
            Stream::WindowState => &mut states,
            _ => &mut reads,
        });
    }
    heard
}

/// The streams `session` reaches, each once, in the order it first reaches them.
fn streams_of(session: &[Recorded]) -> Vec<Stream> {
    let mut streams = Vec::new();
    for entry in session {
        let stream = match entry {
            Recorded::Ime(_) => Stream::Window,
            Recorded::WindowState(_) => Stream::WindowState,
            Recorded::Preferences(_) | Recorded::Displays(_) => Stream::Platform,
            _ => Stream::App,
        };
        if !streams.contains(&stream) {
            streams.push(stream);
        }
    }
    streams
}

/// `reads` as entries, each display numbered by the order in which these reads first saw its
/// id (the reading platform's id, as the label each read names it by); a failed read is not one
/// the table makes.
fn entries_of(reads: Vec<Read>) -> Vec<Recorded> {
    let mut seen: Vec<u64> = Vec::new();
    reads
        .into_iter()
        .map(|read| match read {
            Read::Preferences(Ok(values)) => Recorded::Preferences(Box::new(values)),
            Read::Displays(Ok(displays)) => Recorded::Displays(
                displays
                    .into_iter()
                    .map(|(label, parts)| {
                        let index = seen.iter().position(|known| *known == label);
                        let index = index.unwrap_or_else(|| {
                            seen.push(label);
                            seen.len() - 1
                        });
                        (index, parts)
                    })
                    .collect(),
            ),
            failed @ (Read::Preferences(Err(_)) | Read::Displays(Err(_))) => {
                panic!("a read the session made failed: {failed:?}")
            }
        })
        .collect()
}

/// Write each of `recordings` of the streams `session` reaches, read it back and replay it on a
/// fresh headless host: what the application heard there, beside `live`.
fn replay(
    recordings: Vec<NativeRecording>,
    session: &[Recorded],
    live: Vec<Recorded>,
) -> RecordingReplay {
    let platform = HeadlessPlatform::new();
    let mut events = Some(platform.host_events());
    let mut sources = Some(Sources::of(&platform));
    let log = AppLog::new();
    let owner = run_keeping_owner(Box::new(platform), &log);
    let labels = Labels::default();
    let mut replayed = Vec::new();
    for stream in streams_of(session) {
        let mut journaled = recordings
            .iter()
            .filter(|recording| recording.header.stream == stream);
        let Some(recording) = journaled.next() else {
            return RecordingReplay::Refused {
                live,
                why: format!("the backend journaled no {stream:?} stream"),
            };
        };
        assert!(
            journaled.next().is_none(),
            "a session opens one {stream:?} stream"
        );
        let recording = match written_and_read(recording) {
            Ok(recording) => recording,
            Err(why) => return RecordingReplay::Refused { live, why },
        };
        let replay = replay_stream(
            &recording,
            stream,
            &owner,
            &log,
            &labels,
            &mut events,
            &mut sources,
        );
        match replay {
            Ok(heard) => replayed.extend(heard),
            Err(why) => return RecordingReplay::Refused { live, why },
        }
    }
    RecordingReplay::Replayed { live, replayed }
}

/// `recording` written and read back, as a file of it is.
fn written_and_read(recording: &NativeRecording) -> Result<NativeRecording, String> {
    let mut written = Vec::new();
    recording
        .write(&mut written)
        .map_err(|error| format!("the recording does not write: {error}"))?;
    let read = NativeRecording::read(written.as_slice())
        .map_err(|error| format!("the recording does not read back: {error}"))?;
    if read != *recording {
        return Err("the recording reads back otherwise than it was written".to_owned());
    }
    Ok(read)
}

/// Replay `recording` of `stream` on the headless host `owner` runs: what its application,
/// window or platform delivered.
fn replay_stream(
    recording: &NativeRecording,
    stream: Stream,
    owner: &OwnerPlatform,
    log: &AppLog,
    labels: &Labels,
    events: &mut Option<flui_native::HeadlessHostEvents>,
    sources: &mut Option<Sources>,
) -> Result<Vec<Recorded>, String> {
    let whole = |replayed: Result<flui_native::journal::codec::Replayed, _>| match replayed {
        Ok(flui_native::journal::codec::Replayed::Whole) => Ok(()),
        Ok(cut) => Err(format!("the recording was cut: {cut:?}")),
        Err(error) => Err(format!("the recording does not replay: {error}")),
    };
    match stream {
        Stream::App => {
            let events = events.take().expect("BUG: a session opens one app stream");
            whole(recording.replay_into(&mut HostEventsTarget(events)))?;
            Ok(app_entries(&log.calls()))
        }
        Stream::Window => {
            let (window, handler) = headless_window(owner);
            whole(recording.replay_into(&mut WindowTarget::new(window)))?;
            Ok(ime_entries(&handler))
        }
        Stream::Platform => {
            let sources = sources
                .take()
                .expect("BUG: a session opens one platform stream");
            let mut target = PlatformTarget::new(sources, owner, labels);
            whole(recording.replay_into(&mut target))?;
            Ok(entries_of(target.reads))
        }
        Stream::WindowState => {
            let (window, _handler) = headless_window(owner);
            let mut target = WindowStateTarget::new(window, labels);
            whole(recording.replay_into(&mut target))?;
            // A window's stream begins with the snapshot it opened with, its setup, which the
            // replay publishes first; what the application heard is what followed it.
            Ok(target
                .published
                .into_iter()
                .skip(1)
                .map(|state| Recorded::WindowState(Box::new(state.into_parts())))
                .collect())
        }
        other => Err(format!("no recorded kind journals into a {other:?} stream")),
    }
}
