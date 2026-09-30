//! [`GlobalWidgetsLocalizations`] — direction-aware widgets localizations
//! for any locale.

use flui_painting::typography::TextDirection;
use flui_platform_api::Locale;

use super::{
    BoxedWidgetsLocalizations, DefaultWidgetsLocalizations, LocalizationsDelegate,
    WidgetsLocalizations,
};

/// The set of [`Locale::language`] codes resolved as right-to-left:
/// Arabic, Farsi (Persian), Hebrew, Pashto, Urdu.
///
/// Sindhi (`sd`) is deliberately absent: it is sometimes listed as RTL, but
/// there is no shipped Sindhi widgets localization to resolve it against.
pub const RTL_LANGUAGES: &[&str] = &["ar", "fa", "he", "ps", "ur"];

/// Localized widgets resources for any [`Locale`], with a correctly-resolved
/// [`TextDirection`] and — for now — [`DefaultWidgetsLocalizations`]'s
/// English strings for every other field.
///
/// **Deferred:** per-language translated strings are not provided. Every
/// `GlobalWidgetsLocalizations` instance, regardless of locale, returns the
/// same English `copy_button_label`/`reorder_item_up`/etc. as
/// [`DefaultWidgetsLocalizations`] — only [`text_direction`](Self::text_direction)
/// differs by locale. This is a real, user-visible gap (an Arabic-locale app
/// gets RTL layout with English button labels), not a silent one: it is
/// named here as the next step for this type, gated on a decision for
/// where FLUI sources per-language translations from.
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
/// [`DefaultWidgetsLocalizationsDelegate`](super::DefaultWidgetsLocalizationsDelegate), which is always LTR.
#[derive(Debug, Clone, Copy, Default)]
pub struct GlobalWidgetsLocalizationsDelegate;

impl LocalizationsDelegate for GlobalWidgetsLocalizationsDelegate {
    type Resources = BoxedWidgetsLocalizations;

    /// Always `true` — every locale gets a [`GlobalWidgetsLocalizations`]
    /// (correct direction, English strings). Gating on "does this locale
    /// have translated strings" would only ever produce false negatives,
    /// since there are no translated strings for *any* locale yet (see
    /// [`GlobalWidgetsLocalizations`]'s docs) — so every locale is accepted
    /// instead.
    fn is_supported(&self, _locale: &Locale) -> bool {
        true
    }

    fn load(&self, locale: &Locale) -> Self::Resources {
        BoxedWidgetsLocalizations::new(GlobalWidgetsLocalizations::for_locale(locale))
    }
}
