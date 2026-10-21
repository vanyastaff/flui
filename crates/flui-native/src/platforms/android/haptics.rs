//! Android haptics (ADR-0210 §7): `View.performHapticFeedback` for the
//! semantic tier, the `Vibrator` for patterns.
//!
//! Semantic feedback goes to the decor view on the Java main thread, which
//! owns the view hierarchy, so the call is posted and the owner never waits
//! for it (ADR-0247 §5). One call waits at a time: a request made while it
//! waits is `Suppressed` rather than queued to play late, and a request
//! reports `Suppressed` while the view declines (an answer the Java thread
//! publishes as it makes each call), the degradation table's outcome
//! otherwise (`AndroidViewAnswers`).
//! Patterns go to the `Vibrator`, which any thread may
//! call: one envelope for the whole pattern on API 36 devices that have
//! them, else a composition of primitives or an amplitude waveform, as
//! `android_pattern` plans, each with the touch usage
//! (`android_vibration_usage`). What the vibrator offers is read once, on
//! the first pattern, and kept; an app without the `VIBRATE` permission is
//! offered nothing, so its patterns fall back to the view's feedback.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use android_activity::AndroidApp;
use jni::objects::JObject;
use jni::{JValue, jni_sig, jni_str};

use crate::shared::haptic_degradation::{
    AndroidFeedbackAnswer, AndroidFeedbackCall, AndroidHapticsApi, AndroidPatternPlan,
    AndroidPrimitive, AndroidVibrationUsage, AndroidVibrator, AndroidViewAnswers,
    ComposedPrimitive, EnvelopeStep, android_feedback, android_pattern, android_vibration_usage,
};
use crate::shared::os_version::android::{
    AndroidApi, EnvelopeEffects, HapticComposition, RoundPrimitives, SegmentFeedback,
    VibrationTouchUsage,
};

/// `PackageManager.PERMISSION_GRANTED`.
const PERMISSION_GRANTED: i32 = 0;
use flui_platform_api::haptics::{
    HapticCapabilities, HapticOutcome, HapticPattern, HapticRequest, PatternPlayback,
};
use flui_platform_api::{DeviceId, HapticFeedback, PlatformHaptics};

/// The primitives the composition plan can choose from.
const PRIMITIVES: [AndroidPrimitive; 4] = [
    AndroidPrimitive::CLICK,
    AndroidPrimitive::THUD,
    AndroidPrimitive::TICK,
    AndroidPrimitive::LOW_TICK,
];

/// The window's haptic actuator: the Activity's decor view and the device
/// vibrator.
pub(super) struct AndroidHaptics {
    app: AndroidApp,
    /// The haptics APIs of the device, from its API level.
    api: AndroidHapticsApi,
    vibrator: OnceLock<AndroidVibrator>,
    /// What the decor view answered the latest semantic feedback call.
    view_answers: Arc<AndroidViewAnswers>,
}

impl std::fmt::Debug for AndroidHaptics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidHaptics")
            .field("api", &self.api)
            .field("vibrator", &self.vibrator.get())
            .field("view_answers", &self.view_answers)
            .finish_non_exhaustive()
    }
}

/// The haptics APIs a device at `api` has: the newest set whose gate it passes.
fn haptics_api(api: AndroidApi) -> AndroidHapticsApi {
    if api.since::<EnvelopeEffects>().is_some() {
        AndroidHapticsApi::Envelopes
    } else if api.since::<SegmentFeedback>().is_some() {
        AndroidHapticsApi::SegmentFeedback
    } else if api.since::<VibrationTouchUsage>().is_some() {
        AndroidHapticsApi::TouchUsage
    } else if api.since::<RoundPrimitives>().is_some() {
        AndroidHapticsApi::RoundPrimitives
    } else if api.since::<HapticComposition>().is_some() {
        AndroidHapticsApi::Composition
    } else {
        AndroidHapticsApi::Floor
    }
}

fn millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

impl AndroidHaptics {
    pub(super) fn new(app: AndroidApp, api: AndroidApi) -> Self {
        Self {
            app,
            api: haptics_api(api),
            vibrator: OnceLock::new(),
            view_answers: Arc::new(AndroidViewAnswers::new()),
        }
    }

