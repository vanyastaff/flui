//! Application-scoped widgets: the [`WidgetsApp`] shell, [`SafeArea`], and
//! the [`InheritedTheme`] trait.
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

mod inherited_theme;
mod safe_area;
mod widgets_app;

pub use inherited_theme::InheritedTheme;
pub use safe_area::SafeArea;
pub use widgets_app::{
    AppBuilder, AppForm, NavigatorForm, RouterForm, WidgetsApp, WidgetsAppState,
};

/// Add the standard application keyboard policy to a root view.
///
/// `flui-app` and `HeadlessHost` call this before the runtime installs
/// [`FocusTreeRoot`](flui_view::environment::FocusTreeRoot). Keeping the catalog
/// policy here lets the runtime own the focus-tree contract without depending on
/// shortcuts, actions or any other catalog widget. Custom embedders should use
/// this same function rather than assembling a second root policy.
#[must_use]
pub fn app_root(view: impl flui_view::IntoView) -> crate::DefaultFocusTraversal {
    crate::DefaultFocusTraversal::new(view)
}
