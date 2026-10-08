//! Activity-context observations through public Android ViewConfiguration APIs.
//!
//! The host samples every 500ms while its owner loop runs, plus ConfigChanged,
//! even without a user surface. This is bounded polling, not an OS notification.

use std::time::Duration;

use android_activity::AndroidApp;
use flui_foundation::geometry::DevicePixelRatio;
use jni::{JValue, JavaVM, jni_sig, jni_str, objects::JObject, refs::Global};

use crate::{
    GestureGeometry, GesturePreferences, NativeTouchGeometry, PreferenceQueryError,
    SystemPreferences,
};

struct Reading {
    ratio: DevicePixelRatio,
    touch_slop: i32,
    double_tap_slop: i32,
    min_fling: i32,
    max_fling: i32,
    double_tap: i32,
    long_press: i32,
}

#[expect(
    unsafe_code,
    reason = "AndroidApp lends live VM and Activity references to JNI"
)]
fn with_activity<T>(
    app: &AndroidApp,
    query: impl FnOnce(&mut jni::Env<'_>, &JObject<'_>) -> jni::errors::Result<T>,
) -> Result<T, PreferenceQueryError> {
    // SAFETY: app owns the running VM, and JavaVM is a borrowed handle.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    vm.attach_current_thread(|env| -> jni::errors::Result<_> {
        let result = (|| {
            let raw_activity = app.activity_as_ptr() as jni::sys::jobject;
            // SAFETY: AndroidApp owns this global Activity reference for the
            // entire borrow of app. as_cast_raw borrows it without deleting it.
            let activity = unsafe { env.as_cast_raw::<Global<JObject<'_>>>(&raw_activity)? };
            query(env, activity.as_ref().as_ref())
        })();
        if result.is_err() {
            let _ = env.exception_clear();
        }
        result
    })
    .map_err(|error| PreferenceQueryError::Native {
        message: error.to_string(),
    })
}

fn read(app: &AndroidApp) -> Result<Reading, PreferenceQueryError> {
    with_activity(app, |env, activity| {
        let resources = env
            .call_method(
                activity,
                jni_str!("getResources"),
                jni_sig!("()Landroid/content/res/Resources;"),
                &[],
            )?
            .l()?;
        let metrics = env
            .call_method(
                &resources,
                jni_str!("getDisplayMetrics"),
                jni_sig!("()Landroid/util/DisplayMetrics;"),
                &[],
            )?
            .l()?;
        let density = f64::from(
            env.get_field(&metrics, jni_str!("density"), jni_sig!("F"))?
                .f()?,
        );
        let config = env
            .call_static_method(
                jni_str!("android/view/ViewConfiguration"),
                jni_str!("get"),
                jni_sig!("(Landroid/content/Context;)Landroid/view/ViewConfiguration;"),
                &[JValue::Object(activity)],
            )?
            .l()?;
        let touch_slop = env
            .call_method(
                &config,
                jni_str!("getScaledTouchSlop"),
                jni_sig!("()I"),
                &[],
            )?
            .i()?;
        let double_tap_slop = env
            .call_method(
                &config,
                jni_str!("getScaledDoubleTapSlop"),
                jni_sig!("()I"),
                &[],
            )?
            .i()?;
        let min_fling = env
            .call_method(
                &config,
                jni_str!("getScaledMinimumFlingVelocity"),
                jni_sig!("()I"),
                &[],
            )?
            .i()?;
        let max_fling = env
            .call_method(
                &config,
                jni_str!("getScaledMaximumFlingVelocity"),
                jni_sig!("()I"),
                &[],
            )?
            .i()?;
        let double_tap = env
            .call_static_method(
                jni_str!("android/view/ViewConfiguration"),
                jni_str!("getDoubleTapTimeout"),
                jni_sig!("()I"),
                &[],
            )?
            .i()?;
        let long_press = env
            .call_static_method(
                jni_str!("android/view/ViewConfiguration"),
                jni_str!("getLongPressTimeout"),
                jni_sig!("()I"),
                &[],
            )?
            .i()?;
        Ok((
            density,
            touch_slop,
            double_tap_slop,
            min_fling,
            max_fling,
            double_tap,
            long_press,
        ))
    })
    .and_then(
        |(density, touch_slop, double_tap_slop, min_fling, max_fling, double_tap, long_press)| {
            let ratio =
                DevicePixelRatio::new(density).ok_or(crate::InvalidPreference::PixelRatio)?;
            Ok(Reading {
                ratio,
                touch_slop,
                double_tap_slop,
                min_fling,
                max_fling,
                double_tap,
                long_press,
            })
        },
    )
}

pub(super) fn pixel_ratio(app: &AndroidApp) -> Result<DevicePixelRatio, PreferenceQueryError> {
    let density = with_activity(app, |env, activity| {
        let resources = env
            .call_method(
                activity,
                jni_str!("getResources"),
                jni_sig!("()Landroid/content/res/Resources;"),
                &[],
            )?
            .l()?;
        let metrics = env
            .call_method(
                &resources,
                jni_str!("getDisplayMetrics"),
                jni_sig!("()Landroid/util/DisplayMetrics;"),
                &[],
            )?
            .l()?;
        env.get_field(&metrics, jni_str!("density"), jni_sig!("F"))?
            .f()
            .map(f64::from)
    })?;
    DevicePixelRatio::new(density).ok_or_else(|| crate::InvalidPreference::PixelRatio.into())
}

