//! Activity-context observations through public Android configuration APIs.
//!
//! The host reads them again on `ConfigChanged`, on `Resume` and when
//! `FluiView` reports a changed setting or contrast level, even without a user
//! surface.

use std::time::Duration;

use android_activity::AndroidApp;
use flui_foundation::geometry::DevicePixelRatio;
use flui_platform::{Locale, MotionPreference, TextWeightPreference};
use jni::{
    JValue, JavaVM, jni_sig, jni_str,
    objects::{JObject, JString},
    refs::Global,
};

use crate::shared::os_version::android::{AndroidApi, LocaleList, SystemContrast, TextWeight};
use crate::shared::{android_contrast, preferred_locales};
use flui_platform::{
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
    text_weight: Option<TextWeightPreference>,
    motion: Option<MotionPreference>,
    haptics: Option<bool>,
    locales: Option<Vec<Locale>>,
    high_contrast: Option<bool>,
}

enum ReadScope {
    Geometry,
    Preferences(AndroidApi),
}

#[expect(
    unsafe_code,
    reason = "AndroidApp lends live VM and Activity references to JNI"
)]
pub(crate) fn with_activity<T>(
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
            query(env, activity.as_ref())
        })();
        if result.is_err() {
            env.exception_clear();
        }
        result
    })
    .map_err(|error| PreferenceQueryError::Native {
        message: error.to_string(),
    })
}

fn read(app: &AndroidApp, scope: ReadScope) -> Result<Reading, PreferenceQueryError> {
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
        let long_press = match scope {
            ReadScope::Geometry => long_press,
            ReadScope::Preferences(_) => long_press_timeout(env, activity, long_press)?,
        };
        let text_weight = match scope {
            ReadScope::Geometry => None,
            ReadScope::Preferences(api) => {
                crate::shared::android_text_weight::observe(api.since::<TextWeight>(), || {
                    let configuration = env
                        .call_method(
                            &resources,
                            jni_str!("getConfiguration"),
                            jni_sig!("()Landroid/content/res/Configuration;"),
                            &[],
                        )?
                        .l()?;
                    env.get_field(
                        &configuration,
                        jni_str!("fontWeightAdjustment"),
                        jni_sig!("I"),
                    )?
                    .i()
                })?
            }
        };
        let duration_scale_and_haptics = match scope {
            ReadScope::Geometry => None,
            ReadScope::Preferences(_) => {
                let resolver = env
                    .call_method(
                        activity,
                        jni_str!("getContentResolver"),
                        jni_sig!("()Landroid/content/ContentResolver;"),
                        &[],
                    )?
                    .l()?;
                // `Settings.System.HAPTIC_FEEDBACK_ENABLED`: the user's
                // "touch feedback" switch, 1 when on. Absent reads as on,
                // the platform default.
                let haptics_setting = env
                    .get_static_field(
                        jni_str!("android/provider/Settings$System"),
                        jni_str!("HAPTIC_FEEDBACK_ENABLED"),
                        jni_sig!("Ljava/lang/String;"),
                    )?
                    .l()?;
                let haptics = env
                    .call_static_method(
                        jni_str!("android/provider/Settings$System"),
                        jni_str!("getInt"),
                        jni_sig!("(Landroid/content/ContentResolver;Ljava/lang/String;I)I"),
                        &[
                            JValue::Object(&resolver),
                            JValue::Object(&haptics_setting),
                            JValue::Int(1),
                        ],
                    )?
                    .i()?
                    != 0;
                let setting = env
                    .get_static_field(
                        jni_str!("android/provider/Settings$Global"),
                        jni_str!("ANIMATOR_DURATION_SCALE"),
                        jni_sig!("Ljava/lang/String;"),
                    )?
                    .l()?;
                let duration_scale = f64::from(
                    env.call_static_method(
                        jni_str!("android/provider/Settings$Global"),
                        jni_str!("getFloat"),
                        jni_sig!("(Landroid/content/ContentResolver;Ljava/lang/String;F)F"),
                        &[
                            JValue::Object(&resolver),
                            JValue::Object(&setting),
                            JValue::Float(1.0),
                        ],
                    )?
                    .f()?,
                );
                Some((duration_scale, haptics))
            }
        };
        let languages_and_contrast = match scope {
            ReadScope::Geometry => None,
            ReadScope::Preferences(api) => Some((
                language_tags(env, &resources, api)?,
                contrast(env, activity, api)?,
            )),
        };
        Ok((
            density,
            touch_slop,
            double_tap_slop,
            min_fling,
            max_fling,
            double_tap,
            long_press,
            text_weight,
            duration_scale_and_haptics,
            languages_and_contrast,
        ))
    })
    .and_then(
        |(
            density,
            touch_slop,
            double_tap_slop,
            min_fling,
            max_fling,
            double_tap,
            long_press,
            text_weight,
            duration_scale_and_haptics,
            languages_and_contrast,
        )| {
            let ratio = DevicePixelRatio::new(density)
                .ok_or(flui_platform::InvalidPreference::PixelRatio)?;
            let (duration_scale, haptics) = duration_scale_and_haptics.unzip();
            let (tags, high_contrast) = languages_and_contrast.unzip();
            Ok(Reading {
                ratio,
                touch_slop,
                double_tap_slop,
                min_fling,
                max_fling,
                double_tap,
                long_press,
                text_weight,
                motion: duration_scale
                    .map(MotionPreference::from_duration_scale)
                    .transpose()?,
                haptics,
                locales: tags
                    .and_then(|tags| preferred_locales::from_language_tags(tags.split(','))),
                high_contrast: high_contrast.flatten(),
            })
        },
    )
}

