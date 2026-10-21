//! Capturing native recordings from live registries (`native-capture`).
//!
//! On a native host, run the backend under [`record`] and write each
//! [`NativeRecording`] it returns with [`NativeRecording::write`]: the
//! journal replays to the input the backend delivered, on any host.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use flui_native::shared::pointer_assembly::capture::{
    CaptureGuard, CaptureSource, Captured, capture,
};
use flui_native::shared::pointer_assembly::{
    ContactRegistry, NativeClock, NativeContact, NativeReading, ReadingAction, ReadingTime, decode,
};
use flui_platform_api::EventTime;
use flui_platform_api::pointer::{PointerButton, PointerEvent, PointerKind};
use parking_lot::Mutex;

use super::codec::{NativeRecording, Row};
pub use super::journal::record;
use super::{RECORDINGS, recording};

/// Where the checked-in recordings live.
const GOLDEN_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/native_recording");

/// What a rerun of an injected session must reproduce: the replayed
/// transcript in whole device pixels (every scale set to one, so a host at
/// another DPI compares) with each time left out, since the OS stamps each run
/// anew. A pen is placed from its himetric position, which the OS derives from
/// the injected pixel with a rounding that depends on where the window is, so
/// positions are compared at the pixel they were injected at.
pub fn shape(recording: &NativeRecording) -> Vec<String> {
    fn in_device_pixels(reading: &mut NativeReading) {
        reading.scale = 1.0;
        if let Some(position) = &mut reading.position {
            position.x = position.x.round();
            position.y = position.y.round();
        }
    }
    let mut device = recording.clone();
    for row in &mut device.rows {
        match row {
            Row::Reading(reading) => in_device_pixels(reading),
            Row::Batch {
                coalesced,
                predicted,
                current,
            } => {
                for reading in coalesced.iter_mut().chain(predicted).chain([current]) {
                    in_device_pixels(reading);
                }
            }
            Row::Viewport { frame, .. } | Row::ViewportAbandoned { frame } => frame.scale = 1.0,
            Row::Wheel(reading) => reading.scale = 1.0,
            // A scroll step is in logical pixels already, from its contact's
            // position: a rerun on the same device has the same scale.
            Row::Scroll(_)
            | Row::ClickSettings(_)
            | Row::AddDevice { .. }
            | Row::RemoveDevice { .. }
            | Row::CancelSurface { .. }
            | Row::CancelContact { .. } => {}
        }
    }
    super::transcript(&device.replay())
        .iter()
        .map(|line| without_times(line))
        .collect()
}

/// `line` with every ` t=<nanoseconds>` written ` t=_`.
fn without_times(line: &str) -> String {
    const TIME: &str = " t=";
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find(TIME) {
        let (head, tail) = rest.split_at(at + TIME.len());
        out.push_str(head);
        let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
        out.push('_');
        rest = &tail[digits..];
    }
    out.push_str(rest);
    out
}

/// What a rerun of a session on an Android device must reproduce: the
/// replayed events as their grammar, without positions, deltas or times, a
/// run of moves, pan-zoom updates or scroll changes of one pointer counted
/// once. The device batches and resamples moves by its frame timing, so
/// their number and positions differ from run to run; who moved, pressed,
/// hovered, pinched or scrolled, in which order and with which sensor
/// readings at each press, does not.
pub fn grammar(recording: &NativeRecording) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for input in recording.replay() {
        let Some(event) = input.as_pointer() else {
            continue;
        };
        let head = |verb: &str, pointer: &flui_platform_api::pointer::PointerInfo| {
            format!("{verb} p{} {:?}", pointer.id.get(), pointer.kind)
        };
        let line = match event {
            PointerEvent::Down(press) => format!(
                "{} {:?}{}",
                head("down", &press.pointer),
                press.button(),
                sensors(&press.sample)
            ),
            PointerEvent::Up(release) => {
                format!("{} {:?}", head("up", &release.pointer), release.button())
            }
            PointerEvent::Move(movement) => {
                format!("{} {:?}", head("move", &movement.pointer), movement.buttons)
            }
            PointerEvent::Enter(signal) => head("enter", &signal.pointer),
            PointerEvent::Leave(signal) => head("leave", &signal.pointer),
            PointerEvent::Cancel(cancel) => {
                format!("{} {:?}", head("cancel", &cancel.pointer), cancel.reason)
            }
            PointerEvent::Scroll(scroll) => format!(
                "{} {:?} {:?}",
                head("scroll", &scroll.pointer),
                scroll.phase,
                scroll.delta.unit()
            ),
            PointerEvent::PanZoom(gesture) => {
                let phase = match gesture.phase {
                    flui_platform_api::pointer::PanZoomPhase::Start => "start",
                    flui_platform_api::pointer::PanZoomPhase::Update(_) => "update",
                    flui_platform_api::pointer::PanZoomPhase::End => "end",
                    flui_platform_api::pointer::PanZoomPhase::Cancelled => "cancelled",
                    _ => "?",
                };
                format!("{} {phase}", head("panzoom", gesture.pointer()))
            }
            PointerEvent::DeviceAdded(change) => format!("device+ {:?}", change.kind),
            PointerEvent::DeviceRemoved(change) => format!("device- {:?}", change.kind),
            other => format!("{other:?}"),
        };
        if lines.last() != Some(&line) {
            lines.push(line);
        }
    }
    lines
}

