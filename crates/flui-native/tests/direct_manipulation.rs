//! Win32 DirectManipulation readings of a precision touchpad (ADR-0217 §4):
//! a two-finger pan is a phased `Scroll` with momentum, a pinch is `PanZoom`,
//! and the viewport's own status decides when either ends. The decoder is
//! pure, so Linux CI runs it over the same readings the Win32 backend feeds it.

use std::time::Duration;

use flui_native::shared::direct_manipulation::{
    ContentTransform, TouchpadAssembler, TouchpadFrame, ViewportReading, ViewportStatus,
};
use flui_native::shared::pointer_assembly::{
    ContactRegistry, CounterAnchor, NativeClock, NativeSurface, NativeTime, QpcTicks, ReadingTime,
};
use flui_native::traits::PlatformInput;
use flui_platform_api::EventTime;
use flui_platform_api::keyboard::Modifiers;
use flui_platform_api::pointer::{PanZoomPhase, PointerEvent};
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

fn a_pinch_that_coasts_is_still_reset_and_the_next_pan_starts_at_the_origin() {
    assert_eq!(
        run(&[
            status(Running),
            content(1.1, 0.0, 0.0),
            // The fingers lift with the centroid moving: the viewport coasts
            // its translation, which is neither a scroll nor a pinch step.
            status(Inertia),
            content(1.1, 6.0, -4.0),
            status(Ready),
            content(1.0, 0.0, 0.0),
            status(Running),
            content(1.0, 0.0, -8.0),
            status(Ready),
        ]),
        [
            "pinch start",
            "pinch scale=1.100",
            "pinch end",
            "reset",
            "scroll Began (0, 8)",
            "scroll Ended (0, 0)",
            "reset",
        ]
    );
}

fn after_a_lost_viewport_the_next_pan_is_measured_from_the_origin() {
    // A window paused for its size and move loop discards the content
    // update its reset causes; the next pan must not count the old offset.
    assert_eq!(
        run(&[
            status(Running),
            content(1.0, 0.0, -8.0),
            status(Disabled),
            status(Running),
            content(1.0, 0.0, -3.0),
        ]),
        [
            "scroll Began (0, 8)",
            "scroll Cancelled (0, 0)",
            "reset",
            "scroll Began (0, 3)",
        ]
    );
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
                "a_pinch_that_coasts_is_still_reset_and_the_next_pan_starts_at_the_origin",
                a_pinch_that_coasts_is_still_reset_and_the_next_pan_starts_at_the_origin,
            ),
            (
                "after_a_lost_viewport_the_next_pan_is_measured_from_the_origin",
                after_a_lost_viewport_the_next_pan_is_measured_from_the_origin,
            ),
        ],
    );
}
