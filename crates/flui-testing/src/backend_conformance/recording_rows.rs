//! The recording rows (ADR-0215 §4, ADR-0390): what a backend delivers beside input, its native
//! journal records, and the recording replays on the headless host to what the application
//! heard, one row per kind.
//!
//! Each row hands the backend sessions of its kind as the OS reported them. The backend drives
//! each through its own delivery with a journal capture installed, reads back what the journal
//! recorded and replays it on the headless host ([`BackendUnderTest::replay_recording`]). The row
//! holds when the application heard each session entry for entry and in order, both from the
//! backend and from the replay: an entry the backend lost, changed or added, or one its journal
//! lost, changed or could not replay, breaks it. A backend whose delivery cannot be driven fails
//! it as [`FailureKind::NotDriven`](super::FailureKind::NotDriven).

use flui_foundation::geometry::{DevicePixelRatio, Point, Rect, Size, WindowMetrics};
use flui_platform::{
    ColorGamut, DesktopOrigin, DisplayBounds, DisplayColor, DisplayParts, ImeEvent,
    MemoryPressureLevel, RefreshPeriod, SessionEndPhase, SrgbColor, SystemNotice,
    SystemPreferences, WindowExtent, WindowFocus, WindowGeometry, WindowMode, WindowState,
};

use super::rows::Outcome;
use super::{BackendUnderTest, Recorded, RecordingReplay, RowFailure, RowOutcome};

/// One composition: the input method turns on, a preedit grows into a picked candidate, which
/// commits, and the input method turns off.
fn composition() -> Vec<Recorded> {
    vec![
        Recorded::Ime(ImeEvent::Enabled),
        Recorded::Ime(ImeEvent::Preedit {
            text: "に".to_owned(),
            cursor: Some((0, 3)),
        }),
        Recorded::Ime(ImeEvent::Preedit {
            text: "日本".to_owned(),
            cursor: Some((6, 6)),
        }),
        Recorded::Ime(ImeEvent::Commit("日本".to_owned())),
        Recorded::Ime(ImeEvent::Disabled),
    ]
}

/// What a host reports of the application's lifecycle: the machine sleeps and wakes, a session
/// end is asked about and cancelled, the user reopens the application, memory runs short and
/// the session locks.
fn host_notices() -> Vec<Recorded> {
    vec![
        Recorded::Notice(SystemNotice::Sleep),
        Recorded::Notice(SystemNotice::Wake),
        Recorded::SessionEnd(SessionEndPhase::Query),
        Recorded::SessionEnd(SessionEndPhase::Cancelled),
        Recorded::Reopen,
        Recorded::Notice(SystemNotice::MemoryPressure(MemoryPressureLevel::Critical)),
        Recorded::Notice(SystemNotice::SessionLocked),
    ]
}

/// A window's snapshots as its window system reports them: it takes the keyboard, is
/// maximized, and is given a title. Each is a snapshot its successor rules leave as it is, so
/// the window publishes it unchanged.
fn window_snapshots() -> Vec<Recorded> {
    let size = Size::new(800.0, 600.0);
    let scale = DevicePixelRatio::try_new(2.0).expect("BUG: two is a device pixel ratio");
    let metrics = WindowMetrics::try_new(Size::new(1600, 1200), scale)
        .expect("BUG: 1600 by 1200 pixels is a drawable area");
    let opened = WindowState::new(
        WindowGeometry::try_new(None, size).expect("BUG: 800 by 600 is a window's extent"),
        WindowExtent::try_new(size).expect("BUG: 800 by 600 is a window's extent"),
    )
    .with_metrics(Some(metrics))
    .with_focus(WindowFocus::Focused);
    let maximized = opened.clone().with_mode(WindowMode::Maximized);
    let titled = maximized.clone().with_title(Some("Notes".to_owned()));
    [opened, maximized, titled]
        .into_iter()
        .map(|state| Recorded::WindowState(Box::new(state.into_parts())))
        .collect()
}

/// Two reads of the system preferences: high contrast and larger text, then an accent colour.
fn preference_reads() -> Vec<Recorded> {
    let contrast = SystemPreferences::default()
        .with_high_contrast(true)
        .with_text_scale(1.5)
        .expect("BUG: one and a half is a text scale");
    let accent = SystemPreferences::default()
        .with_accent_color(Some(SrgbColor::from_rgba8(0, 120, 215, 255)));
    vec![
        Recorded::Preferences(Box::new(contrast)),
        Recorded::Preferences(Box::new(accent)),
    ]
}