/// Public API26 factors, or the public theme attribute on API21–25, projected
/// through the same Activity resource metrics that produced the measurement.
pub(super) fn scroll_factors(
    app: &AndroidApp,
) -> Result<crate::shared::android_scroll::PixelFactors, PreferenceQueryError> {
    use crate::shared::android_scroll::{FactorApi, PixelFactors};
    let factors = with_activity(app, |env, activity| {
        let resources = env
            .call_method(
                activity,
                jni_str!("getResources"),
                jni_sig!("()Landroid/content/res/Resources;"),
                &[],
            )?
            .l()?;
        let metrics = env
            .call_method(
                &resources,
                jni_str!("getDisplayMetrics"),
                jni_sig!("()Landroid/util/DisplayMetrics;"),
                &[],
            )?
            .l()?;
        let density = env
            .get_field(&metrics, jni_str!("density"), jni_sig!("F"))?
            .f()?;
        let sdk = env
            .get_static_field(
                jni_str!("android/os/Build$VERSION"),
                jni_str!("SDK_INT"),
                jni_sig!("I"),
            )?
            .i()?;
        if FactorApi::for_sdk(sdk) == FactorApi::ThemeAttribute {
            let value =
                env.new_object(jni_str!("android/util/TypedValue"), jni_sig!("()V"), &[])?;
            let theme = env
                .call_method(
                    activity,
                    jni_str!("getTheme"),
                    jni_sig!("()Landroid/content/res/Resources$Theme;"),
                    &[],
                )?
                .l()?;
            let attribute = env
                .get_static_field(
                    jni_str!("android/R$attr"),
                    jni_str!("listPreferredItemHeight"),
                    jni_sig!("I"),
                )?
                .i()?;
            let resolved = env
                .call_method(
                    &theme,
                    jni_str!("resolveAttribute"),
                    jni_sig!("(ILandroid/util/TypedValue;Z)Z"),
                    &[
                        JValue::Int(attribute),
                        JValue::Object(&value),
                        JValue::Bool(true),
                    ],
                )?
                .z()?;
            if !resolved {
                return Ok(None);
            }
            let factor = env
                .call_method(
                    &value,
                    jni_str!("getDimension"),
                    jni_sig!("(Landroid/util/DisplayMetrics;)F"),
                    &[JValue::Object(&metrics)],
                )?
                .f()?;
            return Ok(Some((
                f64::from(factor),
                f64::from(factor),
                f64::from(density),
            )));
        }
        let config = env
            .call_static_method(
                jni_str!("android/view/ViewConfiguration"),
                jni_str!("get"),
                jni_sig!("(Landroid/content/Context;)Landroid/view/ViewConfiguration;"),
                &[JValue::Object(activity)],
            )?
            .l()?;
        let x = env
            .call_method(
                &config,
                jni_str!("getScaledHorizontalScrollFactor"),
                jni_sig!("()F"),
                &[],
            )?
            .f()?;
        let y = env
            .call_method(
                &config,
                jni_str!("getScaledVerticalScrollFactor"),
                jni_sig!("()F"),
                &[],
            )?
            .f()?;
        Ok(Some((f64::from(x), f64::from(y), f64::from(density))))
    })?;
    let (horizontal, vertical, density) = factors.ok_or_else(|| PreferenceQueryError::Native {
        message: "Activity theme has no scroll factor observation".into(),
    })?;
    let ratio = DevicePixelRatio::new(density).ok_or(crate::InvalidPreference::PixelRatio)?;
    PixelFactors::from_physical(horizontal, vertical, ratio).ok_or_else(|| {
        PreferenceQueryError::Native {
            message: "Android returned invalid scroll factors".into(),
        }
    })
}

pub(super) fn geometry(app: &AndroidApp) -> Result<GestureGeometry, PreferenceQueryError> {
    let reading = read(app)?;
    GestureGeometry::from_native_touch(&native_geometry(&reading)?).map_err(Into::into)
}

fn native_geometry(reading: &Reading) -> Result<NativeTouchGeometry, PreferenceQueryError> {
    NativeTouchGeometry::new(
        reading.touch_slop,
        reading.double_tap_slop,
        reading.min_fling,
        reading.max_fling,
        reading.ratio,
    )
    .map_err(Into::into)
}

pub(super) fn sample(app: &AndroidApp) -> Result<SystemPreferences, PreferenceQueryError> {
    let reading = read(app)?;
    let duration = |value: i32| {
        u64::try_from(value)
            .map(Duration::from_millis)
            .map_err(|_| PreferenceQueryError::Native {
                message: "Android returned a negative gesture timeout".into(),
            })
    };
    let gestures = GesturePreferences::default()
        .with_double_tap_interval(duration(reading.double_tap)?)
        .with_long_press_timeout(duration(reading.long_press)?)
        .with_native_touch_geometry(native_geometry(&reading)?);
    Ok(SystemPreferences::default().with_gestures(gestures))
}
