//! The Android host's system preferences: what the Activity's configuration,
//! `ViewConfiguration` and the system settings report, read through JNI on the
//! owner thread.
//!
//! - `configuration`: the reads, and the source the loop keeps
//!   (`AndroidPreferences`);
//! - [`contrast`]: what the contrast level and the high-contrast-text setting
//!   mean together;
//! - [`text_weight`]: when the text-weight adjustment is read, and what its
//!   sentinel means.
//!
//! The two rule modules make no JNI call, so every host checks them.

#[cfg(target_os = "android")]
pub(crate) use configuration::AndroidPreferences;

#[cfg(target_os = "android")]
mod configuration {
    //! Activity-context observations through public Android configuration APIs.
    //!
    //! The host reads them again on `ConfigChanged`, on `Resume` and when
    //! `FluiView` reports a changed setting or contrast level, even without a user
    //! surface.

    use std::sync::{Arc, Weak};
    use std::time::Duration;

    use android_activity::AndroidApp;
    use flui_foundation::geometry::DevicePixelRatio;
    use flui_platform::{
        HourCycle, Locale, MotionPreference, NumberingSystem, RegionalFormats,
        TextWeightPreference, Weekday,
    };
    use jni::{
        JValue, jni_sig, jni_str,
        objects::{JObject, JString},
    };

    use super::text_weight;
    use crate::error::PlatformError;
    use crate::owner::android::ActivityPreferences;
    use crate::owner::android::activity::with_activity;
    use crate::owner::signal::OwnerSignal;
    use crate::owner::sources::SystemPreferenceSource;
    use crate::preferences::{locales, read::ReadAdmission, regional};
    use crate::version::gate::android::{AndroidApi, LocaleList, SystemContrast, TextWeight};
    use flui_platform::{
        GestureGeometry, GesturePreferences, NativeTouchGeometry, PreferenceQueryError,
        SystemPreferences,
    };

    /// The Android host's preference source: the Activity's configuration and
    /// settings, read under one admission so a read that failed is retried at a
    /// bounded pace and a change is delivered once.
    pub(crate) struct AndroidPreferences {
        app: AndroidApp,
        api: AndroidApi,
        admission: ReadAdmission,
        signal: Weak<OwnerSignal>,
    }

