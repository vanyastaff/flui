//! Win32 DirectManipulation readings of a precision touchpad (ADR-0217 §4):
//! a two-finger pan is a phased `Scroll` with momentum, a pinch is `PanZoom`,
//! and the viewport's own status decides when either ends. The decoder is
//! pure, so Linux CI runs it over the same readings the Win32 backend feeds it.

use std::time::Duration;

use flui_platform::shared::direct_manipulation::{
    ContentTransform, TouchpadAssembler, TouchpadFrame, ViewportReading, ViewportStatus,
};
use flui_platform::shared::input_availability;
use flui_platform::shared::pointer_assembly::{
    ContactRegistry, CounterAnchor, NativeClock, NativeSurface, NativeTime, QpcTicks, ReadingTime,
};
use flui_platform::traits::PlatformInput;
use flui_platform_api::EventTime;
use flui_platform_api::input_capabilities::Availability;
use flui_platform_api::keyboard::Modifiers;
use flui_platform_api::pointer::{PanZoomPhase, PointerEvent, PointerKind, ScrollPhase};
use serde::Deserialize;
use std::num::NonZeroU64;

const EPOCH: EventTime = EventTime::from_nanos(1_000_000_000);
const QPC_HZ: u64 = 10_000_000;

fn clock() -> NativeClock {
    NativeClock::new(
        EPOCH,
        Duration::ZERO,
        Some(CounterAnchor {
            count: QpcTicks(0),
            frequency: NonZeroU64::new(QPC_HZ).expect("nonzero"),
        }),
    )
}

fn frame(qpc: u64, scale: f64) -> TouchpadFrame {
    TouchpadFrame {
        time: ReadingTime {
            coarse: NativeTime::TickMillis {
                tick: 0,
                observed_ms: 0,
            },
            counter: Some(QpcTicks(qpc)),
        },
        position: (300.0, 240.0),
        scale,
        modifiers: Modifiers::NONE,
    }
}

fn content(scale: f64, x: f64, y: f64) -> ViewportReading {
    ViewportReading::Content(ContentTransform::try_new(scale, x, y).expect("a finite transform"))
}

const fn status(status: ViewportStatus) -> ViewportReading {
    ViewportReading::Status(status)
}

/// One input in the terms a consumer sees, without identities.
fn describe(input: &PlatformInput) -> String {
    match input.as_pointer().expect("a pointer event") {
        PointerEvent::Scroll(scroll) => {
            let phase = scroll.phase.map_or("none".to_owned(), |p| format!("{p:?}"));
            format!(
                "scroll {phase} ({}, {})",
                scroll.delta.x(),
                scroll.delta.y()
            )
        }
        PointerEvent::PanZoom(gesture) => match gesture.phase {
            PanZoomPhase::Start => "pinch start".to_owned(),
            PanZoomPhase::Update(transform) => format!("pinch scale={:.3}", transform.scale()),
            PanZoomPhase::End => "pinch end".to_owned(),
            PanZoomPhase::Cancelled => "pinch cancelled".to_owned(),
            _ => "pinch ?".to_owned(),
        },
        other => format!("{other:?}"),
    }
}

/// Feed `readings` and list what was delivered, with `reset` marking each
/// viewport reset the backend is asked to perform.
fn run(readings: &[ViewportReading]) -> Vec<String> {
    let mut contacts = ContactRegistry::new(clock());
    let mut assembler = TouchpadAssembler::new(NativeSurface(0));
    let mut lines = Vec::new();
    for (index, reading) in readings.iter().enumerate() {
        let delivered =
            assembler.assemble(&mut contacts, *reading, frame(index as u64 * 1000, 1.0));
        lines.extend(delivered.inputs.iter().map(describe));
        if delivered.reset {
            lines.push("reset".to_owned());
        }
    }
    lines
}

use ViewportStatus::{Disabled, Inertia, Ready, Running, Suspended};

