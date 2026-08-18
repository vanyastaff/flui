//! [`CupertinoApp`] — the iOS-style application shell.
//!
//! Flutter parity: `cupertino/app.dart` `CupertinoApp` (oracle tag
//! `3.44.0`), composed over `flui-widgets`' design-neutral `WidgetsApp`
//! exactly as the oracle composes it (ADR-0042 §5, ADR-0028): `WidgetsApp`
//! knows nothing about this widget; this widget resolves the
//! [`CupertinoThemeData`] and publishes it through [`CupertinoTheme`],
//! **above** the `WidgetsApp` (the oracle's ordering — unlike `MaterialApp`,
//! which resolves inside the `WidgetsApp` builder).
//!
//! ## Brightness — Apple's model, no `ThemeMode` (ADR-0042 §2)
//!
//! There is deliberately no selection enum here. The theme's optional
//! [`brightness`](CupertinoThemeData::with_brightness) override wins when
//! set; otherwise every dynamic color resolves against the ambient
//! `MediaQueryData::platform_brightness` — the oracle's
//! `effectiveThemeData.brightness ?? MediaQuery.platformBrightnessOf(context)`.
//! Resolution happens in this widget's own `build`
//! (`CupertinoThemeData::resolve_from`, the oracle's `resolveFrom(context)`),
//! which registers a `MediaQuery` dependency — a platform-brightness
//! republish from the realm's root `MediaQuery` rebuilds this element and
//! re-resolves the theme.
//!
//! ## Deferred (named gaps, not silent ones)
//!
//! - **`CupertinoUserInterfaceLevel`** — no elevation ambient exists in FLUI
//!   yet; dynamic colors already resolve elevation as their base variant
//!   (see `colors.rs`).
//! - **`DefaultSelectionStyle`**, **`ScrollConfiguration`** /
//!   `CupertinoScrollBehavior` — neither widget exists in FLUI yet.
//! - **`HeroControllerScope`** — the oracle installs a linear-tween hero
//!   controller; FLUI's `Navigator` auto-installs an equivalent default
//!   controller, so no explicit scope is needed here.
//! - **`CupertinoLocalizations`** — FLUI ships no Cupertino resource set
//!   yet; when one lands its delegate is appended here the way the oracle
//!   appends `DefaultCupertinoLocalizations.delegate`.
//! - **`Title`/`color`, named routes/`Router`, restoration, debug overlays**
//!   — deferred at the `WidgetsApp` layer; see its module docs.

use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use flui_types::platform::Locale;
use flui_view::BoxedView;
use flui_view::prelude::*;
use flui_widgets::{
    AppBuilder, BoxedLocalizationsDelegate, NavigatorHandle, NavigatorObserver, WidgetsApp,
};

use crate::theme::{CupertinoTheme, CupertinoThemeData};

/// An iOS-style application shell: [`WidgetsApp`] under a published,
/// brightness-resolved [`CupertinoTheme`].
///
/// Flutter parity: `CupertinoApp` (`cupertino/app.dart`, oracle tag
/// `3.44.0`) — see the module docs for the brightness model, named
/// deferrals, and ordering.
///
/// # Example
///
/// ```rust,ignore
/// use flui_cupertino::{CupertinoApp, CupertinoThemeData};
/// use flui_types::platform::Brightness;
///
/// flui::run_app(
///     CupertinoApp::new(MyHome::new())
///         // Optional: pin brightness instead of following the platform.
///         .theme(CupertinoThemeData::default().with_brightness(Brightness::Dark)),
/// );
/// ```
#[derive(Clone, StatelessView)]
pub struct CupertinoApp {
    home: Option<BoxedView>,
    navigator: Option<NavigatorHandle>,
    observers: Vec<Arc<dyn NavigatorObserver>>,
    builder: Option<AppBuilder>,
    theme: Option<CupertinoThemeData>,
    locale: Option<Locale>,
    supported_locales: Vec<Locale>,
    localizations_delegates: Vec<BoxedLocalizationsDelegate>,
}

