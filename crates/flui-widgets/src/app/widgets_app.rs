//! [`WidgetsApp`] — the design-neutral application shell.
//!
//! Scoped to what the widget layer owns in FLUI's UI runtime model (ADR-0042 §5,
//! ADR-0027). An application using `WidgetsApp` with neither `flui-material`
//! nor `flui-cupertino` in its dependency graph gets navigation (a typed
//! [`Router`] through [`WidgetsApp::router`], or a [`Navigator`]),
//! localization (including the resolved
//! [`Directionality`](crate::Directionality)), and an
//! app-level builder hook; the design-system shells (`MaterialApp`,
//! `CupertinoApp`) compose this widget and add theme publication on top —
//! this widget knows about neither (ADR-0028).
//!
//! ## What `WidgetsApp` deliberately does NOT own here
//!
//! FLUI's UI runtime model (ADR-0027) installs per-presentation infrastructure
//! at the UI runtime root, so re-owning any of it here would double it up:
//!
//! - **`MediaQuery`.** The UI runtime publishes the live root `MediaQuery`
//!   (size / device-pixel-ratio / platform-brightness republish); the shell
//!   never introduces its own.
//! - **Focus root and default traversal.** The UI runtime wraps the root in
//!   [`FocusRoot`](crate::FocusRoot), which installs the standard Tab /
//!   Shift+Tab traversal bindings.
//! - **Vsync scope and gesture arena.** Runtime-installed
//!   (`VsyncScope`, `GestureArenaScope`).
//!
//! ## Deferred (named gaps, not silent ones)
//!
//! - **`Title` / `color`.** Forwarding the application label to the
//!   platform needs a widget-to-window-title capability. FLUI has none yet (`PlatformWindow::set_title`
//!   exists, but no `BuildContext` capability reaches it); adding one is a
//!   new `LifecycleContext` capability and a change of its own.
//! - **Platform locale plumbing and the locale-resolution callbacks.** FLUI
//!   does not yet deliver preferred locales from the platform, so resolution
//!   runs with no preferred list (resolving to the first supported locale
//!   unless [`locale`](WidgetsApp::locale) is set) and resolution callbacks
//!   are not exposed — a callback that only ever receives an empty platform list
//!   would be dead API. Both arrive together when the platform layer
//!   delivers locales.
//! - **Named-route table, at the *app* level.** The mechanism
//!   itself exists — [`route`](NavigatorHandle::route),
//!   [`on_generate_route`](NavigatorHandle::on_generate_route) and
//!   [`on_unknown_route`](NavigatorHandle::on_unknown_route) carry the
//!   `routes` map and its two hooks, and
//!   [`push_named`](NavigatorHandle::push_named) and its five siblings drive
//!   them (ADR-0024). What this shell does not yet do is *forward* them: there
//!   is no `WidgetsApp::routes(..)` folding `home` + a table + a user generator
//!   into one hook, so an app
//!   registers on the handle it hands to [`WidgetsApp::navigator`]. When that
//!   forwarding lands, the reconciliation contract is already fixed: the app
//!   builder replaces the table wholesale at mount, and the handle mutators
//!   serve imperative or late registration (`ARCHITECTURE.md`,
//!   `## Mapping decisions`). New code roots the app in a typed
//!   [`Router`] with [`WidgetsApp::router`] instead (ADR-0093); the
//!   navigator form stays for code that has not moved.
//! - **Restoration scope, `SharedAppData`, `NavigationNotification`,
//!   shortcut/action overrides, performance overlay, debug banner.** Each
//!   depends on infrastructure FLUI has not built (state restoration,
//!   notification bubbling, tooltip registry, overlay diagnostics).
//!
//! ## Documented limits
//!
//! - The navigator is wrapped in a [`FocusScope`], which does not expose an
//!   autofocus knob yet, so the scope is inserted without autofocus; first focus lands on the first `autofocus` node
//!   below (or nothing), rather than the scope claiming focus on mount.
//! - Swapping the [`navigator`](WidgetsApp::navigator) handle on a live
//!   `WidgetsApp` is not supported: the mount-time handle stays active until
//!   the element remounts.
//! - A rebuilt `WidgetsApp` refreshes the home route's content **one frame
//!   later**: the refresh is scheduled through the route entry's rebuild
//!   handle, and a channel-scheduled mark is processed on the next frame
//!   pump. The one-frame propagation matches every other
//!   channel-scheduled republish in this workspace (the UI runtime's root
//!   `MediaQuery` included).