fn a_two_finger_pan_is_a_phased_scroll_with_momentum() {
    assert_eq!(
        run(&[
            status(Running),
            // Inside the slop: nothing is published yet.
            content(1.0, 0.0, -1.0),
            // Fingers moved up: the scroll offset grows. The pan begins with
            // everything it has accumulated.
            content(1.0, 0.0, -6.0),
            content(1.0, 2.0, -16.0),
            status(Inertia),
            content(1.0, 2.0, -26.0),
            status(Ready),
            // The viewport's return to the origin is not input.
            content(1.0, 0.0, 0.0),
        ]),
        [
            "scroll Began (0, 6)",
            "scroll Changed (-2, 10)",
            "scroll Ended (0, 0)",
            "scroll MomentumBegan (0, 0)",
            "scroll MomentumChanged (0, 10)",
            "scroll MomentumEnded (0, 0)",
            "reset",
        ]
    );
}

fn a_pinch_is_scale_and_its_translation_is_not_a_scroll() {
    assert_eq!(
        run(&[
            status(Running),
            content(1.05, 4.0, 0.0),
            content(1.2, 10.0, 8.0),
            status(Ready),
        ]),
        [
            "pinch start",
            "pinch scale=1.050",
            "pinch scale=1.200",
            "pinch end",
            "reset",
        ]
    );
}

fn a_manipulation_inside_the_slop_publishes_nothing_but_is_reset() {
    assert_eq!(
        run(&[
            status(Running),
            content(1.0, 0.5, -0.5),
            content(1.005, 0.5, -0.5),
            status(Ready),
        ]),
        ["reset"]
    );
}

fn losing_the_viewport_cancels_the_open_gesture() {
    let pan = [status(Running), content(1.0, 0.0, -8.0)];
    let pinch = [status(Running), content(1.1, 0.0, 0.0)];
    let momentum = [status(Running), content(1.0, 0.0, -8.0), status(Inertia)];
    for (name, open, lost, expected) in [
        (
            "pan disabled",
            &pan[..],
            Disabled,
            "scroll Cancelled (0, 0)",
        ),
        (
            "pan suspended",
            &pan[..],
            Suspended,
            "scroll Cancelled (0, 0)",
        ),
        ("pinch disabled", &pinch[..], Disabled, "pinch cancelled"),
        (
            "momentum disabled",
            &momentum[..],
            Disabled,
            "scroll MomentumEnded (0, 0)",
        ),
    ] {
        let mut readings = open.to_vec();
        readings.push(status(lost));
        let lines = run(&readings);
        assert_eq!(
            &lines[lines.len() - 2..],
            [expected, "reset"],
            "{name}: {lines:?}"
        );
    }
}

fn touching_during_momentum_ends_it_and_a_new_pan_begins() {
    assert_eq!(
        run(&[
            status(Running),
            content(1.0, 0.0, -8.0),
            status(Inertia),
            content(1.0, 0.0, -20.0),
            status(Running),
            content(1.0, 0.0, -30.0),
            status(Ready),
        ]),
        [
            "scroll Began (0, 8)",
            "scroll Ended (0, 0)",
            "scroll MomentumBegan (0, 0)",
            "scroll MomentumChanged (0, 12)",
            "scroll MomentumEnded (0, 0)",
            "scroll Began (0, 10)",
            "scroll Ended (0, 0)",
            "reset",
        ]
    );
}

fn abandoning_the_assembler_ends_what_is_open() {
    let mut contacts = ContactRegistry::new(clock());
    let mut assembler = TouchpadAssembler::new(NativeSurface(0));
    for (index, reading) in [status(Running), content(1.0, 0.0, -8.0)]
        .into_iter()
        .enumerate()
    {
        let _ = assembler.assemble(&mut contacts, reading, frame(index as u64, 1.0));
    }
    let delivered = assembler.abandon(&mut contacts, frame(10, 1.0));
    assert_eq!(
        delivered.inputs.iter().map(describe).collect::<Vec<_>>(),
        ["scroll Cancelled (0, 0)"]
    );
    assert!(delivered.reset);
    let again = assembler.abandon(&mut contacts, frame(11, 1.0));
    assert!(again.inputs.is_empty() && !again.reset, "nothing left open");
}

