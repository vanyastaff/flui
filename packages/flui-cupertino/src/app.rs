//! [`CupertinoApp`] — the iOS-style application shell.
//!
//! Composed over `flui-widgets`' design-neutral `WidgetsApp`
//! (ADR-0042 §5, ADR-0028): `WidgetsApp`
//! knows nothing about this widget; this widget resolves the
//! [`CupertinoThemeData`] and publishes it through [`CupertinoTheme`],
//! **above** the `WidgetsApp` (unlike `MaterialApp`,
//! which resolves inside the `WidgetsApp` builder), and installs the iOS
//! parallax slide, [`ParallaxPageTransition`], as the ambient page transition
//! (ADR-0221), so a [`Router`]'s pages slide in as `CupertinoPage`s do.
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
//! - **`DefaultSelectionStyle`** — the app does not install one yet.
//! - **`CupertinoScrollBehavior`** — the app installs no `ScrollConfiguration`,
//!   so its scroll views follow `DefaultScrollBehavior` until the Cupertino
//!   behavior lands with its scrollbar and overscroll indicator.
//! - **`HeroControllerScope`** — FLUI's `Navigator` auto-installs a default
//!   hero controller, so no explicit scope is needed here.
//! - **`CupertinoPageRoute`'s route chrome** — the transition-only barrier
//!   dim and the edge-swipe back gesture are options of a route, which the
//!   ambient page transition does not carry, so a Router's pages here slide
//!   but have neither ([gaps](../../../docs/gaps.md#a-router-page-carries-no-route-chrome)).
//! - **`CupertinoLocalizations`** — FLUI ships no Cupertino resource set
//!   yet; when one lands its delegate is appended here.
//! - **`Title`/`color`, restoration, debug overlays**
//!   — deferred at the `WidgetsApp` layer; see its module docs.

use std::fmt;
use std::rc::Rc;

use flui_sdk::platform::Locale;
use flui_sdk::view::BoxedView;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{
    AppBuilder, BoxedLocalizationsDelegate, PageTransitions, ParallaxPageTransition, Routable,
    Router, WidgetsApp,
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
/// )?;
/// ```
#[derive(Clone, StatelessView)]
pub struct CupertinoApp {
    routing: Routing,
    builder: Option<AppBuilder>,
    theme: Option<CupertinoThemeData>,
    locale: Option<Locale>,
    supported_locales: Vec<Locale>,
    localizations_delegates: Vec<BoxedLocalizationsDelegate>,
}

impl fmt::Debug for CupertinoApp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CupertinoApp")
            .field("routing", &self.routing)
            .field("has_builder", &self.builder.is_some())
            .field("has_theme", &self.theme.is_some())
            .finish_non_exhaustive()
    }
}

impl CupertinoApp {
    fn bare(routing: Routing) -> Self {
        Self {
            routing,
            builder: None,
            theme: None,
            locale: None,
            supported_locales: vec![Locale::en_us()],
            localizations_delegates: Vec::new(),
        }
    }

    /// A Cupertino app showing `home`, Flutter's `CupertinoApp(home:)`: one
    /// page with no URL, on which popups open — see `WidgetsApp::new` for the
    /// seeding contract.
    #[must_use]
    pub fn new(home: impl IntoView) -> Self {
        Self::bare(Routing::Home(home.into_view().boxed()))
    }

    /// A Cupertino app rooted in `router`, Flutter's `CupertinoApp.router`:
    /// the [`Router`] is the app's routing subtree, under the published
    /// [`CupertinoTheme`], and its navigator is the app's only one
    /// (ADR-0330 §4).
    ///
    /// ```
    /// use flui_cupertino::CupertinoApp;
    /// use flui_sdk::view::prelude::*;
    /// use flui_sdk::widgets::{Routable, Router, Text};
    ///
    /// #[derive(Routable, Clone, PartialEq)]
    /// enum AppRoute {
    ///     #[route("/")]
    ///     Home,
    /// }
    ///
    /// let app = CupertinoApp::router(Router::new(AppRoute::Home, |_route: &AppRoute, _ctx| {
    ///     Text::new("Home").into_view().boxed()
    /// }));
    /// # let _ = app;
    /// ```
    #[must_use]
    pub fn router<R: Routable>(router: Router<R>) -> Self {
        Self::bare(Routing::Router(Box::new(WidgetsApp::router(router))))
    }

