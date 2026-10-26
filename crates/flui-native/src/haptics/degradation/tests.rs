//! Every backend's haptics degradation table (ADR-0210 §7), run as pure
//! functions on every host, and the stale-source rule (§5).

use std::sync::Arc;
use std::time::Duration;

use crate::haptics::degradation::{
    ANDROID_USAGE_ASSISTANCE_SONIFICATION, ANDROID_USAGE_TOUCH, AndroidFeedbackAnswer,
    AndroidFeedbackCall, AndroidHapticConstant, AndroidHapticsApi, AndroidPatternPlan,
    AndroidPrimitive, AndroidVibrationUsage, AndroidVibrator, AndroidViewAnswers,
    CORE_HAPTICS_CURVE_POINTS, ComposedPrimitive, CoreHapticsControl, CoreHapticsCurve,
    CoreHapticsEngine, CoreHapticsEvent, EnvelopeStep, IosGenerator, IosImpact, IosNotification,
    MacFeedback, PEN_BUZZ, PenCommand, PenController, PenWaveform, WebVibration, actuator_for,
    android_feedback, android_pattern, android_vibration_usage, ios_feedback, ios_pattern,
    ios_pattern_parts, linux, macos_feedback, macos_pattern, pen_feedback_step, pen_timeline,
    web_feedback, web_pattern, windows_capabilities, windows_feedback, windows_pattern,
};
use crate::haptics::headless::FakeHaptics;
use crate::{HeadlessPlatform, OwnerPlatform, Platform as _};
use flui_platform::WindowOptions;
use flui_platform::haptics::{
    HapticCapabilities, HapticOutcome, HapticPattern, HapticRequest, HapticTiming, PatternPlayback,
};
use flui_platform::{DeviceId, HapticFeedback, PlatformHaptics};

use HapticOutcome::{Approximated, FellBack, Submitted};

const fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

fn transients() -> HapticPattern {
    HapticPattern::builder()
        .transient(ms(0), 1.0, 0.9)
        .transient(ms(40), 0.3, 0.1)
        .build()
        .expect("valid")
}

/// A tap, then a ramp held at 0.6 for 30 ms that falls to 0.2 and ends.
fn mixed() -> HapticPattern {
    HapticPattern::builder()
        .transient(ms(0), 0.5, 0.5)
        .continuous(
            ms(50),
            [(ms(30), 0.6, 0.4), (ms(60), 0.2, 0.4), (ms(100), 0.0, 0.4)],
        )
        .build()
        .expect("valid")
}

fn one_envelope() -> HapticPattern {
    HapticPattern::builder()
        .continuous(ms(0), [(ms(20), 0.8, 0.7), (ms(70), 0.0, 0.2)])
        .build()
        .expect("valid")
}

/// Two continuous events with a gap between them.
fn two_envelopes() -> HapticPattern {
    HapticPattern::builder()
        .continuous(ms(0), [(ms(20), 0.5, 0.3), (ms(30), 0.0, 0.3)])
        .continuous(ms(60), [(ms(10), 1.0, 0.9), (ms(30), 0.0, 0.9)])
        .build()
        .expect("valid")
}

/// Each envelope step's duration and intensity.
fn envelope_shape(steps: &[EnvelopeStep]) -> Vec<(Duration, f32)> {
    steps
        .iter()
        .map(|step| (step.duration, step.intensity))
        .collect()
}

fn overlapping() -> HapticPattern {
    HapticPattern::builder()
        .continuous(ms(0), [(ms(100), 0.0, 0.5)])
        .transient(ms(50), 1.0, 1.0)
        .build()
        .expect("valid")
}