fn a_content_matrix_that_is_not_a_finite_scale_and_offset_is_refused() {
    let good = [1.5_f32, 0.0, 0.0, 1.5, 3.0, -4.0];
    let transform = ContentTransform::from_matrix(&good).expect("a matrix reads");
    assert_eq!(
        (transform.scale(), transform.x(), transform.y()),
        (1.5, 3.0, -4.0)
    );
    for (case, matrix) in [
        ("too short", &good[..5]),
        ("nan offset", &[1.0, 0.0, 0.0, 1.0, f32::NAN, 0.0][..]),
        (
            "infinite scale",
            &[f32::INFINITY, 0.0, 0.0, 1.0, 0.0, 0.0][..],
        ),
        ("zero scale", &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0][..]),
        ("negative scale", &[-1.0, 0.0, 0.0, 1.0, 0.0, 0.0][..]),
    ] {
        assert!(ContentTransform::from_matrix(matrix).is_none(), "{case}");
    }
}

fn the_native_status_numbers_name_every_state() {
    for (raw, expected) in [
        (0, ViewportStatus::Building),
        (1, ViewportStatus::Enabled),
        (2, ViewportStatus::Disabled),
        (3, ViewportStatus::Running),
        (4, ViewportStatus::Inertia),
        (5, ViewportStatus::Ready),
        (6, ViewportStatus::Suspended),
    ] {
        assert_eq!(ViewportStatus::from_raw(raw), Some(expected), "{raw}");
    }
    assert_eq!(ViewportStatus::from_raw(7), None);
    assert_eq!(ViewportStatus::from_raw(-1), None);
}

fn windows_claims_precise_scroll_and_gestures_only_with_direct_manipulation() {
    let with = input_availability::windows(true);
    let without = input_availability::windows(false);
    assert_eq!(
        (with.precise_scroll(), with.trackpad_gestures()),
        (Availability::Native, Availability::Native)
    );
    assert_eq!(
        (without.precise_scroll(), without.trackpad_gestures()),
        (Availability::Unavailable, Availability::Unavailable)
    );
    assert_eq!(with.pen(), Availability::Native);
    assert_eq!(without.pen(), Availability::Native);
}

/// One row of a checked-in DirectManipulation recording.
#[derive(Deserialize)]
#[serde(untagged)]
enum Row {
    Header {
        flui_direct_manipulation_recording: u32,
        scale: f64,
        cursor: (f64, f64),
    },
    Status {
        qpc: u64,
        status: i32,
    },
    Content {
        qpc: u64,
        content: (f32, f32, f32),
    },
}

