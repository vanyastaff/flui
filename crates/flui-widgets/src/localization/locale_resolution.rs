//! [`basic_locale_list_resolution`] — the default locale-resolution
//! algorithm.
//!
//! Flutter parity: `basicLocaleListResolution`
//! (`widgets/app.dart::basicLocaleListResolution`, oracle tag
//! `3.33.0-0.0.pre`, commit `88e87cd9` — the checked-out `packages/flutter`
//! tree; the plan's requested `3.44.0` tag was not present in the checkout).
//! Ported in full, including the deferred-language-match tie-break and the
//! country-only fallback.

use std::collections::HashMap;

use flui_types::platform::Locale;

/// Composite lookup keys, built once per `supported_locales` entry so the
/// resolution loop below is a hash lookup per preferred locale rather than a
/// linear scan. Rust tuple/`Option` keys replace the oracle's
/// `"${a}_${b}_${c}"` string-concatenation keys (Dart's `null` interpolates
/// to the literal substring `"null"`, which only works because no real
/// subtag spells that word) — same partition, no string-formatting
/// footgun.
struct SupportedLocaleIndex<'a> {
    /// language + script + country -> supported locale (perfect match).
    exact: HashMap<(&'a str, Option<&'a str>, Option<&'a str>), &'a Locale>,
    /// language + script -> supported locale.
    language_and_script: HashMap<(&'a str, &'a str), &'a Locale>,
    /// language + country -> supported locale.
    language_and_country: HashMap<(&'a str, &'a str), &'a Locale>,
    /// language -> supported locale.
    language: HashMap<&'a str, &'a Locale>,
    /// country (possibly absent) -> supported locale.
    country: HashMap<Option<&'a str>, &'a Locale>,
}

impl<'a> SupportedLocaleIndex<'a> {
    fn build(supported_locales: &'a [Locale]) -> Self {
        let mut index = Self {
            exact: HashMap::new(),
            language_and_script: HashMap::new(),
            language_and_country: HashMap::new(),
            language: HashMap::new(),
            country: HashMap::new(),
        };
        for locale in supported_locales {
            // `.or_insert` mirrors the oracle's `??=`: only the FIRST
            // supported locale claiming a given key wins.
            index
                .exact
                .entry((locale.language(), locale.script(), locale.country()))
                .or_insert(locale);
            if let Some(script) = locale.script() {
                index
                    .language_and_script
                    .entry((locale.language(), script))
                    .or_insert(locale);
            }
            if let Some(country) = locale.country() {
                index
                    .language_and_country
                    .entry((locale.language(), country))
                    .or_insert(locale);
            }
            index.language.entry(locale.language()).or_insert(locale);
            index.country.entry(locale.country()).or_insert(locale);
        }
        index
    }
}

