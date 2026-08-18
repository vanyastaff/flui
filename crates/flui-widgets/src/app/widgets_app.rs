//! [`WidgetsApp`] — the design-neutral application shell.
//!
//! Flutter parity: `widgets/app.dart` `WidgetsApp` (oracle tag `3.44.0`),
//! scoped to what the widget layer owns in FLUI's realm model (ADR-0042 §5,
//! ADR-0027). An application using `WidgetsApp` with neither `flui-material`
//! nor `flui-cupertino` in its dependency graph gets navigation,
//! localization (including the resolved
//! [`Directionality`](crate::Directionality)), and an
//! app-level builder hook; the design-system shells (`MaterialApp`,
//! `CupertinoApp`) compose this widget and add theme publication on top —
//! this widget knows about neither (ADR-0028).
//!
//! ## What `WidgetsApp` deliberately does NOT own here
//!
//! The oracle's `WidgetsApp` sits under a process-global `WidgetsBinding`;
//! FLUI's realm model (ADR-0027) installs per-presentation infrastructure
//! at the realm root instead, so re-owning any of it here would double it up:
//!
//! - **`MediaQuery`.** The realm publishes the live root `MediaQuery`
//!   (size / device-pixel-ratio / platform-brightness republish). The oracle
//!   agrees: since 3.7 `WidgetsApp` "never introduces its own `MediaQuery`;
//!   the `View` widget takes care of that" (`widgets/app.dart`,
//!   `useInheritedMediaQuery` deprecation notice, oracle tag `3.44.0`).
//! - **Focus root and default traversal.** The realm wraps the root in
//!   [`FocusRoot`](crate::FocusRoot), which installs the standard Tab /
//!   Shift+Tab traversal bindings — the oracle's `WidgetsApp.defaultShortcuts`
//!   / `defaultActions` / `FocusTraversalGroup` band.
//! - **Vsync scope and gesture arena.** Realm-installed
//!   (`VsyncScope`, `GestureArenaScope`).
//!
//! ## Deferred (named gaps, not silent ones)
//!
//! - **`Title` / `color`.** The oracle's `Title` widget forwards the
//!   application label to the platform (`SystemChrome`). FLUI has no
//!   widget-to-window-title capability yet (`PlatformWindow::set_title`
//!   exists, but no `BuildContext` capability reaches it); adding one is a
//!   new frame-capability seam (trigger #22) and lands with its own slice.
//! - **Platform locale plumbing and the locale-resolution callbacks.** The
//!   oracle feeds `platformDispatcher.locales` into resolution and exposes
//!   `localeListResolutionCallback` / `localeResolutionCallback`. FLUI does
//!   not yet deliver preferred locales from the platform, so resolution runs
//!   with no preferred list (resolving to the first supported locale unless
//!   [`locale`](WidgetsApp::locale) is set) and the callbacks are not
//!   exposed — a callback that only ever receives an empty platform list
//!   would be dead API. Both arrive together when the platform layer
//!   delivers locales.
//! - **Named-route table / `Router`.** FLUI's [`Navigator`] is handle-driven
//!   ([`NavigatorHandle`]), not name-driven: there is no `routes` map,
//!   `onGenerateRoute`, or `onUnknownRoute` to port. The deep-link analog is
//!   a pre-seeded handle — see [`WidgetsApp::navigator`].
//! - **Restoration scope, `SharedAppData`, `NavigationNotification`,
//!   shortcut/action overrides, performance overlay, debug banner.** Each
//!   depends on infrastructure FLUI has not built (state restoration,
//!   notification bubbling, tooltip registry, overlay diagnostics).
//!
//! ## Documented divergences
//!
//! - The oracle wraps the navigator in `FocusScope(autofocus: true)`. FLUI's
//!   [`FocusScope`] does not expose an autofocus knob yet, so the scope is
//!   inserted without it; first focus lands on the first `autofocus` node
//!   below (or nothing), rather than the scope claiming focus on mount.
//! - Swapping the [`navigator`](WidgetsApp::navigator) handle on a live
//!   `WidgetsApp` is not supported: the mount-time handle stays active until
//!   the element remounts (the oracle recreates the `Navigator` when
//!   `navigatorKey` changes).
//! - A rebuilt `WidgetsApp` refreshes the home route's content **one frame
//!   later**: the refresh is scheduled through the route entry's rebuild
//!   handle, and a channel-scheduled mark is processed on the next frame
//!   pump. The oracle's `Route.changedExternalState` rebuilds the page in
//!   the same frame; FLUI's one-frame propagation matches every other
//!   channel-scheduled republish in this workspace (the realm's root
//!   `MediaQuery` included).

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use flui_types::platform::Locale;
use flui_types::typography::TextStyle;
use flui_view::BoxedView;
use flui_view::prelude::*;

