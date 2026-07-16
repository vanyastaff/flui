//! # FLUI Localizations
//!
//! Global (multi-language) localized resources for [`flui_widgets`]'s
//! [`WidgetsLocalizations`](flui_widgets::WidgetsLocalizations) contract —
//! the analog of Flutter's `flutter_localizations` package.
//!
//! ## Scope (v1)
//!
//! [`GlobalWidgetsLocalizations`] resolves the correct
//! [`TextDirection`](flui_types::typography::TextDirection) for any
//! [`Locale`](flui_types::platform::Locale) whose language is in
//! [`RTL_LANGUAGES`] — the part of `flutter_localizations`'s per-language
//! generated classes that is behavior, not translated content. The string
//! resources themselves (`copyButtonLabel`, `reorderItemUp`, …) are **not**
//! translated per language yet: every locale gets the same English strings
//! [`flui_widgets::DefaultWidgetsLocalizations`] already provides. Real
//! per-language string catalogs (the ~80-language `getWidgetsTranslation`
//! generated switch in the oracle) are a named follow-up, not a silent gap —
//! see [`GlobalWidgetsLocalizations`]'s docs.
//!
//! ## Flutter parity
//!
//! `package:flutter_localizations`'s `widgets_localizations.dart` /
//! `l10n/generated_widgets_localizations.dart` (oracle tag `3.44.0`).
//!
//! ## Example
//!
//! ```rust
//! use flui_localizations::{BoxedLocalizationsDelegate, GlobalWidgetsLocalizationsDelegate};
//! use flui_types::platform::Locale;
//! use flui_widgets::{Localizations, SizedBox};
//!
//! let delegates = vec![BoxedLocalizationsDelegate::new(
//!     GlobalWidgetsLocalizationsDelegate,
//! )];
//! let _localized = Localizations::new(
//!     Locale::new("ar", None::<&str>),
//!     delegates,
//!     SizedBox::shrink(),
//! );
//! ```

// Lint levels come from `[workspace.lints]`. Every public item is
// documented, matching `flui-widgets`'s ship bar.
#![deny(missing_docs)]

mod global_widgets_localizations;

pub use global_widgets_localizations::{
    GlobalWidgetsLocalizations, GlobalWidgetsLocalizationsDelegate, RTL_LANGUAGES,
};

// Re-exported so a consumer wiring up `Localizations::new` needs only
// `flui_localizations::{...}` alongside `flui_widgets::{...}`, without a
// separate `flui_widgets::BoxedLocalizationsDelegate` import for the common
// case of registering this crate's one delegate.
pub use flui_widgets::BoxedLocalizationsDelegate;
