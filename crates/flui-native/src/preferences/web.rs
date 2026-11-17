//! The web host's system preferences: what the browser's media queries,
//! `navigator.languages`, `Intl` and the `AccentColor` system colour report,
//! owned by the host rather than its canvas.
//!
//! - `browser`: the queries and listeners, and the source the loop keeps
//!   (`WebPreferences`);
//! - [`media`]: which media features the host observes, and what their
//!   matches mean;
//! - [`system_color`]: the colour a computed style serializes a system colour
//!   to.
//!
//! The rule modules read no browser object, so every host checks them.

#[cfg(target_arch = "wasm32")]
pub(crate) use browser::WebPreferences;

#[cfg(target_arch = "wasm32")]
mod browser {
    //! Browser accessibility and language observations, owned by the host rather
    //! than its canvas.
    //!
    //! Media queries report motion and contrast, `navigator.languages` the
    //! preferred locales, and `Intl` the hour cycle and first weekday the
    //! browser formats with, and the computed `AccentColor` the accent. Each
    //! query's `change` and the window's `languagechange` only wake the
    //! owner, which then samples every observation again.

    use std::sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    };

    use flui_platform::{HourCycle, RegionalFormats, SrgbColor, SystemPreferences, Weekday};
    use js_sys::{Array, Intl, Object, Reflect};
    use wasm_bindgen::{JsCast, JsValue, closure::Closure};
    use web_sys::{Event, EventTarget, HtmlElement, MediaQueryList, Window};

    use super::media::{MediaFeature, MediaMatches};
    use super::system_color;
    use crate::{
        PlatformError, owner::signal::OwnerSignal, owner::sources::SystemPreferenceSource,
        preferences::locales as preferred_locales, preferences::regional,
    };

    const LANGUAGE_CHANGE: &str = "languagechange";
    const CHANGE: &str = "change";

    /// What a sample reads, cloned out of the host so no borrow of it is held
    /// while the browser answers.
    #[derive(Clone)]
    pub(super) struct Sampler {
        window: Window,
        queries: Vec<(MediaFeature, MediaQueryList)>,
        active: Arc<AtomicBool>,
    }

    impl Sampler {
        /// Every observation the browser reports now.
        ///
        /// # Errors
        /// Refuses once the source has closed, including during the sample.
        pub(super) fn sample(&self) -> Result<SystemPreferences, PlatformError> {
            self.ensure_active("browser preference source is closed")?;
            let matches = MediaMatches::from_fn(|feature| {
                self.queries
                    .iter()
                    .find(|(observed, _)| *observed == feature)
                    .is_some_and(|(_, query)| query.matches())
            });
            let navigator = self.window.navigator();
            let mut tags: Vec<String> = navigator
                .languages()
                .iter()
                .filter_map(|tag| tag.as_string())
                .collect();
            if tags.is_empty() {
                tags.extend(navigator.language());
            }
            self.ensure_active("browser preference source closed during sampling")?;
            let mut preferences = SystemPreferences::default();
            if let Some(motion) = matches.motion() {
                preferences = preferences.with_motion(motion);
            }
            if let Some(high_contrast) = matches.high_contrast() {
                preferences = preferences.with_high_contrast(high_contrast);
            }
            if let Some(locales) = preferred_locales::from_language_tags(tags) {
                preferences = preferences.with_locales(locales);
            }
            if let Some(accent) = accent_color(&self.window) {
                preferences = preferences.with_accent_color(accent);
            }
            Ok(preferences.with_regional(regional_formats()))
        }

        fn ensure_active(&self, message: &'static str) -> Result<(), PlatformError> {
            if self.active.load(Ordering::Acquire) {
                Ok(())
            } else {
                Err(PlatformError::Preferences {
                    message: message.into(),
                })
            }
        }
    }

    /// The colour the browser computes for the CSS system colour
    /// `AccentColor`, which it takes from the OS where it exposes one.
    ///
    /// A browser that does not know the keyword refuses the declaration, so
    /// the probe's own style stays empty: no observation, rather than the
    /// inherited colour a computed style would report. The probe is a hidden
    /// element that lives only for this read. The browser announces no change
    /// of the accent, so a change shows at the next sample another preference
    /// or the language starts.
    fn accent_color(window: &Window) -> Option<SrgbColor> {
        let document = window.document()?;
        let probe: HtmlElement = document.create_element("span").ok()?.dyn_into().ok()?;
        let style = probe.style();
        style.set_property("color", "AccentColor").ok()?;
        if style.get_property_value("color").ok()?.is_empty() {
            return None;
        }
        style.set_property("display", "none").ok()?;
        let parent = document.document_element()?;
        parent.append_child(&probe).ok()?;
        let computed = window
            .get_computed_style(&probe)
            .ok()
            .flatten()
            .and_then(|computed| computed.get_property_value("color").ok());
        // The probe leaves whether or not the browser answered.
        let _ = parent.remove_child(&probe);
        system_color::from_computed(&computed?)
    }

    /// The hour cycle of the browser's default time format and its default
    /// locale's first weekday (ADR-0278 §3). A browser without `Intl.Locale`
    /// week info reports no weekday.
    fn regional_formats() -> RegionalFormats {
        let mut formats = RegionalFormats::default();
        let options = Object::new();
        // Without an hour field the resolved options carry no hour cycle.
        if Reflect::set(&options, &"hour".into(), &"numeric".into()).is_err() {
            return formats;
        }
        let resolved = Intl::DateTimeFormat::new(&Array::new(), &options).resolved_options();
        let field = |name: &str| {
            Reflect::get(&resolved, &JsValue::from_str(name))
                .ok()
                .and_then(|value| value.as_string())
        };
        if let Some(cycle) = field("hourCycle")
            .as_deref()
            .and_then(HourCycle::from_unicode_value)
        {
            formats = formats.with_hour_cycle(cycle);
        }
        if let Some(day) = field("locale").and_then(|tag| first_day(&tag)) {
            formats = formats.with_first_weekday(day);
        }
        formats
    }

    fn first_day(tag: &str) -> Option<Weekday> {
        let locale = Intl::Locale::new(tag).ok()?;
        let info = match locale.get_week_info() {
            Ok(info) => info,
            // Engines that shipped week info before `getWeekInfo` expose it
            // as a getter.
            Err(_) => Reflect::get(&locale, &"weekInfo".into())
                .ok()
                .filter(JsValue::is_object)?
                .unchecked_into::<Intl::WeekInfo>(),
        };
        regional::iso_weekday(info.first_day())
    }

    pub(crate) struct WebPreferences {
        sampler: Sampler,
        callback: Option<Closure<dyn FnMut(Event)>>,
        /// Each target and event the callback is registered for, in order.
        registered: Vec<(EventTarget, &'static str)>,
    }

    impl WebPreferences {
        pub(crate) fn try_new(signal: &Arc<OwnerSignal>) -> Result<Self, PlatformError> {
            let window = web_sys::window().ok_or_else(|| PlatformError::Preferences {
                message: "browser preference observation requires a Window".into(),
            })?;
            let mut queries = Vec::with_capacity(MediaFeature::ALL.len());
            for feature in MediaFeature::ALL {
                // A browser without a query answers none; that feature then stays
                // unobserved instead of failing the others.
                if let Some(query) = window.match_media(feature.query()).map_err(|error| {
                    PlatformError::Preferences {
                        message: format!("{} failed: {error:?}", feature.query()),
                    }
                })? {
                    queries.push((feature, query));
                }
            }
            let active = Arc::new(AtomicBool::new(true));
            let callback_active = Arc::clone(&active);
            let signal: Weak<OwnerSignal> = Arc::downgrade(signal);
            let callback = Closure::new(move |_: Event| {
                if callback_active.load(Ordering::Acquire)
                    && let Some(signal) = signal.upgrade()
                {
                    let _ = signal.wake();
                }
            });
            // Own every registration as soon as it succeeds: if a later one fails,
            // Drop closes admission and removes the earlier ones.
            let mut preferences = Self {
                sampler: Sampler {
                    window,
                    queries,
                    active,
                },
                callback: Some(callback),
                registered: Vec::with_capacity(MediaFeature::ALL.len() + 1),
            };
            let targets: Vec<(EventTarget, &'static str)> = preferences
                .sampler
                .queries
                .iter()
                .map(|(_, query)| (EventTarget::from(query.clone()), CHANGE))
                .chain([(
                    EventTarget::from(preferences.sampler.window.clone()),
                    LANGUAGE_CHANGE,
                )])
                .collect();
            for (target, event) in targets {
                let callback = preferences
                    .callback
                    .as_ref()
                    .expect("BUG: the callback lives until Drop");
                target
                    .add_event_listener_with_callback(event, callback.as_ref().unchecked_ref())
                    .map_err(|error| PlatformError::Preferences {
                        message: format!("{event} observer registration failed: {error:?}"),
                    })?;
                preferences.registered.push((target, event));
            }
            Ok(preferences)
        }
    }

    impl SystemPreferenceSource for WebPreferences {
        fn sample(&self) -> Result<SystemPreferences, PlatformError> {
            self.sampler.sample()
        }
    }

    impl std::fmt::Debug for WebPreferences {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("WebPreferences").finish_non_exhaustive()
        }
    }

    impl Drop for WebPreferences {
        fn drop(&mut self) {
            self.sampler.active.store(false, Ordering::Release);
            let Some(callback) = self.callback.take() else {
                return;
            };
            let mut failed = false;
            for (target, event) in self.registered.drain(..) {
                if let Err(error) = target
                    .remove_event_listener_with_callback(event, callback.as_ref().unchecked_ref())
                {
                    failed = true;
                    crate::owner::panic_boundary::contain_owner_callback(|| {
                        tracing::warn!(?error, event, "browser preference observer removal failed");
                    });
                }
            }
            if failed {
                // Keep the JS trampoline valid for a registration that survived.
                // Its capture is inert and cannot deliver to a replacement host.
                callback.forget();
            }
        }
    }
}

