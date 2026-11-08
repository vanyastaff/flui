//! Ambient direction and localized-resource infrastructure: [`Directionality`],
//! [`Localizations`], and the [`WidgetsLocalizations`] resource contract.
//!
//! See `localizations`'s module docs for the sync-only-v1 limits (no async
//! delegate loading, no `Semantics` wrapper).

mod directionality;
mod locale_resolution;
mod localizations;
mod widgets_localizations;

pub use directionality::Directionality;
pub use directionality::axis_direction_from_axis_reverse_and_directionality;
pub(crate) use directionality::resolve_alignment;
pub use locale_resolution::basic_locale_list_resolution;
pub use localizations::{
    BoxedLocalizationsDelegate, BoxedWidgetsLocalizations, DefaultWidgetsLocalizationsDelegate,
    Localizations, LocalizationsDelegate,
};
pub use widgets_localizations::{DefaultWidgetsLocalizations, WidgetsLocalizations};
