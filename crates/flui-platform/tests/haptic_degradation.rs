//! Every backend's haptics degradation table (ADR-0210 §7), run as pure
//! functions on every host, and the stale-source rule (§5).

use std::sync::Arc;
use std::time::Duration;

use flui_platform::shared::haptic_degradation::{
    AndroidHapticConstant, AndroidPatternPlan, AndroidPrimitive, AndroidVibrator,
    ComposedPrimitive, CoreHapticsEngine, IosGenerator, IosImpact, IosNotification, MacFeedback,
    PenController, PenWaveform, WebVibration, actuator_for, android_feedback, android_pattern,
    ios_feedback, ios_pattern, linux, macos_feedback, macos_pattern, web_feedback, web_pattern,
    windows_feedback, windows_pattern,
};
use flui_platform::traits::WindowOptions;
use flui_platform::{FakeHaptics, HeadlessPlatform, OwnerPlatform, Platform as _};
use flui_platform_api::haptics::{HapticOutcome, HapticPattern, HapticRequest, HapticTiming};
use flui_platform_api::{DeviceId, HapticFeedback, PlatformHaptics};

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
        let (constant, outcome) = android_feedback(kind, 34);
        assert_eq!(
            (constant.value, outcome),
            (new_value, new_outcome),
            "{kind:?} on 34"
        );
        let (constant, outcome) = android_feedback(kind, 29);
        assert_eq!(
            (constant.value, outcome),
            (old_value, old_outcome),
            "{kind:?} on 29"
        );
        assert!(
            constant.since <= 29,
            "{kind:?}: no constant newer than the device"
        );
    }
    assert_eq!(AndroidHapticConstant::DRAG_START.since, 34);
}

fn vibrator(
    api_level: u32,
    envelopes: bool,
    primitives: &[i32],
    amplitude: bool,
) -> AndroidVibrator {
    AndroidVibrator::new(api_level, envelopes, primitives.to_vec(), amplitude)
}

const ALL_PRIMITIVES: &[i32] = &[1, 2, 7, 8];

fn android_pattern_rows() {
    let modern = vibrator(36, true, ALL_PRIMITIVES, true);
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
        steps.iter().map(|step| step.duration).collect::<Vec<_>>(),
        [ms(20), ms(50)]
    );

    let (plan, outcome) =
        android_pattern(&transients(), &vibrator(33, false, ALL_PRIMITIVES, true));
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
        android_pattern(&transients(), &vibrator(33, false, &[1, 7], true)),
        (AndroidPatternPlan::FallBack, FellBack),
        "one unsupported primitive falls back"
    );
    assert_eq!(
        android_pattern(&transients(), &vibrator(30, false, ALL_PRIMITIVES, true)).1,
        FellBack,
        "LOW_TICK arrived in API 31"
    );

    let (plan, outcome) = android_pattern(&mixed(), &vibrator(33, false, ALL_PRIMITIVES, true));
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
        android_pattern(&mixed(), &vibrator(33, false, ALL_PRIMITIVES, false)).1,
        FellBack,
        "no amplitude control"
    );
    assert_eq!(
        android_pattern(&overlapping(), &vibrator(33, false, ALL_PRIMITIVES, true)).1,
        FellBack
    );
    assert_eq!(
        android_pattern(&one_envelope(), &vibrator(35, true, ALL_PRIMITIVES, true)).1,
        Approximated,
        "envelopes need API 36"
    );
    assert_eq!(
        android_pattern(&transients(), &vibrator(29, false, ALL_PRIMITIVES, true)).1,
        FellBack,
        "no composition before API 30"
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
    assert_eq!(linux(), HapticOutcome::Unavailable);
}

#[test]
fn haptic_degradation_table() {
    super::run_table(
        "haptic degradation",
        &[
            ("ios", ios_rows),
            ("android_feedback", android_feedback_rows),
            ("android_pattern", android_pattern_rows),
            ("windows", windows_rows),
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
    super::run_table(
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
