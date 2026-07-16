//! [`GlobalWidgetsLocalizations`] — direction-aware widgets localizations
//! for any locale.
//!
//! Flutter parity: `GlobalWidgetsLocalizations` /
//! `_WidgetsLocalizationsDelegate`
//! (`flutter_localizations/lib/src/widgets_localizations.dart`), and the
//! generated per-language `TextDirection` assignments in
//! `flutter_localizations/lib/src/l10n/generated_widgets_localizations.dart`
//! (oracle tag `3.33.0-0.0.pre`, commit `88e87cd9`).

use flui_types::platform::Locale;
use flui_types::typography::TextDirection;
use flui_widgets::{DefaultWidgetsLocalizations, LocalizationsDelegate, WidgetsLocalizations};

/// The set of [`Locale::language`] codes the oracle's generated
/// `WidgetsLocalization*` classes construct with `TextDirection.rtl`:
/// Arabic, Farsi (Persian), Hebrew, Pashto, Urdu.
///
/// Oracle citation: every `class WidgetsLocalization<Xx> extends
/// GlobalWidgetsLocalizations` in
/// `generated_widgets_localizations.dart` whose constructor body is
/// `super(TextDirection.rtl)` — checked directly against the generated
/// source, not the class doc comment on `GlobalWidgetsLocalizations`, which
/// **additionally** lists `sd` (Sindhi) as RTL. That listing is stale: no
/// `WidgetsLocalizationSd` class exists in the generated file, and `sd` is
/// absent from `kWidgetsSupportedLanguages`. This port follows the generated
/// code's actual behavior, not the doc comment describing it.
pub const RTL_LANGUAGES: &[&str] = &["ar", "fa", "he", "ps", "ur"];

/// Localized widgets resources for any [`Locale`], with a correctly-resolved
/// [`TextDirection`] and — for now — [`DefaultWidgetsLocalizations`]'s
/// English strings for every other field.
///
/// **Deferred:** per-language translated strings (the oracle's ~80-language
/// generated `getWidgetsTranslation` switch) are not ported. Every
/// `GlobalWidgetsLocalizations` instance, regardless of locale, returns the
/// same English `copy_button_label`/`reorder_item_up`/etc. as
/// [`DefaultWidgetsLocalizations`] — only [`text_direction`](Self::text_direction)
/// differs by locale. This is a real, user-visible gap (an Arabic-locale app
/// gets RTL layout with English button labels), not a silent one: it is
/// named here and in the crate root docs as the next slice of this
/// substrate, gated on a decision for where FLUI sources per-language
/// translations from.
#[derive(Debug, Clone, Copy)]
pub struct GlobalWidgetsLocalizations {
    text_direction: TextDirection,
}

impl GlobalWidgetsLocalizations {
    /// Resolve the [`GlobalWidgetsLocalizations`] for `locale`: RTL
    /// direction when [`Locale::language`] is in [`RTL_LANGUAGES`], LTR
    /// otherwise.
    #[must_use]
    pub fn for_locale(locale: &Locale) -> Self {
        let text_direction = if Self::is_rtl_language(locale.language()) {
            TextDirection::Rtl
        } else {
            TextDirection::Ltr
        };
        Self { text_direction }
    }

    /// Whether `language` (a [`Locale::language`] code) is in
    /// [`RTL_LANGUAGES`].
    #[must_use]
    pub fn is_rtl_language(language: &str) -> bool {
        RTL_LANGUAGES.contains(&language)
    }
}

impl WidgetsLocalizations for GlobalWidgetsLocalizations {
    fn text_direction(&self) -> TextDirection {
        self.text_direction
    }

    fn reorder_item_to_start(&self) -> &'static str {
        DefaultWidgetsLocalizations.reorder_item_to_start()
    }
    fn reorder_item_to_end(&self) -> &'static str {
        DefaultWidgetsLocalizations.reorder_item_to_end()
    }
    fn reorder_item_up(&self) -> &'static str {
        DefaultWidgetsLocalizations.reorder_item_up()
    }
    fn reorder_item_down(&self) -> &'static str {
        DefaultWidgetsLocalizations.reorder_item_down()
    }
    fn reorder_item_left(&self) -> &'static str {
        DefaultWidgetsLocalizations.reorder_item_left()
    }
    fn reorder_item_right(&self) -> &'static str {
        DefaultWidgetsLocalizations.reorder_item_right()
    }

    fn copy_button_label(&self) -> &'static str {
        DefaultWidgetsLocalizations.copy_button_label()
    }
    fn cut_button_label(&self) -> &'static str {
        DefaultWidgetsLocalizations.cut_button_label()
    }
    fn paste_button_label(&self) -> &'static str {
        DefaultWidgetsLocalizations.paste_button_label()
    }
    fn select_all_button_label(&self) -> &'static str {
        DefaultWidgetsLocalizations.select_all_button_label()
    }
    fn look_up_button_label(&self) -> &'static str {
        DefaultWidgetsLocalizations.look_up_button_label()
    }
    fn search_web_button_label(&self) -> &'static str {
        DefaultWidgetsLocalizations.search_web_button_label()
    }
    fn share_button_label(&self) -> &'static str {
        DefaultWidgetsLocalizations.share_button_label()
    }

    fn radio_button_unselected_label(&self) -> &'static str {
        DefaultWidgetsLocalizations.radio_button_unselected_label()
    }
}