/// The default locale-resolution algorithm: resolves the earliest preferred
/// locale that matches the most fields, prioritizing perfect match →
/// language+script → language+country → language-only → (once every
/// preferred locale is exhausted) country-only → the first supported locale.
///
/// This algorithm prioritizes speed over resolution quality on edge cases —
/// it does not account for language distance (e.g. it will not prefer `fr`
/// over `zh` as a fallback for unsupported `de`, even though French is
/// closer to German).
///
/// # Matching priority
///
/// 1. [`Locale::language`], [`Locale::script`], and [`Locale::country`] all match.
/// 2. [`Locale::language`] and [`Locale::script`] only.
/// 3. [`Locale::language`] and [`Locale::country`] only.
/// 4. [`Locale::language`] only — with a caveat: a language-only match found
///    on a non-final preferred locale is *deferred* one iteration, so a
///    higher-accuracy match on the very next preferred locale can supersede
///    it. The first (most-preferred) locale is exempt from this deferral
///    unless the next preferred locale shares its language code.
/// 5. [`Locale::country`] only, once every preferred locale has been checked
///    and none produced a language match.
/// 6. The first element of `supported_locales`.
///
/// # Panics
///
/// Panics if `supported_locales` is empty — mirrors the oracle's unchecked
/// `supportedLocales.first`, which throws `StateError` on an empty
/// `Iterable`. An app must declare at least one supported locale.
#[must_use]
pub fn basic_locale_list_resolution(
    preferred_locales: Option<&[Locale]>,
    supported_locales: &[Locale],
) -> Locale {
    let first_supported = supported_locales
        .first()
        .expect("BUG: basic_locale_list_resolution requires a non-empty supported_locales list");

    // `preferred_locales` is `None`/empty before the platform has reported
    // locales, or on platforms without locale-passing support. Default to
    // the first supported locale, matching the oracle.
    let Some(preferred_locales) = preferred_locales.filter(|locales| !locales.is_empty()) else {
        return first_supported.clone();
    };

    let index = SupportedLocaleIndex::build(supported_locales);

    // A language-only match is possibly low quality, so it isn't returned
    // instantly — the next preferred locale gets a chance at a
    // higher-accuracy match first, and only when that chance is exhausted
    // (or there is no next locale) does the deferred match win.
    let mut matches_language: Option<&Locale> = None;
    let mut matches_country: Option<&Locale> = None;

    for (locale_index, user_locale) in preferred_locales.iter().enumerate() {
        // Perfect match: return the *preferred* locale itself (not the
        // supported-list entry) — oracle parity, `return userLocale;`.
        if index.exact.contains_key(&(
            user_locale.language(),
            user_locale.script(),
            user_locale.country(),
        )) {
            return user_locale.clone();
        }

        // Language + script match.
        if let Some(script) = user_locale.script()
            && let Some(matched) = index
                .language_and_script
                .get(&(user_locale.language(), script))
        {
            return (*matched).clone();
        }

        // Language + country match.
        if let Some(country) = user_locale.country()
            && let Some(matched) = index
                .language_and_country
                .get(&(user_locale.language(), country))
        {
            return (*matched).clone();
        }

        // A deferred language-only match from the previous (higher-ranked)
        // preferred locale wins if this locale did not produce a better one
        // above.
        if let Some(matched) = matches_language {
            return matched.clone();
        }

        // Look up (but don't necessarily return) a language-only match.
        if let Some(matched) = index.language.get(user_locale.language()) {
            matches_language = Some(matched);
            // The first (default) preferred locale is usually highly
            // preferred, so its language-only match returns immediately —
            // unless the *next* preferred locale shares the same language
            // code, in which case we defer to let that iteration's
            // higher-accuracy checks run first.
            let next_shares_language = preferred_locales
                .get(locale_index + 1)
                .is_some_and(|next| next.language() == user_locale.language());
            if locale_index == 0 && !next_shares_language {
                return (*matched).clone();
            }
        }

        // Country-only match: recorded once, from the first preferred
        // locale that has a country and matches one.
        if matches_country.is_none()
            && let Some(country) = user_locale.country()
            && let Some(matched) = index.country.get(&Some(country))
        {
            matches_country = Some(matched);
        }
    }

    matches_language
        .or(matches_country)
        .unwrap_or(first_supported)
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(language: &str, country: Option<&str>) -> Locale {
        Locale::new(language, country)
    }

    fn ls(language: &str, script: Option<&str>, country: Option<&str>) -> Locale {
        Locale::with_script(language, country, script)
    }

    #[test]
    fn empty_preferred_locales_returns_first_supported() {
        let supported = vec![l("en", Some("US")), l("fr", None)];
        assert_eq!(
            basic_locale_list_resolution(Some(&[]), &supported),
            supported[0]
        );
        assert_eq!(basic_locale_list_resolution(None, &supported), supported[0]);
    }

    #[test]
    fn perfect_match_returns_the_preferred_locale_instance() {
        let supported = vec![l("en", Some("US")), l("fr", Some("FR"))];
        let preferred = vec![l("fr", Some("FR"))];
        let resolved = basic_locale_list_resolution(Some(&preferred), &supported);
        assert_eq!(resolved, l("fr", Some("FR")));
    }

    #[test]
    fn language_and_script_match_beats_language_only() {
        let supported = vec![ls("zh", Some("Hans"), None), l("zh", None)];
        let preferred = vec![ls("zh", Some("Hans"), Some("CN"))];
        let resolved = basic_locale_list_resolution(Some(&preferred), &supported);
        assert_eq!(resolved, ls("zh", Some("Hans"), None));
    }

    #[test]
    fn language_and_country_match_beats_language_only() {
        let supported = vec![l("en", Some("GB")), l("en", None)];
        let preferred = vec![l("en", Some("GB"))];
        let resolved = basic_locale_list_resolution(Some(&preferred), &supported);
        assert_eq!(resolved, l("en", Some("GB")));
    }

    #[test]
    fn first_preferred_language_only_match_returns_immediately() {
        // No exact/script/country match for `es`, and the next preferred
        // locale does not share `es`'s language — so the language-only
        // match on the first (most preferred) locale wins immediately.
        let supported = vec![l("en", Some("US")), l("es", Some("MX"))];
        let preferred = vec![l("es", Some("AR")), l("fr", None)];
        let resolved = basic_locale_list_resolution(Some(&preferred), &supported);
        assert_eq!(resolved, l("es", Some("MX")));
    }

    #[test]
    fn deferred_language_match_is_superseded_by_a_better_next_match() {
        // The immediate-return shortcut only applies to the FIRST preferred
        // locale (`es`, which matches nothing here); `de_AT`'s language-only
        // match is on the *second* preferred locale, so it is deferred —
        // and the third preferred locale's perfect match on `fr_FR` must
        // supersede it.
        let supported = vec![l("de", Some("DE")), l("fr", Some("FR"))];
        let preferred = vec![
            l("es", Some("XX")),
            l("de", Some("AT")),
            l("fr", Some("FR")),
        ];
        let resolved = basic_locale_list_resolution(Some(&preferred), &supported);
        assert_eq!(resolved, l("fr", Some("FR")));
    }

    #[test]
    fn first_preferred_language_only_match_returns_immediately_even_with_a_better_next_locale() {
        // Oracle parity subtlety: the FIRST preferred locale's language-only
        // match returns immediately (it is "highly preferred") UNLESS the
        // *next* preferred locale shares the same language code — a later
        // perfect match on an unrelated language does NOT supersede it, even
        // though naive "always defer" reasoning would suggest otherwise.
        let supported = vec![l("de", Some("DE")), l("fr", Some("FR"))];
        let preferred = vec![l("de", Some("AT")), l("fr", Some("FR"))];
        let resolved = basic_locale_list_resolution(Some(&preferred), &supported);
        assert_eq!(resolved, l("de", Some("DE")));
    }

    #[test]
    fn deferred_language_match_wins_when_nothing_better_follows() {
        let supported = vec![l("de", Some("DE")), l("fr", Some("FR"))];
        // `de` language-only match is deferred (not the first preferred
        // locale co-located with a same-language next entry), and the next
        // preferred locale (`it`) has no match at all — so the deferred `de`
        // match must win over the eventual `supported_locales.first()` fallback.
        let preferred = vec![l("es", None), l("de", Some("AT")), l("it", None)];
        let resolved = basic_locale_list_resolution(Some(&preferred), &supported);
        assert_eq!(resolved, l("de", Some("DE")));
    }

    #[test]
    fn same_language_repeated_defers_to_the_next_iteration() {
        // The first preferred locale (`pt_BR`) only gets a language-only
        // match; the *next* preferred locale shares the language (`pt_PT`)
        // and has no better match either — so the first iteration must NOT
        // return immediately (next_shares_language == true), and the
        // deferred match resolves on the second iteration instead.
        let supported = vec![l("pt", Some("PT"))];
        let preferred = vec![l("pt", Some("BR")), l("pt", Some("PT"))];
        let resolved = basic_locale_list_resolution(Some(&preferred), &supported);
        // `pt_PT` is a perfect match on the second iteration and wins.
        assert_eq!(resolved, l("pt", Some("PT")));
    }

    #[test]
    fn country_only_fallback_when_no_language_matches() {
        let supported = vec![l("en", Some("US")), l("fr", Some("CA"))];
        let preferred = vec![l("de", Some("CA"))];
        let resolved = basic_locale_list_resolution(Some(&preferred), &supported);
        assert_eq!(resolved, l("fr", Some("CA")));
    }

    #[test]
    fn no_match_at_all_falls_back_to_first_supported() {
        let supported = vec![l("en", Some("US")), l("fr", Some("FR"))];
        let preferred = vec![l("de", Some("DE"))];
        let resolved = basic_locale_list_resolution(Some(&preferred), &supported);
        assert_eq!(resolved, l("en", Some("US")));
    }

    #[test]
    fn resolution_matches_across_deprecated_locale_aliases() {
        // `iw` canonicalizes to `he` at construction (flui-types), so a
        // preferred `Locale::new("iw", ...)` must resolve exactly like the
        // canonical `he` spelling would.
        let supported = vec![l("en", Some("US")), l("he", Some("IL"))];
        let preferred_deprecated = vec![l("iw", Some("IL"))];
        let preferred_canonical = vec![l("he", Some("IL"))];
        assert_eq!(
            basic_locale_list_resolution(Some(&preferred_deprecated), &supported),
            basic_locale_list_resolution(Some(&preferred_canonical), &supported),
        );
    }

    #[test]
    #[should_panic(expected = "non-empty supported_locales")]
    fn empty_supported_locales_panics() {
        let _ = basic_locale_list_resolution(Some(&[l("en", None)]), &[]);
    }
}
