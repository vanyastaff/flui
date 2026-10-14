//! Capturing native recordings from live registries (`native-capture`).
//!
//! On a native host, run the backend under [`record`] and write each
//! [`NativeRecording`] it returns with [`NativeRecording::write`]: the
//! journal replays to the input the backend delivered, on any host.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use flui_platform::shared::pointer_assembly::capture::{
    CaptureGuard, CaptureSource, Captured, capture,
};
use flui_platform::shared::pointer_assembly::{
    ContactRegistry, NativeClock, NativeContact, NativeReading, ReadingAction, ReadingTime, decode,
};
use flui_platform_api::EventTime;
use flui_platform_api::pointer::{PointerButton, PointerKind};
use parking_lot::Mutex;

use super::codec::{Backend, Header, NativeRecording, Row};
use super::{RECORDINGS, recording};

/// Run `session` with a capture installed on this thread, and return its
/// result with the recording of every registry created during it, in
/// creation order.
pub fn record<T>(backend: Backend, session: impl FnOnce() -> T) -> (T, Vec<NativeRecording>) {
    let journals: Arc<Mutex<Vec<NativeRecording>>> = Arc::default();
    let sink = Arc::clone(&journals);
    let guard = capture(move |source, call| journal(&mut sink.lock(), backend, source, call));
    let result = session();
    drop(guard);
    let recordings = std::mem::take(&mut *journals.lock());
    (result, recordings)
}

/// Fold one captured call into the recording of its registry.
fn journal(
    recordings: &mut Vec<NativeRecording>,
    backend: Backend,
    source: CaptureSource,
    call: Captured<'_>,
) {
    let index = source.0 as usize;
    if let Captured::Started {
        epoch,
        uptime,
        counter,
    } = call
    {
        assert_eq!(index, recordings.len(), "sources are numbered in order");
        recordings.push(NativeRecording {
            header: Header::new(backend, epoch, uptime, counter),
            rows: Vec::new(),
        });
        return;
    }
    let recording = &mut recordings[index];
    let setup = recording.rows.is_empty();
    let header = &mut recording.header;
    let row = match call {
        Captured::ReservedContact(contact) => {
            header.reserved_contact = Some(contact);
            return;
        }
        Captured::FirstPointer(first) => {
            header.first_pointer = Some(first);
            return;
        }
        Captured::FirstDevice(first) => {
            header.first_device = Some(first);
            return;
        }
        Captured::ClickSettings(settings) if setup => {
            header.click_settings = settings;
            return;
        }
        Captured::ClickSettings(settings) => Row::ClickSettings(settings),
        Captured::Reading(reading) => Row::Reading(*reading),
        Captured::Batch {
            coalesced,
            predicted,
            current,
        } => Row::Batch {
            coalesced: coalesced.to_vec(),
            predicted: predicted.to_vec(),
            current: *current,
        },
        Captured::AddDevice { device, kind, time } => Row::AddDevice { device, kind, time },
        Captured::RemoveDevice { device, time } => Row::RemoveDevice { device, time },
        Captured::CancelSurface {
            surface,
            time,
            reason,
        } => Row::CancelSurface {
            surface,
            time,
            reason,
        },
        Captured::CancelContact {
            surface,
            contact,
            time,
            reason,
        } => Row::CancelContact {
            surface,
            contact,
            time,
            reason,
        },
        other => panic!("the capture grew a call the native codec does not mirror: {other:?}"),
    };
    recording.rows.push(row);
}

fn touch(action: ReadingAction, ms: u64) -> NativeReading {
    NativeReading::new(
        NativeContact(7),
        PointerKind::Touch,
        action,
        ReadingTime::epoch(EventTime::from_nanos(ms * 1_000_000)),
    )
    .at(10.0, 20.0, 1.0)
}

const PRESS: ReadingAction = ReadingAction::Press(PointerButton::PRIMARY);

/// A registry driven through every call a checked-in recording makes
/// journals exactly that recording: its clock anchors and setup in the
/// header, and every reading, batch, hot-plug and cancel as a row.
pub fn a_captured_registry_journals_the_recording_it_replays() {
    for (name, text) in RECORDINGS {
        let original = recording(text);
        let (_, captured) = record(original.header.backend, || original.replay());
        assert_eq!(captured, [original], "{name}");
    }
}