impl fmt::Debug for CupertinoApp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CupertinoApp")
            .field("has_home", &self.home.is_some())
            .field("has_builder", &self.builder.is_some())
            .field("has_theme", &self.theme.is_some())
            .finish_non_exhaustive()
    }
}

impl CupertinoApp {
    fn bare() -> Self {
        Self {
            home: None,
            navigator: None,
            observers: Vec::new(),
            builder: None,
            theme: None,
            locale: None,
            supported_locales: vec![Locale::en_us()],
            localizations_delegates: Vec::new(),
        }
    }

    /// A Cupertino app whose navigator shows `home` as its root route — see
    /// `WidgetsApp::new` for the seeding contract.
    #[must_use]
    pub fn new(home: impl IntoView) -> Self {
        Self {
            home: Some(home.into_view().boxed()),
            ..Self::bare()
        }
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
            ..Self::bare()
        }
    }

    /// The app-wide theme — the oracle's `theme`, default
    /// [`CupertinoThemeData::default`]. Its optional brightness override is
    /// the only brightness knob (no `ThemeMode` — see the module docs).
    #[must_use]
    pub fn theme(mut self, theme: CupertinoThemeData) -> Self {
        self.theme = Some(theme);
        self
    }

    /// Drive the navigator through a caller-owned handle — see
    /// `WidgetsApp::navigator` for the pre-seeded-handle contract.
    #[must_use]
    pub fn navigator(mut self, handle: NavigatorHandle) -> Self {
        self.navigator = Some(handle);
        self
    }

    /// Register `observer` on the navigator at mount — the oracle's
    /// `navigatorObservers`; see `WidgetsApp::observer`.
    #[must_use]
    pub fn observer(mut self, observer: Arc<dyn NavigatorObserver>) -> Self {
        self.observers.push(observer);
        self
    }

    /// Wrap the routing subtree — the oracle's `builder`, passed straight
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
    /// resolution reads the first supported locale (the oracle's own
    /// split — `assert(supportedLocales.isNotEmpty)` in debug, a
    /// `StateError` from `supportedLocales.first` in release).
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
        // The oracle's `build`: `(theme ?? CupertinoThemeData()).resolveFrom
        // (context)` — resolved HERE, in this element's own context, so the
        // MediaQuery dependency registered by dynamic-color resolution
        // re-runs this build when the ambient platform brightness changes.
        let effective = self.theme.clone().unwrap_or_default().resolve_from(ctx);

        // The oracle passes `effectiveThemeData.textTheme.textStyle` as the
        // application-level text style.
        let text_style = effective.text_theme().text_style();

        let pass_through = |builder: &AppBuilder| {
            let shared = Rc::clone(builder);
            move |ctx: &dyn BuildContext, child: Option<BoxedView>| (*shared)(ctx, child)
        };
        let mut app = match (&self.home, &self.builder) {
            (Some(home), Some(builder)) => {
                WidgetsApp::new(home.clone()).builder(pass_through(builder))
            }
            (Some(home), None) => WidgetsApp::new(home.clone()),
            (None, Some(builder)) => WidgetsApp::with_builder(pass_through(builder)),
            (None, None) => {
                // `WidgetsApp` requires a home or a builder; mirror its own
                // construction guarantee at this crate's boundary.
                unreachable!(
                    "BUG: CupertinoApp has neither home nor builder; CupertinoApp::new and \
                     CupertinoApp::with_builder each guarantee one"
                )
            }
        };
        app = app
            .supported_locales(self.supported_locales.clone())
            .localizations_delegates(self.localizations_delegates.clone())
            .text_style(text_style);
        if let Some(locale) = &self.locale {
            app = app.locale(locale.clone());
        }
        if let Some(handle) = &self.navigator {
            app = app.navigator(handle.clone());
        }
        for observer in &self.observers {
            app = app.observer(Arc::clone(observer));
        }

        // The published theme sits ABOVE the WidgetsApp — the oracle's
        // `CupertinoTheme(data: effectiveThemeData, child: ...WidgetsApp)`.
        CupertinoTheme::new(effective, app)
    }
}