    /// A Cupertino app with no navigator: `builder` receives `None` and
    /// supplies the whole subtree — see `WidgetsApp::with_builder`. The
    /// [`CupertinoTheme`] still wraps it.
    #[must_use]
    pub fn with_builder(
        builder: impl Fn(&dyn BuildContext, Option<BoxedView>) -> BoxedView + 'static,
    ) -> Self {
        Self {
            builder: Some(Rc::new(builder)),
            ..Self::bare(Routing::Builder)
        }
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
        self.builder = Some(Rc::new(builder));
        self
    }

    /// Force this locale — see `WidgetsApp::locale` for the
    /// resolved-against-supported contract.
    #[must_use]
    pub fn locale(mut self, locale: Locale) -> Self {
        self.locale = Some(locale);
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
        debug_assert!(
            !locales.is_empty(),
            "BUG: supported_locales requires at least one locale \
             (the oracle asserts supportedLocales.isNotEmpty)"
        );
        self.supported_locales = locales;
        self
    }

    /// Delegates producing this application's localized resources — see
    /// `WidgetsApp::localizations_delegates` for the ordering contract.
    #[must_use]
    pub fn localizations_delegates(mut self, delegates: Vec<BoxedLocalizationsDelegate>) -> Self {
        self.localizations_delegates = delegates;
        self
    }
}

impl StatelessView for CupertinoApp {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        // The theme (or the default) is resolved HERE, in this element's own context, so the
        // MediaQuery dependency registered by dynamic-color resolution
        // re-runs this build when the ambient platform brightness changes.
        let effective = self.theme.clone().unwrap_or_default().resolve_from(ctx);

        // The resolved theme's text style is the application-level text style.
        let text_style = effective.text_theme().text_style();

        let app = match &self.routing {
            Routing::Home(home) => self
                .configure(WidgetsApp::new(home.clone()), text_style)
                .boxed(),
            Routing::Router(app) => self.configure((**app).clone(), text_style).boxed(),
            Routing::Builder => {
                let builder = self
                    .builder
                    .clone()
                    .expect("BUG: CupertinoApp::with_builder always sets a builder");
                // `configure` installs the builder again; the shell needs one
                // to start from.
                self.configure(
                    WidgetsApp::with_builder(move |ctx, child| builder(ctx, child)),
                    text_style,
                )
                .boxed()
            }
        };

        // The published theme and the page transition sit ABOVE the
        // WidgetsApp, so its navigator and a Router's read them.
        let app = PageTransitions::new(app).page(Rc::new(ParallaxPageTransition::new()));
        CupertinoTheme::new(effective, app)
    }
}

impl CupertinoApp {
    /// The shell configuration every routing form shares.
    fn configure(&self, app: WidgetsApp, text_style: flui_sdk::painting::TextStyle) -> WidgetsApp {
        let app = match &self.builder {
            Some(builder) => {
                let builder = Rc::clone(builder);
                app.builder(move |ctx, child| builder(ctx, child))
            }
            None => app,
        };
        let app = app
            .supported_locales(self.supported_locales.clone())
            .localizations_delegates(self.localizations_delegates.clone())
            .text_style(text_style);
        match &self.locale {
            Some(locale) => app.locale(locale.clone()),
            None => app,
        }
    }
}

/// What a [`CupertinoApp`] routes with.
#[derive(Clone)]
enum Routing {
    /// [`CupertinoApp::new`]: one home page.
    Home(BoxedView),
    /// [`CupertinoApp::router`]: the router shell, already built.
    Router(Box<WidgetsApp>),
    /// [`CupertinoApp::with_builder`]: no routing.
    Builder,
}

impl fmt::Debug for Routing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Home(_) => "Home",
            Self::Router(_) => "Router",
            Self::Builder => "Builder",
        })
    }
}
