//! UIKit haptics: the feedback generators and Core Haptics (ADR-0210 §7–§8).
//!
//! From iOS 17.5 the generators bind to the window's view
//! (`feedbackGeneratorForView:`), so the feedback comes from the device the
//! view's scene is on; before it the view-less initializers play unaddressed
//! and the degradation table reports every outcome approximated.
//!
//! Patterns play exactly through Core Haptics, each event as its own
//! `CHHapticPattern` started at its time on the engine's clock (see
//! `ios_pattern_parts`). The engine is created and started on the first
//! pattern. When it stops (interruption, suspension, idle shutdown) or resets
//! after a haptic-server error, its handlers, which run on an internal queue,
//! only record a restart obligation and wake the owner, the main thread,
//! which restarts the engine; a pattern requested while it is down plays its
//! tier-1 fallback. Hardware without Core Haptics falls every pattern back.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Weak};

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::{AllocAnyThread, MainThreadMarker};
use objc2_core_haptics::{
    CHHapticDeviceCapability, CHHapticDynamicParameterIDHapticIntensityControl,
    CHHapticDynamicParameterIDHapticSharpnessControl, CHHapticEngine, CHHapticEngineStoppedReason,
    CHHapticEvent, CHHapticEventParameter, CHHapticEventParameterIDHapticIntensity,
    CHHapticEventParameterIDHapticSharpness, CHHapticEventTypeHapticContinuous,
    CHHapticEventTypeHapticTransient, CHHapticParameterCurve, CHHapticParameterCurveControlPoint,
    CHHapticPattern, CHHapticPatternPlayer,
};
use objc2_foundation::{NSArray, NSError, NSOperatingSystemVersion, NSProcessInfo};
use objc2_ui_kit::{
    UIImpactFeedbackGenerator, UIImpactFeedbackStyle, UINotificationFeedbackGenerator,
    UINotificationFeedbackType, UISelectionFeedbackGenerator, UIView,
};

use crate::shared::haptic_degradation::{
    CoreHapticsControl, CoreHapticsEngine, CoreHapticsEvent, CoreHapticsPart, IosGenerator,
    IosImpact, IosNotification, ios_feedback, ios_pattern, ios_pattern_parts,
};
use flui_platform_api::haptics::{HapticCapabilities, HapticOutcome, HapticPattern, HapticRequest};
use flui_platform_api::{DeviceId, HapticFeedback, PlatformHaptics};

/// Whether the generators take a view (iOS 17.5+).
fn view_bound() -> bool {
    NSProcessInfo::processInfo().isOperatingSystemAtLeastVersion(NSOperatingSystemVersion {
        majorVersion: 17,
        minorVersion: 5,
        patchVersion: 0,
    })
}

const fn impact_style(impact: IosImpact) -> UIImpactFeedbackStyle {
    match impact {
        IosImpact::Light => UIImpactFeedbackStyle::Light,
        IosImpact::Medium => UIImpactFeedbackStyle::Medium,
        IosImpact::Rigid => UIImpactFeedbackStyle::Rigid,
        IosImpact::Soft => UIImpactFeedbackStyle::Soft,
        _ => UIImpactFeedbackStyle::Heavy,
    }
}

const fn notification_type(notification: IosNotification) -> UINotificationFeedbackType {
    match notification {
        IosNotification::Success => UINotificationFeedbackType::Success,
        IosNotification::Warning => UINotificationFeedbackType::Warning,
        _ => UINotificationFeedbackType::Error,
    }
}

/// Whether the hardware plays Core Haptics.
fn supports_core_haptics() -> bool {
    // SAFETY: `capabilitiesForHardware` is a class method with no
    // preconditions, and `supportsHaptics` a read-only property of the
    // capability object it returns, which this binding retains.
    unsafe { CHHapticEngine::capabilitiesForHardware().supportsHaptics() }
}

const NOT_CREATED: u8 = 0;
const RUNNING: u8 = 1;
const RESTARTING: u8 = 2;
const UNSUPPORTED: u8 = 3;

/// The window's Core Haptics engine and its restart obligation.
struct CoreHaptics {
    engine: super::native_owner::NativeOwner<RefCell<Option<Retained<CHHapticEngine>>>>,
    state: AtomicU8,
    restart_in_flight: AtomicBool,
}