    impl std::fmt::Debug for AndroidPreferences {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("AndroidPreferences").finish_non_exhaustive()
        }
    }

    impl AndroidPreferences {
        /// A source reading `app`'s Activity at `api`, which wakes the owner behind
        /// `signal` when a read owes the application a delivery.
        pub(crate) fn new(app: &AndroidApp, api: AndroidApi, signal: &Arc<OwnerSignal>) -> Self {
            Self {
                app: app.clone(),
                api,
                admission: ReadAdmission::default(),
                signal: Arc::downgrade(signal),
            }
        }
    }

    /// The loop calls it on the owner thread, which it checked.
    impl SystemPreferenceSource for AndroidPreferences {
        fn sample(&self) -> Result<SystemPreferences, PlatformError> {
            let (current, needs_delivery) =
                self.admission
                    .read_observation(web_time::Instant::now(), || {
                        sample(&self.app, self.api).map_err(|error| PlatformError::Preferences {
                            message: error.to_string(),
                        })
                    })?;
            if needs_delivery && let Some(signal) = self.signal.upgrade() {
                let _ = signal.wake();
            }
            Ok(current)
        }
    }

    impl ActivityPreferences for AndroidPreferences {
        fn retry_deadline(&self) -> Option<web_time::Instant> {
            self.admission.retry_deadline()
        }

        fn night_mode(&self) -> Result<bool, PreferenceQueryError> {
            night_mode(&self.app)
        }

        fn pixel_ratio(&self) -> Result<DevicePixelRatio, PreferenceQueryError> {
            pixel_ratio(&self.app)
        }

        fn gesture_geometry(&self) -> Result<GestureGeometry, PreferenceQueryError> {
            geometry(&self.app)
        }
    }

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
        regional: Option<RegionalFormats>,
    }

    enum ReadScope {
        Geometry,
        Preferences(AndroidApi),
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
                    text_weight::observe(api.since::<TextWeight>(), || {
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
            let regional = match scope {
                ReadScope::Geometry => None,
                ReadScope::Preferences(_) => Some(regional_formats(env, activity)?),
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
                regional,
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
                regional,
            )| {
                let ratio = DevicePixelRatio::try_new(density)
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
                    locales: tags.and_then(|tags| locales::from_language_tags(tags.split(','))),
                    high_contrast: high_contrast.flatten(),
                    regional,
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
        Ok(super::contrast::high_contrast(level, high_text))
    }

    /// The user's regional formats (ADR-0278 §3): the first weekday and the
    /// numbering system Android 14's regional preferences set as the default
    /// locale's `fw` and `nu` keywords, the calendar's first weekday below
    /// that, and the 24-hour setting, which `DateFormat` answers with the
    /// locale's own clock while the user has not chosen.
    fn regional_formats(
        env: &mut jni::Env<'_>,
        activity: &JObject<'_>,
    ) -> jni::errors::Result<RegionalFormats> {
        let mut formats = RegionalFormats::default();
        let locale = env
            .call_static_method(
                jni_str!("java/util/Locale"),
                jni_str!("getDefault"),
                jni_sig!("()Ljava/util/Locale;"),
                &[],
            )?
            .l()?;
        let keyword = unicode_keyword(env, &locale, "fw")?
            .as_deref()
            .and_then(Weekday::from_unicode_value);
        let first_weekday = if keyword.is_some() {
            keyword
        } else {
            let calendar = env
                .call_static_method(
                    jni_str!("java/util/Calendar"),
                    jni_str!("getInstance"),
                    jni_sig!("()Ljava/util/Calendar;"),
                    &[],
                )?
                .l()?;
            let day = env
                .call_method(
                    &calendar,
                    jni_str!("getFirstDayOfWeek"),
                    jni_sig!("()I"),
                    &[],
                )?
                .i()?;
            usize::try_from(day)
                .ok()
                .and_then(regional::sunday_first_weekday)
        };
        if let Some(day) = first_weekday {
            formats = formats.with_first_weekday(day);
        }
        let twenty_four = env
            .call_static_method(
                jni_str!("android/text/format/DateFormat"),
                jni_str!("is24HourFormat"),
                jni_sig!("(Landroid/content/Context;)Z"),
                &[JValue::Object(activity)],
            )?
            .z()?;
        formats = formats.with_hour_cycle(if twenty_four {
            HourCycle::H23
        } else {
            HourCycle::H12
        });
        if let Some(numbering) = unicode_keyword(env, &locale, "nu")?
            .and_then(|value| NumberingSystem::try_new(&value).ok())
        {
            formats = formats.with_numbering(numbering);
        }
        Ok(formats)
    }

    /// The value `locale` carries for the Unicode extension key `key`, if any.
    fn unicode_keyword(
        env: &mut jni::Env<'_>,
        locale: &JObject<'_>,
        key: &str,
    ) -> jni::errors::Result<Option<String>> {
        let key = env.new_string(key)?;
        let value = env
            .call_method(
                locale,
                jni_str!("getUnicodeLocaleType"),
                jni_sig!("(Ljava/lang/String;)Ljava/lang/String;"),
                &[JValue::Object(&key)],
            )?
            .l()?;
        let value = env.cast_local::<JString<'_>>(value)?;
        if value.is_null() {
            return Ok(None);
        }
        value.try_to_string(env).map(Some)
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

    pub(crate) fn pixel_ratio(app: &AndroidApp) -> Result<DevicePixelRatio, PreferenceQueryError> {
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
        DevicePixelRatio::try_new(density)
            .ok_or_else(|| flui_platform::InvalidPreference::PixelRatio.into())
    }

    /// Whether the Activity's current `Configuration.uiMode` is night. The native
    /// `AConfiguration` android-activity refreshes on `ConfigChanged` comes from
    /// the asset manager the Activity started with, which still reports the old
    /// night mode after a change handled in place (a Galaxy S21 on Android 15).
    pub(crate) fn night_mode(app: &AndroidApp) -> Result<bool, PreferenceQueryError> {
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

    pub(crate) fn geometry(app: &AndroidApp) -> Result<GestureGeometry, PreferenceQueryError> {
        let reading = read(app, ReadScope::Geometry)?;
        GestureGeometry::from_native_touch(&native_geometry(&reading)?).map_err(Into::into)
    }

    fn native_geometry(reading: &Reading) -> Result<NativeTouchGeometry, PreferenceQueryError> {
        NativeTouchGeometry::try_new(
            reading.touch_slop,
            reading.double_tap_slop,
            reading.min_fling,
            reading.max_fling,
            reading.ratio,
        )
        .map_err(Into::into)
    }

    fn sample(
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
        if let Some(regional) = reading.regional {
            preferences = preferences.with_regional(regional);
        }
        Ok(match reading.text_weight {
            Some(weight) => preferences.with_text_weight(weight),
            None => preferences,
        })
    }
}

#[cfg(any(target_os = "android", test))]
#[forbid(unsafe_code)]
pub(crate) mod contrast {
    //! What Android's two contrast observations mean together.

    /// High contrast from the system contrast level (`UiModeManager.getContrast`,
    /// API 34: `-1` to `1`, above zero asks for more) and the high-contrast-text
    /// setting. Either asking for more is high contrast; neither readable is no
    /// observation.
    pub(crate) fn high_contrast(level: Option<f32>, high_text: Option<bool>) -> Option<bool> {
        match (level, high_text) {
            (None, None) => None,
            (level, high_text) => {
                Some(level.is_some_and(|level| level > 0.0) || high_text == Some(true))
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::high_contrast;

        #[test]
        fn android_contrast_contract() {
            for (level, high_text, expected) in [
                (None, None, None),
                (Some(0.0), None, Some(false)),
                (Some(-1.0), None, Some(false)),
                (Some(0.5), None, Some(true)),
                (Some(1.0), Some(false), Some(true)),
                (None, Some(true), Some(true)),
                (None, Some(false), Some(false)),
                (Some(0.0), Some(true), Some(true)),
                (Some(f32::NAN), Some(false), Some(false)),
            ] {
                assert_eq!(
                    high_contrast(level, high_text),
                    expected,
                    "level {level:?}, high text {high_text:?}"
                );
            }
        }
    }
}

#[cfg(any(target_os = "android", test))]
#[forbid(unsafe_code)]
pub(crate) mod text_weight {
    //! API admission and absence/error handling for Android's native text-weight query.

    use flui_platform::TextWeightPreference;

    use crate::version::gate::{Since, android::TextWeight};

    /// The native field is absent without [`TextWeight`], and is then never queried. A failed
    /// available query remains an error; Android's undefined sentinel remains an unavailable
    /// observation.
    pub(crate) fn observe<E>(
        field: Option<Since<TextWeight>>,
        query: impl FnOnce() -> Result<i32, E>,
    ) -> Result<Option<TextWeightPreference>, E> {
        if field.is_none() {
            return Ok(None);
        }
        let value = query()?;
        Ok((value != i32::MAX).then(|| TextWeightPreference::from_adjustment(value)))
    }

    #[cfg(test)]
    mod tests {
        use std::{cell::Cell, num::NonZeroI32};

        use super::{TextWeightPreference, observe};
        use crate::version::gate::android::{AndroidApi, TextWeight};

        fn field(sdk: u32) -> Option<crate::version::gate::Since<TextWeight>> {
            AndroidApi::from_level(sdk).since::<TextWeight>()
        }

        #[test]
        fn android_text_weight_query_contract() {
            let adjustment = |value| {
                Some(TextWeightPreference::Adjustment(
                    NonZeroI32::new(value).expect("nonzero fixture"),
                ))
            };
            for (sdk, raw, expected) in [
                (30, 237, None),
                (31, i32::MAX, None),
                (31, 0, Some(TextWeightPreference::NoPreference)),
                (31, 237, adjustment(237)),
                (31, -137, adjustment(-137)),
                (35, i32::MIN, adjustment(i32::MIN)),
                (35, i32::MAX - 1, adjustment(i32::MAX - 1)),
            ] {
                let calls = Cell::new(0);
                let result = observe(field(sdk), || {
                    calls.set(calls.get() + 1);
                    Ok::<_, &str>(raw)
                });
                assert_eq!(result, Ok(expected), "SDK {sdk}, native value {raw}");
                assert_eq!(
                    calls.get(),
                    usize::from(sdk >= 31),
                    "native query admission"
                );
            }

            for sdk in [30, 31] {
                let calls = Cell::new(0);
                let failed = observe(field(sdk), || {
                    calls.set(calls.get() + 1);
                    Err::<i32, _>("native field failure")
                });
                let recovered = observe(field(sdk), || {
                    calls.set(calls.get() + 1);
                    Ok::<_, &str>(237)
                });
                if sdk == 30 {
                    assert_eq!(failed, Ok(None));
                    assert_eq!(recovered, Ok(None));
                    assert_eq!(calls.get(), 0, "unsupported field must never be queried");
                } else {
                    assert_eq!(failed, Err("native field failure"));
                    assert_eq!(recovered, Ok(adjustment(237)));
                    assert_eq!(calls.get(), 2, "failure cannot fabricate a preference");
                }
            }
        }
    }
}
