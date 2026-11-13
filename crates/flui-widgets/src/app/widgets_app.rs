//! [`WidgetsApp`] — the design-neutral application shell.
//!
//! Scoped to what the widget layer owns in FLUI's UI runtime model (ADR-0042 §5,
//! ADR-0027). An application using `WidgetsApp` with neither `flui-material`
//! nor `flui-cupertino` in its dependency graph gets navigation (a typed
//! [`Router`] through [`WidgetsApp::router`], or one page through
//! [`WidgetsApp::new`] on which popups open), localization (including the
//! resolved [`Directionality`](crate::Directionality)), and an app-level
//! builder hook; the design-system shells (`MaterialApp`, `CupertinoApp`)
//! compose this widget and add theme publication on top — this widget knows
//! about neither (ADR-0028).
//!
//! ## What `WidgetsApp` deliberately does NOT own here
//!
//! FLUI's UI runtime model (ADR-0027) installs per-presentation infrastructure
//! at the UI runtime root, so re-owning any of it here would double it up:
//!
//! - **`MediaQuery`.** The UI runtime publishes the live root `MediaQuery`
//!   (size / device-pixel-ratio / platform-brightness republish); the shell
//!   never introduces its own.
//! - **Focus root and default traversal.** The UI runtime roots the focus
//!   tree ([`FocusTreeRoot`](flui_view::environment::FocusTreeRoot)), and the
//!   composition root wraps the application root in
//!   [`app_root`](crate::app_root), which installs the standard Tab /
//!   Shift+Tab traversal bindings.
//! - **Vsync scope and gesture arena.** Runtime-installed
//!   (`VsyncScope`, `GestureArenaScope`).
//!
//! ## Deferred (named gaps, not silent ones)
//!
//! - **`Title` / `color`.** Forwarding the application label to the
//!   platform is not done by this shell. The capability exists:
//!   `LifecycleContext::window()` hands a lifecycle hook a `WindowHandle`
//!   whose `set_title` queues `WindowCommand::SetTitle`; what is missing is a
//!   widget that calls it from the shell's `title`.
//! - **Custom locale-resolution callbacks.** Resolution uses
//!   [`basic_locale_list_resolution`] against the nearest `MediaQuery`'s
//!   preferred languages, unless [`locale`](WidgetsApp::locale) is set.
//!   An unavailable host observation falls back to the first supported locale.
//!   Native observation availability depends on the backend.
//! - **Routes by name, a navigator handle, navigator observers.** There is
//!   no `routes` table, `onGenerateRoute`, `navigatorKey` or
//!   `navigatorObservers`: an application that navigates roots itself in a
//!   typed [`Router`] with [`WidgetsApp::router`], whose route values carry
//!   their paths, and a page reaches its route through
//!   [`PageHandle`](crate::PageHandle) (ADR-0330 §4).
//! - **Restoration scope, `SharedAppData`, `NavigationNotification`,
//!   shortcut/action overrides, performance overlay, debug banner.** Each
//!   depends on infrastructure FLUI has not built (state restoration,
//!   notification bubbling, tooltip registry, overlay diagnostics).
//!
//! ## Documented limits
//!
//! - The home page is wrapped in a [`FocusScope`], which does not expose an
//!   autofocus knob yet, so the scope is inserted without autofocus; first focus lands on the first `autofocus` node
//!   below (or nothing), rather than the scope claiming focus on mount.
//! - A rebuilt `WidgetsApp` refreshes the home route's content **one frame
//!   later**: the refresh is scheduled through the route entry's rebuild
//!   handle, and a channel-scheduled mark is processed on the next frame
//!   pump. The one-frame propagation matches every other
//!   channel-scheduled republish in this workspace (the UI runtime's root
//!   `MediaQuery` included).

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use flui_foundation::BuildPhase;
use flui_painting::typography::TextStyle;
use flui_platform::Locale;
use flui_view::BoxedView;
use flui_view::prelude::*;

use crate::localization::{
    BoxedLocalizationsDelegate, DefaultWidgetsLocalizationsDelegate, Localizations,
    basic_locale_list_resolution,
};
use crate::navigator::{Navigator, NavigatorHandle, PageRoute, RouteId};
use crate::router::{Routable, Router};
use crate::text::DefaultTextStyle;
use crate::{DefaultSelectionControls, FocusScope, PlainSelectionMenu};
use flui_view::environment::{MediaQuery, MediaQueryData};

/// The app-level wrapping hook: receives the routing subtree
/// (`Some` when the app has a router or a home page, `None` otherwise) and returns the
/// subtree to mount in its place. Runs below [`Localizations`], so it may
/// read the resolved locale and ambient resources — see
/// [`WidgetsApp::builder`].
pub type AppBuilder = Rc<dyn Fn(&dyn BuildContext, Option<BoxedView>) -> BoxedView>;

