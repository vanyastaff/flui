//! The Android API level, and the framework APIs above the floor the backend uses.
//!
//! The level is read in [`android_ndk`](crate::shared::android_ndk), where the NDK functions it
//! comes from are looked up; an NDK function above the floor is a gate of its own there.

use crate::shared::platform_floor::ANDROID_API;

use super::{Gate, OsVersion, Since};

/// The API level of the running system (`Build.VERSION.SDK_INT`); private (see the module
/// above).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AndroidApi(u32);

impl AndroidApi {
    /// A level the system would not report, which unlocks nothing.
    pub(crate) const UNKNOWN: Self = Self(0);

    /// The level the system reported.
    pub(crate) const fn from_level(level: u32) -> Self {
        Self(level)
    }

    /// The proof that this level has feature `F`, or `None`.
    pub(crate) fn since<F: Gate<Version = Self>>(self) -> Option<Since<F>> {
        super::since::<F>(self)
    }
}

impl OsVersion for AndroidApi {
    const FLOOR: u64 = ANDROID_API as u64;

    fn ordinal(self) -> Option<u64> {
        (self != Self::UNKNOWN).then_some(u64::from(self.0))
    }
}

/// Shown as `API N`, for logs and about screens; carries no order.
impl std::fmt::Display for AndroidApi {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if *self == Self::UNKNOWN {
            formatter.write_str("API unknown")
        } else {
            write!(formatter, "API {}", self.0)
        }
    }
}

/// `Context.getDisplay` (API 30). Without it the display comes from the window manager's
/// default display.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ContextDisplay;

impl Gate for ContextDisplay {
    type Version = AndroidApi;
    const SINCE: u64 = 30;
}

/// `Configuration.fontWeightAdjustment` (API 31). Without it the text-weight preference is
/// reported as unavailable.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TextWeight;

impl Gate for TextWeight {
    type Version = AndroidApi;
    const SINCE: u64 = 31;
}

/// `ViewConfiguration.getScaledHorizontalScrollFactor` and its vertical twin (API 26). Without
/// them the scroll factor is read from the theme's `listPreferredItemHeight`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ScaledScrollFactor;

impl Gate for ScaledScrollFactor {
    type Version = AndroidApi;
    const SINCE: u64 = 26;
}

/// `WindowInsets.Type` and `WindowInsets.getInsetsIgnoringVisibility` (API 30). Without them
/// the safe area is the stable insets, with the display cutout merged in where
/// [`DisplayCutout`] exists.
#[derive(Debug, Clone, Copy)]
pub(crate) struct InsetsByType;

impl Gate for InsetsByType {
    type Version = AndroidApi;
    const SINCE: u64 = 30;
}

/// `WindowInsets.getDisplayCutout` (API 28). Without it the safe area leaves a cutout out.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DisplayCutout;

impl Gate for DisplayCutout {
    type Version = AndroidApi;
    const SINCE: u64 = 28;
}

/// `Window.getInsetsController` (API 30). Without it the status-bar icons are set with
/// `View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct InsetsController;

impl Gate for InsetsController {
    type Version = AndroidApi;
    const SINCE: u64 = 30;
}

/// `View.startDragAndDrop` and `Activity.requestDragAndDropPermissions` (API 24). Without them
/// the window offers no drag source and every drag stays in its window.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DragSource;

impl Gate for DragSource {
    type Version = AndroidApi;
    const SINCE: u64 = 24;
}

/// `dalvik.system.InMemoryDexClassLoader` (API 26). Without it `FluiView`'s dex is loaded with
/// a `DexClassLoader` over a read-only copy in the code cache.
#[derive(Debug, Clone, Copy)]
pub(crate) struct InMemoryDex;

impl Gate for InMemoryDex {
    type Version = AndroidApi;
    const SINCE: u64 = 26;
}

/// `View.setSystemGestureExclusionRects` (API 29). Without it input regions report
/// `Unavailable` and none is sent to the decor view.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GestureExclusion;

impl Gate for GestureExclusion {
    type Version = AndroidApi;
    const SINCE: u64 = 29;
}

/// `VibrationEffect.Composition` with `PRIMITIVE_CLICK` and `PRIMITIVE_TICK`,
/// `Vibrator.areAllPrimitivesSupported`, and the `CONFIRM`, `REJECT` and `GESTURE_END` feedback
/// constants (API 30). Without them every haptic pattern falls back to the view's feedback, which
/// plays the nearest older constant.
#[derive(Debug, Clone, Copy)]
pub(crate) struct HapticComposition;

impl Gate for HapticComposition {
    type Version = AndroidApi;
    const SINCE: u64 = 30;
}

/// `PRIMITIVE_THUD`, `PRIMITIVE_LOW_TICK` and `Vibrator.getPrimitiveDurations` (API 31). Without
/// them a pattern that needs a round primitive falls back, and a composition's delays are the
/// pattern's gaps as they are.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RoundPrimitives;

impl Gate for RoundPrimitives {
    type Version = AndroidApi;
    const SINCE: u64 = 31;
}

/// `VibrationAttributes.createForUsage` (API 33). Without it a pattern vibration names its touch
/// usage with sonification `AudioAttributes`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct VibrationTouchUsage;

impl Gate for VibrationTouchUsage {
    type Version = AndroidApi;
    const SINCE: u64 = 33;
}

/// `InputMethodManager.startStylusHandwriting` (API 33). Without it a focused field does not
/// offer stylus handwriting and a pen press is only a pointer press.
#[derive(Debug, Clone, Copy)]
pub(crate) struct StylusHandwriting;

impl Gate for StylusHandwriting {
    type Version = AndroidApi;
    const SINCE: u64 = 33;
}

/// The touchpad gesture axes and classifications of `MotionEvent` (API 34). Without them the
/// window reports no precise scrolling and no trackpad gestures.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TouchpadGestures;

impl Gate for TouchpadGestures {
    type Version = AndroidApi;
    const SINCE: u64 = 34;
}

/// The `DRAG_START`, `SEGMENT_TICK`, `SEGMENT_FREQUENT_TICK` and `GESTURE_THRESHOLD_ACTIVATE`
/// feedback constants (API 34). Without them the view plays the nearest older constant.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SegmentFeedback;

impl Gate for SegmentFeedback {
    type Version = AndroidApi;
    const SINCE: u64 = 34;
}

/// `VibrationEffect.BasicEnvelopeBuilder` and `Vibrator.areEnvelopeEffectsSupported` (API 36).
/// Without them a pattern plays as a composition or an amplitude waveform.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EnvelopeEffects;

impl Gate for EnvelopeEffects {
    type Version = AndroidApi;
    const SINCE: u64 = 36;
}