#[cfg(any(target_arch = "wasm32", test))]
#[forbid(unsafe_code)]
pub(crate) mod media {
    //! The media features a browser host observes, and what their matches mean.
    //!
    //! Each feature needs one query per value the host distinguishes: a browser
    //! without the feature matches none of them, and that is no observation rather
    //! than "no preference".

    use flui_platform::MotionPreference;

    /// One media query the browser host keeps, and listens to for `change`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum MediaFeature {
        ReducedMotion,
        NoMotionPreference,
        MoreContrast,
        LessContrast,
        NoContrastPreference,
        ForcedColors,
        UnforcedColors,
    }

    impl MediaFeature {
        pub(crate) const ALL: [Self; 7] = [
            Self::ReducedMotion,
            Self::NoMotionPreference,
            Self::MoreContrast,
            Self::LessContrast,
            Self::NoContrastPreference,
            Self::ForcedColors,
            Self::UnforcedColors,
        ];

        pub(crate) const fn query(self) -> &'static str {
            match self {
                Self::ReducedMotion => "(prefers-reduced-motion: reduce)",
                Self::NoMotionPreference => "(prefers-reduced-motion: no-preference)",
                Self::MoreContrast => "(prefers-contrast: more)",
                Self::LessContrast => "(prefers-contrast: less)",
                Self::NoContrastPreference => "(prefers-contrast: no-preference)",
                Self::ForcedColors => "(forced-colors: active)",
                Self::UnforcedColors => "(forced-colors: none)",
            }
        }
    }

    /// Which of [`MediaFeature::ALL`] matched in one sample.
    #[derive(Clone, Copy, Debug, Default)]
    pub(crate) struct MediaMatches([bool; MediaFeature::ALL.len()]);

    impl MediaMatches {
        pub(crate) fn from_fn(matches: impl FnMut(MediaFeature) -> bool) -> Self {
            Self(MediaFeature::ALL.map(matches))
        }

        fn get(self, feature: MediaFeature) -> bool {
            self.0[feature as usize]
        }

        pub(crate) fn motion(self) -> Option<MotionPreference> {
            if self.get(MediaFeature::ReducedMotion) {
                Some(MotionPreference::Reduce)
            } else if self.get(MediaFeature::NoMotionPreference) {
                Some(MotionPreference::NoPreference)
            } else {
                None
            }
        }

        /// More contrast, or colors the user's own palette forces (Windows high
        /// contrast in a browser), is high contrast. Either feature known in
        /// another value is not; a browser that knows neither has no observation.
        pub(crate) fn high_contrast(self) -> Option<bool> {
            if self.get(MediaFeature::MoreContrast) || self.get(MediaFeature::ForcedColors) {
                Some(true)
            } else if self.get(MediaFeature::LessContrast)
                || self.get(MediaFeature::NoContrastPreference)
                || self.get(MediaFeature::UnforcedColors)
            {
                Some(false)
            } else {
                None
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn matching(features: &[MediaFeature]) -> MediaMatches {
            MediaMatches::from_fn(|feature| features.contains(&feature))
        }

        fn queries_follow_the_feature_order() {
            for (index, feature) in MediaFeature::ALL.into_iter().enumerate() {
                assert_eq!(feature as usize, index);
                assert!(
                    MediaFeature::ALL[..index]
                        .iter()
                        .all(|earlier| earlier.query() != feature.query())
                );
            }
        }

        fn unsupported_features_are_unobserved() {
            let none = matching(&[]);
            assert_eq!(none.motion(), None);
            assert_eq!(none.high_contrast(), None);
        }

        fn each_known_value_maps_once() {
            use MediaFeature::*;
            for (features, motion, contrast) in [
                (&[ReducedMotion][..], Some(MotionPreference::Reduce), None),
                (
                    &[NoMotionPreference][..],
                    Some(MotionPreference::NoPreference),
                    None,
                ),
                (&[MoreContrast][..], None, Some(true)),
                (&[ForcedColors, NoContrastPreference][..], None, Some(true)),
                (&[LessContrast][..], None, Some(false)),
                (
                    &[NoContrastPreference, UnforcedColors][..],
                    None,
                    Some(false),
                ),
                // `custom` contrast matches no `prefers-contrast` value here;
                // unforced colors still say it is not high contrast.
                (&[UnforcedColors][..], None, Some(false)),
            ] {
                let matches = matching(features);
                assert_eq!(matches.motion(), motion, "{features:?}");
                assert_eq!(matches.high_contrast(), contrast, "{features:?}");
            }
        }

        #[test]
        fn browser_media_contract() {
            crate::table_test::run_table(
                "browser_media_contract",
                &[
                    (
                        "queries_follow_the_feature_order",
                        queries_follow_the_feature_order as fn(),
                    ),
                    (
                        "unsupported_features_are_unobserved",
                        unsupported_features_are_unobserved,
                    ),
                    ("each_known_value_maps_once", each_known_value_maps_once),
                ],
            );
        }
    }
}

#[cfg(any(target_arch = "wasm32", test))]
#[forbid(unsafe_code)]
pub(crate) mod system_color {
    //! The colour a browser computes for a CSS system colour, as its style
    //! serializes it.

    use flui_platform::SrgbColor;

    /// The sRGB colour a computed `color` serializes to: CSSOM writes an sRGB
    /// colour as `rgb(r, g, b)` or, with an alpha below one,
    /// `rgba(r, g, b, a)`, each colour channel in `0..=255` and the alpha in
    /// `0..=1`; newer engines may write the channels with a fraction. Any
    /// other form (`color(display-p3 …)`, a keyword) or a channel out of range
    /// is no colour.
    pub(crate) fn from_computed(text: &str) -> Option<SrgbColor> {
        let text = text.trim();
        let (arguments, has_alpha) = if let Some(rest) = text.strip_prefix("rgba(") {
            (rest, true)
        } else {
            (text.strip_prefix("rgb(")?, false)
        };
        let mut parts = arguments.strip_suffix(')')?.split(',').map(str::trim);
        let mut channel = || -> Option<f64> {
            let value = parts.next()?.parse::<f64>().ok()?;
            (0.0..=255.0).contains(&value).then_some(value / 255.0)
        };
        let (red, green, blue) = (channel()?, channel()?, channel()?);
        let alpha = if has_alpha {
            parts.next()?.parse::<f64>().ok()?
        } else {
            1.0
        };
        if parts.next().is_some() {
            return None;
        }
        SrgbColor::try_new(red, green, blue, alpha).ok()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// The forms CSSOM serializes an sRGB colour in read back as that
        /// colour; any other form, a channel out of range and a malformed
        /// list are no colour. Red-check: accept `color(display-p3 …)` or
        /// clamp a channel.
        #[test]
        fn a_computed_colour_reads_back_only_in_srgb() {
            let opaque = SrgbColor::from_rgba8(0, 120, 215, 255);
            for text in [
                "rgb(0, 120, 215)",
                " rgb(0,120,215) ",
                "rgba(0, 120, 215, 1)",
            ] {
                assert_eq!(from_computed(text), Some(opaque), "{text}");
            }
            assert_eq!(
                from_computed("rgba(0, 120, 215, 0.5)"),
                Some(SrgbColor::try_new(0.0, 120.0 / 255.0, 215.0 / 255.0, 0.5).expect("in range"))
            );
            assert_eq!(
                from_computed("rgb(0.5, 127.5, 255)"),
                Some(SrgbColor::try_new(0.5 / 255.0, 0.5, 1.0, 1.0).expect("in range"))
            );
            for text in [
                "",
                "AccentColor",
                "color(display-p3 0 0.5 1)",
                "rgb(0, 120)",
                "rgb(0, 120, 215, 0.5)",
                "rgba(0, 120, 215)",
                "rgb(0, 120, 256)",
                "rgb(-1, 120, 215)",
                "rgba(0, 120, 215, 1.5)",
                "rgb(0, 120, 215",
                "rgb(NaN, 120, 215)",
            ] {
                assert_eq!(from_computed(text), None, "{text:?}");
            }
        }
    }
}
