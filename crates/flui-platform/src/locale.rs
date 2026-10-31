//! Complete language identity for resource selection and platform observations.

use language_tags::LanguageTag;
use std::{collections::HashSet, fmt, str::FromStr};

/// A malformed language identifier or component combination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid language tag")]
pub struct InvalidLocale;

/// A syntactically validated BCP 47 language tag.
///
/// Variants, extensions and private-use subtags remain part of equality and
/// serialization. Parsing normalizes casing and accepts legacy `_` separators.
/// Historical `in`/`iw`/`ji` language aliases and six deprecated region aliases
/// retain their preferred spellings. This is not CLDR maximization or registry
/// validation. Explicit scripts are retained.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Locale(LanguageTag);

fn language_alias(language: &str) -> &str {
    match language {
        "in" => "id",
        "iw" => "he",
        "ji" => "yi",
        _ => language,
    }
}

fn region_alias(region: &str) -> &str {
    match region {
        "BU" => "MM",
        "DD" => "DE",
        "FX" => "FR",
        "TP" => "TL",
        "YD" => "YE",
        "ZR" => "CD",
        _ => region,
    }
}

impl FromStr for Locale {
    type Err = InvalidLocale;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.replace('_', "-");
        // The upstream private-use fast path does not check subtag lengths or
        // empty segments. Enforce the shared lexical boundary before parsing.
        if input.split('-').any(|part| {
            part.is_empty() || part.len() > 8 || !part.bytes().all(|b| b.is_ascii_alphanumeric())
        }) {
            return Err(InvalidLocale);
        }
        let parsed = LanguageTag::parse(&input).map_err(|_| InvalidLocale)?;
        let mut variants = HashSet::new();
        let mut extensions = HashSet::new();
        if parsed.variant_subtags().any(|part| !variants.insert(part))
            || parsed
                .extension_subtags()
                .any(|(key, _)| !extensions.insert(key))
        {
            return Err(InvalidLocale);
        }
        let language = language_alias(parsed.primary_language());
        let region = parsed.region();
        if language == parsed.primary_language()
            && region.is_none_or(|value| region_alias(value) == value)
        {
            return Ok(Self(parsed));
        }
        let mut normalized = language.to_owned();
        let mut region_pending = region;
        for part in parsed.as_str()[parsed.primary_language().len()..]
            .split('-')
            .skip(1)
        {
            normalized.push('-');
            if region_pending == Some(part) {
                normalized.push_str(region_alias(part));
                region_pending = None;
            } else {
                normalized.push_str(part);
            }
        }
        LanguageTag::parse(&normalized)
            .map(Self)
            .map_err(|_| InvalidLocale)
    }
}

impl Locale {
    /// Create a language and optional region identifier.
    ///
    /// # Errors
    /// Rejects malformed components; use string parsing for full language tags.
    pub fn try_new(
        language: impl AsRef<str>,
        country: Option<impl AsRef<str>>,
    ) -> Result<Self, InvalidLocale> {
        Self::with_script(language, country, None::<&str>)
    }

    /// Create a language, optional region and optional script identifier.
    ///
    /// # Errors
    /// Rejects malformed components and components supplied in the wrong role.
    pub fn with_script(
        language: impl AsRef<str>,
        country: Option<impl AsRef<str>>,
        script: Option<impl AsRef<str>>,
    ) -> Result<Self, InvalidLocale> {
        let language = language.as_ref().to_ascii_lowercase();
        let country = country
            .as_ref()
            .map(|value| value.as_ref().to_ascii_uppercase());
        let script = script.as_ref().map(AsRef::as_ref);
        let mut tag = language.clone();
        for part in [script, country.as_deref()].into_iter().flatten() {
            tag.push('-');
            tag.push_str(part);
        }
        let locale: Self = tag.parse()?;
        if locale.language() != language_alias(&language)
            || locale.country() != country.as_deref().map(region_alias)
            || !match (locale.script(), script) {
                (Some(actual), Some(authored)) => actual.eq_ignore_ascii_case(authored),
                (None, None) => true,
                _ => false,
            }
            || language.contains('-')
            || language.contains('_')
            || locale.0.variant().is_some()
            || locale.0.extension().is_some()
            || locale.0.private_use().is_some()
            || locale.0.extended_language().is_some()
        {
            return Err(InvalidLocale);
        }
        Ok(locale)
    }

    /// Language component; grandfathered and private-use-only tags retain their full identity.
    #[must_use]
    pub fn language(&self) -> &str {
        self.0.primary_language()
    }
    /// Optional region component.
    #[must_use]
    pub fn country(&self) -> Option<&str> {
        self.0.region()
    }
    /// Optional script component.
    #[must_use]
    pub fn script(&self) -> Option<&str> {
        self.0.script()
    }
    /// Complete normalized BCP 47 tag, including variants and extensions.
    #[must_use]
    pub fn to_language_tag(&self) -> String {
        self.0.as_str().to_owned()
    }
    /// Whether the language's default text direction is left-to-right.
    #[must_use]
    pub fn is_ltr(&self) -> bool {
        !self.is_rtl()
    }
    /// Whether the language's default text direction is right-to-left.
    /// This language-based policy does not infer direction from a script override.
    #[must_use]
    pub fn is_rtl(&self) -> bool {
        matches!(self.language(), "ar" | "fa" | "he" | "ps" | "ur" | "yi")
    }
    /// Parse a full language tag, returning `None` for malformed input.
    #[must_use]
    pub fn from_language_tag(tag: &str) -> Option<Self> {
        tag.parse().ok()
    }