fn ios_rows() {
    let exact = [
        (
            HapticFeedback::LightImpact,
            IosGenerator::Impact(IosImpact::Light),
        ),
        (
            HapticFeedback::MediumImpact,
            IosGenerator::Impact(IosImpact::Medium),
        ),
        (
            HapticFeedback::HeavyImpact,
            IosGenerator::Impact(IosImpact::Heavy),
        ),
        (HapticFeedback::SelectionClick, IosGenerator::Selection),
        (
            HapticFeedback::SuccessNotification,
            IosGenerator::Notification(IosNotification::Success),
        ),
        (
            HapticFeedback::ErrorNotification,
            IosGenerator::Notification(IosNotification::Error),
        ),
    ];
    for (kind, generator) in exact {
        assert_eq!(ios_feedback(kind, true), (generator, Submitted), "{kind:?}");
        assert_eq!(
            ios_feedback(kind, false),
            (generator, Approximated),
            "{kind:?} before iOS 17.5 plays unaddressed"
        );
    }
    for kind in [
        HapticFeedback::Alignment,
        HapticFeedback::LevelChange,
        HapticFeedback::DragLift,
        HapticFeedback::Drop,
        HapticFeedback::Confirm,
        HapticFeedback::Reject,
        HapticFeedback::Vibrate,
    ] {
        assert_eq!(ios_feedback(kind, true).1, Approximated, "{kind:?}");
    }
    assert_eq!(ios_pattern(CoreHapticsEngine::Running), Submitted);
    assert_eq!(ios_pattern(CoreHapticsEngine::Restarting), FellBack);
    assert_eq!(ios_pattern(CoreHapticsEngine::Unsupported), FellBack);

    let parts = ios_pattern_parts(&mixed());
    assert_eq!(parts.len(), 2, "each event is its own Core Haptics pattern");
    assert_eq!(
        (parts[0].at, parts[0].event, parts[0].curves.len()),
        (
            ms(0),
            CoreHapticsEvent::Transient {
                intensity: 0.5,
                sharpness: 0.5
            },
            0
        )
    );
    assert_eq!(parts[1].at, ms(50));
    assert_eq!(
        parts[1].event,
        CoreHapticsEvent::Continuous { duration: ms(100) }
    );
    assert_eq!(
        parts[1].curves,
        [
            CoreHapticsCurve {
                control: CoreHapticsControl::Intensity,
                at: ms(0),
                points: vec![(ms(0), 0.6), (ms(30), 0.6), (ms(60), 0.2), (ms(100), 0.0)],
            },
            CoreHapticsCurve {
                control: CoreHapticsControl::Sharpness,
                at: ms(0),
                points: vec![(ms(0), 0.4), (ms(30), 0.4), (ms(60), 0.4), (ms(100), 0.4)],
            },
        ],
        "the curve holds the first point from the event's start"
    );

    // Twenty control points (with the held start, 21) split into curves of
    // at most sixteen, each starting where the one before ended.
    let long = HapticPattern::builder()
        .continuous(
            ms(0),
            (1..=20_u64).map(|step| {
                let level = if step == 20 { 0.0 } else { 0.5 };
                (ms(step * 10), level, 0.5)
            }),
        )
        .build()
        .expect("valid");
    let parts = ios_pattern_parts(&long);
    let intensity: Vec<_> = parts[0]
        .curves
        .iter()
        .filter(|curve| curve.control == CoreHapticsControl::Intensity)
        .collect();
    assert_eq!(
        intensity
            .iter()
            .map(|curve| (curve.at, curve.points.len()))
            .collect::<Vec<_>>(),
        [(ms(0), CORE_HAPTICS_CURVE_POINTS), (ms(150), 6)]
    );
    assert_eq!(intensity[1].points[0], (ms(0), 0.5));
    assert_eq!(intensity[1].points[5], (ms(50), 0.0));
}

