//! [`WidgetsApp`] against a mounted tree: the ambient scopes it installs, its
//! localization and locale resolution, the routed and builder configurations,
//! and observer bookkeeping across remounts.

use std::fmt;
use std::sync::{Arc, Mutex};

use flui_view::prelude::*;
use flui_widgets::SizedBox;
use flui_widgets::app::WidgetsApp;
use flui_widgets::navigator::{NavigatorHandle, NavigatorObserver, RouteId};

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

pub(crate) fn locale_override_removal_uses_the_nearest_current_preferences() {
    use flui_platform_api::Locale;
    use flui_widgets::localization::Localizations;
    use flui_widgets::{MediaQuery, MediaQueryData};

    let arabic = Locale::new("ar", None::<&str>).expect("valid Arabic locale");
    let (probe, captured) = capture(Localizations::locale_of);
    let tree = |preferred: Option<Vec<Locale>>, explicit: Option<Locale>| {
        let app =
            WidgetsApp::new(probe.clone()).supported_locales(vec![Locale::en_us(), arabic.clone()]);
        let app = match explicit {
            Some(locale) => app.locale(locale),
            None => app,
        };
        MediaQuery::new(
            MediaQueryData {
                preferred_locales: Some(vec![arabic.clone()].into()),
                ..MediaQueryData::default()
            },
            MediaQuery::new(
                MediaQueryData {
                    preferred_locales: preferred.map(Into::into),
                    ..MediaQueryData::default()
                },
                app,
            ),
        )
    };
    let mut harness = mount(tree(Some(vec![arabic.clone()]), Some(Locale::en_us())));
    assert_eq!(captured_value(&captured), Some(Locale::en_us()));
    for (preferred, explicit, expected) in [
        (
            Some(vec![Locale::en_us()]),
            Some(Locale::en_us()),
            Locale::en_us(),
        ),
        (
            Some(vec![arabic.clone()]),
            Some(Locale::en_us()),
            Locale::en_us(),
        ),
        (Some(vec![arabic.clone()]), None, arabic.clone()),
        (Some(vec![Locale::en_us()]), None, Locale::en_us()),
        (None, None, Locale::en_us()),
    ] {
        harness.swap_root(tree(preferred, explicit));
        assert_eq!(
            captured_value(&captured),
            Some(expected),
            "removing an override reads the latest nearest provider; unknown does not fall through to an outer provider"
        );
    }
}

pub(crate) fn home_is_seeded_once_as_the_root_route() {
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

pub(crate) fn builder_only_app_receives_no_routing_and_supplies_the_subtree() {
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

pub(crate) fn observers_attach_at_mount_and_see_the_home_route() {
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
