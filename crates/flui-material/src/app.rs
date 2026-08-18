//! [`MaterialApp`] and [`ThemeMode`] — the Material application shell.
//!
//! Flutter parity: `material/app.dart` `MaterialApp` / `ThemeMode` (oracle
//! tag `3.44.0`), composed over `flui-widgets`' design-neutral `WidgetsApp`
//! exactly as the oracle composes it (ADR-0042 §5, ADR-0028): `WidgetsApp`
//! knows nothing about this widget; this widget adds theme *selection*
//! ([`ThemeMode`] resolved against the ambient
//! `MediaQueryData::platform_brightness`) and theme *publication* (the
//! resolved [`ThemeData`] through the inherited [`Theme`]), per presentation
//! — two windows resolving different brightness get different themes with
//! no process-global anything (ADR-0027, ADR-0042 §1).
//!
//! ## Composition
//!
//! `MaterialApp` hands `WidgetsApp` a builder (the oracle's
//! `_materialBuilder`) that wraps the routing subtree in, outermost first:
//!
//! 1. [`Theme`] — publishing the [`ThemeData`] resolved by
//!    [`ThemeMode`] × ambient platform brightness (`_themeBuilder`).
//! 2. [`ScaffoldMessenger`] — so `Scaffold`s anywhere below share one
//!    snack-bar rail.
//! 3. The caller's [`builder`](MaterialApp::builder) hook, run from its own
//!    child element so its context resolves the `Theme` published in step 1
//!    (the oracle's builder-inside-a-`Builder` trick, `material/app.dart`'s
//!    "Why are we surrounding a builder with a builder?").
//!
//! ## Deferred (named gaps, not silent ones)
//!
//! - **High-contrast themes** (`highContrastTheme` / `highContrastDarkTheme`)
//!   — FLUI's `MediaQueryData` has no `high_contrast` field yet; the two
//!   slots land with it.
//! - **`AnimatedTheme`** — a theme change is a jump cut, not the oracle's
//!   200ms animated lerp; `themeAnimationDuration`/`Curve`/`Style` defer
//!   with it (`flui-material`'s `AGENTS.md` already names lerp as deferred).
//! - **`DefaultSelectionStyle`**, **`ScrollConfiguration`** /
//!   `MaterialScrollBehavior` — neither widget exists in FLUI yet.
//! - **Material hero motion** — the oracle installs a `HeroControllerScope`
//!   whose controller flies heroes along `MaterialRectArcTween`; FLUI has no
//!   arc tween yet, and the `Navigator`'s auto-installed default controller
//!   already provides (linear) flights, so no scope is inserted here.
//! - **`MaterialLocalizations`** — FLUI ships no Material resource set yet;
//!   when one lands its delegate is appended here the way the oracle appends
//!   `DefaultMaterialLocalizations.delegate`. Caller delegates pass through
//!   to `WidgetsApp` unchanged.
//! - **`Title`/`color`, named routes/`Router`, restoration, debug overlays**
//!   — deferred at the `WidgetsApp` layer; see its module docs.
//!
//! ## Documented divergences
//!
//! - With **no `MediaQuery` ancestor** the oracle's
//!   `MediaQuery.platformBrightnessOf` throws; FLUI resolves
//!   [`ThemeMode::System`] against `MediaQueryData::default()`'s brightness
//!   (light) instead. Under `run_app` the realm always installs the live
//!   root `MediaQuery`, so this path is reachable only from embedders and
//!   harnesses that bypass it — and a panic inside `build` would surface as
//!   a silently-childless subtree (the framework's build-error boundary),
//!   which hides more than the fallback does.

use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use flui_types::Color;
use flui_types::platform::{Brightness, Locale};
use flui_types::typography::{FontWeight, TextStyle};
use flui_view::BoxedView;
use flui_view::prelude::*;
use flui_widgets::{
    AppBuilder, BoxedLocalizationsDelegate, MediaQuery, NavigatorHandle, NavigatorObserver,
    SizedBox, WidgetsApp,
};