impl CoreHaptics {
    fn new(mtm: MainThreadMarker) -> Self {
        Self {
            engine: super::native_owner::NativeOwner::new(RefCell::new(None), mtm),
            state: AtomicU8::new(NOT_CREATED),
            restart_in_flight: AtomicBool::new(false),
        }
    }

    /// The engine's state, creating and starting it on first use, and
    /// scheduling a restart while one is owed.
    fn engine_state(self: &Arc<Self>, mtm: MainThreadMarker) -> CoreHapticsEngine {
        match self.state.load(Ordering::Acquire) {
            NOT_CREATED => self.create(mtm),
            RUNNING => CoreHapticsEngine::Running,
            RESTARTING => {
                Self::schedule_restart(Arc::downgrade(self));
                CoreHapticsEngine::Restarting
            }
            _ => CoreHapticsEngine::Unsupported,
        }
    }

    fn create(self: &Arc<Self>, mtm: MainThreadMarker) -> CoreHapticsEngine {
        if !supports_core_haptics() {
            self.state.store(UNSUPPORTED, Ordering::Release);
            return CoreHapticsEngine::Unsupported;
        }
        // SAFETY: `initAndReturnError:` takes a freshly allocated engine and
        // has no other preconditions; failure is returned as an error.
        let engine = match unsafe { CHHapticEngine::initAndReturnError(CHHapticEngine::alloc()) } {
            Ok(engine) => engine,
            Err(error) => {
                tracing::warn!(%error, "the Core Haptics engine could not be created");
                self.state.store(UNSUPPORTED, Ordering::Release);
                return CoreHapticsEngine::Unsupported;
            }
        };
        let stopped_owner = Arc::downgrade(self);
        let stopped = RcBlock::new(move |_reason: CHHapticEngineStoppedReason| {
            Self::engine_down(&stopped_owner);
        });
        let reset_owner = Arc::downgrade(self);
        let reset = RcBlock::new(move || Self::engine_down(&reset_owner));
        // SAFETY: both pointers are live heap blocks of the handler types
        // these setters take; the engine copies each block, so dropping the
        // `RcBlock`s afterwards leaves its copies alive.
        unsafe {
            engine.setStoppedHandler(RcBlock::as_ptr(&stopped));
            engine.setResetHandler(RcBlock::as_ptr(&reset));
        }
        // SAFETY: `startAndReturnError:` has no preconditions on a created
        // engine; failure is returned as an error.
        let started = unsafe { engine.startAndReturnError() };
        *self.engine.get(mtm).borrow_mut() = Some(engine);
        match started {
            Ok(()) => {
                self.state.store(RUNNING, Ordering::Release);
                CoreHapticsEngine::Running
            }
            Err(error) => {
                tracing::warn!(%error, "the Core Haptics engine did not start");
                self.state.store(RESTARTING, Ordering::Release);
                Self::schedule_restart(Arc::downgrade(self));
                CoreHapticsEngine::Restarting
            }
        }
    }

    /// The stopped and reset handlers: record the restart obligation and
    /// wake the owner, nothing else. They run on Core Haptics' own queue.
    fn engine_down(owner: &Weak<Self>) {
        if let Some(core) = owner.upgrade() {
            core.state.store(RESTARTING, Ordering::Release);
            Self::schedule_restart(Arc::downgrade(&core));
        }
    }

    /// Ask the owner (the main thread) to restart the engine.
    fn schedule_restart(owner: Weak<Self>) {
        DispatchQueue::main().exec_async(move || {
            if let Some(core) = owner.upgrade() {
                core.restart();
            }
        });
    }