    /// The vibrator's facts, read on first use. A device whose vibrator
    /// cannot be read offers nothing, so its patterns fall back, and so does
    /// an app without the `VIBRATE` permission, which `Vibrator.vibrate`
    /// requires: semantic feedback goes through the view and needs none.
    fn vibrator(&self) -> &AndroidVibrator {
        self.vibrator.get_or_init(|| {
            let api = self.api;
            super::preferences::with_activity(&self.app, |env, activity| {
                let permission = env.new_string("android.permission.VIBRATE")?;
                let granted = env
                    .call_method(
                        activity,
                        jni_str!("checkSelfPermission"),
                        jni_sig!("(Ljava/lang/String;)I"),
                        &[JValue::Object(&permission)],
                    )?
                    .i()?
                    == PERMISSION_GRANTED;
                if !granted {
                    tracing::warn!(
                        target: "flui_native::android",
                        "the manifest lacks android.permission.VIBRATE; haptic patterns fall back to feedback"
                    );
                    return Ok(AndroidVibrator::new(api, false, Vec::new(), false));
                }
                let vibrator = system_vibrator(env, activity)?;
                let amplitude_control = env
                    .call_method(
                        &vibrator,
                        jni_str!("hasAmplitudeControl"),
                        jni_sig!("()Z"),
                        &[],
                    )?
                    .z()?;
                let mut primitives = Vec::new();
                if api.includes(AndroidHapticsApi::Composition) {
                    for primitive in PRIMITIVES {
                        if !api.includes(primitive.since) {
                            continue;
                        }
                        let ids = env.new_int_array(1)?;
                        ids.set_region(env, 0, &[primitive.value])?;
                        let supported = env
                            .call_method(
                                &vibrator,
                                jni_str!("areAllPrimitivesSupported"),
                                jni_sig!("([I)Z"),
                                &[JValue::Object(&ids)],
                            )?
                            .z()?;
                        if supported {
                            primitives.push(primitive.value);
                        }
                    }
                }
                let envelopes = api.includes(AndroidHapticsApi::Envelopes)
                    && env
                        .call_method(
                            &vibrator,
                            jni_str!("areEnvelopeEffectsSupported"),
                            jni_sig!("()Z"),
                            &[],
                        )?
                        .z()?;
                Ok(AndroidVibrator::new(
                    api,
                    envelopes,
                    primitives,
                    amplitude_control,
                ))
            })
            .unwrap_or_else(|error| {
                tracing::warn!(
                    target: "flui_native::android",
                    %error,
                    "the vibrator could not be read; haptic patterns fall back"
                );
                AndroidVibrator::new(api, false, Vec::new(), false)
            })
        })
    }

    /// Play `plan` on the vibrator; `false` when the call failed.
    fn vibrate(&self, plan: &AndroidPatternPlan) -> bool {
        let played = super::preferences::with_activity(&self.app, |env, activity| {
            let vibrator = system_vibrator(env, activity)?;
            let effect = match plan {
                AndroidPatternPlan::Envelope {
                    initial_sharpness,
                    steps,
                } => envelope(env, *initial_sharpness, steps)?,
                AndroidPatternPlan::Composition(primitives) => {
                    composition(env, &vibrator, self.api, primitives)?
                }
                AndroidPatternPlan::Waveform(steps) => waveform(env, steps)?,
                _ => return Ok(false),
            };
            match android_vibration_usage(self.api) {
                AndroidVibrationUsage::Attributes { usage } => {
                    let attributes = env
                        .call_static_method(
                            jni_str!("android/os/VibrationAttributes"),
                            jni_str!("createForUsage"),
                            jni_sig!("(I)Landroid/os/VibrationAttributes;"),
                            &[JValue::Int(usage)],
                        )?
                        .l()?;
                    env.call_method(
                        &vibrator,
                        jni_str!("vibrate"),
                        jni_sig!("(Landroid/os/VibrationEffect;Landroid/os/VibrationAttributes;)V"),
                        &[JValue::Object(&effect), JValue::Object(&attributes)],
                    )?;
                }
                AndroidVibrationUsage::Audio { usage } => {
                    let builder = env.new_object(
                        jni_str!("android/media/AudioAttributes$Builder"),
                        jni_sig!("()V"),
                        &[],
                    )?;
                    env.call_method(
                        &builder,
                        jni_str!("setUsage"),
                        jni_sig!("(I)Landroid/media/AudioAttributes$Builder;"),
                        &[JValue::Int(usage)],
                    )?;
                    let attributes = env
                        .call_method(
                            &builder,
                            jni_str!("build"),
                            jni_sig!("()Landroid/media/AudioAttributes;"),
                            &[],
                        )?
                        .l()?;
                    env.call_method(
                        &vibrator,
                        jni_str!("vibrate"),
                        jni_sig!("(Landroid/os/VibrationEffect;Landroid/media/AudioAttributes;)V"),
                        &[JValue::Object(&effect), JValue::Object(&attributes)],
                    )?;
                }
            }
            Ok(true)
        });
        played.unwrap_or_else(|error| {
            tracing::warn!(
                target: "flui_native::android",
                %error,
                "a haptic pattern did not reach the vibrator"
            );
            false
        })
    }
}

