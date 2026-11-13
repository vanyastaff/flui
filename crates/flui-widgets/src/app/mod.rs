//! Application-scoped widgets: the [`WidgetsApp`] shell, [`SafeArea`],
//! the [`InheritedTheme`] trait, and [`app_root()`], the root wrapper every
//! composition root installs.
//!
//! These are infrastructure widgets that sit
//! near the root of the widget tree and provide ambient data every descendant
//! can read without explicit parameter threading.
//!
//! Inherited presentation data ([`MediaQuery`](flui_view::environment::MediaQuery)) now
//! lives in `flui-view`'s `environment` module, so text and interaction
//! consumers can read it without depending on application composition.
//!
//! The Material `Theme`/`ThemeData` inherited widget itself now lives in
//! `flui-material` (`flui_material::Theme`/`ThemeData`), which depends on
//! this crate and implements [`InheritedTheme`] against its own theme value —
//! this crate only owns the trait `Theme` implements, not the widget.

mod app_root;
mod inherited_theme;
mod safe_area;
mod widgets_app;

pub use app_root::app_root;
pub use inherited_theme::InheritedTheme;
pub use safe_area::SafeArea;
pub use widgets_app::{AppBuilder, WidgetsApp, WidgetsAppState};
