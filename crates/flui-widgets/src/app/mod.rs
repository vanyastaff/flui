//! Application-scoped inherited widgets: [`MediaQuery`] and [`Theme`].
//!
//! These are the closest Flutter-equivalent infrastructure widgets — they sit
//! near the root of the widget tree and provide ambient data every descendant
//! can read without explicit parameter threading.
//!
//! | Widget | Data type | Flutter equivalent |
//! |---|---|---|
//! | [`MediaQuery`] | [`MediaQueryData`] | `MediaQuery` / `MediaQueryData` |
//! | [`Theme`] | [`ThemeData`] | `Theme` / `ThemeData` |

mod inherited_theme;
mod media_query;
mod safe_area;
mod theme;

pub use inherited_theme::InheritedTheme;
pub use media_query::{MediaQuery, MediaQueryData};
pub use safe_area::SafeArea;
pub use theme::{Theme, ThemeData};