use crate::FocusScope;
use crate::localization::{
    BoxedLocalizationsDelegate, DefaultWidgetsLocalizationsDelegate, Localizations,
    basic_locale_list_resolution,
};
use crate::navigator::{Navigator, NavigatorHandle, NavigatorObserver, RouteId, SimpleRoute};
use crate::text::DefaultTextStyle;

/// The app-level wrapping hook — the oracle's `TransitionBuilder`
/// (`widgets/app.dart`, oracle tag `3.44.0`): receives the routing subtree
/// (`Some` when the app has a navigator, `None` otherwise) and returns the
/// subtree to mount in its place. Runs below [`Localizations`], so it may
/// read the resolved locale and ambient resources — see
/// [`WidgetsApp::builder`].
pub type AppBuilder = Rc<dyn Fn(&dyn BuildContext, Option<BoxedView>) -> BoxedView>;

/// A design-neutral application shell: navigator, localizations, and the
/// app-level builder hook, with no design system attached.
///
/// Flutter parity: `WidgetsApp` (`widgets/app.dart`, oracle tag `3.44.0`) —
/// see the module docs for what the realm owns instead, what is deferred,
/// and the documented divergences.
///
/// Composition, outermost first (the oracle's `_WidgetsAppState.build`
/// order, restricted to the subset this widget owns):
///
/// 1. [`Localizations`] — the resolved [`Locale`], the caller's delegates
///    followed by the default widgets-localizations delegate, and the
///    resolved [`Directionality`](crate::Directionality).
/// 2. [`DefaultTextStyle`] — only when [`text_style`](Self::text_style) is
///    set.
/// 3. The [`builder`](Self::builder) hook — only when set; receives the
///    routing subtree as its child.
/// 4. [`FocusScope`] > [`Navigator`] — the routing subtree, present when the
///    app has a [`home`](Self::new) or a caller-supplied
///    [`navigator`](Self::navigator) handle.
///
/// # Example
///
/// ```rust,ignore
/// use flui_widgets::WidgetsApp;
///
/// flui::run_app(WidgetsApp::new(MyHome::new()));
/// ```
#[derive(Clone, StatefulView)]
pub struct WidgetsApp {
    /// The route content shown at the bottom of the navigator's stack,
    /// seeded once as the route named `/`.
    home: Option<BoxedView>,
    /// Caller-supplied navigator handle (the oracle's `navigatorKey`).
    navigator: Option<NavigatorHandle>,
    /// Observers registered on the navigator at mount (the oracle's
    /// `navigatorObservers`).
    observers: Vec<Arc<dyn NavigatorObserver>>,
    builder: Option<AppBuilder>,
    /// Explicit locale override (the oracle's `locale`), still resolved
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
            .field("has_builder", &self.builder.is_some())
            .field("locale", &self.locale)
            .field("supported_locales", &self.supported_locales)
            .finish_non_exhaustive()
    }
}

impl WidgetsApp {
    /// An app shell whose navigator shows `home` as its root route.
    ///
    /// `home` is seeded once, at mount, as an instant, unanimated route
    /// named `/` — the navigator's first entry has nothing beneath it to
    /// transition over, matching the oracle's non-animated initial route
    /// (`Navigator.defaultGenerateInitialRoutes`, oracle tag `3.44.0`).
    #[must_use]
    pub fn new(home: impl IntoView) -> Self {
        Self {
            home: Some(home.into_view().boxed()),
            navigator: None,
            observers: Vec::new(),
            builder: None,
            locale: None,
            supported_locales: vec![Locale::en_us()],
            localizations_delegates: Vec::new(),
            text_style: None,
        }
    }

