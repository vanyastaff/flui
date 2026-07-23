//! ## Test parity notes
//!
//! Flutter sources:
//! - `packages/flutter/test/widgets/localizations_test.dart` — the
//!   `Localizations` widget's own oracle test suite.
//! - `packages/flutter/test/widgets/app_test.dart` (the `basicLocaleListResolution`
//!   `test(...)` block, line 482) — the default locale-resolution algorithm.
//!
//! Oracle checkout tag `3.44.0`.
//!
//! Oracle tests ported:
//! - `'English translations exist for all WidgetsLocalizations properties'`
//!   (`localizations_test.dart:14`) — every `DefaultWidgetsLocalizations`
//!   string getter is non-empty. `isNotNull` has no Rust equivalent (`&str`
//!   cannot be null); non-empty is the closest meaningful assertion.
//! - `'Localizations.maybeLocaleOf returns null when no localizations exist'`
//!   (`localizations_test.dart:100`).
//! - `basicLocaleListResolution` (`app_test.dart:482`) — all six inline
//!   `expect(...)` cases, ported as one table test per case.
//!
//! Oracle test **not** ported here, with reason (but genuinely covered
//! elsewhere, not skipped):
//! - `'Localizations.localeOf throws when no localizations exist'` (`:82`) —
//!   covered by `locale_of_panics_with_no_localizations_ancestor` and
//!   `of_panic_message_names_the_requested_type` in
//!   `crates/flui-widgets/src/localization/localizations.rs`'s own unit
//!   tests instead of here, because driving the panic needs a
//!   `ViewState::init_state` probe (the one place a panic escapes the
//!   framework's build-error `catch_unwind` boundary — see that module's
//!   test-section doc for the full explanation) rather than a `build()`-time
//!   assertion, so it doesn't fit this file's `pump_widget`-only style.
//! - `'Locale is available when Localizations widget stops deferring frames'`
//!   (`:36`) and `'Locale is sent to engine...'` (`:64`) — both test the
//!   oracle's async delegate-loading path (`RendererBinding.deferFirstFrame`),
//!   which is explicitly out of scope for this sync-only-v1 port (see
//!   `Localizations`'s module docs).
//! - `'set locale semantics'` (`:157`) / `'application level does not set
//!   semantics'` (`:189`) — both test the `Semantics(localeForSubtree: ...)`
//!   wrapper `Localizations` omits in this port (documented divergence, see
//!   `Localizations`'s module docs).
//!
//! Widget → mechanism mapping:
//! - `Localizations` → `Localizations` (`flui_widgets::Localizations`)
//! - `basicLocaleListResolution` → `flui_widgets::basic_locale_list_resolution`
//!   (exhaustively table-tested per branch in
//!   `crates/flui-widgets/src/localization/locale_resolution.rs`'s own unit
//!   tests; this file carries only the oracle's literal inline cases, for a
//!   direct citation trail independent of the self-authored branch tests).
//!
//! The `Directionality` RTL-sign-flip proof lives in
//! `crates/flui-widgets/src/navigator/page_route_tests.rs`
//! (`back_gesture_edge_drag_sign_flips_with_ambient_directionality`) rather
//! than here — it needs the full `Navigator` + real pointer-dispatch harness
//! that module already owns, and duplicating that setup here would just be a
//! second copy of the same fixture.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use flui_types::platform::Locale;
use flui_widgets::prelude::*;
use flui_widgets::{
    BoxedLocalizationsDelegate, DefaultWidgetsLocalizations, DefaultWidgetsLocalizationsDelegate,
    Localizations, SizedBox, basic_locale_list_resolution,
};

use crate::common::lay_out;
use crate::harness;

fn widgets_only_delegates() -> Vec<BoxedLocalizationsDelegate> {
    vec![BoxedLocalizationsDelegate::new(
        DefaultWidgetsLocalizationsDelegate,
    )]
}

// ============================================================================
// 'English translations exist for all WidgetsLocalizations properties'
// (localizations_test.dart:14)
// ============================================================================

/// Every `DefaultWidgetsLocalizations` string getter is non-empty.
///
/// Flutter parity: `localizations_test.dart:14`
/// (`'English translations exist for all WidgetsLocalizations properties'`).
#[test]
fn default_widgets_localizations_every_property_is_non_empty() {
    let l = DefaultWidgetsLocalizations;
    for (name, value) in [
        ("reorder_item_up", l.reorder_item_up()),
        ("reorder_item_down", l.reorder_item_down()),
        ("reorder_item_left", l.reorder_item_left()),
        ("reorder_item_right", l.reorder_item_right()),
        ("reorder_item_to_end", l.reorder_item_to_end()),
        ("reorder_item_to_start", l.reorder_item_to_start()),
        ("search_results_found", l.search_results_found()),
        ("no_results_found", l.no_results_found()),
        ("copy_button_label", l.copy_button_label()),
        ("cut_button_label", l.cut_button_label()),
        ("paste_button_label", l.paste_button_label()),
        ("select_all_button_label", l.select_all_button_label()),
        ("look_up_button_label", l.look_up_button_label()),
        ("search_web_button_label", l.search_web_button_label()),
        ("share_button_label", l.share_button_label()),
        (
            "radio_button_unselected_label",
            l.radio_button_unselected_label(),
        ),
    ] {
        assert!(!value.is_empty(), "{name} must be non-empty");
    }
}