    /// This locale with `keywords` added to its `u` extension, each a key and
    /// its value. A key the locale already carries keeps its value. The
    /// extensions stay in singleton order and the keywords in key order, as
    /// canonical form has them. A tag with no language subtag first (a
    /// private-use or irregular grandfathered tag), or one that does not
    /// parse with the keywords added, is returned as it is.
    pub(crate) fn with_unicode_keywords(&self, keywords: &[(&str, String)]) -> Self {
        let tag = self.0.as_str();
        let subtags: Vec<&str> = tag.split('-').collect();
        if keywords.is_empty() || subtags.first().is_none_or(|first| first.len() == 1) {
            return self.clone();
        }
        // The subtags before the first singleton, the extensions by
        // singleton, and the private-use part from its `x` on.
        let prefix_end = subtags
            .iter()
            .position(|part| part.len() == 1)
            .unwrap_or(subtags.len());
        let mut extensions: Vec<(String, Vec<&str>)> = Vec::new();
        let mut private_use: &[&str] = &[];
        let mut at = prefix_end;
        while at < subtags.len() {
            let singleton = subtags[at].to_ascii_lowercase();
            if singleton == "x" {
                private_use = &subtags[at..];
                break;
            }
            let end = subtags[at + 1..]
                .iter()
                .position(|part| part.len() == 1)
                .map_or(subtags.len(), |offset| at + 1 + offset);
            extensions.push((singleton, subtags[at + 1..end].to_vec()));
            at = end;
        }
        let u = if let Some(index) = extensions
            .iter()
            .position(|(singleton, _)| singleton == "u")
        {
            index
        } else {
            extensions.push(("u".to_owned(), Vec::new()));
            extensions.len() - 1
        };
        // The `u` extension: attributes (three to eight characters) first,
        // then keywords, each a two-character key and its value subtags.
        let body = &extensions[u].1;
        let attributes_end = body
            .iter()
            .position(|part| part.len() == 2)
            .unwrap_or(body.len());
        let mut merged: Vec<(String, Vec<String>)> = Vec::new();
        for part in &body[attributes_end..] {
            if part.len() == 2 {
                merged.push((part.to_ascii_lowercase(), Vec::new()));
            } else if let Some((_, values)) = merged.last_mut() {
                values.push((*part).to_owned());
            }
        }
        for (key, value) in keywords {
            if !merged.iter().any(|(present, _)| present == key) {
                merged.push(((*key).to_owned(), vec![value.clone()]));
            }
        }
        merged.sort_by(|(left, _), (right, _)| left.cmp(right));
        let mut u_body: Vec<String> = body[..attributes_end]
            .iter()
            .map(|part| (*part).to_owned())
            .collect();
        for (key, values) in merged {
            u_body.push(key);
            u_body.extend(values);
        }
        extensions.sort_by(|(left, _), (right, _)| left.cmp(right));

        let mut rebuilt = subtags[..prefix_end].join("-");
        for (singleton, body) in &extensions {
            rebuilt.push('-');
            rebuilt.push_str(singleton);
            if singleton == "u" {
                for part in &u_body {
                    rebuilt.push('-');
                    rebuilt.push_str(part);
                }
            } else {
                for part in body {
                    rebuilt.push('-');
                    rebuilt.push_str(part);
                }
            }
        }
        for part in private_use {
            rebuilt.push('-');
            rebuilt.push_str(part);
        }
        rebuilt.parse::<Self>().unwrap_or_else(|_| self.clone())
    }
    /// English (United States).
    #[must_use]
    pub fn en_us() -> Self {
        "en-US".parse().expect("BUG: valid built-in locale")
    }
    /// English (United Kingdom).
    #[must_use]
    pub fn en_gb() -> Self {
        "en-GB".parse().expect("BUG: valid built-in locale")
    }
    /// Spanish (Spain).
    #[must_use]
    pub fn es_es() -> Self {
        "es-ES".parse().expect("BUG: valid built-in locale")
    }
    /// French (France).
    #[must_use]
    pub fn fr_fr() -> Self {
        "fr-FR".parse().expect("BUG: valid built-in locale")
    }
    /// German (Germany).
    #[must_use]
    pub fn de_de() -> Self {
        "de-DE".parse().expect("BUG: valid built-in locale")
    }
    /// Chinese (China).
    #[must_use]
    pub fn zh_cn() -> Self {
        "zh-CN".parse().expect("BUG: valid built-in locale")
    }
    /// Japanese (Japan).
    #[must_use]
    pub fn ja_jp() -> Self {
        "ja-JP".parse().expect("BUG: valid built-in locale")
    }
}

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Locale {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0.as_str())
    }
}
#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Locale {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let tag = <String as serde::Deserialize>::deserialize(deserializer)?;
        tag.parse().map_err(serde::de::Error::custom)
    }
}
