//! Integration tests for [`MediaQuery`] — the inherited
//! application-infrastructure widget.
//!
//! Each test drives the real layout pipeline through the headless harness and
//! proves that the inherited read works end-to-end: a descendant view captures
//! the data provided by the ancestor into a shared cell during `build()`.
//!
//! The `Theme`/`ThemeData` analogue of these tests now lives in
//! `flui-material` (`crates/flui-material/tests/theme.rs` and
//! `crates/flui-material/tests/inherited_theme.rs`), since `Theme` moved
//! there — see that crate's docs.
//!
//! ## Correctness invariants tested
//!
//! * `MediaQuery::maybe_of` MUST return the *provided* data, not a default —
//!   the assertions fail if `maybe_of` returns any other value.
//! * `maybe_of` MUST return `None` when no ancestor exists.
//!
//! ## False-pass prevention
//!
//! Each capture cell is `Option<Option<T>>`:
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
use flui_geometry::px;
use flui_types::{Size, platform::Brightness};
use flui_view::prelude::*;
use flui_widgets::{MediaQuery, MediaQueryData, SizedBox};

// ============================================================================
// Capture helpers — stateless views that record inherited data during build()
// ============================================================================

/// Captures whatever [`MediaQuery::maybe_of`] returns during `build()`.
///
/// Outer `None` = `build` not called (framework bug or harness not wired).
/// Inner `None` = `build` ran; no `MediaQuery` ancestor found.
/// Inner `Some(data)` = `build` ran; ancestor data captured.
#[derive(Clone, Debug, StatelessView)]
struct MediaQueryCapture {
    captured: Arc<Mutex<Option<Option<MediaQueryData>>>>,
}

impl StatelessView for MediaQueryCapture {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        *self.captured.lock().unwrap() = Some(MediaQuery::maybe_of(ctx));
        SizedBox::shrink()
    }
}

// ============================================================================
// MediaQuery tests
// ============================================================================

/// A descendant reads `MediaQuery::maybe_of` inside `build` and the captured
/// value equals the data the ancestor `MediaQuery` provided.
#[test]
fn media_query_of_returns_ancestor_data() {
    let captured: Arc<Mutex<Option<Option<MediaQueryData>>>> = Arc::new(Mutex::new(None));
    // Size distinct from the default 800×600 so the assertion fails if
    // `maybe_of` returned the default instead of the provided value.
    let provided = MediaQueryData {
        size: Size::new(px(1280.0), px(800.0)),
        device_pixel_ratio: 2.0,
        platform_brightness: Brightness::Dark,
        ..MediaQueryData::default()
    };

    let _laid = lay_out(
        MediaQuery::new(
            provided.clone(),
            MediaQueryCapture {
                captured: Arc::clone(&captured),
            },
        ),
        loose(100.0),
    );

    let outer = captured.lock().unwrap().clone();
    let got = outer
        .expect(
            "MediaQueryCapture::build was never called — the harness did not traverse the subtree",
        )
        .expect("MediaQuery::maybe_of returned None even though a MediaQuery ancestor was present");

    assert_eq!(
        got, provided,
        "MediaQuery::maybe_of should return exactly the data provided by the ancestor MediaQuery, \
         not a default or wrong scope"
    );
}

/// `MediaQuery::maybe_of` returns `None` when no `MediaQuery` ancestor is
/// present. Proves the lookup is honest, not returning a hidden default.
#[test]
fn media_query_maybe_of_returns_none_without_ancestor() {
    let captured: Arc<Mutex<Option<Option<MediaQueryData>>>> = Arc::new(Mutex::new(None));

    let _laid = lay_out(
        MediaQueryCapture {
            captured: Arc::clone(&captured),
        },
        loose(100.0),
    );

    let outer = captured.lock().unwrap().clone();
    let inner = outer.expect(
        "MediaQueryCapture::build was never called — the harness did not traverse the subtree",
    );

    assert!(
        inner.is_none(),
        "MediaQuery::maybe_of should return None when no MediaQuery ancestor is present, \
         got: {inner:?}"
    );
}

// ============================================================================
// Value-type unit tests
// ============================================================================

/// `MediaQueryData::default()` sentinel values: both scale factors must be
/// `1.0` so tests using the default don't accidentally see accessibility zoom.
#[test]
fn media_query_data_default_has_unit_scale_factors() {
    let data = MediaQueryData::default();
    assert!(
        (data.text_scale_factor - 1.0).abs() < f32::EPSILON,
        "default text_scale_factor should be 1.0, got {}",
        data.text_scale_factor
    );
    assert!(
        (data.device_pixel_ratio - 1.0).abs() < f32::EPSILON,
        "default device_pixel_ratio should be 1.0, got {}",
        data.device_pixel_ratio
    );
    assert_eq!(data.platform_brightness, Brightness::Light);
}