use crate::scaffold_messenger::ScaffoldMessenger;
use crate::theme::Theme;
use crate::theme_data::ThemeData;

/// Which theme a [`MaterialApp`] uses. Flutter parity: `ThemeMode`
/// (`material/app.dart`, oracle tag `3.44.0`).
///
/// Lives in `flui-material`, next to the [`ThemeData`] it selects between —
/// theme *selection* belongs to the design system that defines the tokens,
/// not the application framework (ADR-0042 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    /// Follow the platform's light/dark preference, read from the ambient
    /// `MediaQueryData::platform_brightness`.
    #[default]
    System,
    /// Always use [`MaterialApp::theme`], regardless of the platform signal.
    Light,
    /// Always use [`MaterialApp::dark_theme`] (falling back to
    /// [`MaterialApp::theme`] when no dark theme is provided — the oracle's
    /// `darkTheme != null` guard).
    Dark,
}

impl ThemeMode {
    /// Whether this is [`ThemeMode::System`]. Flutter parity: `isSystem`.
    #[must_use]
    pub fn is_system(self) -> bool {
        self == Self::System
    }

    /// Whether this is [`ThemeMode::Light`]. Flutter parity: `isLight`.
    #[must_use]
    pub fn is_light(self) -> bool {
        self == Self::Light
    }

    /// Whether this is [`ThemeMode::Dark`]. Flutter parity: `isDark`.
    #[must_use]
    pub fn is_dark(self) -> bool {
        self == Self::Dark
    }
}

/// The oracle's `_errorTextStyle`: the deliberately ugly fallback style for
/// text rendered outside a `Material`/`DefaultTextStyle`, passed to
/// `WidgetsApp` as the application-level text style. The oracle's underline
/// decoration is dropped — FLUI's [`TextStyle`] carries no decoration
/// fields yet.
fn error_text_style() -> TextStyle {
    TextStyle {
        color: Some(Color::from_argb(0xD0FF_0000)),
        font_family: Some("monospace".to_owned()),
        font_size: Some(48.0),
        font_weight: Some(FontWeight::W900),
        ..TextStyle::default()
    }
}

/// The oracle's `_themeBuilder` (`material/app.dart`, oracle tag `3.44.0`),
/// minus the high-contrast branches (see the module docs): pick dark when
/// the mode says dark, or when the mode is system and the ambient platform
/// brightness is dark; otherwise (or when no dark theme is provided) fall
/// back to `theme`, and finally to `ThemeData::default()` — the M3 light
/// baseline, the same fallback as the oracle's bare `ThemeData()`.
fn resolve_theme(
    mode: ThemeMode,
    theme: Option<&ThemeData>,
    dark_theme: Option<&ThemeData>,
    ctx: &dyn BuildContext,
) -> ThemeData {
    let platform_brightness = MediaQuery::maybe_of(ctx).map_or_else(
        // Documented divergence (module docs): the oracle throws here.
        || flui_widgets::MediaQueryData::default().platform_brightness,
        |data| data.platform_brightness,
    );
    let use_dark = mode.is_dark() || (mode.is_system() && platform_brightness == Brightness::Dark);
    if use_dark && let Some(dark) = dark_theme {
        return dark.clone();
    }
    theme.cloned().unwrap_or_default()
}

/// A Material application shell: [`WidgetsApp`] plus [`ThemeMode`]
/// resolution against the ambient platform brightness and publication of
/// the resolved [`ThemeData`] through [`Theme`], with a
/// [`ScaffoldMessenger`] installed above the routing subtree.
///
/// Flutter parity: `MaterialApp` (`material/app.dart`, oracle tag
/// `3.44.0`) — see the module docs for composition order, named deferrals,
/// and divergences.
///
/// # Example
///
/// ```rust,ignore
/// use flui_material::{MaterialApp, ThemeData, ThemeMode};
///
/// flui::run_app(
///     MaterialApp::new(MyHome::new())
///         .theme(ThemeData::light())
///         .dark_theme(ThemeData::dark())
///         .theme_mode(ThemeMode::System),
/// );
/// ```
#[derive(Clone, StatelessView)]
pub struct MaterialApp {
    home: Option<BoxedView>,
    navigator: Option<NavigatorHandle>,
    observers: Vec<Arc<dyn NavigatorObserver>>,
    user_builder: Option<AppBuilder>,
    theme: Option<ThemeData>,
    dark_theme: Option<ThemeData>,
    theme_mode: ThemeMode,
    locale: Option<Locale>,
    supported_locales: Vec<Locale>,
    localizations_delegates: Vec<BoxedLocalizationsDelegate>,
}