fn android_feedback_rows() {
    // (kind, API 34 constant value, outcome; API 29 constant value, outcome)
    let rows = [
        (HapticFeedback::LightImpact, 1, Submitted, 1, Submitted),
        (HapticFeedback::HeavyImpact, 6, Submitted, 6, Submitted),
        (HapticFeedback::SelectionClick, 4, Submitted, 4, Submitted),
        (HapticFeedback::Confirm, 16, Submitted, 1, Approximated),
        (HapticFeedback::Reject, 17, Submitted, 0, Approximated),
        (HapticFeedback::DragLift, 25, Submitted, 0, Approximated),
        (HapticFeedback::Drop, 13, Submitted, 1, Approximated),
        (HapticFeedback::Alignment, 26, Submitted, 4, Approximated),
        (HapticFeedback::LevelChange, 27, Submitted, 4, Approximated),
        (
            HapticFeedback::WarningNotification,
            23,
            Approximated,
            0,
            Approximated,
        ),
        (HapticFeedback::Vibrate, 0, Approximated, 0, Approximated),
    ];
    for (kind, new_value, new_outcome, old_value, old_outcome) in rows {
        let (constant, outcome) = android_feedback(kind, AndroidHapticsApi::SegmentFeedback);
        assert_eq!(
            (constant.value, outcome),
            (new_value, new_outcome),
            "{kind:?} with API 34's constants"
        );
        let (constant, outcome) = android_feedback(kind, AndroidHapticsApi::Floor);
        assert_eq!(
            (constant.value, outcome),
            (old_value, old_outcome),
            "{kind:?} with the floor's constants"
        );
        assert!(
            constant.since == AndroidHapticsApi::Floor,
            "{kind:?}: no constant newer than the device"
        );
    }
    assert_eq!(
        AndroidHapticConstant::DRAG_START.since,
        AndroidHapticsApi::SegmentFeedback
    );
    // The owner never waits for the Java main thread and posts one call at
    // a time; a declined view is a state carried to later requests, a failed
    // call is not.
    let answers = AndroidViewAnswers::new();
    let post = |table| answers.request(table);
    use AndroidFeedbackAnswer::{Declined, Failed, Played};
    use AndroidFeedbackCall::{Busy, Post};
    assert_eq!(
        post(Approximated),
        Post(Approximated),
        "nothing answered yet"
    );
    assert_eq!(post(Submitted), Busy, "the first call still waits");
    assert_eq!(Busy.outcome(), HapticOutcome::Suppressed);
    for (answer, table, reported) in [
        (Declined, Submitted, Post(HapticOutcome::Suppressed)),
        (Failed, Submitted, Post(HapticOutcome::Suppressed)),
        (Played, Approximated, Post(Approximated)),
        (Failed, Submitted, Post(Submitted)),
    ] {
        answers.answer(answer);
        assert_eq!(post(table), reported, "after {answer:?}");
    }
}

fn vibrator(
    api: AndroidHapticsApi,
    envelopes: bool,
    primitives: &[i32],
    amplitude: bool,
) -> AndroidVibrator {
    AndroidVibrator::new(api, envelopes, primitives.to_vec(), amplitude)
}

const ALL_PRIMITIVES: &[i32] = &[1, 2, 7, 8];