/// A registry reports to the capture installed when it was created, for as
/// long as that capture's guard lives; a stale guard does not uninstall a
/// newer capture.
pub fn a_registry_reports_to_its_own_capture_while_it_lives() {
    let calls: Arc<Mutex<Vec<(u8, u32)>>> = Arc::default();
    let sink = |name: u8| {
        let calls = Arc::clone(&calls);
        move |source: CaptureSource, call: Captured<'_>| {
            if matches!(call, Captured::Reading(_)) {
                calls.lock().push((name, source.0));
            }
        }
    };
    let first = capture(sink(1));
    let mut early = ContactRegistry::new(NativeClock::epoch_only());
    let second = capture(sink(2));
    let mut late = ContactRegistry::new(NativeClock::epoch_only());
    decode(&mut early, &touch(PRESS, 1)).expect("valid");
    decode(&mut late, &touch(PRESS, 2)).expect("valid");
    drop(first);
    let mut after_stale_drop = ContactRegistry::new(NativeClock::epoch_only());
    decode(&mut early, &touch(ReadingAction::Move, 3)).expect("valid");
    decode(&mut after_stale_drop, &touch(PRESS, 4)).expect("valid");
    drop(second);
    let mut uncaptured = ContactRegistry::new(NativeClock::epoch_only());
    decode(&mut late, &touch(ReadingAction::Move, 5)).expect("valid");
    decode(&mut uncaptured, &touch(PRESS, 6)).expect("valid");
    assert_eq!(*calls.lock(), [(1, 0), (2, 0), (2, 1)]);
}

/// A sink that panics is dropped and its capture ends, and the registry is
/// left as if the call had not been made.
pub fn a_panicking_sink_ends_its_capture_and_leaves_the_registry_unchanged() {
    struct Dropped(Arc<AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }
    let dropped = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let (flag, count) = (Dropped(Arc::clone(&dropped)), Arc::clone(&calls));
    let guard = capture(move |_, call| {
        let _owned = &flag;
        count.fetch_add(1, Ordering::Relaxed);
        assert!(
            !matches!(call, Captured::Reading(_)),
            "the sink refuses readings"
        );
    });
    let mut registry = ContactRegistry::new(NativeClock::epoch_only());
    let failed = catch_unwind(AssertUnwindSafe(|| decode(&mut registry, &touch(PRESS, 1))));
    assert!(failed.is_err(), "the sink's panic propagates");
    assert!(
        dropped.load(Ordering::Relaxed),
        "the panicking sink is dropped, not kept"
    );
    let events = decode(&mut registry, &touch(PRESS, 2)).expect("valid");
    assert_eq!(events.len(), 1, "the press was never committed: {events:?}");
    assert_eq!(
        calls.load(Ordering::Relaxed),
        2,
        "the start and the refused reading only"
    );
    drop(guard);
}

/// A sink may release its own capture, or install a replacement, from inside
/// a call: the release ends the capture after that call, and the replacement
/// takes only registries created after it.
pub fn a_sink_may_release_or_replace_its_capture_from_inside_a_call() {
    let held: Arc<Mutex<Option<CaptureGuard>>> = Arc::default();
    let replacement: Arc<Mutex<Option<CaptureGuard>>> = Arc::default();
    let calls: Arc<Mutex<Vec<(u8, u32)>>> = Arc::default();
    let (owner, swap, log) = (
        Arc::clone(&held),
        Arc::clone(&replacement),
        Arc::clone(&calls),
    );
    *held.lock() = Some(capture(move |source, call| {
        if !matches!(call, Captured::Reading(_)) {
            return;
        }
        log.lock().push((1, source.0));
        let log = Arc::clone(&log);
        *swap.lock() = Some(capture(move |source, call| {
            if matches!(call, Captured::Reading(_)) {
                log.lock().push((2, source.0));
            }
        }));
        drop(owner.lock().take());
    }));
    let mut first = ContactRegistry::new(NativeClock::epoch_only());
    decode(&mut first, &touch(PRESS, 1)).expect("valid");
    assert!(held.lock().is_none(), "the sink released its own capture");
    decode(&mut first, &touch(ReadingAction::Move, 2)).expect("valid");
    let mut second = ContactRegistry::new(NativeClock::epoch_only());
    decode(&mut second, &touch(PRESS, 3)).expect("valid");
    assert_eq!(*calls.lock(), [(1, 0), (2, 0)]);
    drop(replacement.lock().take());
}

/// The capture rows of `native_recording_contracts`.
pub const CASES: [(&str, fn()); 4] = [
    (
        "a_captured_registry_journals_the_recording_it_replays",
        a_captured_registry_journals_the_recording_it_replays,
    ),
    (
        "a_registry_reports_to_its_own_capture_while_it_lives",
        a_registry_reports_to_its_own_capture_while_it_lives,
    ),
    (
        "a_panicking_sink_ends_its_capture_and_leaves_the_registry_unchanged",
        a_panicking_sink_ends_its_capture_and_leaves_the_registry_unchanged,
    ),
    (
        "a_sink_may_release_or_replace_its_capture_from_inside_a_call",
        a_sink_may_release_or_replace_its_capture_from_inside_a_call,
    ),
];
