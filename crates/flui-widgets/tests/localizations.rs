//! [`Localizations`] lookups against a mounted tree. The scope's notification
//! predicate, construction guards and root panic paths stay unit tests in
//! `src/localization/localizations.rs`.

use std::fmt;
use std::sync::{Arc, Mutex};

use flui_painting::typography::TextDirection;
use flui_platform_api::Locale;
use flui_view::prelude::*;
use flui_widgets::SizedBox;
use flui_widgets::localization::{
    BoxedLocalizationsDelegate, DefaultWidgetsLocalizationsDelegate, Directionality,
    GlobalWidgetsLocalizationsDelegate, Localizations, LocalizationsDelegate,
};

use crate::common::harness::mount;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Marker(u32);

#[derive(Debug, Clone, Copy, Default)]
struct MarkerDelegate;

impl LocalizationsDelegate for MarkerDelegate {
    type Resources = Marker;

    fn is_supported(&self, _locale: &Locale) -> bool {
        true
    }

    fn load(&self, _locale: &Locale) -> Self::Resources {
        Marker(7)
    }
}

fn widgets_only_delegates() -> Vec<BoxedLocalizationsDelegate> {
    vec![BoxedLocalizationsDelegate::new(
        DefaultWidgetsLocalizationsDelegate,
    )]
}

fn widgets_and_marker_delegates() -> Vec<BoxedLocalizationsDelegate> {
    vec![
        BoxedLocalizationsDelegate::new(DefaultWidgetsLocalizationsDelegate),
        BoxedLocalizationsDelegate::new(MarkerDelegate),
    ]
}

// `Localizations::of`/`locale_of`'s no-ancestor panic path is
// deliberately not exercised via `mount` + `#[should_panic]`: a panic
// inside `build()` is caught by the framework's build-error boundary
// (an `ErrorView` is substituted) rather than unwinding out to the test
// — the same limitation `tests/theme.rs` documents for `Theme::of`. The
// success and `None` paths below are what a mounted tree can actually
// observe; the root panic paths are unit tests beside the source.

/// A boxed probe closure a [`Capture`] runs against its live
/// `BuildContext` during `build()`.
type ReadFn<T> = Arc<dyn Fn(&dyn BuildContext) -> T + Send + Sync>;

/// Captures whatever a probe closure computes from a live `BuildContext`
/// during `build()`, once. Generic over the captured type so each test
/// below states only what it reads, not a bespoke probe widget.
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

#[test]
fn locale_of_resolves_the_mounted_locale() {
    let (probe, captured) = capture(Localizations::locale_of);
    let _harness =
        mount(Localizations::new(Locale::en_us(), widgets_only_delegates(), probe).boxed());
    assert_eq!(
        captured.lock().expect("test mutex poisoned").clone(),
        Some(Locale::en_us())
    );
}

#[test]
fn of_returns_a_delegate_provided_resource() {
    let (probe, captured) = capture(Localizations::maybe_of::<Marker>);
    let _harness =
        mount(Localizations::new(Locale::en_us(), widgets_and_marker_delegates(), probe).boxed());
    assert_eq!(
        captured
            .lock()
            .expect("test mutex poisoned")
            .clone()
            .flatten()
            .as_deref()
            .copied(),
        Some(Marker(7))
    );
}

/// The global delegate is what makes a right-to-left locale's subtree
/// right-to-left: `Localizations` wraps its child in the `Directionality`
/// its widgets resource names, and `GlobalWidgetsLocalizationsDelegate`
/// resolves `ar` to RTL where the default delegate is always LTR.
#[test]
fn the_global_delegate_makes_an_rtl_locale_subtree_rtl() {
    let (probe, captured) = capture(Directionality::of);
    let _harness = mount(
        Localizations::new(
            Locale::new("ar", None::<&str>),
            vec![BoxedLocalizationsDelegate::new(
                GlobalWidgetsLocalizationsDelegate,
            )],
            probe,
        )
        .boxed(),
    );
    assert_eq!(
        captured.lock().expect("test mutex poisoned").clone(),
        Some(TextDirection::Rtl)
    );
}