/// A design-neutral application shell: routing, localizations, and the
/// app-level builder hook, with no design system attached.
///
/// See the module docs for what the UI runtime owns, what is deferred, and the
/// documented limits.
///
/// Composition, outermost first:
///
/// 1. [`Localizations`] — the resolved [`Locale`], the caller's delegates
///    followed by the default widgets-localizations delegate, and the
///    resolved [`Directionality`](crate::Directionality).
/// 2. [`DefaultTextStyle`] — only when [`text_style`](Self::text_style) is
///    set.
/// 3. The [`builder`](Self::builder) hook — only when set; receives the
///    routing subtree as its child.
/// 4. The routing subtree: the [`Router`] itself in the
///    [`router`](WidgetsApp::router) form, with no focus scope of its
///    own (each of its pages has one); otherwise, with a
///    [`home`](Self::new), a focus scope over the one page the app has,
///    on which popups open.
///
/// # Example
///
/// ```rust,ignore
/// use flui_widgets::WidgetsApp;
///
/// flui::run_app(WidgetsApp::new(MyHome::new()))?;
/// ```
#[derive(Clone, StatefulView)]
pub struct WidgetsApp {
    /// The one page of an app with no Router, seeded once as the route
    /// named `/`.
    home: Option<BoxedView>,
    /// The boxed [`Router`] of the router form: the app's whole routing
    /// subtree, in place of `home`.
    router: Option<BoxedView>,
    builder: Option<AppBuilder>,
    /// Explicit locale override, still resolved
    /// against [`supported_locales`](Self::supported_locales).
    locale: Option<Locale>,
    supported_locales: Vec<Locale>,
    localizations_delegates: Vec<BoxedLocalizationsDelegate>,
    text_style: Option<TextStyle>,
}

impl fmt::Debug for WidgetsApp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WidgetsApp")
            .field("has_home", &self.home.is_some())
            .field("has_router", &self.router.is_some())
            .field("has_builder", &self.builder.is_some())
            .field("locale", &self.locale)
            .field("supported_locales", &self.supported_locales)
            .finish_non_exhaustive()
    }
}

impl WidgetsApp {
    /// An app shell with one page, `home`, on which popups open: Flutter's
    /// `WidgetsApp(home:)`.
    ///
    /// `home` is seeded once, at mount, as an instant, unanimated route
    /// named `/`. An app with more than one page roots itself in a
    /// [`Router`] with [`router`](Self::router).
    #[must_use]
    pub fn new(home: impl IntoView) -> Self {
        Self::shell(Some(home.into_view().boxed()), None, None)
    }

    /// An app shell with no routing: `builder` receives `None` and supplies
    /// the whole subtree.
    #[must_use]
    pub fn with_builder(
        builder: impl Fn(&dyn BuildContext, Option<BoxedView>) -> BoxedView + 'static,
    ) -> Self {
        Self::shell(None, None, Some(Rc::new(builder)))
    }

    /// An app shell rooted in `router`: the [`Router`] is the app's routing
    /// subtree, below [`Localizations`] and the [`builder`](Self::builder)
    /// hook, and its navigator is the app's only one (ADR-0330 §4).
    ///
    /// ```
    /// use flui_widgets::prelude::*;
    /// use flui_widgets::{Router, Text, WidgetsApp};
    ///
    /// #[derive(Routable, Clone, PartialEq)]
    /// enum AppRoute {
    ///     #[route("/")]
    ///     Home,
    ///     #[route("/note/:id")]
    ///     Note { id: u32 },
    /// }
    ///
    /// let app = WidgetsApp::router(Router::new(AppRoute::Home, |route: &AppRoute, _ctx| {
    ///     match route {
    ///         AppRoute::Home => Text::new("Home").into_view().boxed(),
    ///         AppRoute::Note { id } => Text::new(format!("Note {id}")).into_view().boxed(),
    ///     }
    /// }));
    /// # let _ = app;
    /// ```
    #[must_use]
    pub fn router<R: Routable>(router: Router<R>) -> Self {
        Self::shell(None, Some(router.boxed()), None)
    }

    fn shell(
        home: Option<BoxedView>,
        router: Option<BoxedView>,
        builder: Option<AppBuilder>,
    ) -> Self {
        Self {
            home,
            router,
            builder,
            locale: None,
            supported_locales: vec![Locale::en_us()],
            localizations_delegates: Vec::new(),
            text_style: None,
        }
    }

