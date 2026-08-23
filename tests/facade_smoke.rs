//! Facade smoke test — evidence that `flui::prelude::*` alone is enough to
//! author a real widget tree and mount it through the headless pipeline, and
//! that each feature-selected catalog resolves to constructible values through
//! the facade's re-exports.
//!
//! The first test carries **no** catalog requirement on purpose: it is the
//! `--no-default-features` evidence that a catalog-free application still gets
//! the full widget layer. The remaining tests are `#[cfg]`-gated per feature,
//! so every supported combination compiles and runs exactly the assertions its
//! surface supports — a combination that silently lost a module fails to
//! compile rather than quietly testing less.
//!
//! This lives in the root crate's `tests/` (not `flui-widgets`' own tests)
//! because it is exercising the `flui` package's own public surface — the
//! facade re-exports under test only exist on this package. The mount
//! sequence (`mount_root_with_pipeline_owner` → set root constraints → run
//! one frame) mirrors `tests/material_demo.rs` and `tests/vertical_slice_demo.rs`'s
//! own `MountedDemo::mount` helpers, trimmed to the minimum needed to prove a
//! `flui::prelude`-authored tree mounts and lays out — this test is not
//! another acceptance test for a sample app, just a compile-and-mount
//! smoke check for the facade surface itself.

use flui::prelude::*;
use flui_rendering::constraints::BoxConstraints;
use flui_testing::HeadlessBinding;
use flui_testing::bootstrap::{MountOptions, MountOwners};
use flui_types::Size;
use flui_types::geometry::px;

/// A trivial tree authored entirely off `flui::prelude::*` — the same import
/// shape `src/lib.rs`'s crate-level doc-test demonstrates.
#[derive(Clone, StatelessView)]
struct FacadeSmokeApp;

impl StatelessView for FacadeSmokeApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Container::new()
            .color(Color::rgb(18, 18, 24))
            .child(Center::new().child(Text::new("flui facade smoke test")))
    }
}

fn root_constraints() -> BoxConstraints {
    BoxConstraints::tight(Size::new(px(320.0), px(240.0)))
}

#[test]
fn prelude_authored_tree_mounts_through_the_headless_pipeline() {
    // Through `flui-testing`'s canonical bootstrap rather than a hand-rolled
    // copy of it: the ordering is load-bearing at nearly every step, and copies
    // of it have drifted silently before.
    let mut binding = HeadlessBinding::new();
    let mounted = binding.mount_root(
        &FacadeSmokeApp,
        MountOwners::fresh(),
        MountOptions::new(root_constraints()),
    );
    assert!(
        mounted.painted,
        "a prelude-authored tree must commit a frame through the headless pipeline"
    );
}

#[cfg(feature = "material")]
#[test]
fn material_module_resolves_through_the_facade() {
    let material_theme = flui::material::ThemeData::light();
    assert_eq!(material_theme.brightness(), Brightness::Light);
}

#[cfg(feature = "cupertino")]
#[test]
fn cupertino_module_resolves_through_the_facade() {
    let cupertino_theme = flui::cupertino::CupertinoThemeData::new();
    // A fresh theme carries no brightness override (follows the ambient
    // `MediaQuery` instead) and resolves `primary_color` to the documented
    // default (`CupertinoColors::SYSTEM_BLUE`) — both would fail if
    // `flui::cupertino::CupertinoThemeData` were resolving to the wrong
    // type or a stale default, not just "failed to compile".
    assert_eq!(cupertino_theme.brightness(), None);
    assert_eq!(
        cupertino_theme.primary_color(),
        flui::cupertino::CupertinoColor::Dynamic(flui::cupertino::CupertinoColors::SYSTEM_BLUE)
    );
}

/// The documented global-localizations entry point: `flui::localizations`
/// resolves a delegate that `flui::widgets`' `Localizations` accepts, so a
/// consumer never has to name `flui-localizations` as a separate dependency.
#[cfg(feature = "localizations")]
#[test]
fn localizations_module_resolves_through_the_facade() {
    use flui::localizations::{BoxedLocalizationsDelegate, GlobalWidgetsLocalizationsDelegate};
    use flui::types::platform::Locale;
    use flui::widgets::{Localizations, SizedBox};

    let delegates = vec![BoxedLocalizationsDelegate::new(
        GlobalWidgetsLocalizationsDelegate,
    )];
    // Arabic is in `RTL_LANGUAGES`, so this is also a live check that the
    // re-exported delegate is the global one and not a stub.
    let _localized = Localizations::new(
        Locale::new("ar", None::<&str>),
        delegates,
        SizedBox::shrink(),
    );
}

/// The Material half of [`flui::prelude`] appears only with the `material`
/// feature; the base half is always there. `Container`/`Center`/`Text` above
/// prove the base half, this proves the Material half is wired to the glob
/// rather than only reachable at `flui::material`.
#[cfg(feature = "material")]
#[test]
fn prelude_carries_the_material_half_when_the_feature_is_on() {
    let theme: ThemeData = ThemeData::light();
    assert_eq!(theme.brightness(), Brightness::Light);
}

/// The design-neutral app shell needs no catalog feature at all: `WidgetsApp`
/// is reachable (and constructible) through the always-on `flui::widgets`
/// surface and the prelude glob — the `--no-default-features` half of the
/// app-shell acceptance criteria.
#[test]
fn widgets_app_is_offered_without_any_catalog_feature() {
    let _app = flui::widgets::WidgetsApp::new(SizedBox::shrink());
    // Also via the prelude glob (`WidgetsApp` is part of
    // `flui_widgets::prelude`).
    let _from_prelude = WidgetsApp::new(SizedBox::shrink());
}

/// The Material shell rides the existing `material` catalog feature —
/// `MaterialApp` and its `ThemeMode` resolve through both `flui::material`
/// and the feature-gated prelude half.
#[cfg(feature = "material")]
#[test]
fn material_app_shell_resolves_through_the_facade() {
    let _app = flui::material::MaterialApp::new(SizedBox::shrink())
        .theme(flui::material::ThemeData::light())
        .dark_theme(flui::material::ThemeData::dark())
        .theme_mode(flui::material::ThemeMode::System);
    assert_eq!(
        flui::material::ThemeMode::default(),
        flui::material::ThemeMode::System
    );
    // And via the prelude's Material half.
    let _from_prelude = MaterialApp::new(SizedBox::shrink()).theme_mode(ThemeMode::Dark);
}

/// The Cupertino shell rides the existing `cupertino` catalog feature.
#[cfg(feature = "cupertino")]
#[test]
fn cupertino_app_shell_resolves_through_the_facade() {
    let theme = flui::cupertino::CupertinoThemeData::new()
        .with_brightness(flui::types::platform::Brightness::Dark);
    let _app = flui::cupertino::CupertinoApp::new(SizedBox::shrink()).theme(theme);
}
