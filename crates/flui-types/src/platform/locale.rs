//! Locale information

use std::fmt;

/// Deprecated ISO 639 language subtags mapped to their IANA "preferred value"
/// replacement.
///
/// Oracle: `dart:ui`'s `Locale._deprecatedLanguageSubtagMap`
/// (`engine/src/flutter/lib/ui/platform_dispatcher.dart`, oracle tag
/// `3.44.0`, table comment "Mappings generated for language subtag registry
/// as of 2019-02-27"). The oracle table lists ~90 historical ISO 639-3
/// retirements; only the three that are reachable through FLUI's RTL
/// detection and locale-resolution surfaces today (`iw`/`in`/`ji`, all
/// three-letter-vs-two-letter Bidi-relevant subtags) are ported. The rest are
/// deferred — a future full-CLDR canonicalizer can extend this table without
/// changing its shape.
const DEPRECATED_LANGUAGE_SUBTAGS: &[(&str, &str)] = &[
    ("in", "id"), // Indonesian; deprecated 1989-01-01
    ("iw", "he"), // Hebrew; deprecated 1989-01-01
    ("ji", "yi"), // Yiddish; deprecated 1989-01-01
];

/// Deprecated ISO 3166 region subtags mapped to their IANA "preferred value"
/// replacement.
///
/// Oracle: `dart:ui`'s `Locale._deprecatedRegionSubtagMap` (same file/tag as
/// [`DEPRECATED_LANGUAGE_SUBTAGS`]). Ported in full — six entries, no scope
/// cut needed.
const DEPRECATED_REGION_SUBTAGS: &[(&str, &str)] = &[
    ("BU", "MM"), // Burma; deprecated 1989-12-05
    ("DD", "DE"), // German Democratic Republic; deprecated 1990-10-30
    ("FX", "FR"), // Metropolitan France; deprecated 1997-07-14
    ("TP", "TL"), // East Timor; deprecated 2002-05-20
    ("YD", "YE"), // Democratic Yemen; deprecated 1990-08-14
    ("ZR", "CD"), // Zaire; deprecated 1997-07-14
];

/// Replaces a deprecated language subtag with its preferred code, if `code`
/// appears in [`DEPRECATED_LANGUAGE_SUBTAGS`]; otherwise returns `code`
/// unchanged.
fn canonicalize_language_subtag(code: &str) -> &str {
    DEPRECATED_LANGUAGE_SUBTAGS
        .iter()
        .find_map(|(deprecated, preferred)| (*deprecated == code).then_some(*preferred))
        .unwrap_or(code)
}

/// Replaces a deprecated region subtag with its preferred code, if `code`
/// appears in [`DEPRECATED_REGION_SUBTAGS`]; otherwise returns `code`
/// unchanged.
fn canonicalize_region_subtag(code: &str) -> &str {
    DEPRECATED_REGION_SUBTAGS
        .iter()
        .find_map(|(deprecated, preferred)| (*deprecated == code).then_some(*preferred))
        .unwrap_or(code)
}

/// An identifier for a user's language and regional preferences.
///
/// Mirrors Flutter's `Locale`: a language code plus optional country
/// and script subtags (e.g. `en_US`, `zh_Hans_CN`), used for
/// localization and text-direction resolution.
///
/// ## Canonicalization
///
/// Deprecated language/region subtags are canonicalized to their preferred
/// form at construction time (`Locale::new("iw", None::<&str>).language() ==
/// "he"`), so two `Locale`s built from different historical spellings of the
/// same subtag compare equal and hash identically — mirroring `dart:ui`'s
/// `Locale` (see this module's deprecated-subtag tables for the oracle
/// citation). The script subtag is passed through unchanged; the oracle
/// does not canonicalize scripts.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
// Deserialize is routed through `LocaleShadow` (below) so the derive cannot
// bypass canonicalization by assigning raw subtags straight into the private
// fields — see `LocaleShadow`'s doc for why this is load-bearing, not
// decorative.
#[cfg_attr(feature = "serde", serde(from = "LocaleShadow"))]
pub struct Locale {
    /// The language code (e.g., "en", "es", "fr")
    language: String,