use std::cell::RefCell;
use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;

use flui_painting::typography::TextStyle;
use flui_platform_api::Locale;
use flui_view::BoxedView;
use flui_view::prelude::*;

use crate::FocusScope;
use crate::localization::{
    BoxedLocalizationsDelegate, DefaultWidgetsLocalizationsDelegate, Localizations,
    basic_locale_list_resolution,
};
use crate::navigator::{Navigator, NavigatorHandle, NavigatorObserver, RouteId, SimpleRoute};
use crate::router::{Routable, Router};
use crate::text::DefaultTextStyle;

/// The app-level wrapping hook: receives the routing subtree
/// (`Some` when the app has a router or a navigator, `None` otherwise) and returns the
/// subtree to mount in its place. Runs below [`Localizations`], so it may
/// read the resolved locale and ambient resources — see
/// [`WidgetsApp::builder`].
pub type AppBuilder = Rc<dyn Fn(&dyn BuildContext, Option<BoxedView>) -> BoxedView>;

/// A design-neutral application shell: navigator, localizations, and the
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
///    own (each of its pages has one); otherwise [`FocusScope`] > [`Navigator`], present
///    when the app has a [`home`](Self::new) or a caller-supplied
///    [`navigator`](Self::navigator) handle.
///
/// The form is a type parameter: [`WidgetsApp::router`] returns a
/// `WidgetsApp<RouterForm>`, which has no [`navigator`](Self::navigator) or
/// [`observer`](Self::observer) builder, because its Router owns its
/// navigator. Every other builder serves both forms.
///
/// # Example
///
/// ```rust,ignore
/// use flui_widgets::WidgetsApp;
///
/// flui::run_app(WidgetsApp::new(MyHome::new()));
/// ```
#[derive(Clone, StatefulView)]
pub struct WidgetsApp<F: AppForm = NavigatorForm> {
    /// The route content shown at the bottom of the navigator's stack,
    /// seeded once as the route named `/`.
    home: Option<BoxedView>,
    /// Caller-supplied navigator handle.
    navigator: Option<NavigatorHandle>,
    /// Observers registered on the navigator at mount.
    observers: Vec<Arc<dyn NavigatorObserver>>,
    /// The boxed [`Router`] of the router form: the app's whole routing
    /// subtree, in place of `home` and `navigator`.
    router: Option<BoxedView>,
    builder: Option<AppBuilder>,
    /// Explicit locale override, still resolved
    /// against [`supported_locales`](Self::supported_locales).
    locale: Option<Locale>,
    supported_locales: Vec<Locale>,
    localizations_delegates: Vec<BoxedLocalizationsDelegate>,
    text_style: Option<TextStyle>,
    form: PhantomData<F>,
}

mod sealed {
    pub trait Sealed {}
}

/// Which routing an app shell has: [`NavigatorForm`] or [`RouterForm`].
/// Sealed; the two forms are the only ones.
pub trait AppForm: sealed::Sealed + Clone + fmt::Debug + 'static {}

/// The form of [`WidgetsApp::new`] and [`WidgetsApp::with_builder`]: a
/// [`Navigator`] the app owns or is handed, or none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NavigatorForm;

/// The form of [`WidgetsApp::router`]: a typed [`Router`], whose navigator
/// is the app's only one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RouterForm;

