//! [`CupertinoApp`] — the iOS-style application shell.
//!
//! Composed over `flui-widgets`' design-neutral `WidgetsApp`
//! (ADR-0042 §5, ADR-0028): `WidgetsApp`
//! knows nothing about this widget; this widget resolves the
//! [`CupertinoThemeData`] and publishes it through [`CupertinoTheme`],
//! **above** the `WidgetsApp` (unlike `MaterialApp`,
//! which resolves inside the `WidgetsApp` builder).
//!
//! ## Brightness — Apple's model, no `ThemeMode` (ADR-0042 §2)
//!
//! There is deliberately no selection enum here. The theme's optional
//! [`brightness`](CupertinoThemeData::with_brightness) override wins when
//! set; otherwise every dynamic color resolves against the ambient
//! `MediaQueryData::platform_brightness`.
//! Resolution happens in this widget's own `build`
//! (`CupertinoThemeData::resolve_from`),
//! which registers a `MediaQuery` dependency — a platform-brightness
//! republish from the UI runtime's root `MediaQuery` rebuilds this element and
//! re-resolves the theme.
//!
//! ## Deferred (named gaps, not silent ones)
//!
//! - **`CupertinoUserInterfaceLevel`** — no elevation ambient exists in FLUI
//!   yet; dynamic colors already resolve elevation as their base variant
//!   (see `colors.rs`).
//! - **`DefaultSelectionStyle`**, **`ScrollConfiguration`** /
//!   `CupertinoScrollBehavior` — neither widget exists in FLUI yet.
//! - **`HeroControllerScope`** — FLUI's `Navigator` auto-installs a default
//!   hero controller, so no explicit scope is needed here.
//! - **`CupertinoLocalizations`** — FLUI ships no Cupertino resource set
//!   yet; when one lands its delegate is appended here.
//! - **`Title`/`color`, restoration, debug overlays** — deferred at the
//!   `WidgetsApp` layer; see its module docs.

use std::fmt;
use std::sync::Arc;

use flui_sdk::platform::Locale;
use flui_sdk::view::BoxedView;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{
    AppForm, BoxedLocalizationsDelegate, NavigatorForm, NavigatorHandle, NavigatorObserver,
    Routable, Router, RouterForm, WidgetsApp,
};

use crate::theme::{CupertinoTheme, CupertinoThemeData};

/// An iOS-style application shell: [`WidgetsApp`] under a published,
/// brightness-resolved [`CupertinoTheme`].
///
/// See the module docs for the brightness model, named deferrals, and
/// ordering.
///
/// # Example
///
/// ```rust,ignore
/// use flui_cupertino::{CupertinoApp, CupertinoThemeData};
/// use flui_sdk::platform::Brightness;
///
/// flui::run_app(
///     CupertinoApp::new(MyHome::new())
///         // Optional: pin brightness instead of following the platform.
///         .theme(CupertinoThemeData::default().with_brightness(Brightness::Dark)),
///     flui::AppLaunch::new(),
/// )?;
/// ```
#[derive(Clone, StatelessView)]
pub struct CupertinoApp<F: AppForm = NavigatorForm> {
    app: WidgetsApp<F>,
    theme: Option<CupertinoThemeData>,
}

impl<F: AppForm> fmt::Debug for CupertinoApp<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CupertinoApp")
            .field("app", &self.app)
            .field("has_theme", &self.theme.is_some())
            .finish_non_exhaustive()
    }
}

impl CupertinoApp<NavigatorForm> {
    /// A Cupertino app whose navigator shows `home` as its root route — see
    /// `WidgetsApp::new` for the seeding contract.
    #[must_use]
    pub fn new(home: impl IntoView) -> Self {
        Self::from_widgets_app(WidgetsApp::new(home))
    }

