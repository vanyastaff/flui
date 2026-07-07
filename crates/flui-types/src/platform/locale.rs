//! Locale information

use std::fmt;

/// An identifier for a user's language and regional preferences.
///
/// Mirrors Flutter's `Locale`: a language code plus optional country
/// and script subtags (e.g. `en_US`, `zh_Hans_CN`), used for
/// localization and text-direction resolution.
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Locale {
    /// The language code (e.g., "en", "es", "fr")
    language: String,

    /// The country/region code (e.g., "US", "GB", "MX")
    country: Option<String>,

    /// Optional script code (e.g., "Latn", "Cyrl")
    script: Option<String>,
}

impl Locale {
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
        Self {
            language: language.into(),
            country: country.map(Into::into),
            script: None,
        }
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
        Self {
            language: language.into(),
            country: country.map(Into::into),
            script: script.map(Into::into),
        }
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