impl sealed::Sealed for NavigatorForm {}
impl sealed::Sealed for RouterForm {}
impl AppForm for NavigatorForm {}
impl AppForm for RouterForm {}

impl<F: AppForm> fmt::Debug for WidgetsApp<F> {
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

impl WidgetsApp<NavigatorForm> {
    /// An app shell whose navigator shows `home` as its root route.
    ///
    /// `home` is seeded once, at mount, as an instant, unanimated route
    /// named `/` — the navigator's first entry has nothing beneath it to
    /// transition over.
    #[must_use]
    pub fn new(home: impl IntoView) -> Self {
        Self {
            home: Some(home.into_view().boxed()),
            navigator: None,
            observers: Vec::new(),
            router: None,
            builder: None,
            locale: None,
            supported_locales: vec![Locale::en_us()],
            localizations_delegates: Vec::new(),
            text_style: None,
            form: PhantomData,
        }
    }

    /// An app shell with no navigator: `builder` receives `None` and
    /// supplies the whole subtree.
    ///
    /// Attach a navigator later with [`navigator`](Self::navigator) (the
    /// builder then receives the routing subtree as `Some`).
    #[must_use]
    pub fn with_builder(
        builder: impl Fn(&dyn BuildContext, Option<BoxedView>) -> BoxedView + 'static,
    ) -> Self {
        Self {
            home: None,
            navigator: None,
            observers: Vec::new(),
            router: None,
            builder: Some(Rc::new(builder)),
            locale: None,
            supported_locales: vec![Locale::en_us()],
            localizations_delegates: Vec::new(),
            text_style: None,
            form: PhantomData,
        }
    }

    /// Drive the navigator through a caller-owned [`NavigatorHandle`]: keep a clone
    /// to push and pop from outside the tree.
    ///
    /// A handle that already carries routes wins over `home`: the home route
    /// is seeded only into an empty navigator, so a caller-built back stack
    /// (FLUI's deep-link analog, several `seed_initial` calls) is mounted
    /// as-is.
    #[must_use]
    pub fn navigator(mut self, handle: NavigatorHandle) -> Self {
        self.navigator = Some(handle);
        self
    }

    /// Register `observer` on the navigator at mount.
    ///
    /// Requires the app to have a navigator (a `home` or a
    /// [`navigator`](Self::navigator) handle); observers must stay empty in the
    /// builder-only form.
    #[must_use]
    pub fn observer(mut self, observer: Arc<dyn NavigatorObserver>) -> Self {
        self.observers.push(observer);
        self
    }
}

impl WidgetsApp<RouterForm> {
    /// An app shell rooted in `router`: the [`Router`] is the app's routing
    /// subtree, below [`Localizations`] and the [`builder`](Self::builder)
    /// hook, and its navigator is the app's only one (ADR-0093 §2).
    ///
    /// The result is a `WidgetsApp<RouterForm>`, which has no
    /// [`navigator`](WidgetsApp::navigator) handle and no
    /// [`observer`](WidgetsApp::observer): the Router owns its navigator, so
    /// either would be dead configuration, so here the call does not compile.
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
    /// let app = WidgetsApp::router(Router::new(AppRoute::Home, |route: &AppRoute, _cx| {
    ///     match route {
    ///         AppRoute::Home => Text::new("Home").into_view().boxed(),
    ///         AppRoute::Note { id } => Text::new(format!("Note {id}")).into_view().boxed(),
    ///     }
    /// }));
    /// # let _ = app;
    /// ```
    #[must_use]
    pub fn router<R: Routable>(router: Router<R>) -> Self {
        Self {
            home: None,
            navigator: None,
            observers: Vec::new(),
            router: Some(router.boxed()),
            builder: None,
            locale: None,
            supported_locales: vec![Locale::en_us()],
            localizations_delegates: Vec::new(),
            text_style: None,
            form: PhantomData,
        }
    }
}

impl<F: AppForm> WidgetsApp<F> {
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