    /// The country/region code (e.g., "US", "GB", "MX")
    country: Option<String>,

    /// Optional script code (e.g., "Latn", "Cyrl")
    script: Option<String>,
}

/// The wire shape `Locale` deserializes through — plain, uncanonicalized
/// subtags, matching exactly what [`Locale`]'s own (unmodified) `Serialize`
/// derive produces (same field names, so round-tripping is transparent).
///
/// Without this indirection, `#[derive(Deserialize)]` on `Locale` directly
/// would assign incoming JSON straight into the private `language`/`country`
/// fields, bypassing [`Locale::canonical`] entirely: deserializing
/// `{"language":"iw",...}` would produce a `Locale` whose `language()` is
/// still `"iw"` — silently breaking the `Locale::new("iw") ==
/// Locale::new("he")` / matching-hash guarantee the type's own docs promise
/// for every OTHER construction path. Routing through `#[serde(from =
/// "LocaleShadow")]` keeps `Locale::canonical` the sole construction path,
/// including for deserialization.
#[cfg(feature = "serde")]
#[derive(serde::Deserialize)]
struct LocaleShadow {
    language: String,
    country: Option<String>,
    script: Option<String>,
}

#[cfg(feature = "serde")]
impl From<LocaleShadow> for Locale {
    fn from(shadow: LocaleShadow) -> Self {
        Self::canonical(shadow.language, shadow.country, shadow.script)
    }
}

impl Locale {
    /// Builds a `Locale` from already-owned subtags, canonicalizing the
    /// language and region against the deprecated-subtag tables. The sole
    /// construction path every public constructor below (and, behind the
    /// `serde` feature, `LocaleShadow`'s `From` impl) routes through, so
    /// canonicalization happens in exactly one place.
    fn canonical(language: String, country: Option<String>, script: Option<String>) -> Self {
        Self {
            language: canonicalize_language_subtag(&language).to_owned(),
            country: country.map(|code| canonicalize_region_subtag(&code).to_owned()),
            script,
        }
    }

    /// Creates a new locale
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::Locale;
    ///
    /// let locale = Locale::new("en", Some("US"));
    /// assert_eq!(locale.language(), "en");
    /// assert_eq!(locale.country(), Some("US"));
    /// ```
    #[inline]
    pub fn new(language: impl Into<String>, country: Option<impl Into<String>>) -> Self {
        Self::canonical(language.into(), country.map(Into::into), None)
    }

    /// Creates a new locale with a script code
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::Locale;
    ///
    /// let locale = Locale::with_script("zh", Some("CN"), Some("Hans"));
    /// assert_eq!(locale.language(), "zh");
    /// assert_eq!(locale.country(), Some("CN"));
    /// assert_eq!(locale.script(), Some("Hans"));
    /// ```
    #[inline]
    pub fn with_script(
        language: impl Into<String>,
        country: Option<impl Into<String>>,
        script: Option<impl Into<String>>,
    ) -> Self {
        Self::canonical(
            language.into(),
            country.map(Into::into),
            script.map(Into::into),
        )
    }

    /// Returns the language code (e.g. `"en"`).
    #[must_use]
    #[inline]
    pub fn language(&self) -> &str {
        &self.language
    }

    /// Returns the country/region code, if any (e.g. `"US"`).
    #[must_use]
    #[inline]
    pub fn country(&self) -> Option<&str> {
        self.country.as_deref()
    }

    /// Returns the script code, if any (e.g. `"Hans"`).
    #[must_use]
    #[inline]
    pub fn script(&self) -> Option<&str> {
        self.script.as_deref()
    }

    /// Formats this locale as an underscore-separated language tag
    /// (e.g. `"en_US"`, or just `"en"` when there is no country).
    ///
    /// Note: the script code is not included in the output.
    #[must_use]
    #[inline]
    pub fn to_language_tag(&self) -> String {
        if let Some(country) = &self.country {
            format!("{}_{}", self.language, country)
        } else {
            self.language.clone()
        }
    }

    /// Returns `true` if this locale's text direction is left-to-right.
    ///
    /// The complement of [`is_rtl`](Self::is_rtl).
    #[must_use]
    #[inline]
    pub fn is_ltr(&self) -> bool {
        !self.is_rtl()
    }