/// A display named `name` whose persistent key is `key`, `width` by `height` logical pixels at
/// `x` on the desktop, at a scale of `scale`.
fn display(
    name: &str,
    key: &str,
    x: i32,
    (width, height): (f64, f64),
    scale: f64,
    primary: bool,
) -> DisplayParts {
    let extent = Size::new(width, height);
    DisplayParts {
        name: name.to_owned(),
        origin: DesktopOrigin::new(x, 0),
        bounds: DisplayBounds::try_new(
            extent,
            Rect::from_origin_size(Point::new(0.0, 25.0), Size::new(width, height - 25.0)),
        )
        .expect("BUG: the usable area lies within the display"),
        scale_factor: DevicePixelRatio::try_new(scale).expect("BUG: a positive ratio"),
        refresh_period: Some(RefreshPeriod::from_hz(60.0).expect("BUG: 60 Hz is a refresh rate")),
        is_primary: primary,
        persistent_key: Some(key.to_owned()),
        color: Ok(DisplayColor::Standard(ColorGamut::Srgb)),
    }
}

/// Three reads of the displays: the built-in one alone, then with an external one beside it,
/// then with a second external one of the same model, which only its id, key and place tell
/// apart.
fn display_reads() -> Vec<Recorded> {
    let built_in = (
        0,
        display("Built-in", "EDID-0610-A050", 0, (1440.0, 900.0), 2.0, true),
    );
    let left = (
        1,
        display(
            "DELL U2720Q",
            "EDID-10AC-D0A1-1",
            1440,
            (2560.0, 1440.0),
            1.0,
            false,
        ),
    );
    let right = (
        2,
        display(
            "DELL U2720Q",
            "EDID-10AC-D0A1-2",
            4000,
            (2560.0, 1440.0),
            1.0,
            false,
        ),
    );
    vec![
        Recorded::Displays(vec![built_in.clone()]),
        Recorded::Displays(vec![built_in.clone(), left.clone()]),
        Recorded::Displays(vec![built_in, left, right]),
    ]
}

/// Drive each of `sessions`, of `kind`, through `backend` and replay what it journaled: held
/// when the application heard every session as it was reported, from the backend and from the
/// replay.
fn replays<B: BackendUnderTest>(
    backend: &mut B,
    kind: &str,
    sessions: &[Vec<Recorded>],
) -> Outcome {
    for session in sessions {
        match backend.replay_recording(session) {
            RecordingReplay::NotDriven(why) => {
                return Err(RowFailure::not_driven(format!(
                    "the backend's delivery of {kind} is not driven: {why}"
                )));
            }
            RecordingReplay::Refused { live, why } => {
                return Err(RowFailure::output(format!(
                    "the backend delivered {kind} as {live:?}, and its recording did not \
                     replay: {why}"
                )));
            }
            RecordingReplay::Replayed { live, .. } if live != *session => {
                return Err(RowFailure::output(format!(
                    "the backend delivered {kind} as {live:?}, not {session:?}"
                )));
            }
            RecordingReplay::Replayed { replayed, .. } if replayed != *session => {
                return Err(RowFailure::output(format!(
                    "the recording of {kind} replayed as {replayed:?}, not {session:?}"
                )));
            }
            RecordingReplay::Replayed { .. } => {}
        }
    }
    Ok(RowOutcome::Held)
}

/// A recorded composition replays to the input method events the window was delivered.
pub(super) fn a_recorded_composition_replays_as_it_was_delivered<B: BackendUnderTest>(
    backend: &mut B,
) -> Outcome {
    replays(backend, "a composition", &[composition()])
}

/// A recorded lifecycle replays to what the host reported: its notices, a session end's
/// phases and a reopen to the application, and each snapshot its window system reported of
/// the window.
pub(super) fn a_recorded_lifecycle_replays_as_it_was_reported<B: BackendUnderTest>(
    backend: &mut B,
) -> Outcome {
    replays(
        backend,
        "a lifecycle",
        &[host_notices(), window_snapshots()],
    )
}

/// Recorded reads of the system preferences replay to what each read answered.
pub(super) fn recorded_preference_reads_replay_as_they_were_answered<B: BackendUnderTest>(
    backend: &mut B,
) -> Outcome {
    replays(backend, "preference reads", &[preference_reads()])
}

/// Recorded reads of the displays replay to what each read answered, every display in the
/// snapshot's order.
pub(super) fn recorded_display_reads_replay_as_they_were_answered<B: BackendUnderTest>(
    backend: &mut B,
) -> Outcome {
    replays(backend, "display reads", &[display_reads()])
}