    /// Locale resolution, minus the
    /// platform preferred-locale list FLUI does not deliver yet (see the
    /// module docs): an explicit locale resolves as a one-element preferred
    /// list; otherwise resolution runs with no preferred list, which
    /// [`basic_locale_list_resolution`] answers with the first supported
    /// locale.
    fn resolve_locale(&self) -> Locale {
        match &self.locale {
            Some(explicit) => basic_locale_list_resolution(
                Some(std::slice::from_ref(explicit)),
                &self.supported_locales,
            ),
            None => basic_locale_list_resolution(None, &self.supported_locales),
        }
    }
}

/// Persistent state for [`WidgetsApp`]: owns the [`NavigatorHandle`], seeds
/// the home route exactly once (re-seeding on every `build` would re-push
/// the home route on every rebuild), keeps that route's content in sync
/// with the current view configuration, and reconciles the observer
/// registrations it made against the handle across updates and disposal.
pub struct WidgetsAppState {
    /// `None` in the builder-only form (no navigator at all) until an update introduces routing.
    ///
    /// Only `home` and a caller-supplied handle give the shell a navigator:
    /// it has no named-route table or route-generator hooks of its own to
    /// consult (the module docs say why), so a handle whose *named* routes
    /// are registered directly must be handed in through
    /// [`WidgetsApp::navigator`] to make this `Some`.
    navigator: Option<NavigatorHandle>,
    /// The seeded home route and the live cell its content builder reads —
    /// `did_update_view` writes the current view's `home` into the cell and
    /// marks the route, so a rebuilt `WidgetsApp` shows the new home.
    /// `None` when home
    /// was not seeded (builder-only, or a pre-seeded handle won).
    home_route: Option<(RouteId, Rc<RefCell<BoxedView>>)>,
    /// Exactly the observers THIS shell registered on the handle, so update
    /// reconciliation and `dispose` remove only its own registrations —
    /// a caller-retained handle must not accumulate stale shell
    /// registrations across remounts.
    registered_observers: Vec<Arc<dyn NavigatorObserver>>,
}

impl fmt::Debug for WidgetsAppState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WidgetsAppState")
            .field("has_navigator", &self.navigator.is_some())
            .field("has_home_route", &self.home_route.is_some())
            .field("registered_observers", &self.registered_observers.len())
            .finish()
    }
}

impl WidgetsAppState {
    /// Adopt a navigator for `view`: choose the handle, seed home into an
    /// empty one, and register the configured observers. Shared by
    /// `create_state` (mount) and `did_update_view` (a builder-only element
    /// updated to a routed configuration).
    fn install_navigator<F: AppForm>(view: &WidgetsApp<F>) -> Self {
        let handle = view.navigator.clone().unwrap_or_default();
        let mut home_route = None;
        if let Some(home) = &view.home {
            if handle.route_ids().is_empty() {
                let cell = Rc::new(RefCell::new(home.clone()));
                let reader = Rc::clone(&cell);
                handle.seed_initial(
                    SimpleRoute::<()>::new(move |_ctx| reader.borrow().clone()).named("/"),
                );
                let id = handle
                    .current()
                    .expect("BUG: seed_initial must leave the home route current");
                home_route = Some((id, cell));
            } else {
                tracing::debug!(
                    "WidgetsApp: navigator handle already seeded; home not mounted \
                     (the caller-built initial stack wins)"
                );
            }
        }
        let mut registered_observers = Vec::with_capacity(view.observers.len());
        for observer in &view.observers {
            handle.add_observer(Arc::clone(observer));
            registered_observers.push(Arc::clone(observer));
        }
        WidgetsAppState {
            navigator: Some(handle),
            home_route,
            registered_observers,
        }
    }
}

impl<F: AppForm> StatefulView for WidgetsApp<F> {
    type State = WidgetsAppState;