    /// Returns `true` if this locale's text direction is right-to-left.
    ///
    /// Determined by the language code against a fixed set of RTL
    /// languages (Arabic, Hebrew, Persian, Urdu, Yiddish); the script
    /// code is not consulted.
    #[must_use]
    #[inline]
    pub fn is_rtl(&self) -> bool {
        matches!(
            self.language.as_str(),
            "ar" | "he" | "fa" | "ur" | "yi" | "ji"
        )
    }

    /// Parses a locale from a language tag with `-` or `_` separators.
    ///
    /// Accepts `"en"`, `"en_US"`/`"en-US"`, `"zh_Hans"` (a 4-character
    /// second subtag is treated as a script), and `"zh_Hans_CN"`.
    /// Returns `None` for empty input or more than three subtags.
    #[must_use]
    #[inline]
    pub fn from_language_tag(tag: &str) -> Option<Self> {
        if tag.is_empty() {
            return None;
        }

        // Normalize separators to underscore
        let normalized = tag.replace('-', "_");
        let parts: Vec<&str> = normalized.split('_').collect();

        match parts.len() {
            1 => {
                // Just language: "en"
                Some(Self::new(parts[0], None::<String>))
            }
            2 => {
                // Language + country OR language + script
                // Country codes are typically 2 chars, script codes are 4
                if parts[1].len() == 4 {
                    // Probably a script: "zh_Hans"
                    Some(Self::with_script(parts[0], None::<String>, Some(parts[1])))
                } else {
                    // Probably a country: "en_US"
                    Some(Self::new(parts[0], Some(parts[1])))
                }
            }
            3 => {
                // Language + script + country: "zh_Hans_CN"
                Some(Self::with_script(parts[0], Some(parts[2]), Some(parts[1])))
            }
            _ => None, // Invalid format
        }
    }
}

impl fmt::Display for Locale {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_language_tag())
    }
}

// Common locales
impl Locale {
    /// English (United States)
    #[inline]
    pub fn en_us() -> Self {
        Self::new("en", Some("US"))
    }

    /// English (United Kingdom)
    #[inline]
    pub fn en_gb() -> Self {
        Self::new("en", Some("GB"))
    }

    /// Spanish (Spain)
    #[inline]
    pub fn es_es() -> Self {
        Self::new("es", Some("ES"))
    }

    /// French (France)
    #[inline]
    pub fn fr_fr() -> Self {
        Self::new("fr", Some("FR"))
    }

    /// German (Germany)
    #[inline]
    pub fn de_de() -> Self {
        Self::new("de", Some("DE"))
    }

    /// Chinese (China)
    #[inline]
    pub fn zh_cn() -> Self {
        Self::new("zh", Some("CN"))
    }