// ============================================================================
// 'Localizations.maybeLocaleOf returns null when no localizations exist'
// (localizations_test.dart:100)
// ============================================================================

/// Captures `Localizations::maybe_locale_of(ctx)` during `build()`. `built`
/// is a separate flag (not `Option<Option<Locale>>`) so "build never ran" and
/// "build ran, `maybe_locale_of` returned `None`" stay distinguishable
/// without nesting `Option`.
#[derive(Clone, StatefulView)]
struct MaybeLocaleOfProbe {
    built: Arc<std::sync::atomic::AtomicBool>,
    captured: Arc<std::sync::Mutex<Option<Locale>>>,
}

struct MaybeLocaleOfProbeState {
    built: Arc<std::sync::atomic::AtomicBool>,
    captured: Arc<std::sync::Mutex<Option<Locale>>>,
}

impl StatefulView for MaybeLocaleOfProbe {
    type State = MaybeLocaleOfProbeState;

    fn create_state(&self) -> Self::State {
        MaybeLocaleOfProbeState {
            built: Arc::clone(&self.built),
            captured: Arc::clone(&self.captured),
        }
    }
}

impl ViewState<MaybeLocaleOfProbe> for MaybeLocaleOfProbeState {
    fn build(&self, _view: &MaybeLocaleOfProbe, ctx: &dyn BuildContext) -> impl IntoView {
        self.built.store(true, Ordering::Relaxed);
        *self.captured.lock().expect("test mutex poisoned") = Localizations::maybe_locale_of(ctx);
        SizedBox::shrink()
    }
}

/// With no `Localizations` ancestor, `Localizations::maybe_locale_of` reports
/// `None` rather than panicking.
///
/// Flutter parity: `localizations_test.dart:100`
/// (`'Localizations.maybeLocaleOf returns null when no localizations exist'`).
#[test]
fn maybe_locale_of_returns_none_without_a_localizations_ancestor() {
    let built = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let captured = Arc::new(std::sync::Mutex::new(None));
    let probe = MaybeLocaleOfProbe {
        built: Arc::clone(&built),
        captured: Arc::clone(&captured),
    };
    let _laid = lay_out(probe, harness::screen());

    assert!(
        built.load(Ordering::Relaxed),
        "the probe must have built at least once"
    );
    assert_eq!(
        captured.lock().expect("test mutex poisoned").clone(),
        None,
        "no Localizations ancestor: maybe_locale_of must report None, not panic"
    );
}

// ============================================================================
// 'basicLocaleListResolution' (app_test.dart:482) — every inline oracle case
// ============================================================================

fn l(language: &str, country: Option<&str>) -> Locale {
    Locale::new(language, country)
}

fn ls(language: &str, script: Option<&str>, country: Option<&str>) -> Locale {
    Locale::with_script(language, country, script)
}

/// Matches exactly for language code. (`app_test.dart:484-490`)
#[test]
fn basic_locale_list_resolution_matches_exactly_for_language_code() {
    let preferred = [l("zh", None), l("un", None), l("en", None)];
    let supported = [l("en", None)];
    assert_eq!(
        basic_locale_list_resolution(Some(&preferred), &supported),
        l("en", None)
    );
}

/// Matches exactly for language code and country code. (`app_test.dart:493-499`)
#[test]
fn basic_locale_list_resolution_matches_exactly_for_language_and_country_code() {
    let preferred = [l("en", None), l("en", Some("US"))];
    let supported = [l("en", Some("US"))];
    assert_eq!(
        basic_locale_list_resolution(Some(&preferred), &supported),
        l("en", Some("US"))
    );
}

/// Matches language+script over language+country. (`app_test.dart:502-513`)
#[test]
fn basic_locale_list_resolution_matches_language_and_script_over_language_and_country() {
    let preferred = [ls("zh", Some("Hant"), Some("HK"))];
    let supported = [ls("zh", None, Some("HK")), ls("zh", Some("Hant"), None)];
    assert_eq!(
        basic_locale_list_resolution(Some(&preferred), &supported),
        ls("zh", Some("Hant"), None)
    );
}

