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
