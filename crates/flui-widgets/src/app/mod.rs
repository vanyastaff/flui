//! Application-scoped widgets: the [`WidgetsApp`] shell, [`SafeArea`], and
//! the [`InheritedTheme`] trait.
//!
//! These are infrastructure widgets that sit
//! near the root of the widget tree and provide ambient data every descendant
//! can read without explicit parameter threading.
//!
//! Inherited presentation data is provided by [`crate::MediaQuery`] in the
//! lower widget layer, so text and interaction consumers can read it without
//! depending on application composition.
//!
//! The Material `Theme`/`ThemeData` inherited widget itself now lives in
//! `flui-material` (`flui_material::Theme`/`ThemeData`), which depends on
//! this crate and implements [`InheritedTheme`] against its own theme value —
//! this crate only owns the trait `Theme` implements, not the widget.

mod inherited_theme;
mod safe_area;
mod widgets_app;

pub use inherited_theme::InheritedTheme;
pub use safe_area::SafeArea;
pub use widgets_app::{
    AppBuilder, AppForm, NavigatorForm, RouterForm, WidgetsApp, WidgetsAppState,
};