/// The configuration's preferred languages as comma-separated BCP 47 tags:
/// its `LocaleList` from API 24, its one `locale` below.
fn language_tags(
    env: &mut jni::Env<'_>,
    resources: &JObject<'_>,
    api: AndroidApi,
) -> jni::errors::Result<String> {
    let configuration = env
        .call_method(
            resources,
            jni_str!("getConfiguration"),
            jni_sig!("()Landroid/content/res/Configuration;"),
            &[],
        )?
        .l()?;
    let tags = if api.since::<LocaleList>().is_some() {
        let locales = env
            .call_method(
                &configuration,
                jni_str!("getLocales"),
                jni_sig!("()Landroid/os/LocaleList;"),
                &[],
            )?
            .l()?;
        env.call_method(
            &locales,
            jni_str!("toLanguageTags"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?
        .l()?
    } else {
        let locale = env
            .get_field(
                &configuration,
                jni_str!("locale"),
                jni_sig!("Ljava/util/Locale;"),
            )?
            .l()?;
        if locale.is_null() {
            return Ok(String::new());
        }
        env.call_method(
            &locale,
            jni_str!("toLanguageTag"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?
        .l()?
    };
    let tags = env.cast_local::<JString<'_>>(tags)?;
    if tags.is_null() {
        return Ok(String::new());
    }
    tags.try_to_string(env)
}

/// High contrast from the system contrast level (API 34) and the
/// high-contrast-text setting. A setting this app may not read is no
/// observation of it rather than a failed read.
fn contrast(
    env: &mut jni::Env<'_>,
    activity: &JObject<'_>,
    api: AndroidApi,
) -> jni::errors::Result<Option<bool>> {
    let resolver = env
        .call_method(
            activity,
            jni_str!("getContentResolver"),
            jni_sig!("()Landroid/content/ContentResolver;"),
            &[],
        )?
        .l()?;
    let high_text =
        secure_int(env, &resolver, "high_text_contrast_enabled", 0)?.map(|value| value != 0);
    let level = match api.since::<SystemContrast>() {
        Some(_) => {
            let service = env.new_string("uimode")?;
            let modes = env
                .call_method(
                    activity,
                    jni_str!("getSystemService"),
                    jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                    &[JValue::Object(&service)],
                )?
                .l()?;
            if modes.is_null() {
                None
            } else {
                Some(
                    env.call_method(&modes, jni_str!("getContrast"), jni_sig!("()F"), &[])?
                        .f()?,
                )
            }
        }
        None => None,
    };
    Ok(android_contrast::high_contrast(level, high_text))
}

/// The long-press timeout as the secure setting holds it, `fallback` while it
/// is unset or withheld. `ViewConfiguration` reads a copy the system pushes to
/// the app separately, which can still be old when the setting's observer fires.
fn long_press_timeout(
    env: &mut jni::Env<'_>,
    activity: &JObject<'_>,
    fallback: i32,
) -> jni::errors::Result<i32> {
    let resolver = env
        .call_method(
            activity,
            jni_str!("getContentResolver"),
            jni_sig!("()Landroid/content/ContentResolver;"),
            &[],
        )?
        .l()?;
    Ok(secure_int(env, &resolver, "long_press_timeout", fallback)?.unwrap_or(fallback))
}

/// The secure setting `key`, `default` while unset. A key Android withholds
/// from apps throws `SecurityException`, which is no observation of it rather
/// than a failed read.
fn secure_int(
    env: &mut jni::Env<'_>,
    resolver: &JObject<'_>,
    key: &str,
    default: i32,
) -> jni::errors::Result<Option<i32>> {
    let key = env.new_string(key)?;
    match env.call_static_method(
        jni_str!("android/provider/Settings$Secure"),
        jni_str!("getInt"),
        jni_sig!("(Landroid/content/ContentResolver;Ljava/lang/String;I)I"),
        &[
            JValue::Object(resolver),
            JValue::Object(&key),
            JValue::Int(default),
        ],
    ) {
        Ok(value) => value.i().map(Some),
        Err(jni::errors::Error::JavaException) => {
            env.exception_clear();
            Ok(None)
        }
        Err(error) => Err(error),
    }
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
    DevicePixelRatio::new(density)
        .ok_or_else(|| flui_platform::InvalidPreference::PixelRatio.into())
}

/// Whether the Activity's current `Configuration.uiMode` is night. The native
/// `AConfiguration` android-activity refreshes on `ConfigChanged` comes from
/// the asset manager the Activity started with, which still reports the old
/// night mode after a change handled in place (a Galaxy S21 on Android 15).
pub(super) fn night_mode(app: &AndroidApp) -> Result<bool, PreferenceQueryError> {
    // `Configuration.UI_MODE_NIGHT_MASK` and `Configuration.UI_MODE_NIGHT_YES`.
    const NIGHT_MASK: i32 = 0x30;
    const NIGHT_YES: i32 = 0x20;
    let ui_mode = with_activity(app, |env, activity| {
        let resources = env
            .call_method(
                activity,
                jni_str!("getResources"),
                jni_sig!("()Landroid/content/res/Resources;"),
                &[],
            )?
            .l()?;
        let configuration = env
            .call_method(
                &resources,
                jni_str!("getConfiguration"),
                jni_sig!("()Landroid/content/res/Configuration;"),
                &[],
            )?
            .l()?;
        env.get_field(&configuration, jni_str!("uiMode"), jni_sig!("I"))?
            .i()
    })?;
    Ok(ui_mode & NIGHT_MASK == NIGHT_YES)
}

/// Public API26 factors, or the public theme attribute on API21–25, projected
/// through the same Activity resource metrics that produced the measurement.
pub(crate) fn scroll_factors(
    app: &AndroidApp,
    api: AndroidApi,
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
        if FactorApi::for_api(api) == FactorApi::ThemeAttribute {
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
    let ratio =
        DevicePixelRatio::new(density).ok_or(flui_platform::InvalidPreference::PixelRatio)?;
    PixelFactors::from_physical(horizontal, vertical, ratio).ok_or_else(|| {
        PreferenceQueryError::Native {
            message: "Android returned invalid scroll factors".into(),
        }
    })
}

pub(super) fn geometry(app: &AndroidApp) -> Result<GestureGeometry, PreferenceQueryError> {
    let reading = read(app, ReadScope::Geometry)?;
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

pub(crate) fn sample(
    app: &AndroidApp,
    api: AndroidApi,
) -> Result<SystemPreferences, PreferenceQueryError> {
    let reading = read(app, ReadScope::Preferences(api))?;
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
    let mut preferences = SystemPreferences::default().with_gestures(gestures);
    if let Some(motion) = reading.motion {
        preferences = preferences.with_motion(motion);
    }
    if let Some(haptics) = reading.haptics {
        preferences = preferences.with_haptics(haptics);
    }
    if let Some(high_contrast) = reading.high_contrast {
        preferences = preferences.with_high_contrast(high_contrast);
    }
    if let Some(locales) = reading.locales {
        preferences = preferences.with_locales(locales);
    }
    Ok(match reading.text_weight {
        Some(weight) => preferences.with_text_weight(weight),
        None => preferences,
    })
}