    /// A Cupertino app with no navigator: `builder` receives `None` and
    /// supplies the whole subtree — see `WidgetsApp::with_builder`. The
    /// [`CupertinoTheme`] still wraps it.
    #[must_use]
    pub fn with_builder(
        builder: impl Fn(&dyn BuildContext, Option<BoxedView>) -> BoxedView + 'static,
    ) -> Self {
        Self::from_widgets_app(WidgetsApp::with_builder(builder))
    }

    /// Drive the navigator through a caller-owned handle — see
    /// `WidgetsApp::navigator` for the pre-seeded-handle contract.
    #[must_use]
    pub fn navigator(mut self, handle: NavigatorHandle) -> Self {
        self.app = self.app.navigator(handle);
        self
    }

    /// Register `observer` on the navigator at mount; see
    /// `WidgetsApp::observer`.
    #[must_use]
    pub fn observer(mut self, observer: Arc<dyn NavigatorObserver>) -> Self {
        self.app = self.app.observer(observer);
        self
    }
}

impl CupertinoApp<RouterForm> {
    /// A Cupertino app rooted in `router`, below the published Cupertino theme.
    ///
    /// The returned router form has no `navigator` or `observer` builder:
    /// the Router owns the app's only navigator (ADR-0093 §4).
    #[must_use]
    pub fn router<R: Routable>(router: Router<R>) -> Self {
        Self::from_widgets_app(WidgetsApp::router(router))
    }
}

impl<F: AppForm> CupertinoApp<F> {
    fn from_widgets_app(app: WidgetsApp<F>) -> Self {
        Self { app, theme: None }
    }

    /// The app-wide theme, default
    /// [`CupertinoThemeData::default`]. Its optional brightness override is
    /// the only brightness knob (no `ThemeMode` — see the module docs).
    #[must_use]
    pub fn theme(mut self, theme: CupertinoThemeData) -> Self {
        self.theme = Some(theme);
        self
    }

    /// Wrap the routing subtree — passed straight
    /// through to `WidgetsApp` (the published [`CupertinoTheme`] sits above
    /// the whole `WidgetsApp`, so the builder's context resolves it without
    /// any extra wrapper — unlike `MaterialApp`'s double-`Builder`).
    #[must_use]
    pub fn builder(
        mut self,
        builder: impl Fn(&dyn BuildContext, Option<BoxedView>) -> BoxedView + 'static,
    ) -> Self {
        self.app = self.app.builder(builder);
        self
    }

    /// Force this locale — see `WidgetsApp::locale` for the
    /// resolved-against-supported contract.
    #[must_use]
    pub fn locale(mut self, locale: Locale) -> Self {
        self.app = self.app.locale(locale);
        self
    }

    /// The locales this application supports — see
    /// `WidgetsApp::supported_locales`.
    ///
    /// # Panics
    ///
    /// An empty list always fails: debug builds panic here, at
    /// construction; release builds panic later, during build, when locale
    /// resolution reads the first supported locale.
    #[must_use]
    pub fn supported_locales(mut self, locales: Vec<Locale>) -> Self {
        self.app = self.app.supported_locales(locales);
        self
    }

    /// Delegates producing this application's localized resources — see
    /// `WidgetsApp::localizations_delegates` for the ordering contract.
    #[must_use]
    pub fn localizations_delegates(mut self, delegates: Vec<BoxedLocalizationsDelegate>) -> Self {
        self.app = self.app.localizations_delegates(delegates);
        self
    }
}

impl<F: AppForm> StatelessView for CupertinoApp<F> {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        // The theme (or the default) is resolved HERE, in this element's own context, so the
        // MediaQuery dependency registered by dynamic-color resolution
        // re-runs this build when the ambient platform brightness changes.
        let effective = self.theme.clone().unwrap_or_default().resolve_from(ctx);

        // The resolved theme's text style is the application-level text style.
        let text_style = effective.text_theme().text_style();
        let app = self.app.clone().text_style(text_style);

        // The published theme sits ABOVE the WidgetsApp.
        CupertinoTheme::new(effective, app)
    }
}
