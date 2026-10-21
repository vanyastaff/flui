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

/// The locales a POSIX environment prefers for messages, read as gettext
/// reads them; `var` answers one environment variable.
///
/// The messages locale is the first non-empty `LC_ALL`, `LC_MESSAGES`, `LANG`.
/// `C` or `POSIX` there turns translation off, and so is no observation, as is
/// no locale at all. Otherwise `LANGUAGE`'s colon-separated list is the
/// priority order, and the messages locale stands in when that list names
/// nothing usable. An encoding (`.UTF-8`) or modifier (`@euro`) does not name
/// a locale and is dropped.
#[cfg(any(all(target_os = "linux", feature = "winit-backend"), test))]
pub(crate) fn from_posix_environment(var: impl Fn(&str) -> Option<String>) -> Option<Vec<Locale>> {
    let messages = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|name| var(name).filter(|value| !value.is_empty()))?;
    let messages = posix_locale_name(&messages);
    if matches!(messages, "C" | "POSIX") {
        return None;
    }
    let language = var("LANGUAGE").unwrap_or_default();
    from_language_tags(language.split(':').map(posix_locale_name))
        .or_else(|| from_language_tags([messages]))
}

#[cfg(any(all(target_os = "linux", feature = "winit-backend"), test))]
fn posix_locale_name(value: &str) -> &str {
    value.split(['.', '@']).next().unwrap_or_default()
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

    fn posix(variables: &[(&str, &str)]) -> Option<Vec<String>> {
        tags(from_posix_environment(|name| {
            variables
                .iter()
                .find(|(variable, _)| *variable == name)
                .map(|(_, value)| (*value).to_owned())
        }))
    }

    fn a_posix_environment_is_read_as_gettext_reads_it() {
        for (variables, expected) in [
            (&[("LANG", "fr_CA.UTF-8")][..], Some(&["fr-CA"][..])),
            (
                &[
                    ("LANG", "de_DE.UTF-8"),
                    ("LANGUAGE", "sr_RS@latin:en_GB:en"),
                ][..],
                Some(&["sr-RS", "en-GB", "en"][..]),
            ),
            (
                &[("LANG", "en_US.UTF-8"), ("LC_MESSAGES", "pt_BR.UTF-8")][..],
                Some(&["pt-BR"][..]),
            ),
            (
                &[("LC_ALL", "ja_JP.UTF-8"), ("LC_MESSAGES", "pt_BR")][..],
                Some(&["ja-JP"][..]),
            ),
            // An empty variable does not name the messages locale.
            (
                &[("LC_ALL", ""), ("LANG", "it_IT")][..],
                Some(&["it-IT"][..]),
            ),
            // Nothing usable in LANGUAGE leaves the messages locale.
            (
                &[("LANG", "nl_NL"), ("LANGUAGE", ":%%:")][..],
                Some(&["nl-NL"][..]),
            ),
            // C turns translation off, LANGUAGE included.
            (&[("LANG", "C.UTF-8"), ("LANGUAGE", "fr")][..], None),
            (&[("LC_ALL", "POSIX")][..], None),
            (&[][..], None),
        ] {
            assert_eq!(
                posix(variables),
                expected.map(|tags| tags.iter().map(|tag| (*tag).to_owned()).collect()),
                "{variables:?}"
            );
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
                (
                    "a_posix_environment_is_read_as_gettext_reads_it",
                    a_posix_environment_is_read_as_gettext_reads_it,
                ),
            ],
        );
    }
}
