//! [`WidgetsLocalizations`] and [`DefaultWidgetsLocalizations`] — localized
//! resources for the lowest levels of the widget catalog.
//!
//! Flutter parity: `widgets/localizations.dart` `WidgetsLocalizations` /
//! `DefaultWidgetsLocalizations` (oracle tag `3.33.0-0.0.pre`, commit
//! `88e87cd9` — the checked-out `packages/flutter` tree; the plan's
//! requested `3.44.0` tag was not present in the checkout).

use std::any::Any;
use std::fmt;

use flui_types::typography::TextDirection;

/// Interface for localized resource values consumed by the lowest levels of
/// the widget catalog (reorderable-list semantics labels, text-editing menu
/// labels, and the resolved reading [`TextDirection`] for a locale).
///
/// A `WidgetsLocalizations` implementation is what a [`LocalizationsDelegate`]
/// (see the sibling `localizations` module) produces for a given
/// [`Locale`](flui_types::platform::Locale); [`Localizations::of`] retrieves
/// it by (trait-object) type from the ambient scope.
///
/// [`LocalizationsDelegate`]: crate::LocalizationsDelegate
/// [`Localizations::of`]: crate::Localizations::of
///
/// Flutter parity: `WidgetsLocalizations` (`widgets/localizations.dart`).
pub trait WidgetsLocalizations: Any + fmt::Debug + Send + Sync {
    /// The reading direction for text in this locale.
    fn text_direction(&self) -> TextDirection;

    /// Semantics label to reorder an item to the start of a reorderable list.
    fn reorder_item_to_start(&self) -> &'static str;
    /// Semantics label to reorder an item to the end of a reorderable list.
    fn reorder_item_to_end(&self) -> &'static str;
    /// Semantics label to reorder an item one space up a reorderable list.
    fn reorder_item_up(&self) -> &'static str;
    /// Semantics label to reorder an item one space down a reorderable list.
    fn reorder_item_down(&self) -> &'static str;
    /// Semantics label to reorder an item one space left in a reorderable list.
    fn reorder_item_left(&self) -> &'static str;
    /// Semantics label to reorder an item one space right in a reorderable list.
    fn reorder_item_right(&self) -> &'static str;

    /// Semantics label announced when an autocomplete options list becomes
    /// non-empty.
    fn search_results_found(&self) -> &'static str {
        "Search results found"
    }
    /// Semantics label announced when an autocomplete options list becomes
    /// empty.
    fn no_results_found(&self) -> &'static str {
        "No results found"
    }

    /// Label for "copy" edit buttons and menu items.
    fn copy_button_label(&self) -> &'static str;
    /// Label for "cut" edit buttons and menu items.
    fn cut_button_label(&self) -> &'static str;
    /// Label for "paste" edit buttons and menu items.
    fn paste_button_label(&self) -> &'static str;
    /// Label for "select all" edit buttons and menu items.
    fn select_all_button_label(&self) -> &'static str;
    /// Label for "look up" edit buttons and menu items.
    fn look_up_button_label(&self) -> &'static str;
    /// Label for "search web" edit buttons and menu items.
    fn search_web_button_label(&self) -> &'static str;
    /// Label for "share" edit buttons and menu items.
    fn share_button_label(&self) -> &'static str;

    /// The accessibility hint for an unselected radio button.
    fn radio_button_unselected_label(&self) -> &'static str;
}

/// US English localizations for the widgets library — the only locale FLUI
/// ships resource strings for today.
///
/// Flutter parity: `DefaultWidgetsLocalizations`
/// (`widgets/localizations.dart`). Always [`TextDirection::Ltr`], matching
/// the oracle (`DefaultWidgetsLocalizations` is unconditionally LTR; only
/// `GlobalWidgetsLocalizations` — see `flui-localizations` — resolves RTL
/// locales).
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultWidgetsLocalizations;

impl WidgetsLocalizations for DefaultWidgetsLocalizations {
    fn text_direction(&self) -> TextDirection {
        TextDirection::Ltr
    }

    fn reorder_item_to_start(&self) -> &'static str {
        "Move to the start"
    }
    fn reorder_item_to_end(&self) -> &'static str {
        "Move to the end"
    }
    fn reorder_item_up(&self) -> &'static str {
        "Move up"
    }
    fn reorder_item_down(&self) -> &'static str {
        "Move down"
    }
    fn reorder_item_left(&self) -> &'static str {
        "Move left"
    }
    fn reorder_item_right(&self) -> &'static str {
        "Move right"
    }

    fn copy_button_label(&self) -> &'static str {
        "Copy"
    }
    fn cut_button_label(&self) -> &'static str {
        "Cut"
    }
    fn paste_button_label(&self) -> &'static str {
        "Paste"
    }
    fn select_all_button_label(&self) -> &'static str {
        "Select all"
    }
    fn look_up_button_label(&self) -> &'static str {
        "Look Up"
    }
    fn search_web_button_label(&self) -> &'static str {
        "Search Web"
    }
    fn share_button_label(&self) -> &'static str {
        "Share"
    }

    fn radio_button_unselected_label(&self) -> &'static str {
        "Not selected"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_widgets_localizations_is_always_ltr() {
        assert_eq!(
            DefaultWidgetsLocalizations.text_direction(),
            TextDirection::Ltr
        );
    }

    #[test]
    fn default_widgets_localizations_matches_the_oracle_strings() {
        let l = DefaultWidgetsLocalizations;
        assert_eq!(l.reorder_item_to_start(), "Move to the start");
        assert_eq!(l.reorder_item_to_end(), "Move to the end");
        assert_eq!(l.reorder_item_up(), "Move up");
        assert_eq!(l.reorder_item_down(), "Move down");
        assert_eq!(l.reorder_item_left(), "Move left");
        assert_eq!(l.reorder_item_right(), "Move right");
        assert_eq!(l.search_results_found(), "Search results found");
        assert_eq!(l.no_results_found(), "No results found");
        assert_eq!(l.copy_button_label(), "Copy");
        assert_eq!(l.cut_button_label(), "Cut");
        assert_eq!(l.paste_button_label(), "Paste");
        assert_eq!(l.select_all_button_label(), "Select all");
        assert_eq!(l.look_up_button_label(), "Look Up");
        assert_eq!(l.search_web_button_label(), "Search Web");
        assert_eq!(l.share_button_label(), "Share");
        assert_eq!(l.radio_button_unselected_label(), "Not selected");
    }
}