fn android_pattern_rows() {
    let modern = vibrator(AndroidHapticsApi::Envelopes, true, ALL_PRIMITIVES, true);
    let (plan, outcome) = android_pattern(&one_envelope(), &modern);
    assert_eq!(outcome, Submitted, "one envelope plays exactly");
    let AndroidPatternPlan::Envelope {
        initial_sharpness,
        steps,
    } = plan
    else {
        panic!("expected an envelope, got {plan:?}");
    };
    assert!((initial_sharpness - 0.7).abs() < f32::EPSILON);
    assert_eq!(
        envelope_shape(&steps),
        [(ms(1), 0.8), (ms(19), 0.8), (ms(50), 0.0)],
        "a ramp to the first point, held until it, then the curve"
    );

    let (plan, outcome) = android_pattern(&two_envelopes(), &modern);
    assert_eq!(outcome, Submitted, "several continuous events play exactly");
    let AndroidPatternPlan::Envelope { steps, .. } = plan else {
        panic!("expected an envelope, got {plan:?}");
    };
    assert_eq!(
        envelope_shape(&steps),
        [
            (ms(1), 0.5),
            (ms(19), 0.5),
            (ms(10), 0.0),
            (ms(30), 0.0),
            (ms(1), 1.0),
            (ms(9), 1.0),
            (ms(20), 0.0),
        ],
        "a silent step joins the events"
    );

    let (plan, outcome) = android_pattern(&mixed(), &modern);
    assert_eq!(
        outcome, Approximated,
        "a transient is a pulse within the envelope"
    );
    let AndroidPatternPlan::Envelope { steps, .. } = plan else {
        panic!("expected an envelope, got {plan:?}");
    };
    assert_eq!(
        envelope_shape(&steps),
        [
            (ms(1), 0.5),
            (ms(8), 0.5),
            (ms(1), 0.0),
            (ms(40), 0.0),
            (ms(1), 0.6),
            (ms(29), 0.6),
            (ms(30), 0.2),
            (ms(40), 0.0),
        ]
    );
    assert_eq!(
        android_pattern(&overlapping(), &modern).1,
        FellBack,
        "overlapping events take the waveform route, which cannot play them"
    );

    let (plan, outcome) = android_pattern(
        &transients(),
        &vibrator(AndroidHapticsApi::TouchUsage, false, ALL_PRIMITIVES, true),
    );
    assert_eq!(
        outcome, Approximated,
        "sharpness is quantized to a primitive"
    );
    assert_eq!(
        plan,
        AndroidPatternPlan::Composition(vec![
            ComposedPrimitive {
                primitive: AndroidPrimitive::CLICK,
                scale: 1.0,
                at: ms(0),
            },
            ComposedPrimitive {
                primitive: AndroidPrimitive::LOW_TICK,
                scale: 0.3,
                at: ms(40),
            },
        ])
    );
    assert_eq!(
        android_pattern(
            &transients(),
            &vibrator(AndroidHapticsApi::TouchUsage, false, &[1, 7], true)
        ),
        (AndroidPatternPlan::FallBack, FellBack),
        "one unsupported primitive falls back"
    );
    assert_eq!(
        android_pattern(
            &transients(),
            &vibrator(AndroidHapticsApi::Composition, false, ALL_PRIMITIVES, true)
        )
        .1,
        FellBack,
        "LOW_TICK arrived in API 31"
    );

    let (plan, outcome) = android_pattern(
        &mixed(),
        &vibrator(AndroidHapticsApi::TouchUsage, false, ALL_PRIMITIVES, true),
    );
    assert_eq!(outcome, Approximated, "sharpness dropped");
    assert_eq!(
        plan,
        AndroidPatternPlan::Waveform(vec![
            (ms(10), 127),
            (ms(40), 0),
            (ms(30), 153),
            (ms(30), 51),
            (ms(40), 0),
        ]),
        "steps take the lower end and amplitudes round down"
    );
    assert_eq!(
        android_pattern(
            &mixed(),
            &vibrator(AndroidHapticsApi::TouchUsage, false, ALL_PRIMITIVES, false)
        )
        .1,
        FellBack,
        "no amplitude control"
    );
    assert_eq!(
        android_pattern(
            &overlapping(),
            &vibrator(AndroidHapticsApi::TouchUsage, false, ALL_PRIMITIVES, true)
        )
        .1,
        FellBack
    );
    assert_eq!(
        android_pattern(
            &one_envelope(),
            &vibrator(
                AndroidHapticsApi::SegmentFeedback,
                true,
                ALL_PRIMITIVES,
                true
            )
        )
        .1,
        Approximated,
        "envelopes need API 36"
    );
    assert_eq!(
        android_pattern(
            &transients(),
            &vibrator(AndroidHapticsApi::Floor, false, ALL_PRIMITIVES, true)
        )
        .1,
        FellBack,
        "no composition before API 30"
    );
    // A Galaxy S21 on Android 15: amplitude control, no primitives. Its
    // transients fall back, its continuous events play as a waveform.
    let amplitude_only = vibrator(AndroidHapticsApi::SegmentFeedback, false, &[], true);
    assert_eq!(android_pattern(&transients(), &amplitude_only).1, FellBack);
    assert!(matches!(
        android_pattern(&one_envelope(), &amplitude_only),
        (AndroidPatternPlan::Waveform(_), Approximated)
    ));
    // Without the VIBRATE permission the backend offers the vibrator nothing.
    for pattern in [transients(), one_envelope()] {
        assert_eq!(
            android_pattern(
                &pattern,
                &vibrator(AndroidHapticsApi::SegmentFeedback, false, &[], false)
            )
            .1,
            FellBack
        );
    }
    // Pattern vibrations are touch feedback: VibrationAttributes from API
    // 33, sonification audio attributes before it.
    assert_eq!(
        android_vibration_usage(AndroidHapticsApi::TouchUsage),
        AndroidVibrationUsage::Attributes {
            usage: ANDROID_USAGE_TOUCH
        }
    );
    assert_eq!(
        android_vibration_usage(AndroidHapticsApi::RoundPrimitives),
        AndroidVibrationUsage::Audio {
            usage: ANDROID_USAGE_ASSISTANCE_SONIFICATION
        }
    );
    assert_eq!(
        (ANDROID_USAGE_TOUCH, ANDROID_USAGE_ASSISTANCE_SONIFICATION),
        (18, 13),
        "the platform's constants"
    );
}