    /// Wrap the routing subtree. The closure's context sits **below**
    /// [`Localizations`], so it may read the resolved locale and ambient
    /// resources.
    #[must_use]
    pub fn builder(
        mut self,
        builder: impl Fn(&dyn BuildContext, Option<BoxedView>) -> BoxedView + 'static,
    ) -> Self {
        self.builder = Some(Rc::new(builder));
        self
    }

    /// Force this locale instead of resolving from the platform.
    ///
    /// Still resolved against [`supported_locales`](Self::supported_locales)
    /// — the explicit value is resolved as a one-element preferred list, so an
    /// unsupported explicit locale resolves to the best-fit supported one
    /// rather than being used verbatim.
    #[must_use]
    pub fn locale(mut self, locale: Locale) -> Self {
        self.locale = Some(locale);
        self
    }

    /// The locales this application supports, best-fit-resolved by
    /// [`basic_locale_list_resolution`]. Defaults to US English.
    ///
    /// # Panics
    ///
    /// An empty list always fails: debug builds panic here, at
    /// construction; release builds panic later, during build, when
    /// [`basic_locale_list_resolution`] reads the first supported locale.
    #[must_use]
    pub fn supported_locales(mut self, locales: Vec<Locale>) -> Self {
        debug_assert!(
            !locales.is_empty(),
            "BUG: WidgetsApp::supported_locales requires at least one locale \
             (the oracle asserts supportedLocales.isNotEmpty)"
        );
        self.supported_locales = locales;
        self
    }

    /// The delegates producing this application's localized resources.
    ///
    /// The default widgets-localizations delegate is appended **after**
    /// these, and only the first delegate of a given resource type loads, so
    /// a caller-supplied `WidgetsLocalizations` delegate (for example
    /// [`GlobalWidgetsLocalizationsDelegate`](crate::GlobalWidgetsLocalizationsDelegate))
    /// overrides the US-English default.
    #[must_use]
    pub fn localizations_delegates(mut self, delegates: Vec<BoxedLocalizationsDelegate>) -> Self {
        self.localizations_delegates = delegates;
        self
    }

    /// Provide a [`DefaultTextStyle`] for the whole application.
    /// Absent means no `DefaultTextStyle` is inserted.
    #[must_use]
    pub fn text_style(mut self, style: TextStyle) -> Self {
        self.text_style = Some(style);
        self
    }

    /// Resolve the authored override, or subscribe to the nearest inherited
    /// preferred languages. Missing observations use the first supported locale.
    fn resolve_locale(&self, ctx: &dyn BuildContext) -> Locale {
        match &self.locale {
            Some(explicit) => basic_locale_list_resolution(
                Some(std::slice::from_ref(explicit)),
                &self.supported_locales,
            ),
            None => {
                MediaQuery::depend_on_fields(ctx, MediaQueryData::FIELD_PREFERRED_LOCALES, |data| {
                    basic_locale_list_resolution(
                        data.preferred_locales.as_deref(),
                        &self.supported_locales,
                    )
                })
                .unwrap_or_else(|| basic_locale_list_resolution(None, &self.supported_locales))
            }
        }
    }
}

/// Persistent state for [`WidgetsApp`]: owns the home page's navigator,
/// seeds the home route exactly once (re-seeding on every `build` would
/// re-push the home route on every rebuild), and keeps that route's content
/// in sync with the current view configuration.
pub struct WidgetsAppState {
    /// The navigator holding the home page and the popups opened on it;
    /// `None` in the router and builder-only forms.
    navigator: Option<NavigatorHandle>,
    /// The seeded home route and the live cell its content builder reads —
    /// `did_update_view` writes the current view's `home` into the cell and
    /// marks the route, so a rebuilt `WidgetsApp` shows the new home.
    home_route: Option<(RouteId, Rc<RefCell<BoxedView>>)>,
    /// The build phase of the UI runtime the app mounts in, from
    /// `create_state`: the home navigator is bound to it.
    build_phase: BuildPhase,
}

impl fmt::Debug for WidgetsAppState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WidgetsAppState")
            .field("has_navigator", &self.navigator.is_some())
            .field("has_home_route", &self.home_route.is_some())
            .field("building", &self.build_phase.is_building())
            .finish()
    }
}