    /// Japanese (Japan)
    #[inline]
    pub fn ja_jp() -> Self {
        Self::new("ja", Some("JP"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deprecated_language_subtag_canonicalizes_on_construction() {
        let iw = Locale::new("iw", None::<&str>);
        assert_eq!(
            iw.language(),
            "he",
            "iw must canonicalize to he on construction, not on read"
        );
    }

    #[test]
    fn deprecated_and_preferred_language_subtags_are_equal_and_hash_equal() {
        let iw = Locale::new("iw", None::<&str>);
        let he = Locale::new("he", None::<&str>);
        assert_eq!(
            iw, he,
            "Locale(\"iw\") and Locale(\"he\") must be the same locale"
        );

        let mut set = std::collections::HashSet::new();
        set.insert(iw);
        assert!(
            set.contains(&he),
            "canonicalized locales must hash identically, not just compare equal"
        );
    }

    #[test]
    fn deprecated_region_subtag_canonicalizes_on_construction() {
        // `de_DD` (East Germany) canonicalizes to `de_DE`.
        let dd = Locale::new("de", Some("DD"));
        let de = Locale::new("de", Some("DE"));
        assert_eq!(dd.country(), Some("DE"));
        assert_eq!(dd, de);
    }

    #[test]
    fn all_deprecated_language_subtags_canonicalize() {
        for (deprecated, preferred) in DEPRECATED_LANGUAGE_SUBTAGS {
            assert_eq!(
                Locale::new(*deprecated, None::<&str>).language(),
                *preferred
            );
        }
    }

    #[test]
    fn all_deprecated_region_subtags_canonicalize() {
        for (deprecated, preferred) in DEPRECATED_REGION_SUBTAGS {
            let locale = Locale::new("en", Some(*deprecated));
            assert_eq!(locale.country(), Some(*preferred));
        }
    }

    #[test]
    fn unrecognized_subtags_pass_through_unchanged() {
        let locale = Locale::new("xx", Some("YY"));
        assert_eq!(locale.language(), "xx");
        assert_eq!(locale.country(), Some("YY"));
    }

    #[test]
    fn rtl_detection_matches_the_deprecated_alias() {
        // `iw` canonicalizes to `he`, which is in the RTL set — so the alias
        // must resolve to the same is_rtl() answer as the canonical form,
        // not require every call site to know about the deprecated spelling.
        assert!(Locale::new("iw", None::<&str>).is_rtl());
        assert!(Locale::new("he", None::<&str>).is_rtl());
    }

    #[test]
    fn from_language_tag_canonicalizes_deprecated_subtags() {
        let iw = Locale::from_language_tag("iw").expect("valid single-subtag input");
        assert_eq!(iw, Locale::new("he", None::<&str>));

        let iw_dd = Locale::from_language_tag("iw_DD").expect("valid two-subtag input");
        assert_eq!(iw_dd, Locale::new("he", Some("DE")));
    }

    #[test]
    fn script_subtag_is_not_canonicalized() {
        // The oracle canonicalizes language and region subtags only.
        let locale = Locale::with_script("zh", Some("CN"), Some("Hans"));
        assert_eq!(locale.script(), Some("Hans"));
    }

    // ------------------------------------------------------------------
    // serde: Deserialize must route through the same canonicalizing
    // constructor as every other construction path (LocaleShadow).
    // ------------------------------------------------------------------

    #[cfg(feature = "serde")]
    mod serde_tests {
        use super::*;

        #[test]
        fn deserializing_a_deprecated_subtag_canonicalizes_it() {
            // Raw JSON, not a value built through `Locale::new` — this is
            // exactly the path a bare `#[derive(Deserialize)]` on `Locale`
            // would have bypassed by writing "iw" straight into the private
            // `language` field.
            let iw: Locale =
                serde_json::from_str(r#"{"language":"iw","country":null,"script":null}"#)
                    .expect("valid Locale JSON");
            assert_eq!(
                iw,
                Locale::new("he", None::<&str>),
                "deserializing {{language: \"iw\"}} must canonicalize to \"he\", matching \
                 Locale::new(\"iw\")"
            );
            assert_eq!(iw.language(), "he");
            assert!(
                iw.is_rtl(),
                "the deserialized locale must resolve is_rtl() from the canonical \
                 language, not the raw deprecated spelling"
            );
        }

        #[test]
        fn deserializing_a_deprecated_region_canonicalizes_it() {
            let dd: Locale =
                serde_json::from_str(r#"{"language":"de","country":"DD","script":null}"#)
                    .expect("valid Locale JSON");
            assert_eq!(dd, Locale::new("de", Some("DE")));
            assert_eq!(dd.country(), Some("DE"));
        }

        #[test]
        fn deserialized_deprecated_alias_hashes_identically_to_the_canonical_form() {
            let iw: Locale =
                serde_json::from_str(r#"{"language":"iw","country":null,"script":null}"#)
                    .expect("valid Locale JSON");
            let he = Locale::new("he", None::<&str>);

            let mut set = std::collections::HashSet::new();
            set.insert(iw);
            assert!(
                set.contains(&he),
                "a deserialized deprecated-alias Locale must hash identically to the \
                 canonical spelling, not just compare equal"
            );
        }

        #[test]
        fn serialize_then_deserialize_round_trips_an_already_canonical_locale() {
            let original = Locale::with_script("zh", Some("CN"), Some("Hans"));
            let json = serde_json::to_string(&original).expect("serialize");
            let round_tripped: Locale = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(round_tripped, original);
        }
    }
}