/// A Win32 precision-touchpad session: a pan that flings into momentum, then a
/// pinch, at 150% scale and a cursor at device pixel (300, 240).
fn win32_recording_replays_to_scroll_and_pinch() {
    let text = include_str!("native_recording/win32_direct_manipulation.jsonl");
    let mut rows = text
        .lines()
        .map(|line| serde_json::from_str::<Row>(line).expect("a row"));
    let Some(Row::Header {
        flui_direct_manipulation_recording: 1,
        scale,
        cursor,
    }) = rows.next()
    else {
        panic!("the recording starts with its version 1 header");
    };
    let mut contacts = ContactRegistry::new(clock());
    let mut assembler = TouchpadAssembler::new(NativeSurface(0));
    let mut events = Vec::new();
    for row in rows {
        let (qpc, reading) = match row {
            Row::Status { qpc, status } => (
                qpc,
                ViewportReading::Status(
                    ViewportStatus::from_raw(status).expect("a documented status"),
                ),
            ),
            Row::Content {
                qpc,
                content: (scale, x, y),
            } => (
                qpc,
                ViewportReading::Content(
                    ContentTransform::from_matrix(&[scale, 0.0, 0.0, scale, x, y])
                        .expect("a finite matrix"),
                ),
            ),
            Row::Header { .. } => panic!("one header"),
        };
        let mut at = frame(qpc, scale);
        at.position = cursor;
        events.extend(assembler.assemble(&mut contacts, reading, at).inputs);
    }
    let lines: Vec<String> = events.iter().map(describe).collect();
    assert_eq!(
        lines,
        [
            "scroll Began (0, 4)",
            "scroll Changed (0, 4)",
            "scroll Changed (-2, 6)",
            "scroll Ended (0, 0)",
            "scroll MomentumBegan (0, 0)",
            "scroll MomentumChanged (-1, 5)",
            "scroll MomentumChanged (0, 3)",
            "scroll MomentumEnded (0, 0)",
            "pinch start",
            "pinch scale=1.100",
            "pinch scale=1.250",
            "pinch end",
        ]
    );
    // Logical pixels at 150%: a 6 px device pan is 4 logical; the cursor
    // (300, 240) is logical (200, 160); stamps follow the counter.
    let PointerEvent::Scroll(first) = events[0].as_pointer().expect("pointer") else {
        panic!("a scroll first");
    };
    assert_eq!(first.pointer.kind, PointerKind::Mouse);
    assert_eq!(first.position.get().x, 200.0);
    assert_eq!(first.position.get().y, 160.0);
    assert_eq!(first.phase, Some(ScrollPhase::Began));
    assert_eq!(first.time.as_nanos(), EPOCH.as_nanos() + 4_000_000);
    let PointerEvent::PanZoom(pinch) = events[8].as_pointer().expect("pointer") else {
        panic!("a pinch after the pan");
    };
    assert_eq!(pinch.pointer().kind, PointerKind::Trackpad);
    assert_eq!(pinch.position.get().x, 200.0);
    assert_eq!(pinch.position.get().y, 160.0);
}

#[test]
fn direct_manipulation_contracts() {
    crate::run_table(
        "direct_manipulation_contracts",
        &[
            (
                "a_two_finger_pan_is_a_phased_scroll_with_momentum",
                a_two_finger_pan_is_a_phased_scroll_with_momentum,
            ),
            (
                "a_pinch_is_scale_and_its_translation_is_not_a_scroll",
                a_pinch_is_scale_and_its_translation_is_not_a_scroll,
            ),
            (
                "a_manipulation_inside_the_slop_publishes_nothing_but_is_reset",
                a_manipulation_inside_the_slop_publishes_nothing_but_is_reset,
            ),
            (
                "losing_the_viewport_cancels_the_open_gesture",
                losing_the_viewport_cancels_the_open_gesture,
            ),
            (
                "touching_during_momentum_ends_it_and_a_new_pan_begins",
                touching_during_momentum_ends_it_and_a_new_pan_begins,
            ),
            (
                "abandoning_the_assembler_ends_what_is_open",
                abandoning_the_assembler_ends_what_is_open,
            ),
            (
                "a_content_matrix_that_is_not_a_finite_scale_and_offset_is_refused",
                a_content_matrix_that_is_not_a_finite_scale_and_offset_is_refused,
            ),
            (
                "the_native_status_numbers_name_every_state",
                the_native_status_numbers_name_every_state,
            ),
            (
                "windows_claims_precise_scroll_and_gestures_only_with_direct_manipulation",
                windows_claims_precise_scroll_and_gestures_only_with_direct_manipulation,
            ),
            (
                "win32_recording_replays_to_scroll_and_pinch",
                win32_recording_replays_to_scroll_and_pinch,
            ),
        ],
    );
}