fn windows_rows() {
    let full = PenController::new(
        vec![
            PenWaveform::Click,
            PenWaveform::Press,
            PenWaveform::Release,
            PenWaveform::BuzzContinuous,
            PenWaveform::Success,
            PenWaveform::Error,
        ],
        true,
    );
    assert_eq!(windows_feedback(HapticFeedback::LightImpact, None), None);
    assert_eq!(
        windows_feedback(HapticFeedback::DragLift, Some(&full)),
        Some((PenWaveform::Press, Submitted))
    );
    assert_eq!(
        windows_feedback(HapticFeedback::Reject, Some(&full)),
        Some((PenWaveform::Error, Approximated))
    );
    let click_only = PenController::new(vec![PenWaveform::Click], false);
    assert_eq!(
        windows_feedback(HapticFeedback::Drop, Some(&click_only)),
        Some((PenWaveform::Click, Approximated))
    );
    assert_eq!(
        windows_feedback(HapticFeedback::Drop, Some(&PenController::default())),
        None
    );
    let (steps, outcome) = windows_pattern(&mixed(), Some(&full)).expect("a pen");
    assert_eq!(outcome, Approximated);
    assert_eq!(steps[1].waveform, PenWaveform::BuzzContinuous);
    assert_eq!(steps[1].length, ms(100));
    assert_eq!(
        steps[1].intensity,
        Some(0.0),
        "never above the curve's lowest"
    );
    let (steps, _) = windows_pattern(
        &transients(),
        Some(&PenController::new(vec![PenWaveform::Click], false)),
    )
    .expect("a pen");
    assert!(
        steps.iter().all(|step| step.intensity.is_none()),
        "intensity unsupported"
    );
    assert_eq!(
        windows_pattern(&mixed(), Some(&click_only)).map(|p| p.1),
        Some(FellBack)
    );
    assert_eq!(windows_pattern(&mixed(), None), None);

    assert_eq!(windows_capabilities(None), HapticCapabilities::NONE);
    assert_eq!(
        windows_capabilities(Some(&PenController::new(vec![PenWaveform::Press], true))),
        HapticCapabilities::NONE,
        "without Click not every kind plays"
    );
    assert_eq!(
        windows_capabilities(Some(&full)),
        HapticCapabilities::new(true, PatternPlayback::Approximated, true, false)
    );
    assert_eq!(
        windows_capabilities(Some(&click_only)),
        HapticCapabilities::new(true, PatternPlayback::Approximated, false, false)
    );
}

/// What a pen is told to play, and that no waveform outlasts its step.
fn pen_timeline_rows() {
    let stopped = PenController::new(vec![PenWaveform::Click, PenWaveform::BuzzContinuous], true);
    let timed = stopped.clone().with_play_duration(true);
    let (steps, _) = windows_pattern(&mixed(), Some(&stopped)).expect("a pen");
    assert_eq!(
        pen_timeline(&steps, &stopped),
        [
            PenCommand::Play {
                at: ms(0),
                waveform: PenWaveform::Click,
                intensity: Some(0.5),
                length: None,
            },
            PenCommand::Play {
                at: ms(50),
                waveform: PenWaveform::BuzzContinuous,
                intensity: Some(0.0),
                length: None,
            },
            PenCommand::Stop { at: ms(150) },
        ],
        "a controller without a play duration is stopped at the step's end"
    );
    assert_eq!(
        pen_timeline(&steps, &timed)[1],
        PenCommand::Play {
            at: ms(50),
            waveform: PenWaveform::BuzzContinuous,
            intensity: Some(0.0),
            length: Some(ms(100)),
        },
        "one that takes a play duration is handed the length"
    );
    assert_eq!(pen_timeline(&steps, &timed).len(), 2);

    assert_eq!(
        pen_timeline(&[pen_feedback_step(PenWaveform::BuzzContinuous)], &stopped),
        [
            PenCommand::Play {
                at: ms(0),
                waveform: PenWaveform::BuzzContinuous,
                intensity: None,
                length: None,
            },
            PenCommand::Stop { at: PEN_BUZZ },
        ],
        "a semantic buzz ends"
    );
    assert_eq!(
        pen_timeline(&[pen_feedback_step(PenWaveform::Click)], &stopped),
        [PenCommand::Play {
            at: ms(0),
            waveform: PenWaveform::Click,
            intensity: None,
            length: None,
        }]
    );

    let back_to_back = HapticPattern::builder()
        .continuous(ms(0), [(ms(20), 0.5, 0.5), (ms(30), 0.0, 0.5)])
        .transient(ms(30), 0.4, 0.4)
        .build()
        .expect("valid");
    let (steps, _) = windows_pattern(&back_to_back, Some(&stopped)).expect("a pen");
    let commands = pen_timeline(&steps, &stopped);
    assert_eq!(commands[1], PenCommand::Stop { at: ms(30) });
    assert!(
        matches!(commands[2], PenCommand::Play { at, waveform: PenWaveform::Click, .. } if at == ms(30)),
        "a step starting as another ends plays after its stop: {commands:?}"
    );
}