/// The sensor readings a press carries, to two decimals.
fn sensors(sample: &flui_platform_api::pointer::PointerSample) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    if let Some(pressure) = sample.pressure {
        write!(out, " pressure={:.2}", pressure.get()).expect("BUG: a String accepts writes");
    }
    if let Some(orientation) = sample.orientation {
        write!(
            out,
            " altitude={:.2} azimuth={:.2}",
            orientation.altitude().unwrap_or(f64::NAN),
            orientation.azimuth().unwrap_or(f64::NAN)
        )
        .expect("BUG: a String accepts writes");
    }
    if sample.contact_size.is_some() {
        out.push_str(" size");
    }
    out
}

/// Hold the recording an injected session `name` captured against its
/// checked-in golden `<name>.jsonl`: the two must have the same [`shape`].
///
/// With `FLUI_NATIVE_RECORDING_BLESS` set, the capture replaces the golden
/// instead. With
/// `FLUI_NATIVE_RECORDING_DIR` set, it is also written there, for a reader
/// to diff the rows themselves.
pub fn hold_against_golden(name: &str, fresh: &NativeRecording) {
    hold(name, fresh, shape);
}

/// Hold `fresh` against the golden `<name>.jsonl` by `compare`.
fn hold(name: &str, fresh: &NativeRecording, compare: fn(&NativeRecording) -> Vec<String>) {
    let file = format!("{name}.jsonl");
    let write = |directory: &std::path::Path| {
        let path = directory.join(&file);
        let out = std::fs::File::create(&path).expect("create the recording file");
        fresh
            .write(std::io::BufWriter::new(out))
            .expect("write the recording");
        eprintln!("native recording written to {}", path.display());
    };
    if let Some(directory) = std::env::var_os("FLUI_NATIVE_RECORDING_DIR") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).expect("create the recording directory");
        write(&directory);
    }
    let golden_dir = std::path::Path::new(GOLDEN_DIR);
    if std::env::var_os("FLUI_NATIVE_RECORDING_BLESS").is_some() {
        write(golden_dir);
        eprintln!("NATIVE_RECORDING_HELD={name}");
        return;
    }
    let text = std::fs::read_to_string(golden_dir.join(&file)).unwrap_or_else(|error| {
        panic!("{name}: no golden to compare with ({error}); record one with --bless")
    });
    let golden = recording(&text);
    let (expected, found) = (compare(&golden), compare(fresh));
    assert!(
        expected == found,
        "{name}: the injected session no longer replays to its golden\n golden: {expected:#?}\n  fresh: {found:#?}"
    );
    eprintln!("{name}: {} events match the golden", found.len());
    eprintln!("NATIVE_RECORDING_HELD={name}");
}

/// Hold the sessions a device run pulled into `FLUI_DEVICE_RECORDING_DIR`:
/// each `<name>.jsonl` must replay
/// through the shared decoder to exactly the pointer events the backend
/// delivered on the device, which the device run wrote beside it as
/// `<name>.delivered.txt` (one `Debug` line each), and then to the shape of
/// its [`grammar`]. Returns how many sessions were held; none without the
/// variable.
pub fn hold_pulled_device_recordings() -> usize {
    let Some(directory) = std::env::var_os("FLUI_DEVICE_RECORDING_DIR") else {
        return 0;
    };
    let mut names: Vec<String> = std::fs::read_dir(&directory)
        .expect("read the pulled recordings")
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            Some(name.strip_suffix(".jsonl")?.to_owned())
        })
        .collect();
    names.sort();
    let directory = std::path::PathBuf::from(directory);
    for name in &names {
        let text = std::fs::read_to_string(directory.join(format!("{name}.jsonl")))
            .expect("read a pulled recording");
        let fresh = recording(&text);
        let delivered = std::fs::read_to_string(directory.join(format!("{name}.delivered.txt")))
            .expect("every pulled recording has its delivered events");
        let delivered: Vec<&str> = delivered.lines().collect();
        let replayed: Vec<String> = fresh
            .replay()
            .iter()
            .filter_map(|input| input.as_pointer().map(|event| format!("{event:?}")))
            .collect();
        if let Some(at) = (0..delivered.len().max(replayed.len()))
            .find(|&at| delivered.get(at).copied() != replayed.get(at).map(String::as_str))
        {
            panic!(
                "{name}: the device recording does not replay to what the window delivered: {} delivered, {} replayed, first difference at {at}\n delivered: {:?}\n  replayed: {:?}",
                delivered.len(),
                replayed.len(),
                delivered.get(at),
                replayed.get(at)
            );
        }
        eprintln!(
            "{name}: {} replayed events equal the delivered ones",
            replayed.len()
        );
        hold(name, &fresh, grammar);
    }
    names.len()
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
/// header, and every reading, batch, wheel turn, hot-plug, cancel and
/// touchpad viewport call as a row — a viewport's pinch as its viewport
/// call, not as the gesture readings replay derives from it. A capture
/// writes no provenance note. A live capture's journals read the same
/// recording before it ends, as the device probe reads them.
pub fn a_captured_registry_journals_the_recording_it_replays() {
    for (name, text) in RECORDINGS {
        let mut original = recording(text);
        let (_, captured) = record(original.header.backend, || original.replay());
        let recorder = super::journal::Recorder::start(original.header.backend);
        original.replay();
        let live = recorder.journals().snapshot();
        drop(recorder.finish());
        original.header.provenance = None;
        assert_eq!(captured, [original], "{name}");
        assert_eq!(live, captured, "{name}: read while live");
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