impl WidgetsAppState {
    /// The state for `view`: a navigator seeded with its home page when it
    /// has one and no Router, otherwise none. Shared by `create_state`
    /// (mount) and `did_update_view` (an element updated to or from the
    /// home form).
    fn for_view(view: &WidgetsApp, build_phase: BuildPhase) -> Self {
        let Some(home) = view.home.as_ref().filter(|_| view.router.is_none()) else {
            return WidgetsAppState {
                navigator: None,
                home_route: None,
                build_phase,
            };
        };
        let handle = NavigatorHandle::new(build_phase.clone());
        let cell = Rc::new(RefCell::new(home.clone()));
        let reader = Rc::clone(&cell);
        // A page route, as the router form's pages are: it owns a focus
        // scope and a `PageScope`, so a menu or a dialog popped off it gives
        // the focus back to where it was on the home page, and the page
        // reaches its route through `PageHandle::of`.
        handle.seed_initial(
            PageRoute::<()>::new(move |_ctx, _animation, _secondary| reader.borrow().clone())
                .named("/"),
        );
        let id = handle
            .current()
            .expect("BUG: seed_initial must leave the home route current");
        WidgetsAppState {
            navigator: Some(handle),
            home_route: Some((id, cell)),
            build_phase,
        }
    }
}

impl StatefulView for WidgetsApp {
    type State = WidgetsAppState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        WidgetsAppState::for_view(self, ctx.build_phase())
    }
}

impl ViewState<WidgetsApp> for WidgetsAppState {
    fn did_update_view(&mut self, _old_view: &WidgetsApp, new_view: &WidgetsApp) {
        let wants_home = new_view.home.is_some() && new_view.router.is_none();
        match (&self.home_route, wants_home) {
            // The home page re-renders from the current view configuration
            // on every update.
            (Some((route, cell)), true) => {
                if let Some(home) = &new_view.home {
                    let _previous = std::mem::replace(&mut *cell.borrow_mut(), home.clone());
                }
                if let Some(handle) = &self.navigator {
                    handle.mark_route_needs_build(*route);
                }
            }
            // An element updated into or out of the home form: the
            // navigator is created or released with it.
            (None, true) | (Some(_), false) => {
                *self = WidgetsAppState::for_view(new_view, self.build_phase.clone());
            }
            (None, false) => {}
        }
    }

    fn build(&self, view: &WidgetsApp, ctx: &dyn BuildContext) -> impl IntoView {
        // The routing subtree: FocusScope > Navigator over the home page
        // (autofocus limit in the module docs).
        // In the router form, the Router alone, whose navigator is the
        // app's only one, with no FocusScope around it (each page has its
        // route scope).
        let routing: Option<BoxedView> = match (&view.router, &self.navigator) {
            (Some(router), _) => Some(router.clone()),
            (None, Some(handle)) => Some(FocusScope::new(Navigator::new(handle.clone())).boxed()),
            (None, None) => None,
        };

        // The `builder` band: when set, it supplies the subtree
        // (receiving the routing as its child); otherwise routing IS the
        // subtree. Runs from a dedicated child element (`AppBuilderScope`)
        // so its context sits below `Localizations`.
        let content: BoxedView = match (&view.builder, routing) {
            (Some(builder), routing) => AppBuilderScope {
                builder: Rc::clone(builder),
                child: routing,
            }
            .boxed(),
            (None, Some(routing)) => routing,
            (None, None) => unreachable!(
                "BUG: WidgetsApp has no home, router or builder; \
                 WidgetsApp::new, WidgetsApp::router and WidgetsApp::with_builder each \
                 guarantee one"
            ),
        };

        // The `text_style` band: DefaultTextStyle OUTSIDE the
        // builder result, inserted only when set.
        let content: BoxedView = match &view.text_style {
            Some(style) => DefaultTextStyle::new(style.clone(), content).boxed(),
            None => content,
        };

        // The plain selection menu, which a design package's
        // `DefaultSelectionControls` beneath replaces.
        let content = DefaultSelectionControls::new(content)
            .menu(PlainSelectionMenu)
            .boxed();

        // The application-level `Localizations`: caller delegates
        // first, the default widgets delegate appended (first-of-type wins).
        let mut delegates = view.localizations_delegates.clone();
        delegates.push(BoxedLocalizationsDelegate::new(
            DefaultWidgetsLocalizationsDelegate,
        ));
        Localizations::new(view.resolve_locale(ctx), delegates, content)
    }
}

/// Runs the [`AppBuilder`] hook from its own element, so the closure's
/// `BuildContext` sits below everything [`WidgetsApp`] wraps around it
/// (notably [`Localizations`]).
#[derive(Clone, StatelessView)]
struct AppBuilderScope {
    builder: AppBuilder,
    child: Option<BoxedView>,
}

impl fmt::Debug for AppBuilderScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppBuilderScope")
            .field("has_child", &self.child.is_some())
            .finish_non_exhaustive()
    }
}

impl StatelessView for AppBuilderScope {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        (self.builder)(ctx, self.child.clone())
    }
}
