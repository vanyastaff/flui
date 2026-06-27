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

mod media_query;
mod theme;

pub use media_query::{MediaQuery, MediaQueryData};
pub use theme::{Theme, ThemeData};