    /// Restart the engine on the main thread; patterns play again once it
    /// reports started.
    fn restart(self: &Arc<Self>) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        if self.state.load(Ordering::Acquire) != RESTARTING
            || self.restart_in_flight.swap(true, Ordering::AcqRel)
        {
            return;
        }
        let Some(engine) = self.engine.get(mtm).borrow().clone() else {
            self.restart_in_flight.store(false, Ordering::Release);
            return;
        };
        let owner = Arc::downgrade(self);
        let completion = RcBlock::new(move |error: *mut NSError| {
            let Some(core) = owner.upgrade() else {
                return;
            };
            if error.is_null() {
                core.state.store(RUNNING, Ordering::Release);
            } else {
                tracing::warn!("the Core Haptics engine did not restart; patterns fall back");
            }
            core.restart_in_flight.store(false, Ordering::Release);
        });
        // SAFETY: the pointer is a live heap block of the completion type
        // `startWithCompletionHandler:` takes; the engine copies it.
        unsafe { engine.startWithCompletionHandler(RcBlock::as_ptr(&completion)) };
    }

    /// Start every part at its time on the engine's clock; `false` when the
    /// engine refused one, which owes a restart.
    fn play(self: &Arc<Self>, mtm: MainThreadMarker, parts: &[CoreHapticsPart]) -> bool {
        let Some(engine) = self.engine.get(mtm).borrow().clone() else {
            return false;
        };
        // SAFETY: `currentTime` is a read-only property of a live engine.
        let now = unsafe { engine.currentTime() };
        for part in parts {
            // SAFETY: the player is created from a live engine and started at
            // a time on that engine's clock.
            let started = unsafe {
                build_pattern(part).and_then(|pattern| {
                    let player = engine.createPlayerWithPattern_error(&pattern)?;
                    player.startAtTime_error(now + part.at.as_secs_f64())
                })
            };
            if let Err(error) = started {
                tracing::warn!(%error, "a Core Haptics pattern did not play; restarting the engine");
                self.state.store(RESTARTING, Ordering::Release);
                Self::schedule_restart(Arc::downgrade(self));
                return false;
            }
        }
        true
    }
}

/// One part as a `CHHapticPattern`, from freshly allocated Core Haptics
/// objects and the framework's parameter identifiers.
fn build_pattern(part: &CoreHapticsPart) -> Result<Retained<CHHapticPattern>, Retained<NSError>> {
    let parameter = |id, value| {
        // SAFETY: a freshly allocated parameter with a framework identifier.
        unsafe {
            CHHapticEventParameter::initWithParameterID_value(
                CHHapticEventParameter::alloc(),
                id,
                value,
            )
        }
    };
    // SAFETY: the identifiers are the framework's extern constants, valid
    // for the life of the process.
    let (intensity_id, sharpness_id) = unsafe {
        (
            CHHapticEventParameterIDHapticIntensity,
            CHHapticEventParameterIDHapticSharpness,
        )
    };
    let event = match part.event {
        CoreHapticsEvent::Transient {
            intensity,
            sharpness,
        } => {
            let parameters = NSArray::from_retained_slice(&[
                parameter(intensity_id, intensity),
                parameter(sharpness_id, sharpness),
            ]);
            // SAFETY: a freshly allocated event of a framework event type.
            unsafe {
                CHHapticEvent::initWithEventType_parameters_relativeTime(
                    CHHapticEvent::alloc(),
                    CHHapticEventTypeHapticTransient,
                    &parameters,
                    0.0,
                )
            }
        }
        CoreHapticsEvent::Continuous { duration } => {
            let parameters = NSArray::from_retained_slice(&[
                parameter(intensity_id, 1.0),
                parameter(sharpness_id, 0.0),
            ]);
            // SAFETY: a freshly allocated event of a framework event type.
            unsafe {
                CHHapticEvent::initWithEventType_parameters_relativeTime_duration(
                    CHHapticEvent::alloc(),
                    CHHapticEventTypeHapticContinuous,
                    &parameters,
                    0.0,
                    duration.as_secs_f64(),
                )
            }
        }
    };
    let curves: Vec<_> = part
        .curves
        .iter()
        .map(|curve| {
            let points: Vec<_> = curve
                .points
                .iter()
                .map(|&(time, value)| {
                    // SAFETY: a freshly allocated control point.
                    unsafe {
                        CHHapticParameterCurveControlPoint::initWithRelativeTime_value(
                            CHHapticParameterCurveControlPoint::alloc(),
                            time.as_secs_f64(),
                            value,
                        )
                    }
                })
                .collect();
            // SAFETY: the dynamic parameter identifiers are the framework's
            // extern constants.
            let id = unsafe {
                match curve.control {
                    CoreHapticsControl::Sharpness => {
                        CHHapticDynamicParameterIDHapticSharpnessControl
                    }
                    _ => CHHapticDynamicParameterIDHapticIntensityControl,
                }
            };
            // SAFETY: a freshly allocated curve with a framework identifier
            // and at most sixteen control points.
            unsafe {
                CHHapticParameterCurve::initWithParameterID_controlPoints_relativeTime(
                    CHHapticParameterCurve::alloc(),
                    id,
                    &NSArray::from_retained_slice(&points),
                    curve.at.as_secs_f64(),
                )
            }
        })
        .collect();
    // SAFETY: a freshly allocated pattern of the events and curves above.
    unsafe {
        CHHapticPattern::initWithEvents_parameterCurves_error(
            CHHapticPattern::alloc(),
            &NSArray::from_retained_slice(&[event]),
            &NSArray::from_retained_slice(&curves),
        )
    }
}