fn system_vibrator<'local>(
    env: &mut jni::Env<'local>,
    activity: &JObject<'_>,
) -> jni::errors::Result<JObject<'local>> {
    let name = env.new_string("vibrator")?;
    env.call_method(
        activity,
        jni_str!("getSystemService"),
        jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
        &[JValue::Object(&name)],
    )?
    .l()
}

fn envelope<'local>(
    env: &mut jni::Env<'local>,
    initial_sharpness: f32,
    steps: &[EnvelopeStep],
) -> jni::errors::Result<JObject<'local>> {
    let builder = env.new_object(
        jni_str!("android/os/VibrationEffect$BasicEnvelopeBuilder"),
        jni_sig!("()V"),
        &[],
    )?;
    env.call_method(
        &builder,
        jni_str!("setInitialSharpness"),
        jni_sig!("(F)Landroid/os/VibrationEffect$BasicEnvelopeBuilder;"),
        &[JValue::Float(initial_sharpness)],
    )?;
    for step in steps {
        env.call_method(
            &builder,
            jni_str!("addControlPoint"),
            jni_sig!("(FFJ)Landroid/os/VibrationEffect$BasicEnvelopeBuilder;"),
            &[
                JValue::Float(step.intensity),
                JValue::Float(step.sharpness),
                JValue::Long(millis(step.duration).max(1)),
            ],
        )?;
    }
    env.call_method(
        &builder,
        jni_str!("build"),
        jni_sig!("()Landroid/os/VibrationEffect;"),
        &[],
    )?
    .l()
}

/// A composition whose primitives start at their pattern times. A delay
/// counts from the end of the primitive before it, so the gap between two
/// starts loses that primitive's duration, which API 31 reports
/// (`getPrimitiveDurations`); on API 30 the gap is used as is.
fn composition<'local>(
    env: &mut jni::Env<'local>,
    vibrator: &JObject<'_>,
    api: AndroidHapticsApi,
    primitives: &[ComposedPrimitive],
) -> jni::errors::Result<JObject<'local>> {
    let mut durations = vec![0_i32; primitives.len()];
    if api.includes(AndroidHapticsApi::RoundPrimitives) && !primitives.is_empty() {
        let ids: Vec<i32> = primitives.iter().map(|step| step.primitive.value).collect();
        let id_array = env.new_int_array(ids.len())?;
        id_array.set_region(env, 0, &ids)?;
        let reported = env
            .call_method(
                vibrator,
                jni_str!("getPrimitiveDurations"),
                jni_sig!("([I)[I"),
                &[JValue::Object(&id_array)],
            )?
            .l()?;
        let reported = jni::objects::JIntArray::cast_local(env, reported)?;
        reported.get_region(env, 0, &mut durations)?;
    }
    let composition = env
        .call_static_method(
            jni_str!("android/os/VibrationEffect"),
            jni_str!("startComposition"),
            jni_sig!("()Landroid/os/VibrationEffect$Composition;"),
            &[],
        )?
        .l()?;
    // When the primitive before ends, after the pattern starts.
    let mut previous_end = Duration::ZERO;
    for (step, duration) in primitives.iter().zip(durations) {
        let delay =
            i32::try_from(step.at.saturating_sub(previous_end).as_millis()).unwrap_or(i32::MAX);
        let length = Duration::from_millis(u64::try_from(duration).unwrap_or(0));
        previous_end = step.at.max(previous_end).saturating_add(length);
        env.call_method(
            &composition,
            jni_str!("addPrimitive"),
            jni_sig!("(IFI)Landroid/os/VibrationEffect$Composition;"),
            &[
                JValue::Int(step.primitive.value),
                JValue::Float(step.scale),
                JValue::Int(delay),
            ],
        )?;
    }
    env.call_method(
        &composition,
        jni_str!("compose"),
        jni_sig!("()Landroid/os/VibrationEffect;"),
        &[],
    )?
    .l()
}

