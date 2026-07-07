//! Integration test for [`Theme::of`] — the panicking ancestor accessor.
//!
//! `tests/inherited_app.rs` already proves `Theme::maybe_of` finds the
//! ancestor and returns `None` without one. `Theme::of` wraps the exact same
//! lookup with `.expect(...)` — distinct code, so far exercised by nothing —
//! and this file proves its success path returns the ancestor's data
//! unchanged.
//!
//! The no-ancestor (panicking) branch is deliberately **not** tested here:
//! a panic inside `build()` is caught by the framework's build-error
//! boundary (`build_owner.rs` substitutes an `ErrorView` for the panicking
//! node) rather than unwinding out to the test, so `#[should_panic]` around
//! the harness would not observe it.

// Target-level lint relaxations — crate-level allows don't reach this
// target. `unwrap` in test/example code: a panic IS the failure report
// (docs/PANIC-POLICY.md); style items here are ship-wave debt.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::{Arc, Mutex};

use common::{lay_out, loose};
use flui_types::platform::Brightness;
use flui_types::styling::Color;
use flui_view::prelude::*;
use flui_widgets::{SizedBox, Theme, ThemeData};

/// Captures whatever [`Theme::of`] returns during `build()`.
///
/// `Option` (not `Option<Option<_>>` like `ThemeCapture` in
/// `inherited_app.rs`): if `Theme::of` panics, `build()` never reaches the
/// assignment and the harness substitutes an `ErrorView`, so `captured`
/// simply stays `None` — the `.expect(...)` below turns that into a loud
/// failure rather than a silent false-pass.
#[derive(Clone, Debug, StatelessView)]
struct ThemeOfCapture {
    captured: Arc<Mutex<Option<ThemeData>>>,
}

impl StatelessView for ThemeOfCapture {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        *self.captured.lock().unwrap() = Some(Theme::of(ctx));
        SizedBox::shrink()
    }
}

/// `Theme::of` returns exactly the ancestor's data when a `Theme` is present
/// — the success path of the panicking accessor, distinct from (and until
/// now uncovered by) the `Theme::maybe_of` tests in `inherited_app.rs`.
#[test]
fn theme_of_panicking_accessor_returns_ancestor_theme_data() {
    let captured: Arc<Mutex<Option<ThemeData>>> = Arc::new(Mutex::new(None));
    // Sentinel primary_color distinct from both presets so the assertion
    // fails if `Theme::of` returned a preset instead of the provided value.
    let provided = ThemeData {
        brightness: Brightness::Dark,
        primary_color: Color::rgb(10, 20, 30),
        ..ThemeData::dark()
    };

    let _laid = lay_out(
        Theme::new(
            provided.clone(),
            ThemeOfCapture {
                captured: Arc::clone(&captured),
            },
        ),
        loose(100.0),
    );

    let got = captured.lock().unwrap().clone().expect(
        "ThemeOfCapture::build never populated `captured` — either it was not called, \
         or Theme::of panicked (the Theme ancestor is present, so it should not have)",
    );

    assert_eq!(
        got, provided,
        "Theme::of should return exactly the data provided by the ancestor Theme, \
         not a default or wrong scope"
    );
}
