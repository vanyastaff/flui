//! [`WidgetsApp`] against a mounted tree: the ambient scopes it installs, its
//! localization and locale resolution, the routed and builder configurations,
//! and observer bookkeeping across remounts.

use std::any::TypeId;
use std::fmt;
use std::sync::{Arc, Mutex};

use flui_types::platform::Locale;
use flui_types::typography::{TextDirection, TextStyle};
use flui_view::BoxedView;
use flui_view::prelude::*;
use flui_widgets::app::WidgetsApp;
use flui_widgets::interaction::FocusScope;
use flui_widgets::localization::{
    BoxedLocalizationsDelegate, Directionality, Localizations, LocalizationsDelegate,
};
use flui_widgets::navigator::{
    Navigator, NavigatorHandle, NavigatorObserver, RouteId, SimpleRoute,
};
use flui_widgets::text::DefaultTextStyle;
use flui_widgets::{
    BoxedWidgetsLocalizations, DefaultWidgetsLocalizations, MediaQuery, SizedBox,
    WidgetsLocalizations,
};

use crate::common::harness::mount;

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
    handle.seed_initial(SimpleRoute::<()>::new(|_ctx| SizedBox::shrink().boxed()).named("/deep"));
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
    // onUnknownRoute are all absent; builder receives a null child. Of
    // those four, only `home` is a knob this shell has — named-route
    // registration lives on `NavigatorHandle`, and a caller who wants it
    // supplies the handle through `WidgetsApp::navigator`.
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
