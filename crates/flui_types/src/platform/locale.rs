//! Locale information

use std::fmt;

/// An identifier for a locale
///
/// Similar to Flutter's `Locale`. Identifies a specific language and
/// optionally a country/region.
///
/// # Examples
///
/// ```
/// use flui_types::platform::Locale;
///
/// let locale = Locale::new("en", Some("US"));
/// assert_eq!(locale.language(), "en");
/// assert_eq!(locale.country(), Some("US"));
/// assert_eq!(locale.to_string(), "en_US");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

    /// Returns the language code
    pub fn language(&self) -> &str {
        &self.language
    }

    /// Returns the country code, if set
    pub fn country(&self) -> Option<&str> {
        self.country.as_deref()
    }

    /// Returns the script code, if set
    pub fn script(&self) -> Option<&str> {
        self.script.as_deref()
    }

    /// Returns a locale tag string (language_country)
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::Locale;
    ///
    /// let locale = Locale::new("en", Some("US"));
    /// assert_eq!(locale.to_language_tag(), "en_US");
    ///
    /// let locale = Locale::new("fr", None::<String>);
    /// assert_eq!(locale.to_language_tag(), "fr");
    /// ```
    pub fn to_language_tag(&self) -> String {
        if let Some(country) = &self.country {
            format!("{}_{}", self.language, country)
        } else {
            self.language.clone()
        }
    }
}

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_language_tag())
    }
}

// Common locales
impl Locale {
    /// English (United States)
    pub fn en_us() -> Self {
        Self::new("en", Some("US"))
    }

    /// English (United Kingdom)
    pub fn en_gb() -> Self {
        Self::new("en", Some("GB"))
    }

    /// Spanish (Spain)
    pub fn es_es() -> Self {
        Self::new("es", Some("ES"))
    }

    /// French (France)
    pub fn fr_fr() -> Self {
        Self::new("fr", Some("FR"))
    }

    /// German (Germany)
    pub fn de_de() -> Self {
        Self::new("de", Some("DE"))
    }

    /// Chinese (China)
    pub fn zh_cn() -> Self {
        Self::new("zh", Some("CN"))
    }

    /// Japanese (Japan)
    pub fn ja_jp() -> Self {
        Self::new("ja", Some("JP"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locale_new() {
        let locale = Locale::new("en", Some("US"));
        assert_eq!(locale.language(), "en");
        assert_eq!(locale.country(), Some("US"));
        assert_eq!(locale.script(), None);
    }

    #[test]
    fn test_locale_with_script() {
        let locale = Locale::with_script("zh", Some("CN"), Some("Hans"));
        assert_eq!(locale.language(), "zh");
        assert_eq!(locale.country(), Some("CN"));
        assert_eq!(locale.script(), Some("Hans"));
    }

    #[test]
    fn test_locale_to_language_tag() {
        let locale1 = Locale::new("en", Some("US"));
        assert_eq!(locale1.to_language_tag(), "en_US");

        let locale2 = Locale::new("fr", None::<String>);
        assert_eq!(locale2.to_language_tag(), "fr");
    }

    #[test]
    fn test_locale_display() {
        let locale = Locale::new("en", Some("GB"));
        assert_eq!(locale.to_string(), "en_GB");
    }

    #[test]
    fn test_locale_common() {
        assert_eq!(Locale::en_us().to_language_tag(), "en_US");
        assert_eq!(Locale::en_gb().to_language_tag(), "en_GB");
        assert_eq!(Locale::es_es().to_language_tag(), "es_ES");
        assert_eq!(Locale::fr_fr().to_language_tag(), "fr_FR");
        assert_eq!(Locale::de_de().to_language_tag(), "de_DE");
        assert_eq!(Locale::zh_cn().to_language_tag(), "zh_CN");
        assert_eq!(Locale::ja_jp().to_language_tag(), "ja_JP");
    }
}
