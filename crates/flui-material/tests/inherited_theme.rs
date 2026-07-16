//! Integration tests for [`Theme`] — the Material inherited theming widget.
//!
//! Migrated from `flui-widgets/tests/inherited_app.rs` (the `Theme`-specific
//! subset) when `Theme` moved to this crate; `MediaQuery`'s tests stayed in
//! `flui-widgets`.
//!
//! Each test drives the real layout pipeline through the headless harness and
//! proves that the inherited read works end-to-end: a descendant view
//! captures the data provided by the ancestor into a shared cell during
//! `build()`.
//!
//! ## Correctness invariants tested
//!
//! * `Theme::maybe_of` MUST return the *provided* data, not a default — the
//!   assertions fail if `maybe_of` returns any other value.
//! * `maybe_of` MUST return `None` when no ancestor exists.
//!
//! ## False-pass prevention
//!
//! The capture cell is `Option<Option<ThemeData>>`:
//! * `None` (outer) — `build()` was never invoked.
//! * `Some(None)` — `build()` ran; `maybe_of` found no ancestor.
//! * `Some(Some(data))` — `build()` ran; `maybe_of` returned `data`.
//!
//! "No ancestor" tests first assert `is_some()` on the outer option so they
//! fail loudly if the framework skips `build()` entirely.

// Target-level lint relaxations — crate-level allows don't reach this
// target. `unwrap` in test/example code: a panic IS the failure report
// (docs/PANIC-POLICY.md); style items here are ship-wave debt.
#![allow(clippy::option_option, clippy::unwrap_used)]

mod common;

use std::sync::{Arc, Mutex};

use common::{lay_out, loose};
use flui_material::{ColorSchemeOverrides, Theme, ThemeData};
use flui_types::platform::Brightness;
use flui_types::styling::Color;
use flui_view::prelude::*;
use flui_widgets::SizedBox;

/// Captures whatever [`Theme::maybe_of`] returns during `build()`.
///
/// Outer `None` = `build` not called (framework bug or harness not wired).
/// Inner `None` = `build` ran; no `Theme` ancestor found.
/// Inner `Some(data)` = `build` ran; ancestor data captured.
#[derive(Clone, Debug, StatelessView)]
struct ThemeCapture {
    captured: Arc<Mutex<Option<Option<ThemeData>>>>,
}

impl StatelessView for ThemeCapture {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        *self.captured.lock().unwrap() = Some(Theme::maybe_of(ctx));
        SizedBox::shrink()
    }
}

/// A descendant reads `Theme::maybe_of` inside `build` and the captured value
/// equals the data the ancestor `Theme` provided. The assertion fails if:
/// - `maybe_of` returns `None` (no lookup performed), or
/// - `maybe_of` returns `Some(wrong_data)` (wrong scope resolved).
#[test]
fn theme_of_returns_ancestor_theme_data() {
    let captured: Arc<Mutex<Option<Option<ThemeData>>>> = Arc::new(Mutex::new(None));
    // Sentinel primary color distinct from both presets so the assertion
    // fails if `maybe_of` returns any preset instead of the provided value.
    let sentinel = Color::from_argb(0xFF00_C864);
    let base = ThemeData::dark();
    let scheme = base.color_scheme.copy_with(ColorSchemeOverrides {
        primary: Some(sentinel),
        ..Default::default()
    });
    let provided = base.copy_with(Some(scheme), None);

    let _laid = lay_out(
        Theme::new(
            provided.clone(),
            ThemeCapture {
                captured: Arc::clone(&captured),
            },
        ),
        loose(100.0),
    );

    let outer = captured.lock().unwrap().clone();
    let got = outer
        .expect("ThemeCapture::build was never called — the harness did not traverse the subtree")
        .expect(
            "Theme::maybe_of returned None even though a Theme ancestor was present in the tree",
        );

    assert_eq!(
        got, provided,
        "Theme::maybe_of should return exactly the data provided by the ancestor Theme, \
         not a default or wrong scope"
    );
    assert_eq!(got.brightness(), Brightness::Dark);
    assert_eq!(got.color_scheme.primary, sentinel);
}

/// `Theme::maybe_of` returns `None` when no `Theme` ancestor is present.
/// Proves the lookup is honest, not returning a hidden default.
#[test]
fn theme_maybe_of_returns_none_without_ancestor() {
    let captured: Arc<Mutex<Option<Option<ThemeData>>>> = Arc::new(Mutex::new(None));

    let _laid = lay_out(
        ThemeCapture {
            captured: Arc::clone(&captured),
        },
        loose(100.0),
    );

    let outer = captured.lock().unwrap().clone();
    // If the outer is None, build() was never called — the harness is broken.
    let inner = outer
        .expect("ThemeCapture::build was never called — the harness did not traverse the subtree");

    assert!(
        inner.is_none(),
        "Theme::maybe_of should return None when no Theme ancestor is present, \
         got: {inner:?}"
    );
}

/// `ThemeData::light()` and `ThemeData::dark()` must differ on at least
/// `brightness` and `color_scheme.primary`.
#[test]
fn theme_data_light_and_dark_presets_are_distinct() {
    let light = ThemeData::light();
    let dark = ThemeData::dark();

    assert_ne!(light, dark, "light and dark themes should not be equal");
    assert_eq!(light.brightness(), Brightness::Light);
    assert_eq!(dark.brightness(), Brightness::Dark);
    assert_ne!(
        light.color_scheme.primary, dark.color_scheme.primary,
        "light and dark presets should have different primary colors"
    );
}