fn waveform<'local>(
    env: &mut jni::Env<'local>,
    steps: &[(Duration, u8)],
) -> jni::errors::Result<JObject<'local>> {
    let timings: Vec<i64> = steps
        .iter()
        .map(|(duration, _)| millis(*duration))
        .collect();
    let amplitudes: Vec<i32> = steps.iter().map(|(_, level)| i32::from(*level)).collect();
    let timing_array = env.new_long_array(timings.len())?;
    timing_array.set_region(env, 0, &timings)?;
    let amplitude_array = env.new_int_array(amplitudes.len())?;
    amplitude_array.set_region(env, 0, &amplitudes)?;
    env.call_static_method(
        jni_str!("android/os/VibrationEffect"),
        jni_str!("createWaveform"),
        jni_sig!("([J[II)Landroid/os/VibrationEffect;"),
        &[
            JValue::Object(&timing_array),
            JValue::Object(&amplitude_array),
            JValue::Int(-1),
        ],
    )?
    .l()
}

/// Publishes the decor view's answer to one posted call; `Failed` when the
/// call is dropped unrun.
struct Answering(Option<Arc<AndroidViewAnswers>>);

impl Answering {
    fn answer(mut self, answer: AndroidFeedbackAnswer) {
        if let Some(answers) = self.0.take() {
            answers.answer(answer);
        }
    }
}

impl Drop for Answering {
    fn drop(&mut self) {
        if let Some(answers) = self.0.take() {
            answers.answer(AndroidFeedbackAnswer::Failed);
        }
    }
}

/// Post `constant` to the decor view on the Java main thread, which owns it,
/// without waiting: a busy Java thread, or one blocked on the native thread
/// in a lifecycle callback, never stalls the owner (ADR-0247 §5). What
/// `performHapticFeedback` returns is published to `answers`, which the next
/// request reports from.
fn perform_on_view(app: &AndroidApp, constant: i32, answers: &Arc<AndroidViewAnswers>) {
    let answering = Answering(Some(Arc::clone(answers)));
    let app = app.clone();
    app.clone().run_on_java_main_thread(Box::new(move || {
        let result = super::preferences::with_activity(&app, |env, activity| {
            let window = env
                .call_method(
                    activity,
                    jni_str!("getWindow"),
                    jni_sig!("()Landroid/view/Window;"),
                    &[],
                )?
                .l()?;
            let decor = env
                .call_method(
                    &window,
                    jni_str!("getDecorView"),
                    jni_sig!("()Landroid/view/View;"),
                    &[],
                )?
                .l()?;
            env.call_method(
                &decor,
                jni_str!("performHapticFeedback"),
                jni_sig!("(I)Z"),
                &[JValue::Int(constant)],
            )?
            .z()
        });
        let answer = match result {
            Ok(true) => AndroidFeedbackAnswer::Played,
            Ok(false) => AndroidFeedbackAnswer::Declined,
            Err(error) => {
                tracing::warn!(
                    target: "flui_native::android",
                    %error,
                    "haptic feedback did not reach the decor view"
                );
                AndroidFeedbackAnswer::Failed
            }
        };
        answering.answer(answer);
    }));
}

impl PlatformHaptics for AndroidHaptics {
    fn perform(&self, feedback: HapticFeedback) {
        let _ = self.perform_with(feedback, &HapticRequest::default());
    }

    fn perform_with(&self, feedback: HapticFeedback, _request: &HapticRequest) -> HapticOutcome {
        let (constant, table) = android_feedback(feedback, self.api);
        let call = self.view_answers.request(table);
        if let AndroidFeedbackCall::Post(_) = call {
            perform_on_view(&self.app, constant.value, &self.view_answers);
        }
        call.outcome()
    }

    fn play(
        &self,
        pattern: &HapticPattern,
        fallback: HapticFeedback,
        request: &HapticRequest,
    ) -> HapticOutcome {
        let (plan, outcome) = android_pattern(pattern, self.vibrator());
        if outcome != HapticOutcome::FellBack && self.vibrate(&plan) {
            return outcome;
        }
        match self.perform_with(fallback, request) {
            played if played.played() => HapticOutcome::FellBack,
            other => other,
        }
    }

    fn capabilities(&self, _source: Option<DeviceId>) -> HapticCapabilities {
        let vibrator = self.vibrator();
        let patterns = if vibrator.envelopes {
            // Exact for one continuous event; other patterns approximate.
            PatternPlayback::Exact
        } else if vibrator.api.includes(AndroidHapticsApi::Composition)
            && (!vibrator.primitives.is_empty() || vibrator.amplitude_control)
        {
            PatternPlayback::Approximated
        } else {
            PatternPlayback::FallsBack
        };
        HapticCapabilities::new(
            true,
            patterns,
            vibrator.amplitude_control,
            vibrator.envelopes,
        )
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
