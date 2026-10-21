//! Preferred-locale lists from the BCP 47 tags a host reads, in priority order.

use flui_platform::Locale;

/// The locales `tags` name, in their order, as a browser's
/// `navigator.languages`, Apple's `preferredLanguages` and Android's
/// `LocaleList` report them.
///
/// A tag that does not parse is skipped rather than failing the list: one
/// entry the OS spells unusually must not hide the user's other languages. A
/// repeated locale keeps its first position. A list with no usable tag is no
/// observation.
pub(crate) fn from_language_tags(
    tags: impl IntoIterator<Item = impl AsRef<str>>,
) -> Option<Vec<Locale>> {
    let mut locales: Vec<Locale> = Vec::new();
    for tag in tags {
        let Ok(locale) = tag.as_ref().trim().parse::<Locale>() else {
            continue;
        };
        if !locales.contains(&locale) {
            locales.push(locale);
        }
    }
    (!locales.is_empty()).then_some(locales)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(locales: Option<Vec<Locale>>) -> Option<Vec<String>> {
        locales.map(|locales| locales.iter().map(Locale::to_language_tag).collect())
    }

    fn priority_order_survives_without_unusable_or_repeated_tags() {
        assert_eq!(
            tags(from_language_tags([
                "fr-CA",
                " en-US ",
                "",
                "en--US",
                "fr-CA",
                "zh-Hant-TW",
                "en_GB",
            ])),
            Some(
                ["fr-CA", "en-US", "zh-Hant-TW", "en-GB"]
                    .map(String::from)
                    .to_vec()
            )
        );
    }

    fn no_usable_tag_is_no_observation() {
        for list in [&[][..], &["", "%%", "en--US"][..]] {
            assert_eq!(from_language_tags(list), None, "{list:?}");
        }
    }

    #[test]
    fn preferred_locale_tags_contract() {
        crate::table_test::run_table(
            "preferred_locale_tags_contract",
            &[
                (
                    "priority_order_survives_without_unusable_or_repeated_tags",
                    priority_order_survives_without_unusable_or_repeated_tags as fn(),
                ),
                (
                    "no_usable_tag_is_no_observation",
                    no_usable_tag_is_no_observation,
                ),
            ],
        );
    }
}