    fn create_state(&self) -> Self::State {
        // The router form: only `WidgetsApp::router` sets it, and a
        // `WidgetsApp<RouterForm>` has no navigator builders.
        if self.router.is_some() {
            return WidgetsAppState {
                navigator: None,
                home_route: None,
                registered_observers: Vec::new(),
            };
        }
        if self.home.is_some() || self.navigator.is_some() {
            WidgetsAppState::install_navigator(self)
        } else {
            debug_assert!(
                self.observers.is_empty(),
                "BUG: WidgetsApp observers require a navigator — give the app a home or a \
                 navigator handle (the oracle asserts navigatorObservers is empty in the \
                 builder-only form)"
            );
            WidgetsAppState {
                navigator: None,
                home_route: None,
                registered_observers: Vec::new(),
            }
        }
    }
}

impl<F: AppForm> ViewState<WidgetsApp<F>> for WidgetsAppState {
    fn did_update_view(&mut self, _old_view: &WidgetsApp<F>, new_view: &WidgetsApp<F>) {
        // A switch between the navigator and router forms changes the view
        // type, so it remounts this element (disposing the navigator form's
        // state) rather than reaching here; the router form has no navigator
        // of its own, so everything below is a no-op for it.
        match (&self.navigator, &new_view.navigator) {
            // Documented limit (module docs): the mount-time handle
            // stays active.
            (Some(current), Some(incoming)) if !current.is_same(incoming) => {
                tracing::warn!(
                    "WidgetsApp: navigator handle changed after mount; the mount-time handle \
                     stays active — remount the WidgetsApp to swap navigators"
                );
            }
            // A builder-only element updated to a routed configuration
            // gains its navigator now.
            (None, _) if new_view.home.is_some() || new_view.navigator.is_some() => {
                *self = WidgetsAppState::install_navigator(new_view);
                return;
            }
            _ => {}
        }

        // Home refresh — the home route re-renders from the CURRENT view
        // configuration on every update.
        if let (Some((route, cell)), Some(home)) = (&self.home_route, &new_view.home) {
            let _prev = std::mem::replace(&mut *cell.borrow_mut(), home.clone());
            if let Some(handle) = &self.navigator {
                handle.mark_route_needs_build(*route);
            }
        }

        // Observer reconciliation, by `Arc` identity. Only registrations
        // this shell made are touched; observers the caller attached to the
        // handle directly are not this widget's to remove.
        if let Some(handle) = &self.navigator {
            self.registered_observers.retain(|registered| {
                let keep = new_view
                    .observers
                    .iter()
                    .any(|observer| Arc::ptr_eq(observer, registered));
                if !keep {
                    handle.remove_observer(registered);
                }
                keep
            });
            for observer in &new_view.observers {
                if !self
                    .registered_observers
                    .iter()
                    .any(|registered| Arc::ptr_eq(registered, observer))
                {
                    handle.add_observer(Arc::clone(observer));
                    self.registered_observers.push(Arc::clone(observer));
                }
            }
        }
    }

    fn dispose(&mut self) {
        // Deregister exactly what this shell registered, so a
        // caller-retained handle sees no duplicate attachments (and no
        // stale callbacks) when a later WidgetsApp mounts over it.
        if let Some(handle) = &self.navigator {
            for observer in self.registered_observers.drain(..) {
                handle.remove_observer(&observer);
            }
        }
    }

    fn build(&self, view: &WidgetsApp<F>, _ctx: &dyn BuildContext) -> impl IntoView {
        // The routing subtree: FocusScope > Navigator (autofocus limit in
        // the module docs).
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
                "BUG: WidgetsApp has no home, router, navigator, or builder; \
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

        // The application-level `Localizations`: caller delegates
        // first, the default widgets delegate appended (first-of-type wins).
        let mut delegates = view.localizations_delegates.clone();
        delegates.push(BoxedLocalizationsDelegate::new(
            DefaultWidgetsLocalizationsDelegate,
        ));
        Localizations::new(view.resolve_locale(), delegates, content)
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