    /// An app shell with no navigator: `builder` receives `None` and
    /// supplies the whole subtree — the oracle's builder-only form
    /// (`WidgetsApp(builder: ...)` with no `home`/`routes`).
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
            builder: Some(Rc::new(builder)),
            locale: None,
            supported_locales: vec![Locale::en_us()],
            localizations_delegates: Vec::new(),
            text_style: None,
        }
    }

    /// Drive the navigator through a caller-owned [`NavigatorHandle`] — the
    /// oracle's `navigatorKey`, as a handle instead of a key: keep a clone
    /// to push and pop from outside the tree.
    ///
    /// A handle that already carries routes wins over `home`: the home route
    /// is seeded only into an empty navigator, so a caller-built back stack
    /// (FLUI's deep-link analog, several `seed_initial` calls) is mounted
    /// as-is. The oracle draws the same line by making `home` redundant when
    /// `onGenerateInitialRoutes` is specified.
    #[must_use]
    pub fn navigator(mut self, handle: NavigatorHandle) -> Self {
        self.navigator = Some(handle);
        self
    }

    /// Register `observer` on the navigator at mount — the oracle's
    /// `navigatorObservers`.
    ///
    /// Requires the app to have a navigator (a `home` or a
    /// [`navigator`](Self::navigator) handle) — the oracle asserts the same
    /// (`navigatorObservers` must stay empty in the builder-only form).
    #[must_use]
    pub fn observer(mut self, observer: Arc<dyn NavigatorObserver>) -> Self {
        self.observers.push(observer);
        self
    }

    /// Wrap the routing subtree — the oracle's `builder`
    /// (`TransitionBuilder`). The closure's context sits **below**
    /// [`Localizations`], so it may read the resolved locale and ambient
    /// resources, exactly like the oracle's `Builder`-wrapped callback.
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
    /// — the oracle's `LocalizationsResolver.locale` runs the explicit value
    /// through `_resolveLocales([locale], supportedLocales)`
    /// (`widgets/localizations.dart`, oracle tag `3.44.0`), so an
    /// unsupported explicit locale resolves to the best-fit supported one
    /// rather than being used verbatim.
    #[must_use]
    pub fn locale(mut self, locale: Locale) -> Self {
        self.locale = Some(locale);
        self
    }

    /// The locales this application supports, best-fit-resolved by
    /// [`basic_locale_list_resolution`]. Defaults to US English, matching
    /// the oracle's `supportedLocales` default.
    ///
    /// # Panics
    ///
    /// An empty list always fails: debug builds panic here, at
    /// construction (the oracle's `assert(supportedLocales.isNotEmpty)`);
    /// release builds panic later, during build, when
    /// [`basic_locale_list_resolution`] reads the first supported locale
    /// (the oracle's `StateError` from `supportedLocales.first`).
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

    /// The delegates producing this application's localized resources — the
    /// oracle's `localizationsDelegates`.
    ///
    /// The default widgets-localizations delegate is appended **after**
    /// these, and only the first delegate of a given resource type loads, so
    /// a caller-supplied `WidgetsLocalizations` delegate (for example
    /// `flui-localizations`' global one) overrides the US-English default —
    /// the oracle's `LocalizationsResolver.localizationsDelegates` contract.
    #[must_use]
    pub fn localizations_delegates(mut self, delegates: Vec<BoxedLocalizationsDelegate>) -> Self {
        self.localizations_delegates = delegates;
        self
    }

    /// Provide a [`DefaultTextStyle`] for the whole application — the
    /// oracle's `textStyle`. Absent means no `DefaultTextStyle` is inserted,
    /// matching the oracle.
    #[must_use]
    pub fn text_style(mut self, style: TextStyle) -> Self {
        self.text_style = Some(style);
        self
    }

    /// The oracle's locale resolution (`LocalizationsResolver`), minus the
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
    /// `None` in the builder-only form (no navigator at all — the oracle
    /// builds no `Navigator` when `home`, `routes`, `onGenerateRoute`, and
    /// `onUnknownRoute` are all absent) until an update introduces routing.
    navigator: Option<NavigatorHandle>,
    /// The seeded home route and the live cell its content builder reads —
    /// `did_update_view` writes the current view's `home` into the cell and
    /// marks the route, so a rebuilt `WidgetsApp` shows the new home (the
    /// oracle's builder reads `widget.home` live and
    /// `Route.changedExternalState` rebuilds the page). `None` when home
    /// was not seeded (builder-only, or a pre-seeded handle won).
    home_route: Option<(RouteId, Rc<RefCell<BoxedView>>)>,
    /// Exactly the observers THIS shell registered on the handle, so update
    /// reconciliation and `dispose` remove only its own registrations —
    /// a caller-retained handle must not accumulate stale shell
    /// registrations across remounts (the oracle's `NavigatorState`
    /// detaches its observers in `didUpdateWidget`/`dispose`).
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
    /// updated to a routed configuration — the oracle's `_updateRouting`).
    fn install_navigator(view: &WidgetsApp) -> Self {
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

impl StatefulView for WidgetsApp {
    type State = WidgetsAppState;

    fn create_state(&self) -> Self::State {
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

impl ViewState<WidgetsApp> for WidgetsAppState {
    fn did_update_view(&mut self, _old_view: &WidgetsApp, new_view: &WidgetsApp) {
        match (&self.navigator, &new_view.navigator) {
            // Documented divergence (module docs): the mount-time handle
            // stays active; the oracle recreates the Navigator on a
            // navigatorKey swap.
            (Some(current), Some(incoming)) if !current.is_same(incoming) => {
                tracing::warn!(
                    "WidgetsApp: navigator handle changed after mount; the mount-time handle \
                     stays active — remount the WidgetsApp to swap navigators"
                );
            }
            // A builder-only element updated to a routed configuration
            // gains its navigator now — the oracle's `_updateRouting` runs
            // from `didUpdateWidget` and builds the Navigator on the next
            // build.
            (None, _) if new_view.home.is_some() || new_view.navigator.is_some() => {
                *self = WidgetsAppState::install_navigator(new_view);
                return;
            }
            _ => {}
        }

        // Home refresh — the oracle's builder reads `widget.home` at build
        // time and `NavigatorState.didUpdateWidget` runs
        // `Route.changedExternalState` over every live route
        // unconditionally, so the home route re-renders from the CURRENT
        // view configuration on every update.
        if let (Some((route, cell)), Some(home)) = (&self.home_route, &new_view.home) {
            *cell.borrow_mut() = home.clone();
            if let Some(handle) = &self.navigator {
                handle.mark_route_needs_build(*route);
            }
        }

        // Observer reconciliation, by `Arc` identity — the oracle swaps its
        // effective-observer list in `didUpdateWidget`. Only registrations
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
        // stale callbacks) when a later WidgetsApp mounts over it — the
        // oracle's `NavigatorState.dispose` detaches its observers.
        if let Some(handle) = &self.navigator {
            for observer in self.registered_observers.drain(..) {
                handle.remove_observer(&observer);
            }
        }
    }

    fn build(&self, view: &WidgetsApp, _ctx: &dyn BuildContext) -> impl IntoView {
        // The routing subtree: FocusScope > Navigator, the oracle's
        // `FocusScope(child: Navigator(...))` band (autofocus divergence in
        // the module docs).
        let routing: Option<BoxedView> = self
            .navigator
            .as_ref()
            .map(|handle| FocusScope::new(Navigator::new(handle.clone())).boxed());

        // The oracle's `builder` band: when set, it supplies the subtree
        // (receiving the routing as its child); otherwise routing IS the
        // subtree. Runs from a dedicated child element (`AppBuilderScope`)
        // so its context sits below `Localizations`, like the oracle's
        // `Builder` wrapper.
        let content: BoxedView = match (&view.builder, routing) {
            (Some(builder), routing) => AppBuilderScope {
                builder: Rc::clone(builder),
                child: routing,
            }
            .boxed(),
            (None, Some(routing)) => routing,
            (None, None) => unreachable!(
                "BUG: WidgetsApp has no home, navigator, or builder; \
                 WidgetsApp::new and WidgetsApp::with_builder each guarantee one"
            ),
        };

        // The oracle's `textStyle` band: DefaultTextStyle OUTSIDE the
        // builder result, inserted only when set.
        let content: BoxedView = match &view.text_style {
            Some(style) => DefaultTextStyle::new(style.clone(), content).boxed(),
            None => content,
        };

        // The oracle's application-level `Localizations`: caller delegates
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
/// (notably [`Localizations`]) — the oracle wraps its `builder` callback in
/// a `Builder` for exactly this reason (`widgets/app.dart`, oracle tag
/// `3.44.0`).
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

#[cfg(test)]
mod tests {
    use std::any::TypeId;
    use std::sync::Mutex;

    use flui_types::typography::TextDirection;

    use super::*;
    use crate::localization::{Directionality, LocalizationsDelegate};
    use crate::test_harness::mount;
    use crate::{
        BoxedWidgetsLocalizations, DefaultWidgetsLocalizations, MediaQuery, SizedBox,
        WidgetsLocalizations,
    };

    /// A boxed probe closure a [`Capture`] runs against its live
    /// `BuildContext` during `build()` — the same probe shape the
    /// `localizations` tests use.
    type ReadFn<T> = Arc<dyn Fn(&dyn BuildContext) -> T + Send + Sync>;

    /// Captures whatever a probe closure computes from a live `BuildContext`
    /// during `build()`.
    #[derive(Clone, StatelessView)]
    struct Capture<T: Clone + Send + Sync + 'static> {
        read: ReadFn<T>,
        captured: Arc<Mutex<Option<T>>>,
    }

    impl<T: Clone + Send + Sync + 'static> fmt::Debug for Capture<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("Capture").finish_non_exhaustive()
        }
    }

    impl<T: Clone + Send + Sync + 'static> StatelessView for Capture<T> {
        fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
            *self.captured.lock().expect("test mutex poisoned") = Some((self.read)(ctx));
            SizedBox::shrink()
        }
    }

    fn capture<T: Clone + Send + Sync + 'static>(
        read: impl Fn(&dyn BuildContext) -> T + Send + Sync + 'static,
    ) -> (Capture<T>, Arc<Mutex<Option<T>>>) {
        let captured = Arc::new(Mutex::new(None));
        (
            Capture {
                read: Arc::new(read),
                captured: Arc::clone(&captured),
            },
            captured,
        )
    }

    fn captured_value<T: Clone>(cell: &Arc<Mutex<Option<T>>>) -> Option<T> {
        cell.lock().expect("test mutex poisoned").clone()
    }

    #[test]
    fn home_gets_localizations_and_directionality() {
        // The oracle's WidgetsApp always inserts an application-level
        // Localizations; its resolved WidgetsLocalizations supplies the
        // ambient Directionality. Neither material nor cupertino is linked
        // into this crate's tests — this IS the design-neutral guarantee.
        let (probe, captured) = capture(|ctx| {
            (
                Localizations::maybe_locale_of(ctx),
                Directionality::maybe_of(ctx),
            )
        });
        mount(WidgetsApp::new(probe));
        let (locale, direction) = captured_value(&captured).expect("home must build");
        assert_eq!(
            locale,
            Some(Locale::en_us()),
            "the default supported_locales list is [en-US], the oracle's default"
        );
        assert_eq!(
            direction,
            Some(TextDirection::Ltr),
            "DefaultWidgetsLocalizations resolves LTR"
        );
    }

    #[test]
    fn explicit_locale_is_resolved_against_supported_locales_not_used_verbatim() {
        // The oracle's LocalizationsResolver.locale runs an explicit locale
        // through `_resolveLocales([locale], supportedLocales)` — fr-CA
        // against [en-US, fr-FR] must resolve to fr-FR, not stay fr-CA.
        let (probe, captured) = capture(Localizations::maybe_locale_of);
        mount(
            WidgetsApp::new(probe)
                .locale(Locale::new("fr", Some("CA")))
                .supported_locales(vec![Locale::en_us(), Locale::fr_fr()]),
        );
        assert_eq!(
            captured_value(&captured).expect("home must build"),
            Some(Locale::fr_fr()),
            "an unsupported explicit locale must resolve to the best-fit supported locale"
        );
    }

    #[test]
    fn absent_locale_resolves_to_first_supported() {
        // With no explicit locale and no platform preferred-locale list
        // (unplumbed in FLUI — module docs), basicLocaleListResolution
        // answers with the first supported locale.
        let (probe, captured) = capture(Localizations::maybe_locale_of);
        mount(WidgetsApp::new(probe).supported_locales(vec![Locale::ja_jp(), Locale::en_us()]));
        assert_eq!(
            captured_value(&captured).expect("home must build"),
            Some(Locale::ja_jp()),
        );
    }

    /// A caller-supplied `WidgetsLocalizations` resolving RTL — stands in
    /// for `flui-localizations`' global delegate.
    #[derive(Debug, Clone, Copy)]
    struct RtlWidgetsLocalizations;

    impl WidgetsLocalizations for RtlWidgetsLocalizations {
        fn text_direction(&self) -> TextDirection {
            TextDirection::Rtl
        }

        fn reorder_item_to_start(&self) -> &'static str {
            DefaultWidgetsLocalizations.reorder_item_to_start()
        }
        fn reorder_item_to_end(&self) -> &'static str {
            DefaultWidgetsLocalizations.reorder_item_to_end()
        }
        fn reorder_item_up(&self) -> &'static str {
            DefaultWidgetsLocalizations.reorder_item_up()
        }
        fn reorder_item_down(&self) -> &'static str {
            DefaultWidgetsLocalizations.reorder_item_down()
        }
        fn reorder_item_left(&self) -> &'static str {
            DefaultWidgetsLocalizations.reorder_item_left()
        }
        fn reorder_item_right(&self) -> &'static str {
            DefaultWidgetsLocalizations.reorder_item_right()
        }
        fn copy_button_label(&self) -> &'static str {
            DefaultWidgetsLocalizations.copy_button_label()
        }
        fn cut_button_label(&self) -> &'static str {
            DefaultWidgetsLocalizations.cut_button_label()
        }
        fn paste_button_label(&self) -> &'static str {
            DefaultWidgetsLocalizations.paste_button_label()
        }
        fn select_all_button_label(&self) -> &'static str {
            DefaultWidgetsLocalizations.select_all_button_label()
        }
        fn look_up_button_label(&self) -> &'static str {
            DefaultWidgetsLocalizations.look_up_button_label()
        }
        fn search_web_button_label(&self) -> &'static str {
            DefaultWidgetsLocalizations.search_web_button_label()
        }
        fn share_button_label(&self) -> &'static str {
            DefaultWidgetsLocalizations.share_button_label()
        }
        fn radio_button_unselected_label(&self) -> &'static str {
            DefaultWidgetsLocalizations.radio_button_unselected_label()
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct RtlDelegate;

    impl LocalizationsDelegate for RtlDelegate {
        type Resources = BoxedWidgetsLocalizations;

        fn is_supported(&self, _locale: &Locale) -> bool {
            true
        }

        fn load(&self, _locale: &Locale) -> Self::Resources {
            BoxedWidgetsLocalizations::new(RtlWidgetsLocalizations)
        }
    }

    #[test]
    fn caller_delegate_overrides_the_default_widgets_localizations() {
        // The oracle appends DefaultWidgetsLocalizations.delegate AFTER the
        // caller's delegates, and only the first delegate of a resource type
        // loads — so a caller's WidgetsLocalizations delegate must win.
        let (probe, captured) = capture(Directionality::maybe_of);
        mount(
            WidgetsApp::new(probe)
                .localizations_delegates(vec![BoxedLocalizationsDelegate::new(RtlDelegate)]),
        );
        assert_eq!(
            captured_value(&captured).expect("home must build"),
            Some(TextDirection::Rtl),
            "the caller's delegate must load instead of the appended default"
        );
    }

    #[test]
    fn home_is_seeded_once_as_the_root_route() {
        let handle = NavigatorHandle::new();
        let (probe, captured) = capture(|_ctx| true);
        let app = WidgetsApp::new(probe).navigator(handle.clone());
        let mut harness = mount(app.clone());
        assert!(handle.is_mounted(), "the navigator must be in the tree");
        assert_eq!(
            handle.route_ids().len(),
            1,
            "exactly the home route is seeded"
        );
        assert!(
            !handle.can_pop(),
            "the home route is the bottom of the stack — nothing to pop"
        );
        assert!(
            captured_value(&captured).is_some(),
            "the home content must build inside the navigator"
        );

        // A rebuild of the same app must not re-seed: seeding lives in
        // `create_state`, which runs once per element lifetime.
        harness.swap_root(app);
        assert_eq!(
            handle.route_ids().len(),
            1,
            "a rebuild must not re-seed the home route"
        );
    }

    #[test]
    fn preseeded_navigator_handle_wins_over_home() {
        // FLUI's deep-link analog (module docs): a caller-built initial
        // stack on the supplied handle suppresses home seeding — the
        // oracle's home-is-redundant-with-onGenerateInitialRoutes line.
        let handle = NavigatorHandle::new();
        handle
            .seed_initial(SimpleRoute::<()>::new(|_ctx| SizedBox::shrink().boxed()).named("/deep"));
        let (probe, captured) = capture(|_ctx| true);
        mount(WidgetsApp::new(probe).navigator(handle.clone()));
        assert_eq!(
            handle.route_ids().len(),
            1,
            "the pre-seeded stack must be mounted as-is"
        );
        assert!(
            captured_value(&captured).is_none(),
            "home must not build when the handle already carries routes"
        );
    }

    #[test]
    fn builder_only_app_receives_no_routing_and_supplies_the_subtree() {
        // The oracle builds no Navigator when home/routes/onGenerateRoute/
        // onUnknownRoute are all absent; builder receives a null child.
        let received_none = Arc::new(Mutex::new(None::<bool>));
        let received = Arc::clone(&received_none);
        let (probe, captured) = capture(|_ctx| true);
        mount(WidgetsApp::with_builder(move |_ctx, child| {
            *received.lock().expect("test mutex poisoned") = Some(child.is_none());
            probe.clone().boxed()
        }));
        assert_eq!(
            captured_value(&received_none),
            Some(true),
            "the builder-only form must receive None routing"
        );
        assert!(
            captured_value(&captured).is_some(),
            "the builder's subtree must mount"
        );
    }

    #[test]
    fn builder_receives_the_routing_subtree_and_home_builds_below_it() {
        let (probe, captured) = capture(|_ctx| true);
        let got_routing = Arc::new(Mutex::new(None::<bool>));
        let got = Arc::clone(&got_routing);
        mount(WidgetsApp::new(probe).builder(move |_ctx, child| {
            *got.lock().expect("test mutex poisoned") = Some(child.is_some());
            child.expect("routing must be present when home is set")
        }));
        assert_eq!(
            captured_value(&got_routing),
            Some(true),
            "the builder must receive the routing subtree"
        );
        assert!(
            captured_value(&captured).is_some(),
            "home must build below the builder's wrapper"
        );
    }

    #[test]
    fn builder_context_sits_below_localizations() {
        // The oracle wraps its builder callback in a Builder precisely so
        // the callback's context can read Localizations — pin the same
        // altitude here.
        let seen_locale = Arc::new(Mutex::new(None::<Option<Locale>>));
        let seen = Arc::clone(&seen_locale);
        mount(WidgetsApp::with_builder(move |ctx, _child| {
            *seen.lock().expect("test mutex poisoned") = Some(Localizations::maybe_locale_of(ctx));
            SizedBox::shrink().boxed()
        }));
        assert_eq!(
            captured_value(&seen_locale),
            Some(Some(Locale::en_us())),
            "the builder's context must resolve the app-level Localizations"
        );
    }

    #[test]
    fn text_style_installs_a_default_text_style() {
        let style = TextStyle {
            font_size: Some(41.0),
            ..TextStyle::default()
        };
        let (probe, captured) =
            capture(|ctx| ctx.depend_on::<DefaultTextStyle, _>(|dts| dts.data().clone()));
        mount(WidgetsApp::new(probe).text_style(style.clone()));
        assert_eq!(
            captured_value(&captured).expect("home must build"),
            Some(style),
            "the configured text style must reach descendants"
        );
    }

    #[test]
    fn absent_text_style_installs_no_default_text_style() {
        // The oracle inserts DefaultTextStyle only when textStyle is
        // non-null.
        let (probe, captured) =
            capture(|ctx| ctx.depend_on::<DefaultTextStyle, _>(|dts| dts.data().clone()));
        mount(WidgetsApp::new(probe));
        assert_eq!(
            captured_value(&captured).expect("home must build"),
            None,
            "no DefaultTextStyle band without a configured text style"
        );
    }

    #[test]
    fn widgets_app_does_not_install_a_media_query() {
        // ADR-0042 / realm ownership: the live root MediaQuery is
        // realm-installed; WidgetsApp must not re-own it (the oracle agrees
        // since 3.7 — the View widget owns MediaQuery, not WidgetsApp).
        let (probe, captured) = capture(MediaQuery::maybe_of);
        mount(WidgetsApp::new(probe));
        assert_eq!(
            captured_value(&captured).expect("home must build"),
            None,
            "WidgetsApp must not introduce its own MediaQuery"
        );
    }

    #[test]
    fn a_focus_scope_sits_between_widgets_app_and_the_navigator() {
        // The oracle's routing band is FocusScope(child: Navigator(...)).
        let mut harness = mount(WidgetsApp::new(SizedBox::shrink()));
        let navigators = harness.elements_of_type(TypeId::of::<Navigator>());
        assert_eq!(navigators.len(), 1, "exactly one Navigator element");
        let app_root = harness.root();
        let mut cursor = navigators[0];
        let mut saw_focus_scope = false;
        while let Some(parent) = harness.parent_of(cursor) {
            if harness.view_type_of(parent) == TypeId::of::<FocusScope>() {
                saw_focus_scope = true;
            }
            if parent == app_root {
                break;
            }
            cursor = parent;
        }
        assert!(
            saw_focus_scope,
            "a FocusScope must enclose the Navigator inside the WidgetsApp subtree"
        );
    }

    /// Records the observer lifecycle a [`WidgetsApp`]-registered observer
    /// sees across mounts, updates, and unmounts.
    #[derive(Debug, Default)]
    struct RecordingObserver {
        attaches: Mutex<u32>,
        detaches: Mutex<u32>,
        pushes: Mutex<Vec<RouteId>>,
    }

    impl RecordingObserver {
        fn attaches(&self) -> u32 {
            *self.attaches.lock().expect("test mutex poisoned")
        }

        fn detaches(&self) -> u32 {
            *self.detaches.lock().expect("test mutex poisoned")
        }
    }

    impl NavigatorObserver for RecordingObserver {
        fn did_attach(&self, _navigator: NavigatorHandle) {
            *self.attaches.lock().expect("test mutex poisoned") += 1;
        }

        fn did_detach(&self) {
            *self.detaches.lock().expect("test mutex poisoned") += 1;
        }

        fn did_push(&self, route: RouteId, _previous: Option<RouteId>) {
            self.pushes.lock().expect("test mutex poisoned").push(route);
        }
    }

    #[test]
    fn observers_attach_at_mount_and_see_the_home_route() {
        let observer = Arc::new(RecordingObserver::default());
        mount(WidgetsApp::new(SizedBox::shrink()).observer(observer.clone()));
        assert_eq!(
            observer.attaches(),
            1,
            "the observer must be attached exactly once when the navigator mounts"
        );
        assert_eq!(
            observer.pushes.lock().expect("test mutex poisoned").len(),
            1,
            "the observer must see the seeded home route arrive"
        );
    }

    /// Mounts `child` when `show`, an empty box otherwise — the harness's
    /// documented way to unmount a subtree (`swap_root` is keyed by the
    /// root's `TypeId`, so the root type must stay fixed while a field
    /// toggle unmounts what is below it).
    #[derive(Clone, StatelessView)]
    struct Toggle {
        show: bool,
        child: BoxedView,
    }

    impl fmt::Debug for Toggle {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("Toggle")
                .field("show", &self.show)
                .finish_non_exhaustive()
        }
    }

    impl StatelessView for Toggle {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            if self.show {
                self.child.clone()
            } else {
                SizedBox::shrink().boxed()
            }
        }
    }

    #[test]
    fn rebuilding_with_a_new_home_shows_the_new_home_content() {
        // The oracle's home route builder reads `widget.home` at build time,
        // and `NavigatorState.didUpdateWidget` runs
        // `Route.changedExternalState` over live routes — so rebuilding the
        // app with a different home re-renders the route with the NEW home.
        let handle = NavigatorHandle::new();
        let (probe_a, captured_a) = capture(|_ctx| true);
        let (probe_b, captured_b) = capture(|_ctx| true);
        let mut harness = mount(WidgetsApp::new(probe_a).navigator(handle.clone()));
        assert!(captured_value(&captured_a).is_some(), "first home builds");
        assert!(captured_value(&captured_b).is_none());

        harness.swap_root(WidgetsApp::new(probe_b).navigator(handle.clone()));
        // One extra pump: the refresh is scheduled through the entry's
        // `RebuildHandle` from `did_update_view`, and a channel-scheduled
        // mark lands on the next frame pump — the module-docs divergence
        // from the oracle's same-frame `changedExternalState` rebuild, and
        // the same one-frame propagation every channel-scheduled republish
        // in this crate has (e.g. the realm's root `MediaQuery`).
        harness.tick();
        assert!(
            captured_value(&captured_b).is_some(),
            "the updated home must build on the frame after the app rebuilds"
        );
        assert_eq!(
            handle.route_ids().len(),
            1,
            "refreshing home must rebuild the existing route, not push a new one"
        );
    }

    #[test]
    fn unmount_and_remount_over_a_retained_handle_does_not_duplicate_observers() {
        // A caller-retained handle outlives the shell. Each mount registers
        // the configured observers; dispose must deregister them, or the
        // handle accumulates stale registrations and the observer hears
        // every event N times after N remounts (the oracle's NavigatorState
        // detaches its observers in dispose).
        let handle = NavigatorHandle::new();
        let observer = Arc::new(RecordingObserver::default());
        let app = |handle: &NavigatorHandle, observer: &Arc<RecordingObserver>| {
            WidgetsApp::new(SizedBox::shrink())
                .navigator(handle.clone())
                .observer(observer.clone())
        };

        let mut harness = mount(Toggle {
            show: true,
            child: app(&handle, &observer).boxed(),
        });
        assert_eq!(observer.attaches(), 1, "first mount attaches once");

        harness.swap_root(Toggle {
            show: false,
            child: SizedBox::shrink().boxed(),
        });
        harness.swap_root(Toggle {
            show: true,
            child: app(&handle, &observer).boxed(),
        });
        assert_eq!(
            observer.attaches(),
            2,
            "the remount must attach the observer exactly once more — a duplicate \
             registration left behind by the first mount would attach it twice"
        );
    }

    #[test]
    fn updating_away_an_observer_detaches_it() {
        // The oracle reconciles `navigatorObservers` in `didUpdateWidget`;
        // an observer no longer configured must stop receiving callbacks.
        let handle = NavigatorHandle::new();
        let observer = Arc::new(RecordingObserver::default());
        let mut harness = mount(
            WidgetsApp::new(SizedBox::shrink())
                .navigator(handle.clone())
                .observer(observer.clone()),
        );
        assert_eq!(observer.attaches(), 1);
        assert_eq!(observer.detaches(), 0);

        harness.swap_root(WidgetsApp::new(SizedBox::shrink()).navigator(handle.clone()));
        assert_eq!(
            observer.detaches(),
            1,
            "an observer dropped from the configuration must be detached on update"
        );
    }

    #[test]
    fn a_builder_only_app_updated_to_a_home_configuration_gains_routing() {
        // `with_builder`'s doc promises a navigator can be attached later;
        // the oracle's `_updateRouting` handles the same transition from
        // `didUpdateWidget`. Without the adoption path this update would
        // panic in `build` (no navigator, no builder).
        let (probe, captured) = capture(|_ctx| true);
        let mut harness = mount(WidgetsApp::with_builder(|_ctx, _child| {
            SizedBox::shrink().boxed()
        }));
        assert!(captured_value(&captured).is_none());

        harness.swap_root(WidgetsApp::new(probe));
        assert!(
            captured_value(&captured).is_some(),
            "the routed configuration must mount a navigator and build home"
        );
    }

    #[test]
    // Debug-only: the guard compiles out in release, where `#[should_panic]`
    // would otherwise report "did not panic as expected".
    #[cfg(debug_assertions)]
    #[should_panic(expected = "requires at least one locale")]
    fn empty_supported_locales_panics_at_construction() {
        let _ = WidgetsApp::new(SizedBox::shrink()).supported_locales(Vec::new());
    }
}