impl fmt::Debug for MaterialApp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MaterialApp")
            .field("has_home", &self.home.is_some())
            .field("has_builder", &self.user_builder.is_some())
            .field("theme_mode", &self.theme_mode)
            .field("has_dark_theme", &self.dark_theme.is_some())
            .finish_non_exhaustive()
    }
}

impl MaterialApp {
    fn bare() -> Self {
        Self {
            home: None,
            navigator: None,
            observers: Vec::new(),
            user_builder: None,
            theme: None,
            dark_theme: None,
            theme_mode: ThemeMode::System,
            locale: None,
            supported_locales: vec![Locale::en_us()],
            localizations_delegates: Vec::new(),
        }
    }

    /// A Material app whose navigator shows `home` as its root route — see
    /// `WidgetsApp::new` for the seeding contract.
    #[must_use]
    pub fn new(home: impl IntoView) -> Self {
        Self {
            home: Some(home.into_view().boxed()),
            ..Self::bare()
        }
    }

    /// A Material app with no navigator: `builder` receives `None` and
    /// supplies the whole subtree — see `WidgetsApp::with_builder`. The
    /// Material bands ([`Theme`], [`ScaffoldMessenger`]) still wrap it.
    #[must_use]
    pub fn with_builder(
        builder: impl Fn(&dyn BuildContext, Option<BoxedView>) -> BoxedView + 'static,
    ) -> Self {
        Self {
            user_builder: Some(Rc::new(builder)),
            ..Self::bare()
        }
    }

    /// The light (or only) theme — the oracle's `theme`. Defaults to
    /// [`ThemeData::default`] (the M3 light baseline) when absent.
    #[must_use]
    pub fn theme(mut self, theme: ThemeData) -> Self {
        self.theme = Some(theme);
        self
    }

    /// The theme used when dark is selected — the oracle's `darkTheme`.
    /// Without one, dark selection falls back to [`theme`](Self::theme)
    /// (never an automatic dark derivation — the oracle's behavior).
    #[must_use]
    pub fn dark_theme(mut self, theme: ThemeData) -> Self {
        self.dark_theme = Some(theme);
        self
    }