/// Matches exactly for language code, script code and country code.
/// (`app_test.dart:516-527`)
#[test]
fn basic_locale_list_resolution_matches_exactly_for_language_script_and_country_code() {
    let preferred = [ls("zh", None, None), ls("zh", Some("Hant"), Some("TW"))];
    let supported = [ls("zh", Some("Hant"), Some("TW"))];
    assert_eq!(
        basic_locale_list_resolution(Some(&preferred), &supported),
        ls("zh", Some("Hant"), Some("TW"))
    );
}

/// Selects for country code if the language code is not found in the
/// preferred locales list. (`app_test.dart:531-540`)
#[test]
fn basic_locale_list_resolution_selects_country_code_when_language_code_is_not_found() {
    let preferred = [ls("en", None, None), ls("ar", None, Some("tn"))];
    let supported = [ls("fr", None, Some("tn"))];
    assert_eq!(
        basic_locale_list_resolution(Some(&preferred), &supported),
        ls("fr", None, Some("tn"))
    );
}

/// Selects first (default) locale when no match at all is found.
/// (`app_test.dart:543-549`)
#[test]
fn basic_locale_list_resolution_selects_first_supported_locale_when_no_match_is_found() {
    let preferred = [l("tn", None)];
    let supported = [l("zh", None), l("un", None), l("en", None)];
    assert_eq!(
        basic_locale_list_resolution(Some(&preferred), &supported),
        l("zh", None)
    );
}

// ============================================================================
// Scope propagation — locale change rebuilds dependents
// ============================================================================

/// Captures `Localizations::locale_of(ctx)` on every `build()` and counts how
/// many times `build()` ran.
#[derive(Clone, StatefulView)]
struct LocaleOfProbe {
    build_count: Arc<AtomicU32>,
    captured: Arc<std::sync::Mutex<Option<Locale>>>,
}

struct LocaleOfProbeState {
    build_count: Arc<AtomicU32>,
    captured: Arc<std::sync::Mutex<Option<Locale>>>,
}

impl StatefulView for LocaleOfProbe {
    type State = LocaleOfProbeState;

    fn create_state(&self) -> Self::State {
        LocaleOfProbeState {
            build_count: Arc::clone(&self.build_count),
            captured: Arc::clone(&self.captured),
        }
    }
}

impl ViewState<LocaleOfProbe> for LocaleOfProbeState {
    fn build(&self, _view: &LocaleOfProbe, ctx: &dyn BuildContext) -> impl IntoView {
        self.build_count.fetch_add(1, Ordering::Relaxed);
        *self.captured.lock().expect("test mutex poisoned") = Some(Localizations::locale_of(ctx));
        SizedBox::shrink()
    }
}

/// Swapping a mounted `Localizations`'s locale rebuilds a descendant that
/// depends on it (via `Localizations::locale_of`), and the descendant's next
/// build observes the *new* locale — not a stale one.
///
/// This is a coarse (whole-subtree) rebuild by design — see
/// [`Localizations`]'s module docs on why `update_should_notify` is
/// locale-keyed, not a per-resource diff — so this test asserts the
/// build actually ran again and picked up the new value (a genuinely
/// mutation-honest check: it would fail if `Localizations` silently kept
/// serving the old locale, or if the dependent's `build()` never reran at
/// all), not that the rebuild count is minimal.
///
/// The finer-grained claim — *unrelated* locale values do not spuriously
/// notify — is `scope_update_should_notify_same_locale_is_false` in
/// `localizations.rs`'s own unit tests, which calls
/// `LocalizationsScope::update_should_notify` directly rather than through a
/// full root-swap (a root-swap rebuilds every descendant regardless of any
/// single `InheritedView`'s notify decision, so it cannot isolate that
/// claim on its own).
#[test]
fn locale_change_rebuilds_a_dependent_with_the_new_locale() {
    let build_count = Arc::new(AtomicU32::new(0));
    let captured = Arc::new(std::sync::Mutex::new(None));
    let probe = LocaleOfProbe {
        build_count: Arc::clone(&build_count),
        captured: Arc::clone(&captured),
    };

    let mut laid = lay_out(
        Localizations::new(Locale::en_us(), widgets_only_delegates(), probe.clone()),
        harness::screen(),
    );

    let builds_after_initial = build_count.load(Ordering::Relaxed);
    assert!(builds_after_initial >= 1, "must have built at least once");
    assert_eq!(
        captured.lock().expect("test mutex poisoned").clone(),
        Some(Locale::en_us()),
        "the initial build must observe the mounted locale"
    );

    laid.pump_widget(Localizations::new(
        Locale::fr_fr(),
        widgets_only_delegates(),
        probe,
    ));

    assert!(
        build_count.load(Ordering::Relaxed) > builds_after_initial,
        "changing the locale must rebuild the dependent"
    );
    assert_eq!(
        captured.lock().expect("test mutex poisoned").clone(),
        Some(Locale::fr_fr()),
        "the rebuilt dependent must observe the NEW locale, not a stale one"
    );
}