fn macos_rows() {
    assert_eq!(
        macos_feedback(HapticFeedback::Alignment),
        (MacFeedback::Alignment, Submitted)
    );
    assert_eq!(
        macos_feedback(HapticFeedback::LevelChange),
        (MacFeedback::LevelChange, Submitted)
    );
    for kind in HapticFeedback::ALL {
        if !matches!(
            kind,
            HapticFeedback::Alignment | HapticFeedback::LevelChange
        ) {
            assert_eq!(
                macos_feedback(kind),
                (MacFeedback::Generic, Approximated),
                "{kind:?}"
            );
        }
    }
    assert_eq!(macos_pattern(), FellBack);
}

fn web_rows() {
    assert_eq!(
        web_feedback(HapticFeedback::Vibrate, WebVibration::Available),
        Some((vec![200], Submitted))
    );
    assert_eq!(
        web_feedback(HapticFeedback::Confirm, WebVibration::Available).map(|f| f.1),
        Some(Approximated)
    );
    assert_eq!(
        web_feedback(HapticFeedback::Confirm, WebVibration::Unavailable),
        None
    );
    assert_eq!(
        web_pattern(&mixed(), WebVibration::Available),
        Some((vec![10, 40, 60], Approximated)),
        "a pulse, a pause, the ramp's nonzero steps merged, the trailing pause dropped"
    );
    assert_eq!(
        web_pattern(&overlapping(), WebVibration::Available),
        Some((Vec::new(), FellBack))
    );
    assert_eq!(web_pattern(&mixed(), WebVibration::Unavailable), None);
}

fn linux_rows() {
    assert_eq!(
        linux().unsupported().map(|unsupported| unsupported.reason),
        Some(flui_platform::UnsupportedReason::NotOnThisPlatform)
    );
}

#[test]
fn haptic_degradation_table() {
    crate::table_test::run_table(
        "haptic degradation",
        &[
            ("ios", ios_rows),
            ("android_feedback", android_feedback_rows),
            ("android_pattern", android_pattern_rows),
            ("windows", windows_rows),
            ("pen_timeline", pen_timeline_rows),
            ("macos", macos_rows),
            ("web", web_rows),
            ("linux", linux_rows),
        ],
    );
}

fn the_rule_resolves_unknown_devices_to_the_window() {
    let pen = DeviceId::new(std::num::NonZeroU64::new(7).expect("nonzero"));
    let gone = DeviceId::new(std::num::NonZeroU64::new(8).expect("nonzero"));
    assert_eq!(actuator_for(Some(pen), |device| device == pen), Some(pen));
    assert_eq!(actuator_for(Some(gone), |device| device == pen), None);
    assert_eq!(actuator_for(None, |_| true), None);
}

fn the_headless_window_plays_a_stale_source_on_its_actuator() {
    let saved = std::rc::Rc::new(std::cell::RefCell::new(None));
    let installed = std::rc::Rc::clone(&saved);
    Box::new(HeadlessPlatform::new())
        .run(Box::new(move |owner| {
            *installed.borrow_mut() = Some(owner);
            Ok(())
        }))
        .expect("headless owner");
    let owner: OwnerPlatform = saved.borrow_mut().take().expect("started owner");
    let window = owner
        .open_window(WindowOptions::default())
        .expect("open")
        .try_ready()
        .expect("ready");
    let haptics: Arc<dyn PlatformHaptics> = window.haptics().expect("headless has an actuator");
    let fake = haptics
        .as_any()
        .downcast_ref::<FakeHaptics>()
        .expect("the headless actuator records");
    let unplugged = DeviceId::new(std::num::NonZeroU64::new(42).expect("nonzero"));
    let request = HapticRequest::new(HapticTiming::Immediate).with_source(unplugged);
    assert_eq!(
        haptics.perform_with(HapticFeedback::Drop, &request),
        Submitted,
        "a stale source is not an error"
    );
    assert_eq!(
        fake.calls()[0].actuator,
        None,
        "it played on the window's actuator"
    );
}

#[test]
fn stale_haptic_source_plays_on_the_window_actuator() {
    crate::table_test::run_table(
        "stale haptic source",
        &[
            (
                "the_rule_resolves_unknown_devices_to_the_window",
                the_rule_resolves_unknown_devices_to_the_window,
            ),
            (
                "the_headless_window_plays_a_stale_source_on_its_actuator",
                the_headless_window_plays_a_stale_source_on_its_actuator,
            ),
        ],
    );
}