    /// How [`theme`](Self::theme) vs [`dark_theme`](Self::dark_theme) is
    /// chosen — the oracle's `themeMode`, default [`ThemeMode::System`].
    #[must_use]
    pub fn theme_mode(mut self, mode: ThemeMode) -> Self {
        self.theme_mode = mode;
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

    /// Wrap the routing subtree — the oracle's `builder`. Runs **below**
    /// the published [`Theme`] and [`ScaffoldMessenger`], so `Theme::of`
    /// inside it resolves the theme this app just selected (the oracle's
    /// double-`Builder` contract).
    #[must_use]
    pub fn builder(
        mut self,
        builder: impl Fn(&dyn BuildContext, Option<BoxedView>) -> BoxedView + 'static,
    ) -> Self {
        self.user_builder = Some(Rc::new(builder));
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

impl StatelessView for MaterialApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        // The oracle's `_materialBuilder`, handed to `WidgetsApp` as its
        // builder: resolves the theme against the ambient MediaQuery *at the
        // builder's own altitude* (below `Localizations`, above routing) and
        // wraps the routing subtree in the Material bands. `MediaQuery::
        // maybe_of` registers a dependency from the builder's element, so a
        // platform-brightness republish re-runs this closure and re-resolves
        // `ThemeMode::System` — the live path the realm's root MediaQuery
        // feeds.
        let theme = self.theme.clone();
        let dark_theme = self.dark_theme.clone();
        let mode = self.theme_mode;
        let user_builder = self.user_builder.clone();
        let material_builder = move |ctx: &dyn BuildContext, child: Option<BoxedView>| {
            let resolved = resolve_theme(mode, theme.as_ref(), dark_theme.as_ref(), ctx);
            let inner: BoxedView = match &user_builder {
                // The double-Builder trick: the caller's builder runs from
                // its own element BELOW the Theme published here, so
                // `Theme::of` inside it resolves this resolved theme.
                Some(builder) => MaterialBuilderScope {
                    builder: Rc::clone(builder),
                    child,
                }
                .boxed(),
                None => child.unwrap_or_else(|| SizedBox::shrink().boxed()),
            };
            Theme::new(resolved, ScaffoldMessenger::new(inner)).boxed()
        };

        let mut app = match &self.home {
            Some(home) => WidgetsApp::new(home.clone()).builder(material_builder),
            None => WidgetsApp::with_builder(material_builder),
        };
        app = app
            .supported_locales(self.supported_locales.clone())
            .localizations_delegates(self.localizations_delegates.clone())
            .text_style(error_text_style());
        if let Some(locale) = &self.locale {
            app = app.locale(locale.clone());
        }
        if let Some(handle) = &self.navigator {
            app = app.navigator(handle.clone());
        }
        for observer in &self.observers {
            app = app.observer(Arc::clone(observer));
        }
        app
    }
}

/// Runs the caller's builder hook from its own element, below the [`Theme`]
/// and [`ScaffoldMessenger`] bands — the oracle's builder-inside-a-`Builder`
/// ("Why are we surrounding a builder with a builder?", `material/app.dart`,
/// oracle tag `3.44.0`): without an element boundary between them,
/// `Theme::of` inside the caller's builder could not see the theme published
/// above it.
#[derive(Clone, StatelessView)]
struct MaterialBuilderScope {
    builder: AppBuilder,
    child: Option<BoxedView>,
}

impl fmt::Debug for MaterialBuilderScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MaterialBuilderScope")
            .field("has_child", &self.child.is_some())
            .finish_non_exhaustive()
    }
}

impl StatelessView for MaterialBuilderScope {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        (self.builder)(ctx, self.child.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_mode_defaults_to_system() {
        assert_eq!(ThemeMode::default(), ThemeMode::System);
        let app = MaterialApp::new(SizedBox::shrink());
        assert_eq!(app.theme_mode, ThemeMode::System);
    }

    #[test]
    fn theme_mode_predicates_match_the_oracle_getters() {
        assert!(ThemeMode::System.is_system());
        assert!(!ThemeMode::System.is_light());
        assert!(ThemeMode::Light.is_light());
        assert!(!ThemeMode::Light.is_dark());
        assert!(ThemeMode::Dark.is_dark());
        assert!(!ThemeMode::Dark.is_system());
    }

    #[test]
    // Debug-only: the guard compiles out in release, where `#[should_panic]`
    // would otherwise report "did not panic as expected" (release still
    // panics, but later, during build — see the setter's doc).
    #[cfg(debug_assertions)]
    #[should_panic(expected = "requires at least one locale")]
    fn empty_supported_locales_panics_at_construction() {
        let _ = MaterialApp::new(SizedBox::shrink()).supported_locales(Vec::new());
    }

    #[test]
    fn error_text_style_matches_the_oracle_subset() {
        let style = error_text_style();
        assert_eq!(style.color, Some(Color::from_argb(0xD0FF_0000)));
        assert_eq!(style.font_family.as_deref(), Some("monospace"));
        assert_eq!(style.font_size, Some(48.0));
        assert_eq!(style.font_weight, Some(FontWeight::W900));
    }
}
