//! The web host's system preferences: what the browser's media queries and
//! `navigator.languages` report, owned by the host rather than its canvas.
//!
//! - `browser`: the queries and listeners, and the source the loop keeps
//!   (`WebPreferences`);
//! - [`media`]: which media features the host observes, and what their
//!   matches mean.
//!
//! The rule module reads no browser object, so every host checks it.

#[cfg(target_arch = "wasm32")]
pub(crate) use browser::WebPreferences;

#[cfg(target_arch = "wasm32")]
mod browser {
    //! Browser accessibility and language observations, owned by the host rather
    //! than its canvas.
    //!
    //! Media queries report motion and contrast, and `navigator.languages` the
    //! preferred locales. Each query's `change` and the window's `languagechange`
    //! only wake the owner, which then samples every observation again.

    use std::sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    };

    use flui_platform::SystemPreferences;
    use wasm_bindgen::{JsCast, closure::Closure};
    use web_sys::{Event, EventTarget, MediaQueryList, Window};

    use super::media::{MediaFeature, MediaMatches};
    use crate::{
        PlatformError, owner::signal::OwnerSignal, owner::sources::SystemPreferenceSource,
        preferences::locales as preferred_locales,
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
            Ok(preferences)
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
