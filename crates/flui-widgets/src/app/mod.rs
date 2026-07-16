//! Application-scoped inherited widgets: [`MediaQuery`] and the
//! [`InheritedTheme`] trait.
//!
//! These are the closest Flutter-equivalent infrastructure widgets — they sit
//! near the root of the widget tree and provide ambient data every descendant
//! can read without explicit parameter threading.
//!
//! | Widget | Data type | Flutter equivalent |
//! |---|---|---|
//! | [`MediaQuery`] | [`MediaQueryData`] | `MediaQuery` / `MediaQueryData` |
//!
//! The Material `Theme`/`ThemeData` inherited widget itself now lives in
//! `flui-material` (`flui_material::Theme`/`ThemeData`), which depends on
//! this crate and implements [`InheritedTheme`] against its own theme value —
//! this crate only owns the trait `Theme` implements, not the widget.

mod inherited_theme;
mod media_query;
mod safe_area;

pub use inherited_theme::InheritedTheme;
pub use media_query::{MediaQuery, MediaQueryData};
pub use safe_area::SafeArea;