/// The window's haptic actuator, bound to its view on the main thread.
pub(super) struct IOSHaptics {
    view: super::native_owner::NativeOwner<Retained<UIView>>,
    core: Arc<CoreHaptics>,
}

impl std::fmt::Debug for IOSHaptics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IOSHaptics")
            .field("engine", &self.core.state.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl IOSHaptics {
    pub(super) fn new(view: Retained<UIView>, mtm: MainThreadMarker) -> Self {
        Self {
            view: super::native_owner::NativeOwner::new(view, mtm),
            core: Arc::new(CoreHaptics::new(mtm)),
        }
    }

    /// Play `generator`, through the view from iOS 17.5.
    #[expect(
        deprecated,
        reason = "the view-less initializers are the only ones before iOS 17.5"
    )]
    fn play_generator(&self, generator: IosGenerator, bound: bool, mtm: MainThreadMarker) {
        let view = self.view.get(mtm);
        match generator {
            IosGenerator::Selection => {
                let generator = if bound {
                    UISelectionFeedbackGenerator::feedbackGeneratorForView(view)
                } else {
                    UISelectionFeedbackGenerator::new(mtm)
                };
                generator.selectionChanged();
            }
            IosGenerator::Notification(notification) => {
                let generator = if bound {
                    UINotificationFeedbackGenerator::feedbackGeneratorForView(view)
                } else {
                    UINotificationFeedbackGenerator::new(mtm)
                };
                generator.notificationOccurred(notification_type(notification));
            }
            IosGenerator::Impact(impact) => {
                let style = impact_style(impact);
                let generator = if bound {
                    UIImpactFeedbackGenerator::feedbackGeneratorWithStyle_forView(style, view)
                } else {
                    UIImpactFeedbackGenerator::initWithStyle(mtm.alloc(), style)
                };
                generator.impactOccurred();
            }
        }
    }
}

impl PlatformHaptics for IOSHaptics {
    fn perform(&self, feedback: HapticFeedback) {
        let _ = self.perform_with(feedback, &HapticRequest::default());
    }

    fn perform_with(&self, feedback: HapticFeedback, _request: &HapticRequest) -> HapticOutcome {
        // Requests run on the owner thread, which UIKit makes the main thread.
        let Some(mtm) = MainThreadMarker::new() else {
            return HapticOutcome::Unavailable;
        };
        let bound = view_bound();
        let (generator, outcome) = ios_feedback(feedback, bound);
        self.play_generator(generator, bound, mtm);
        outcome
    }

    fn play(
        &self,
        pattern: &HapticPattern,
        fallback: HapticFeedback,
        request: &HapticRequest,
    ) -> HapticOutcome {
        let Some(mtm) = MainThreadMarker::new() else {
            return HapticOutcome::Unavailable;
        };
        let outcome = ios_pattern(self.core.engine_state(mtm));
        if outcome == HapticOutcome::Submitted && self.core.play(mtm, &ios_pattern_parts(pattern)) {
            return outcome;
        }
        match self.perform_with(fallback, request) {
            played if played.played() => HapticOutcome::FellBack,
            other => other,
        }
    }

    fn capabilities(&self, _source: Option<DeviceId>) -> HapticCapabilities {
        if supports_core_haptics() {
            HapticCapabilities::FULL
        } else {
            HapticCapabilities::FEEDBACK_ONLY
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