/// A [`LocalizationsDelegate`] that resolves a [`GlobalWidgetsLocalizations`]
/// for any locale — the multi-language counterpart of
/// `flui_widgets::DefaultWidgetsLocalizationsDelegate`, which is always LTR.
///
/// Flutter parity: `GlobalWidgetsLocalizations.delegate`
/// (`_WidgetsLocalizationsDelegate` in `widgets_localizations.dart`).
#[derive(Debug, Clone, Copy, Default)]
pub struct GlobalWidgetsLocalizationsDelegate;

impl LocalizationsDelegate for GlobalWidgetsLocalizationsDelegate {
    type Resources = flui_widgets::BoxedWidgetsLocalizations;

    /// Always `true` — every locale gets a [`GlobalWidgetsLocalizations`]
    /// (correct direction, English strings). Flutter's own delegate instead
    /// gates on `kWidgetsSupportedLanguages`, a proxy for "does this locale
    /// have translated strings"; since this port has no translated strings
    /// for *any* locale yet (see [`GlobalWidgetsLocalizations`]'s docs), that
    /// gate would only ever produce false negatives here, not a meaningful
    /// signal — so every locale is accepted instead.
    fn is_supported(&self, _locale: &Locale) -> bool {
        true
    }

    fn load(&self, locale: &Locale) -> Self::Resources {
        flui_widgets::BoxedWidgetsLocalizations::new(GlobalWidgetsLocalizations::for_locale(locale))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtl_languages_matches_the_oracles_generated_classes() {
        assert_eq!(RTL_LANGUAGES, &["ar", "fa", "he", "ps", "ur"]);
    }

    #[test]
    fn sindhi_is_not_rtl_despite_the_stale_oracle_doc_comment() {
        // `sd` appears in `GlobalWidgetsLocalizations`'s doc comment but has
        // no generated class — see `RTL_LANGUAGES`'s doc for the citation.
        assert!(!GlobalWidgetsLocalizations::is_rtl_language("sd"));
    }

    #[test]
    fn for_locale_resolves_rtl_for_every_rtl_language() {
        for language in RTL_LANGUAGES {
            let resolved =
                GlobalWidgetsLocalizations::for_locale(&Locale::new(*language, None::<&str>));
            assert_eq!(
                resolved.text_direction(),
                TextDirection::Rtl,
                "{language} must resolve to RTL"
            );
        }
    }

    #[test]
    fn for_locale_resolves_ltr_for_a_non_rtl_language() {
        let resolved = GlobalWidgetsLocalizations::for_locale(&Locale::new("en", Some("US")));
        assert_eq!(resolved.text_direction(), TextDirection::Ltr);
    }

    /// End-to-end canonicalization proof: `Locale::new("iw", ...)`
    /// canonicalizes to `he` in `flui-types`'s constructor (not here), so
    /// resolving through the deprecated `iw` spelling must produce the exact
    /// same [`TextDirection`] as the canonical `he` spelling — the
    /// canonicalization is genuinely load-bearing for RTL detection, not
    /// just an equality/hash curiosity.
    #[test]
    fn deprecated_iw_alias_resolves_rtl_exactly_like_the_canonical_he_spelling() {
        let iw = GlobalWidgetsLocalizations::for_locale(&Locale::new("iw", None::<&str>));
        let he = GlobalWidgetsLocalizations::for_locale(&Locale::new("he", None::<&str>));
        assert_eq!(iw.text_direction(), TextDirection::Rtl);
        assert_eq!(iw.text_direction(), he.text_direction());
    }

    #[test]
    fn global_widgets_localizations_strings_match_the_default_english_set() {
        let global = GlobalWidgetsLocalizations::for_locale(&Locale::new("ar", None::<&str>));
        let default = DefaultWidgetsLocalizations;
        assert_eq!(global.copy_button_label(), default.copy_button_label());
        assert_eq!(global.share_button_label(), default.share_button_label());
        assert_eq!(
            global.radio_button_unselected_label(),
            default.radio_button_unselected_label()
        );
    }

    #[test]
    fn delegate_is_supported_for_every_locale() {
        let delegate = GlobalWidgetsLocalizationsDelegate;
        assert!(delegate.is_supported(&Locale::new("ar", None::<&str>)));
        assert!(delegate.is_supported(&Locale::new("xx", None::<&str>)));
    }

    #[test]
    fn delegate_load_resolves_the_locales_direction() {
        let delegate = GlobalWidgetsLocalizationsDelegate;
        let resources = delegate.load(&Locale::new("ur", None::<&str>));
        assert_eq!(resources.text_direction(), TextDirection::Rtl);
    }
}
